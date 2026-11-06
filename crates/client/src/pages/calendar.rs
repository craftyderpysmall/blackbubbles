use bb_proto::{ApiError, Calendar, CalendarEvent, Event, NewEvent, Request, Response};
use chrono::{Datelike, Days, Local, NaiveDate, NaiveTime, TimeZone};
use eframe::egui::{self, Align, Align2, CornerRadius, Layout, Sense, Stroke, Vec2};

use crate::conn::Pending;
use crate::kit::{self, icon};
use crate::pages::Cx;
use crate::theme::{self, BORDER, FG, FG_MUTED, FG_SUBTLE, PRIMARY, PRIMARY_FG, RAISED};
use crate::util;

type Req = Pending<Result<Response, ApiError>>;

/// Below this width the agenda moves under the month grid.
const SIDE_BY_SIDE: f32 = 980.0;

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
        self.events_req = Some(cx.client.call(Request::ListEvents { from: local_ms(start, NaiveTime::MIN), to: local_ms(start + Days::new(42), NaiveTime::MIN) }));
    }

    pub fn on_event(&mut self, event: &Event, cx: &mut Cx) {
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
        self.calendars.iter().find(|c| c.id == event.calendar_id).map_or(FG_MUTED, |c| theme::accent(c.color))
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

    fn go(&mut self, month: NaiveDate, cx: &Cx) {
        self.month = month;
        self.refresh(cx);
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        if !self.loaded && cx.connected {
            self.refresh(cx);
        }
        self.poll(cx);
        let width = ui.available_width();
        if width >= SIDE_BY_SIDE {
            let (left, right) = kit::split_h(ui, width - 336.0);
            kit::area(ui, left, |ui| kit::card(ui, |ui| self.month_view(ui, cx)));
            kit::area(ui, right, |ui| kit::card(ui, |ui| self.agenda(ui, cx)));
        } else {
            let top = (ui.available_height() * 0.6).max(320.0);
            let (upper, lower) = kit::split_v(ui, top);
            kit::area(ui, upper, |ui| kit::card(ui, |ui| self.month_view(ui, cx)));
            kit::area(ui, lower, |ui| kit::card(ui, |ui| self.agenda(ui, cx)));
        }
        self.draft_modal(ui, cx);
    }

    fn month_view(&mut self, ui: &mut egui::Ui, cx: &Cx) {
        ui.horizontal(|ui| {
            ui.label(kit::heading(self.month.format("%B %Y").to_string()));
            if self.events_req.is_some() {
                ui.add(egui::Spinner::new().size(14.0).color(FG_SUBTLE));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if kit::primary(ui, Some(icon::PLUS), "New event", cx.connected).clicked() {
                    self.new_draft(self.selected);
                }
                if kit::icon_button(ui, icon::RIGHT, "Next month").clicked() {
                    self.go(add_months(self.month, 1), cx);
                }
                if kit::outline(ui, None, "Today", true).clicked() {
                    let today = Local::now().date_naive();
                    self.selected = today;
                    self.go(today.with_day(1).unwrap(), cx);
                }
                if kit::icon_button(ui, icon::LEFT, "Previous month").clicked() {
                    self.go(add_months(self.month, -1), cx);
                }
            });
        });
        ui.add_space(12.0);

        let width = ui.available_width() / 7.0;
        let compact = width < 96.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for name in ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"] {
                ui.add_sized([width, 18.0], egui::Label::new(kit::subtle(name)));
            }
        });
        ui.add_space(4.0);
        ui.spacing_mut().item_spacing.y = 0.0;
        let height = (ui.available_height() / 6.0).floor().max(44.0);
        let today = Local::now().date_naive();
        let start = grid_start(self.month);
        for week in 0..6 {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for d in 0..7 {
                    let day = start + Days::new(week * 7 + d);
                    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
                    let cell = rect.shrink(2.0);
                    let painter = ui.painter_at(rect);
                    let hover = kit::animate(ui, response.id.with("hover"), response.hovered());
                    let fill = if day == self.selected { BORDER } else { egui::Color32::TRANSPARENT.lerp_to_gamma(RAISED, hover) };
                    painter.rect_filled(cell, CornerRadius::same(theme::R_CONTROL), fill);
                    let in_month = day.month() == self.month.month();
                    let number = day.day().to_string();
                    let number_pos = cell.left_top() + egui::vec2(16.0, 14.0);
                    if day == today {
                        // Today is the grid's single inverted pill.
                        painter.circle_filled(number_pos, 11.0, PRIMARY);
                        painter.text(number_pos, Align2::CENTER_CENTER, number, theme::semibold(12.0), PRIMARY_FG);
                    } else {
                        painter.text(number_pos, Align2::CENTER_CENTER, number, theme::medium(12.0), if in_month { FG } else { FG_SUBTLE });
                    }
                    let events = self.events_on(day);
                    if compact {
                        // Dots only on small cells.
                        for (i, event) in events.iter().take(4).enumerate() {
                            painter.circle_filled(egui::pos2(cell.left() + 10.0 + i as f32 * 8.0, cell.bottom() - 9.0), 2.5, self.color_of(event));
                        }
                    } else {
                        let mut y = cell.top() + 30.0;
                        let room = ((cell.bottom() - y - 4.0) / 17.0).floor().max(0.0) as usize;
                        for (i, event) in events.iter().enumerate() {
                            if i >= room {
                                break;
                            }
                            if i + 1 == room && events.len() > room {
                                painter.text(egui::pos2(cell.left() + 8.0, y), Align2::LEFT_TOP, format!("+{} more", events.len() - i), theme::body(11.0), FG_SUBTLE);
                                break;
                            }
                            painter.circle_filled(egui::pos2(cell.left() + 11.0, y + 7.0), 3.0, self.color_of(event));
                            let max = ((cell.width() - 24.0) / 6.0).max(3.0) as usize;
                            let title: String = event.title.chars().take(max).collect();
                            painter.text(egui::pos2(cell.left() + 19.0, y), Align2::LEFT_TOP, title, theme::body(11.5), if in_month { FG_MUTED } else { FG_SUBTLE });
                            y += 17.0;
                        }
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
    }

    fn new_draft(&mut self, day: NaiveDate) {
        self.selected = day;
        let calendar = self.calendars.iter().find(|c| c.writable).map(|c| c.id.clone()).unwrap_or_default();
        self.draft = Some(Draft { title: String::new(), start: "09:00".into(), end: "10:00".into(), all_day: false, location: String::new(), calendar });
    }

    fn agenda(&mut self, ui: &mut egui::Ui, cx: &Cx) {
        ui.label(kit::title(self.selected.format("%A").to_string()));
        ui.label(kit::muted(self.selected.format("%B %-d, %Y").to_string()));
        ui.add_space(12.0);
        let events: Vec<CalendarEvent> = self.events_on(self.selected).into_iter().cloned().collect();
        if events.is_empty() {
            kit::empty_state(ui, icon::CALENDAR, "Nothing scheduled", "Double-click a day to add an event");
            return;
        }
        let mut delete = None;
        egui::ScrollArea::vertical().id_salt("agenda").auto_shrink(false).show(ui, |ui| {
            for event in &events {
                egui::Frame::new()
                    .fill(RAISED)
                    .stroke(Stroke::new(1.0, BORDER))
                    .corner_radius(CornerRadius::same(theme::R_CONTROL))
                    .inner_margin(egui::Margin::same(12))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let (dot, _) = ui.allocate_exact_size(Vec2::new(8.0, 36.0), Sense::hover());
                            ui.painter().circle_filled(egui::pos2(dot.center().x, dot.top() + 8.0), 4.0, self.color_of(event));
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.label(egui::RichText::new(&event.title).font(theme::medium(13.5)).color(FG));
                                let when = if event.all_day {
                                    "All day".to_string()
                                } else {
                                    let fmt = |ms| util::local(ms).map(|t| t.format("%H:%M").to_string()).unwrap_or_default();
                                    format!("{} – {}", fmt(event.start), fmt(event.end))
                                };
                                kit::icon_label(ui, icon::CLOCK, kit::subtle(when), FG_SUBTLE);
                                if let Some(location) = &event.location {
                                    kit::icon_label(ui, icon::FIND_MY, kit::subtle(location), FG_SUBTLE);
                                }
                            });
                            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                                if kit::icon_button(ui, icon::TRASH, "Delete event").clicked() {
                                    delete = Some(event.id.clone());
                                }
                            });
                        });
                    });
                ui.add_space(4.0);
            }
        });
        if let Some(id) = delete {
            self.write_req = Some(cx.client.call(Request::DeleteEvent { id }));
        }
    }

    fn draft_modal(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let Some(draft) = &mut self.draft else { return };
        let mut create = false;
        let mut cancel = false;
        let calendars = self.calendars.clone();
        let selected = self.selected;
        let modal = egui::Modal::new(egui::Id::unique("new_event"))
            .backdrop_color(egui::Color32::from_black_alpha(150))
            .frame(kit::card_frame().shadow(ui.visuals().window_shadow).inner_margin(egui::Margin::same(20)))
            .show(ui.ctx(), |ui| {
                ui.set_width(360.0);
                ui.label(kit::title("New event"));
                ui.label(kit::muted(selected.format("%A, %B %-d").to_string()));
                ui.add_space(12.0);
                kit::input(ui, &mut draft.title, "Title", 336.0, false);
                ui.add_space(4.0);
                kit::input(ui, &mut draft.location, "Location (optional)", 336.0, false);
                ui.add_space(8.0);
                kit::switch(ui, &mut draft.all_day, "All day", "");
                if !draft.all_day {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(kit::muted("From"));
                        kit::input(ui, &mut draft.start, "09:00", 56.0, false);
                        ui.label(kit::muted("to"));
                        kit::input(ui, &mut draft.end, "10:00", 56.0, false);
                    });
                }
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                    for c in calendars.iter().filter(|c| c.writable) {
                        let label = format!("● {}", c.title);
                        let response = if draft.calendar == c.id { kit::outline(ui, None, &label, true) } else { kit::ghost(ui, None, &label) };
                        if response.clicked() {
                            draft.calendar = c.id.clone();
                        }
                    }
                });
                ui.add_space(16.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    create = kit::primary(ui, Some(icon::CALENDAR_PLUS), "Add event", !draft.title.trim().is_empty()).clicked();
                    cancel = kit::ghost(ui, None, "Cancel").clicked();
                });
            });
        if modal.should_close() {
            cancel = true;
        }
        if create {
            let parse = |s: &str| NaiveTime::parse_from_str(s.trim(), "%H:%M").ok();
            let (start, end) = if draft.all_day {
                (local_ms(selected, NaiveTime::MIN), local_ms(selected + Days::new(1), NaiveTime::MIN))
            } else {
                match (parse(&draft.start), parse(&draft.end)) {
                    (Some(s), Some(e)) if e > s => (local_ms(selected, s), local_ms(selected, e)),
                    _ => {
                        cx.toast("Use HH:MM times, with the end after the start");
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
    }
}
