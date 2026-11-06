//! egui front-end. Owns GUI state only; documents, rendering and storage
//! live in the library modules.

mod canvas;
mod dragout;
mod history;
pub mod icons;
mod script;
mod theme;
mod toolbar;

use crate::clipboard::{self, ClipEvent, ClipboardWorker, PasteContent};
use crate::geometry::{Pt, ViewTransform};
use crate::model::{
    Annotation, AnnotationId, Document, PALETTE, PIXEL_BLOCKS, Rgba, STROKE_WIDTHS, Shape, TEXT_SIZES,
    TextFormat, size_unit,
};
use crate::render::PixelGrid;
use crate::settings::{Settings, ThemePref, Tool, ToolStyle};
use crate::store::{self, OriginalData, Store, ThumbJob, Writer, WriterEvent};
use crate::text;
use crate::undo::UndoStack;
use canvas::Drag;
use eframe::egui::{
    self, Color32, ColorImage, Key, KeyboardShortcut, Modifiers, TextureHandle, TextureOptions,
};
use history::HistoryState;
use image::RgbaImage;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

/// Process start time, set by `main` for startup tracing.
pub static PROCESS_START: OnceLock<Instant> = OnceLock::new();

const AUTOSAVE_DEBOUNCE: Duration = Duration::from_millis(400);

/// Results delivered from background threads.
enum Bg {
    Loaded {
        token: u64,
        result: Result<Box<Loaded>, String>,
    },
    OpenPicked(Option<PathBuf>),
    ExportPicked(Option<PathBuf>),
    Exported(Result<PathBuf, String>),
    HistoryListed(Vec<store::HistoryEntry>),
    Thumbnail {
        id: String,
        image: Option<ColorImage>,
    },
}

enum LoadSource {
    Stored(Box<Document>),
    Pixels(RgbaImage),
    File(PathBuf),
}

struct Loaded {
    doc: Document,
    is_new: bool,
    image: Arc<RgbaImage>,
    texture: ColorImage,
    /// Texture pixels per image pixel (< 1 when the image exceeds the GPU's
    /// maximum texture size).
    tex_scale: f32,
    has_alpha: bool,
    thumb_base: Arc<RgbaImage>,
    thumb_scale: f32,
    original: Option<OriginalData>,
}

/// The currently open document plus its GPU-side caches.
struct OpenDoc {
    doc: Document,
    image: Arc<RgbaImage>,
    texture: TextureHandle,
    tex_scale: f32,
    has_alpha: bool,
    thumb_base: Arc<RgbaImage>,
    thumb_scale: f32,
    undo: UndoStack,
    pixel_cache: HashMap<AnnotationId, PixelCacheEntry>,
    /// Multiplier applied to size presets for this image.
    unit: f32,
}

struct PixelCacheEntry {
    key: (u32, u32, u32, u32, u32),
    grid: PixelGrid,
    texture: TextureHandle,
}

struct TextEditing {
    id: AnnotationId,
    buffer: String,
    before: Vec<Annotation>,
    focus_requested: bool,
    had_focus: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum ToastKind {
    Info,
    Error,
}

struct Toast {
    text: String,
    kind: ToastKind,
    until: Instant,
}

pub struct MarklyApp {
    ctx: egui::Context,
    store: Option<Store>,
    writer: Option<Writer>,
    writer_rx: Receiver<WriterEvent>,
    clip: ClipboardWorker,
    clip_rx: Receiver<ClipEvent>,
    bg_tx: Sender<Bg>,
    bg_rx: Receiver<Bg>,
    settings: Settings,

    doc: Option<OpenDoc>,
    /// Label of an in-flight load (shown while the image decodes).
    loading: Option<String>,
    load_token: u64,
    load_started: Instant,
    max_texture_side: usize,

    tool: Tool,
    /// The drawing tool whose style the palette shows while in Select mode
    /// with nothing selected (so colour/size controls never disappear).
    last_draw_tool: Tool,
    selection: Option<AnnotationId>,
    drag: Option<Drag>,
    editing: Option<TextEditing>,
    view: ViewTransform,
    fit_mode: bool,
    canvas_size: Pt,

    history: HistoryState,
    toast: Option<Toast>,
    dirty_since: Option<Instant>,
    dialog_open: bool,
    /// Set by the top bar when the drag-out handle starts a drag; the OS
    /// session is started from `ui()`, which has the window handle.
    drag_out_requested: bool,
    drag_out_ended: Arc<std::sync::atomic::AtomicBool>,
    window_size: Option<[f32; 2]>,
    applied_theme: Option<ThemePref>,
    trace: bool,
    bench_exit: bool,
    first_frame_done: bool,
    /// Developer aid: `MARKLY_DEBUG_SCREENSHOT=<png>` saves a window
    /// screenshot once the document is shown, then exits.
    debug_shot: Option<(PathBuf, u32)>,
    /// Developer aid: `MARKLY_DEBUG_SCRIPT=<file>` replays synthetic input.
    script: Option<script::Script>,
}

fn rgba_to_color32(c: Rgba) -> Color32 {
    let [r, g, b, a] = c.0;
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// Platform-appropriate name of the command modifier, for hints.
fn cmd() -> &'static str {
    if cfg!(target_os = "macos") { "⌘" } else { "Ctrl+" }
}

fn tool_for(shape: &Shape) -> Tool {
    match shape {
        Shape::Arrow { .. } => Tool::Arrow,
        Shape::CurvedArrow { .. } => Tool::CurvedArrow,
        Shape::Rectangle { .. } => Tool::Rectangle,
        Shape::Ellipse { .. } => Tool::Ellipse,
        Shape::Text { .. } => Tool::Text,
        Shape::Freehand { .. } => Tool::Freehand,
        Shape::Pixelate { .. } => Tool::Pixelate,
    }
}

fn presets_for(tool: Tool) -> &'static [f32; 5] {
    match tool {
        Tool::Text => &TEXT_SIZES,
        Tool::Pixelate => &PIXEL_BLOCKS,
        _ => &STROKE_WIDTHS,
    }
}

