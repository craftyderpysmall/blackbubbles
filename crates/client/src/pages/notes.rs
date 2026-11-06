use bb_proto::{ApiError, Event, NoteFolder, NoteSummary, Request, Response};
use eframe::egui::{self, RichText};

use crate::conn::Pending;
use crate::pages::Cx;
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

    pub fn on_event(&mut self, event: &Event, cx: &Cx) {
        if matches!(event, Event::NotesChanged) && self.loaded {
            self.refresh(cx);
            // Reload the open note unless there are unsaved edits.
            if let Some(id) = self.open_id.clone()
                && self.body == self.saved_body
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

        egui::Panel::left("note_folders").resizable(true).default_size(180.0).show(ui, |ui| {
            ui.add_space(6.0);
            ui.heading("Folders");
            ui.add_space(4.0);
            let mut chosen = None;
            if ui.selectable_label(self.folder.is_none(), "All Notes").clicked() {
                chosen = Some(None);
            }
            let mut account = "";
            for folder in &self.folders {
                if folder.account != account {
                    account = &folder.account;
                    ui.add_space(6.0);
                    ui.label(RichText::new(account).weak().small());
                }
                let label = format!("{}  {}", folder.name, folder.count);
                if ui.selectable_label(self.folder.as_deref() == Some(folder.id.as_str()), label).clicked() {
                    chosen = Some(Some(folder.id.clone()));
                }
            }
            if let Some(folder) = chosen {
                self.folder = folder;
                self.notes_req = Some(cx.client.call(Request::ListNotes { folder_id: self.folder.clone() }));
            }
        });

        egui::Panel::left("note_list").resizable(true).default_size(260.0).show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("Search").desired_width(ui.available_width() - 48.0));
                if ui.button("New").clicked() {
                    self.create_req = Some(cx.client.call(Request::CreateNote { folder_id: self.folder.clone(), body: "New Note\n".into() }));
                }
            });
            ui.add_space(4.0);
            let needle = self.filter.to_lowercase();
            let mut chosen = None;
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                for note in self.notes.iter().filter(|n| needle.is_empty() || n.title.to_lowercase().contains(&needle) || n.snippet.to_lowercase().contains(&needle)) {
                    let selected = self.open_id.as_deref() == Some(note.id.as_str());
                    let response = ui.add(egui::Button::selectable(selected, RichText::new(&note.title).strong()).min_size(egui::vec2(ui.available_width(), 0.0)));
                    ui.label(RichText::new(format!("{}  {}", util::short_time(note.modified), note.snippet)).weak().small());
                    ui.separator();
                    if response.clicked() {
                        chosen = Some(note.id.clone());
                    }
                }
            });
            if let Some(id) = chosen {
                if self.dirty() {
                    self.save(cx);
                }
                self.open(id, cx);
            }
        });

        egui::CentralPanel::default().show(ui, |ui| {
            if self.open_id.is_none() {
                ui.centered_and_justified(|ui| ui.label(RichText::new("Select a note").weak().size(16.0)));
                return;
            }
            if self.note_req.is_some() {
                ui.centered_and_justified(|ui| ui.spinner());
                return;
            }
            ui.horizontal(|ui| {
                let save = ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S));
                if ui.add_enabled(self.dirty(), egui::Button::new("Save")).clicked() || (save && self.dirty()) {
                    self.save(cx);
                }
                ui.label(RichText::new(if self.dirty() { "Edited" } else { "Saved" }).weak());
                ui.label(RichText::new("· the first line is the title").weak().small());
            });
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                ui.add(egui::TextEdit::multiline(&mut self.body).desired_width(f32::INFINITY).desired_rows(30).font(egui::TextStyle::Body).frame(egui::Frame::NONE));
            });
        });
    }
}
