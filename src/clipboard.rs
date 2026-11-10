//! System clipboard access on a dedicated worker thread.
//!
//! Reading/writing large images can take tens of milliseconds (and on
//! Linux involves IPC with the display server), so the UI thread only sends
//! requests and receives results through a channel.
//!
//! Copied images are served by this process; on Linux (X11/Wayland) they
//! disappear from the clipboard when Skrib quits unless a clipboard manager
//! keeps them. That is intentional: paste before quitting.

use crate::model::Annotation;
use crate::render;
use image::RgbaImage;
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};

#[derive(Debug)]
pub enum PasteContent {
    Image(RgbaImage),
    /// A copied file (e.g. from a file manager) to open.
    File(PathBuf),
}

#[derive(Debug)]
pub enum ClipEvent {
    Pasted(Result<PasteContent, String>),
    Copied(Result<(u32, u32), String>),
}

enum Request {
    Paste,
    Copy {
        image: Arc<RgbaImage>,
        annotations: Vec<Annotation>,
    },
}

pub struct ClipboardWorker {
    tx: Sender<Request>,
}

impl ClipboardWorker {
    pub fn spawn(events: Sender<ClipEvent>, notify: impl Fn() + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("skrib-clipboard".into())
            .spawn(move || worker(rx, events, notify))
            .expect("spawn clipboard thread");
        Self { tx }
    }

    pub fn paste(&self) {
        let _ = self.tx.send(Request::Paste);
    }

    /// Flattens on the worker and puts the result on the clipboard.
    pub fn copy(&self, image: Arc<RgbaImage>, annotations: Vec<Annotation>) {
        let _ = self.tx.send(Request::Copy { image, annotations });
    }
}

fn to_image(d: arboard::ImageData<'_>) -> Option<RgbaImage> {
    RgbaImage::from_raw(d.width as u32, d.height as u32, d.bytes.into_owned())
}

const IMAGE_EXTS: [&str; 5] = ["png", "jpg", "jpeg", "webp", "jfif"];

pub fn is_supported_image_path(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| IMAGE_EXTS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Interprets clipboard text as a path or `file://` URI to an image.
pub fn path_from_text(text: &str) -> Option<PathBuf> {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'))?;
    let path = if let Some(rest) = line.strip_prefix("file://") {
        // Strip an optional host component and percent-decode.
        let rest = rest.strip_prefix("localhost").unwrap_or(rest);
        PathBuf::from(percent_decode(rest))
    } else {
        PathBuf::from(line)
    };
    (path.is_absolute() && is_supported_image_path(&path) && path.is_file()).then_some(path)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16)
        {
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn worker(rx: Receiver<Request>, events: Sender<ClipEvent>, notify: impl Fn()) {
    let mut clipboard: Option<arboard::Clipboard> = None;
    let emit = |e: ClipEvent| {
        let _ = events.send(e);
        notify();
    };
    while let Ok(req) = rx.recv() {
        if clipboard.is_none() {
            match arboard::Clipboard::new() {
                Ok(c) => clipboard = Some(c),
                Err(e) => {
                    let msg = format!("Clipboard unavailable: {e}");
                    match req {
                        Request::Paste => emit(ClipEvent::Pasted(Err(msg))),
                        Request::Copy { .. } => emit(ClipEvent::Copied(Err(msg))),
                    }
                    continue;
                }
            }
        }
        let cb = clipboard.as_mut().unwrap();
        match req {
            Request::Paste => {
                let result = match cb.get_image().ok().and_then(to_image) {
                    Some(img) => Ok(PasteContent::Image(img)),
                    None => {
                        let file = cb
                            .get()
                            .file_list()
                            .ok()
                            .and_then(|l| l.into_iter().find(|p| is_supported_image_path(p) && p.is_file()))
                            .or_else(|| cb.get_text().ok().and_then(|t| path_from_text(&t)));
                        file.map(PasteContent::File)
                            .ok_or_else(|| "The clipboard does not contain an image".to_string())
                    }
                };
                emit(ClipEvent::Pasted(result));
            }
            Request::Copy { image, annotations } => {
                let flat = render::flatten(&image, &annotations);
                let (w, h) = flat.dimensions();
                let data = arboard::ImageData {
                    width: w as usize,
                    height: h as usize,
                    bytes: Cow::Owned(flat.into_raw()),
                };
                let res = cb
                    .set_image(data)
                    .map(|_| (w, h))
                    .map_err(|e| format!("Copy failed: {e}"));
                emit(ClipEvent::Copied(res));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_and_path_parsing() {
        let dir = std::env::temp_dir().join(format!("skrib clip {}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("shot 1.png");
        std::fs::write(&file, b"x").unwrap();
        let uri = format!("file://{}", file.to_string_lossy().replace(' ', "%20"));
        if cfg!(unix) {
            assert_eq!(path_from_text(&uri), Some(file.clone()));
            assert_eq!(
                path_from_text(&format!("# comment\n{}\n", file.display())),
                Some(file.clone())
            );
        }
        assert_eq!(path_from_text("hello world"), None);
        assert_eq!(path_from_text("/definitely/missing.png"), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn percent_decoding() {
        assert_eq!(percent_decode("/a%20b/%E2%9C%93.png"), "/a b/✓.png");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
    }
}
