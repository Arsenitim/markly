//! The image canvas: view transform, pointer interaction and painting.
//!
//! Every interaction converts pointer positions into image space via
//! [`ViewTransform`]; annotations are never stored in screen coordinates.

use super::theme::{ACCENT, Palette};
use super::{MarklyApp, OpenDoc, PixelCacheEntry, TextEditing, cmd, icons, rgba_to_color32};
use crate::geometry::{
    Pt, Rect, ViewTransform, arrow_geometry, rect_corner_radius, smooth_stroke, snap_angle, square_from,
};
use crate::model::{Annotation, AnnotationId, Handle, Shape, Style};
use crate::render::{halo_radius, pixel_grid};
use crate::settings::Tool;
use crate::text;
use eframe::egui::{
    self, Color32, CornerRadius, CursorIcon, FontFamily, FontId, Id, Painter, PointerButton, Pos2, Sense,
    Stroke, StrokeKind, TextureOptions, pos2, vec2,
};

const FIT_MARGIN: f32 = 28.0;
/// Pointer tolerance for picking annotations, in screen points.
const HIT_TOLERANCE: f32 = 6.0;
const HANDLE_RADIUS: f32 = 5.5;

pub(super) enum Drag {
    Create {
        id: AnnotationId,
        origin: Pt,
        before: Vec<Annotation>,
    },
    Freehand {
        id: AnnotationId,
        raw: Vec<Pt>,
        before: Vec<Annotation>,
    },
    Move {
        id: AnnotationId,
        last: Pt,
        before: Vec<Annotation>,
    },
    Resize {
        id: AnnotationId,
        handle: Handle,
        orig: Shape,
        before: Vec<Annotation>,
    },
    Pan {
        last: Pos2,
    },
}

pub(super) fn text_edit_id() -> Id {
    Id::unique("markly-text-edit")
}

fn to_pos(p: Pt) -> Pos2 {
    pos2(p.x, p.y)
}
fn to_pt(p: Pos2) -> Pt {
    Pt::new(p.x, p.y)
}
fn to_rect(r: Rect) -> egui::Rect {
    egui::Rect::from_min_max(to_pos(r.min), to_pos(r.max))
}

pub(super) fn annotation_font(size_points: f32) -> FontId {
    FontId::new(
        size_points.max(1.0),
        FontFamily::Name(text::EGUI_FONT_NAME.into()),
    )
}

impl MarklyApp {
    pub(super) fn canvas(&mut self, ui: &mut egui::Ui, pal: &Palette) {
        let rect = ui.max_rect();
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, CornerRadius::ZERO, pal.canvas);
        self.canvas_size = Pt::new(rect.width(), rect.height());

        let hovering_files = self.ctx.input(|i| !i.raw.hovered_files.is_empty());

