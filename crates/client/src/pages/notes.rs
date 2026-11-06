use bb_proto::{ApiError, Event, NoteFolder, NoteSummary, Request, Response};
use eframe::egui::{self, Align, Align2, Layout, Margin, RichText};

use crate::conn::Pending;
use crate::kit::{self, icon};
use crate::pages::{Cx, NARROW};
use crate::theme::{self, FG, FG_MUTED, FG_SUBTLE};
use crate::util;

type Req = Pending<Result<Response, ApiError>>;

#[derive(Default)]
pub struct NotesPage {
    folders: Vec<NoteFolder>,
    folders_req: Option<Req>,
    folder: Option<String>,
    notes: Vec<NoteSummary>,
    notes_req: Option<Req>,
    open_id: Option<String>,
    note_req: Option<Req>,
    body: String,
    saved_body: String,
    save_req: Option<Req>,
    create_req: Option<Req>,
    filter: String,
    loaded: bool,
}

impl NotesPage {
    fn refresh(&mut self, cx: &Cx) {
        self.loaded = true;
        self.folders_req = Some(cx.client.call(Request::ListNoteFolders));
        self.notes_req = Some(cx.client.call(Request::ListNotes { folder_id: self.folder.clone() }));
    }

    pub fn on_event(&mut self, event: &Event, cx: &mut Cx) {
        if matches!(event, Event::NotesChanged) && self.loaded {
            self.refresh(cx);
            if let Some(id) = self.open_id.clone()
                && !self.dirty()
            {
                self.note_req = Some(cx.client.call(Request::GetNote { id }));
            }
        }
    }

    fn dirty(&self) -> bool {
        self.open_id.is_some() && self.body != self.saved_body
    }

    fn poll(&mut self, cx: &Cx) {
        if let Some(r) = self.folders_req.as_mut().and_then(Pending::take) {
            self.folders_req = None;
            match r {
                Ok(Response::NoteFolders(f)) => self.folders = f,
                Err(e) => cx.toast(format!("Notes: {}", e.message)),
                _ => {}
            }
        }
        if let Some(r) = self.notes_req.as_mut().and_then(Pending::take) {
            self.notes_req = None;
            if let Ok(Response::Notes(n)) = r {
                self.notes = n;
            }
        }
        if let Some(r) = self.note_req.as_mut().and_then(Pending::take) {
            self.note_req = None;
            match r {
                Ok(Response::Note(note)) => {
                    self.body = note.body.clone();
                    self.saved_body = note.body;
                }
                Err(e) => cx.toast(e.message),
                _ => {}
            }
        }
        if let Some(r) = self.save_req.as_mut().and_then(Pending::take) {
            self.save_req = None;
            match r {
                Ok(_) => cx.toast("Note saved"),
                Err(e) => cx.toast(format!("Not saved: {}", e.message)),
            }
        }
        if let Some(r) = self.create_req.as_mut().and_then(Pending::take) {
            self.create_req = None;
            match r {
                Ok(Response::Created { id }) => {
                    self.open(id, cx);
                    self.refresh(cx);
                }
                Err(e) => cx.toast(format!("Couldn't create note: {}", e.message)),
                _ => {}
            }
        }
    }

    fn open(&mut self, id: String, cx: &Cx) {
        if self.dirty() {
            self.save(cx);
        }
        self.note_req = Some(cx.client.call(Request::GetNote { id: id.clone() }));
        self.open_id = Some(id);
        self.body.clear();
        self.saved_body.clear();
    }

