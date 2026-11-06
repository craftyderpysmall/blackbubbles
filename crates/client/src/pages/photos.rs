use bb_proto::{Album, ApiError, Asset, BlobKind, Event, Request, Response};
use eframe::egui::{self, Align, Align2, Color32, Layout, Vec2};

use crate::conn::Pending;
use crate::kit::{self, icon};
use crate::media;
use crate::pages::Cx;
use crate::theme::{self, FG, FG_SUBTLE};
use crate::util;

/// Tiles stretch between these sizes so the grid always fills the width exactly.
const MIN_TILE: f32 = 132.0;
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
        Self { albums: vec![], albums_req: None, album: "all".into(), assets: vec![], assets_req: None, exhausted: false, viewing: None, loaded: false }
    }
}

impl PhotosPage {
    fn refresh(&mut self, cx: &Cx) {
        self.loaded = true;
        self.albums_req = Some(cx.client.call(Request::ListAlbums));
        self.assets.clear();
        self.exhausted = false;
        self.assets_req = None;
        self.load_more(cx);
    }

    fn load_more(&mut self, cx: &Cx) {
        if self.assets_req.is_some() || self.exhausted {
            return;
        }
        self.assets_req = Some(cx.client.call(Request::ListAssets { album_id: Some(self.album.clone()), offset: self.assets.len() as u32, limit: PAGE }));
    }

    pub fn on_event(&mut self, event: &Event, cx: &mut Cx) {
        if matches!(event, Event::PhotosChanged) && self.loaded {
            self.refresh(cx);
        }
    }

    fn poll(&mut self, cx: &Cx) {
        if let Some(result) = self.albums_req.as_mut().and_then(Pending::take) {
            self.albums_req = None;
            match result {
                Ok(Response::Albums(albums)) => self.albums = albums,
                Err(err) => cx.toast(format!("Photos: {}", err.message)),
                _ => {}
            }
        }
        if let Some(result) = self.assets_req.as_mut().and_then(Pending::take) {
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
            kit::card(ui, |ui| self.lightbox(ui, cx, index));
            return;
        }
        kit::card(ui, |ui| {
            self.toolbar(ui, cx);
            ui.add_space(12.0);
            self.grid(ui, cx);
        });
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, cx: &Cx) {
        ui.horizontal(|ui| {
            ui.label(kit::title("Photos"));
            ui.add_space(4.0);
            // Albums as pills; the selected album is the primary.
            let mut chosen = None;
            egui::ScrollArea::horizontal().id_salt("albums").max_width(ui.available_width() - 48.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for album in &self.albums {
                        let label = format!("{}  {}", album.title, album.count);
                        let clicked = if album.id == self.album { kit::primary(ui, None, &label, true).clicked() } else { kit::ghost(ui, None, &label).clicked() };
                        if clicked {
                            chosen = Some(album.id.clone());
                        }
                    }
                });
            });
            if let Some(id) = chosen {
                self.album = id;
                self.assets.clear();
                self.exhausted = false;
                self.assets_req = None;
                self.load_more(cx);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if kit::icon_button(ui, icon::REFRESH, "Refresh").clicked() {
                    self.refresh(cx);
                }
            });
        });
    }

    fn grid(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        if self.assets.is_empty() {
            if self.assets_req.is_some() {
                kit::spinner_center(ui);
            } else {
                kit::empty_state(ui, icon::PHOTOS, "No photos", if cx.connected { "This album is empty" } else { "Connect to your Mac in Settings" });
            }
            return;
        }
        let width = ui.available_width();
        let columns = ((width + GAP) / (MIN_TILE + GAP)).floor().max(2.0) as usize;
        let tile = (width - GAP * (columns as f32 - 1.0)) / columns as f32;
        let rows = self.assets.len().div_ceil(columns);
        let mut open = None;
        let mut near_end = false;
        egui::ScrollArea::vertical().id_salt("photo_grid").auto_shrink(false).show_rows(ui, tile + GAP, rows, |ui, range| {
            ui.spacing_mut().item_spacing = Vec2::splat(GAP);
            near_end = range.end + 4 >= rows;
            for row in range {
                ui.horizontal(|ui| {
                    for col in 0..columns {
                        let i = row * columns + col;
                        let Some(asset) = self.assets.get(i) else { break };
                        let response = media::show(ui, cx.images, cx.client, BlobKind::PhotoThumbnail, &asset.id, Vec2::splat(tile));
                        let rect = response.rect;
                        let hover = kit::animate(ui, response.id.with("hover"), response.hovered());
                        if hover > 0.0 {
                            ui.painter().rect_filled(rect, theme::R_CONTROL, Color32::from_white_alpha((hover * 18.0) as u8));
                        }
                        let badge = |glyph: char, align: Align2, pos: egui::Pos2| {
                            ui.painter().text(pos + egui::vec2(1.0, 1.0), align, glyph, theme::icons(14.0), Color32::from_black_alpha(160));
                            ui.painter().text(pos, align, glyph, theme::icons(14.0), FG);
                        };
                        if asset.is_video {
                            badge(icon::VIDEO, Align2::LEFT_BOTTOM, rect.left_bottom() + egui::vec2(8.0, -8.0));
                        }
                        if asset.favorite {
                            badge(icon::HEART, Align2::RIGHT_BOTTOM, rect.right_bottom() + egui::vec2(-8.0, -8.0));
                        }
                        if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
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
    }

    fn lightbox(&mut self, ui: &mut egui::Ui, cx: &mut Cx, index: usize) {
        let Some(asset) = self.assets.get(index).cloned() else {
            self.viewing = None;
            return;
        };
        let (left, right, escape) = ui.input(|i| (i.key_pressed(egui::Key::ArrowLeft), i.key_pressed(egui::Key::ArrowRight), i.key_pressed(egui::Key::Escape)));
        ui.horizontal(|ui| {
            if kit::icon_button(ui, icon::BACK, "Back (Esc)").clicked() || escape {
                self.viewing = None;
            }
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(kit::title(util::long_time(asset.created)));
                let mut meta = format!("{} × {}", asset.width, asset.height);
                if asset.is_video {
                    meta.push_str(" · video, still frame");
                }
                ui.label(kit::subtle(meta));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if kit::primary(ui, Some(icon::DOWNLOAD), "Save", true).clicked() {
                    cx.download(BlobKind::Photo, asset.id.clone(), format!("photo-{}.jpg", asset.created));
                }
                ui.label(egui::RichText::new(format!("{} / {}", index + 1, self.assets.len())).color(FG_SUBTLE));
                if kit::icon_button(ui, icon::RIGHT, "Next (→)").clicked() && index + 1 < self.assets.len() {
                    self.viewing = Some(index + 1);
                }
                if kit::icon_button(ui, icon::LEFT, "Previous (←)").clicked() && index > 0 {
                    self.viewing = Some(index - 1);
                }
            });
        });
        ui.add_space(12.0);
        let size = ui.available_size();
        ui.centered_and_justified(|ui| {
            media::show(ui, cx.images, cx.client, BlobKind::Photo, &asset.id, size);
        });
        if left && index > 0 {
            self.viewing = Some(index - 1);
        }
        if right && index + 1 < self.assets.len() {
            self.viewing = Some(index + 1);
        }
    }
}
