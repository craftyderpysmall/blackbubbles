//! Small component kit on top of egui, following the Neutral theme rules.

use eframe::egui::{self, Align2, Color32, CornerRadius, Margin, Rect, Response, RichText, Sense, Stroke, Ui};

use crate::theme::{self, BORDER, BORDER_STRONG, FG, FG_MUTED, FG_SUBTLE, PRIMARY, PRIMARY_FG, RAISED, SURFACE};

/// Lucide icon codepoints (bundled font, ISC license). Kept as a complete set.
#[allow(dead_code)]
pub mod icon {
    pub const MESSAGES: char = '\u{e116}';
    pub const PHOTOS: char = '\u{e0f6}';
    pub const NOTES: char = '\u{e596}';
    pub const CALENDAR: char = '\u{e063}';
    pub const CALENDAR_PLUS: char = '\u{e2bc}';
    pub const MEMOS: char = '\u{e55a}';
    pub const FIND_MY: char = '\u{e111}';
    pub const SETTINGS: char = '\u{e154}';
    pub const SEARCH: char = '\u{e151}';
    pub const SEND: char = '\u{e04a}';
    pub const PAPERCLIP: char = '\u{e12d}';
    pub const REFRESH: char = '\u{e145}';
    pub const PLUS: char = '\u{e13d}';
    pub const TRASH: char = '\u{e18e}';
    pub const PLAY: char = '\u{e13c}';
    pub const PAUSE: char = '\u{e12e}';
    pub const STOP: char = '\u{e167}';
    pub const DOWNLOAD: char = '\u{e0b2}';
    pub const HEART: char = '\u{e0f2}';
    pub const LEFT: char = '\u{e06e}';
    pub const RIGHT: char = '\u{e06f}';
    pub const BACK: char = '\u{e048}';
    pub const CLOSE: char = '\u{e1b2}';
    pub const BELL: char = '\u{e059}';
    pub const BELL_OFF: char = '\u{e05a}';
    pub const PHONE: char = '\u{e163}';
    pub const LAPTOP: char = '\u{e1cd}';
    pub const TABLET: char = '\u{e17e}';
    pub const WATCH: char = '\u{e1ad}';
    pub const HEADPHONES: char = '\u{e0f1}';
    pub const BATTERY: char = '\u{e053}';
    pub const MONITOR: char = '\u{e11d}';
    pub const RADAR: char = '\u{e497}';
    pub const VIDEO: char = '\u{e1a5}';
    pub const LOCK: char = '\u{e10b}';
    pub const MAIL: char = '\u{e10f}';
    pub const ALERT: char = '\u{e193}';
    pub const CLOCK: char = '\u{e087}';
    pub const FOLDER: char = '\u{e0d7}';
    pub const LINK: char = '\u{e102}';
    pub const KEYBOARD: char = '\u{e284}';
    pub const MOON: char = '\u{e11e}';
    pub const FILE: char = '\u{e0c0}';
    pub const WIFI_OFF: char = '\u{e1af}';
    pub const CHECK: char = '\u{e06c}';
    pub const PENCIL: char = '\u{e1f9}';
    pub const MAP: char = '\u{e110}';
    pub const INFO: char = '\u{e0f9}';
    pub const SHIELD: char = '\u{e1ff}';
    pub const BLUETOOTH: char = '\u{e05c}';
    pub const MENU: char = '\u{e12a}';
}

/// Decelerating ease (the theme's `decel`).
pub fn decel(t: f32) -> f32 {
    egui::emath::easing::cubic_out(t.clamp(0.0, 1.0))
}

/// 0→1 when `on` becomes true over [`theme::ENTER_SECS`], back over [`theme::EXIT_SECS`].
pub fn animate(ui: &Ui, id: egui::Id, on: bool) -> f32 {
    let secs = if on { theme::ENTER_SECS } else { theme::EXIT_SECS };
    let t = ui.ctx().animate_bool_with_time(id, on, secs);
    if on { decel(t) } else { egui::emath::easing::cubic_in(t) }
}

