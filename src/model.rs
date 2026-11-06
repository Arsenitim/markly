//! Document model: editable annotations stored in original-image
//! coordinates. Contains no GUI or rendering-backend types.

use crate::geometry::{
    Pt, Rect, arrow_geometry, curved_arrow_geometry, dist_to_polyline, dist_to_segment, ellipse_points,
    point_in_triangle, square_from,
};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

pub const DOCUMENT_FORMAT_VERSION: u32 = 1;

/// Straight (non-premultiplied) sRGB colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rgba(pub [u8; 4]);

impl Rgba {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Rgba([r, g, b, 255])
    }
    /// Relative luminance in 0..1 (approximate, sRGB weights).
    pub fn luminance(self) -> f32 {
        let [r, g, b, _] = self.0;
        (0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32) / 255.0
    }
    /// Contrasting halo colour used behind text.
    pub fn halo(self) -> Rgba {
        if self.luminance() > 0.6 {
            Rgba([20, 20, 24, 255])
        } else {
            Rgba([255, 255, 255, 255])
        }
    }
}

/// The curated palette.
/// The first entry is the default colour for every tool.
pub const PALETTE: [(&str, Rgba); 9] = [
    ("Pink", Rgba::rgb(255, 51, 108)),
    ("Red", Rgba::rgb(234, 59, 49)),
    ("Orange", Rgba::rgb(246, 138, 28)),
    ("Yellow", Rgba::rgb(250, 204, 21)),
    ("Green", Rgba::rgb(34, 177, 90)),
    ("Blue", Rgba::rgb(37, 122, 240)),
    ("Purple", Rgba::rgb(146, 76, 222)),
    ("Black", Rgba::rgb(24, 24, 27)),
    ("White", Rgba::rgb(250, 250, 250)),
];

/// Stroke widths (before image-size scaling) for line-based tools.
pub const STROKE_WIDTHS: [f32; 5] = [2.0, 3.5, 5.5, 8.0, 12.0];
/// Text sizes (before image-size scaling).
pub const TEXT_SIZES: [f32; 5] = [14.0, 19.0, 26.0, 36.0, 52.0];
/// Pixelation block sizes (before image-size scaling).
pub const PIXEL_BLOCKS: [f32; 5] = [6.0, 9.0, 13.0, 19.0, 28.0];

