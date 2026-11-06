use std::time::{Duration, Instant};

use bb_proto::{ApiError, DeviceKind, FindMyDevice, Request, Response};
use eframe::egui::{self, Align, Align2, Color32, CornerRadius, Layout, Margin, Stroke};
use walkers::{HttpTiles, Map, MapMemory, Plugin, Position, Projector, lon_lat, sources::OpenStreetMap};

use crate::conn::Pending;
use crate::kit::{self, icon};
use crate::pages::{Cx, NARROW};
use crate::theme::{self, BORDER, BORDER_STRONG, FG, FG_MUTED, PRIMARY, PRIMARY_FG, SURFACE};
use crate::util;

const REFRESH: Duration = Duration::from_secs(60);

type Req = Pending<Result<Response, ApiError>>;

#[derive(Default)]
pub struct FindMyPage {
    devices: Vec<FindMyDevice>,
    req: Option<Req>,
    last: Option<Instant>,
    selected: Option<String>,
    tiles: Option<HttpTiles>,
    memory: MapMemory,
    error: Option<String>,
}

struct Marker {
    position: Position,
    label: String,
    glyph: char,
    selected: bool,
}

struct Markers(Vec<Marker>);

impl Plugin for Markers {
    fn run(self: Box<Self>, ui: &mut egui::Ui, _response: &egui::Response, projector: &Projector, _memory: &MapMemory) {
        let painter = ui.painter();
        for m in self.0 {
            let p = projector.project(m.position).to_pos2();
            // Soft shadow, then a monochrome pin: selected is the inverted white one.
            painter.circle_filled(p + egui::vec2(0.0, 3.0), 13.0, Color32::from_black_alpha(70));
            let (fill, glyph_color) = if m.selected { (PRIMARY, PRIMARY_FG) } else { (SURFACE, FG) };
            painter.circle(p, 12.0, fill, Stroke::new(1.5, if m.selected { PRIMARY } else { BORDER_STRONG }));
            painter.text(p, Align2::CENTER_CENTER, m.glyph, theme::icons(12.0), glyph_color);
            let galley = painter.layout_no_wrap(m.label, theme::medium(12.0), FG);
            let rect = egui::Rect::from_center_size(p + egui::vec2(0.0, -30.0), galley.size() + egui::vec2(16.0, 8.0));
            painter.rect(rect, CornerRadius::same(theme::R_PILL), SURFACE, Stroke::new(1.0, BORDER), egui::StrokeKind::Inside);
            painter.galley(rect.min + egui::vec2(8.0, 4.0), galley, FG);
        }
    }
}

fn device_icon(device: &FindMyDevice) -> char {
    let model = device.model.to_lowercase();
    match device.kind {
        DeviceKind::Beacon => icon::MONITOR,
        _ if model.contains("iphone") => icon::PHONE,
        _ if model.contains("ipad") => icon::TABLET,
        _ if model.contains("watch") => icon::WATCH,
        _ if model.contains("airpods") || model.contains("beats") => icon::HEADPHONES,
        _ => icon::LAPTOP,
    }
}

impl FindMyPage {
    fn refresh(&mut self, cx: &Cx) {
        self.last = Some(Instant::now());
        self.req = Some(cx.client.call(Request::ListFindMyDevices));
    }

