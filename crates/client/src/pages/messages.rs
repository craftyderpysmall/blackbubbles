use std::collections::HashMap;

use bb_proto::{ApiError, BlobKind, Chat, Event, Message, Request, Response, Service};
use eframe::egui::{self, Align, Color32, Layout, RichText, Sense, Vec2};

use crate::conn::Pending;
use crate::pages::Cx;
use crate::{media, util};

const ROW_HEIGHT: f32 = 56.0;
const PAGE: u32 = 60;

type Req = Pending<Result<Response, ApiError>>;

#[derive(Default)]
struct Thread {
    messages: Vec<Message>,
    loading: Option<Req>,
    older: Option<Req>,
    exhausted: bool,
}

#[derive(Default)]
pub struct MessagesPage {
    chats: Vec<Chat>,
    chats_req: Option<Req>,
    loaded: bool,
    selected: Option<String>,
    threads: HashMap<String, Thread>,
    filter: String,
    draft: String,
    sends: Vec<Req>,
    local_id: u64,
}

fn chat_title(chat: &Chat) -> String {
    chat.display_name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| chat.participants.join(", "))
}

fn bubble_color(ui: &egui::Ui, from_me: bool, service: Service) -> (Color32, Color32) {
    if from_me {
        let fill = if service == Service::IMessage { Color32::from_rgb(10, 132, 255) } else { Color32::from_rgb(48, 209, 88) };
        (fill, Color32::WHITE)
    } else {
        (ui.visuals().widgets.inactive.bg_fill, ui.visuals().strong_text_color())
    }
}

impl MessagesPage {
    pub fn refresh(&mut self, cx: &Cx) {
        self.chats_req = Some(cx.client.call(Request::ListChats { limit: 300, before: None }));
        self.loaded = true;
    }

    pub fn on_event(&mut self, event: &Event, cx: &Cx) {
        let Event::NewMessage(message) = event else { return };
        if let Some(thread) = self.threads.get_mut(&message.chat_guid) {
            if message.is_from_me {
                // Replace our optimistic copy.
                if let Some(pos) = thread.messages.iter().position(|m| m.guid.starts_with("local-") && m.text == message.text) {
                    thread.messages.remove(pos);
                }
            }
            if !thread.messages.iter().any(|m| m.guid == message.guid) {
                thread.messages.push(message.clone());
            }
        }
        match self.chats.iter().position(|c| c.guid == message.chat_guid) {
            Some(i) => {
                let mut chat = self.chats.remove(i);
                chat.last_message = Some(message.clone());
                self.chats.insert(0, chat);
            }
            None => self.refresh(cx),
        }
    }

    fn open(&mut self, guid: &str, cx: &Cx) {
        self.selected = Some(guid.to_string());
        let thread = self.threads.entry(guid.to_string()).or_default();
        if thread.messages.is_empty() && thread.loading.is_none() {
            thread.loading = Some(cx.client.call(Request::ListMessages { chat_guid: guid.into(), limit: PAGE, before: None }));
        }
    }

    fn poll(&mut self, cx: &mut Cx) {
        if let Some(req) = &mut self.chats_req
            && let Some(result) = req.take()
        {
            self.chats_req = None;
            match result {
                Ok(Response::Chats(chats)) => {
                    self.chats = chats;
                    // Developer aid for screenshots.
                    if self.selected.is_none() && std::env::var_os("BLACKBUBBLES_OPEN_FIRST_CHAT").is_some()
                        && let Some(first) = self.chats.iter().find(|c| c.guid.starts_with("iMessage")).map(|c| c.guid.clone())
                    {
                        self.open(&first, cx);
                    }
                }
                Ok(_) => {}
                Err(err) => cx.toast(format!("Messages: {}", err.message)),
            }
        }
        for thread in self.threads.values_mut() {
            if let Some(req) = &mut thread.loading
                && let Some(result) = req.take()
            {
                thread.loading = None;
                match result {
                    Ok(Response::Messages(list)) => {
                        thread.exhausted = list.len() < PAGE as usize;
                        thread.messages = list;
                    }
                    Ok(_) => {}
                    Err(err) => cx.toast(err.message),
                }
            }
            if let Some(req) = &mut thread.older
                && let Some(result) = req.take()
            {
                thread.older = None;
                if let Ok(Response::Messages(mut list)) = result {
                    thread.exhausted = list.len() < PAGE as usize;
                    list.append(&mut thread.messages);
                    thread.messages = list;
                }
            }
        }
        self.sends.retain_mut(|req| match req.take() {
            Some(Err(err)) => {
                cx.toast(format!("Not sent: {}", err.message));
                false
            }
            Some(Ok(_)) => false,
            None => true,
        });
    }

