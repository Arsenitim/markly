//! Annotation typeface and text layout shared by hit-testing and export.
//!
//! The same font bytes are registered with egui for the interactive preview
//! so on-screen text matches exported text closely.

use crate::geometry::{Pt, Rect};
use ab_glyph::{Font, FontRef, GlyphId, PxScale, ScaleFont};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// Formatting of a whole text annotation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct TextFormat {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
}

impl TextFormat {
    pub fn is_plain(&self) -> bool {
        *self == TextFormat::default()
    }
}

/// Noto Sans (SIL Open Font License 1.1, see assets/fonts/OFL.txt): regular
/// text is SemiBold, bold is ExtraBold, plus the matching italics.
static FONTS: [(&str, &[u8]); 4] = [
    (
        "skrib-annotation",
        include_bytes!("../assets/fonts/NotoSans-SemiBold.ttf"),
    ),
    (
        "skrib-annotation-bold",
        include_bytes!("../assets/fonts/NotoSans-ExtraBold.ttf"),
    ),
    (
        "skrib-annotation-italic",
        include_bytes!("../assets/fonts/NotoSans-SemiBoldItalic.ttf"),
    ),
    (
        "skrib-annotation-bold-italic",
        include_bytes!("../assets/fonts/NotoSans-ExtraBoldItalic.ttf"),
    ),
];

fn variant(fmt: TextFormat) -> usize {
    fmt.bold as usize + 2 * fmt.italic as usize
}

/// (egui family name, font bytes) for every variant, for registering with egui.
pub fn all_fonts() -> &'static [(&'static str, &'static [u8])] {
    &FONTS
}

/// Name under which the variant for `fmt` is registered with egui.
pub fn egui_font_name(fmt: TextFormat) -> &'static str {
    FONTS[variant(fmt)].0
}

pub fn font(fmt: TextFormat) -> &'static FontRef<'static> {
    static PARSED: OnceLock<Vec<FontRef<'static>>> = OnceLock::new();
    &PARSED.get_or_init(|| {
        FONTS
            .iter()
            .map(|(_, b)| FontRef::try_from_slice(b).expect("bundled font is valid"))
            .collect()
    })[variant(fmt)]
}

