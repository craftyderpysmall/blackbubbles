//! egui's bundled fonts lack many symbols and scripts; append whatever system fonts exist.
//! Color emoji fonts (CBDT/sbix bitmaps) are skipped: egui renders them as nothing.

use std::sync::Arc;

use eframe::egui::{self, FontData, FontDefinitions, FontFamily};

const CANDIDATES: &[&str] = &[
    // Linux
    "/usr/share/fonts/noto/NotoEmoji-Regular.ttf",
    "/usr/share/fonts/noto/NotoSansSymbols2-Regular.ttf",
    "/usr/share/fonts/noto/NotoSansSymbols-Regular.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    // macOS
    "/System/Library/Fonts/Apple Symbols.ttf",
    "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
];

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    for path in CANDIDATES {
        let Ok(bytes) = std::fs::read(path) else { continue };
        let name = format!("system:{path}");
        fonts.font_data.insert(name.clone(), Arc::new(FontData::from_owned(bytes)));
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            fonts.families.entry(family).or_default().push(name.clone());
        }
    }
    ctx.set_fonts(fonts);
}