    fn save(&mut self, cx: &Cx) {
        if let Some(id) = self.open_id.clone() {
            self.saved_body = self.body.clone();
            self.save_req = Some(cx.client.call(Request::UpdateNote { id, body: self.body.clone() }));
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        if !self.loaded && cx.connected {
            self.refresh(cx);
        }
        self.poll(cx);
        if ui.available_width() < NARROW {
            if self.open_id.is_some() {
                kit::card(ui, |ui| self.editor(ui, cx, true));
            } else {
                kit::bare_card(ui, |ui| self.list(ui, cx));
            }
        } else {
            let left = (ui.available_width() * 0.32).clamp(280.0, 380.0);
            let (l, r) = kit::split_h(ui, left);
            kit::area(ui, l, |ui| kit::bare_card(ui, |ui| self.list(ui, cx)));
            kit::area(ui, r, |ui| kit::card(ui, |ui| self.editor(ui, cx, false)));
        }
    }

    fn list(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        egui::Frame::new().inner_margin(Margin { left: 15, right: 15, top: 15, bottom: 4 }).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(kit::title("Notes"));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if kit::primary(ui, Some(icon::PLUS), "New", cx.connected).clicked() {
                        self.create_req = Some(cx.client.call(Request::CreateNote { folder_id: self.folder.clone(), body: "New Note\n".into() }));
                    }
                });
            });
            ui.add_space(4.0);
            kit::search(ui, &mut self.filter, "Search notes");
            ui.add_space(4.0);
            // Folders as wrapping chips: compact enough for every width.
            let mut chosen = None;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                if chip(ui, "All", self.folder.is_none()).clicked() {
                    chosen = Some(None);
                }
                for folder in &self.folders {
                    if chip(ui, &format!("{} {}", folder.name, folder.count), self.folder.as_deref() == Some(folder.id.as_str())).on_hover_text(&folder.account).clicked() {
                        chosen = Some(Some(folder.id.clone()));
                    }
                }
            });
            if let Some(folder) = chosen {
                self.folder = folder;
                self.notes_req = Some(cx.client.call(Request::ListNotes { folder_id: self.folder.clone() }));
            }
        });
        let needle = self.filter.to_lowercase();
        let notes: Vec<&NoteSummary> = self.notes.iter().filter(|n| needle.is_empty() || n.title.to_lowercase().contains(&needle) || n.snippet.to_lowercase().contains(&needle)).collect();
        if notes.is_empty() {
            if self.notes_req.is_some() { kit::spinner_center(ui) } else { kit::empty_state(ui, icon::NOTES, "No notes", "") }
            return;
        }
        let mut chosen = None;
        egui::Frame::new().inner_margin(Margin::symmetric(7, 4)).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("note_list").auto_shrink(false).show_rows(ui, 58.0, notes.len(), |ui, range| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for note in &notes[range] {
                    let selected = self.open_id.as_deref() == Some(note.id.as_str());
                    let response = kit::row(ui, selected, 56.0, |ui, rect| {
                        let painter = ui.painter_at(rect.expand(6.0));
                        let chars = (rect.width() / 7.0) as usize;
                        painter.text(rect.left_top() + egui::vec2(0.0, 9.0), Align2::LEFT_CENTER, truncate(&note.title, chars), theme::medium(13.5), FG);
                        let meta = format!("{}  {}", util::short_time(note.modified), note.snippet.replace('\n', " "));
                        painter.text(rect.left_top() + egui::vec2(0.0, 29.0), Align2::LEFT_CENTER, truncate(&meta, (rect.width() / 6.2) as usize), theme::body(12.0), FG_MUTED);
                    });
                    if response.clicked() {
                        chosen = Some(note.id.clone());
                    }
                }
            });
        });
        if let Some(id) = chosen {
            self.open(id, cx);
        }
    }

    fn editor(&mut self, ui: &mut egui::Ui, cx: &mut Cx, narrow: bool) {
        if self.open_id.is_none() {
            kit::empty_state(ui, icon::NOTES, "Select a note", "The first line becomes the title");
            return;
        }
        let save_key = ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S));
        ui.horizontal(|ui| {
            if narrow && kit::icon_button(ui, icon::BACK, "Back").clicked() {
                if self.dirty() {
                    self.save(cx);
                }
                self.open_id = None;
            }
            let title = self.body.lines().next().unwrap_or("New Note").to_string();
            ui.label(kit::title(truncate(&title, 48)));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let dirty = self.dirty();
                if kit::primary(ui, Some(icon::CHECK), "Save", dirty).on_hover_text("Ctrl/⌘ + S").clicked() || (save_key && dirty) {
                    self.save(cx);
                }
                let (text, dot) = if self.save_req.is_some() { ("Saving…", theme::WARNING) } else if dirty { ("Edited", theme::WARNING) } else { ("Saved", theme::SUCCESS) };
                kit::badge(ui, text, Some(dot));
            });
        });
        ui.add_space(12.0);
        kit::divider(ui);
        ui.add_space(12.0);
        if self.open_id.is_none() {
            return;
        }
        if self.note_req.is_some() {
            kit::spinner_center(ui);
            return;
        }
        egui::ScrollArea::vertical().id_salt("note_editor").auto_shrink(false).show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut self.body)
                    .desired_width(f32::INFINITY)
                    .desired_rows(24)
                    .font(theme::body(14.5))
                    .text_color(FG)
                    .frame(egui::Frame::NONE)
                    .hint_text(RichText::new("Start writing…").color(FG_SUBTLE)),
            );
        });
    }
}

fn chip(ui: &mut egui::Ui, text: &str, selected: bool) -> egui::Response {
    if selected { kit::outline(ui, None, text, true) } else { kit::ghost(ui, None, text) }
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max.max(4) {
        text.to_string()
    } else {
        let mut out: String = text.chars().take(max.max(4) - 1).collect();
        out.push('…');
        out
    }
}
