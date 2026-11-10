//! End-to-end check of the core workflow through the public library API:
//! paste → annotate → autosave → "restart" → restore → identical export.

use skrib::geometry::{Pt, Rect};
use skrib::model::{Document, PALETTE, Shape, Style, TEXT_SIZES};
use skrib::render;
use skrib::settings::Settings;
use skrib::store::{self, OriginalData, Store, ThumbJob, Writer};
use skrib::undo::UndoStack;
use std::sync::{Arc, mpsc};
use std::time::Duration;

fn temp_dir(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("skrib-it-{tag}-{}", skrib::model::new_document_id()))
}

fn screenshot() -> image::RgbaImage {
    image::RgbaImage::from_fn(640, 400, |x, y| {
        image::Rgba([(x % 256) as u8, (y % 256) as u8, 128, 255])
    })
}

#[test]
fn paste_annotate_restart_restore() {
    let dir = temp_dir("workflow");
    let img = Arc::new(screenshot());
    let red = Style {
        color: PALETTE[0].1,
        width: 5.5,
    };

    // --- First session -----------------------------------------------------
    let doc_id;
    let expected;
    {
        let store = Store::open(dir.clone()).unwrap();
        let (tx, _rx) = mpsc::channel();
        let mut writer = Writer::spawn(store.clone(), tx, || {});
        let mut doc = Document::new(img.width(), img.height(), "original.png", None);
        writer.save_original(&doc.id, &doc.original_file, OriginalData::Pixels(img.clone()));

        let mut undo = UndoStack::default();
        let before = doc.annotations.clone();
        doc.add(Shape::Arrow {
            start: Pt::new(50.0, 50.0),
            end: Pt::new(300.0, 200.0),
            style: red,
        });
        undo.commit(before, &doc.annotations);
        let before = doc.annotations.clone();
        doc.add(Shape::Arrow {
            start: Pt::new(600.0, 50.0),
            end: Pt::new(350.0, 200.0),
            style: red,
        });
        undo.commit(before, &doc.annotations);
        let before = doc.annotations.clone();
        doc.add(Shape::Text {
            pos: Pt::new(220.0, 230.0),
            text: "Broken buttons".into(),
            color: PALETTE[0].1,
            size: TEXT_SIZES[2],
            format: Default::default(),
        });
        doc.add(Shape::Pixelate {
            rect: Rect::from_points(Pt::new(10.0, 300.0), Pt::new(200.0, 390.0)),
            block: 12.0,
        });
        undo.commit(before, &doc.annotations);

        let (base, scale) = store::thumbnail_base(&img);
        writer.save_document(
            doc.clone(),
            Some(ThumbJob {
                base: Arc::new(base),
                scale,
            }),
        );
        writer.save_settings(Settings {
            last_document: Some(doc.id.clone()),
            ..Settings::default()
        });
        // Closing the app flushes everything; nothing was "saved" by the user.
        assert!(writer.shutdown(Duration::from_secs(10)));

        expected = render::flatten(&img, &doc.annotations);
        doc_id = doc.id;
    }

    // --- Relaunch ----------------------------------------------------------
    let store = Store::open(dir.clone()).unwrap();
    let settings = store.load_settings();
    assert_eq!(settings.last_document.as_deref(), Some(doc_id.as_str()));
    let doc = store.load_document(&doc_id).unwrap();
    assert_eq!(
        doc.annotations.len(),
        4,
        "annotations restored and still editable"
    );
    assert!(matches!(&doc.annotations[2].shape, Shape::Text { text, .. } if text == "Broken buttons"));
    let original = store::decode_image(&std::fs::read(store.original_path(&doc)).unwrap()).unwrap();
    assert_eq!(
        original.as_raw(),
        img.as_raw(),
        "original is stored losslessly and unmodified"
    );

    let restored = render::flatten(&original, &doc.annotations);
    assert_eq!(
        restored.as_raw(),
        expected.as_raw(),
        "export after restart is identical"
    );
    assert_eq!(restored.dimensions(), (640, 400));

    // History lists it with a thumbnail.
    let entries = store.list_documents();
    assert_eq!(entries.len(), 1);
    assert!(store.thumbnail_path(&doc_id).is_file());

    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn new_paste_preserves_previous_document() {
    let dir = temp_dir("two-docs");
    let store = Store::open(dir.clone()).unwrap();
    let (tx, _rx) = mpsc::channel();
    let mut writer = Writer::spawn(store.clone(), tx, || {});
    let mut ids = Vec::new();
    for i in 0..3u8 {
        let img = Arc::new(image::RgbaImage::from_pixel(
            32,
            32,
            image::Rgba([i * 40, 0, 0, 255]),
        ));
        let mut doc = Document::new(32, 32, "original.png", None);
        doc.modified_ms += i as u64; // deterministic ordering
        writer.save_original(&doc.id, "original.png", OriginalData::Pixels(img));
        writer.save_document(doc.clone(), None);
        ids.push(doc.id);
    }
    assert!(writer.shutdown(Duration::from_secs(10)));
    let listed: Vec<_> = store.list_documents().into_iter().map(|e| e.id).collect();
    ids.reverse();
    assert_eq!(listed, ids, "all documents kept, newest first");
    std::fs::remove_dir_all(dir).unwrap();
}
