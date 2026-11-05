//! Local, file-based persistence.
//!
//! ```text
//! <data dir>/
//!   settings.json
//!   documents/<document-id>/
//!     original.png | original.jpg | original.webp
//!     document.json
//!     thumbnail.png
//! ```
//!
//! All writes are atomic (write temp file → fsync → rename). Expensive work
//! (PNG encoding, thumbnail rendering, disk I/O) happens on a single
//! background writer thread fed by a channel, so the UI never blocks on it.

use crate::model::{Document, is_valid_document_id};
use crate::render;
use crate::settings::Settings;
use image::{ImageFormat, RgbaImage};
use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

pub const DOCUMENT_JSON: &str = "document.json";
pub const THUMBNAIL_PNG: &str = "thumbnail.png";
pub const THUMBNAIL_MAX: u32 = 320;
/// Oldest documents beyond this count are pruned from history.
pub const HISTORY_LIMIT: usize = 300;

#[derive(Clone, Debug)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    /// The platform data directory, overridable with `MARKLY_DATA_DIR`
    /// (used by tests and benchmarks).
    pub fn open_default() -> io::Result<Store> {
        let root = match std::env::var_os("MARKLY_DATA_DIR") {
            Some(p) => PathBuf::from(p),
            None => directories::ProjectDirs::from("", "", "Markly")
                .map(|d| d.data_dir().to_path_buf())
                .ok_or_else(|| io::Error::other("no home directory"))?,
        };
        Store::open(root)
    }

    pub fn open(root: PathBuf) -> io::Result<Store> {
        fs::create_dir_all(root.join("documents"))?;
        Ok(Store { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn settings_path(&self) -> PathBuf {
        self.root.join("settings.json")
    }
    pub fn documents_dir(&self) -> PathBuf {
        self.root.join("documents")
    }
    pub fn doc_dir(&self, id: &str) -> PathBuf {
        self.documents_dir().join(id)
    }

    pub fn load_settings(&self) -> Settings {
        fs::read(self.settings_path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save_settings(&self, s: &Settings) -> io::Result<()> {
        let json = serde_json::to_vec_pretty(s).map_err(io::Error::other)?;
        atomic_write(&self.settings_path(), &json)
    }

    /// Loads `document.json`. Fails if the file is missing or corrupt or the
    /// stored original image is missing.
    pub fn load_document(&self, id: &str) -> io::Result<Document> {
        if !is_valid_document_id(id) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid document id"));
        }
        let dir = self.doc_dir(id);
        let bytes = fs::read(dir.join(DOCUMENT_JSON))?;
        let mut doc: Document =
            serde_json::from_slice(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        if doc.id != id || !is_original_name(&doc.original_file) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "inconsistent document",
            ));
        }
        if !dir.join(&doc.original_file).is_file() {
            return Err(io::Error::new(io::ErrorKind::NotFound, "original image missing"));
        }
        doc.sanitize();
        Ok(doc)
    }

    pub fn original_path(&self, doc: &Document) -> PathBuf {
        self.doc_dir(&doc.id).join(&doc.original_file)
    }

    pub fn thumbnail_path(&self, id: &str) -> PathBuf {
        self.doc_dir(id).join(THUMBNAIL_PNG)
    }

    /// Lists history entries, newest first. Corrupt or incomplete documents
    /// are skipped (never deleted automatically).
    pub fn list_documents(&self) -> Vec<HistoryEntry> {
        let Ok(rd) = fs::read_dir(self.documents_dir()) else {
            return Vec::new();
        };
        let mut out: Vec<HistoryEntry> = rd
            .flatten()
            .filter_map(|e| {
                let id = e.file_name().to_str()?.to_string();
                let doc = self.load_document(&id).ok()?;
                Some(HistoryEntry {
                    id,
                    created_ms: doc.created_ms,
                    modified_ms: doc.modified_ms,
                    width: doc.width,
                    height: doc.height,
                    annotations: doc.annotations.len(),
                    source_name: doc
                        .source_path
                        .as_deref()
                        .and_then(|p| Path::new(p).file_name().map(|n| n.to_string_lossy().into_owned())),
                })
            })
            .collect();
        out.sort_by(|a, b| b.modified_ms.cmp(&a.modified_ms).then(b.id.cmp(&a.id)));
        out
    }

    pub fn delete_document(&self, id: &str) -> io::Result<()> {
        if !is_valid_document_id(id) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid document id"));
        }
        match fs::remove_dir_all(self.doc_dir(id)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    /// Deletes the oldest documents beyond `limit`, never touching `keep`.
    pub fn prune(&self, limit: usize, keep: Option<&str>) -> usize {
        let entries = self.list_documents();
        let mut removed = 0;
        for e in entries.iter().skip(limit) {
            if Some(e.id.as_str()) != keep && self.delete_document(&e.id).is_ok() {
                removed += 1;
            }
        }
        removed
    }

    /// Removes leftover `*.tmp` files from interrupted writes and empty
    /// document directories. Cheap; runs in the background at startup.
    pub fn cleanup(&self) {
        let Ok(rd) = fs::read_dir(self.documents_dir()) else {
            return;
        };
        for e in rd.flatten() {
            let dir = e.path();
            if let Ok(files) = fs::read_dir(&dir) {
                let mut any = false;
                for f in files.flatten() {
                    any = true;
                    if f.file_name().to_string_lossy().ends_with(".tmp") {
                        let _ = fs::remove_file(f.path());
                    }
                }
                if !any {
                    let _ = fs::remove_dir(&dir);
                }
            }
        }
    }
}

fn is_original_name(name: &str) -> bool {
    matches!(name, "original.png" | "original.jpg" | "original.webp")
}

#[derive(Clone, Debug, PartialEq)]
pub struct HistoryEntry {
    pub id: String,
    pub created_ms: u64,
    pub modified_ms: u64,
    pub width: u32,
    pub height: u32,
    pub annotations: usize,
    pub source_name: Option<String>,
}

/// Write `bytes` to `path` atomically: a crash leaves either the old or the
/// new file, never a truncated one.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::other("path has no parent"))?;
    fs::create_dir_all(dir)?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("path has no file name"))?;
    let tmp = dir.join(format!(".{}.{}.tmp", name.to_string_lossy(), std::process::id()));
    let result = (|| {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

// ---------------------------------------------------------------------------
// Image decoding / encoding helpers
// ---------------------------------------------------------------------------

/// Format of an image we can keep byte-for-byte as the document original.
pub fn original_name_for(bytes: &[u8]) -> Option<&'static str> {
    match image::guess_format(bytes).ok()? {
        ImageFormat::Png => Some("original.png"),
        ImageFormat::Jpeg => Some("original.jpg"),
        ImageFormat::WebP => Some("original.webp"),
        _ => None,
    }
}

/// Decodes PNG/JPEG/WebP bytes, applying EXIF orientation.
pub fn decode_image(bytes: &[u8]) -> Result<RgbaImage, String> {
    use image::ImageDecoder;
    let fmt = image::guess_format(bytes).map_err(|_| "Unsupported image format".to_string())?;
    if !matches!(fmt, ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP) {
        return Err(format!("Unsupported image format: {fmt:?}"));
    }
    let mut reader = image::ImageReader::with_format(io::Cursor::new(bytes), fmt);
    reader.no_limits();
    let mut decoder = reader.into_decoder().map_err(|e| e.to_string())?;
    let orientation = decoder.orientation().ok();
    let mut img = image::DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    if let Some(o) = orientation {
        img.apply_orientation(o);
    }
    Ok(img.into_rgba8())
}

/// Fast PNG encoding (used for originals and thumbnails).
pub fn encode_png(img: &RgbaImage) -> Result<Vec<u8>, String> {
    use image::ImageEncoder;
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    let mut out = Vec::with_capacity(img.as_raw().len() / 3);
    PngEncoder::new_with_quality(&mut out, CompressionType::Fast, FilterType::Adaptive)
        .write_image(
            img.as_raw(),
            img.width(),
            img.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| e.to_string())?;
    Ok(out)
}

/// Downscaled copy used as the base for thumbnails. The returned scale maps
/// image coordinates to thumbnail coordinates.
pub fn thumbnail_base(img: &RgbaImage) -> (RgbaImage, f32) {
    let (w, h) = img.dimensions();
    let scale = (THUMBNAIL_MAX as f32 / w.max(h).max(1) as f32).min(1.0);
    let tw = ((w as f32 * scale).round() as u32).max(1);
    let th = ((h as f32 * scale).round() as u32).max(1);
    if (tw, th) == (w, h) {
        return (img.clone(), 1.0);
    }
    let thumb = image::imageops::thumbnail(img, tw, th);
    (thumb, tw as f32 / w as f32)
}

// ---------------------------------------------------------------------------
// Background writer
// ---------------------------------------------------------------------------

pub enum OriginalData {
    /// Already-encoded bytes (e.g. the opened file) written verbatim.
    Encoded(Vec<u8>),
    /// Pixels to PNG-encode in the background.
    Pixels(Arc<RgbaImage>),
}

pub struct ThumbJob {
    pub base: Arc<RgbaImage>,
    pub scale: f32,
}

enum Job {
    Original {
        id: String,
        name: String,
        data: OriginalData,
    },
    Document {
        doc: Box<Document>,
        thumb: Option<ThumbJob>,
    },
    Settings(Box<Settings>),
    Delete(String),
    DeleteAll {
        keep: Option<String>,
    },
    Prune {
        keep: Option<String>,
    },
    Flush(Sender<()>),
}

/// Notifications from the writer back to the UI.
#[derive(Debug, Clone, PartialEq)]
pub enum WriterEvent {
    ThumbnailWritten(String),
    Error(String),
    HistoryChanged,
}

pub struct Writer {
    tx: Option<Sender<Job>>,
    handle: Option<JoinHandle<()>>,
}

impl Writer {
    /// `notify` is invoked after each event (e.g. to request a UI repaint).
    pub fn spawn(store: Store, events: Sender<WriterEvent>, notify: impl Fn() + Send + 'static) -> Writer {
        let (tx, rx) = mpsc::channel::<Job>();
        let handle = std::thread::Builder::new()
            .name("markly-writer".into())
            .spawn(move || writer_loop(store, rx, events, notify))
            .expect("spawn writer thread");
        Writer {
            tx: Some(tx),
            handle: Some(handle),
        }
    }

    fn send(&self, job: Job) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(job);
        }
    }

    pub fn save_original(&self, id: &str, name: &str, data: OriginalData) {
        self.send(Job::Original {
            id: id.into(),
            name: name.into(),
            data,
        });
    }
    pub fn save_document(&self, doc: Document, thumb: Option<ThumbJob>) {
        self.send(Job::Document {
            doc: Box::new(doc),
            thumb,
        });
    }
    pub fn save_settings(&self, s: Settings) {
        self.send(Job::Settings(Box::new(s)));
    }
    pub fn delete(&self, id: &str) {
        self.send(Job::Delete(id.into()));
    }
    pub fn delete_all(&self, keep: Option<String>) {
        self.send(Job::DeleteAll { keep });
    }
    pub fn prune(&self, keep: Option<String>) {
        self.send(Job::Prune { keep });
    }

    /// Blocks until every queued job is done, or `timeout` elapses.
    pub fn flush(&self, timeout: Duration) -> bool {
        let (tx, rx) = mpsc::channel();
        self.send(Job::Flush(tx));
        rx.recv_timeout(timeout).is_ok()
    }

    /// Flushes and stops the thread.
    pub fn shutdown(&mut self, timeout: Duration) -> bool {
        let done = self.flush(timeout);
        self.tx = None;
        if done && let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        done
    }
}

