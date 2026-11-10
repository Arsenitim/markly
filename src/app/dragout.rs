//! Drag the annotated image out of the window as a PNG file, straight onto
//! the desktop or into another app (macOS and Windows only: on Linux the
//! drag-source APIs need a GTK window, which winit does not provide).

use crate::model::Annotation;
use eframe::egui;
use image::RgbaImage;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) const SUPPORTED: bool = cfg!(any(target_os = "macos", target_os = "windows"));

/// Longest side of the image shown under the cursor while dragging, in px.
const PREVIEW_SIDE: u32 = 160;

/// A rendered export, ready to hand to the OS drag session.
#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
pub(super) struct Prepared {
    pub path: PathBuf,
    pub preview_png: Vec<u8>,
}

fn drag_dir() -> PathBuf {
    std::env::temp_dir().join("skrib-drag")
}

/// Whether `path` is a file this module wrote for a drag.
pub(super) fn is_own_file(path: &Path) -> bool {
    let dir = drag_dir();
    path.starts_with(&dir)
        || std::fs::canonicalize(path)
            .is_ok_and(|p| std::fs::canonicalize(&dir).is_ok_and(|d| p.starts_with(d)))
}

/// Renders the image to `<temp>/skrib-drag/<file_name>`. Earlier drag files
/// are removed first: the receiving app has copied them by now.
pub(super) fn prepare(image: &RgbaImage, anns: &[Annotation], file_name: &str) -> Result<Prepared, String> {
    let dir = drag_dir();
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(file_name);

    let flat = crate::render::flatten(image, anns);
    write_png(&path, &flat)?;

    let thumb = image::imageops::thumbnail(
        &flat,
        scaled(flat.width(), flat.width(), flat.height()),
        scaled(flat.height(), flat.width(), flat.height()),
    );
    let mut preview_png = Vec::new();
    encode_png(&mut preview_png, &thumb)?;
    Ok(Prepared { path, preview_png })
}

fn scaled(v: u32, w: u32, h: u32) -> u32 {
    let longest = w.max(h).max(1);
    if longest <= PREVIEW_SIDE {
        v.max(1)
    } else {
        ((v as u64 * PREVIEW_SIDE as u64) / longest as u64).max(1) as u32
    }
}

fn encode_png(out: &mut Vec<u8>, img: &RgbaImage) -> Result<(), String> {
    use image::ImageEncoder;
    image::codecs::png::PngEncoder::new(out)
        .write_image(
            img.as_raw(),
            img.width(),
            img.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| e.to_string())
}

fn write_png(path: &Path, img: &RgbaImage) -> Result<(), String> {
    let mut bytes = Vec::new();
    encode_png(&mut bytes, img)?;
    std::fs::write(path, bytes).map_err(|e| e.to_string())
}

/// Starts the OS drag session. Must run while the mouse button is still
/// down. `ended` is set when the session finishes, so the app can release
/// the button egui never saw go up (the OS drag loop swallows it).
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(super) fn start(
    frame: &eframe::Frame,
    prepared: Prepared,
    ended: Arc<AtomicBool>,
    ctx: egui::Context,
) -> Result<(), String> {
    drag::start_drag(
        frame,
        drag::DragItem::Files(vec![prepared.path]),
        drag::Image::Raw(prepared.preview_png),
        move |_result, _pos| {
            ended.store(true, Ordering::SeqCst);
            ctx.request_repaint();
        },
        drag::Options::default(),
    )
    .map_err(|e| e.to_string())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(super) fn start(
    _frame: &eframe::Frame,
    _prepared: Prepared,
    _ended: Arc<AtomicBool>,
    _ctx: egui::Context,
) -> Result<(), String> {
    Err("Dragging out is not supported on this platform".into())
}

/// After a drag session, tells egui the primary button is up again.
pub(super) fn release_pointer_if_ended(ended: &AtomicBool, ctx: &egui::Context, raw: &mut egui::RawInput) {
    if !ended.swap(false, Ordering::SeqCst) {
        return;
    }
    let pos = ctx.input(|i| i.pointer.latest_pos()).unwrap_or(egui::Pos2::ZERO);
    raw.events.push(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: ctx.input(|i| i.modifiers),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_keeps_aspect_and_never_upscales() {
        assert_eq!((scaled(2000, 2000, 1000), scaled(1000, 2000, 1000)), (160, 80));
        assert_eq!((scaled(100, 100, 50), scaled(50, 100, 50)), (100, 50));
        assert_eq!(scaled(1, 10_000, 1), 1);
    }

    #[test]
    fn prepare_writes_png_and_preview() {
        let img = RgbaImage::from_pixel(400, 200, image::Rgba([10, 20, 30, 255]));
        let p = prepare(&img, &[], "dragout-test.png").unwrap();
        let written = image::open(&p.path).unwrap();
        assert_eq!((written.width(), written.height()), (400, 200));
        let preview = image::load_from_memory(&p.preview_png).unwrap();
        assert_eq!((preview.width(), preview.height()), (160, 80));
    }
}
