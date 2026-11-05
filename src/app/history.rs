//! History side panel: lazily listed documents with lazily loaded
//! thumbnails. Full-resolution images are never loaded here.

use super::icons::{self, Icon};
use super::theme::{ACCENT, Palette};
use super::toolbar::icon_button;
use super::{Bg, MarklyApp};
use crate::model::now_ms;
use crate::store::HistoryEntry;
use eframe::egui::{
    self, Color32, ColorImage, CornerRadius, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, vec2,
};
use std::collections::HashMap;

#[derive(Default)]
struct Thumb {
    texture: Option<TextureHandle>,
    loading: bool,
    stale: bool,
    missing: bool,
}

#[derive(Default)]
pub(super) struct HistoryState {
    entries: Option<Vec<HistoryEntry>>,
    listing: bool,
    stale: bool,
    thumbs: HashMap<String, Thumb>,
    pub confirm_clear: bool,
}

impl HistoryState {
    pub fn mark_stale(&mut self) {
        self.stale = true;
    }

    pub fn invalidate_thumb(&mut self, id: &str) {
        if let Some(t) = self.thumbs.get_mut(id) {
            t.stale = true;
        }
    }

    pub fn set_entries(&mut self, entries: Vec<HistoryEntry>) {
        let live: std::collections::HashSet<&str> = entries.iter().map(|e| e.id.as_str()).collect();
        self.thumbs.retain(|id, _| live.contains(id.as_str()));
        self.entries = Some(entries);
        self.listing = false;
    }

    pub fn set_thumb(&mut self, ctx: &egui::Context, id: String, image: Option<ColorImage>) {
        let t = self.thumbs.entry(id.clone()).or_default();
        t.loading = false;
        match image {
            Some(img) => {
                t.missing = false;
                t.texture = Some(ctx.load_texture(format!("thumb-{id}"), img, TextureOptions::LINEAR));
            }
            None => t.missing = t.texture.is_none(),
        }
    }

    /// Keep the open document's row current without re-reading every file.
    fn update_entry(&mut self, id: &str, modified_ms: u64, annotations: usize) {
        if let Some(e) = self
            .entries
            .as_mut()
            .and_then(|v| v.iter_mut().find(|e| e.id == id))
        {
            e.modified_ms = modified_ms;
            e.annotations = annotations;
        }
    }
}

/// "just now", "5 min ago", "yesterday", or a UTC date.
fn relative_time(ms: u64) -> String {
    let now = now_ms();
    let secs = now.saturating_sub(ms) / 1000;
    match secs {
        0..=59 => "Just now".into(),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=86_399 => format!("{} h ago", secs / 3600),
        86_400..=172_799 => "Yesterday".into(),
        172_800..=604_799 => format!("{} days ago", secs / 86_400),
        _ => {
            let (y, m, d) = civil_from_days((ms / 86_400_000) as i64);
            format!("{y}-{m:02}-{d:02}")
        }
    }
}

/// Days since 1970-01-01 → (year, month, day). Howard Hinnant's algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

