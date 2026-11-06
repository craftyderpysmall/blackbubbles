use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, CornerRadius, Margin, RichText, Sense};

use crate::config::Config;
use crate::conn::{Client, Status};
use crate::kit::{self, icon};
use crate::media::Images;
use crate::notify::{self, Notifier};
use crate::pages::{self, Cx};
use crate::theme::{self, BG, BORDER, FG, FG_MUTED, FG_SUBTLE, PRIMARY, PRIMARY_FG, RAISED, SURFACE};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Messages,
    Photos,
    Notes,
    Calendar,
    VoiceMemos,
    FindMy,
    Settings,
}

pub const TABS: [(Tab, char, &str); 7] = [
    (Tab::Messages, icon::MESSAGES, "Messages"),
    (Tab::Photos, icon::PHOTOS, "Photos"),
    (Tab::Notes, icon::NOTES, "Notes"),
    (Tab::Calendar, icon::CALENDAR, "Calendar"),
    (Tab::VoiceMemos, icon::MEMOS, "Voice Memos"),
    (Tab::FindMy, icon::FIND_MY, "Find My"),
    (Tab::Settings, icon::SETTINGS, "Settings"),
];

/// Below this window width the rail shows icons only.
const RAIL_COMPACT_BELOW: f32 = 1040.0;
const TOAST_SECS: f32 = 4.5;