fn shape_size(shape: &Shape) -> f32 {
    match shape {
        Shape::Arrow { style, .. }
        | Shape::CurvedArrow { style, .. }
        | Shape::Rectangle { style, .. }
        | Shape::Ellipse { style, .. }
        | Shape::Freehand { style, .. } => style.width,
        Shape::Text { size, .. } => *size,
        Shape::Pixelate { block, .. } => *block,
    }
}

impl MarklyApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        store: Option<Store>,
        settings: Settings,
        file: Option<PathBuf>,
    ) -> Self {
        let ctx = cc.egui_ctx.clone();
        install_fonts(&ctx);
        theme::install(&ctx);
        ctx.options_mut(|o| {
            o.zoom_with_keyboard = false;
            // A click that creates a text box must not immediately end its
            // editing on release.
            o.input_options.surrender_focus_on = egui::SurrenderFocusOn::Presses;
        });

        let (writer_tx, writer_rx) = mpsc::channel();
        let writer = store.as_ref().map(|s| {
            let c = ctx.clone();
            Writer::spawn(s.clone(), writer_tx, move || c.request_repaint())
        });
        let (clip_tx, clip_rx) = mpsc::channel();
        let c = ctx.clone();
        let clip = ClipboardWorker::spawn(clip_tx, move || c.request_repaint());
        let (bg_tx, bg_rx) = mpsc::channel();

        let mut app = MarklyApp {
            ctx: ctx.clone(),
            store,
            writer,
            writer_rx,
            clip,
            clip_rx,
            bg_tx,
            bg_rx,
            tool: settings.tool,
            last_draw_tool: if settings.tool == Tool::Select {
                Tool::Arrow
            } else {
                settings.tool
            },
            settings,
            doc: None,
            loading: None,
            load_token: 0,
            load_started: Instant::now(),
            max_texture_side: query_max_texture_side(cc),
            selection: None,
            drag: None,
            editing: None,
            view: ViewTransform::default(),
            fit_mode: true,
            canvas_size: Pt::new(800.0, 600.0),
            history: HistoryState::default(),
            toast: None,
            dirty_since: None,
            dialog_open: false,
            drag_out_requested: false,
            drag_out_ended: Arc::default(),
            window_size: None,
            applied_theme: None,
            trace: std::env::var_os("MARKLY_TRACE").is_some(),
            bench_exit: std::env::var_os("MARKLY_BENCH_EXIT").is_some(),
            first_frame_done: false,
            debug_shot: std::env::var_os("MARKLY_DEBUG_SCREENSHOT").map(|p| (PathBuf::from(p), 0)),
            script: std::env::var_os("MARKLY_DEBUG_SCRIPT").and_then(|p| {
                match script::Script::load(Path::new(&p)) {
                    Ok(s) => Some(s),
                    Err(e) => {
                        eprintln!("markly: bad script: {e}");
                        None
                    }
                }
            }),
        };

        if app.store.is_none() {
            app.toast_error("Local storage unavailable — work will not be remembered");
        }

        if let Some(path) = file {
            app.start_load(LoadSource::File(path.clone()), &file_label(&path));
        } else if let (Some(store), Some(id)) = (&app.store, app.settings.last_document.clone()) {
            match store.load_document(&id) {
                Ok(doc) => app.start_load(LoadSource::Stored(Box::new(doc)), "Restoring last document…"),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => app.settings.last_document = None,
                Err(e) => {
                    app.settings.last_document = None;
                    app.toast_error(format!("Could not restore last document: {e}"));
                }
            }
        }
        if let Some(w) = &app.writer {
            // Housekeeping off the UI thread: temp files, history limit.
            w.prune(app.settings.last_document.clone());
        }
        app
    }

    // -----------------------------------------------------------------
    // Loading documents
    // -----------------------------------------------------------------

    fn start_load(&mut self, source: LoadSource, label: &str) {
        self.load_token += 1;
        let token = self.load_token;
        self.loading = Some(label.to_string());
        self.load_started = Instant::now();
        let max_tex = self.max_texture_side.max(self.ctx.input(|i| i.max_texture_side));
        let tx = self.bg_tx.clone();
        let ctx = self.ctx.clone();
        let store = self.store.clone();
        std::thread::Builder::new()
            .name("markly-load".into())
            .spawn(move || {
                let t0 = Instant::now();
                let result = load_document(source, store.as_ref(), max_tex).map(Box::new);
                if std::env::var_os("MARKLY_TRACE").is_some() {
                    eprintln!(
                        "[markly] background load {:.1} ms",
                        t0.elapsed().as_secs_f64() * 1000.0
                    );
                }
                let _ = tx.send(Bg::Loaded { token, result });
                ctx.request_repaint();
            })
            .expect("spawn loader");
    }

    fn install_loaded(&mut self, l: Loaded) {
        // Persist the outgoing document first so it is safely in history.
        self.finish_interactions();
        self.save_now();

        let texture = self.ctx.load_texture(
            "document",
            l.texture,
            TextureOptions {
                magnification: egui::TextureFilter::Linear,
                minification: egui::TextureFilter::Linear,
                wrap_mode: egui::TextureWrapMode::ClampToEdge,
                mipmap_mode: Some(egui::TextureFilter::Linear),
            },
        );
        let unit = size_unit(l.doc.width, l.doc.height);
        if l.is_new
            && let Some(w) = &self.writer
        {
            if let Some(orig) = l.original {
                w.save_original(&l.doc.id, &l.doc.original_file, orig);
            }
            w.save_document(
                l.doc.clone(),
                Some(ThumbJob {
                    base: l.thumb_base.clone(),
                    scale: l.thumb_scale,
                }),
            );
            w.prune(Some(l.doc.id.clone()));
        }
        self.settings.last_document = Some(l.doc.id.clone());
        self.persist_settings();
        self.doc = Some(OpenDoc {
            doc: l.doc,
            image: l.image,
            texture,
            tex_scale: l.tex_scale,
            has_alpha: l.has_alpha,
            thumb_base: l.thumb_base,
            thumb_scale: l.thumb_scale,
            undo: UndoStack::default(),
            pixel_cache: HashMap::new(),
            unit,
        });
        self.loading = None;
        self.selection = None;
        self.fit_mode = true;
        self.history.mark_stale();
        if std::env::var_os("MARKLY_DEBUG_SELECT_LAST").is_some() {
            self.selection = self
                .doc
                .as_ref()
                .and_then(|d| d.doc.annotations.last())
                .map(|a| a.id);
        }
        if self.trace
            && let Some(t0) = PROCESS_START.get()
        {
            eprintln!(
                "[markly] document ready at {:.1} ms (load took {:.1} ms)",
                t0.elapsed().as_secs_f64() * 1000.0,
                self.load_started.elapsed().as_secs_f64() * 1000.0
            );
        }
    }

    fn open_stored(&mut self, id: &str) {
        if self.doc.as_ref().is_some_and(|d| d.doc.id == id) {
            return;
        }
        let Some(store) = &self.store else { return };
        match store.load_document(id) {
            Ok(doc) => self.start_load(LoadSource::Stored(Box::new(doc)), "Opening…"),
            Err(e) => self.toast_error(format!("Could not open document: {e}")),
        }
    }

    fn open_path(&mut self, path: PathBuf) {
        let label = file_label(&path);
        self.start_load(LoadSource::File(path), &label);
    }

    fn close_document(&mut self) {
        self.finish_interactions();
        self.doc = None;
        self.selection = None;
        self.dirty_since = None;
        self.settings.last_document = None;
        self.persist_settings();
    }

    // -----------------------------------------------------------------
    // Persistence
    // -----------------------------------------------------------------

    fn mark_dirty(&mut self) {
        if let Some(od) = &mut self.doc {
            od.doc.touch();
            self.dirty_since.get_or_insert_with(Instant::now);
        }
    }

    fn save_now(&mut self) {
        if self.dirty_since.take().is_none() {
            return;
        }
        if let (Some(w), Some(od)) = (&self.writer, &self.doc) {
            w.save_document(
                od.doc.clone(),
                Some(ThumbJob {
                    base: od.thumb_base.clone(),
                    scale: od.thumb_scale,
                }),
            );
        }
    }

    fn autosave_tick(&mut self) {
        let Some(since) = self.dirty_since else { return };
        // Don't save half-finished drags; they are saved when released.
        if self.drag.is_some() {
            return;
        }
        let elapsed = since.elapsed();
        if elapsed >= AUTOSAVE_DEBOUNCE {
            self.save_now();
        } else {
            self.ctx.request_repaint_after(AUTOSAVE_DEBOUNCE - elapsed);
        }
    }

    fn persist_settings(&mut self) {
        self.settings.tool = self.tool;
        if let Some(sz) = self.window_size {
            self.settings.window_size = Some(sz);
        }
        if let Some(w) = &self.writer {
            w.save_settings(self.settings.clone());
        }
    }

    fn shutdown(&mut self) {
        let t0 = Instant::now();
        self.finish_interactions();
        self.save_now();
        self.persist_settings();
        if let Some(w) = &mut self.writer
            && !w.shutdown(Duration::from_secs(5))
        {
            eprintln!("markly: timed out flushing autosave");
        }
        if self.trace {
            eprintln!(
                "[markly] shutdown took {:.1} ms",
                t0.elapsed().as_secs_f64() * 1000.0
            );
        }
    }

    // -----------------------------------------------------------------
    // Editing helpers
    // -----------------------------------------------------------------

    /// Commit an in-progress text edit and finish any drag.
    fn finish_interactions(&mut self) {
        self.commit_text_edit();
        if self.drag.is_some() {
            self.end_drag();
        }
    }

    /// Run `f` on the document as one undoable step.
    fn edit_doc(&mut self, f: impl FnOnce(&mut Document)) {
        let Some(od) = &mut self.doc else { return };
        let before = od.doc.annotations.clone();
        f(&mut od.doc);
        if before != od.doc.annotations {
            od.undo.commit(before, &od.doc.annotations);
            self.mark_dirty();
        }
    }

    fn undo(&mut self) {
        self.finish_interactions();
        if let Some(od) = &mut self.doc
            && let Some(prev) = od.undo.undo(&od.doc.annotations)
        {
            od.doc.annotations = prev;
            self.after_history_change();
        }
    }

    fn redo(&mut self) {
        self.finish_interactions();
        if let Some(od) = &mut self.doc
            && let Some(next) = od.undo.redo(&od.doc.annotations)
        {
            od.doc.annotations = next;
            self.after_history_change();
        }
    }

    fn after_history_change(&mut self) {
        if let (Some(od), Some(sel)) = (&self.doc, self.selection)
            && od.doc.get(sel).is_none()
        {
            self.selection = None;
        }
        self.mark_dirty();
    }

    fn delete_selection(&mut self) {
        if let Some(id) = self.selection.take() {
            self.edit_doc(|d| {
                d.remove(id);
            });
        }
    }

    fn nudge_selection(&mut self, d: Pt) {
        if let Some(id) = self.selection {
            self.edit_doc(|doc| {
                if let Some(a) = doc.get_mut(id) {
                    a.shape.translate(d);
                }
            });
        }
    }

    fn set_tool(&mut self, tool: Tool) {
        self.finish_interactions();
        if tool != self.tool {
            self.tool = tool;
            if tool != Tool::Select {
                self.selection = None;
                self.last_draw_tool = tool;
            }
            self.persist_settings();
        }
    }

    /// Tool whose style the palette currently edits.
    fn style_target(&self) -> Option<Tool> {
        let sel = self.editing.as_ref().map(|e| e.id).or(self.selection);
        if let (Some(od), Some(id)) = (&self.doc, sel)
            && let Some(a) = od.doc.get(id)
        {
            return Some(tool_for(&a.shape));
        }
        Some(if self.tool == Tool::Select {
            self.last_draw_tool
        } else {
            self.tool
        })
    }

    /// Indices of the style shown as active in the palette.
    fn current_style(&self) -> Option<ToolStyle> {
        let target = self.style_target()?;
        let mut style = self.settings.styles.get(target)?;
        style.color = self.settings.color;
        let sel = self.editing.as_ref().map(|e| e.id).or(self.selection);
        if let (Some(od), Some(id)) = (&self.doc, sel)
            && let Some(a) = od.doc.get(id)
        {
            if let Some(c) = a.shape.color() {
                style.color = PALETTE.iter().position(|(_, p)| *p == c).unwrap_or(usize::MAX);
            }
            let size = shape_size(&a.shape) / od.unit;
            style.size = presets_for(target)
                .iter()
                .enumerate()
                .min_by(|a, b| (a.1 - size).abs().total_cmp(&(b.1 - size).abs()))
                .map(|(i, _)| i)
                .unwrap_or(2);
        }
        Some(style)
    }

    fn apply_style(&mut self, color: Option<usize>, size: Option<usize>) {
        let Some(target) = self.style_target() else { return };
        if let Some(c) = color {
            self.settings.color = c;
        }
        if let Some(z) = size
            && let Some(s) = self.settings.styles.get_mut(target)
        {
            s.size = z;
        }
        self.persist_settings();
        let sel = self.editing.as_ref().map(|e| e.id).or(self.selection);
        let Some(id) = sel else { return };
        let unit = self.doc.as_ref().map_or(1.0, |d| d.unit);
        let apply = |doc: &mut Document| {
            if let Some(a) = doc.get_mut(id) {
                if let Some(c) = color {
                    a.shape.set_color(PALETTE[c].1);
                }
                if let Some(z) = size {
                    a.shape.set_size(presets_for(target)[z] * unit);
                }
            }
        };
        if self.editing.is_some() {
            // Part of the running text-edit undo step.
            if let Some(od) = &mut self.doc {
                apply(&mut od.doc);
            }
        } else {
            self.edit_doc(apply);
        }
    }

    /// Formatting shown in the rail: the selected/edited label's, else the
    /// default for new labels.
    fn current_text_format(&self) -> TextFormat {
        let sel = self.editing.as_ref().map(|e| e.id).or(self.selection);
        if let (Some(od), Some(id)) = (&self.doc, sel)
            && let Some(Annotation {
                shape: Shape::Text { format, .. },
                ..
            }) = od.doc.get(id)
        {
            return *format;
        }
        self.settings.text_format
    }

    /// Flips one formatting flag on the selected/edited label (if any) and
    /// remembers the result for new labels.
    fn toggle_text_format(&mut self, flip: fn(&mut TextFormat)) {
        let mut fmt = self.current_text_format();
        flip(&mut fmt);
        self.settings.text_format = fmt;
        self.persist_settings();
        let sel = self.editing.as_ref().map(|e| e.id).or(self.selection);
        let Some(id) = sel else { return };
        let apply = move |doc: &mut Document| {
            if let Some(Annotation {
                shape: Shape::Text { format, .. },
                ..
            }) = doc.get_mut(id)
            {
                *format = fmt;
            }
        };
        if self.editing.is_some() {
            // Part of the running text-edit undo step.
            if let Some(od) = &mut self.doc {
                apply(&mut od.doc);
            }
        } else if self
            .doc
            .as_ref()
            .and_then(|od| od.doc.get(id))
            .is_some_and(|a| matches!(a.shape, Shape::Text { .. }))
        {
            self.edit_doc(apply);
        }
    }

    /// Style values (colour, size in image px) for a new annotation.
    fn new_style(&self, tool: Tool) -> (Rgba, f32) {
        let s = self.settings.styles.get(tool).unwrap_or_default();
        let unit = self.doc.as_ref().map_or(1.0, |d| d.unit);
        let color = PALETTE.get(self.settings.color).map_or(PALETTE[0].1, |p| p.1);
        (color, presets_for(tool)[s.size.min(4)] * unit)
    }

    fn commit_text_edit(&mut self) {
        let Some(ed) = self.editing.take() else { return };
        self.ctx.memory_mut(|m| m.surrender_focus(canvas::text_edit_id()));
        let Some(od) = &mut self.doc else { return };
        if ed.buffer.trim().is_empty() {
            od.doc.remove(ed.id);
            if self.selection == Some(ed.id) {
                self.selection = None;
            }
        } else if let Some(Annotation {
            shape: Shape::Text { text, .. },
            ..
        }) = od.doc.get_mut(ed.id)
        {
            *text = ed.buffer.trim_end_matches('\n').to_string();
            self.selection = Some(ed.id);
        }
        if ed.before != od.doc.annotations {
            od.undo.commit(ed.before, &od.doc.annotations);
            self.mark_dirty();
        }
    }

    // -----------------------------------------------------------------
    // Clipboard, files, export
    // -----------------------------------------------------------------

    fn copy_to_clipboard(&mut self) {
        self.finish_interactions();
        let Some(od) = &self.doc else {
            self.toast_info("Nothing to copy yet");
            return;
        };
        self.clip.copy(od.image.clone(), od.doc.annotations.clone());
    }

    fn paste_requested(&mut self) {
        self.clip.paste();
    }

    fn handle_paste_image(&mut self, img: &ColorImage) {
        // egui already read the clipboard; reuse it when lossless (opaque).
        if img.pixels.iter().all(|p| p.a() == 255) {
            let [w, h] = img.size;
            let raw: Vec<u8> = img.pixels.iter().flat_map(|p| p.to_array()).collect();
            if let Some(rgba) = RgbaImage::from_raw(w as u32, h as u32, raw) {
                self.start_load(LoadSource::Pixels(rgba), "Pasting…");
                return;
            }
        }
        self.paste_requested();
    }

    fn pick_open_file(&mut self) {
        if self.dialog_open {
            return;
        }
        let start_dir = self
            .doc
            .as_ref()
            .and_then(|d| d.doc.source_path.as_ref())
            .and_then(|p| Path::new(p).parent().map(Path::to_path_buf));
        self.run_dialog(move || {
            let mut d = rfd::FileDialog::new()
                .set_title("Open image")
                .add_filter("Images", &["png", "jpg", "jpeg", "webp"]);
            if let Some(dir) = start_dir {
                d = d.set_directory(dir);
            }
            Bg::OpenPicked(d.pick_file())
        });
    }

    fn pick_export_file(&mut self) {
        if self.dialog_open {
            return;
        }
        self.finish_interactions();
        let Some(od) = &self.doc else {
            self.toast_info("Nothing to export yet");
            return;
        };
        let source = od.doc.source_path.as_ref().map(PathBuf::from);
        let stem = export_stem(od);
        let dir = source.as_ref().and_then(|p| p.parent().map(Path::to_path_buf));
        self.run_dialog(move || {
            let mut d = rfd::FileDialog::new()
                .set_title("Export image")
                .add_filter("PNG image", &["png"])
                .add_filter("JPEG image", &["jpg", "jpeg"])
                .set_file_name(format!("{stem}.png"));
            if let Some(dir) = dir {
                d = d.set_directory(dir);
            }
            Bg::ExportPicked(d.save_file())
        });
    }

    /// File dialogs block. On Linux (xdg portal) they run on a helper
    /// thread so the window keeps repainting; macOS requires the main
    /// thread, and Windows' dialog is modal and pumps messages itself.
    fn run_dialog(&mut self, f: impl FnOnce() -> Bg + Send + 'static) {
        if cfg!(any(target_os = "macos", target_os = "windows")) {
            let r = f();
            self.handle_bg(r);
        } else {
            self.dialog_open = true;
            let tx = self.bg_tx.clone();
            let ctx = self.ctx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(f());
                ctx.request_repaint();
            });
        }
    }

    fn export_to(&mut self, mut path: PathBuf) {
        let Some(od) = &self.doc else { return };
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase);
        if !matches!(ext.as_deref(), Some("png" | "jpg" | "jpeg")) {
            path.set_extension("png");
        }
        let image = od.image.clone();
        let anns = od.doc.annotations.clone();
        let tx = self.bg_tx.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let res = crate::export::export_file(&image, &anns, &path).map(|_| path);
            let _ = tx.send(Bg::Exported(res));
            ctx.request_repaint();
        });
    }

    /// Renders the image to a temp PNG and hands it to the OS drag session.
    fn start_drag_out(&mut self, frame: &eframe::Frame) {
        self.finish_interactions();
        let Some(od) = &self.doc else { return };
        let name = format!("{}.png", export_stem(od));
        let t0 = Instant::now();
        let res = dragout::prepare(&od.image, &od.doc.annotations, &name).and_then(|p| {
            if self.trace {
                let ms = t0.elapsed().as_secs_f64() * 1000.0;
                eprintln!("[markly] drag-out render {ms:.1} ms");
            }
            dragout::start(frame, p, self.drag_out_ended.clone(), self.ctx.clone())
        });
        if let Err(e) = res {
            self.toast_error(format!("Couldn't start drag: {e}"));
        }
    }

    // -----------------------------------------------------------------
    // Background results
    // -----------------------------------------------------------------

    fn poll_background(&mut self) {
        while let Ok(msg) = self.bg_rx.try_recv() {
            self.handle_bg(msg);
        }
        while let Ok(ev) = self.clip_rx.try_recv() {
            match ev {
                ClipEvent::Pasted(Ok(PasteContent::Image(img))) => {
                    self.start_load(LoadSource::Pixels(img), "Pasting…")
                }
                ClipEvent::Pasted(Ok(PasteContent::File(p))) => self.open_path(p),
                ClipEvent::Pasted(Err(e)) => self.toast_info(e),
                ClipEvent::Copied(Ok((w, h))) => self.toast_info(format!("Copied {w} × {h} image")),
                ClipEvent::Copied(Err(e)) => self.toast_error(e),
            }
        }
        while let Ok(ev) = self.writer_rx.try_recv() {
            match ev {
                WriterEvent::ThumbnailWritten(id) => self.history.invalidate_thumb(&id),
                WriterEvent::HistoryChanged => self.history.mark_stale(),
                WriterEvent::Error(e) => self.toast_error(e),
            }
        }
    }

    fn handle_bg(&mut self, msg: Bg) {
        match msg {
            Bg::Loaded { token, result } => {
                if token != self.load_token {
                    return; // superseded by a newer load
                }
                match result {
                    Ok(l) => self.install_loaded(*l),
                    Err(e) => {
                        self.loading = None;
                        self.toast_error(e);
                    }
                }
            }
            Bg::OpenPicked(p) => {
                self.dialog_open = false;
                if let Some(p) = p {
                    self.open_path(p);
                }
            }
            Bg::ExportPicked(p) => {
                self.dialog_open = false;
                if let Some(p) = p {
                    self.export_to(p);
                }
            }
            Bg::Exported(Ok(p)) => self.toast_info(format!("Exported {}", file_label(&p))),
            Bg::Exported(Err(e)) => self.toast_error(format!("Export failed: {e}")),
            Bg::HistoryListed(entries) => self.history.set_entries(entries),
            Bg::Thumbnail { id, image } => self.history.set_thumb(&self.ctx, id, image),
        }
    }

    // -----------------------------------------------------------------
    // Input
    // -----------------------------------------------------------------

    fn text_input_active(&self) -> bool {
        self.editing.is_some()
    }

    fn handle_input(&mut self) {
        let ctx = self.ctx.clone();

        // Dropped files.
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .collect()
        });
        // Our own drag-out released back over the window: nothing to open.
        let dropped_is_empty = dropped.is_empty();
        let own_drag = !dropped_is_empty && dropped.iter().all(|p| dragout::is_own_file(p));
        if own_drag {
            // Ignore.
        } else if let Some(p) = dropped
            .into_iter()
            .find(|p| clipboard::is_supported_image_path(p))
        {
            self.open_path(p);
        } else if !dropped_is_empty {
            self.toast_info("Drop a PNG, JPEG or WebP image");
        }

        let text_active = self.text_input_active();

        // Clipboard shortcuts arrive as events; leave them to text fields
        // while one is focused.
        if !text_active {
            let mut copy = false;
            let mut paste_text: Option<String> = None;
            let mut paste_image: Option<Arc<ColorImage>> = None;
            ctx.input_mut(|i| {
                i.events.retain(|e| match e {
                    egui::Event::Copy => {
                        copy = true;
                        false
                    }
                    egui::Event::Paste(t) => {
                        paste_text = Some(t.clone());
                        false
                    }
                    egui::Event::PasteImage(img) => {
                        paste_image = Some(img.clone());
                        false
                    }
                    _ => true,
                })
            });
            if copy {
                self.copy_to_clipboard();
            }
            if let Some(img) = paste_image {
                self.handle_paste_image(&img);
            } else if let Some(t) = paste_text {
                match clipboard::path_from_text(&t) {
                    Some(p) => self.open_path(p),
                    // Text may accompany an image (e.g. from a browser).
                    None => self.paste_requested(),
                }
            }
        }

        let sc = |m: Modifiers, k: Key| KeyboardShortcut::new(m, k);
        let cmd_shift = Modifiers::COMMAND | Modifiers::SHIFT;
        let pressed = |s: KeyboardShortcut| ctx.input_mut(|i| i.consume_shortcut(&s));

        if pressed(sc(Modifiers::COMMAND, Key::O)) {
            self.pick_open_file();
        }
        if pressed(sc(cmd_shift, Key::S)) || pressed(sc(Modifiers::COMMAND, Key::S)) {
            self.pick_export_file();
        }
        if pressed(sc(Modifiers::COMMAND, Key::Num0)) {
            self.fit_mode = true;
        }
        if pressed(sc(Modifiers::COMMAND, Key::Num1)) {
            self.zoom_actual_size();
        }
        if pressed(sc(Modifiers::COMMAND, Key::Equals)) || pressed(sc(Modifiers::COMMAND, Key::Plus)) {
            self.zoom_step(1.25);
        }
        if pressed(sc(Modifiers::COMMAND, Key::Minus)) {
            self.zoom_step(0.8);
        }

        // Text formatting: while typing, or with the Text tool / a label selected.
        if text_active || self.style_target() == Some(Tool::Text) {
            if pressed(sc(Modifiers::COMMAND, Key::B)) {
                self.toggle_text_format(|f| f.bold = !f.bold);
            }
            if pressed(sc(Modifiers::COMMAND, Key::I)) {
                self.toggle_text_format(|f| f.italic = !f.italic);
            }
            if pressed(sc(Modifiers::COMMAND, Key::U)) {
                self.toggle_text_format(|f| f.underline = !f.underline);
            }
            if pressed(sc(cmd_shift, Key::X)) {
                self.toggle_text_format(|f| f.strikethrough = !f.strikethrough);
            }
        }

        if text_active {
            return;
        }
        if pressed(sc(cmd_shift, Key::Z)) || pressed(sc(Modifiers::COMMAND, Key::Y)) {
            self.redo();
        }
        if pressed(sc(Modifiers::COMMAND, Key::Z)) {
            self.undo();
        }
        if pressed(sc(Modifiers::NONE, Key::Delete)) || pressed(sc(Modifiers::NONE, Key::Backspace)) {
            self.delete_selection();
        }
        if pressed(sc(Modifiers::NONE, Key::Escape)) {
            if self.drag.is_some() {
                self.cancel_drag();
            } else {
                self.selection = None;
            }
        }
        for (key, d) in [
            (Key::ArrowLeft, Pt::new(-1.0, 0.0)),
            (Key::ArrowRight, Pt::new(1.0, 0.0)),
            (Key::ArrowUp, Pt::new(0.0, -1.0)),
            (Key::ArrowDown, Pt::new(0.0, 1.0)),
        ] {
            if self.selection.is_some() {
                if pressed(sc(Modifiers::SHIFT, key)) {
                    self.nudge_selection(d * 10.0);
                } else if pressed(sc(Modifiers::NONE, key)) {
                    self.nudge_selection(d);
                }
            }
        }
        let tool_keys = [
            (Key::V, Tool::Select),
            (Key::A, Tool::Arrow),
            (Key::C, Tool::CurvedArrow),
            (Key::R, Tool::Rectangle),
            (Key::E, Tool::Ellipse),
            (Key::T, Tool::Text),
            (Key::P, Tool::Freehand),
            (Key::X, Tool::Pixelate),
        ];
        for (key, tool) in tool_keys {
            if pressed(sc(Modifiers::NONE, key)) {
                self.set_tool(tool);
            }
        }
        if pressed(sc(Modifiers::NONE, Key::H)) {
            self.toggle_history();
        }
    }

    fn toggle_history(&mut self) {
        self.settings.history_open = !self.settings.history_open;
        self.history.confirm_clear = false;
        self.persist_settings();
    }

    fn zoom_step(&mut self, factor: f32) {
        if self.doc.is_none() {
            return;
        }
        let size = self.view_canvas_size();
        self.view.zoom_about(size * 0.5, factor);
        self.fit_mode = false;
    }

    fn zoom_actual_size(&mut self) {
        let Some(od) = &self.doc else { return };
        let (w, h) = (od.doc.width as f32, od.doc.height as f32);
        let size = self.view_canvas_size();
        self.view.zoom = self.view.actual_size_zoom();
        self.view.center(w, h, size);
        self.fit_mode = false;
    }

    fn view_canvas_size(&self) -> Pt {
        self.canvas_size
    }

    // -----------------------------------------------------------------
    // Toasts
    // -----------------------------------------------------------------

    fn toast_info(&mut self, text: impl Into<String>) {
        self.toast = Some(Toast {
            text: text.into(),
            kind: ToastKind::Info,
            until: Instant::now() + Duration::from_millis(2200),
        });
        self.ctx.request_repaint();
    }

    fn toast_error(&mut self, text: impl Into<String>) {
        self.toast = Some(Toast {
            text: text.into(),
            kind: ToastKind::Error,
            until: Instant::now() + Duration::from_millis(4500),
        });
        self.ctx.request_repaint();
    }

    fn show_toast(&mut self) {
        let Some(t) = &self.toast else { return };
        let now = Instant::now();
        if now >= t.until {
            self.toast = None;
            return;
        }
        self.ctx.request_repaint_after(t.until - now);
        let screen = self.ctx.content_rect();
        let dark = self.ctx.global_style().visuals.dark_mode;
        let (bg, fg) = match (t.kind, dark) {
            (ToastKind::Error, _) => (Color32::from_rgb(190, 45, 45), Color32::WHITE),
            (ToastKind::Info, true) => (Color32::from_rgb(235, 236, 240), Color32::from_rgb(25, 26, 30)),
            (ToastKind::Info, false) => (Color32::from_rgb(36, 38, 44), Color32::WHITE),
        };
        let text = t.text.clone();
        egui::Area::new(egui::Id::unique("markly-toast"))
            .order(egui::Order::Tooltip)
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -22.0))
            .interactable(false)
            .constrain_to(screen)
            .show(&self.ctx, |ui| {
                egui::Frame::new()
                    .fill(bg)
                    .corner_radius(egui::CornerRadius::same(18))
                    .inner_margin(egui::Margin::symmetric(16, 9))
                    .shadow(egui::Shadow {
                        offset: [0, 4],
                        blur: 16,
                        spread: 0,
                        color: Color32::from_black_alpha(60),
                    })
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new(text).color(fg).size(13.5));
                    });
            });
    }
}

