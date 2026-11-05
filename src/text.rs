//! Annotation typeface and text layout shared by hit-testing and export.
//!
//! The same font bytes are registered with egui for the interactive preview
//! so on-screen text matches exported text closely.

use crate::geometry::Pt;
use ab_glyph::{Font, FontRef, GlyphId, PxScale, ScaleFont};
use std::sync::OnceLock;

/// Noto Sans SemiBold (SIL Open Font License 1.1, see assets/fonts/OFL.txt).
pub static FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/NotoSans-SemiBold.ttf");

/// Name under which the font is registered with egui.
pub const EGUI_FONT_NAME: &str = "markly-annotation";

pub fn font() -> &'static FontRef<'static> {
    static FONT: OnceLock<FontRef<'static>> = OnceLock::new();
    FONT.get_or_init(|| FontRef::try_from_slice(FONT_BYTES).expect("bundled font is valid"))
}

/// ab_glyph scales by (ascent − descent) height; egui and CSS use the em
/// size. Convert so `size` means the same thing everywhere.
pub fn px_scale(size: f32) -> PxScale {
    let f = font();
    let upem = f.units_per_em().unwrap_or(1000.0);
    PxScale::from(size * f.height_unscaled() / upem)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineMetrics {
    pub ascent: f32,
    pub descent: f32,
    pub line_height: f32,
}

pub fn metrics(size: f32) -> LineMetrics {
    let sf = font().as_scaled(px_scale(size));
    LineMetrics {
        ascent: sf.ascent(),
        descent: sf.descent(),
        line_height: sf.ascent() - sf.descent() + sf.line_gap(),
    }
}

#[derive(Clone, Debug)]
pub struct TextLayout {
    /// Glyph id and its baseline origin relative to the block's top-left.
    pub glyphs: Vec<(GlyphId, Pt)>,
    pub width: f32,
    pub height: f32,
    pub scale: PxScale,
}

pub fn layout(text: &str, size: f32) -> TextLayout {
    let f = font();
    let scale = px_scale(size);
    let sf = f.as_scaled(scale);
    let m = metrics(size);
    let mut glyphs = Vec::with_capacity(text.len());
    let mut width = 0.0f32;
    let mut lines = 0usize;
    for (i, line) in text.split('\n').enumerate() {
        lines += 1;
        let baseline = m.ascent + i as f32 * m.line_height;
        let mut x = 0.0f32;
        let mut prev: Option<GlyphId> = None;
        for ch in line.chars() {
            let ch = if ch == '\t' { ' ' } else { ch };
            if ch.is_control() {
                continue;
            }
            let id = f.glyph_id(ch);
            if let Some(p) = prev {
                x += sf.kern(p, id);
            }
            glyphs.push((id, Pt::new(x, baseline)));
            x += sf.h_advance(id);
            prev = Some(id);
        }
        width = width.max(x);
    }
    TextLayout {
        glyphs,
        width,
        height: lines.max(1) as f32 * m.line_height,
        scale,
    }
}

/// Size of the text block in image pixels. Empty text still has a caret-
/// sized footprint so it remains clickable.
pub fn measure(text: &str, size: f32) -> (f32, f32) {
    let l = layout(text, size);
    (l.width.max(size * 0.4), l.height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_loads_and_measures() {
        let (w1, h1) = measure("Hello", 20.0);
        let (w2, h2) = measure("Hello", 40.0);
        assert!(w1 > 20.0 && h1 > 20.0);
        assert!((w2 / w1 - 2.0).abs() < 0.05);
        assert!((h2 / h1 - 2.0).abs() < 0.05);
    }

    #[test]
    fn multiline_height() {
        let m = metrics(20.0);
        let (_, h) = measure("a\nb\nc", 20.0);
        assert!((h - 3.0 * m.line_height).abs() < 1e-3);
        let (_, h) = measure("trailing\n", 20.0);
        assert!((h - 2.0 * m.line_height).abs() < 1e-3);
    }

    #[test]
    fn em_size_semantics() {
        // Typical Latin fonts have cap height ~0.7 em and ascent ~1 em.
        let m = metrics(100.0);
        assert!(m.ascent > 80.0 && m.ascent < 120.0, "ascent {}", m.ascent);
    }
}