impl MarklyApp {
    pub(super) fn history_panel(&mut self, ui: &mut egui::Ui, pal: &Palette) {
        let Some(store) = self.store.clone() else {
            ui.label("History is unavailable (no storage directory).");
            return;
        };
        if let Some(od) = &self.doc {
            self.history
                .update_entry(&od.doc.id, od.doc.modified_ms, od.doc.annotations.len());
        }
        if (self.history.entries.is_none() || self.history.stale) && !self.history.listing {
            self.history.listing = true;
            self.history.stale = false;
            let tx = self.bg_tx.clone();
            let ctx = self.ctx.clone();
            let s = store.clone();
            std::thread::spawn(move || {
                let _ = tx.send(Bg::HistoryListed(s.list_documents()));
                ctx.request_repaint();
            });
        }

        let count = self.history.entries.as_ref().map_or(0, Vec::len);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("History").size(15.0).strong());
            ui.label(
                egui::RichText::new(format!("{count}"))
                    .size(12.5)
                    .color(pal.text_muted),
            );
        });
        ui.add_space(4.0);

        let Some(entries) = self.history.entries.clone() else {
            ui.label(egui::RichText::new("Loading…").color(pal.text_muted));
            return;
        };
        if entries.is_empty() {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new("Pasted and opened images\nappear here automatically.")
                        .color(pal.text_muted),
                );
            });
            return;
        }

        let current = self.doc.as_ref().map(|d| d.doc.id.clone());
        let mut open: Option<String> = None;
        let mut delete: Option<String> = None;
        let mut want_thumbs = Vec::new();
        let footer_h = 40.0;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .max_height((ui.available_height() - footer_h).max(60.0))
            .show(ui, |ui| {
                for e in &entries {
                    let width = ui.available_width();
                    let thumb_h = 120.0;
                    let (rect, resp) = ui.allocate_exact_size(vec2(width, thumb_h + 40.0), Sense::click());
                    if !ui.is_rect_visible(rect) {
                        continue;
                    }
                    let is_current = current.as_deref() == Some(e.id.as_str());
                    let p = ui.painter();
                    p.rect_filled(
                        rect,
                        CornerRadius::same(10),
                        if resp.hovered() { pal.hover } else { pal.card },
                    );
                    if is_current {
                        p.rect_stroke(
                            rect,
                            CornerRadius::same(10),
                            Stroke::new(2.0, ACCENT),
                            StrokeKind::Inside,
                        );
                    }
                    let thumb_area = egui::Rect::from_min_size(
                        rect.min + vec2(8.0, 8.0),
                        vec2(width - 16.0, thumb_h - 8.0),
                    );
                    let t = self.history.thumbs.entry(e.id.clone()).or_default();
                    if (t.texture.is_none() && !t.missing || t.stale) && !t.loading {
                        t.loading = true;
                        t.stale = false;
                        want_thumbs.push(e.id.clone());
                    }
                    if let Some(tex) = &t.texture {
                        let [tw, th] = tex.size();
                        let s = (thumb_area.width() / tw as f32).min(thumb_area.height() / th as f32);
                        let r = egui::Rect::from_center_size(
                            thumb_area.center(),
                            vec2(tw as f32 * s, th as f32 * s),
                        );
                        p.image(
                            tex.id(),
                            r,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    } else {
                        icons::paint(
                            p,
                            egui::Rect::from_center_size(thumb_area.center(), vec2(28.0, 28.0)),
                            Icon::Image,
                            pal.icon_muted,
                        );
                    }
                    let title = e.source_name.clone().unwrap_or_else(|| "Pasted image".to_owned());
                    p.text(
                        rect.left_bottom() + vec2(10.0, -26.0),
                        egui::Align2::LEFT_CENTER,
                        title,
                        egui::FontId::proportional(13.0),
                        ui.visuals().text_color(),
                    );
                    let anns = match e.annotations {
                        0 => String::new(),
                        1 => " · 1 mark".into(),
                        n => format!(" · {n} marks"),
                    };
                    p.text(
                        rect.left_bottom() + vec2(10.0, -11.0),
                        egui::Align2::LEFT_CENTER,
                        format!(
                            "{} × {}{} · {}",
                            e.width,
                            e.height,
                            anns,
                            relative_time(e.modified_ms)
                        ),
                        egui::FontId::proportional(11.5),
                        pal.text_muted,
                    );
                    // Delete button, shown on hover.
                    let del_rect =
                        egui::Rect::from_min_size(rect.right_top() + vec2(-36.0, 10.0), vec2(26.0, 26.0));
                    let mut over_delete = false;
                    if resp.hovered() || ui.rect_contains_pointer(del_rect) {
                        let del = ui.interact(del_rect, ui.scope_id().with(("del", &e.id)), Sense::click());
                        over_delete = del.hovered();
                        let bg = if del.hovered() {
                            Color32::from_rgb(200, 50, 50)
                        } else {
                            Color32::from_black_alpha(140)
                        };
                        ui.painter().rect_filled(del_rect, CornerRadius::same(6), bg);
                        icons::paint(ui.painter(), del_rect.shrink(5.0), Icon::Trash, Color32::WHITE);
                        if del.on_hover_text("Delete from history").clicked() {
                            delete = Some(e.id.clone());
                        }
                    }
                    if resp.clicked() && !over_delete {
                        open = Some(e.id.clone());
                    }
                    ui.add_space(4.0);
                }
            });

        if !want_thumbs.is_empty() {
            let tx = self.bg_tx.clone();
            let ctx = self.ctx.clone();
            std::thread::spawn(move || {
                for id in want_thumbs {
                    let image = std::fs::read(store.thumbnail_path(&id))
                        .ok()
                        .and_then(|b| image::load_from_memory_with_format(&b, image::ImageFormat::Png).ok())
                        .map(|img| {
                            let img = img.into_rgba8();
                            ColorImage::from_rgba_unmultiplied(
                                [img.width() as usize, img.height() as usize],
                                img.as_raw(),
                            )
                        });
                    let _ = tx.send(Bg::Thumbnail { id, image });
                    ctx.request_repaint();
                }
            });
        }

        // Footer: clear all, with inline confirmation.
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if self.history.confirm_clear {
                ui.label(egui::RichText::new(format!("Delete all {count}?")).color(pal.text_muted));
                let del = egui::Button::new(egui::RichText::new("Delete").color(Color32::WHITE))
                    .fill(Color32::from_rgb(200, 50, 50));
                if ui.add(del).clicked() {
                    self.history.confirm_clear = false;
                    if let Some(w) = &self.writer {
                        w.delete_all(None);
                    }
                    self.history.entries = Some(Vec::new());
                    self.history.thumbs.clear();
                    self.close_document();
                }
                if ui.button("Cancel").clicked() {
                    self.history.confirm_clear = false;
                }
            } else {
                if icon_button(ui, Icon::Trash, "Clear all history…", true, false, pal, 28.0).clicked() {
                    self.history.confirm_clear = true;
                }
                ui.label(
                    egui::RichText::new("Stored only on this computer")
                        .size(11.5)
                        .color(pal.text_muted),
                );
            }
        });

        if let Some(id) = delete {
            if let Some(w) = &self.writer {
                w.delete(&id);
            }
            if let Some(v) = &mut self.history.entries {
                v.retain(|e| e.id != id);
            }
            self.history.thumbs.remove(&id);
            if current.as_deref() == Some(id.as_str()) {
                self.close_document();
            }
        } else if let Some(id) = open {
            self.open_stored(&id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::civil_from_days;

    #[test]
    fn civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_762), (2026, 11, 5));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
    }
}