impl eframe::App for MarklyApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if self.applied_theme != Some(self.settings.theme) {
            theme::apply(&self.ctx, self.settings.theme);
            self.applied_theme = Some(self.settings.theme);
        }
        self.poll_background();
        self.handle_input();
        // Window size in points (inner_rect is unavailable on Wayland).
        let r = self.ctx.viewport_rect();
        if r.width() > 0.0 && r.height() > 0.0 {
            self.window_size = Some([r.width(), r.height()]);
        }

        let dark = ui.visuals().dark_mode;
        let pal = theme::palette(dark);
        let bar_frame = egui::Frame::new()
            .fill(pal.panel)
            .inner_margin(egui::Margin::symmetric(8, 0));

        egui::Panel::top("markly-top")
            .exact_size(46.0)
            .resizable(false)
            .frame(bar_frame)
            .show(ui, |ui| self.top_bar(ui, &pal));
        if std::mem::take(&mut self.drag_out_requested) {
            self.start_drag_out(frame);
        }
        egui::Panel::left("markly-tools")
            .exact_size(64.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(pal.panel)
                    .inner_margin(egui::Margin::symmetric(0, 8)),
            )
            .show(ui, |ui| self.tool_rail(ui, &pal));
        if self.settings.history_open {
            egui::Panel::right("markly-history")
                .default_size(250.0)
                .size_range(200.0..=420.0)
                .resizable(true)
                .frame(
                    egui::Frame::new()
                        .fill(pal.panel)
                        .inner_margin(egui::Margin::same(10)),
                )
                .show(ui, |ui| self.history_panel(ui, &pal));
        }
        egui::CentralPanel::no_frame().show(ui, |ui| self.canvas(ui, &pal));

        self.show_toast();
        self.autosave_tick();

        if !self.first_frame_done {
            self.first_frame_done = true;
            if self.trace
                && let Some(t0) = PROCESS_START.get()
            {
                eprintln!(
                    "[markly] first frame at {:.1} ms",
                    t0.elapsed().as_secs_f64() * 1000.0
                );
            }
        }
        if self.bench_exit && self.loading.is_none() {
            self.ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.debug_screenshot_tick();
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        dragout::release_pointer_if_ended(&self.drag_out_ended, ctx, raw);
        let Some(s) = &mut self.script else { return };
        if self.loading.is_some() || self.dialog_open {
            ctx.request_repaint();
            return;
        }
        match s.feed(ctx, raw) {
            Some(script::Step::Screenshot(path)) => save_screenshot(ctx, path, false),
            Some(script::Step::Quit) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            _ => {}
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.shutdown();
    }
}

