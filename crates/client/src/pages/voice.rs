use std::time::Duration;

use bb_proto::{ApiError, BlobKind, Event, Request, Response, VoiceMemo};
use eframe::egui::{self, RichText};

use crate::conn::Pending;
use crate::pages::Cx;
use crate::util;

type Req = Pending<Result<Response, ApiError>>;

struct Playing {
    id: String,
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
    loading: Option<(String, f64, Pending<Result<(Vec<u8>, Option<String>), ApiError>>)>,
}

impl VoicePage {
    fn refresh(&mut self, cx: &Cx) {
        self.loaded = true;
        self.req = Some(cx.client.call(Request::ListVoiceMemos));
    }

    pub fn on_event(&mut self, event: &Event, cx: &Cx) {
        if matches!(event, Event::VoiceMemosChanged) && self.loaded {
            self.refresh(cx);
        }
    }

    fn play_bytes(&mut self, id: String, duration: f64, bytes: Vec<u8>, cx: &Cx) {
        if self.audio.is_none() {
            match rodio::DeviceSinkBuilder::open_default_sink() {
                Ok(sink) => self.audio = Some(sink),
                Err(err) => {
                    cx.toast(format!("No audio output: {err}"));
                    return;
                }
            }
        }
        let sink = self.audio.as_ref().unwrap();
        match rodio::Decoder::try_from(std::io::Cursor::new(bytes)) {
            Ok(source) => {
                let player = rodio::Player::connect_new(sink.mixer());
                player.append(source);
                self.playing = Some(Playing { id, duration, player });
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
        if let Some((_, _, pending)) = &mut self.loading
            && let Some(result) = pending.take()
        {
            let (id, duration, _) = self.loading.take().unwrap();
            match result {
                Ok((bytes, _)) => self.play_bytes(id, duration, bytes, cx),
                Err(e) => cx.toast(e.message),
            }
        }
        if let Some(playing) = &self.playing {
            if playing.player.empty() {
                self.playing = None;
            } else if !playing.player.is_paused() {
                ui.ctx().request_repaint_after(Duration::from_millis(250));
            }
        }

        egui::Panel::top("voice_bar").show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading("Voice Memos");
                if ui.button("Refresh").clicked() {
                    self.refresh(cx);
                }
            });
            ui.add_space(4.0);
        });

        if let Some(playing) = &mut self.playing {
            egui::Panel::bottom("player").show(ui, |ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let paused = playing.player.is_paused();
                    if ui.button(if paused { "Play" } else { "Pause" }).clicked() {
                        if paused { playing.player.play() } else { playing.player.pause() }
                    }
                    if ui.button("Stop").clicked() {
                        playing.player.stop();
                    }
                    let pos = playing.player.get_pos().as_secs_f64();
                    let mut seek = pos;
                    let slider = ui.add(egui::Slider::new(&mut seek, 0.0..=playing.duration.max(pos).max(0.1)).show_value(false));
                    if slider.drag_stopped() || (slider.changed() && !slider.dragged()) {
                        let _ = playing.player.try_seek(Duration::from_secs_f64(seek));
                    }
                    ui.label(format!("{} / {}", util::duration(pos), util::duration(playing.duration)));
                });
                ui.add_space(4.0);
            });
        }

        egui::CentralPanel::default().show(ui, |ui| {
            if self.memos.is_empty() {
                ui.centered_and_justified(|ui| {
                    if self.req.is_some() { ui.spinner(); } else { ui.label(RichText::new("No recordings").weak()); }
                });
                return;
            }
            let mut play = None;
            let mut save = None;
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                for memo in &self.memos {
                    ui.horizontal(|ui| {
                        let active = self.playing.as_ref().is_some_and(|p| p.id == memo.id);
                        let loading = self.loading.as_ref().is_some_and(|(id, _, _)| *id == memo.id);
                        if loading {
                            ui.spinner();
                        } else if ui.add_enabled(!active, egui::Button::new("Play")).clicked() {
                            play = Some(memo.clone());
                        }
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&memo.title).strong());
                            ui.label(RichText::new(format!("{} · {} · {}", util::long_time(memo.created), util::duration(memo.duration_secs), util::bytes(memo.bytes))).weak().small());
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("Save").clicked() {
                                save = Some(memo.clone());
                            }
                        });
                    });
                    ui.separator();
                }
            });
            if let Some(memo) = play {
                if let Some(p) = self.playing.take() {
                    p.player.stop();
                }
                self.loading = Some((memo.id.clone(), memo.duration_secs, cx.client.fetch_blob(BlobKind::VoiceMemo, memo.id)));
            }
            if let Some(memo) = save {
                cx.download(BlobKind::VoiceMemo, memo.id, format!("{}.m4a", memo.title));
            }
        });
    }
}