pub fn card_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(theme::R_CARD))
        .inner_margin(Margin::same(theme::OUTER as i8))
}

/// A surface card filling the available space.
pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    card_frame()
        .show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            add(ui)
        })
        .inner
}

/// A card with no inner padding (for lists that draw edge to edge, maps…).
pub fn bare_card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    card_frame()
        .inner_margin(Margin::same(1))
        .show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            add(ui)
        })
        .inner
}

/// Split the available space into left/right rects with the outer gap between.
pub fn split_h(ui: &mut Ui, left_width: f32) -> (Rect, Rect) {
    let rect = ui.available_rect_before_wrap();
    ui.allocate_rect(rect, Sense::hover());
    let left = Rect::from_min_size(rect.min, egui::vec2(left_width, rect.height()));
    let right = Rect::from_min_max(egui::pos2(left.right() + theme::OUTER, rect.top()), rect.max);
    (left, right)
}

/// Split the available space into top/bottom rects with the outer gap between.
pub fn split_v(ui: &mut Ui, top_height: f32) -> (Rect, Rect) {
    let rect = ui.available_rect_before_wrap();
    ui.allocate_rect(rect, Sense::hover());
    let top = Rect::from_min_size(rect.min, egui::vec2(rect.width(), top_height));
    let bottom = Rect::from_min_max(egui::pos2(rect.left(), top.bottom() + theme::OUTER), rect.max);
    (top, bottom)
}

/// Lay out `add` inside `rect`.
pub fn area<R>(ui: &mut Ui, rect: Rect, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), add).inner
}

/// Paint `glyph` + `text` as one unit (icons must use the icon font explicitly: Inter
/// has its own glyphs in the Private Use Area).
pub fn paint_icon_text(painter: &egui::Painter, anchor: egui::Pos2, align: Align2, glyph: char, text: &str, font: egui::FontId, color: Color32) {
    let text_g = painter.layout_no_wrap(text.to_string(), font.clone(), color);
    let icon_g = painter.layout_no_wrap(glyph.to_string(), theme::icons(font.size), color);
    let gap = if text.is_empty() { 0.0 } else { 4.0 };
    let size = egui::vec2(icon_g.size().x + gap + text_g.size().x, text_g.size().y.max(icon_g.size().y));
    let rect = align.anchor_size(anchor, size);
    let icon_w = icon_g.size().x;
    painter.galley(egui::pos2(rect.left(), rect.center().y - icon_g.size().y / 2.0), icon_g, color);
    painter.galley(egui::pos2(rect.left() + icon_w + gap, rect.center().y - text_g.size().y / 2.0), text_g, color);
}

/// An icon followed by a label, in a row.
pub fn icon_label(ui: &mut Ui, glyph: char, text: RichText, color: Color32) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(icon_text(glyph, 12.0, color));
        ui.label(text);
    });
}

pub fn title(text: impl Into<String>) -> RichText {
    RichText::new(text).font(theme::semibold(15.0)).color(FG)
}

pub fn heading(text: impl Into<String>) -> RichText {
    RichText::new(text).font(theme::semibold(20.0)).color(FG)
}

pub fn muted(text: impl Into<String>) -> RichText {
    RichText::new(text).color(FG_MUTED)
}

pub fn subtle(text: impl Into<String>) -> RichText {
    RichText::new(text).color(FG_SUBTLE).size(11.5)
}

pub fn icon_text(glyph: char, size: f32, color: Color32) -> RichText {
    RichText::new(glyph.to_string()).font(theme::icons(size)).color(color)
}

