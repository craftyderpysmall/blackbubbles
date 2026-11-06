use bb_proto::{Album, ApiError, Asset, BlobKind, Event, Request, Response};
use eframe::egui::{self, RichText, Vec2};

use crate::conn::Pending;
use crate::media;
use crate::pages::Cx;
use crate::util;

const TILE: f32 = 150.0;
const GAP: f32 = 4.0;
const PAGE: u32 = 240;

type Req = Pending<Result<Response, ApiError>>;

pub struct PhotosPage {
    albums: Vec<Album>,
    albums_req: Option<Req>,
    album: String,
    assets: Vec<Asset>,
    assets_req: Option<Req>,
    exhausted: bool,
    viewing: Option<usize>,
    loaded: bool,
}

impl Default for PhotosPage {
    fn default() -> Self {
        Self {
            albums: vec![],
            albums_req: None,
            album: "all".into(),
            assets: vec![],
            assets_req: None,
            exhausted: false,
            viewing: None,
            loaded: false,
        }
    }
}

impl PhotosPage {
    fn refresh(&mut self, cx: &Cx) {
        self.loaded = true;
        self.albums_req = Some(cx.client.call(Request::ListAlbums));
        self.assets.clear();
        self.exhausted = false;
        self.load_more(cx);
    }

    fn load_more(&mut self, cx: &Cx) {
        if self.assets_req.is_some() || self.exhausted {
            return;
        }
        self.assets_req = Some(cx.client.call(Request::ListAssets {
            album_id: Some(self.album.clone()),
            offset: self.assets.len() as u32,
            limit: PAGE,
        }));
    }

    pub fn on_event(&mut self, event: &Event, cx: &Cx) {
        if matches!(event, Event::PhotosChanged) && self.loaded {
            self.refresh(cx);
        }
    }

    fn poll(&mut self, cx: &Cx) {
        if let Some(req) = &mut self.albums_req
            && let Some(result) = req.take()
        {
            self.albums_req = None;
            match result {
                Ok(Response::Albums(albums)) => self.albums = albums,
                Err(err) => cx.toast(format!("Photos: {}", err.message)),
                _ => {}
            }
        }
        if let Some(req) = &mut self.assets_req
            && let Some(result) = req.take()
        {
            self.assets_req = None;
            match result {
                Ok(Response::Assets(mut page)) => {
                    self.exhausted = page.len() < PAGE as usize;
                    self.assets.append(&mut page);
                }
                Err(err) => {
                    self.exhausted = true;
                    cx.toast(format!("Photos: {}", err.message));
                }
                _ => {}
            }
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        if !self.loaded && cx.connected {
            self.refresh(cx);
        }
        self.poll(cx);

        if let Some(index) = self.viewing {
            self.lightbox(ui, cx, index);
            return;
        }

        egui::Panel::top("photos_bar").show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading("Photos");
                let current = self.albums.iter().find(|a| a.id == self.album).map(|a| format!("{} ({})", a.title, a.count));
                let mut chosen = None;
                egui::ComboBox::from_id_salt("album")
                    .selected_text(current.unwrap_or_else(|| "Library".into()))
                    .width(240.0)
                    .show_ui(ui, |ui| {
                        for album in &self.albums {
                            if ui.selectable_label(album.id == self.album, format!("{} ({})", album.title, album.count)).clicked() {
                                chosen = Some(album.id.clone());
                            }
                        }
                    });
                if let Some(id) = chosen {
                    self.album = id;
                    self.assets.clear();
                    self.exhausted = false;
                    self.assets_req = None;
                    self.load_more(cx);
                }
                if ui.button("Refresh").clicked() {
                    self.refresh(cx);
                }
            });
            ui.add_space(4.0);
        });

        egui::CentralPanel::default().show(ui, |ui| {
            if self.assets.is_empty() {
                ui.centered_and_justified(|ui| {
                    if self.assets_req.is_some() {
                        ui.spinner();
                    } else {
                        ui.label(RichText::new("No photos").weak());
                    }
                });
                return;
            }
            let columns = ((ui.available_width() + GAP) / (TILE + GAP)).floor().max(1.0) as usize;
            let rows = self.assets.len().div_ceil(columns);
            let mut open = None;
            let mut near_end = false;
            egui::ScrollArea::vertical().auto_shrink(false).show_rows(ui, TILE + GAP, rows, |ui, range| {
                near_end = range.end + 4 >= rows;
                for row in range {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = GAP;
                        for col in 0..columns {
                            let i = row * columns + col;
                            let Some(asset) = self.assets.get(i) else { break };
                            let response = media::show(ui, cx.images, cx.client, BlobKind::PhotoThumbnail, &asset.id, Vec2::splat(TILE));
                            if asset.is_video {
                                ui.painter().text(response.rect.left_bottom() + egui::vec2(6.0, -6.0), egui::Align2::LEFT_BOTTOM, "▶", egui::FontId::proportional(14.0), egui::Color32::WHITE);
                            }
                            if asset.favorite {
                                ui.painter().text(response.rect.right_bottom() + egui::vec2(-6.0, -6.0), egui::Align2::RIGHT_BOTTOM, "♥", egui::FontId::proportional(14.0), egui::Color32::WHITE);
                            }
                            if response.clicked() {
                                open = Some(i);
                            }
                        }
                    });
                }
            });
            if near_end {
                self.load_more(cx);
            }
            if open.is_some() {
                self.viewing = open;
            }
        });
    }

    fn lightbox(&mut self, ui: &mut egui::Ui, cx: &mut Cx, index: usize) {
        let Some(asset) = self.assets.get(index).cloned() else {
            self.viewing = None;
            return;
        };
        let (left, right, escape) = ui.input(|i| (i.key_pressed(egui::Key::ArrowLeft), i.key_pressed(egui::Key::ArrowRight), i.key_pressed(egui::Key::Escape)));
        egui::Panel::top("lightbox_bar").show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("← Back").clicked() || escape {
                    self.viewing = None;
                }
                ui.label(util::long_time(asset.created));
                ui.label(RichText::new(format!("{} × {}", asset.width, asset.height)).weak());
                if asset.is_video {
                    ui.label(RichText::new("Video (still frame)").weak());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Save").clicked() {
                        cx.download(BlobKind::Photo, asset.id.clone(), format!("photo-{}.jpg", asset.created));
                    }
                });
            });
            ui.add_space(4.0);
        });
        egui::CentralPanel::default().show(ui, |ui| {
            let size = ui.available_size();
            ui.centered_and_justified(|ui| {
                media::show(ui, cx.images, cx.client, BlobKind::Photo, &asset.id, size);
            });
        });
        if left && index > 0 {
            self.viewing = Some(index - 1);
        }
        if right && index + 1 < self.assets.len() {
            self.viewing = Some(index + 1);
        }
    }
}