/// Multiplier so presets look similar on small crops and large HiDPI
/// screenshots.
pub fn size_unit(image_w: u32, image_h: u32) -> f32 {
    (image_w.max(image_h) as f32 / 1600.0).clamp(1.0, 4.0)
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Style {
    pub color: Rgba,
    /// Stroke width in image pixels.
    pub width: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Shape {
    Arrow {
        start: Pt,
        end: Pt,
        style: Style,
    },
    /// Bowed, tapered arrow with a large head (see `curved_arrow_geometry`).
    CurvedArrow {
        start: Pt,
        end: Pt,
        style: Style,
        /// Control-point offset in arrow-local units (see
        /// `geometry::DEFAULT_CURVE_BEND`); adjusted with the middle handle.
        #[serde(default = "default_bend")]
        bend: Pt,
    },
    Rectangle {
        rect: Rect,
        style: Style,
    },
    Ellipse {
        rect: Rect,
        style: Style,
    },
    /// `pos` is the top-left of the text block; `size` is the em size in
    /// image pixels.
    Text {
        pos: Pt,
        text: String,
        color: Rgba,
        size: f32,
    },
    Freehand {
        points: Vec<Pt>,
        style: Style,
    },
    /// Obscures the image under `rect` with `block`-sized squares.
    Pixelate {
        rect: Rect,
        block: f32,
    },
}

fn default_bend() -> Pt {
    crate::geometry::DEFAULT_CURVE_BEND
}

pub type AnnotationId = u64;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Annotation {
    pub id: AnnotationId,
    #[serde(flatten)]
    pub shape: Shape,
}

/// Which handle of a shape is being dragged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    Start,
    End,
    /// Curved arrow: the bend control at the curve's midpoint.
    Mid,
    /// Bounding-box handle: `(hx, hy)` with each in {-1, 0, 1}; -1 = min
    /// side, 1 = max side, 0 = unaffected.
    Box(i8, i8),
}

impl Shape {
    pub fn kind_name(&self) -> &'static str {
        match self {
            Shape::Arrow { .. } => "arrow",
            Shape::CurvedArrow { .. } => "curved_arrow",
            Shape::Rectangle { .. } => "rectangle",
            Shape::Ellipse { .. } => "ellipse",
            Shape::Text { .. } => "text",
            Shape::Freehand { .. } => "freehand",
            Shape::Pixelate { .. } => "pixelate",
        }
    }

    pub fn color(&self) -> Option<Rgba> {
        match self {
            Shape::Arrow { style, .. }
            | Shape::CurvedArrow { style, .. }
            | Shape::Rectangle { style, .. }
            | Shape::Ellipse { style, .. }
            | Shape::Freehand { style, .. } => Some(style.color),
            Shape::Text { color, .. } => Some(*color),
            Shape::Pixelate { .. } => None,
        }
    }

    pub fn set_color(&mut self, c: Rgba) {
        match self {
            Shape::Arrow { style, .. }
            | Shape::CurvedArrow { style, .. }
            | Shape::Rectangle { style, .. }
            | Shape::Ellipse { style, .. }
            | Shape::Freehand { style, .. } => style.color = c,
            Shape::Text { color, .. } => *color = c,
            Shape::Pixelate { .. } => {}
        }
    }

    /// Sets the "size" property: stroke width, text size or block size.
    pub fn set_size(&mut self, v: f32) {
        match self {
            Shape::Arrow { style, .. }
            | Shape::CurvedArrow { style, .. }
            | Shape::Rectangle { style, .. }
            | Shape::Ellipse { style, .. }
            | Shape::Freehand { style, .. } => style.width = v,
            Shape::Text { size, .. } => *size = v,
            Shape::Pixelate { block, .. } => *block = v,
        }
    }

    /// Visual bounds in image space (including stroke width).
    pub fn bounds(&self) -> Rect {
        match self {
            Shape::Arrow { start, end, style } => {
                Rect::from_points(*start, *end).expand(style.width * 2.5 + 4.0)
            }
            Shape::CurvedArrow {
                start,
                end,
                style,
                bend,
            } => {
                let pad = style.width * 2.8 + 5.0;
                let base = Rect::from_points(*start, *end);
                curved_arrow_geometry(*start, *end, style.width, *bend)
                    .and_then(|g| Rect::bounding(&g.spine_points()))
                    .map_or(base, |r| r.union(&base))
                    .expand(pad)
            }
            Shape::Rectangle { rect, style } | Shape::Ellipse { rect, style } => {
                rect.expand(style.width * 0.5)
            }
            Shape::Text { pos, text, size, .. } => {
                let (w, h) = crate::text::measure(text, *size);
                Rect::from_min_size(*pos, w, h)
            }
            Shape::Freehand { points, style } => Rect::bounding(points)
                .unwrap_or(Rect::from_points(Pt::ZERO, Pt::ZERO))
                .expand(style.width * 0.5),
            Shape::Pixelate { rect, .. } => *rect,
        }
    }

    /// Hit test in image space. `tol` is the extra tolerance in image px.
    pub fn hit(&self, p: Pt, tol: f32) -> bool {
        match self {
            Shape::Arrow { start, end, style } => {
                if dist_to_segment(p, *start, *end) <= style.width * 0.5 + tol {
                    return true;
                }
                arrow_geometry(*start, *end, style.width).is_some_and(|g| {
                    g.head_triangles()
                        .iter()
                        .any(|t| point_in_triangle(p, t[0], t[1], t[2]))
                })
            }
            Shape::CurvedArrow {
                start,
                end,
                style,
                bend,
            } => curved_arrow_geometry(*start, *end, style.width, *bend).is_some_and(|g| {
                dist_to_polyline(p, &g.spine_points()) <= style.width * 0.6 + tol
                    || g.head_triangles()
                        .iter()
                        .any(|t| point_in_triangle(p, t[0], t[1], t[2]))
            }),
            Shape::Rectangle { rect, style } => {
                let r = rect;
                let reach = style.width * 0.5 + tol;
                let corners = [
                    r.min,
                    Pt::new(r.max.x, r.min.y),
                    r.max,
                    Pt::new(r.min.x, r.max.y),
                    r.min,
                ];
                dist_to_polyline(p, &corners) <= reach
            }
            Shape::Ellipse { rect, style } => {
                let mut pts = ellipse_points(rect, 72);
                pts.push(pts[0]);
                dist_to_polyline(p, &pts) <= style.width * 0.5 + tol
            }
            Shape::Text { .. } | Shape::Pixelate { .. } => self.bounds().expand(tol).contains(p),
            Shape::Freehand { points, style } => dist_to_polyline(p, points) <= style.width * 0.5 + tol,
        }
    }

    pub fn translate(&mut self, d: Pt) {
        match self {
            Shape::Arrow { start, end, .. } | Shape::CurvedArrow { start, end, .. } => {
                *start = *start + d;
                *end = *end + d;
            }
            Shape::Rectangle { rect, .. } | Shape::Ellipse { rect, .. } | Shape::Pixelate { rect, .. } => {
                *rect = rect.translate(d)
            }
            Shape::Text { pos, .. } => *pos = *pos + d,
            Shape::Freehand { points, .. } => points.iter_mut().for_each(|p| *p = *p + d),
        }
    }

    /// Editable handles and their image-space positions.
    pub fn handles(&self) -> Vec<(Handle, Pt)> {
        let box_handles = |r: &Rect, edges: bool| {
            let mut v = Vec::with_capacity(8);
            for hy in -1i8..=1 {
                for hx in -1i8..=1 {
                    if (hx == 0 && hy == 0) || (!edges && (hx == 0 || hy == 0)) {
                        continue;
                    }
                    let x = match hx {
                        -1 => r.min.x,
                        0 => r.center().x,
                        _ => r.max.x,
                    };
                    let y = match hy {
                        -1 => r.min.y,
                        0 => r.center().y,
                        _ => r.max.y,
                    };
                    v.push((Handle::Box(hx, hy), Pt::new(x, y)));
                }
            }
            v
        };
        match self {
            Shape::Arrow { start, end, .. } => vec![(Handle::Start, *start), (Handle::End, *end)],
            Shape::CurvedArrow { start, end, bend, .. } => vec![
                (Handle::Start, *start),
                (Handle::End, *end),
                (
                    Handle::Mid,
                    crate::geometry::curved_arrow_mid(*start, *end, *bend),
                ),
            ],
            Shape::Rectangle { rect, .. } | Shape::Ellipse { rect, .. } | Shape::Pixelate { rect, .. } => {
                box_handles(rect, true)
            }
            Shape::Freehand { points, .. } => match Rect::bounding(points) {
                Some(r) if r.width() > 1.0 && r.height() > 1.0 => box_handles(&r, false),
                _ => Vec::new(),
            },
            Shape::Text { .. } => Vec::new(),
        }
    }

    /// Returns a copy of `orig` with `handle` dragged to `p`. `constrain`
    /// (Shift) keeps squares/circles/aspect.
    pub fn with_handle_moved(orig: &Shape, handle: Handle, p: Pt, constrain: bool) -> Shape {
        let mut s = orig.clone();
        let move_box = |r: &Rect, hx: i8, hy: i8| -> Rect {
            // Anchor = opposite side.
            let anchor = Pt::new(
                if hx < 0 { r.max.x } else { r.min.x },
                if hy < 0 { r.max.y } else { r.min.y },
            );
            let mut q = p;
            if constrain && hx != 0 && hy != 0 {
                q = square_from(anchor, p);
            }
            let (mut min, mut max) = (r.min, r.max);
            match hx {
                -1 => min.x = q.x,
                1 => max.x = q.x,
                _ => {}
            }
            match hy {
                -1 => min.y = q.y,
                1 => max.y = q.y,
                _ => {}
            }
            Rect::from_points(min, max)
        };
        match (&mut s, handle) {
            (Shape::Arrow { start, end, .. } | Shape::CurvedArrow { start, end, .. }, Handle::Start) => {
                *start = if constrain {
                    crate::geometry::snap_angle(*end, p)
                } else {
                    p
                };
            }
            (Shape::CurvedArrow { start, end, bend, .. }, Handle::Mid) => {
                *bend = crate::geometry::bend_through(*start, *end, p, constrain);
            }
            (Shape::Arrow { start, end, .. } | Shape::CurvedArrow { start, end, .. }, Handle::End) => {
                *end = if constrain {
                    crate::geometry::snap_angle(*start, p)
                } else {
                    p
                };
            }
            (
                Shape::Rectangle { rect, .. } | Shape::Ellipse { rect, .. } | Shape::Pixelate { rect, .. },
                Handle::Box(hx, hy),
            ) => *rect = move_box(rect, hx, hy),
            (Shape::Freehand { points, .. }, Handle::Box(hx, hy)) => {
                if let Some(old) = Rect::bounding(points) {
                    let new = move_box(&old, hx, hy);
                    let sx = new.width() / old.width().max(1e-3);
                    let sy = new.height() / old.height().max(1e-3);
                    for pt in points.iter_mut() {
                        pt.x = new.min.x + (pt.x - old.min.x) * sx;
                        pt.y = new.min.y + (pt.y - old.min.y) * sy;
                    }
                }
            }
            _ => {}
        }
        s
    }

    /// Too small to be intentional (e.g. a click with a drawing tool).
    pub fn is_degenerate(&self) -> bool {
        match self {
            Shape::Arrow { start, end, .. } | Shape::CurvedArrow { start, end, .. } => start.dist(*end) < 3.0,
            Shape::Rectangle { rect, .. } | Shape::Ellipse { rect, .. } | Shape::Pixelate { rect, .. } => {
                rect.width() < 2.0 || rect.height() < 2.0
            }
            Shape::Text { text, .. } => text.trim().is_empty(),
            Shape::Freehand { points, .. } => points.is_empty(),
        }
    }

    pub fn is_finite(&self) -> bool {
        match self {
            Shape::Arrow { start, end, style } => {
                start.is_finite() && end.is_finite() && style.width.is_finite()
            }
            Shape::CurvedArrow {
                start,
                end,
                style,
                bend,
            } => start.is_finite() && end.is_finite() && style.width.is_finite() && bend.is_finite(),
            Shape::Rectangle { rect, style } | Shape::Ellipse { rect, style } => {
                rect.min.is_finite() && rect.max.is_finite() && style.width.is_finite()
            }
            Shape::Text { pos, size, .. } => pos.is_finite() && size.is_finite(),
            Shape::Freehand { points, style } => {
                points.iter().all(|p| p.is_finite()) && style.width.is_finite()
            }
            Shape::Pixelate { rect, block } => {
                rect.min.is_finite() && rect.max.is_finite() && block.is_finite()
            }
        }
    }
}

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Sortable, filesystem-safe unique document id: `<unix-ms>-<random hex>`.
pub fn new_document_id() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut h = RandomState::new().build_hasher();
    h.write_u128(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
    );
    h.write_u32(std::process::id());
    format!("{:013}-{:08x}", now_ms(), h.finish() as u32)
}

