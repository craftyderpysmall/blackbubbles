use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use eframe::egui::{self, RichText};

use crate::config::Config;
use crate::conn::{Client, Status};
use crate::media::Images;
use crate::pages::{self, Cx};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Messages,
    Photos,
    Notes,
    Calendar,
    VoiceMemos,
    FindMy,
    Settings,
}

const TABS: [(Tab, &str); 7] = [
    (Tab::Messages, "Messages"),
    (Tab::Photos, "Photos"),
    (Tab::Notes, "Notes"),
    (Tab::Calendar, "Calendar"),
    (Tab::VoiceMemos, "Memos"),
    (Tab::FindMy, "Find My"),
    (Tab::Settings, "Settings"),
];

pub struct App {
    rt: tokio::runtime::Handle,
    client: Client,
    config: Config,
    autofill: Option<Sender<bb_autofill::Incoming>>,
    tab: Tab,
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
    pub fn new(cc: &eframe::CreationContext<'_>, rt: tokio::runtime::Handle, config: Config, autofill: Option<Sender<bb_autofill::Incoming>>, startup_error: Option<String>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        crate::fonts::install(&cc.egui_ctx);
        let client = Client::new(rt.clone(), config.server.clone());
        client.set_repaint(cc.egui_ctx.clone());
        if let Some(tx) = &autofill
            && config.autofill.from_server
        {
            client.set_autofill(tx.clone());
        }
        let notices = channel();
        if let Some(error) = startup_error {
            let _ = notices.0.send(error);
        }
        let mut tab = if config.server.is_some() { Tab::Messages } else { Tab::Settings };
        // Developer aid for screenshots: BLACKBUBBLES_TAB=photos etc.
        if let Ok(name) = std::env::var("BLACKBUBBLES_TAB") {
            let name = name.to_lowercase();
            if let Some((t, _)) = TABS.iter().find(|(_, n)| n.to_lowercase().replace(' ', "") == name.replace(' ', "")) {
                tab = *t;
            }
        }
        Self {
            rt,
            client,
            config,
            autofill,
            tab,
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
        }
    }

    fn reconnect(&mut self, ctx: &egui::Context) {
        self.client = Client::new(self.rt.clone(), self.config.server.clone());
        self.client.set_repaint(ctx.clone());
        if let Some(tx) = &self.autofill
            && self.config.autofill.from_server
        {
            self.client.set_autofill(tx.clone());
        }
        // Fresh pages so they reload from the new server.
        self.messages = Default::default();
        self.photos = Default::default();
        self.notes = Default::default();
        self.calendar = Default::default();
        self.voice = Default::default();
        self.findmy = Default::default();
        self.tab = Tab::Messages;
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let connected = matches!(self.client.status(), Status::Connected { .. });

        let events = self.client.take_events();
        {
            let cx = Cx { client: &self.client, images: &mut self.images, connected, notices: self.notices.0.clone() };
            for event in &events {
                self.messages.on_event(event, &cx);
                self.photos.on_event(event, &cx);
                self.notes.on_event(event, &cx);
                self.calendar.on_event(event, &cx);
                self.voice.on_event(event, &cx);
            }
        }
        while let Ok(message) = self.notices.1.try_recv() {
            self.toasts.push((message, Instant::now()));
        }
        self.toasts.retain(|(_, at)| at.elapsed() < Duration::from_secs(5));

        egui::Panel::left("tabs").exact_size(96.0).resizable(false).show(ui, |ui| {
            ui.add_space(10.0);
            ui.vertical_centered(|ui| {
                for (tab, name) in TABS {
                    let selected = self.tab == tab;
                    let button = egui::Button::selectable(selected, RichText::new(name).size(13.5)).min_size(egui::vec2(84.0, 36.0));
                    if ui.add(button).clicked() {
                        self.tab = tab;
                    }
                    ui.add_space(4.0);
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    ui.add_space(8.0);
                    let (dot, tip) = match self.client.status() {
                        Status::Connected { server_name } => (egui::Color32::from_rgb(48, 209, 88), format!("Connected to {server_name}")),
                        Status::Connecting => (egui::Color32::from_rgb(255, 214, 10), "Connecting…".into()),
                        Status::NotPaired => (egui::Color32::GRAY, "Not paired".into()),
                        Status::Disconnected(e) => (egui::Color32::from_rgb(255, 69, 58), e),
                    };
                    let (rect, response) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect.center(), 5.0, dot);
                    response.on_hover_text(tip);
                });
            });
        });

        if !self.toasts.is_empty() {
            egui::Panel::bottom("toasts").show(ui, |ui| {
                for (message, _) in &self.toasts {
                    ui.label(message);
                }
            });
            ctx.request_repaint_after(Duration::from_millis(500));
        }

        let mut cx = Cx { client: &self.client, images: &mut self.images, connected, notices: self.notices.0.clone() };
        let mut action = None;
        match self.tab {
            Tab::Messages => self.messages.ui(ui, &mut cx),
            Tab::Photos => self.photos.ui(ui, &mut cx),
            Tab::Notes => self.notes.ui(ui, &mut cx),
            Tab::Calendar => self.calendar.ui(ui, &mut cx),
            Tab::VoiceMemos => self.voice.ui(ui, &mut cx),
            Tab::FindMy => self.findmy.ui(ui, &mut cx),
            Tab::Settings => action = self.settings.ui(ui, &mut cx, &mut self.config),
        }
        if let Some(pages::settings::Action::Paired) = action {
            self.reconnect(&ctx);
        }
    }
}
