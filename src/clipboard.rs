//! System clipboard access on a dedicated worker thread.
//!
//! Reading/writing large images can take tens of milliseconds (and on
//! Linux involves IPC with the display server), so the UI thread only sends
//! requests and receives results through a channel.
//!
//! ## Linux clipboard persistence
//! X11 and Wayland clipboards are served by the *owning process*: when it
//! exits, the content disappears unless a clipboard manager has copied it.
//! On exit, if our flattened image still owns the clipboard, we hand it to a
//! short-lived child process (`markly --hold-clipboard <file>`) that keeps
//! serving it **only until something else is copied**, then exits — the
//! same approach `xclip`/`wl-copy` use. There is no long-running daemon.

use crate::model::Annotation;
use crate::render;
use image::RgbaImage;
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

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
    Handoff {
        dir: PathBuf,
        done: Sender<bool>,
    },
}

pub struct ClipboardWorker {
    tx: Sender<Request>,
}

impl ClipboardWorker {
    pub fn spawn(events: Sender<ClipEvent>, notify: impl Fn() + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("markly-clipboard".into())
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

    /// Linux: keep our copied image available after exit (see module docs).
    /// Returns whether a holder process was started. No-op elsewhere.
    pub fn handoff_on_exit(&self, dir: &Path, timeout: Duration) -> bool {
        if !cfg!(all(unix, not(target_os = "macos"))) {
            return false;
        }
        let (done, rx) = mpsc::channel();
        if self
            .tx
            .send(Request::Handoff {
                dir: dir.to_path_buf(),
                done,
            })
            .is_err()
        {
            return false;
        }
        rx.recv_timeout(timeout).unwrap_or(false)
    }
}

fn to_image(d: arboard::ImageData<'_>) -> Option<RgbaImage> {
    RgbaImage::from_raw(d.width as u32, d.height as u32, d.bytes.into_owned())
}

/// Cheap content fingerprint (FNV-1a over 8-byte words).
fn fingerprint(img: &RgbaImage) -> (u32, u32, u64) {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let (words, rest) = img.as_raw().as_chunks::<8>();
    for w in words {
        h = (h ^ u64::from_le_bytes(*w)).wrapping_mul(0x100_0000_01b3);
    }
    for b in rest {
        h = (h ^ *b as u64).wrapping_mul(0x100_0000_01b3);
    }
    (img.width(), img.height(), h)
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
    let mut last_copy: Option<(u32, u32, u64)> = None;
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
                        Request::Handoff { done, .. } => {
                            let _ = done.send(false);
                        }
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
                let fp = fingerprint(&flat);
                let data = arboard::ImageData {
                    width: w as usize,
                    height: h as usize,
                    bytes: Cow::Owned(flat.into_raw()),
                };
                let res = cb
                    .set_image(data)
                    .map(|_| (w, h))
                    .map_err(|e| format!("Copy failed: {e}"));
                if res.is_ok() {
                    last_copy = Some(fp);
                }
                emit(ClipEvent::Copied(res));
            }
            Request::Handoff { dir, done } => {
                let ok = last_copy.is_some_and(|fp| {
                    // Only hand off if our image is still what's on the
                    // clipboard; never clobber something copied later.
                    cb.get_image()
                        .ok()
                        .and_then(to_image)
                        .filter(|img| fingerprint(img) == fp)
                        .is_some_and(|img| spawn_holder(&dir, &img).is_ok())
                });
                let _ = done.send(ok);
            }
        }
    }
}

const HANDOFF_MAGIC: &[u8; 8] = b"MRKLYCB1";

fn spawn_holder(dir: &Path, img: &RgbaImage) -> std::io::Result<()> {
    let path = dir.join(format!("clipboard-{}.raw", std::process::id()));
    let mut buf = Vec::with_capacity(16 + img.as_raw().len());
    buf.extend_from_slice(HANDOFF_MAGIC);
    buf.extend_from_slice(&img.width().to_le_bytes());
    buf.extend_from_slice(&img.height().to_le_bytes());
    buf.extend_from_slice(img.as_raw());
    std::fs::write(&path, buf)?;
    let exe = std::env::current_exe()?;
    let spawned = std::process::Command::new(exe)
        .arg("--hold-clipboard")
        .arg(&path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    if spawned.is_err() {
        let _ = std::fs::remove_file(&path);
    }
    spawned.map(|_| ())
}

/// Entry point of the `--hold-clipboard` child process. Serves the image
/// until another application takes clipboard ownership, then returns.
pub fn hold_clipboard(path: &Path) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(path);
    if bytes.len() < 16 || &bytes[..8] != HANDOFF_MAGIC {
        return Err("bad handoff file".into());
    }
    let w = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let h = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let px = &bytes[16..];
    if px.len() != w * h * 4 {
        return Err("bad handoff size".into());
    }
    let mut cb = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    let data = arboard::ImageData {
        width: w,
        height: h,
        bytes: Cow::Borrowed(px),
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        use arboard::SetExtLinux;
        cb.set().wait().image(data).map_err(|e| e.to_string())
    }
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    {
        cb.set_image(data).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_and_path_parsing() {
        let dir = std::env::temp_dir().join(format!("markly clip {}", std::process::id()));
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

    #[test]
    fn fingerprints_differ() {
        let a = RgbaImage::from_pixel(3, 3, image::Rgba([1, 2, 3, 4]));
        let mut b = a.clone();
        b.put_pixel(2, 2, image::Rgba([1, 2, 3, 5]));
        assert_ne!(fingerprint(&a), fingerprint(&b));
        assert_eq!(fingerprint(&a), fingerprint(&a.clone()));
    }
}
