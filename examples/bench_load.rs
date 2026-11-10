//! Times each stage of opening a stored original (decode, alpha scan,
//! texture conversion, thumbnail) — the work done off the UI thread.
use std::time::Instant;
fn main() {
    let path = std::env::args().nth(1).expect("usage: bench_load <image>");
    let bytes = std::fs::read(&path).unwrap();
    let t = Instant::now();
    let img = skrib::store::decode_image(&bytes).unwrap();
    let decode = t.elapsed();
    let t = Instant::now();
    let alpha = img.as_raw().as_chunks::<4>().0.iter().any(|p| p[3] != 255);
    let scan = t.elapsed();
    let t = Instant::now();
    let ci = eframe::egui::ColorImage::from_rgba_unmultiplied(
        [img.width() as usize, img.height() as usize],
        img.as_raw(),
    );
    let conv = t.elapsed();
    let t = Instant::now();
    let (thumb, _) = skrib::store::thumbnail_base(&img);
    let th = t.elapsed();
    let t = Instant::now();
    let png = skrib::store::encode_png(&img).unwrap();
    let enc = t.elapsed();
    let t = Instant::now();
    let flat = skrib::render::flatten(&img, &[]);
    let fl = t.elapsed();
    println!(
        "{}x{} ({} KB png): decode {:?}, alpha-scan {:?} ({alpha}), to-texture {:?}, thumbnail {:?} ({}x{}), fast-png-encode {:?} ({} KB), flatten {:?} ({} px)",
        img.width(),
        img.height(),
        bytes.len() / 1024,
        decode,
        scan,
        conv,
        th,
        thumb.width(),
        thumb.height(),
        enc,
        png.len() / 1024,
        fl,
        ci.pixels.len() + flat.as_raw().len() / 4
    );
}