/// A document id is only ever used as a directory name; reject anything
/// that could escape the documents directory.
pub fn is_valid_document_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// Persistent, GUI-independent document state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Document {
    #[serde(default)]
    pub format_version: u32,
    pub id: String,
    pub created_ms: u64,
    pub modified_ms: u64,
    pub width: u32,
    pub height: u32,
    /// File name of the stored original inside the document directory.
    pub original_file: String,
    #[serde(default)]
    pub source_path: Option<String>,
    #[serde(default)]
    pub annotations: Vec<Annotation>,
    #[serde(default)]
    pub next_annotation_id: AnnotationId,
}

impl Document {
    pub fn new(width: u32, height: u32, original_file: &str, source_path: Option<String>) -> Self {
        let now = now_ms();
        Self {
            format_version: DOCUMENT_FORMAT_VERSION,
            id: new_document_id(),
            created_ms: now,
            modified_ms: now,
            width,
            height,
            original_file: original_file.to_string(),
            source_path,
            annotations: Vec::new(),
            next_annotation_id: 1,
        }
    }

    pub fn add(&mut self, shape: Shape) -> AnnotationId {
        let id = self.alloc_id();
        self.annotations.push(Annotation { id, shape });
        id
    }

    pub fn alloc_id(&mut self) -> AnnotationId {
        // Guard against hand-edited files with stale counters.
        let max = self.annotations.iter().map(|a| a.id).max().unwrap_or(0);
        self.next_annotation_id = self.next_annotation_id.max(max + 1);
        let id = self.next_annotation_id;
        self.next_annotation_id += 1;
        id
    }