fn writer_loop(store: Store, rx: Receiver<Job>, events: Sender<WriterEvent>, notify: impl Fn()) {
    let emit = |e: WriterEvent| {
        let _ = events.send(e);
        notify();
    };
    while let Ok(first) = rx.recv() {
        // Drain the queue and coalesce document saves: only the latest
        // state of each document matters.
        let mut batch = vec![first];
        batch.extend(rx.try_iter());
        let mut latest: HashMap<String, usize> = HashMap::new();
        for (i, j) in batch.iter().enumerate() {
            if let Job::Document { doc, .. } = j {
                latest.insert(doc.id.clone(), i);
            }
        }
        for (i, job) in batch.into_iter().enumerate() {
            match job {
                Job::Original { id, name, data } => {
                    let bytes = match data {
                        OriginalData::Encoded(b) => Ok(b),
                        OriginalData::Pixels(img) => encode_png(&img),
                    };
                    let res = bytes.and_then(|b| {
                        atomic_write(&store.doc_dir(&id).join(&name), &b).map_err(|e| e.to_string())
                    });
                    if let Err(e) = res {
                        emit(WriterEvent::Error(format!("Could not store image: {e}")));
                    }
                }
                Job::Document { doc, thumb } => {
                    if latest.get(&doc.id) != Some(&i) {
                        continue;
                    }
                    let dir = store.doc_dir(&doc.id);
                    if !dir.exists() {
                        // Deleted while the save was queued.
                        continue;
                    }
                    let json = serde_json::to_vec_pretty(&*doc).expect("document serialises");
                    if let Err(e) = atomic_write(&dir.join(DOCUMENT_JSON), &json) {
                        emit(WriterEvent::Error(format!("Autosave failed: {e}")));
                        continue;
                    }
                    if let Some(t) = thumb {
                        let img = render::render_scaled(&t.base, &doc.annotations, t.scale);
                        if let Ok(png) = encode_png(&img)
                            && atomic_write(&dir.join(THUMBNAIL_PNG), &png).is_ok()
                        {
                            emit(WriterEvent::ThumbnailWritten(doc.id.clone()));
                        }
                    }
                }
                Job::Settings(s) => {
                    let _ = store.save_settings(&s);
                }
                Job::Delete(id) => {
                    if let Err(e) = store.delete_document(&id) {
                        emit(WriterEvent::Error(format!("Could not delete: {e}")));
                    }
                    emit(WriterEvent::HistoryChanged);
                }
                Job::DeleteAll { keep } => {
                    for e in store.list_documents() {
                        if Some(&e.id) != keep.as_ref() {
                            let _ = store.delete_document(&e.id);
                        }
                    }
                    emit(WriterEvent::HistoryChanged);
                }
                Job::Prune { keep } => {
                    store.cleanup();
                    if store.prune(HISTORY_LIMIT, keep.as_deref()) > 0 {
                        emit(WriterEvent::HistoryChanged);
                    }
                }
                Job::Flush(done) => {
                    let _ = done.send(());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Pt;
    use crate::model::{PALETTE, Shape, Style};

    fn temp_store(name: &str) -> Store {
        let dir =
            std::env::temp_dir().join(format!("markly-test-{name}-{}", crate::model::new_document_id()));
        Store::open(dir).unwrap()
    }

    fn png_bytes(w: u32, h: u32) -> Vec<u8> {
        encode_png(&RgbaImage::from_pixel(w, h, image::Rgba([1, 2, 3, 255]))).unwrap()
    }

    fn write_doc(store: &Store, doc: &Document) {
        let dir = store.doc_dir(&doc.id);
        atomic_write(&dir.join(&doc.original_file), &png_bytes(doc.width, doc.height)).unwrap();
        atomic_write(&dir.join(DOCUMENT_JSON), &serde_json::to_vec(doc).unwrap()).unwrap();
    }

    #[test]
    fn atomic_write_replaces_and_leaves_no_temp() {
        let store = temp_store("atomic");
        let p = store.root().join("x.json");
        atomic_write(&p, b"one").unwrap();
        atomic_write(&p, b"two").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"two");
        let leftovers: Vec<_> = fs::read_dir(store.root())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
        fs::remove_dir_all(store.root()).unwrap();
    }

    #[test]
    fn document_persistence_and_history() {
        let store = temp_store("history");
        let mut a = Document::new(40, 30, "original.png", None);
        a.add(Shape::Arrow {
            start: Pt::new(1.0, 2.0),
            end: Pt::new(30.0, 20.0),
            style: Style {
                color: PALETTE[0].1,
                width: 3.0,
            },
        });
        a.modified_ms = 10;
        let mut b = Document::new(10, 10, "original.png", Some("/x/shot.png".into()));
        b.modified_ms = 20;
        write_doc(&store, &a);
        write_doc(&store, &b);

        let loaded = store.load_document(&a.id).unwrap();
        assert_eq!(loaded, a);

        let list = store.list_documents();
        assert_eq!(
            list.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
            [&b.id, &a.id]
        );
        assert_eq!(list[0].source_name.as_deref(), Some("shot.png"));
        assert_eq!(list[1].annotations, 1);

        store.delete_document(&b.id).unwrap();
        assert_eq!(store.list_documents().len(), 1);
        fs::remove_dir_all(store.root()).unwrap();
    }

    #[test]
    fn corrupt_and_missing_files_are_handled() {
        let store = temp_store("corrupt");
        let good = Document::new(5, 5, "original.png", None);
        write_doc(&store, &good);

        // Corrupt JSON.
        let bad = Document::new(5, 5, "original.png", None);
        write_doc(&store, &bad);
        fs::write(store.doc_dir(&bad.id).join(DOCUMENT_JSON), b"{ not json").unwrap();
        assert!(store.load_document(&bad.id).is_err());

        // Missing original.
        let orphan = Document::new(5, 5, "original.png", None);
        write_doc(&store, &orphan);
        fs::remove_file(store.doc_dir(&orphan.id).join("original.png")).unwrap();
        assert_eq!(
            store.load_document(&orphan.id).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );

        // Path traversal and unknown ids.
        assert!(store.load_document("../../etc").is_err());
        assert!(store.load_document("123-nope").is_err());

        let ids: Vec<_> = store.list_documents().into_iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![good.id.clone()]);
        fs::remove_dir_all(store.root()).unwrap();
    }

    #[test]
    fn settings_roundtrip_and_corrupt_settings_fall_back() {
        let store = temp_store("settings");
        let s = Settings {
            last_document: Some("1-abc".into()),
            ..Settings::default()
        };
        store.save_settings(&s).unwrap();
        assert_eq!(store.load_settings().last_document.as_deref(), Some("1-abc"));
        fs::write(store.settings_path(), b"garbage").unwrap();
        assert_eq!(store.load_settings(), Settings::default());
        fs::remove_dir_all(store.root()).unwrap();
    }

    #[test]
    fn writer_saves_coalesces_and_flushes() {
        let store = temp_store("writer");
        let (tx, rx) = mpsc::channel();
        let mut w = Writer::spawn(store.clone(), tx, || {});
        let img = Arc::new(RgbaImage::from_pixel(64, 32, image::Rgba([0, 128, 255, 255])));
        let mut doc = Document::new(64, 32, "original.png", None);
        w.save_original(&doc.id, "original.png", OriginalData::Pixels(img.clone()));
        for i in 0..20 {
            doc.add(Shape::Text {
                pos: Pt::new(i as f32, 0.0),
                text: format!("{i}"),
                color: PALETTE[0].1,
                size: 10.0,
            });
            let (base, scale) = thumbnail_base(&img);
            w.save_document(
                doc.clone(),
                Some(ThumbJob {
                    base: Arc::new(base),
                    scale,
                }),
            );
        }
        assert!(w.shutdown(Duration::from_secs(10)));
        let loaded = store.load_document(&doc.id).unwrap();
        assert_eq!(loaded.annotations.len(), 20);
        let original = decode_image(&fs::read(store.original_path(&loaded)).unwrap()).unwrap();
        assert_eq!(original.dimensions(), (64, 32));
        assert!(store.thumbnail_path(&doc.id).is_file());
        assert!(
            rx.try_iter()
                .any(|e| e == WriterEvent::ThumbnailWritten(doc.id.clone()))
        );
        fs::remove_dir_all(store.root()).unwrap();
    }

    #[test]
    fn prune_keeps_newest_and_current() {
        let store = temp_store("prune");
        let mut ids = Vec::new();
        for i in 0..5 {
            let mut d = Document::new(2, 2, "original.png", None);
            d.modified_ms = i;
            write_doc(&store, &d);
            ids.push(d.id);
        }
        // Keep the 2 newest plus the oldest (currently open) one.
        assert_eq!(store.prune(2, Some(&ids[0])), 2);
        let mut left: Vec<_> = store.list_documents().into_iter().map(|e| e.id).collect();
        left.sort();
        let mut expected = vec![ids[0].clone(), ids[3].clone(), ids[4].clone()];
        expected.sort();
        assert_eq!(left, expected);
        fs::remove_dir_all(store.root()).unwrap();
    }

    #[test]
    fn decode_rejects_garbage() {
        assert!(decode_image(b"definitely not an image").is_err());
        assert_eq!(original_name_for(&png_bytes(1, 1)), Some("original.png"));
    }
}
