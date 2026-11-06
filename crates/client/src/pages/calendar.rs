use bb_proto::{ApiError, Calendar, CalendarEvent, Event, NewEvent, Request, Response};
use chrono::{Datelike, Days, Local, NaiveDate, NaiveTime, TimeZone};
use eframe::egui::{self, RichText, Sense, Vec2};

use crate::conn::Pending;
use crate::pages::Cx;
use crate::util;

type Req = Pending<Result<Response, ApiError>>;

struct Draft {
    title: String,
    start: String,
    end: String,
    all_day: bool,
    location: String,
    calendar: String,
}

pub struct CalendarPage {
    month: NaiveDate,
    selected: NaiveDate,
    calendars: Vec<Calendar>,
    events: Vec<CalendarEvent>,
    calendars_req: Option<Req>,
    events_req: Option<Req>,
    write_req: Option<Req>,
    draft: Option<Draft>,
    loaded: bool,
}

impl Default for CalendarPage {
    fn default() -> Self {
        let today = Local::now().date_naive();
        Self {
            month: today.with_day(1).unwrap(),
            selected: today,
            calendars: vec![],
            events: vec![],
            calendars_req: None,
            events_req: None,
            write_req: None,
            draft: None,
            loaded: false,
        }
    }
}

fn local_ms(date: NaiveDate, time: NaiveTime) -> i64 {
    Local.from_local_datetime(&date.and_time(time)).earliest().map(|t| t.timestamp_millis()).unwrap_or(0)
}

fn grid_start(month: NaiveDate) -> NaiveDate {
    // Weeks start on Monday.
    month - Days::new(month.weekday().num_days_from_monday() as u64)
}

fn add_months(date: NaiveDate, months: i32) -> NaiveDate {
    let total = date.year() * 12 + date.month0() as i32 + months;
    NaiveDate::from_ymd_opt(total.div_euclid(12), total.rem_euclid(12) as u32 + 1, 1).unwrap()
}

impl CalendarPage {
    fn refresh(&mut self, cx: &Cx) {
        self.loaded = true;
        self.calendars_req = Some(cx.client.call(Request::ListCalendars));
        let start = grid_start(self.month);
        let end = start + Days::new(42);
        self.events_req = Some(cx.client.call(Request::ListEvents {
            from: local_ms(start, NaiveTime::MIN),
            to: local_ms(end, NaiveTime::MIN),
        }));
    }

    pub fn on_event(&mut self, event: &Event, cx: &Cx) {
        if matches!(event, Event::CalendarChanged) && self.loaded {
            self.refresh(cx);
        }
    }

    fn events_on(&self, day: NaiveDate) -> Vec<&CalendarEvent> {
        let (start, end) = (local_ms(day, NaiveTime::MIN), local_ms(day + Days::new(1), NaiveTime::MIN));
        let mut list: Vec<_> = self.events.iter().filter(|e| e.start < end && e.end > start).collect();
        list.sort_by_key(|e| (!e.all_day, e.start));
        list
    }

    fn color_of(&self, event: &CalendarEvent) -> egui::Color32 {
        self.calendars.iter().find(|c| c.id == event.calendar_id).map_or(egui::Color32::GRAY, |c| util::color(c.color))
    }

