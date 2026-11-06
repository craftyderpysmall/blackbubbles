use std::sync::mpsc;

use base64::Engine;
use eframe::egui::{self, Align, Layout, RichText};

use crate::beacon;
use crate::config::{self, ServerConfig};
use crate::conn::{self, Status};
use crate::kit::{self, icon};
use crate::pages::Cx;
use crate::theme::{self, FG, FG_MUTED};

pub enum Action {
    /// Paired with a new server: rebuild the connection.
    Paired,
}

#[derive(Default)]
pub struct SettingsPage {
    pair_uri: String,
    device_name: String,
    pairing: Option<mpsc::Receiver<anyhow::Result<ServerConfig>>>,
    beacon_busy: Option<mpsc::Receiver<anyhow::Result<config::BeaconConfig>>>,
    gmail_address: String,
    gmail_password: String,
}

/// A settings section: one card with a title, a description and content.
fn section(ui: &mut egui::Ui, glyph: char, title: &str, description: &str, add: impl FnOnce(&mut egui::Ui)) {
    kit::card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(kit::icon_text(glyph, 16.0, FG));
            ui.label(kit::title(title));
        });
        if !description.is_empty() {
            ui.label(kit::muted(description));
        }
        ui.add_space(8.0);
        add(ui);
    });
    ui.add_space(theme::OUTER);
}

fn group_label(ui: &mut egui::Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(text.to_uppercase()).size(11.0).color(theme::FG_SUBTLE).font(theme::medium(11.0)));
}

