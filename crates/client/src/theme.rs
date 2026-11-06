//! The "Neutral" theme from the user's desktop (~/.config/hypr/hyprland.lua):
//! monochrome dark, shadcn/ui-inspired.
//!
//! - Layering is lightness: bg < surface < raised. Never darker on top of lighter.
//! - Every card has a 1px `BORDER`; focus/active steps up to `BORDER_STRONG`.
//! - Radii: cards 16, inner controls 10, buttons/badges fully rounded.
//! - One primary (inverted white pill) per surface; everything else is ghost.
//! - Color is information only (semantic dots/badges), never decoration.
//! - 4px grid: 8 inner gap, 16 outer gap / card padding.
//! - Motion: enter decelerating (~250-350 ms), exit accelerating at about half that.

use std::sync::Arc;

use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle};

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

// Surfaces
pub const BG: Color32 = hex(0x0a0a0a);
pub const SURFACE: Color32 = hex(0x171717);
pub const RAISED: Color32 = hex(0x212121);
pub const BORDER: Color32 = hex(0x262626);
pub const BORDER_STRONG: Color32 = hex(0x404040);
// Text
pub const FG: Color32 = hex(0xfafafa);
pub const FG_MUTED: Color32 = hex(0xa1a1a1);
pub const FG_SUBTLE: Color32 = hex(0x737373);
// Primary (inverted pill)
pub const PRIMARY: Color32 = FG;
pub const PRIMARY_FG: Color32 = hex(0x171717);
// Data scale, light → dark (for charts)
#[allow(dead_code)]
pub const SCALE: [Color32; 5] = [hex(0xe5e5e5), hex(0xa3a3a3), hex(0x737373), hex(0x525252), hex(0x404040)];
// Semantic (dots, badges; never large fills)
pub const DANGER: Color32 = hex(0xe5787a);
pub const SUCCESS: Color32 = hex(0x8ccf9a);
pub const WARNING: Color32 = hex(0xe6c879);
pub const INFO: Color32 = hex(0x82a9e0);
pub const MAGENTA: Color32 = hex(0xb49ae6);
pub const CYAN: Color32 = hex(0x7cc6cf);

pub const R_CARD: u8 = 16;
pub const R_CONTROL: u8 = 10;
pub const R_PILL: u8 = 255;
pub const GAP: f32 = 8.0;
pub const OUTER: f32 = 16.0;

/// Enter: 300 ms decelerating. Exit: ~150 ms accelerating.
pub const ENTER_SECS: f32 = 0.3;
pub const EXIT_SECS: f32 = 0.15;

pub const ICONS: &str = "lucide";
pub const MEDIUM: &str = "inter-medium";
pub const SEMIBOLD: &str = "inter-semibold";

pub fn icons(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(ICONS.into()))
}

pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(MEDIUM.into()))
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

pub fn body(size: f32) -> FontId {
    FontId::proportional(size)
}

/// Fonts that may exist on the system and cover scripts/symbols Inter doesn't.
const FALLBACKS: &[&str] = &[
    "/usr/share/fonts/noto/NotoSansSymbols2-Regular.ttf",
    "/usr/share/fonts/noto/NotoSansSymbols-Regular.ttf",
    "/usr/share/fonts/noto/NotoEmoji-Regular.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/System/Library/Fonts/Apple Symbols.ttf",
    "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
];

const MONO: &[&str] = &[
    "/usr/share/fonts/TTF/JetBrainsMonoNerdFont-Regular.ttf",
    "/usr/share/fonts/TTF/JetBrainsMono-Regular.ttf",
    "/Library/Fonts/JetBrainsMono-Regular.ttf",
];

fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let builtin_mono = fonts.families.get(&FontFamily::Monospace).cloned().unwrap_or_default();
    let mut add = |name: &str, bytes: Vec<u8>| {
        fonts.font_data.insert(name.into(), Arc::new(FontData::from_owned(bytes)));
    };
    add("inter", include_bytes!("../assets/fonts/Inter-Regular.ttf").to_vec());
    add(MEDIUM, include_bytes!("../assets/fonts/Inter-Medium.ttf").to_vec());
    add(SEMIBOLD, include_bytes!("../assets/fonts/Inter-SemiBold.ttf").to_vec());
    add(ICONS, include_bytes!("../assets/fonts/lucide.ttf").to_vec());
    let fallbacks: Vec<String> = FALLBACKS
        .iter()
        .filter_map(|path| std::fs::read(path).ok().map(|b| (format!("sys:{path}"), b)))
        .map(|(name, bytes)| {
            add(&name, bytes);
            name
        })
        .collect();
    let mono: Option<String> = MONO.iter().find_map(|path| std::fs::read(path).ok().map(|b| (format!("sys:{path}"), b))).map(|(name, bytes)| {
        add(&name, bytes);
        name
    });

    // Lucide is only reachable through its own family: Inter defines glyphs in the
    // Private Use Area too, so a fallback chain would never get to it.
    let chain = |first: &str| -> Vec<String> {
        let mut list = vec![first.to_string()];
        list.extend(fallbacks.iter().cloned());
        list
    };
    fonts.families.insert(FontFamily::Proportional, chain("inter"));
    fonts.families.insert(FontFamily::Name(MEDIUM.into()), chain(MEDIUM));
    fonts.families.insert(FontFamily::Name(SEMIBOLD.into()), chain(SEMIBOLD));
    fonts.families.insert(FontFamily::Name(ICONS.into()), vec![ICONS.to_string()]);
    let mut mono_chain: Vec<String> = mono.into_iter().collect();
    mono_chain.extend(builtin_mono);
    fonts.families.insert(FontFamily::Monospace, mono_chain);
    fonts
}

