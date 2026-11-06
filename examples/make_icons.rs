//! Regenerates the committed icon assets from `markly::icon`:
//!
//! ```sh
//! cargo run --example make_icons
//! ```
//!
//! * `assets/icons/markly.svg`, `markly-{256,512}.png` — general use / README
//! * `assets/icons/markly.ico` — embedded into the Windows executable
//! * `assets/icons/macos/markly.iconset/` — turned into `.icns` by `iconutil`
//!   in the macOS release job

use image::codecs::ico::{IcoEncoder, IcoFrame};
use markly::icon::{self, MARGIN_DEFAULT, MARGIN_MACOS};
use std::path::Path;

fn png(size: u32, margin: f32) -> Vec<u8> {
    markly::store::encode_png(&icon::render(size, margin)).unwrap()
}

fn main() {
    let dir = Path::new("assets/icons");
    std::fs::create_dir_all(dir.join("macos/markly.iconset")).unwrap();
    std::fs::write(dir.join("markly.svg"), icon::svg(MARGIN_DEFAULT)).unwrap();
    for s in [256, 512] {
        std::fs::write(dir.join(format!("markly-{s}.png")), png(s, MARGIN_DEFAULT)).unwrap();
    }

    let frames: Vec<IcoFrame> = [16, 24, 32, 48, 64, 128, 256]
        .iter()
        .map(|&s| {
            IcoFrame::with_encoded(png(s, MARGIN_DEFAULT), s, s, image::ExtendedColorType::Rgba8).unwrap()
        })
        .collect();
    let mut ico = Vec::new();
    IcoEncoder::new(&mut ico).encode_images(&frames).unwrap();
    std::fs::write(dir.join("markly.ico"), ico).unwrap();

    for (name, size) in [
        ("icon_16x16", 16),
        ("icon_16x16@2x", 32),
        ("icon_32x32", 32),
        ("icon_32x32@2x", 64),
        ("icon_128x128", 128),
        ("icon_128x128@2x", 256),
        ("icon_256x256", 256),
        ("icon_256x256@2x", 512),
        ("icon_512x512", 512),
        ("icon_512x512@2x", 1024),
    ] {
        std::fs::write(
            dir.join(format!("macos/markly.iconset/{name}.png")),
            png(size, MARGIN_MACOS),
        )
        .unwrap();
    }
    println!("icons written to {}", dir.display());
}
