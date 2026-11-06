use std::time::Duration;

use bb_proto::{ApiError, BlobKind, Event, Request, Response, VoiceMemo};
use eframe::egui::{self, Align, Align2, Layout, Margin, Sense, Vec2};

use crate::conn::Pending;
use crate::kit::{self, icon};
use crate::pages::Cx;
use crate::theme::{self, BORDER_STRONG, FG, FG_MUTED, FG_SUBTLE, PRIMARY, PRIMARY_FG};
use crate::util;

type Req = Pending<Result<Response, ApiError>>;

struct Playing {
    id: String,
    title: String,
    duration: f64,
    player: rodio::Player,
}

#[derive(Default)]
pub struct VoicePage {
    memos: Vec<VoiceMemo>,
    req: Option<Req>,
    loaded: bool,
    audio: Option<rodio::MixerDeviceSink>,
    playing: Option<Playing>,
    loading: Option<(VoiceMemo, Pending<Result<(Vec<u8>, Option<String>), ApiError>>)>,
    filter: String,
}

impl VoicePage {
    fn refresh(&mut self, cx: &Cx) {
        self.loaded = true;
        self.req = Some(cx.client.call(Request::ListVoiceMemos));
    }

    pub fn on_event(&mut self, event: &Event, cx: &mut Cx) {
        if matches!(event, Event::VoiceMemosChanged) && self.loaded {
            self.refresh(cx);
        }
    }