        if self.doc.is_none() {
            if let Some(label) = &self.loading {
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    label,
                    FontId::proportional(15.0),
                    pal.text_muted,
                );
            } else {
                self.empty_state(ui, rect, pal);
            }
            if hovering_files {
                drop_overlay(&painter, rect);
            }
            return;
        }

        // ---- view transform -------------------------------------------------
        let (iw, ih) = {
            let od = self.doc.as_ref().unwrap();
            (od.doc.width as f32, od.doc.height as f32)
        };
        let size = Pt::new(rect.width(), rect.height());
        self.view.canvas_min = to_pt(rect.min);
        self.view.pixels_per_point = self.ctx.pixels_per_point();
        if resp.hovered() {
            let (zoom, scroll, ptr) = self
                .ctx
                .input(|i| (i.zoom_delta(), i.smooth_scroll_delta, i.pointer.hover_pos()));
            if (zoom - 1.0).abs() > 1e-4 {
                let anchor = ptr.map_or(size * 0.5, |p| self.view.screen_to_canvas(to_pt(p)));
                if self.fit_mode {
                    self.view.fit(iw, ih, size, FIT_MARGIN);
                }
                self.view.zoom_about(anchor, zoom);
                self.fit_mode = false;
            } else if scroll != egui::Vec2::ZERO && !self.fit_mode {
                self.view.pan = self.view.pan + Pt::new(scroll.x, scroll.y);
            }
        }
        if self.fit_mode {
            self.view.fit(iw, ih, size, FIT_MARGIN);
        } else {
            self.view.clamp_pan(iw, ih, size, FIT_MARGIN);
        }
        self.view.snap_to_pixels();

        // ---- interaction ---------------------------------------------------
        self.interact(&resp);

        // ---- painting ------------------------------------------------------
        let view = self.view;
        let tool = self.effective_tool();
        let od = self.doc.as_mut().unwrap();
        let img_rect = to_rect(view.image_rect_to_screen(Rect::from_min_size(Pt::ZERO, iw, ih)));
        painter.add(
            egui::Shadow {
                offset: [0, 3],
                blur: 18,
                spread: 0,
                color: Color32::from_black_alpha(70),
            }
            .as_shape(img_rect, CornerRadius::ZERO),
        );
        if od.has_alpha {
            paint_checker(&painter, img_rect, pal);
        }
        painter.image(
            od.texture.id(),
            img_rect,
            egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        let _ = od.tex_scale;

        let img_painter = painter.with_clip_rect(img_rect.intersect(rect));
        paint_pixelations(&self.ctx, &img_painter, &view, od);
        let editing_id = self.editing.as_ref().map(|e| e.id);
        for a in &od.doc.annotations {
            if !matches!(a.shape, Shape::Pixelate { .. }) {
                paint_shape(&img_painter, &view, &a.shape, editing_id == Some(a.id));
            }
        }

        // Selection / hover chrome.
        let hover_pos = self.ctx.input(|i| i.pointer.hover_pos());
        if let Some(sel) = self.selection
            && let Some(a) = od.doc.get(sel)
            && self.editing.is_none()
        {
            paint_selection(&painter, &view, &a.shape);
        } else if tool == Tool::Select
            && self.drag.is_none()
            && resp.hovered()
            && let Some(p) = hover_pos
        {
            let ip = view.screen_to_image(to_pt(p));
            if let Some(id) = od.doc.hit_test(ip, view.screen_len_to_image(HIT_TOLERANCE))
                && let Some(a) = od.doc.get(id)
            {
                let r = to_rect(view.image_rect_to_screen(a.shape.bounds())).expand(3.0);
                painter.rect_stroke(
                    r,
                    CornerRadius::same(3),
                    Stroke::new(1.0, ACCENT.gamma_multiply(0.7)),
                    StrokeKind::Outside,
                );
            }
        }

        self.text_editor_overlay();
        self.update_cursor(&resp);

        if hovering_files {
            drop_overlay(&painter, rect);
        }
    }

    fn empty_state(&mut self, ui: &mut egui::Ui, rect: egui::Rect, pal: &Palette) {
        let c = rect.center();
        let card = egui::Rect::from_center_size(c, vec2(360.0, 230.0));
        let painter = ui.painter_at(rect);
        painter.rect(
            card,
            CornerRadius::same(16),
            pal.panel,
            Stroke::new(1.0, pal.separator),
            StrokeKind::Inside,
        );
        icons::paint(
            &painter,
            egui::Rect::from_center_size(c - vec2(0.0, 62.0), vec2(48.0, 48.0)),
            icons::Icon::Image,
            ACCENT,
        );
        painter.text(
            c - vec2(0.0, 14.0),
            egui::Align2::CENTER_CENTER,
            "Paste an image to start",
            FontId::proportional(19.0),
            ui.visuals().strong_text_color(),
        );
        painter.text(
            c + vec2(0.0, 14.0),
            egui::Align2::CENTER_CENTER,
            format!("{}V to paste  ·  {}O to open  ·  or drop a file", cmd(), cmd()),
            FontId::proportional(13.0),
            pal.text_muted,
        );
        let btn_area = egui::Rect::from_center_size(c + vec2(0.0, 62.0), vec2(220.0, 34.0));
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(btn_area)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        let paste = child.add_sized(
            [104.0, 32.0],
            egui::Button::new(egui::RichText::new("Paste").color(Color32::WHITE)).fill(ACCENT),
        );
        if paste.clicked() {
            self.paste_requested();
        }
        if child
            .add_sized([104.0, 32.0], egui::Button::new("Open…").fill(pal.hover))
            .clicked()
        {
            self.pick_open_file();
        }
    }

    // -----------------------------------------------------------------
    // Pointer interaction
    // -----------------------------------------------------------------

    fn interact(&mut self, resp: &egui::Response) {
        let (pressed, down, released, pos, mods, double, middle_pressed, middle_down, space) =
            self.ctx.input(|i| {
                (
                    i.pointer.primary_pressed(),
                    i.pointer.primary_down(),
                    i.pointer.primary_released(),
                    i.pointer.interact_pos(),
                    i.modifiers,
                    i.pointer.button_double_clicked(PointerButton::Primary),
                    i.pointer.button_pressed(PointerButton::Middle),
                    i.pointer.button_down(PointerButton::Middle),
                    i.key_down(egui::Key::Space),
                )
            });
        let Some(pos) = pos else { return };
        let view = self.view;
        let ip = view.screen_to_image(to_pt(pos));

        if (pressed || middle_pressed) && resp.hovered() && self.drag.is_none() {
            if self.editing.is_some() {
                self.commit_text_edit();
            }
            if middle_pressed || (space && pressed) {
                self.drag = Some(Drag::Pan { last: pos });
            } else if double && self.effective_tool() == Tool::Select {
                self.try_edit_text_at(ip);
            } else {
                self.begin_press(ip, pos, mods.shift);
            }
        }

        let active = down || middle_down;
        if let Some(drag) = &mut self.drag {
            if !active || released {
                self.end_drag();
            } else {
                let od = self.doc.as_mut().unwrap();
                match drag {
                    Drag::Pan { last } => {
                        let d = pos - *last;
                        *last = pos;
                        self.view.pan = self.view.pan + Pt::new(d.x, d.y);
                        self.fit_mode = false;
                    }
                    Drag::Create { id, origin, .. } => {
                        if let Some(a) = od.doc.get_mut(*id) {
                            update_created(&mut a.shape, *origin, ip, mods.shift);
                        }
                    }
                    Drag::Freehand { id, raw, .. } => {
                        let min_step = view.screen_len_to_image(1.0);
                        if raw.last().is_none_or(|l| l.dist(ip) >= min_step) {
                            raw.push(ip);
                            if let Some(Annotation {
                                shape: Shape::Freehand { points, .. },
                                ..
                            }) = od.doc.get_mut(*id)
                            {
                                points.clone_from(raw);
                            }
                        }
                    }
                    Drag::Move { id, last, .. } => {
                        let d = ip - *last;
                        *last = ip;
                        if let Some(a) = od.doc.get_mut(*id) {
                            a.shape.translate(d);
                        }
                    }
                    Drag::Resize { id, handle, orig, .. } => {
                        if let Some(a) = od.doc.get_mut(*id) {
                            a.shape = Shape::with_handle_moved(orig, *handle, ip, mods.shift);
                        }
                    }
                }
            }
        }
    }

    fn handle_at(&self, screen: Pos2) -> Option<(AnnotationId, Handle)> {
        let od = self.doc.as_ref()?;
        let id = self.selection?;
        let a = od.doc.get(id)?;
        a.shape
            .handles()
            .into_iter()
            .find(|(_, p)| to_pos(self.view.image_to_screen(*p)).distance(screen) <= HANDLE_RADIUS + 3.0)
            .map(|(h, _)| (id, h))
    }

    fn begin_press(&mut self, ip: Pt, screen: Pos2, shift: bool) {
        let tol = self.view.screen_len_to_image(HIT_TOLERANCE);
        let before = self.doc.as_ref().unwrap().doc.annotations.clone();

        if let Some((id, handle)) = self.handle_at(screen) {
            let orig = self.doc.as_ref().unwrap().doc.get(id).unwrap().shape.clone();
            self.drag = Some(Drag::Resize {
                id,
                handle,
                orig,
                before,
            });
            return;
        }

        let tool = self.effective_tool();
        match tool {
            Tool::Select => {
                let hit = self.doc.as_ref().unwrap().doc.hit_test(ip, tol);
                self.selection = hit;
                self.drag = Some(match hit {
                    Some(id) => Drag::Move { id, last: ip, before },
                    None => Drag::Pan { last: screen },
                });
            }
            Tool::Text => {
                if !self.try_edit_text_at(ip) {
                    let (color, size) = self.new_style(Tool::Text);
                    let m = text::metrics(size);
                    let pos = Pt::new(ip.x, ip.y - m.line_height * 0.5);
                    let od = self.doc.as_mut().unwrap();
                    let id = od.doc.add(Shape::Text {
                        pos,
                        text: String::new(),
                        color,
                        size,
                    });
                    self.selection = Some(id);
                    self.editing = Some(TextEditing {
                        id,
                        buffer: String::new(),
                        before,
                        focus_requested: false,
                        had_focus: false,
                    });
                }
            }
            Tool::Freehand => {
                let (color, width) = self.new_style(tool);
                let od = self.doc.as_mut().unwrap();
                let id = od.doc.add(Shape::Freehand {
                    points: vec![ip],
                    style: Style { color, width },
                });
                self.selection = Some(id);
                self.drag = Some(Drag::Freehand {
                    id,
                    raw: vec![ip],
                    before,
                });
            }
            Tool::Arrow | Tool::Rectangle | Tool::Ellipse | Tool::Pixelate => {
                let (color, size) = self.new_style(tool);
                let style = Style { color, width: size };
                let r = Rect::from_points(ip, ip);
                let shape = match tool {
                    Tool::Arrow => Shape::Arrow {
                        start: ip,
                        end: ip,
                        style,
                    },
                    Tool::Rectangle => Shape::Rectangle { rect: r, style },
                    Tool::Ellipse => Shape::Ellipse { rect: r, style },
                    _ => Shape::Pixelate { rect: r, block: size },
                };
                let od = self.doc.as_mut().unwrap();
                let id = od.doc.add(shape);
                self.selection = Some(id);
                self.drag = Some(Drag::Create {
                    id,
                    origin: ip,
                    before,
                });
                let _ = shift;
            }
        }
    }

    /// Starts editing the text annotation under `ip`, if any.
    fn try_edit_text_at(&mut self, ip: Pt) -> bool {
        let tol = self.view.screen_len_to_image(2.0);
        let Some(od) = self.doc.as_ref() else { return false };
        let hit = od
            .doc
            .annotations
            .iter()
            .rev()
            .find(|a| matches!(a.shape, Shape::Text { .. }) && a.shape.hit(ip, tol));
        let Some(Annotation {
            id,
            shape: Shape::Text { text, .. },
        }) = hit
        else {
            return false;
        };
        self.editing = Some(TextEditing {
            id: *id,
            buffer: text.clone(),
            before: od.doc.annotations.clone(),
            focus_requested: false,
            had_focus: false,
        });
        self.selection = Some(*id);
        self.drag = None;
        true
    }

    pub(super) fn end_drag(&mut self) {
        let Some(drag) = self.drag.take() else { return };
        let Some(od) = self.doc.as_mut() else { return };
        let before = match drag {
            Drag::Pan { .. } => return,
            Drag::Create { id, before, .. } => {
                if od.doc.get(id).is_some_and(|a| a.shape.is_degenerate()) {
                    od.doc.remove(id);
                    self.selection = None;
                    return;
                }
                before
            }
            Drag::Freehand { id, raw, before } => {
                if let Some(Annotation {
                    shape: Shape::Freehand { points, style },
                    ..
                }) = od.doc.get_mut(id)
                {
                    *points = smooth_stroke(&raw, style.width);
                }
                before
            }
            Drag::Move { before, .. } | Drag::Resize { before, .. } => before,
        };
        if before != od.doc.annotations {
            od.undo.commit(before, &od.doc.annotations);
            self.mark_dirty();
        }
    }

    pub(super) fn cancel_drag(&mut self) {
        let Some(drag) = self.drag.take() else { return };
        let Some(od) = self.doc.as_mut() else { return };
        match drag {
            Drag::Pan { .. } => {}
            Drag::Create { before, .. } | Drag::Freehand { before, .. } => {
                od.doc.annotations = before;
                self.selection = None;
            }
            Drag::Move { before, .. } | Drag::Resize { before, .. } => od.doc.annotations = before,
        }
    }

    fn update_cursor(&self, resp: &egui::Response) {
        if !resp.hovered() && self.drag.is_none() {
            return;
        }
        let space = self.ctx.input(|i| i.key_down(egui::Key::Space));
        let pos = self.ctx.input(|i| i.pointer.hover_pos());
        let icon = match &self.drag {
            Some(Drag::Pan { .. }) => CursorIcon::Grabbing,
            Some(Drag::Move { .. }) => CursorIcon::Move,
            Some(Drag::Resize { handle, .. }) => handle_cursor(*handle),
            Some(_) => CursorIcon::Crosshair,
            None if space => CursorIcon::Grab,
            None => {
                if let Some((_, h)) = pos.and_then(|p| self.handle_at(p)) {
                    handle_cursor(h)
                } else {
                    match self.effective_tool() {
                        Tool::Select => {
                            let over = pos.zip(self.doc.as_ref()).and_then(|(p, od)| {
                                od.doc.hit_test(
                                    self.view.screen_to_image(to_pt(p)),
                                    self.view.screen_len_to_image(HIT_TOLERANCE),
                                )
                            });
                            if over.is_some() {
                                CursorIcon::Move
                            } else {
                                CursorIcon::Default
                            }
                        }
                        Tool::Text => CursorIcon::Text,
                        _ => CursorIcon::Crosshair,
                    }
                }
            }
        };
        self.ctx.set_cursor_icon(icon);
    }

    // -----------------------------------------------------------------
    // On-canvas text editing
    // -----------------------------------------------------------------

    fn text_editor_overlay(&mut self) {
        let Some(ed) = &mut self.editing else { return };
        let Some(od) = &mut self.doc else { return };
        let Some(Annotation {
            shape:
                Shape::Text {
                    pos,
                    color,
                    size,
                    text,
                },
            ..
        }) = od.doc.get_mut(ed.id)
        else {
            self.editing = None;
            return;
        };
        let view = self.view;
        let font = annotation_font(*size * view.zoom);
        let screen = to_pos(view.image_to_screen(*pos));
        let caret = rgba_to_color32(*color);
        let id = text_edit_id();
        let mut buffer = std::mem::take(&mut ed.buffer);
        let out = egui::Area::new(Id::unique("markly-text-area"))
            .fixed_pos(screen)
            .order(egui::Order::Foreground)
            .show(&self.ctx, |ui| {
                ui.visuals_mut().text_cursor.stroke = Stroke::new(2.0, caret);
                ui.visuals_mut().selection.bg_fill = ACCENT.gamma_multiply(0.35);
                let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, _w: f32| {
                    ui.fonts_mut(|f| {
                        f.layout_no_wrap(buf.as_str().to_owned(), font.clone(), Color32::TRANSPARENT)
                    })
                };
                egui::TextEdit::multiline(&mut buffer)
                    .id(id)
                    .frame(egui::Frame::NONE)
                    .margin(egui::Margin::ZERO)
                    .desired_width(4.0)
                    .desired_rows(1)
                    .clip_text(false)
                    .layouter(&mut layouter)
                    .show(ui)
            })
            .inner;
        *text = buffer.clone();
        ed.buffer = buffer;
        if !ed.focus_requested {
            ed.focus_requested = true;
            self.ctx.memory_mut(|m| m.request_focus(id));
        } else if out.response.has_focus() {
            ed.had_focus = true;
        } else if ed.had_focus {
            // Escape, a press elsewhere, or a toolbar click ended editing.
            self.commit_text_edit();
        }
    }
}

