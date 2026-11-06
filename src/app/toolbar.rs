//! Top action bar and the left tool/style rail.

use super::icons::{self, Icon};
use super::theme::{ACCENT, Palette};
use super::{MarklyApp, cmd, presets_for, rgba_to_color32};
use crate::model::PALETTE;
use crate::settings::{ThemePref, Tool};
use eframe::egui::{
    self, Align, Color32, CornerRadius, Layout, Response, Sense, Stroke, StrokeKind, Ui, vec2,
};

pub(super) fn icon_button(
    ui: &mut Ui,
    icon: Icon,
    tip: &str,
    enabled: bool,
    active: bool,
    pal: &Palette,
    size: f32,
) -> Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), sense);
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let bg = if active {
            ACCENT
        } else if enabled && resp.is_pointer_button_down_on() {
            pal.separator
        } else if enabled && resp.hovered() {
            pal.hover
        } else {
            Color32::TRANSPARENT
        };
        p.rect_filled(rect, CornerRadius::same(8), bg);
        let color = if active {
            Color32::WHITE
        } else if enabled {
            pal.icon
        } else {
            pal.icon_muted
        };
        icons::paint(p, rect.shrink(size * 0.2), icon, color);
    }
    if enabled {
        resp.on_hover_text(tip)
    } else {
        resp.on_hover_text(format!("{tip} — unavailable"))
    }
}

fn separator_v(ui: &mut Ui, pal: &Palette) {
    let (rect, _) = ui.allocate_exact_size(vec2(9.0, 22.0), Sense::hover());
    ui.painter()
        .vline(rect.center().x, rect.y_range(), Stroke::new(1.0, pal.separator));
}

fn separator_h(ui: &mut Ui, pal: &Palette) {
    let (rect, _) = ui.allocate_exact_size(vec2(36.0, 11.0), Sense::hover());
    ui.painter()
        .hline(rect.x_range(), rect.center().y, Stroke::new(1.0, pal.separator));
}

