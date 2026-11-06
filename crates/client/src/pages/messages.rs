use std::collections::HashMap;

use bb_proto::{ApiError, BlobKind, Chat, Event, Message, Request, Response, Service};
use eframe::egui::{self, Align, Align2, CornerRadius, Layout, Margin, RichText, Stroke, Vec2};

use crate::conn::Pending;
use crate::kit::{self, icon};
use crate::pages::{Cx, NARROW};
use crate::theme::{self, BORDER, BORDER_STRONG, FG, FG_MUTED, FG_SUBTLE, PRIMARY, PRIMARY_FG, RAISED};
use crate::{media, util};

const ROW_HEIGHT: f32 = 60.0;
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

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

impl MessagesPage {
    pub fn refresh(&mut self, cx: &Cx) {
        self.chats_req = Some(cx.client.call(Request::ListChats { limit: 300, before: None }));
        self.loaded = true;
    }

    pub fn on_event(&mut self, event: &Event, cx: &mut Cx) {
        let Event::NewMessage(message) = event else { return };
        if let Some(thread) = self.threads.get_mut(&message.chat_guid) {
            if message.is_from_me
                && let Some(pos) = thread.messages.iter().position(|m| m.guid.starts_with("local-") && m.text == message.text)
            {
                thread.messages.remove(pos);
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
        if let Some(result) = self.chats_req.as_mut().and_then(Pending::take) {
            self.chats_req = None;
            match result {
                Ok(Response::Chats(chats)) => {
                    self.chats = chats;
                    // Developer aid for screenshots.
                    if self.selected.is_none()
                        && std::env::var_os("BLACKBUBBLES_OPEN_FIRST_CHAT").is_some()
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
            if let Some(result) = thread.loading.as_mut().and_then(Pending::take) {
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
            if let Some(result) = thread.older.as_mut().and_then(Pending::take) {
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

        let dropped = ui.input(|i| i.raw.dropped_files.clone());
        if !dropped.is_empty() {
            self.send_files(dropped, cx);
        }

        if ui.available_width() < NARROW {
            // One pane at a time on narrow windows.
            if self.selected.is_some() {
                kit::card(ui, |ui| self.thread(ui, cx, true));
            } else {
                kit::bare_card(ui, |ui| self.list(ui, cx));
            }
        } else {
            let left = (ui.available_width() * 0.3).clamp(280.0, 360.0);
            let (l, r) = kit::split_h(ui, left);
            kit::area(ui, l, |ui| kit::bare_card(ui, |ui| self.list(ui, cx)));
            kit::area(ui, r, |ui| kit::card(ui, |ui| self.thread(ui, cx, false)));
        }
    }

    fn list(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        egui::Frame::new().inner_margin(Margin { left: 15, right: 15, top: 15, bottom: 4 }).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(kit::title("Messages"));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if self.chats_req.is_some() {
                        ui.add(egui::Spinner::new().size(14.0).color(FG_SUBTLE));
                    }
                });
            });
            ui.add_space(4.0);
            kit::search(ui, &mut self.filter, "Search");
        });
        if self.chats.is_empty() {
            if self.chats_req.is_some() {
                kit::spinner_center(ui);
            } else {
                kit::empty_state(ui, icon::MESSAGES, "No conversations", if cx.connected { "" } else { "Connect to your Mac in Settings" });
            }
            return;
        }
        let needle = self.filter.to_lowercase();
        let visible: Vec<usize> = (0..self.chats.len())
            .filter(|&i| {
                let chat = &self.chats[i];
                needle.is_empty()
                    || chat_title(chat).to_lowercase().contains(&needle)
                    || chat.last_message.as_ref().and_then(|m| m.text.as_ref()).is_some_and(|t| t.to_lowercase().contains(&needle))
            })
            .collect();
        let mut clicked = None;
        egui::Frame::new().inner_margin(Margin::symmetric(7, 4)).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("chat_list").auto_shrink(false).show_rows(ui, ROW_HEIGHT + 2.0, visible.len(), |ui, range| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for &i in &visible[range] {
                    let chat = &self.chats[i];
                    let selected = self.selected.as_deref() == Some(chat.guid.as_str());
                    let muted = cx.config.notifications.muted_chats.contains(&chat.guid);
                    let response = kit::row(ui, selected, ROW_HEIGHT, |ui, rect| {
                        let painter = ui.painter_at(rect.expand(6.0));
                        let name = chat_title(chat);
                        kit::avatar(ui, egui::pos2(rect.left() + 18.0, rect.center().y), 18.0, &name);
                        let x = rect.left() + 48.0;
                        let time = chat.last_message.as_ref().map(|m| util::short_time(m.date)).unwrap_or_default();
                        painter.text(egui::pos2(rect.right(), rect.top() + 10.0), Align2::RIGHT_CENTER, &time, theme::body(11.5), FG_SUBTLE);
                        let max_name = ((rect.right() - x - 60.0) / 7.0).max(6.0) as usize;
                        let title_rect = painter.text(egui::pos2(x, rect.top() + 10.0), Align2::LEFT_CENTER, truncate(&name, max_name), theme::medium(13.5), FG);
                        if muted {
                            painter.text(egui::pos2(title_rect.right() + 6.0, rect.top() + 10.0), Align2::LEFT_CENTER, icon::BELL_OFF, theme::icons(12.0), FG_SUBTLE);
                        }
                        if let Some(last) = &chat.last_message {
                            let chars = ((rect.right() - x) / 6.3) as usize;
                            match &last.text {
                                Some(text) => {
                                    painter.text(egui::pos2(x, rect.top() + 31.0), Align2::LEFT_CENTER, truncate(&text.replace('\n', " "), chars.max(8)), theme::body(12.5), FG_MUTED);
                                }
                                None if !last.attachments.is_empty() => {
                                    kit::paint_icon_text(&painter, egui::pos2(x, rect.top() + 31.0), Align2::LEFT_CENTER, icon::PAPERCLIP, "Attachment", theme::body(12.5), FG_MUTED);
                                }
                                None => {}
                            }
                        }
                    });
                    if response.clicked() {
                        clicked = Some(chat.guid.clone());
                    }
                }
            });
        });
        if let Some(guid) = clicked {
            self.open(&guid, cx);
        }
    }

    fn header(&mut self, ui: &mut egui::Ui, cx: &mut Cx, chat: Option<&Chat>, narrow: bool) {
        ui.horizontal(|ui| {
            if narrow && kit::icon_button(ui, icon::BACK, "Back").clicked() {
                self.selected = None;
            }
            let name = chat.map(chat_title).unwrap_or_default();
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(36.0), egui::Sense::hover());
            kit::avatar(ui, rect.center(), 18.0, &name);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                ui.label(kit::title(&name));
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let service = match chat.map(|c| c.service) {
                        Some(Service::Sms) => "SMS",
                        Some(Service::Rcs) => "RCS",
                        _ => "iMessage",
                    };
                    kit::badge(ui, service, None);
                    if let Some(mine) = chat.and_then(|c| c.my_address.clone()) {
                        let email = mine.contains('@');
                        let glyph = if email { icon::MAIL } else { icon::PHONE };
                        kit::badge_icon(ui, Some(glyph), &format!("from {mine}"), email.then_some(theme::WARNING)).on_hover_text(if email {
                            "This conversation sends from your email address, and replies keep using it. See Settings → Sending address to use your phone number."
                        } else {
                            "Replies in this conversation are sent from your phone number."
                        });
                    }
                });
            });
            if let Some(chat) = chat {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let muted = cx.config.notifications.muted_chats.contains(&chat.guid);
                    let (glyph, tip) = if muted { (icon::BELL_OFF, "Unmute notifications") } else { (icon::BELL, "Mute notifications") };
                    if kit::icon_button(ui, glyph, tip).clicked() {
                        let list = &mut cx.config.notifications.muted_chats;
                        if muted {
                            list.retain(|g| *g != chat.guid);
                        } else {
                            list.push(chat.guid.clone());
                        }
                        cx.save();
                        cx.toast(if muted { "Notifications on for this conversation" } else { "Conversation muted" });
                    }
                });
            }
        });
        ui.add_space(12.0);
        kit::divider(ui);
    }

    fn thread(&mut self, ui: &mut egui::Ui, cx: &mut Cx, narrow: bool) {
        let Some(chat_guid) = self.selected.clone() else {
            kit::empty_state(ui, icon::MESSAGES, "Select a conversation", "Tip: drop files on the window to send them");
            return;
        };
        let chat = self.chats.iter().find(|c| c.guid == chat_guid).cloned();
        self.header(ui, cx, chat.as_ref(), narrow);
        if self.selected.is_none() {
            return;
        }
        let service = chat.as_ref().map_or(Service::IMessage, |c| c.service);

        // The composer is pinned to the bottom of the card.
        let composer_height = 56.0;
        let full = ui.available_rect_before_wrap();
        let list_rect = egui::Rect::from_min_max(full.min, egui::pos2(full.right(), full.bottom() - composer_height));
        let composer_rect = egui::Rect::from_min_max(egui::pos2(full.left(), full.bottom() - composer_height + 12.0), full.max);

        ui.scope_builder(egui::UiBuilder::new().max_rect(list_rect), |ui| {
            let thread = self.threads.entry(chat_guid.clone()).or_default();
            if thread.loading.is_some() {
                kit::spinner_center(ui);
                return;
            }
            egui::ScrollArea::vertical().id_salt(("thread", chat_guid.as_str())).auto_shrink(false).stick_to_bottom(true).show(ui, |ui| {
                ui.add_space(8.0);
                if !thread.exhausted && !thread.messages.is_empty() {
                    ui.vertical_centered(|ui| {
                        if thread.older.is_some() {
                            ui.add(egui::Spinner::new().size(14.0).color(FG_SUBTLE));
                        } else if kit::ghost(ui, None, "Load earlier messages").clicked() {
                            let before = thread.messages.first().map(|m| m.date);
                            thread.older = Some(cx.client.call(Request::ListMessages { chat_guid: chat_guid.clone(), limit: PAGE, before }));
                        }
                    });
                }
                let max_width = (ui.available_width() * 0.72).clamp(180.0, 560.0);
                let group = chat.as_ref().is_some_and(|c| c.participants.len() > 1);
                let mut last_day = None;
                let mut last_sender: Option<(bool, Option<String>)> = None;
                for message in &thread.messages {
                    let day = util::local(message.date).map(|t| t.date_naive());
                    if day != last_day {
                        last_day = day;
                        last_sender = None;
                        ui.add_space(8.0);
                        ui.vertical_centered(|ui| ui.label(kit::subtle(util::long_time(message.date))));
                        ui.add_space(4.0);
                    }
                    let who = (message.is_from_me, message.sender.clone());
                    let first_of_run = last_sender.as_ref() != Some(&who);
                    last_sender = Some(who);
                    if first_of_run {
                        ui.add_space(6.0);
                    }
                    let layout = if message.is_from_me { Layout::right_to_left(Align::TOP) } else { Layout::left_to_right(Align::TOP) };
                    ui.with_layout(layout, |ui| {
                        ui.vertical(|ui| {
                            ui.set_max_width(max_width);
                            ui.spacing_mut().item_spacing.y = 4.0;
                            if group
                                && first_of_run
                                && !message.is_from_me
                                && let Some(sender) = &message.sender
                            {
                                ui.label(kit::subtle(sender));
                            }
                            for attachment in &message.attachments {
                                let is_image = attachment.mime_type.as_deref().is_some_and(|m| m.starts_with("image/"));
                                if is_image {
                                    media::show(ui, cx.images, cx.client, BlobKind::Attachment, &attachment.guid, Vec2::new(240.0, 240.0));
                                } else {
                                    let name = attachment.file_name.clone().unwrap_or_else(|| "attachment".into());
                                    if kit::outline(ui, Some(icon::DOWNLOAD), &format!("{name} · {}", util::bytes(attachment.total_bytes)), true).clicked() {
                                        cx.download(BlobKind::Attachment, attachment.guid.clone(), name);
                                    }
                                }
                            }
                            if let Some(text) = &message.text {
                                // Monochrome bubbles: your iMessages are the inverted pill; SMS
                                // you sent are outlined; received ones sit on `raised`.
                                let pending = message.guid.starts_with("local-");
                                let (fill, color, stroke) = match (message.is_from_me, service) {
                                    (true, Service::IMessage) => (PRIMARY, PRIMARY_FG, Stroke::NONE),
                                    (true, _) => (RAISED, FG, Stroke::new(1.0, BORDER_STRONG)),
                                    (false, _) => (RAISED, FG, Stroke::new(1.0, BORDER)),
                                };
                                egui::Frame::new()
                                    .fill(if pending { fill.gamma_multiply(0.7) } else { fill })
                                    .stroke(stroke)
                                    .corner_radius(CornerRadius::same(18))
                                    .inner_margin(Margin::symmetric(12, 8))
                                    .show(ui, |ui| {
                                        ui.add(egui::Label::new(RichText::new(text).color(color).size(13.5)).wrap().selectable(true));
                                    });
                            }
                        });
                    });
                    ui.add_space(2.0);
                }
                ui.add_space(8.0);
            });
        });

        ui.scope_builder(egui::UiBuilder::new().max_rect(composer_rect), |ui| {
            ui.horizontal(|ui| {
                let hint = if service == Service::IMessage { "iMessage" } else { "Text Message" };
                let edit_width = ui.available_width() - 46.0;
                let edit = egui::Frame::new()
                    .fill(RAISED)
                    .stroke(Stroke::new(1.0, BORDER))
                    .corner_radius(CornerRadius::same(theme::R_PILL))
                    .inner_margin(Margin::symmetric(14, 8))
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.draft)
                                .hint_text(RichText::new(hint).color(FG_SUBTLE))
                                .frame(egui::Frame::NONE)
                                .desired_rows(1)
                                .desired_width(edit_width - 28.0)
                                .return_key(None),
                        )
                    })
                    .inner;
                let enter = edit.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);
                if enter {
                    while self.draft.ends_with('\n') {
                        self.draft.pop();
                    }
                }
                let can_send = !self.draft.trim().is_empty();
                // Send is this surface's one primary; it lights up when there's text.
                let (rect, response) = ui.allocate_exact_size(Vec2::splat(36.0), egui::Sense::click());
                let t = kit::animate(ui, response.id, can_send);
                ui.painter().circle_filled(rect.center(), 18.0, RAISED.lerp_to_gamma(PRIMARY, t));
                ui.painter().text(rect.center(), Align2::CENTER_CENTER, icon::SEND, theme::icons(16.0), FG_MUTED.lerp_to_gamma(PRIMARY_FG, t));
                if (response.clicked() && can_send) || enter {
                    self.send(cx);
                    edit.request_focus();
                }
            });
        });
        ui.allocate_rect(full, egui::Sense::hover());
    }
}