    fn poll(&mut self) {
        if let Some(r) = self.req.as_mut().and_then(Pending::take) {
            self.req = None;
            match r {
                Ok(Response::FindMyDevices(devices)) => {
                    self.error = None;
                    if self.selected.is_none()
                        && let Some(first) = devices.iter().find(|d| d.location.is_some())
                    {
                        self.selected = Some(first.id.clone());
                        let l = first.location.unwrap();
                        self.memory.center_at(lon_lat(l.lon, l.lat));
                        let _ = self.memory.set_zoom(13.0);
                    }
                    self.devices = devices;
                }
                Err(e) => self.error = Some(e.message),
                _ => {}
            }
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        if cx.connected && self.req.is_none() && self.last.is_none_or(|t| t.elapsed() > REFRESH) {
            self.refresh(cx);
        }
        self.poll();
        ui.ctx().request_repaint_after(REFRESH);
        if self.tiles.is_none() {
            self.tiles = Some(HttpTiles::new(OpenStreetMap, ui.ctx().clone()));
        }

        let width = ui.available_width();
        if width < NARROW {
            let top = ui.available_height() * 0.55;
            let (upper, lower) = kit::split_v(ui, top);
            kit::area(ui, upper, |ui| kit::bare_card(ui, |ui| self.map(ui)));
            kit::area(ui, lower, |ui| kit::bare_card(ui, |ui| self.list(ui, cx)));
        } else {
            let left = (width * 0.3).clamp(280.0, 340.0);
            let (l, r) = kit::split_h(ui, left);
            kit::area(ui, l, |ui| kit::bare_card(ui, |ui| self.list(ui, cx)));
            kit::area(ui, r, |ui| kit::bare_card(ui, |ui| self.map(ui)));
        }
    }

    fn list(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        egui::Frame::new().inner_margin(Margin { left: 15, right: 15, top: 15, bottom: 8 }).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(kit::title("Devices"));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if self.req.is_some() {
                        ui.add(egui::Spinner::new().size(14.0).color(FG_MUTED));
                    } else if kit::icon_button(ui, icon::REFRESH, "Refresh").clicked() {
                        self.refresh(cx);
                    }
                });
            });
            if let Some(error) = &self.error {
                ui.horizontal_wrapped(|ui| {
                    ui.label(kit::icon_text(icon::ALERT, 13.0, theme::WARNING));
                    ui.label(kit::muted(error));
                });
            }
        });
        if self.devices.is_empty() {
            if self.req.is_some() { kit::spinner_center(ui) } else { kit::empty_state(ui, icon::FIND_MY, "No devices", "Run bbd findmy-login on the Mac") }
            return;
        }
        let mut center = None;
        egui::Frame::new().inner_margin(Margin::symmetric(7, 0)).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("devices").auto_shrink(false).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for device in &self.devices {
                    let selected = self.selected.as_deref() == Some(device.id.as_str());
                    let response = kit::row(ui, selected, 58.0, |ui, rect| {
                        let painter = ui.painter_at(rect.expand(6.0));
                        let c = egui::pos2(rect.left() + 16.0, rect.center().y);
                        painter.circle(c, 16.0, theme::RAISED, Stroke::new(1.0, BORDER_STRONG));
                        painter.text(c, Align2::CENTER_CENTER, device_icon(device), theme::icons(15.0), FG);
                        let x = rect.left() + 46.0;
                        painter.text(egui::pos2(x, rect.top() + 10.0), Align2::LEFT_CENTER, &device.name, theme::medium(13.5), FG);
                        let detail = match device.location {
                            Some(l) => format!("{} · ±{:.0} m", util::ago(l.at), l.accuracy_m),
                            None => "No location".into(),
                        };
                        painter.text(egui::pos2(x, rect.top() + 30.0), Align2::LEFT_CENTER, detail, theme::body(12.0), FG_MUTED);
                        if let Some(battery) = device.battery {
                            let pct = (battery * 100.0).round();
                            // Color is information: only low battery is colored.
                            let color = if pct < 20.0 { theme::DANGER } else { FG_MUTED };
                            kit::paint_icon_text(&painter, egui::pos2(rect.right(), rect.top() + 10.0), Align2::RIGHT_CENTER, icon::BATTERY, &format!("{pct:.0}%"), theme::body(12.0), color);
                        }
                        if device.kind == DeviceKind::Beacon {
                            kit::paint_icon_text(&painter, egui::pos2(rect.right(), rect.top() + 30.0), Align2::RIGHT_CENTER, icon::BLUETOOTH, "beacon", theme::body(11.5), FG_MUTED);
                        }
                    });
                    if response.clicked() {
                        self.selected = Some(device.id.clone());
                        center = device.location;
                    }
                }
            });
        });
        if let Some(l) = center {
            self.memory.center_at(lon_lat(l.lon, l.lat));
        }
    }

    fn map(&mut self, ui: &mut egui::Ui) {
        let markers = Markers(
            self.devices
                .iter()
                .filter_map(|d| {
                    let l = d.location?;
                    Some(Marker {
                        position: lon_lat(l.lon, l.lat),
                        label: d.name.clone(),
                        glyph: device_icon(d),
                        selected: self.selected.as_deref() == Some(d.id.as_str()),
                    })
                })
                .collect(),
        );
        let home = self.devices.iter().find_map(|d| d.location).map_or(lon_lat(-122.009, 37.3349), |l| lon_lat(l.lon, l.lat));
        let rect = ui.available_rect_before_wrap();
        let tiles = self.tiles.as_mut().unwrap();
        ui.add(Map::new(Some(tiles), &mut self.memory, home).with_plugin(markers));
        // Round the map's corners to match its card: a thick ground-colored rounded stroke
        // whose inner edge is the card's radius covers the square tile corners.
        let painter = ui.painter();
        let r = f32::from(theme::R_CARD);
        painter.rect_stroke(rect.expand(6.0), CornerRadius::same((r + 6.0) as u8), Stroke::new(12.0, theme::BG), egui::StrokeKind::Middle);
        painter.rect_stroke(rect.expand(1.0), CornerRadius::same(theme::R_CARD), Stroke::new(1.0, BORDER), egui::StrokeKind::Inside);
        painter.text(rect.right_bottom() + egui::vec2(-14.0, -10.0), Align2::RIGHT_BOTTOM, "© OpenStreetMap", theme::body(10.0), Color32::from_gray(90));
    }
}
