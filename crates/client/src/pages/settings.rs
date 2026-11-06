use std::sync::mpsc;

use base64::Engine;
use eframe::egui::{self, RichText};

use crate::beacon;
use crate::config::{self, Config, ServerConfig};
use crate::conn::{self, Status};
use crate::pages::Cx;

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

impl SettingsPage {
    pub fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx, config: &mut Config) -> Option<Action> {
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
                    config.server = Some(server);
                    match config.save() {
                        Ok(()) => cx.toast("Paired"),
                        Err(e) => cx.toast(format!("Paired, but couldn't save config: {e:#}")),
                    }
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
                    config.beacon = Some(beacon);
                    let _ = config.save();
                    cx.toast("This PC is registered as a Find My beacon");
                }
                Err(e) => cx.toast(format!("Beacon setup failed: {e:#}")),
            }
        }

        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.set_max_width(720.0);
                ui.heading("Mac connection");
                let status = cx.client.status();
                let (text, color) = match &status {
                    Status::NotPaired => ("Not paired".to_string(), ui.visuals().warn_fg_color),
                    Status::Connecting => ("Connecting…".to_string(), ui.visuals().weak_text_color()),
                    Status::Connected { server_name } => (format!("Connected to {server_name}"), egui::Color32::from_rgb(48, 209, 88)),
                    Status::Disconnected(e) => (format!("Disconnected: {e}"), ui.visuals().error_fg_color),
                };
                ui.label(RichText::new(text).color(color));
                if let Some(server) = &config.server {
                    ui.label(RichText::new(format!("{}:{} · pinned {}…", server.host, server.port, &server.fingerprint[..16.min(server.fingerprint.len())])).weak().small());
                }
                ui.add_space(6.0);
                ui.label("On your Mac run `bbd pair --host <name or Tailscale IP>` and paste the result:");
                ui.add(egui::TextEdit::singleline(&mut self.pair_uri).hint_text("blackbubbles://…").desired_width(f32::INFINITY));
                ui.horizontal(|ui| {
                    ui.label("Device name");
                    ui.text_edit_singleline(&mut self.device_name);
                    let busy = self.pairing.is_some();
                    if ui.add_enabled(!busy && !self.pair_uri.trim().is_empty(), egui::Button::new(if busy { "Pairing…" } else { "Pair" })).clicked() {
                        let (tx, rx) = mpsc::channel();
                        let (uri, name, ctx) = (self.pair_uri.trim().to_string(), self.device_name.clone(), ui.ctx().clone());
                        cx.client.spawn(async move {
                            let _ = tx.send(conn::pair(&uri, &name).await);
                            ctx.request_repaint();
                        });
                        self.pairing = Some(rx);
                    }
                });

                ui.add_space(16.0);
                ui.heading("Autofill");
                ui.label(RichText::new("Verification codes from texts and email go straight to your clipboard (built-in linuxautofill).").weak());
                let mut changed = false;
                changed |= ui.checkbox(&mut config.autofill.enabled, "Enabled").changed();
                changed |= ui.checkbox(&mut config.autofill.from_server, "Codes from iMessage/SMS on the Mac").changed();
                ui.horizontal(|ui| {
                    ui.label("Restore clipboard after");
                    changed |= ui.add(egui::DragValue::new(&mut config.autofill.clear_after).range(5..=600).suffix(" s")).changed();
                });

                ui.add_space(6.0);
                ui.label(RichText::new("iPhone Shortcut endpoint (works even when the Mac is offline)").strong());
                match &config.autofill.http {
                    Some(http) => {
                        ui.label(format!("POST http://<this computer>.local:{}/message", http.port));
                        ui.horizontal(|ui| {
                            ui.label("Header:");
                            ui.code(format!("Authorization: Bearer {}", http.token));
                        });
                        if ui.button("Disable").clicked() {
                            config.autofill.http = None;
                            changed = true;
                        }
                    }
                    None => {
                        if ui.button("Enable with a new token").clicked() {
                            let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>());
                            config.autofill.http = Some(bb_autofill::config::HttpConfig { host: "0.0.0.0".into(), port: 8787, token });
                            changed = true;
                        }
                    }
                }

                ui.add_space(6.0);
                ui.label(RichText::new("Gmail (IMAP, app password)").strong());
                let mut remove = None;
                for (i, account) in config.autofill.gmail.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(&account.address);
                        if ui.small_button("Remove").clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    config.autofill.gmail.remove(i);
                    changed = true;
                }
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.gmail_address).hint_text("you@gmail.com").desired_width(200.0));
                    ui.add(egui::TextEdit::singleline(&mut self.gmail_password).hint_text("app password").password(true).desired_width(160.0));
                    if ui.add_enabled(self.gmail_address.contains('@') && !self.gmail_password.is_empty(), egui::Button::new("Add")).clicked() {
                        config.autofill.gmail.push(bb_autofill::config::GmailConfig {
                            address: std::mem::take(&mut self.gmail_address),
                            app_password: std::mem::take(&mut self.gmail_password),
                        });
                        changed = true;
                    }
                });
                if changed {
                    match config.save() {
                        Ok(()) => cx.toast("Saved. Autofill source changes apply after restarting BlackBubbles."),
                        Err(e) => cx.toast(format!("Couldn't save: {e:#}")),
                    }
                }
                if let Ok(path) = config::path() {
                    ui.label(RichText::new(format!("Config: {}", path.display())).weak().small());
                }

                if cfg!(target_os = "linux") {
                    ui.add_space(16.0);
                    ui.heading("Find My for this PC");
                    ui.label(RichText::new("Broadcasts an Offline-Finding Bluetooth beacon so nearby Apple devices report this PC's location. It shows on the BlackBubbles map (Apple's own Find My app can't list non-Apple hardware).").weak());
                    match &config.beacon {
                        Some(b) => {
                            ui.label(format!("Registered as “{}” with {} rotating keys.", b.name, b.private_keys.len()));
                            ui.label("Start the advertiser (needs Bluetooth and root):");
                            ui.code(format!("sudo systemctl enable --now blackbubbles-beacon@{}", std::env::var("USER").unwrap_or_else(|_| "$USER".into())));
                        }
                        None => {
                            let busy = self.beacon_busy.is_some();
                            let connected = matches!(status, Status::Connected { .. });
                            if ui.add_enabled(!busy && connected, egui::Button::new(if busy { "Registering…" } else { "Make this PC findable" })).clicked() {
                                let (tx, rx) = mpsc::channel();
                                let (client, name, ctx) = (cx.client.clone(), self.device_name.clone(), ui.ctx().clone());
                                cx.client.spawn(async move {
                                    let _ = tx.send(beacon::setup(&client, &name).await);
                                    ctx.request_repaint();
                                });
                                self.beacon_busy = Some(rx);
                            }
                            if !connected {
                                ui.label(RichText::new("Connect to the Mac first.").weak().small());
                            }
                        }
                    }
                }
            });
        });
        action
    }
}