    fn send(&mut self, cx: &Cx) {
        let text = self.draft.trim().to_string();
        let Some(chat) = self.selected.clone() else { return };
        if text.is_empty() {
            return;
        }
        self.draft.clear();
        self.local_id += 1;
        if let Some(thread) = self.threads.get_mut(&chat) {
            thread.messages.push(Message {
                guid: format!("local-{}", self.local_id),
                chat_guid: chat.clone(),
                sender: None,
                is_from_me: true,
                text: Some(text.clone()),
                date: chrono::Utc::now().timestamp_millis(),
                attachments: vec![],
            });
        }
        self.sends.push(cx.client.call(Request::SendText { chat_guid: chat, text }));
    }

    fn send_files(&mut self, files: Vec<egui::DroppedFileHandle>, cx: &mut Cx) {
        let Some(chat) = self.selected.clone() else { return };
        for file in files {
            let name = file.path().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "attachment".into());
            match file.bytes() {
                Ok(data) => {
                    cx.toast(format!("Sending {name}…"));
                    self.sends.push(cx.client.call(Request::SendAttachment { chat_guid: chat.clone(), file_name: name, data }));
                }
                Err(err) => cx.toast(format!("Can't read {name}: {err}")),
            }
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        if !self.loaded && cx.connected {
            self.refresh(cx);
        }
        self.poll(cx);

        egui::Panel::left("chat_list").resizable(true).default_size(300.0).min_size(220.0).show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("Search").desired_width(f32::INFINITY));
            });
            ui.add_space(4.0);
            let needle = self.filter.to_lowercase();
            let visible: Vec<usize> = (0..self.chats.len())
                .filter(|&i| needle.is_empty() || chat_title(&self.chats[i]).to_lowercase().contains(&needle))
                .collect();
            if self.chats_req.is_some() && self.chats.is_empty() {
                ui.centered_and_justified(|ui| ui.spinner());
                return;
            }
            let mut clicked = None;
            egui::ScrollArea::vertical().auto_shrink(false).show_rows(ui, ROW_HEIGHT, visible.len(), |ui, range| {
                for &i in &visible[range] {
                    let chat = &self.chats[i];
                    let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_HEIGHT), Sense::click());
                    let selected = self.selected.as_deref() == Some(chat.guid.as_str());
                    if selected {
                        ui.painter().rect_filled(rect, 8.0, ui.visuals().selection.bg_fill);
                    } else if response.hovered() {
                        ui.painter().rect_filled(rect, 8.0, ui.visuals().widgets.hovered.weak_bg_fill);
                    }
                    let text_color = if selected { ui.visuals().selection.stroke.color } else { ui.visuals().text_color() };
                    let weak = if selected { text_color } else { ui.visuals().weak_text_color() };
                    let painter = ui.painter_at(rect.shrink(8.0));
                    let left = rect.left() + 12.0;
                    painter.text(egui::pos2(left, rect.top() + 18.0), egui::Align2::LEFT_CENTER, chat_title(chat), egui::FontId::proportional(14.5), text_color);
                    if let Some(last) = &chat.last_message {
                        let preview = last.text.clone().unwrap_or_else(|| if last.attachments.is_empty() { String::new() } else { "Attachment".into() });
                        let preview: String = preview.replace('\n', " ").chars().take(60).collect();
                        painter.text(egui::pos2(left, rect.top() + 38.0), egui::Align2::LEFT_CENTER, preview, egui::FontId::proportional(12.5), weak);
                        painter.text(egui::pos2(rect.right() - 12.0, rect.top() + 18.0), egui::Align2::RIGHT_CENTER, util::short_time(last.date), egui::FontId::proportional(11.5), weak);
                    }
                    if response.clicked() {
                        clicked = Some(chat.guid.clone());
                    }
                }
            });
            if let Some(guid) = clicked {
                self.open(&guid, cx);
            }
        });

        let Some(chat_guid) = self.selected.clone() else {
            egui::CentralPanel::default().show(ui, |ui| {
                ui.centered_and_justified(|ui| ui.label(RichText::new("Select a conversation").weak().size(16.0)));
            });
            return;
        };
        let chat = self.chats.iter().find(|c| c.guid == chat_guid).cloned();
        let service = chat.as_ref().map_or(Service::IMessage, |c| c.service);

        egui::Panel::top("chat_header").show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading(chat.as_ref().map(chat_title).unwrap_or_default());
                let label = match service {
                    Service::IMessage => "iMessage",
                    Service::Sms => "SMS",
                    Service::Rcs => "RCS",
                };
                ui.label(RichText::new(label).weak());
            });
            ui.add_space(4.0);
        });

        egui::Panel::bottom("composer").show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let hint = if service == Service::IMessage { "iMessage" } else { "Text Message" };
                let edit = ui.add(
                    egui::TextEdit::multiline(&mut self.draft)
                        .hint_text(hint)
                        .desired_rows(1)
                        .desired_width(ui.available_width() - 70.0)
                        .return_key(None),
                );
                let enter = edit.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);
                if enter {
                    // Drop the newline the TextEdit may have inserted.
                    while self.draft.ends_with('\n') {
                        self.draft.pop();
                    }
                }
                if ui.add_enabled(!self.draft.trim().is_empty(), egui::Button::new("Send")).clicked() || enter {
                    self.send(cx);
                    edit.request_focus();
                }
            });
            ui.label(RichText::new("Drop files on the window to send them").weak().small());
            ui.add_space(4.0);
        });

        let dropped = ui.input(|i| i.raw.dropped_files.clone());
        if !dropped.is_empty() {
            self.send_files(dropped, cx);
        }

        egui::CentralPanel::default().show(ui, |ui| {
            let thread = self.threads.entry(chat_guid.clone()).or_default();
            if thread.loading.is_some() {
                ui.centered_and_justified(|ui| ui.spinner());
                return;
            }
            egui::ScrollArea::vertical().auto_shrink(false).stick_to_bottom(true).show(ui, |ui| {
                if !thread.exhausted && !thread.messages.is_empty() {
                    ui.vertical_centered(|ui| {
                        if thread.older.is_some() {
                            ui.spinner();
                        } else if ui.button("Load earlier messages").clicked() {
                            let before = thread.messages.first().map(|m| m.date);
                            thread.older = Some(cx.client.call(Request::ListMessages { chat_guid: chat_guid.clone(), limit: PAGE, before }));
                        }
                    });
                }
                let max_width = (ui.available_width() * 0.7).max(200.0);
                let mut last_day = None;
                for message in &thread.messages {
                    let day = util::local(message.date).map(|t| t.date_naive());
                    if day != last_day {
                        last_day = day;
                        ui.add_space(8.0);
                        ui.vertical_centered(|ui| ui.label(RichText::new(util::long_time(message.date)).weak().small()));
                    }
                    let layout = if message.is_from_me { Layout::right_to_left(Align::TOP) } else { Layout::left_to_right(Align::TOP) };
                    ui.with_layout(layout, |ui| {
                        ui.set_max_width(ui.available_width());
                        ui.vertical(|ui| {
                            ui.set_max_width(max_width);
                            if !message.is_from_me
                                && chat.as_ref().is_some_and(|c| c.participants.len() > 1)
                                && let Some(sender) = &message.sender
                            {
                                ui.label(RichText::new(sender).weak().small());
                            }
                            for attachment in &message.attachments {
                                let is_image = attachment.mime_type.as_deref().is_some_and(|m| m.starts_with("image/"));
                                if is_image {
                                    media::show(ui, cx.images, cx.client, BlobKind::Attachment, &attachment.guid, Vec2::new(240.0, 240.0));
                                } else {
                                    let name = attachment.file_name.clone().unwrap_or_else(|| "attachment".into());
                                    if ui.button(format!("⬇ {name} ({})", util::bytes(attachment.total_bytes))).clicked() {
                                        cx.download(BlobKind::Attachment, attachment.guid.clone(), name);
                                    }
                                }
                            }
                            if let Some(text) = &message.text {
                                let (fill, color) = bubble_color(ui, message.is_from_me, service);
                                let pending = message.guid.starts_with("local-");
                                egui::Frame::new()
                                    .fill(if pending { fill.gamma_multiply(0.6) } else { fill })
                                    .corner_radius(14.0)
                                    .inner_margin(egui::Margin::symmetric(10, 6))
                                    .show(ui, |ui| {
                                        ui.add(egui::Label::new(RichText::new(text).color(color).size(14.0)).wrap().selectable(true));
                                    });
                            }
                        });
                    });
                    ui.add_space(3.0);
                }
            });
        });
    }
}