pub struct App {
    rt: tokio::runtime::Handle,
    client: Client,
    config: Config,
    autofill: Option<Sender<bb_autofill::Incoming>>,
    notifications: notify::Shared,
    notifier: Option<Arc<Notifier>>,
    /// This window owns autofill and notifications (no daemon is running).
    primary_process: bool,
    tab: Tab,
    tab_changed: Instant,
    images: Images,
    notices: (Sender<String>, Receiver<String>),
    toasts: Vec<(String, Instant)>,
    messages: pages::messages::MessagesPage,
    photos: pages::photos::PhotosPage,
    notes: pages::notes::NotesPage,
    calendar: pages::calendar::CalendarPage,
    voice: pages::voice::VoicePage,
    findmy: pages::findmy::FindMyPage,
    settings: pages::settings::SettingsPage,
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        rt: tokio::runtime::Handle,
        config: Config,
        autofill: Option<Sender<bb_autofill::Incoming>>,
        primary_process: bool,
        startup_error: Option<String>,
    ) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        theme::apply(&cc.egui_ctx);
        let notifications = Arc::new(RwLock::new(config.notifications.clone()));
        let notices = channel();
        if let Some(error) = startup_error {
            let _ = notices.0.send(error);
        }
        let mut tab = if config.server.is_some() { Tab::Messages } else { Tab::Settings };
        // Developer aid for screenshots: BLACKBUBBLES_TAB=photos etc.
        if let Ok(name) = std::env::var("BLACKBUBBLES_TAB") {
            let want = name.to_lowercase().replace(' ', "");
            if let Some((t, _, _)) = TABS.iter().find(|(_, _, n)| n.to_lowercase().replace(' ', "") == want) {
                tab = *t;
            }
        }
        let mut app = Self {
            client: Client::new(rt.clone(), None),
            rt,
            config,
            autofill,
            notifications,
            notifier: None,
            primary_process,
            tab,
            tab_changed: Instant::now(),
            images: Images::default(),
            notices,
            toasts: vec![],
            messages: Default::default(),
            photos: Default::default(),
            notes: Default::default(),
            calendar: Default::default(),
            voice: Default::default(),
            findmy: Default::default(),
            settings: Default::default(),
        };
        app.connect(&cc.egui_ctx);
        app
    }

    fn connect(&mut self, ctx: &egui::Context) {
        self.client = Client::new(self.rt.clone(), self.config.server.clone());
        self.client.set_repaint(ctx.clone());
        if let Some(tx) = &self.autofill
            && self.config.autofill.from_server
        {
            self.client.set_autofill(tx.clone());
        }
        self.notifier = self.primary_process.then(|| {
            let autofill_notifies = self.autofill.is_some() && self.config.autofill.notify;
            Notifier::start(&self.client, self.notifications.clone(), autofill_notifies)
        });
    }

    fn reconnect(&mut self, ctx: &egui::Context) {
        self.connect(ctx);
        self.messages = Default::default();
        self.photos = Default::default();
        self.notes = Default::default();
        self.calendar = Default::default();
        self.voice = Default::default();
        self.findmy = Default::default();
        self.switch(Tab::Messages);
    }

    fn switch(&mut self, tab: Tab) {
        if self.tab != tab {
            self.tab = tab;
            self.tab_changed = Instant::now();
        }
    }

    fn rail(&mut self, ui: &mut egui::Ui, compact: bool) {
        let width = if compact { 64.0 } else { 216.0 };
        egui::Panel::left("rail")
            .exact_size(width)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(BG).inner_margin(Margin { left: 12, right: 0, top: 16, bottom: 16 }))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(40.0, 32.0), Sense::hover());
                    ui.painter().circle_filled(rect.center(), 14.0, PRIMARY);
                    ui.painter().text(rect.center(), Align2::CENTER_CENTER, icon::MESSAGES, theme::icons(15.0), PRIMARY_FG);
                    if !compact {
                        ui.label(RichText::new("BlackBubbles").font(theme::semibold(15.0)).color(FG));
                    }
                });
                ui.add_space(16.0);

                let mut clicked = None;
                for (tab, glyph, name) in TABS {
                    if tab != Tab::Settings && nav_item(ui, glyph, name, self.tab == tab, compact).clicked() {
                        clicked = Some(tab);
                    }
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    self.status(ui, compact);
                    ui.add_space(4.0);
                    if nav_item(ui, icon::SETTINGS, "Settings", self.tab == Tab::Settings, compact).clicked() {
                        clicked = Some(Tab::Settings);
                    }
                });
                if let Some(tab) = clicked {
                    self.switch(tab);
                }
            });
    }

    fn status(&self, ui: &mut egui::Ui, compact: bool) {
        let status = self.client.status();
        let (dot, label) = match &status {
            Status::Connected { server_name } => (theme::SUCCESS, server_name.clone()),
            Status::Connecting => (theme::WARNING, "Connecting…".to_string()),
            Status::NotPaired => (FG_SUBTLE, "Not paired".to_string()),
            Status::Disconnected(_) => (theme::DANGER, "Offline".to_string()),
        };
        let tooltip = match status {
            Status::Disconnected(e) => e,
            _ => label.clone(),
        };
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(40.0, 28.0), Sense::hover());
            ui.painter().circle_filled(rect.center(), 4.0, dot);
            if !compact {
                ui.label(RichText::new(label).color(FG_MUTED).size(12.0));
            }
        })
        .response
        .on_hover_text(tooltip);
    }

    fn toasts(&mut self, ctx: &egui::Context) {
        self.toasts.retain(|(_, at)| at.elapsed().as_secs_f32() < TOAST_SECS);
        if self.toasts.is_empty() {
            return;
        }
        egui::Area::new(egui::Id::unique("toasts"))
            .anchor(Align2::RIGHT_BOTTOM, egui::vec2(-theme::OUTER, -theme::OUTER))
            .order(egui::Order::Foreground)
            .interactable(false)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = theme::GAP;
                for (message, at) in &self.toasts {
                    let age = at.elapsed().as_secs_f32();
                    let enter = kit::decel(age / theme::ENTER_SECS);
                    let exit = ((TOAST_SECS - age) / theme::EXIT_SECS).clamp(0.0, 1.0);
                    ui.scope(|ui| {
                        ui.set_opacity(enter.min(exit));
                        egui::Frame::new()
                            .fill(SURFACE)
                            .stroke(egui::Stroke::new(1.0, BORDER))
                            .corner_radius(CornerRadius::same(theme::R_CARD))
                            .shadow(ui.visuals().popup_shadow)
                            .inner_margin(Margin::symmetric(16, 12))
                            .show(ui, |ui| {
                                ui.set_max_width(360.0);
                                ui.label(RichText::new(message).color(FG));
                            });
                    });
                }
            });
        ctx.request_repaint_after(Duration::from_millis(33));
    }
}

