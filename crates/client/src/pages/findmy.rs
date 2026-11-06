use std::time::{Duration, Instant};

use bb_proto::{ApiError, DeviceKind, FindMyDevice, Request, Response};
use eframe::egui::{self, Color32, RichText};
use walkers::{HttpTiles, Map, MapMemory, Plugin, Position, Projector, lon_lat, sources::OpenStreetMap};

use crate::conn::Pending;
use crate::pages::Cx;
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

struct Markers(Vec<(Position, String, Color32, bool)>);

impl Plugin for Markers {
    fn run(self: Box<Self>, ui: &mut egui::Ui, _response: &egui::Response, projector: &Projector, _memory: &MapMemory) {
        for (position, label, color, selected) in self.0 {
            let p = projector.project(position).to_pos2();
            let radius = if selected { 10.0 } else { 7.0 };
            ui.painter().circle(p, radius, color, egui::Stroke::new(2.0, Color32::WHITE));
            let galley = ui.painter().layout_no_wrap(label, egui::FontId::proportional(12.0), Color32::BLACK);
            let rect = egui::Rect::from_center_size(p + egui::vec2(0.0, -radius - 12.0), galley.size() + egui::vec2(10.0, 4.0));
            ui.painter().rect_filled(rect, 4.0, Color32::from_white_alpha(230));
            ui.painter().galley(rect.min + egui::vec2(5.0, 2.0), galley, Color32::BLACK);
        }
    }
}

fn marker(device: &FindMyDevice) -> &'static str {
    if device.kind == DeviceKind::Beacon { "◆" } else { "●" }
}

impl FindMyPage {
    fn refresh(&mut self, cx: &Cx) {
        self.last = Some(Instant::now());
        self.req = Some(cx.client.call(Request::ListFindMyDevices));
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        if cx.connected && self.req.is_none() && self.last.is_none_or(|t| t.elapsed() > REFRESH) {
            self.refresh(cx);
        }
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
        ui.ctx().request_repaint_after(REFRESH);
        let tiles = self.tiles.get_or_insert_with(|| HttpTiles::new(OpenStreetMap, ui.ctx().clone()));

        egui::Panel::left("devices").resizable(true).default_size(280.0).show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading("Devices");
                if self.req.is_some() {
                    ui.spinner();
                } else if ui.button("Refresh").clicked() {
                    let (last, req) = (Some(Instant::now()), Some(cx.client.call(Request::ListFindMyDevices)));
                    self.last = last;
                    self.req = req;
                }
            });
            if let Some(error) = &self.error {
                ui.label(RichText::new(error).color(ui.visuals().warn_fg_color));
            }
            ui.add_space(4.0);
            let mut center = None;
            egui::ScrollArea::vertical().show(ui, |ui| {
                for device in &self.devices {
                    let selected = self.selected.as_deref() == Some(device.id.as_str());
                    let title = format!("{} {}", marker(device), device.name);
                    let response = ui.add(egui::Button::selectable(selected, RichText::new(title).strong()).min_size(egui::vec2(ui.available_width(), 0.0)));
                    let mut detail = device.model.clone();
                    if let Some(battery) = device.battery {
                        detail.push_str(&format!(" · battery {:.0}%", battery * 100.0));
                    }
                    match device.location {
                        Some(l) => detail.push_str(&format!(" · {} · ±{:.0} m", util::ago(l.at), l.accuracy_m)),
                        None => detail.push_str(" · no location"),
                    }
                    ui.label(RichText::new(detail).weak().small());
                    ui.separator();
                    if response.clicked() {
                        self.selected = Some(device.id.clone());
                        center = device.location;
                    }
                }
            });
            if let Some(l) = center {
                self.memory.center_at(lon_lat(l.lon, l.lat));
            }
        });

        egui::CentralPanel::default().frame(egui::Frame::new()).show(ui, |ui| {
            let markers = Markers(
                self.devices
                    .iter()
                    .filter_map(|d| {
                        let l = d.location?;
                        let color = if d.kind == DeviceKind::Beacon { Color32::from_rgb(255, 159, 10) } else { Color32::from_rgb(10, 132, 255) };
                        Some((lon_lat(l.lon, l.lat), d.name.clone(), color, self.selected.as_deref() == Some(d.id.as_str())))
                    })
                    .collect(),
            );
            let home = self.devices.iter().find_map(|d| d.location).map_or(lon_lat(-122.009, 37.3349), |l| lon_lat(l.lon, l.lat));
            ui.add(Map::new(Some(tiles), &mut self.memory, home).with_plugin(markers));
            ui.painter().text(
                ui.max_rect().right_bottom() + egui::vec2(-6.0, -4.0),
                egui::Align2::RIGHT_BOTTOM,
                "© OpenStreetMap contributors",
                egui::FontId::proportional(10.0),
                Color32::DARK_GRAY,
            );
        });
    }
}