    fn play_bytes(&mut self, memo: VoiceMemo, bytes: Vec<u8>, cx: &Cx) {
        if self.audio.is_none() {
            match rodio::DeviceSinkBuilder::open_default_sink() {
                Ok(sink) => self.audio = Some(sink),
                Err(err) => return cx.toast(format!("No audio output: {err}")),
            }
        }
        match rodio::Decoder::try_from(std::io::Cursor::new(bytes)) {
            Ok(source) => {
                let player = rodio::Player::connect_new(self.audio.as_ref().unwrap().mixer());
                player.append(source);
                self.playing = Some(Playing { id: memo.id, title: memo.title, duration: memo.duration_secs, player });
            }
            Err(err) => cx.toast(format!("Can't play this recording: {err}")),
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        if !self.loaded && cx.connected {
            self.refresh(cx);
        }
        if let Some(r) = self.req.as_mut().and_then(Pending::take) {
            self.req = None;
            match r {
                Ok(Response::VoiceMemos(m)) => self.memos = m,
                Err(e) => cx.toast(format!("Voice Memos: {}", e.message)),
                _ => {}
            }
        }
        if let Some((_, pending)) = &mut self.loading
            && let Some(result) = pending.take()
        {
            let (memo, _) = self.loading.take().unwrap();
            match result {
                Ok((bytes, _)) => self.play_bytes(memo, bytes, cx),
                Err(e) => cx.toast(e.message),
            }
        }
        if let Some(playing) = &self.playing {
            if playing.player.empty() {
                self.playing = None;
            } else if !playing.player.is_paused() {
                ui.ctx().request_repaint_after(Duration::from_millis(100));
            }
        }

        let player_height = if self.playing.is_some() { 92.0 } else { 0.0 };
        if player_height > 0.0 {
            let list_height = ui.available_height() - player_height - theme::OUTER;
            let (upper, lower) = kit::split_v(ui, list_height);
            kit::area(ui, upper, |ui| kit::bare_card(ui, |ui| self.list(ui, cx)));
            kit::area(ui, lower, |ui| kit::card(ui, |ui| self.player(ui)));
        } else {
            kit::bare_card(ui, |ui| self.list(ui, cx));
        }
    }

    fn list(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        egui::Frame::new().inner_margin(Margin { left: 15, right: 15, top: 15, bottom: 4 }).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(kit::title("Voice Memos"));
                ui.label(kit::subtle(format!("{} recordings", self.memos.len())));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if kit::icon_button(ui, icon::REFRESH, "Refresh").clicked() {
                        self.refresh(cx);
                    }
                });
            });
            ui.add_space(4.0);
            kit::search(ui, &mut self.filter, "Search recordings");
        });
        if self.memos.is_empty() {
            if self.req.is_some() { kit::spinner_center(ui) } else { kit::empty_state(ui, icon::MEMOS, "No recordings", "") }
            return;
        }
        let needle = self.filter.to_lowercase();
        let memos: Vec<VoiceMemo> = self.memos.iter().filter(|m| needle.is_empty() || m.title.to_lowercase().contains(&needle)).cloned().collect();
        let mut play = None;
        let mut save = None;
        egui::Frame::new().inner_margin(Margin::symmetric(7, 4)).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("memo_list").auto_shrink(false).show_rows(ui, 58.0, memos.len(), |ui, range| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for memo in &memos[range] {
                    let active = self.playing.as_ref().is_some_and(|p| p.id == memo.id);
                    let loading = self.loading.as_ref().is_some_and(|(m, _)| m.id == memo.id);
                    let response = kit::row(ui, active, 56.0, |ui, rect| {
                        let painter = ui.painter_at(rect.expand(6.0));
                        let center = egui::pos2(rect.left() + 16.0, rect.center().y);
                        painter.circle_stroke(center, 16.0, egui::Stroke::new(1.0, BORDER_STRONG));
                        if loading {
                            painter.text(center, Align2::CENTER_CENTER, icon::CLOCK, theme::icons(14.0), FG_MUTED);
                        } else {
                            painter.text(center + egui::vec2(1.0, 0.0), Align2::CENTER_CENTER, if active { icon::MEMOS } else { icon::PLAY }, theme::icons(14.0), FG);
                        }
                        let x = rect.left() + 46.0;
                        painter.text(egui::pos2(x, rect.top() + 10.0), Align2::LEFT_CENTER, &memo.title, theme::medium(13.5), FG);
                        painter.text(egui::pos2(x, rect.top() + 30.0), Align2::LEFT_CENTER, format!("{} · {}", util::long_time(memo.created), util::bytes(memo.bytes)), theme::body(12.0), FG_MUTED);
                        painter.text(egui::pos2(rect.right() - 40.0, rect.center().y), Align2::RIGHT_CENTER, util::duration(memo.duration_secs), theme::body(12.5), FG_SUBTLE);
                    });
                    // Save button over the row's right edge.
                    let save_rect = egui::Rect::from_center_size(egui::pos2(response.rect.right() - 22.0, response.rect.center().y), Vec2::splat(28.0));
                    let save_response = ui.interact(save_rect, response.id.with("save"), Sense::click());
                    let hover = save_response.hovered();
                    ui.painter().text(save_rect.center(), Align2::CENTER_CENTER, icon::DOWNLOAD, theme::icons(15.0), if hover { FG } else { FG_SUBTLE });
                    if save_response.on_hover_text("Save to Downloads").clicked() {
                        save = Some(memo.clone());
                    } else if response.clicked() && !active && !loading {
                        play = Some(memo.clone());
                    }
                }
            });
        });
        if let Some(memo) = play {
            if let Some(p) = self.playing.take() {
                p.player.stop();
            }
            let pending = cx.client.fetch_blob(BlobKind::VoiceMemo, memo.id.clone());
            self.loading = Some((memo, pending));
        }
        if let Some(memo) = save {
            cx.download(BlobKind::VoiceMemo, memo.id, format!("{}.m4a", memo.title));
        }
    }

    fn player(&mut self, ui: &mut egui::Ui) {
        let Some(playing) = &mut self.playing else { return };
        ui.horizontal_centered(|ui| {
            let paused = playing.player.is_paused();
            // Play/pause is the player's one primary.
            let (rect, response) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::click());
            ui.painter().circle_filled(rect.center(), 20.0, PRIMARY);
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, if paused { icon::PLAY } else { icon::PAUSE }, theme::icons(17.0), PRIMARY_FG);
            if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                if paused { playing.player.play() } else { playing.player.pause() }
            }
            if kit::icon_button(ui, icon::STOP, "Stop").clicked() {
                playing.player.stop();
            }
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.label(egui::RichText::new(&playing.title).font(theme::medium(13.5)).color(FG));
                ui.horizontal(|ui| {
                    let pos = playing.player.get_pos().as_secs_f64();
                    let total = playing.duration.max(pos).max(0.1);
                    ui.label(kit::subtle(util::duration(pos)));
                    let mut seek = pos;
                    ui.spacing_mut().slider_width = (ui.available_width() - 60.0).max(80.0);
                    let slider = ui.add(egui::Slider::new(&mut seek, 0.0..=total).show_value(false));
                    if slider.drag_stopped() || (slider.changed() && !slider.dragged()) {
                        let _ = playing.player.try_seek(Duration::from_secs_f64(seek));
                    }
                    ui.label(kit::subtle(util::duration(total)));
                });
            });
        });
    }
}
