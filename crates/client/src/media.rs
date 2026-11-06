//! Lazily fetched images (thumbnails, photos, attachments) with an LRU bound.
//!
//! Fetches are capped so scrolling a big library doesn't flood the connection; egui's
//! image loader decodes the bytes and caches the texture under the same URI.

use std::collections::{HashMap, VecDeque};

use bb_proto::{ApiError, BlobKind};
use eframe::egui;

use crate::conn::{Client, Pending};

const MAX_ENTRIES: usize = 400;
const MAX_IN_FLIGHT: usize = 24;

type Key = (BlobKind, String);

enum Entry {
    Loading(Pending<Result<(Vec<u8>, Option<String>), ApiError>>),
    Ready(String),
    Failed,
}

#[derive(Default)]
pub struct Images {
    entries: HashMap<Key, Entry>,
    order: VecDeque<Key>,
}

fn extension(mime: Option<&str>, bytes: &[u8]) -> &'static str {
    match mime.unwrap_or_default() {
        m if m.contains("png") => "png",
        m if m.contains("gif") => "gif",
        m if m.contains("webp") => "webp",
        m if m.contains("bmp") => "bmp",
        m if m.contains("jpeg") || m.contains("jpg") => "jpg",
        _ if bytes.starts_with(b"\x89PNG") => "png",
        _ if bytes.starts_with(b"BM") => "bmp",
        _ if bytes.starts_with(b"GIF") => "gif",
        _ => "jpg",
    }
}

impl Images {
    /// The image if it's loaded; otherwise starts loading it (if there's capacity) and
    /// returns `None`.
    pub fn get(&mut self, ctx: &egui::Context, client: &Client, kind: BlobKind, id: &str) -> Option<egui::Image<'static>> {
        let key = (kind, id.to_string());
        let in_flight = self.entries.values().filter(|e| matches!(e, Entry::Loading(_))).count();
        let entry = match self.entries.get_mut(&key) {
            Some(entry) => entry,
            None if in_flight >= MAX_IN_FLIGHT => return None,
            None => {
                self.order.push_back(key.clone());
                self.evict(ctx);
                self.entries.entry(key.clone()).or_insert_with(|| Entry::Loading(client.fetch_blob(kind, id.to_string())))
            }
        };
        if let Entry::Loading(pending) = entry
            && let Some(result) = pending.take()
        {
            *entry = match result {
                Ok((bytes, mime)) => {
                    let uri = format!("bytes://{kind:?}/{}.{}", hex::encode(id.as_bytes()), extension(mime.as_deref(), &bytes));
                    ctx.include_bytes(uri.clone(), bytes);
                    Entry::Ready(uri)
                }
                Err(_) => Entry::Failed,
            };
        }
        match entry {
            Entry::Ready(uri) => Some(egui::Image::new(uri.clone())),
            _ => None,
        }
    }

    pub fn failed(&self, kind: BlobKind, id: &str) -> bool {
        matches!(self.entries.get(&(kind, id.to_string())), Some(Entry::Failed))
    }

    fn evict(&mut self, ctx: &egui::Context) {
        while self.order.len() > MAX_ENTRIES {
            let Some(old) = self.order.pop_front() else { break };
            if let Some(Entry::Ready(uri)) = self.entries.remove(&old) {
                ctx.forget_image(&uri);
            }
        }
    }
}

/// Draw an image (or a placeholder while it loads) fitted into `size`.
pub fn show(ui: &mut egui::Ui, images: &mut Images, client: &Client, kind: BlobKind, id: &str, size: egui::Vec2) -> egui::Response {
    match images.get(ui.ctx(), client, kind, id) {
        Some(image) => ui.add(image.fit_to_exact_size(size).corner_radius(6.0).sense(egui::Sense::click())),
        None => {
            let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
            ui.painter().rect_filled(rect, 6.0, ui.visuals().faint_bg_color);
            let label = if images.failed(kind, id) { "⚠" } else { "…" };
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(16.0), ui.visuals().weak_text_color());
            response
        }
    }
}