impl MarklyApp {
    fn debug_screenshot_tick(&mut self) {
        let Some((path, frames)) = &mut self.debug_shot else {
            return;
        };
        if self.loading.is_some() {
            self.ctx.request_repaint();
            return;
        }
        *frames += 1;
        if *frames == 6 {
            save_screenshot(&self.ctx, path.clone(), true);
        }
        if *frames < 30 {
            self.ctx.request_repaint();
        }
    }
}

/// egui only learns the GPU texture limit after the first frame, but the
/// previous document starts loading before that; ask GL directly.
fn query_max_texture_side(cc: &eframe::CreationContext<'_>) -> usize {
    use eframe::glow::HasContext as _;
    cc.gl
        .as_ref()
        // SAFETY: plain state query on the context eframe just created.
        .map(|gl| unsafe { gl.get_parameter_i32(eframe::glow::MAX_TEXTURE_SIZE) })
        .filter(|&v| v > 0)
        .map_or(8192, |v| v as usize)
}

fn save_screenshot(ctx: &egui::Context, path: PathBuf, close: bool) {
    let c = ctx.clone();
    ctx.request_screenshot(move |img| {
        let [w, h] = img.size;
        let raw: Vec<u8> = img.pixels.iter().flat_map(|p| p.to_array()).collect();
        if let Some(rgba) = RgbaImage::from_raw(w as u32, h as u32, raw)
            && let Err(e) = rgba.save(&path)
        {
            eprintln!("markly: screenshot failed: {e}");
        }
        if close {
            c.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        c.request_repaint();
    });
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    use_system_ui_font(&mut fonts);
    // Fall back to egui's fonts (e.g. emoji) for missing glyphs.
    let fallback = fonts
        .families
        .get(&egui::FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    // One egui family per bold/italic variant of the annotation font.
    for (name, bytes) in text::all_fonts() {
        fonts
            .font_data
            .insert((*name).to_owned(), Arc::new(egui::FontData::from_static(bytes)));
        let mut family = vec![(*name).to_owned()];
        family.extend(fallback.iter().cloned());
        fonts
            .families
            .insert(egui::FontFamily::Name((*name).into()), family);
    }
    add_system_symbol_fallback(&mut fonts);
    ctx.set_fonts(fonts);
}

/// egui's default UI font (Ubuntu Light) has thin stems that render soft and
/// uneven on macOS. Use the system font there (read at runtime, not bundled)
/// so the UI text matches native apps.
fn use_system_ui_font(fonts: &mut egui::FontDefinitions) {
    if !cfg!(target_os = "macos") {
        return;
    }
    const NAME: &str = "system-ui";
    let Ok(bytes) = std::fs::read("/System/Library/Fonts/SFNS.ttf") else {
        return;
    };
    fonts
        .font_data
        .insert(NAME.to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
    if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        family.insert(0, NAME.to_owned());
    }
}

/// Neither egui's bundled fonts nor Noto Sans have the macOS key symbols
/// (⌘ ⌥ ⇧ ⌃), which then render as tofu boxes. Append Apple Symbols, which
/// ships with every macOS, as a last-resort fallback for UI text.
fn add_system_symbol_fallback(fonts: &mut egui::FontDefinitions) {
    if !cfg!(target_os = "macos") {
        return;
    }
    const NAME: &str = "system-symbols";
    let Ok(bytes) = std::fs::read("/System/Library/Fonts/Apple Symbols.ttf") else {
        return;
    };
    fonts
        .font_data
        .insert(NAME.to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
    for family in fonts.families.values_mut() {
        family.push(NAME.to_owned());
    }
}

/// File name stem for an export of `od`: `<source>-annotated` or `annotated-<id>`.
fn export_stem(od: &OpenDoc) -> String {
    od.doc
        .source_path
        .as_ref()
        .and_then(|p| Path::new(p).file_stem())
        .map(|s| format!("{}-annotated", s.to_string_lossy()))
        .unwrap_or_else(|| format!("annotated-{}", od.doc.id.split('-').next().unwrap_or("image")))
}

fn file_label(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.display().to_string())
}

/// Background part of opening a document: read, decode, prepare texture
/// and thumbnail base.
fn load_document(source: LoadSource, store: Option<&Store>, max_tex: usize) -> Result<Loaded, String> {
    let (doc, image, is_new, original) = match source {
        LoadSource::Stored(doc) => {
            let store = store.ok_or("storage unavailable")?;
            let bytes =
                std::fs::read(store.original_path(&doc)).map_err(|e| format!("Could not read image: {e}"))?;
            let img = store::decode_image(&bytes)?;
            let mut doc = *doc;
            // Trust the pixels over the metadata if they disagree.
            (doc.width, doc.height) = img.dimensions();
            (doc, img, false, None)
        }
        LoadSource::Pixels(img) => {
            if img.width() == 0 || img.height() == 0 {
                return Err("The clipboard image is empty".into());
            }
            let doc = Document::new(img.width(), img.height(), "original.png", None);
            let shared = Arc::new(img);
            return finish_load(
                doc,
                shared.clone(),
                true,
                Some(OriginalData::Pixels(shared)),
                max_tex,
            );
        }
        LoadSource::File(path) => {
            let bytes =
                std::fs::read(&path).map_err(|e| format!("Could not open {}: {e}", file_label(&path)))?;
            let img = store::decode_image(&bytes)
                .map_err(|e| format!("Could not open {}: {e}", file_label(&path)))?;
            let name = store::original_name_for(&bytes).unwrap_or("original.png");
            let source = std::fs::canonicalize(&path)
                .unwrap_or(path)
                .to_string_lossy()
                .into_owned();
            let doc = Document::new(img.width(), img.height(), name, Some(source));
            (doc, img, true, Some(OriginalData::Encoded(bytes)))
        }
    };
    finish_load(doc, Arc::new(image), is_new, original, max_tex)
}

fn finish_load(
    doc: Document,
    image: Arc<RgbaImage>,
    is_new: bool,
    original: Option<OriginalData>,
    max_tex: usize,
) -> Result<Loaded, String> {
    let (w, h) = image.dimensions();
    let has_alpha = image.as_raw().as_chunks::<4>().0.iter().any(|p| p[3] != 255);
    let longest = w.max(h) as usize;
    // Opaque pixels need no premultiplication; that path is a plain copy.
    let to_color_image = |size: [usize; 2], raw: &[u8]| {
        if has_alpha {
            ColorImage::from_rgba_unmultiplied(size, raw)
        } else {
            ColorImage::from_rgba_premultiplied(size, raw)
        }
    };
    // Texture conversion and thumbnail downscale run in parallel.
    let ((texture, tex_scale), (thumb, thumb_scale)) = std::thread::scope(|sc| {
        let thumb = sc.spawn(|| store::thumbnail_base(&image));
        let tex = if longest > max_tex {
            let s = max_tex as f32 / longest as f32;
            let (tw, th) = (((w as f32 * s) as u32).max(1), ((h as f32 * s) as u32).max(1));
            let small = image::imageops::resize(&*image, tw, th, image::imageops::FilterType::Triangle);
            (
                to_color_image([tw as usize, th as usize], small.as_raw()),
                tw as f32 / w as f32,
            )
        } else {
            (to_color_image([w as usize, h as usize], image.as_raw()), 1.0)
        };
        (tex, thumb.join().expect("thumbnail thread"))
    });
    Ok(Loaded {
        doc,
        is_new,
        image,
        texture,
        tex_scale,
        has_alpha,
        thumb_base: Arc::new(thumb),
        thumb_scale,
        original,
    })
}
