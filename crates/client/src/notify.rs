//! Desktop notifications, filtered by the user's settings.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use bb_proto::{DeviceKind, Event, Message, Request, Response, Service};

use crate::config::Notifications;
use crate::conn::{Client, Status};

pub type Shared = Arc<RwLock<Notifications>>;

pub struct Notifier {
    client: Client,
    settings: Shared,
    /// Whether the window has focus (always false in the daemon).
    pub focused: Arc<AtomicBool>,
    /// Autofill is running in this process and notifies about codes itself.
    autofill_notifies: bool,
    chat_names: Mutex<HashMap<String, String>>,
}

fn send(summary: &str, body: &str) {
    bb_autofill::platform::simple_notify(summary, body);
}

impl Notifier {
    pub fn start(client: &Client, settings: Shared, autofill_notifies: bool) -> Arc<Self> {
        let this = Arc::new(Self {
            client: client.clone(),
            settings,
            focused: Arc::new(AtomicBool::new(false)),
            autofill_notifies,
            chat_names: Mutex::default(),
        });
        let hook = this.clone();
        client.set_event_hook(Arc::new(move |event| hook.on_event(event)));
        let poller = this.clone();
        client.spawn(async move { poller.poll_loop().await });
        this
    }

    /// Current settings if notifications may be shown right now.
    fn active(&self) -> Option<Notifications> {
        let settings = self.settings.read().unwrap().clone();
        (settings.enabled && !settings.in_quiet_hours(chrono::Local::now().time())).then_some(settings)
    }

    fn on_event(self: &Arc<Self>, event: &Event) {
        let Some(s) = self.active() else { return };
        match event {
            Event::NewMessage(message) => self.on_message(&s, message),
            Event::PhotosChanged if s.photos => send("Photos", "New items in your library"),
            Event::NotesChanged if s.notes => send("Notes", "A note was changed on your Mac"),
            Event::VoiceMemosChanged if s.voice_memos => send("Voice Memos", "Your recordings changed"),
            _ => {}
        }
    }

    fn on_message(self: &Arc<Self>, s: &Notifications, message: &Message) {
        if message.is_from_me || !s.messages || s.muted_chats.contains(&message.chat_guid) {
            return;
        }
        if s.only_when_unfocused && self.focused.load(Ordering::Relaxed) {
            return;
        }
        let service = if message.chat_guid.starts_with("SMS") || message.chat_guid.starts_with("RCS") { Service::Sms } else { Service::IMessage };
        if (service == Service::Sms && !s.sms) || (service == Service::IMessage && !s.imessage) {
            return;
        }
        let text = message.text.clone().unwrap_or_else(|| if message.attachments.is_empty() { String::new() } else { "Attachment".into() });
        if s.skip_codes && self.autofill_notifies && bb_autofill::find_code(&text).is_some() {
            return;
        }
        let body = if s.show_preview { text } else { "New message".into() };
        let (show_sender, guid, sender) = (s.show_sender, message.chat_guid.clone(), message.sender.clone().unwrap_or_default());
        let known = self.chat_names.lock().unwrap().get(&guid).cloned();
        let this = self.clone();
        self.client.spawn(async move {
            let name = match known {
                Some(name) => name,
                None => this.lookup_chat(&guid).await.unwrap_or(sender),
            };
            let title = if show_sender { name } else { "BlackBubbles".into() };
            send(&title, &body);
        });
    }

    async fn lookup_chat(&self, guid: &str) -> Option<String> {
        let Ok(Response::Chats(chats)) = self.client.request(Request::ListChats { limit: 500, before: None }).await else { return None };
        let mut names = self.chat_names.lock().unwrap();
        for chat in chats {
            let name = chat.display_name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| chat.participants.join(", "));
            names.insert(chat.guid, name);
        }
        names.get(guid).cloned()
    }

    /// Calendar reminders, Find My battery/beacon checks and connection health.
    async fn poll_loop(self: Arc<Self>) {
        let mut reminded: HashSet<(String, i64)> = HashSet::new();
        let mut low: HashSet<String> = HashSet::new();
        let mut beacon_seen: HashMap<String, i64> = HashMap::new();
        let mut last_find_my = Instant::now() - Duration::from_secs(3600);
        let mut down_since: Option<Instant> = None;
        let mut told_down = false;
        let mut tick = tokio::time::interval(Duration::from_secs(30));
        loop {
            tick.tick().await;
            let settings = self.active();
            let connected = matches!(self.client.status(), Status::Connected { .. });

            // Connection health (reported even if other categories are off).
            match (connected, down_since) {
                (false, None) => down_since = Some(Instant::now()),
                (false, Some(since)) if !told_down && since.elapsed() > Duration::from_secs(90) => {
                    if settings.as_ref().is_some_and(|s| s.connection) && !matches!(self.client.status(), Status::NotPaired) {
                        send("Lost connection to your Mac", "Messages and autofill from the Mac are paused until it's back.");
                    }
                    told_down = true;
                }
                (true, Some(_)) => {
                    if told_down && settings.as_ref().is_some_and(|s| s.connection) {
                        send("Reconnected to your Mac", "Everything is syncing again.");
                    }
                    down_since = None;
                    told_down = false;
                }
                _ => {}
            }
            let Some(s) = settings else { continue };
            if !connected {
                continue;
            }

            if s.calendar_reminders {
                let now = chrono::Utc::now().timestamp_millis();
                let lead = i64::from(s.reminder_minutes) * 60_000;
                if let Ok(Response::Events(events)) = self.client.request(Request::ListEvents { from: now, to: now + lead + 60_000 }).await {
                    for event in events.into_iter().filter(|e| !e.all_day && e.start > now - 60_000 && e.start - lead <= now) {
                        if reminded.insert((event.id.clone(), event.start)) {
                            let minutes = ((event.start - now) / 60_000).max(0);
                            let when = if minutes == 0 { "Starting now".to_string() } else { format!("In {minutes} min") };
                            let body = match &event.location {
                                Some(location) => format!("{when} · {location}"),
                                None => when,
                            };
                            send(&event.title, &body);
                        }
                    }
                }
                reminded.retain(|(_, start)| *start > now - 86_400_000);
            }

            if (s.low_battery || s.beacon_located) && last_find_my.elapsed() > Duration::from_secs(600) {
                last_find_my = Instant::now();
                if let Ok(Response::FindMyDevices(devices)) = self.client.request(Request::ListFindMyDevices).await {
                    for device in devices {
                        if let Some(battery) = device.battery {
                            let percent = (battery * 100.0).round() as u8;
                            if percent < s.low_battery_percent {
                                if low.insert(device.id.clone()) && s.low_battery {
                                    send(&format!("{} battery low", device.name), &format!("{percent}% remaining"));
                                }
                            } else {
                                low.remove(&device.id);
                            }
                        }
                        if device.kind == DeviceKind::Beacon
                            && let Some(location) = device.location
                        {
                            let previous = beacon_seen.insert(device.id.clone(), location.at);
                            if s.beacon_located && previous.is_some_and(|p| location.at > p) {
                                send(&format!("{} was located", device.name), "A nearby Apple device reported its position.");
                            }
                        }
                    }
                }
            }
        }
    }
}
