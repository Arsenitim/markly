//! Seeds a data directory with a synthetic screenshot and one annotation of
//! every kind, then writes the flattened export next to it. Used for manual
//! QA, visual comparison of preview vs export, and benchmarks.
//!
//! ```sh
//! cargo run --release --example demo_document -- /tmp/skrib-demo [WIDTH HEIGHT]
//! SKRIB_DATA_DIR=/tmp/skrib-demo cargo run --release
//! ```

use skrib::geometry::{Pt, Rect};
use skrib::model::{Document, PALETTE, Shape, Style, TEXT_SIZES, size_unit};
use skrib::settings::Settings;
use skrib::store::{self, Store};
use skrib::text;
use std::path::PathBuf;

fn synthetic_screenshot(w: u32, h: u32) -> image::RgbaImage {
    let s = w as f32 / 1600.0;
    let mut img = image::RgbaImage::from_fn(w, h, |_x, y| {
        let t = y as f32 / h as f32;
        let v = (246.0 - 10.0 * t) as u8;
        image::Rgba([v, v, (v as f32 + 4.0).min(255.0) as u8, 255])
    });
    let fill = |img: &mut image::RgbaImage, x: f32, y: f32, ww: f32, hh: f32, c: [u8; 4]| {
        for yy in (y * s) as u32..((y + hh) * s).min(h as f32 / s * s) as u32 {
            for xx in (x * s) as u32..((x + ww) * s) as u32 {
                if xx < w && yy < h {
                    img.put_pixel(xx, yy, image::Rgba(c));
                }
            }
        }
    };
    // Title bar, sidebar, cards, and "text" lines.
    fill(&mut img, 0.0, 0.0, 1600.0, 56.0, [38, 42, 56, 255]);
    fill(&mut img, 0.0, 56.0, 260.0, 1200.0, [229, 232, 240, 255]);
    for i in 0..8 {
        fill(
            &mut img,
            24.0,
            90.0 + i as f32 * 44.0,
            200.0,
            14.0,
            [190, 196, 210, 255],
        );
    }
    for (i, c) in [[66, 133, 244, 255], [52, 168, 83, 255], [251, 188, 5, 255]]
        .iter()
        .enumerate()
    {
        let x = 300.0 + i as f32 * 420.0;
        fill(&mut img, x, 100.0, 380.0, 220.0, [255, 255, 255, 255]);
        fill(&mut img, x + 24.0, 124.0, 120.0, 120.0, *c);
        for j in 0..4 {
            fill(
                &mut img,
                x + 164.0,
                130.0 + j as f32 * 26.0,
                190.0 - j as f32 * 30.0,
                12.0,
                [120, 126, 140, 255],
            );
        }
    }
    fill(&mut img, 300.0, 360.0, 1240.0, 420.0, [255, 255, 255, 255]);
    for j in 0..12 {
        fill(
            &mut img,
            330.0,
            390.0 + j as f32 * 30.0,
            1100.0 - (j % 4) as f32 * 160.0,
            12.0,
            [150, 156, 170, 255],
        );
    }
    // A fake "secret" token to pixelate.
    fill(&mut img, 330.0, 820.0, 520.0, 46.0, [255, 245, 220, 255]);
    let token = skrib::render::render_scaled(
        &img,
        &[skrib::model::Annotation {
            id: 0,
            shape: Shape::Text {
                pos: Pt::new(340.0 * s, 826.0 * s),
                text: "sk-live-4f9a-SECRET-TOKEN".into(),
                color: skrib::model::Rgba::rgb(60, 40, 10),
                size: 22.0 * s,
                format: Default::default(),
            },
        }],
        1.0,
    );
    img = token;
    img
}

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(args.next().expect("usage: demo_document <data-dir> [w h]"));
    let w: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(1600);
    let h: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(1000);
    let store = Store::open(dir.clone()).expect("open store");

    let img = synthetic_screenshot(w, h);
    let s = w as f32 / 1600.0;
    let p = |x: f32, y: f32| Pt::new(x * s, y * s);
    let unit = size_unit(w, h);
    let red = PALETTE[0].1;
    let st = |c, wd: f32| Style {
        color: c,
        width: wd * unit,
    };

    let mut doc = Document::new(w, h, "original.png", None);
    doc.add(Shape::Pixelate {
        rect: Rect::from_points(p(330.0, 820.0), p(850.0, 866.0)),
        block: 13.0 * unit,
    });
    doc.add(Shape::Arrow {
        start: p(980.0, 560.0),
        end: p(560.0, 250.0),
        style: st(red, 5.5),
    });
    doc.add(Shape::CurvedArrow {
        start: p(1120.0, 600.0),
        end: p(1350.0, 330.0),
        style: st(red, 5.5),
        bend: skrib::geometry::DEFAULT_CURVE_BEND,
    });
    doc.add(Shape::CurvedArrow {
        start: p(300.0, 960.0),
        end: p(320.0, 880.0),
        style: st(red, 3.5),
        bend: Pt::new(0.0, -0.25),
    });
    doc.add(Shape::Rectangle {
        rect: Rect::from_points(p(290.0, 90.0), p(690.0, 330.0)),
        style: st(PALETTE[5].1, 5.5),
    });
    doc.add(Shape::Ellipse {
        rect: Rect::from_points(p(1140.0, 300.0), p(1560.0, 340.0)),
        style: st(PALETTE[4].1, 3.5),
    });
    let pts: Vec<Pt> = (0..80)
        .map(|i| {
            let t = i as f32 / 79.0;
            p(330.0 + 700.0 * t, 760.0 + 14.0 * (t * 18.0).sin())
        })
        .collect();
    doc.add(Shape::Freehand {
        points: skrib::geometry::smooth_stroke(&pts, 4.0),
        style: st(PALETTE[2].1, 5.5),
    });
    let tsize = TEXT_SIZES[2] * unit;
    doc.add(Shape::Text {
        pos: Pt::new(980.0 * s, 560.0 * s + 4.0),
        text: "These two need fixing!\nSee the notes below.".into(),
        color: red,
        size: tsize,
        // Exercise formatting in preview-vs-export comparisons.
        format: skrib::model::TextFormat {
            bold: true,
            underline: true,
            ..Default::default()
        },
    });
    let _ = text::measure("x", tsize, Default::default());

    let dd = store.doc_dir(&doc.id);
    store::atomic_write(&dd.join("original.png"), &store::encode_png(&img).unwrap()).unwrap();
    store::atomic_write(
        &dd.join(store::DOCUMENT_JSON),
        &serde_json::to_vec_pretty(&doc).unwrap(),
    )
    .unwrap();
    let (thumb, scale) = store::thumbnail_base(&img);
    let t = skrib::render::render_scaled(&thumb, &doc.annotations, scale);
    store::atomic_write(&dd.join(store::THUMBNAIL_PNG), &store::encode_png(&t).unwrap()).unwrap();

    let mut settings = store.load_settings();
    settings.last_document = Some(doc.id.clone());
    if settings == Settings::default() || settings.window_size.is_none() {
        settings.window_size = Some([1280.0, 860.0]);
    }
    store.save_settings(&settings).unwrap();

    let t0 = std::time::Instant::now();
    let flat = skrib::render::flatten(&img, &doc.annotations);
    let render_ms = t0.elapsed().as_secs_f64() * 1000.0;
    flat.save(dir.join("export.png")).unwrap();
    println!(
        "document {} ({w}x{h}); flatten took {render_ms:.1} ms; export at {}",
        doc.id,
        dir.join("export.png").display()
    );
}