    pub fn get(&self, id: AnnotationId) -> Option<&Annotation> {
        self.annotations.iter().find(|a| a.id == id)
    }

    pub fn get_mut(&mut self, id: AnnotationId) -> Option<&mut Annotation> {
        self.annotations.iter_mut().find(|a| a.id == id)
    }

    pub fn remove(&mut self, id: AnnotationId) -> Option<Annotation> {
        let i = self.annotations.iter().position(|a| a.id == id)?;
        Some(self.annotations.remove(i))
    }

    /// Topmost annotation under `p`, honouring render order (pixelation is
    /// always drawn beneath other annotations).
    pub fn hit_test(&self, p: Pt, tol: f32) -> Option<AnnotationId> {
        let top = self
            .annotations
            .iter()
            .rev()
            .filter(|a| !matches!(a.shape, Shape::Pixelate { .. }))
            .find(|a| a.shape.hit(p, tol));
        top.or_else(|| {
            self.annotations
                .iter()
                .rev()
                .filter(|a| matches!(a.shape, Shape::Pixelate { .. }))
                .find(|a| a.shape.hit(p, tol))
        })
        .map(|a| a.id)
    }

    /// Drops annotations with non-finite geometry (e.g. a corrupted file)
    /// and fixes the id counter. Returns how many were removed.
    pub fn sanitize(&mut self) -> usize {
        let before = self.annotations.len();
        self.annotations.retain(|a| a.shape.is_finite());
        let mut seen = std::collections::HashSet::new();
        self.annotations.retain(|a| seen.insert(a.id));
        let max = self.annotations.iter().map(|a| a.id).max().unwrap_or(0);
        self.next_annotation_id = self.next_annotation_id.max(max + 1);
        before - self.annotations.len()
    }