impl SettingsPage {
    pub fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) -> Option<Action> {
        let mut action = None;
        if self.device_name.is_empty() {
            self.device_name = conn::device_name();
        }
        if let Some(rx) = &self.pairing
            && let Ok(result) = rx.try_recv()
        {
            self.pairing = None;
            match result {
                Ok(server) => {
                    cx.config.server = Some(server);
                    cx.save();
                    cx.toast("Paired with your Mac");
                    self.pair_uri.clear();
                    action = Some(Action::Paired);
                }
                Err(e) => cx.toast(format!("Pairing failed: {e:#}")),
            }
        }
        if let Some(rx) = &self.beacon_busy
            && let Ok(result) = rx.try_recv()
        {
            self.beacon_busy = None;
            match result {
                Ok(beacon) => {
                    cx.config.beacon = Some(beacon);
                    cx.save();
                    cx.toast("This PC is registered as a Find My beacon");
                }
                Err(e) => cx.toast(format!("Beacon setup failed: {e:#}")),
            }
        }

        egui::ScrollArea::vertical().id_salt("settings").auto_shrink(false).show(ui, |ui| {
            // A centered, readable column.
            let width = ui.available_width().min(760.0);
            let pad = ((ui.available_width() - width) / 2.0).max(0.0);
            ui.horizontal(|ui| {
                ui.add_space(pad);
                ui.vertical(|ui| {
                    ui.set_width(width);
                    ui.label(kit::heading("Settings"));
                    ui.add_space(theme::OUTER);
                    self.connection(ui, cx);
                    self.sending_address(ui);
                    self.notifications(ui, cx);
                    self.autofill(ui, cx);
                    if cfg!(target_os = "linux") {
                        self.beacon(ui, cx);
                    }
                    self.about(ui);
                });
            });
        });
        action
    }

    fn connection(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        section(ui, icon::LINK, "Mac connection", "", |ui| {
            let status = cx.client.status();
            let (text, dot) = match &status {
                Status::NotPaired => ("Not paired".to_string(), theme::FG_SUBTLE),
                Status::Connecting => ("Connecting…".to_string(), theme::WARNING),
                Status::Connected { server_name } => (format!("Connected to {server_name}"), theme::SUCCESS),
                Status::Disconnected(e) => (format!("Offline: {e}"), theme::DANGER),
            };
            ui.horizontal(|ui| {
                kit::badge(ui, &text, Some(dot));
                if let Some(server) = &cx.config.server {
                    ui.label(kit::subtle(format!("{}:{} · pinned {}…", server.host, server.port, &server.fingerprint[..12.min(server.fingerprint.len())])));
                }
            });
            ui.add_space(8.0);
            ui.label(kit::muted("On your Mac, run `bbd pair --host <its name or Tailscale IP>` and paste the result here."));
            ui.add_space(4.0);
            kit::input(ui, &mut self.pair_uri, "blackbubbles://…", ui.available_width() - 24.0, false);
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(kit::muted("Device name"));
                kit::input(ui, &mut self.device_name, "", 220.0, false);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let busy = self.pairing.is_some();
                    if kit::primary(ui, Some(icon::LINK), if busy { "Pairing…" } else { "Pair" }, !busy && !self.pair_uri.trim().is_empty()).clicked() {
                        let (tx, rx) = mpsc::channel();
                        let (uri, name, ctx) = (self.pair_uri.trim().to_string(), self.device_name.clone(), ui.ctx().clone());
                        cx.client.spawn(async move {
                            let _ = tx.send(conn::pair(&uri, &name).await);
                            ctx.request_repaint();
                        });
                        self.pairing = Some(rx);
                    }
                });
            });
        });
    }

    fn sending_address(&mut self, ui: &mut egui::Ui) {
        section(
            ui,
            icon::PHONE,
            "Sending address",
            "Messages go out from the address your Mac uses for each conversation. BlackBubbles never picks email on its own.",
            |ui| {
                ui.label(RichText::new("To send from your phone number:").color(FG));
                for step in [
                    "On your iPhone: Settings → Messages → Send & Receive → under “Start New Conversations From”, choose your phone number.",
                    "On the Mac: Messages → Settings → iMessage → make sure your phone number is ticked, and set “Start new conversations from” to it.",
                    "Conversations that already use your email keep using it. Each chat header shows the address (a yellow badge marks email); start a new conversation to switch it.",
                ] {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(kit::icon_text(icon::CHECK, 13.0, FG_MUTED));
                        ui.label(kit::muted(step));
                    });
                }
            },
        );
    }

    fn notifications(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let mut changed = false;
        section(ui, icon::BELL, "Notifications", "Choose exactly what shows up as a desktop notification.", |ui| {
            let n = &mut cx.config.notifications;
            changed |= kit::switch(ui, &mut n.enabled, "Desktop notifications", "Turn everything off with one switch").changed();
            ui.add_enabled_ui(n.enabled, |ui| {
                group_label(ui, "Messages");
                changed |= kit::switch(ui, &mut n.messages, "New messages", "").changed();
                ui.add_enabled_ui(n.messages, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(24.0);
                        ui.vertical(|ui| {
                        changed |= kit::switch(ui, &mut n.imessage, "iMessage", "").changed();
                        changed |= kit::switch(ui, &mut n.sms, "SMS / RCS", "").changed();
                        changed |= kit::switch(ui, &mut n.show_sender, "Show sender", "Off: notifications just say BlackBubbles").changed();
                        changed |= kit::switch(ui, &mut n.show_preview, "Show message preview", "Off: “New message” instead of the text").changed();
                        changed |= kit::switch(ui, &mut n.skip_codes, "Skip verification-code texts", "Autofill already shows those").changed();
                        changed |= kit::switch(ui, &mut n.only_when_unfocused, "Only when BlackBubbles isn't focused", "").changed();
                        if !n.muted_chats.is_empty() {
                            ui.horizontal(|ui| {
                                ui.label(kit::muted(format!("{} muted conversation(s)", n.muted_chats.len())));
                                if kit::ghost(ui, None, "Unmute all").clicked() {
                                    n.muted_chats.clear();
                                    changed = true;
                                }
                            });
                        } else {
                            kit::icon_label(ui, icon::BELL, kit::subtle("Mute a conversation with the bell in its header."), theme::FG_SUBTLE);
                        }
                        });
                    });
                });

                group_label(ui, "Calendar");
                changed |= kit::switch(ui, &mut n.calendar_reminders, "Event reminders", "").changed();
                ui.add_enabled_ui(n.calendar_reminders, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(44.0);
                        ui.label(kit::muted("Remind me"));
                        changed |= ui.add(egui::DragValue::new(&mut n.reminder_minutes).range(0..=240).suffix(" min")).changed();
                        ui.label(kit::muted("before events start"));
                    });
                });

                group_label(ui, "Find My");
                changed |= kit::switch(ui, &mut n.low_battery, "Low battery on a device", "").changed();
                ui.add_enabled_ui(n.low_battery, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(44.0);
                        ui.label(kit::muted("Below"));
                        changed |= ui.add(egui::DragValue::new(&mut n.low_battery_percent).range(5..=50).suffix("%")).changed();
                    });
                });
                changed |= kit::switch(ui, &mut n.beacon_located, "This PC was located", "When nearby Apple devices report the beacon").changed();

                group_label(ui, "Library");
                changed |= kit::switch(ui, &mut n.photos, "New photos", "").changed();
                changed |= kit::switch(ui, &mut n.notes, "Notes changed on the Mac", "").changed();
                changed |= kit::switch(ui, &mut n.voice_memos, "New voice memos", "").changed();

                group_label(ui, "System");
                changed |= kit::switch(ui, &mut n.connection, "Connection lost / restored", "After the Mac has been unreachable for 90 s").changed();
                changed |= kit::switch(ui, &mut n.quiet_hours, "Quiet hours", "Nothing is shown during this window").changed();
                ui.add_enabled_ui(n.quiet_hours, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(44.0);
                        ui.label(kit::muted("From"));
                        changed |= kit::input(ui, &mut n.quiet_start, "22:00", 48.0, false).changed();
                        ui.label(kit::muted("to"));
                        changed |= kit::input(ui, &mut n.quiet_end, "07:00", 48.0, false).changed();
                    });
                });
            });
            ui.add_space(6.0);
            ui.label(kit::subtle("Shown by the BlackBubbles process that owns autofill: the background daemon if it runs, otherwise this window."));
        });
        if changed {
            cx.save();
        }
    }

    fn autofill(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let mut changed = false;
        section(ui, icon::KEYBOARD, "Autofill", "Verification codes and links from texts and email go straight to your clipboard (built-in linuxautofill).", |ui| {
            let a = &mut cx.config.autofill;
            changed |= kit::switch(ui, &mut a.enabled, "Enabled", "").changed();
            changed |= kit::switch(ui, &mut a.from_server, "Codes from iMessage / SMS on the Mac", "").changed();
            changed |= kit::switch(ui, &mut a.notify, "Click-to-paste notification", "Off: codes are copied silently").changed();
            ui.horizontal(|ui| {
                ui.add_space(44.0);
                ui.label(kit::muted("Restore the clipboard after"));
                changed |= ui.add(egui::DragValue::new(&mut a.clear_after).range(5..=600).suffix(" s")).changed();
            });

            group_label(ui, "iPhone Shortcut endpoint");
            match &a.http {
                Some(http) => {
                    ui.label(kit::muted(format!("POST http://<this computer>.local:{}/message", http.port)));
                    ui.horizontal(|ui| {
                        ui.label(kit::muted("Header"));
                        ui.code(format!("Authorization: Bearer {}", http.token));
                    });
                    if kit::outline(ui, None, "Disable endpoint", true).clicked() {
                        a.http = None;
                        changed = true;
                    }
                }
                None => {
                    ui.label(kit::subtle("Works even when the Mac is offline."));
                    if kit::outline(ui, Some(icon::PLUS), "Enable with a new token", true).clicked() {
                        let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>());
                        a.http = Some(bb_autofill::config::HttpConfig { host: "0.0.0.0".into(), port: 8787, token });
                        changed = true;
                    }
                }
            }

            group_label(ui, "Gmail");
            let mut remove = None;
            for (i, account) in a.gmail.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(kit::icon_text(icon::MAIL, 14.0, FG_MUTED));
                    ui.label(RichText::new(&account.address).color(FG));
                    if kit::ghost(ui, Some(icon::TRASH), "Remove").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                a.gmail.remove(i);
                changed = true;
            }
            ui.horizontal_wrapped(|ui| {
                kit::input(ui, &mut self.gmail_address, "you@gmail.com", 200.0, false);
                kit::input(ui, &mut self.gmail_password, "App password", 160.0, true);
                if kit::outline(ui, Some(icon::PLUS), "Add", self.gmail_address.contains('@') && !self.gmail_password.is_empty()).clicked() {
                    a.gmail.push(bb_autofill::config::GmailConfig { address: std::mem::take(&mut self.gmail_address), app_password: std::mem::take(&mut self.gmail_password) });
                    changed = true;
                }
            });
            ui.add_space(6.0);
            ui.label(kit::subtle("Source and endpoint changes apply after restarting BlackBubbles."));
        });
        if changed {
            cx.save();
        }
    }

    fn beacon(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        section(
            ui,
            icon::RADAR,
            "Find My for this PC",
            "Broadcasts an Offline-Finding Bluetooth beacon so nearby Apple devices report where this PC is. It appears on the BlackBubbles map (Apple's own Find My app can't list non-Apple hardware).",
            |ui| match &cx.config.beacon {
                Some(b) => {
                    kit::badge(ui, &format!("Registered as “{}” · {} rotating keys", b.name, b.private_keys.len()), Some(theme::SUCCESS));
                    ui.add_space(6.0);
                    ui.label(kit::muted("Start the advertiser (needs Bluetooth on):"));
                    ui.code(format!("sudo systemctl enable --now blackbubbles-beacon@{}", std::env::var("USER").unwrap_or_else(|_| "$USER".into())));
                }
                None => {
                    let busy = self.beacon_busy.is_some();
                    if kit::primary(ui, Some(icon::RADAR), if busy { "Registering…" } else { "Make this PC findable" }, !busy && cx.connected).clicked() {
                        let (tx, rx) = mpsc::channel();
                        let (client, name, ctx) = (cx.client.clone(), self.device_name.clone(), ui.ctx().clone());
                        cx.client.spawn(async move {
                            let _ = tx.send(beacon::setup(&client, &name).await);
                            ctx.request_repaint();
                        });
                        self.beacon_busy = Some(rx);
                    }
                    if !cx.connected {
                        ui.label(kit::subtle("Connect to your Mac first."));
                    }
                }
            },
        );
    }

    fn about(&mut self, ui: &mut egui::Ui) {
        section(ui, icon::INFO, "About", "", |ui| {
            ui.label(kit::muted(format!("BlackBubbles {}", env!("CARGO_PKG_VERSION"))));
            if let Ok(path) = config::path() {
                ui.label(kit::subtle(format!("Settings file: {}", path.display())));
            }
        });
    }
}