impl MarklyApp {
    pub(super) fn top_bar(&mut self, ui: &mut Ui, pal: &Palette) {
        let has_doc = self.doc.is_some();
        let (can_undo, can_redo) = self
            .doc
            .as_ref()
            .map_or((false, false), |d| (d.undo.can_undo(), d.undo.can_redo()));
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if icon_button(
                ui,
                Icon::Open,
                &format!("Open image ({}O)", cmd()),
                true,
                false,
                pal,
                32.0,
            )
            .clicked()
            {
                self.pick_open_file();
            }
            if icon_button(
                ui,
                Icon::Paste,
                &format!("Paste image ({}V)", cmd()),
                true,
                false,
                pal,
                32.0,
            )
            .clicked()
            {
                self.paste_requested();
            }
            separator_v(ui, pal);
            if icon_button(
                ui,
                Icon::Undo,
                &format!("Undo ({}Z)", cmd()),
                can_undo,
                false,
                pal,
                32.0,
            )
            .clicked()
            {
                self.undo();
            }
            let redo_tip = format!("Redo ({}Shift+Z)", cmd());
            if icon_button(ui, Icon::Redo, &redo_tip, can_redo, false, pal, 32.0).clicked() {
                self.redo();
            }
            if has_doc {
                separator_v(ui, pal);
                if icon_button(
                    ui,
                    Icon::ZoomOut,
                    &format!("Zoom out ({}−)", cmd()),
                    true,
                    false,
                    pal,
                    32.0,
                )
                .clicked()
                {
                    self.zoom_step(0.8);
                }
                let pct = format!("{:.0}%", self.view.zoom_percent());
                let zr = ui
                    .add_sized(
                        [52.0, 28.0],
                        egui::Button::new(egui::RichText::new(pct).size(13.0)).frame(false),
                    )
                    .on_hover_text(format!("Actual size ({}1)", cmd()));
                if zr.clicked() {
                    self.zoom_actual_size();
                }
                if icon_button(
                    ui,
                    Icon::ZoomIn,
                    &format!("Zoom in ({}+)", cmd()),
                    true,
                    false,
                    pal,
                    32.0,
                )
                .clicked()
                {
                    self.zoom_step(1.25);
                }
                if icon_button(
                    ui,
                    Icon::Fit,
                    &format!("Fit to window ({}0)", cmd()),
                    true,
                    self.fit_mode,
                    pal,
                    32.0,
                )
                .clicked()
                {
                    self.fit_mode = true;
                }
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if self.copy_button(ui, has_doc).clicked() {
                    self.copy_to_clipboard();
                }
                let export_tip = format!("Export image… ({}Shift+S)", cmd());
                if icon_button(ui, Icon::Export, &export_tip, has_doc, false, pal, 32.0).clicked() {
                    self.pick_export_file();
                }
                if icon_button(
                    ui,
                    Icon::History,
                    "History (H)",
                    true,
                    self.settings.history_open,
                    pal,
                    32.0,
                )
                .clicked()
                {
                    self.toggle_history();
                }
                let (theme_icon, theme_tip, next) = match self.settings.theme {
                    ThemePref::System => (Icon::ThemeSystem, "Theme: system", ThemePref::Light),
                    ThemePref::Light => (Icon::ThemeLight, "Theme: light", ThemePref::Dark),
                    ThemePref::Dark => (Icon::ThemeDark, "Theme: dark", ThemePref::System),
                };
                if icon_button(ui, theme_icon, theme_tip, true, false, pal, 32.0).clicked() {
                    self.settings.theme = next;
                    self.persist_settings();
                }
                if let Some(od) = &self.doc {
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(format!("{} × {}", od.doc.width, od.doc.height))
                            .size(12.5)
                            .color(pal.text_muted),
                    );
                } else if let Some(l) = &self.loading {
                    ui.label(egui::RichText::new(l).size(12.5).color(pal.text_muted));
                }
            });
        });
    }

    fn copy_button(&self, ui: &mut Ui, enabled: bool) -> Response {
        let size = vec2(82.0, 30.0);
        let (rect, resp) =
            ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
        if ui.is_rect_visible(rect) {
            let p = ui.painter();
            let fill = match (enabled, resp.hovered()) {
                (false, _) => ACCENT.gamma_multiply(0.35),
                (true, true) => ACCENT.gamma_multiply(1.12),
                (true, false) => ACCENT,
            };
            p.rect_filled(rect, CornerRadius::same(8), fill);
            let icon_rect =
                egui::Rect::from_center_size(rect.left_center() + vec2(19.0, 0.0), vec2(16.0, 16.0));
            icons::paint(p, icon_rect, Icon::Copy, Color32::WHITE);
            p.text(
                rect.left_center() + vec2(33.0, 0.0),
                egui::Align2::LEFT_CENTER,
                "Copy",
                egui::FontId::proportional(14.0),
                Color32::WHITE,
            );
        }
        resp.on_hover_text(format!("Copy annotated image ({}C)", cmd()))
    }

    pub(super) fn tool_rail(&mut self, ui: &mut Ui, pal: &Palette) {
        egui::ScrollArea::vertical()
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| self.tool_rail_contents(ui, pal));
    }

    fn tool_rail_contents(&mut self, ui: &mut Ui, pal: &Palette) {
        ui.set_width(ui.available_width());
        ui.with_layout(Layout::top_down(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            for tool in Tool::ALL {
                let icon = match tool {
                    Tool::Select => Icon::Select,
                    Tool::Arrow => Icon::Arrow,
                    Tool::CurvedArrow => Icon::CurvedArrow,
                    Tool::Rectangle => Icon::Rectangle,
                    Tool::Ellipse => Icon::Ellipse,
                    Tool::Text => Icon::Text,
                    Tool::Freehand => Icon::Pen,
                    Tool::Pixelate => Icon::Pixelate,
                };
                let tip = format!("{} ({})", tool.label(), tool.key_hint());
                if icon_button(ui, icon, &tip, true, self.effective_tool() == tool, pal, 36.0).clicked() {
                    self.set_tool(tool);
                }
            }

            let Some(target) = self.style_target() else { return };
            let Some(style) = self.current_style() else { return };

            if target.has_color() {
                separator_h(ui, pal);
                let mut picked = None;
                // 3×3 grid of swatches.
                for row in PALETTE.chunks(3).enumerate() {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 1.0;
                        ui.add_space(((ui.available_width() - 59.0) * 0.5).max(0.0));
                        for (j, (name, c)) in row.1.iter().enumerate() {
                            let idx = row.0 * 3 + j;
                            let (rect, resp) = ui.allocate_exact_size(vec2(19.0, 19.0), Sense::click());
                            let color = rgba_to_color32(*c);
                            let p = ui.painter();
                            let selected = style.color == idx;
                            if selected {
                                // Neutral ring: a pink ring would blur into the pink/red swatches.
                                p.circle_stroke(
                                    rect.center(),
                                    8.8,
                                    Stroke::new(1.8, ui.visuals().strong_text_color()),
                                );
                            } else if resp.hovered() {
                                p.circle_stroke(rect.center(), 8.8, Stroke::new(1.4, pal.separator));
                            }
                            p.circle_filled(rect.center(), 6.3, color);
                            // Keep light/dark swatches visible on any panel.
                            p.circle_stroke(
                                rect.center(),
                                6.3,
                                Stroke::new(1.0, Color32::from_black_alpha(40)),
                            );
                            if resp.on_hover_text(*name).clicked() {
                                picked = Some(idx);
                            }
                        }
                    });
                }
                if picked.is_some() {
                    self.apply_style(picked, None);
                }
            }

            separator_h(ui, pal);
            let presets = presets_for(target);
            let mut picked = None;
            for (i, _) in presets.iter().enumerate() {
                let (rect, resp) = ui.allocate_exact_size(vec2(44.0, 23.0), Sense::click());
                let p = ui.painter();
                let selected = style.size == i;
                if selected {
                    p.rect_filled(rect, CornerRadius::same(6), pal.hover);
                    p.rect_stroke(
                        rect,
                        CornerRadius::same(6),
                        Stroke::new(1.0, ACCENT),
                        StrokeKind::Inside,
                    );
                } else if resp.hovered() {
                    p.rect_filled(rect, CornerRadius::same(6), pal.hover);
                }
                let preview_color = if target.has_color() {
                    PALETTE
                        .get(style.color)
                        .map_or(pal.icon, |c| rgba_to_color32(c.1))
                } else {
                    pal.icon
                };
                // Keep white swatch previews visible on light panels.
                let preview_color =
                    if preview_color.r() > 240 && preview_color.g() > 240 && !ui.visuals().dark_mode {
                        pal.icon
                    } else {
                        preview_color
                    };
                let c = rect.center();
                let tip = match target {
                    Tool::Text => {
                        let sz = [9.0, 11.0, 13.5, 16.0, 19.0][i];
                        p.text(
                            c,
                            egui::Align2::CENTER_CENTER,
                            "Aa",
                            egui::FontId::proportional(sz),
                            preview_color,
                        );
                        ["Small", "Medium", "Large", "Extra large", "Huge"][i]
                    }
                    Tool::Pixelate => {
                        let cell = [2.5, 3.5, 4.5, 5.5, 6.5][i];
                        for (dx, dy, a) in [
                            (-1.0, -1.0, 1.0),
                            (0.0, -1.0, 0.4),
                            (-1.0, 0.0, 0.4),
                            (0.0, 0.0, 1.0),
                        ] {
                            let r =
                                egui::Rect::from_min_size(c + vec2(dx * cell, dy * cell), vec2(cell, cell));
                            p.rect_filled(r, CornerRadius::ZERO, preview_color.gamma_multiply(a));
                        }
                        ["Fine", "Small", "Medium", "Coarse", "Very coarse"][i]
                    }
                    _ => {
                        let w = [1.5, 2.5, 4.0, 6.0, 8.5][i];
                        p.line_segment(
                            [c - vec2(12.0, 0.0), c + vec2(12.0, 0.0)],
                            Stroke::new(w, preview_color),
                        );
                        p.circle_filled(c - vec2(12.0, 0.0), w * 0.5, preview_color);
                        p.circle_filled(c + vec2(12.0, 0.0), w * 0.5, preview_color);
                        ["Thin", "Light", "Medium", "Bold", "Heavy"][i]
                    }
                };
                if resp.on_hover_text(tip).clicked() {
                    picked = Some(i);
                }
            }
            if picked.is_some() {
                self.apply_style(None, picked);
            }
        });
    }
}