fn pill_button(ui: &mut Ui, glyph: Option<char>, text: &str, fill: Color32, hover_fill: Color32, fg: Color32, outline: bool, enabled: bool) -> Response {
    let font = theme::medium(13.0);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, fg);
    let icon_w = if glyph.is_some() { 16.0 + if text.is_empty() { 0.0 } else { 6.0 } } else { 0.0 };
    let size = egui::vec2((galley.size().x + icon_w + if text.is_empty() { 16.0 } else { 28.0 }).max(32.0), 32.0);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    if ui.is_rect_visible(rect) {
        let hover = animate(ui, response.id.with("hover"), enabled && response.hovered());
        let fill = fill.lerp_to_gamma(hover_fill, hover);
        let pressed = response.is_pointer_button_down_on();
        let rect = if pressed { rect.shrink(0.5) } else { rect };
        let stroke = if outline { Stroke::new(1.0, if response.hovered() { BORDER_STRONG } else { BORDER }) } else { Stroke::NONE };
        ui.painter().rect(rect, CornerRadius::same(theme::R_PILL), fill, stroke, egui::StrokeKind::Inside);
        let fg = if enabled { fg } else { FG_SUBTLE };
        let content_w = icon_w + galley.size().x;
        let mut x = rect.center().x - content_w / 2.0;
        if let Some(g) = glyph {
            ui.painter().text(egui::pos2(x + 8.0, rect.center().y), Align2::CENTER_CENTER, g, theme::icons(15.0), fg);
            x += icon_w;
        }
        if !text.is_empty() {
            ui.painter().text(egui::pos2(x, rect.center().y), Align2::LEFT_CENTER, text, theme::medium(13.0), fg);
        }
    }
    if enabled { response.on_hover_cursor(egui::CursorIcon::PointingHand) } else { response }
}

/// The one inverted white pill per surface.
pub fn primary(ui: &mut Ui, glyph: Option<char>, text: &str, enabled: bool) -> Response {
    pill_button(ui, glyph, text, PRIMARY, Color32::from_rgb(0xe5, 0xe5, 0xe5), PRIMARY_FG, false, enabled)
}

/// Transparent, muted text, `raised` on hover.
pub fn ghost(ui: &mut Ui, glyph: Option<char>, text: &str) -> Response {
    pill_button(ui, glyph, text, Color32::TRANSPARENT, RAISED, FG_MUTED, false, true)
}

pub fn outline(ui: &mut Ui, glyph: Option<char>, text: &str, enabled: bool) -> Response {
    pill_button(ui, glyph, text, Color32::TRANSPARENT, RAISED, FG, true, enabled)
}

/// Round icon-only ghost button with a tooltip.
pub fn icon_button(ui: &mut Ui, glyph: char, tooltip: &str) -> Response {
    pill_button(ui, Some(glyph), "", Color32::TRANSPARENT, RAISED, FG_MUTED, false, true).on_hover_text(tooltip)
}

/// A raised, rounded search field with a leading icon.
pub fn search(ui: &mut Ui, value: &mut String, hint: &str) -> Response {
    let width = ui.available_width();
    egui::Frame::new()
        .fill(RAISED)
        .corner_radius(CornerRadius::same(theme::R_CONTROL))
        .inner_margin(Margin::symmetric(10, 7))
        .show(ui, |ui| {
            ui.spacing_mut().interact_size.y = 18.0;
            ui.set_width(width - 20.0);
            ui.horizontal(|ui| {
                ui.label(icon_text(icon::SEARCH, 14.0, FG_SUBTLE));
                ui.add(egui::TextEdit::singleline(value).hint_text(RichText::new(hint).color(FG_SUBTLE)).frame(egui::Frame::NONE).desired_width(f32::INFINITY))
            })
            .inner
        })
        .inner
}

/// A rounded text input on `raised`.
pub fn input(ui: &mut Ui, value: &mut String, hint: &str, width: f32, password: bool) -> Response {
    egui::Frame::new()
        .fill(RAISED)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(theme::R_CONTROL))
        .inner_margin(Margin::symmetric(10, 7))
        .show(ui, |ui| {
            ui.spacing_mut().interact_size.y = 18.0;
            ui.add(
                egui::TextEdit::singleline(value)
                    .hint_text(RichText::new(hint).color(FG_SUBTLE))
                    .frame(egui::Frame::NONE)
                    .password(password)
                    .desired_width(width),
            )
        })
        .inner
}