/// Rail entry. The selected one is the surface's single inverted pill.
fn nav_item(ui: &mut egui::Ui, glyph: char, name: &str, selected: bool, compact: bool) -> egui::Response {
    let width = if compact { 40.0 } else { ui.available_width() - 12.0 };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 36.0), Sense::click());
    let t = kit::animate(ui, response.id.with("sel"), selected);
    let hover = kit::animate(ui, response.id.with("hover"), response.hovered() && !selected);
    let fill = egui::Color32::TRANSPARENT.lerp_to_gamma(RAISED, hover).lerp_to_gamma(PRIMARY, t);
    ui.painter().rect_filled(rect, CornerRadius::same(theme::R_PILL), fill);
    let fg = FG_MUTED.lerp_to_gamma(FG, hover).lerp_to_gamma(PRIMARY_FG, t);
    let icon_x = if compact { rect.center().x } else { rect.left() + 20.0 };
    ui.painter().text(egui::pos2(icon_x, rect.center().y), Align2::CENTER_CENTER, glyph, theme::icons(16.0), fg);
    if !compact {
        ui.painter().text(egui::pos2(rect.left() + 40.0, rect.center().y), Align2::LEFT_CENTER, name, theme::medium(13.0), fg);
    }
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if compact { response.on_hover_text(name) } else { response }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // Developer aid for checking responsive layouts on a tiling WM:
        // BLACKBUBBLES_FORCE_WIDTH=700 lays the UI out as if the window were 700 px wide.
        if let Some(width) = std::env::var("BLACKBUBBLES_FORCE_WIDTH").ok().and_then(|w| w.parse::<f32>().ok()) {
            let full = ui.max_rect();
            let rect = egui::Rect::from_min_size(full.min, egui::vec2(width.min(full.width()), full.height()));
            ui.painter().rect_filled(full, 0.0, egui::Color32::from_gray(40));
            ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                ui.set_clip_rect(rect);
                self.frame_ui(ui, frame)
            });
            return;
        }
        self.frame_ui(ui, frame);
    }
}

impl App {
    fn frame_ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let _guard = self.rt.enter();
        if let Some(notifier) = &self.notifier {
            notifier.focused.store(ctx.input(|i| i.focused), Ordering::Relaxed);
        }
        let connected = matches!(self.client.status(), Status::Connected { .. });
        let mut dirty = false;

        let events = self.client.take_events();
        {
            let mut cx = Cx { client: &self.client, images: &mut self.images, connected, notices: self.notices.0.clone(), config: &mut self.config, dirty: &mut dirty };
            for event in &events {
                self.messages.on_event(event, &mut cx);
                self.photos.on_event(event, &mut cx);
                self.notes.on_event(event, &mut cx);
                self.calendar.on_event(event, &mut cx);
                self.voice.on_event(event, &mut cx);
            }
        }
        while let Ok(message) = self.notices.1.try_recv() {
            self.toasts.push((message, Instant::now()));
        }

        let width = ui.available_width();
        self.rail(ui, width < RAIL_COMPACT_BELOW);

        let mut action = None;
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(Margin::same(theme::OUTER as i8)))
            .show(ui, |ui| {
                let t = (self.tab_changed.elapsed().as_secs_f32() / theme::ENTER_SECS).min(1.0);
                if t < 1.0 {
                    ui.set_opacity(kit::decel(t));
                    ctx.request_repaint();
                }
                let mut cx = Cx { client: &self.client, images: &mut self.images, connected, notices: self.notices.0.clone(), config: &mut self.config, dirty: &mut dirty };
                match self.tab {
                    Tab::Messages => self.messages.ui(ui, &mut cx),
                    Tab::Photos => self.photos.ui(ui, &mut cx),
                    Tab::Notes => self.notes.ui(ui, &mut cx),
                    Tab::Calendar => self.calendar.ui(ui, &mut cx),
                    Tab::VoiceMemos => self.voice.ui(ui, &mut cx),
                    Tab::FindMy => self.findmy.ui(ui, &mut cx),
                    Tab::Settings => action = self.settings.ui(ui, &mut cx),
                }
            });
        self.toasts(&ctx);

        if dirty {
            *self.notifications.write().unwrap() = self.config.notifications.clone();
            if let Err(e) = self.config.save() {
                let _ = self.notices.0.send(format!("Couldn't save settings: {e:#}"));
            }
        }
        if let Some(pages::settings::Action::Paired) = action {
            self.reconnect(&ctx);
        }
    }
}