fn handle_cursor(h: Handle) -> CursorIcon {
    match h {
        Handle::Start | Handle::End => CursorIcon::Crosshair,
        Handle::Box(-1, -1) | Handle::Box(1, 1) => CursorIcon::ResizeNwSe,
        Handle::Box(1, -1) | Handle::Box(-1, 1) => CursorIcon::ResizeNeSw,
        Handle::Box(_, 0) => CursorIcon::ResizeHorizontal,
        Handle::Box(..) => CursorIcon::ResizeVertical,
    }
}

fn update_created(shape: &mut Shape, origin: Pt, p: Pt, constrain: bool) {
    match shape {
        Shape::Arrow { end, .. } => *end = if constrain { snap_angle(origin, p) } else { p },
        Shape::Rectangle { rect, .. } | Shape::Ellipse { rect, .. } | Shape::Pixelate { rect, .. } => {
            let q = if constrain { square_from(origin, p) } else { p };
            *rect = Rect::from_points(origin, q);
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Painting
// ---------------------------------------------------------------------------

fn drop_overlay(painter: &Painter, rect: egui::Rect) {
    painter.rect(
        rect.shrink(10.0),
        CornerRadius::same(14),
        ACCENT.gamma_multiply(0.12),
        Stroke::new(2.0, ACCENT),
        StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "Drop to open",
        FontId::proportional(20.0),
        ACCENT,
    );
}

fn paint_checker(painter: &Painter, r: egui::Rect, pal: &Palette) {
    painter.rect_filled(r, CornerRadius::ZERO, pal.checker_a);
    let cell = 10.0;
    let clip = r.intersect(painter.clip_rect());
    if !clip.is_positive() {
        return;
    }
    let x0 = ((clip.min.x - r.min.x) / cell).floor() as i32;
    let y0 = ((clip.min.y - r.min.y) / cell).floor() as i32;
    let x1 = ((clip.max.x - r.min.x) / cell).ceil() as i32;
    let y1 = ((clip.max.y - r.min.y) / cell).ceil() as i32;
    let mut mesh = egui::Mesh::default();
    for y in y0..y1 {
        for x in x0..x1 {
            if (x + y) % 2 == 0 {
                continue;
            }
            let c =
                egui::Rect::from_min_size(r.min + vec2(x as f32 * cell, y as f32 * cell), vec2(cell, cell));
            mesh.add_colored_rect(c.intersect(r), pal.checker_b);
        }
    }
    painter.add(mesh);
}

fn paint_pixelations(ctx: &egui::Context, painter: &Painter, view: &ViewTransform, od: &mut OpenDoc) {
    let live: std::collections::HashSet<AnnotationId> = od.doc.annotations.iter().map(|a| a.id).collect();
    od.pixel_cache.retain(|id, _| live.contains(id));
    for a in &od.doc.annotations {
        let Shape::Pixelate { rect, block } = &a.shape else {
            continue;
        };
        let key = (
            rect.min.x.to_bits(),
            rect.min.y.to_bits(),
            rect.max.x.to_bits(),
            rect.max.y.to_bits(),
            block.to_bits(),
        );
        let fresh = od.pixel_cache.get(&a.id).is_some_and(|e| e.key == key);
        if !fresh {
            match pixel_grid(&od.image, *rect, *block) {
                Some(grid) => {
                    let pixels: Vec<u8> = grid.colors.iter().flatten().copied().collect();
                    let img = egui::ColorImage::from_rgba_unmultiplied(
                        [grid.cols as usize, grid.rows as usize],
                        &pixels,
                    );
                    let texture =
                        ctx.load_texture(format!("pixelate-{}", a.id), img, TextureOptions::NEAREST);
                    od.pixel_cache
                        .insert(a.id, PixelCacheEntry { key, grid, texture });
                }
                None => {
                    od.pixel_cache.remove(&a.id);
                    continue;
                }
            }
        }
        let e = &od.pixel_cache[&a.id];
        let g = &e.grid;
        let full = Rect::from_min_size(
            g.origin,
            g.cols as f32 * g.block as f32,
            g.rows as f32 * g.block as f32,
        );
        let clip = to_rect(view.image_rect_to_screen(g.clip)).intersect(painter.clip_rect());
        painter.with_clip_rect(clip).image(
            e.texture.id(),
            to_rect(view.image_rect_to_screen(full)),
            egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }
}

/// Paints one annotation with egui shapes using the same geometry helpers
/// as the export renderer.
pub(super) fn paint_shape(painter: &Painter, view: &ViewTransform, shape: &Shape, editing: bool) {
    let z = view.zoom;
    let sp = |p: Pt| to_pos(view.image_to_screen(p));
    match shape {
        Shape::Arrow { start, end, style } => {
            let Some(g) = arrow_geometry(*start, *end, style.width) else {
                return;
            };
            let c = rgba_to_color32(style.color);
            painter.add(egui::Shape::convex_polygon(
                g.shaft.iter().map(|p| sp(*p)).collect(),
                c,
                Stroke::NONE,
            ));
            for t in g.head_triangles() {
                painter.add(egui::Shape::convex_polygon(
                    t.iter().map(|p| sp(*p)).collect(),
                    c,
                    Stroke::NONE,
                ));
            }
            painter.circle_filled(sp(g.tail), g.tail_radius * z, c);
        }
        Shape::Rectangle { rect, style } => {
            let r = to_rect(view.image_rect_to_screen(*rect));
            let radius = (rect_corner_radius(rect, style.width) * z).clamp(0.0, 255.0);
            painter.rect_stroke(
                r,
                CornerRadius::same(radius.round() as u8),
                Stroke::new(style.width * z, rgba_to_color32(style.color)),
                StrokeKind::Middle,
            );
        }
        Shape::Ellipse { rect, style } => {
            // egui's built-in ellipse uses few segments; tessellate finely
            // so large ellipses stay smooth.
            let r = view.image_rect_to_screen(*rect);
            let perimeter = std::f32::consts::PI * (r.width() + r.height()) * 0.5;
            let segments = ((perimeter / 3.0) as usize).clamp(32, 512);
            let pts = crate::geometry::ellipse_points(&r, segments)
                .into_iter()
                .map(to_pos)
                .collect();
            painter.add(egui::Shape::closed_line(
                pts,
                Stroke::new(style.width * z, rgba_to_color32(style.color)),
            ));
        }
        Shape::Freehand { points, style } => {
            let c = rgba_to_color32(style.color);
            let w = style.width * z;
            let pts: Vec<Pos2> = points.iter().map(|p| sp(*p)).collect();
            if let (Some(first), Some(last)) = (pts.first(), pts.last()) {
                painter.circle_filled(*first, w * 0.5, c);
                painter.circle_filled(*last, w * 0.5, c);
            }
            if pts.len() > 1 {
                painter.add(egui::Shape::line(pts, Stroke::new(w, c)));
            }
        }
        Shape::Text {
            pos,
            text,
            color,
            size,
        } => {
            if text.is_empty() && !editing {
                return;
            }
            let font = annotation_font(size * z);
            let galley = painter.layout_no_wrap(text.clone(), font, rgba_to_color32(*color));
            let origin = sp(*pos);
            let halo = rgba_to_color32(color.halo());
            let r = halo_radius(*size) * z;
            if r >= 0.3 {
                for i in 0..12 {
                    let a = i as f32 / 12.0 * std::f32::consts::TAU;
                    painter.galley_with_override_text_color(
                        origin + vec2(a.cos(), a.sin()) * r,
                        galley.clone(),
                        halo,
                    );
                }
            }
            painter.galley(origin, galley, rgba_to_color32(*color));
        }
        Shape::Pixelate { .. } => {}
    }
}

fn paint_selection(painter: &Painter, view: &ViewTransform, shape: &Shape) {
    let handle_stroke = Stroke::new(1.5, ACCENT);
    match shape {
        Shape::Arrow { .. } => {}
        _ => {
            let r = to_rect(view.image_rect_to_screen(shape.bounds())).expand(
                if matches!(shape, Shape::Text { .. }) {
                    4.0
                } else {
                    0.0
                },
            );
            painter.rect_stroke(
                r,
                CornerRadius::same(2),
                Stroke::new(1.0, ACCENT.gamma_multiply(0.8)),
                StrokeKind::Middle,
            );
        }
    }
    for (h, p) in shape.handles() {
        let c = to_pos(view.image_to_screen(p));
        match h {
            Handle::Start | Handle::End => {
                painter.circle(c, HANDLE_RADIUS, Color32::WHITE, handle_stroke);
            }
            Handle::Box(..) => {
                let r = egui::Rect::from_center_size(c, vec2(HANDLE_RADIUS * 1.7, HANDLE_RADIUS * 1.7));
                painter.rect(
                    r,
                    CornerRadius::same(2),
                    Color32::WHITE,
                    handle_stroke,
                    StrokeKind::Middle,
                );
            }
        }
    }
}