    pub fn touch(&mut self) {
        self.modified_ms = now_ms().max(self.modified_ms);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style() -> Style {
        Style {
            color: PALETTE[0].1,
            width: 4.0,
        }
    }

    fn sample_doc() -> Document {
        let mut d = Document::new(800, 600, "original.png", Some("/tmp/a.png".into()));
        d.add(Shape::Arrow {
            start: Pt::new(10.0, 10.0),
            end: Pt::new(100.0, 50.0),
            style: style(),
        });
        d.add(Shape::Rectangle {
            rect: Rect::from_points(Pt::new(5.0, 5.0), Pt::new(50.0, 40.0)),
            style: style(),
        });
        d.add(Shape::Ellipse {
            rect: Rect::from_points(Pt::new(200.0, 200.0), Pt::new(300.0, 260.0)),
            style: style(),
        });
        d.add(Shape::Text {
            pos: Pt::new(400.0, 100.0),
            text: "Hello\nworld".into(),
            color: PALETTE[4].1,
            size: 24.0,
        });
        d.add(Shape::Freehand {
            points: vec![Pt::new(0.0, 0.0), Pt::new(5.0, 5.0), Pt::new(10.0, 0.0)],
            style: style(),
        });
        d.add(Shape::Pixelate {
            rect: Rect::from_points(Pt::new(600.0, 400.0), Pt::new(700.0, 500.0)),
            block: 10.0,
        });
        d
    }

    #[test]
    fn serde_roundtrip() {
        let d = sample_doc();
        let json = serde_json::to_string_pretty(&d).unwrap();
        assert!(json.contains("\"kind\": \"arrow\""));
        let back: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn unknown_fields_and_missing_optionals_are_tolerated() {
        let json = r#"{"id":"1-a","created_ms":1,"modified_ms":2,"width":10,"height":10,
            "original_file":"original.png","future_field":true}"#;
        let d: Document = serde_json::from_str(json).unwrap();
        assert!(d.annotations.is_empty());
        assert_eq!(d.source_path, None);
    }

    #[test]
    fn curved_arrows_from_older_files_get_the_default_bend() {
        let json = r#"{"id":1,"kind":"curved_arrow","start":{"x":0,"y":0},"end":{"x":9,"y":9},
            "style":{"color":[255,51,108,255],"width":4.0}}"#;
        let a: Annotation = serde_json::from_str(json).unwrap();
        assert!(
            matches!(a.shape, Shape::CurvedArrow { bend, .. } if bend == crate::geometry::DEFAULT_CURVE_BEND)
        );
        assert_eq!(a.shape.handles().len(), 3, "start, end and bend handles");
    }

    #[test]
    fn ids_are_unique_and_stable() {
        let mut d = sample_doc();
        let ids: Vec<_> = d.annotations.iter().map(|a| a.id).collect();
        let mut sorted = ids.clone();
        sorted.dedup();
        assert_eq!(ids, sorted);
        let removed = d.remove(ids[1]).unwrap();
        assert_eq!(removed.id, ids[1]);
        let new = d.add(Shape::Text {
            pos: Pt::ZERO,
            text: "x".into(),
            color: PALETTE[0].1,
            size: 10.0,
        });
        assert!(!ids.contains(&new));
    }