/// ab_glyph scales by (ascent − descent) height; egui and CSS use the em
/// size. Convert so `size` means the same thing everywhere.
pub fn px_scale(size: f32, fmt: TextFormat) -> PxScale {
    let f = font(fmt);
    let upem = f.units_per_em().unwrap_or(1000.0);
    PxScale::from(size * f.height_unscaled() / upem)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineMetrics {
    pub ascent: f32,
    pub descent: f32,
    pub line_height: f32,
}

/// Line metrics. All variants share the family's vertical metrics, so the
/// regular face stands in for all of them (line positions never depend on
/// formatting).
pub fn metrics(size: f32) -> LineMetrics {
    let fmt = TextFormat::default();
    let sf = font(fmt).as_scaled(px_scale(size, fmt));
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
    /// Width of each line, for underline and strikethrough.
    pub line_widths: Vec<f32>,
}

pub fn layout(text: &str, size: f32, fmt: TextFormat) -> TextLayout {
    let f = font(fmt);
    let scale = px_scale(size, fmt);
    let sf = f.as_scaled(scale);
    let m = metrics(size);
    let mut glyphs = Vec::with_capacity(text.len());
    let mut width = 0.0f32;
    let mut line_widths = Vec::new();
    for (i, line) in text.split('\n').enumerate() {
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
        line_widths.push(x);
    }
    // Italic glyphs lean past their advance on the right.
    if fmt.italic {
        width += size * ITALIC_OVERHANG;
    }
    TextLayout {
        glyphs,
        width,
        height: line_widths.len().max(1) as f32 * m.line_height,
        scale,
        line_widths,
    }
}

const ITALIC_OVERHANG: f32 = 0.12;

/// Size of the text block in image pixels. Empty text still has a caret-
/// sized footprint so it remains clickable.
pub fn measure(text: &str, size: f32, fmt: TextFormat) -> (f32, f32) {
    let l = layout(text, size, fmt);
    (l.width.max(size * 0.4), l.height)
}

/// Underline and strikethrough bars, relative to the block's top-left.
/// Shared by the on-screen preview and the export so both match.
pub fn decoration_rects(text: &str, size: f32, fmt: TextFormat) -> Vec<Rect> {
    if !fmt.underline && !fmt.strikethrough {
        return Vec::new();
    }
    let l = layout(text, size, fmt);
    let m = metrics(size);
    let thick = size * if fmt.bold { 0.075 } else { 0.06 };
    let mut out = Vec::new();
    for (i, &w) in l.line_widths.iter().enumerate() {
        if w <= 0.0 {
            continue;
        }
        let baseline = m.ascent + i as f32 * m.line_height;
        // Italic bars start a touch right, under the slanted letters.
        let x0 = if fmt.italic { size * 0.04 } else { 0.0 };
        let x1 = w + if fmt.italic { size * 0.06 } else { 0.0 };
        // Noto Sans: underline just below the baseline, strike through the
        // middle of the x-height (0.536 em).
        let mut bar = |center: f32| {
            out.push(Rect::from_points(
                Pt::new(x0, center - thick * 0.5),
                Pt::new(x1, center + thick * 0.5),
            ));
        };
        if fmt.underline {
            bar(baseline + size * 0.11);
        }
        if fmt.strikethrough {
            bar(baseline - size * 0.27);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_loads_and_measures() {
        let plain = TextFormat::default();
        let (w1, h1) = measure("Hello", 20.0, plain);
        let (w2, h2) = measure("Hello", 40.0, plain);
        assert!(w1 > 20.0 && h1 > 20.0);
        assert!((w2 / w1 - 2.0).abs() < 0.05);
        assert!((h2 / h1 - 2.0).abs() < 0.05);
    }

    #[test]
    fn multiline_height() {
        let m = metrics(20.0);
        let plain = TextFormat::default();
        let (_, h) = measure("a\nb\nc", 20.0, plain);
        assert!((h - 3.0 * m.line_height).abs() < 1e-3);
        let (_, h) = measure("trailing\n", 20.0, plain);
        assert!((h - 2.0 * m.line_height).abs() < 1e-3);
    }

    #[test]
    fn variants_load_and_differ() {
        let plain = TextFormat::default();
        let bold = TextFormat { bold: true, ..plain };
        let italic = TextFormat {
            italic: true,
            ..plain
        };
        let both = TextFormat {
            bold: true,
            italic: true,
            ..plain
        };
        let names: std::collections::HashSet<_> = [plain, bold, italic, both]
            .map(egui_font_name)
            .into_iter()
            .collect();
        assert_eq!(names.len(), 4);
        // Bold is wider; line height is shared so lines never jump.
        assert!(measure("Hello", 20.0, bold).0 > measure("Hello", 20.0, plain).0);
        assert_eq!(measure("a\nb", 20.0, both).1, measure("a\nb", 20.0, plain).1);
    }

    #[test]
    fn decorations_per_line() {
        let plain = TextFormat::default();
        assert!(decoration_rects("ab\ncd", 20.0, plain).is_empty());
        let u = TextFormat {
            underline: true,
            ..plain
        };
        assert_eq!(
            decoration_rects("ab\n\ncd", 20.0, u).len(),
            2,
            "empty line has no bar"
        );
        let us = TextFormat {
            strikethrough: true,
            ..u
        };
        let r = decoration_rects("ab", 20.0, us);
        assert_eq!(r.len(), 2);
        let m = metrics(20.0);
        assert!(r[0].min.y > m.ascent, "underline sits below the baseline");
        assert!(r[1].max.y < m.ascent, "strike sits above the baseline");
    }

    #[test]
    fn em_size_semantics() {
        // Typical Latin fonts have cap height ~0.7 em and ascent ~1 em.
        let m = metrics(100.0);
        assert!(m.ascent > 80.0 && m.ascent < 120.0, "ascent {}", m.ascent);
    }
}