    fn poll(&mut self, cx: &Cx) {
        if let Some(r) = self.calendars_req.as_mut().and_then(Pending::take) {
            self.calendars_req = None;
            match r {
                Ok(Response::Calendars(c)) => self.calendars = c,
                Err(e) => cx.toast(format!("Calendar: {}", e.message)),
                _ => {}
            }
        }
        if let Some(r) = self.events_req.as_mut().and_then(Pending::take) {
            self.events_req = None;
            if let Ok(Response::Events(e)) = r {
                self.events = e;
            }
        }
        if let Some(r) = self.write_req.as_mut().and_then(Pending::take) {
            self.write_req = None;
            match r {
                Ok(_) => self.refresh(cx),
                Err(e) => cx.toast(format!("Calendar: {}", e.message)),
            }
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        if !self.loaded && cx.connected {
            self.refresh(cx);
        }
        self.poll(cx);

        egui::Panel::right("agenda").resizable(true).default_size(300.0).show(ui, |ui| self.agenda(ui, cx));

        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button("◀").clicked() {
                    self.month = add_months(self.month, -1);
                    self.refresh(cx);
                }
                if ui.button("Today").clicked() {
                    let today = Local::now().date_naive();
                    self.month = today.with_day(1).unwrap();
                    self.selected = today;
                    self.refresh(cx);
                }
                if ui.button("▶").clicked() {
                    self.month = add_months(self.month, 1);
                    self.refresh(cx);
                }
                ui.heading(self.month.format("%B %Y").to_string());
                if self.events_req.is_some() {
                    ui.spinner();
                }
            });
            ui.add_space(6.0);
            let width = ui.available_width() / 7.0;
            let height = ((ui.available_height() - 24.0) / 6.0).max(60.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for name in ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"] {
                    ui.add_sized([width, 18.0], egui::Label::new(RichText::new(name).weak()));
                }
            });
            let today = Local::now().date_naive();
            let start = grid_start(self.month);
            for week in 0..6 {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    for d in 0..7 {
                        let day = start + Days::new(week * 7 + d);
                        let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
                        let painter = ui.painter_at(rect);
                        let visuals = ui.visuals();
                        if day == self.selected {
                            painter.rect_filled(rect.shrink(1.0), 6.0, visuals.selection.bg_fill.gamma_multiply(0.35));
                        }
                        painter.rect_stroke(rect, 0.0, visuals.widgets.noninteractive.bg_stroke, egui::StrokeKind::Inside);
                        let in_month = day.month() == self.month.month();
                        let number_color = if day == today { egui::Color32::from_rgb(255, 69, 58) } else if in_month { visuals.text_color() } else { visuals.weak_text_color() };
                        painter.text(rect.left_top() + egui::vec2(6.0, 4.0), egui::Align2::LEFT_TOP, day.day().to_string(), egui::FontId::proportional(13.0), number_color);
                        let mut y = rect.top() + 22.0;
                        let events = self.events_on(day);
                        let room = ((height - 24.0) / 16.0).floor() as usize;
                        for (i, event) in events.iter().enumerate() {
                            if i + 1 == room && events.len() > room {
                                painter.text(egui::pos2(rect.left() + 6.0, y), egui::Align2::LEFT_TOP, format!("+{} more", events.len() - i), egui::FontId::proportional(11.0), visuals.weak_text_color());
                                break;
                            }
                            if i >= room {
                                break;
                            }
                            let dot = egui::Rect::from_min_size(egui::pos2(rect.left() + 6.0, y + 3.0), Vec2::new(4.0, 9.0));
                            painter.rect_filled(dot, 2.0, self.color_of(event));
                            let title: String = event.title.chars().take(((width - 20.0) / 6.0) as usize).collect();
                            painter.text(egui::pos2(rect.left() + 14.0, y), egui::Align2::LEFT_TOP, title, egui::FontId::proportional(11.5), visuals.text_color());
                            y += 16.0;
                        }
                        if response.clicked() {
                            self.selected = day;
                        }
                        if response.double_clicked() {
                            self.new_draft(day);
                        }
                    }
                });
            }
        });
    }

    fn new_draft(&mut self, day: NaiveDate) {
        self.selected = day;
        let calendar = self.calendars.iter().find(|c| c.writable).map(|c| c.id.clone()).unwrap_or_default();
        self.draft = Some(Draft { title: String::new(), start: "09:00".into(), end: "10:00".into(), all_day: false, location: String::new(), calendar });
    }

    fn agenda(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.heading(self.selected.format("%A, %b %-d").to_string());
            if ui.button("New event").clicked() {
                self.new_draft(self.selected);
            }
        });
        ui.separator();
        if let Some(draft) = &mut self.draft {
            let mut create = false;
            let mut cancel = false;
            egui::Grid::new("new_event").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                ui.label("Title");
                ui.text_edit_singleline(&mut draft.title);
                ui.end_row();
                ui.label("All day");
                ui.checkbox(&mut draft.all_day, "");
                ui.end_row();
                if !draft.all_day {
                    ui.label("Starts");
                    ui.add(egui::TextEdit::singleline(&mut draft.start).desired_width(60.0));
                    ui.end_row();
                    ui.label("Ends");
                    ui.add(egui::TextEdit::singleline(&mut draft.end).desired_width(60.0));
                    ui.end_row();
                }
                ui.label("Location");
                ui.text_edit_singleline(&mut draft.location);
                ui.end_row();
                ui.label("Calendar");
                let name = self.calendars.iter().find(|c| c.id == draft.calendar).map(|c| c.title.clone()).unwrap_or_default();
                egui::ComboBox::from_id_salt("draft_cal").selected_text(name).show_ui(ui, |ui| {
                    for c in self.calendars.iter().filter(|c| c.writable) {
                        ui.selectable_value(&mut draft.calendar, c.id.clone(), &c.title);
                    }
                });
                ui.end_row();
            });
            ui.horizontal(|ui| {
                create = ui.add_enabled(!draft.title.trim().is_empty(), egui::Button::new("Add")).clicked();
                cancel = ui.button("Cancel").clicked();
            });
            if create {
                let parse = |s: &str| NaiveTime::parse_from_str(s.trim(), "%H:%M").ok();
                let day = self.selected;
                let (start, end) = if draft.all_day {
                    (local_ms(day, NaiveTime::MIN), local_ms(day + Days::new(1), NaiveTime::MIN))
                } else {
                    match (parse(&draft.start), parse(&draft.end)) {
                        (Some(s), Some(e)) if e > s => (local_ms(day, s), local_ms(day, e)),
                        _ => {
                            cx.toast("Times must be HH:MM, end after start");
                            return;
                        }
                    }
                };
                self.write_req = Some(cx.client.call(Request::CreateEvent(NewEvent {
                    calendar_id: draft.calendar.clone(),
                    title: draft.title.trim().to_string(),
                    location: Some(draft.location.trim().to_string()).filter(|s| !s.is_empty()),
                    notes: None,
                    start,
                    end,
                    all_day: draft.all_day,
                })));
                self.draft = None;
            } else if cancel {
                self.draft = None;
            }
            ui.separator();
        }
        let events: Vec<CalendarEvent> = self.events_on(self.selected).into_iter().cloned().collect();
        if events.is_empty() {
            ui.label(RichText::new("No events").weak());
        }
        let mut delete = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for event in &events {
                ui.horizontal(|ui| {
                    let (dot, _) = ui.allocate_exact_size(Vec2::new(6.0, 30.0), Sense::hover());
                    ui.painter().rect_filled(dot, 3.0, self.color_of(event));
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&event.title).strong());
                        let when = if event.all_day {
                            "All day".to_string()
                        } else {
                            let fmt = |ms| util::local(ms).map(|t| t.format("%H:%M").to_string()).unwrap_or_default();
                            format!("{} – {}", fmt(event.start), fmt(event.end))
                        };
                        ui.label(RichText::new(when).weak().small());
                        if let Some(location) = &event.location {
                            ui.label(RichText::new(location.as_str()).weak().small());
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Delete").clicked() {
                            delete = Some(event.id.clone());
                        }
                    });
                });
                ui.separator();
            }
        });
        if let Some(id) = delete {
            self.write_req = Some(cx.client.call(Request::DeleteEvent { id }));
        }
    }
}