    #[test]
    fn hit_testing() {
        let d = sample_doc();
        let ids: Vec<_> = d.annotations.iter().map(|a| a.id).collect();
        // On the arrow shaft.
        assert_eq!(d.hit_test(Pt::new(55.0, 30.0), 2.0), Some(ids[0]));
        // On the rectangle border, but not in its empty interior.
        assert_eq!(d.hit_test(Pt::new(50.0, 25.0), 2.0), Some(ids[1]));
        assert_eq!(d.hit_test(Pt::new(30.0, 25.0), 0.0), None);
        // On the ellipse outline (rightmost point).
        assert_eq!(d.hit_test(Pt::new(300.0, 230.0), 2.0), Some(ids[2]));
        assert_eq!(d.hit_test(Pt::new(250.0, 230.0), 2.0), None);
        // Inside the text block.
        assert_eq!(d.hit_test(Pt::new(410.0, 110.0), 0.0), Some(ids[3]));
        // Pixelation area.
        assert_eq!(d.hit_test(Pt::new(650.0, 450.0), 0.0), Some(ids[5]));
        // Empty space.
        assert_eq!(d.hit_test(Pt::new(780.0, 10.0), 2.0), None);
    }

    #[test]
    fn non_pixelate_wins_over_pixelate() {
        let mut d = Document::new(100, 100, "original.png", None);
        let arrow = d.add(Shape::Arrow {
            start: Pt::new(0.0, 50.0),
            end: Pt::new(100.0, 50.0),
            style: style(),
        });
        // Pixelation added later (higher in the list) but rendered below.
        d.add(Shape::Pixelate {
            rect: Rect::from_points(Pt::ZERO, Pt::new(100.0, 100.0)),
            block: 8.0,
        });
        assert_eq!(d.hit_test(Pt::new(50.0, 50.0), 1.0), Some(arrow));
    }

    #[test]
    fn handle_resizing() {
        let r = Shape::Rectangle {
            rect: Rect::from_points(Pt::new(10.0, 10.0), Pt::new(50.0, 30.0)),
            style: style(),
        };
        let moved = Shape::with_handle_moved(&r, Handle::Box(1, 1), Pt::new(80.0, 90.0), false);
        assert_eq!(moved.bounds().max, Pt::new(82.0, 92.0));
        // Dragging past the anchor flips but stays normalised.
        let flipped = Shape::with_handle_moved(&r, Handle::Box(1, 0), Pt::new(0.0, 999.0), false);
        if let Shape::Rectangle { rect, .. } = flipped {
            assert_eq!(rect, Rect::from_points(Pt::new(0.0, 10.0), Pt::new(10.0, 30.0)));
        } else {
            unreachable!()
        }
        let a = Shape::Arrow {
            start: Pt::ZERO,
            end: Pt::new(10.0, 0.0),
            style: style(),
        };
        let a2 = Shape::with_handle_moved(&a, Handle::End, Pt::new(5.0, 7.0), false);
        assert_eq!(a2.handles()[1].1, Pt::new(5.0, 7.0));
    }

    #[test]
    fn freehand_scales_with_box_handles() {
        let f = Shape::Freehand {
            points: vec![Pt::new(0.0, 0.0), Pt::new(10.0, 10.0)],
            style: style(),
        };
        let g = Shape::with_handle_moved(&f, Handle::Box(1, 1), Pt::new(20.0, 40.0), false);
        if let Shape::Freehand { points, .. } = g {
            assert_eq!(points[1], Pt::new(20.0, 40.0));
            assert_eq!(points[0], Pt::new(0.0, 0.0));
        } else {
            unreachable!()
        }
    }

    #[test]
    fn sanitize_drops_bad_annotations() {
        let mut d = sample_doc();
        d.annotations[0].shape = Shape::Arrow {
            start: Pt::new(f32::NAN, 0.0),
            end: Pt::ZERO,
            style: style(),
        };
        let dup = d.annotations[1].clone();
        d.annotations.push(dup);
        d.next_annotation_id = 0;
        assert_eq!(d.sanitize(), 2);
        assert!(d.next_annotation_id > d.annotations.iter().map(|a| a.id).max().unwrap());
    }

    #[test]
    fn document_ids() {
        let a = new_document_id();
        assert!(is_valid_document_id(&a));
        assert!(!is_valid_document_id("../etc"));
        assert!(!is_valid_document_id(""));
        assert_ne!(a, new_document_id());
    }
}