/// A clickable list row (10px radius). Selected rows use `border`, hover uses `raised`.
pub fn row(ui: &mut Ui, selected: bool, height: f32, paint: impl FnOnce(&Ui, Rect)) -> Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), Sense::click());
    if ui.is_rect_visible(rect) {
        let hover = animate(ui, response.id.with("hover"), response.hovered());
        let fill = if selected { BORDER } else { Color32::TRANSPARENT.lerp_to_gamma(RAISED, hover) };
        ui.painter().rect_filled(rect, CornerRadius::same(theme::R_CONTROL), fill);
        paint(ui, rect.shrink2(egui::vec2(12.0, 8.0)));
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Animated pill switch with a label and optional description.
pub fn switch(ui: &mut Ui, on: &mut bool, label: &str, description: &str) -> Response {
    let mut response = ui
        .with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            let (rect, mut response) = ui.allocate_exact_size(egui::vec2(36.0, 20.0), Sense::click());
            if response.clicked() {
                *on = !*on;
                response.mark_changed();
            }
            let t = animate(ui, response.id, *on);
            let track = BORDER_STRONG.lerp_to_gamma(PRIMARY, t);
            ui.painter().rect_filled(rect, CornerRadius::same(theme::R_PILL), track);
            let knob_x = egui::lerp(rect.left() + 10.0..=rect.right() - 10.0, t);
            ui.painter().circle_filled(egui::pos2(knob_x, rect.center().y), 7.0, if *on { PRIMARY_FG } else { FG });
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new(label).color(FG));
                if !description.is_empty() {
                    ui.label(subtle(description));
                }
            });
            response
        })
        .inner;
    response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    response
}

/// Small pill badge; `dot` is a semantic color (or none).
pub fn badge(ui: &mut Ui, text: &str, dot: Option<Color32>) -> Response {
    badge_icon(ui, None, text, dot)
}

/// Badge with a leading icon drawn in the icon font.
pub fn badge_icon(ui: &mut Ui, glyph: Option<char>, text: &str, dot: Option<Color32>) -> Response {
    egui::Frame::new()
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(theme::R_PILL))
        .inner_margin(Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if let Some(color) = dot {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(6.0, 6.0), Sense::hover());
                    ui.painter().circle_filled(r.center(), 3.0, color);
                }
                if let Some(glyph) = glyph {
                    ui.label(icon_text(glyph, 12.0, FG_MUTED));
                }
                ui.label(RichText::new(text).size(11.5).color(FG_MUTED));
            });
        })
        .response
}

/// Centered icon + title + hint, for empty or not-yet-loaded views.
pub fn empty_state(ui: &mut Ui, glyph: char, title_text: &str, hint: &str) {
    ui.vertical_centered(|ui| {
        let top = (ui.available_height() / 2.0 - 60.0).max(0.0);
        ui.add_space(top);
        ui.label(icon_text(glyph, 32.0, FG_SUBTLE));
        ui.add_space(4.0);
        ui.label(title(title_text));
        if !hint.is_empty() {
            ui.label(muted(hint));
        }
    });
}

pub fn spinner_center(ui: &mut Ui) {
    ui.centered_and_justified(|ui| {
        ui.add(egui::Spinner::new().color(FG_SUBTLE).size(20.0));
    });
}

/// A thin horizontal divider in `border`.
pub fn divider(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, BORDER);
}



/// Avatar circle with initials (monochrome).
pub fn avatar(ui: &Ui, center: egui::Pos2, radius: f32, name: &str) {
    let initials: String = name
        .split_whitespace()
        .filter_map(|w| w.chars().find(|c| c.is_alphanumeric()))
        .take(2)
        .collect::<String>()
        .to_uppercase();
    let initials = if initials.is_empty() { "#".into() } else { initials };
    ui.painter().circle(center, radius, RAISED, Stroke::new(1.0, BORDER_STRONG));
    ui.painter().text(center, Align2::CENTER_CENTER, initials, theme::medium(radius * 0.8), FG);
}


