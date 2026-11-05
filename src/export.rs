//! Export of the flattened, annotated image to PNG or JPEG.

use crate::model::Annotation;
use crate::render;
use crate::store;
use image::{ImageEncoder, RgbaImage};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    Png,
    Jpeg,
}

impl ExportFormat {
    /// Chosen from the file extension; anything unknown becomes PNG.
    pub fn from_path(path: &Path) -> ExportFormat {
        match path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("jpg" | "jpeg") => ExportFormat::Jpeg,
            _ => ExportFormat::Png,
        }
    }
}

/// Encodes the original image with all annotations at original resolution.
/// JPEG has no alpha, so transparent areas are composited onto white.
pub fn encode(image: &RgbaImage, anns: &[Annotation], format: ExportFormat) -> Result<Vec<u8>, String> {
    let flat = render::flatten(image, anns);
    let mut bytes = Vec::new();
    match format {
        ExportFormat::Jpeg => {
            let rgb = render::flatten_alpha(&flat, [255, 255, 255]);
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 92)
                .write_image(
                    rgb.as_raw(),
                    rgb.width(),
                    rgb.height(),
                    image::ExtendedColorType::Rgb8,
                )
                .map_err(|e| e.to_string())?;
        }
        ExportFormat::Png => {
            image::codecs::png::PngEncoder::new(&mut bytes)
                .write_image(
                    flat.as_raw(),
                    flat.width(),
                    flat.height(),
                    image::ExtendedColorType::Rgba8,
                )
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(bytes)
}

/// Renders and atomically writes the export to `path`.
pub fn export_file(image: &RgbaImage, anns: &[Annotation], path: &Path) -> Result<(), String> {
    let bytes = encode(image, anns, ExportFormat::from_path(path))?;
    store::atomic_write(path, &bytes).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{Pt, Rect};
    use crate::model::{Document, PALETTE, Shape, Style};

    fn sample() -> (RgbaImage, Vec<Annotation>) {
        let img = RgbaImage::from_fn(300, 120, |x, _| {
            image::Rgba([0, 0, 0, if x < 150 { 0 } else { 255 }])
        });
        let mut d = Document::new(300, 120, "original.png", None);
        d.add(Shape::Rectangle {
            rect: Rect::from_points(Pt::new(20.0, 20.0), Pt::new(280.0, 100.0)),
            style: Style {
                color: PALETTE[0].1,
                width: 6.0,
            },
        });
        (img, d.annotations)
    }

    #[test]
    fn png_export_keeps_size_and_alpha() {
        let (img, anns) = sample();
        let png = encode(&img, &anns, ExportFormat::Png).unwrap();
        let back = image::load_from_memory(&png).unwrap().into_rgba8();
        assert_eq!(back.dimensions(), (300, 120));
        assert_eq!(back.get_pixel(5, 5).0[3], 0, "transparent area stays transparent");
        assert_eq!(back.get_pixel(150, 20).0, PALETTE[0].1.0, "annotation is drawn");
    }

    #[test]
    fn jpeg_export_flattens_onto_white() {
        let (img, anns) = sample();
        let jpg = encode(&img, &anns, ExportFormat::Jpeg).unwrap();
        assert_eq!(image::guess_format(&jpg).unwrap(), image::ImageFormat::Jpeg);
        let back = image::load_from_memory(&jpg).unwrap().into_rgb8();
        assert_eq!(back.dimensions(), (300, 120));
        assert!(
            back.get_pixel(5, 60).0.iter().all(|&c| c > 245),
            "transparent → white"
        );
        assert!(
            back.get_pixel(200, 60).0.iter().all(|&c| c < 10),
            "opaque black stays black"
        );
    }

    #[test]
    fn format_from_extension() {
        assert_eq!(ExportFormat::from_path(Path::new("a.JPG")), ExportFormat::Jpeg);
        assert_eq!(ExportFormat::from_path(Path::new("a.jpeg")), ExportFormat::Jpeg);
        assert_eq!(ExportFormat::from_path(Path::new("a.png")), ExportFormat::Png);
        assert_eq!(ExportFormat::from_path(Path::new("a")), ExportFormat::Png);
    }

    #[test]
    fn export_file_writes_atomically() {
        let (img, anns) = sample();
        let dir = std::env::temp_dir().join(format!("markly-export-{}", crate::model::new_document_id()));
        let path = dir.join("out.png");
        export_file(&img, &anns, &path).unwrap();
        assert_eq!(image::open(&path).unwrap().width(), 300);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