pub fn apply(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    ctx.set_theme(egui::Theme::Dark);
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        style.text_styles = [
            (TextStyle::Small, body(11.5)),
            (TextStyle::Body, body(13.5)),
            (TextStyle::Button, medium(13.0)),
            (TextStyle::Heading, semibold(17.0)),
            (TextStyle::Monospace, FontId::monospace(12.5)),
        ]
        .into();
        style.animation_time = 0.18;
        let spacing = &mut style.spacing;
        spacing.item_spacing = egui::vec2(GAP, GAP);
        spacing.button_padding = egui::vec2(12.0, 6.0);
        spacing.interact_size.y = 32.0;
        spacing.window_margin = Margin::same(OUTER as i8);
        spacing.menu_margin = Margin::same(6);
        spacing.text_edit_width = 240.0;
        spacing.scroll = egui::style::ScrollStyle {
            bar_width: 6.0,
            floating: true,
            ..egui::style::ScrollStyle::floating()
        };

        let v = &mut style.visuals;
        *v = egui::Visuals::dark();
        v.dark_mode = true;
        v.override_text_color = None;
        v.panel_fill = BG;
        v.window_fill = SURFACE;
        v.extreme_bg_color = RAISED; // text edits
        v.faint_bg_color = RAISED;
        v.code_bg_color = RAISED;
        v.window_stroke = Stroke::new(1.0, BORDER);
        v.window_corner_radius = CornerRadius::same(R_CARD);
        v.menu_corner_radius = CornerRadius::same(R_CONTROL);
        // Depth from soft, large, offset-down black shadows — not glow.
        v.window_shadow = Shadow { offset: [0, 12], blur: 40, spread: 0, color: Color32::from_black_alpha(140) };
        v.popup_shadow = Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(120) };
        v.hyperlink_color = FG;
        v.warn_fg_color = WARNING;
        v.error_fg_color = DANGER;
        v.selection.bg_fill = BORDER_STRONG;
        v.selection.stroke = Stroke::new(1.0, FG);
        v.text_cursor.stroke = Stroke::new(2.0, FG);
        v.striped = false;
        v.slider_trailing_fill = true;
        v.handle_shape = egui::style::HandleShape::Circle;

        let w = &mut v.widgets;
        let control = CornerRadius::same(R_CONTROL);
        // Labels, separators.
        w.noninteractive.bg_fill = SURFACE;
        w.noninteractive.weak_bg_fill = SURFACE;
        w.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        w.noninteractive.fg_stroke = Stroke::new(1.0, FG_MUTED);
        w.noninteractive.corner_radius = control;
        // Ghost by default: transparent, muted text, `raised` on hover.
        w.inactive.bg_fill = RAISED;
        w.inactive.weak_bg_fill = Color32::TRANSPARENT;
        w.inactive.bg_stroke = Stroke::NONE;
        w.inactive.fg_stroke = Stroke::new(1.0, FG_MUTED);
        w.inactive.corner_radius = control;
        w.inactive.expansion = 0.0;
        w.hovered.bg_fill = RAISED;
        w.hovered.weak_bg_fill = RAISED;
        w.hovered.bg_stroke = Stroke::new(1.0, BORDER);
        w.hovered.fg_stroke = Stroke::new(1.5, FG);
        w.hovered.corner_radius = control;
        w.hovered.expansion = 0.0;
        w.active.bg_fill = BORDER_STRONG;
        w.active.weak_bg_fill = BORDER;
        w.active.bg_stroke = Stroke::new(1.0, BORDER_STRONG);
        w.active.fg_stroke = Stroke::new(1.5, FG);
        w.active.corner_radius = control;
        w.active.expansion = 0.0;
        w.open = w.hovered;
    });
}

/// Stable, low-saturation color for a calendar/tag, from the data scale + semantic set.
pub fn accent(rgb: u32) -> Color32 {
    // Calendars carry their own colors; map them onto the desaturated semantic palette
    // so the UI stays monochrome-first.
    let palette = [INFO, SUCCESS, WARNING, DANGER, MAGENTA, CYAN];
    let (r, g, b) = ((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8);
    let hue = egui::ecolor::Hsva::from(Color32::from_rgb(r, g, b)).h;
    // Nearest by hue.
    palette
        .iter()
        .min_by(|a, b| {
            let d = |c: &&Color32| {
                let h = egui::ecolor::Hsva::from(**c).h;
                let diff = (h - hue).abs();
                diff.min(1.0 - diff)
            };
            d(a).total_cmp(&d(b))
        })
        .copied()
        .unwrap_or(FG_MUTED)
}
