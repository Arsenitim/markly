//! CPU renderer used for clipboard copy, export and thumbnails.
//!
//! It never touches the GUI: it composites the original image and the
//! annotation list with tiny-skia at any scale. The interactive preview uses
//! the same geometry helpers (`geometry::arrow_geometry`, `text::layout`,
//! [`pixel_grid`], …) so both outputs match.

use crate::geometry::{Pt, Rect, arrow_geometry, rect_corner_radius};
use crate::model::{Annotation, Rgba, Shape};
use crate::text;
use ab_glyph::{Font, OutlineCurve, ScaleFont};
use image::RgbaImage;
use tiny_skia::{
    BlendMode, FillRule, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, Stroke, Transform,
};

/// Render `anns` on top of `image` at full resolution.
pub fn flatten(image: &RgbaImage, anns: &[Annotation]) -> RgbaImage {
    render_scaled(image, anns, 1.0)
}

/// Render onto `base`, which is the original image already resized by
/// `scale` (used for thumbnails).
pub fn render_scaled(base: &RgbaImage, anns: &[Annotation], scale: f32) -> RgbaImage {
    let Some(mut pixmap) = pixmap_from_image(base) else {
        return base.clone();
    };
    draw_annotations(&mut pixmap, base, anns, scale);
    image_from_pixmap(&pixmap)
}

/// Draws annotations: pixelation first (it obscures the image layer), then
/// everything else in document order.
pub fn draw_annotations(pixmap: &mut Pixmap, base: &RgbaImage, anns: &[Annotation], scale: f32) {
    let t = Transform::from_scale(scale, scale);
    for a in anns {
        if let Shape::Pixelate { rect, block } = &a.shape {
            let r = Rect {
                min: rect.min * scale,
                max: rect.max * scale,
            };
            draw_pixelate(pixmap, base, r, (block * scale).max(1.0));
        }
    }
    for a in anns {
        draw_shape(pixmap, &a.shape, t);
    }
}

fn paint(c: Rgba) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(c.0[0], c.0[1], c.0[2], c.0[3]);
    p.anti_alias = true;
    p
}

fn round_stroke(width: f32) -> Stroke {
    Stroke {
        width: width.max(0.1),
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Stroke::default()
    }
}

pub fn draw_shape(pixmap: &mut Pixmap, shape: &Shape, t: Transform) {
    match shape {
        Shape::Arrow { start, end, style } => {
            if let Some(path) = arrow_path(*start, *end, style.width) {
                pixmap.fill_path(&path, &paint(style.color), FillRule::Winding, t, None);
            }
        }
        Shape::CurvedArrow { start, end, style } => {
            if let Some(path) = curved_arrow_path(*start, *end, style.width) {
                pixmap.fill_path(&path, &paint(style.color), FillRule::Winding, t, None);
            }
        }
        Shape::Rectangle { rect, style } => {
            let r = rect_corner_radius(rect, style.width);
            if let Some(path) = rounded_rect_path(rect, r) {
                pixmap.stroke_path(&path, &paint(style.color), &round_stroke(style.width), t, None);
            }
        }
        Shape::Ellipse { rect, style } => {
            if let Some(r) = tiny_skia::Rect::from_ltrb(rect.min.x, rect.min.y, rect.max.x, rect.max.y)
                && let Some(path) = PathBuilder::from_oval(r)
            {
                pixmap.stroke_path(&path, &paint(style.color), &round_stroke(style.width), t, None);
            }
        }
        Shape::Freehand { points, style } => draw_polyline(pixmap, points, style.width, style.color, t),
        Shape::Text {
            pos,
            text,
            color,
            size,
        } => {
            if let Some(path) = text_path(text, *size, *pos) {
                let halo_w = halo_radius(*size) * 2.0;
                pixmap.stroke_path(&path, &paint(color.halo()), &round_stroke(halo_w), t, None);
                pixmap.fill_path(&path, &paint(*color), FillRule::Winding, t, None);
            }
        }
        Shape::Pixelate { .. } => {} // handled in `draw_annotations`
    }
}

/// Radius of the contrasting outline around text, in image pixels.
pub fn halo_radius(size: f32) -> f32 {
    (size * 0.075).max(1.0)
}

fn draw_polyline(pixmap: &mut Pixmap, pts: &[Pt], width: f32, color: Rgba, t: Transform) {
    let mut pb = PathBuilder::new();
    match pts {
        [] => return,
        [p] => pb.push_circle(p.x, p.y, width * 0.5),
        _ => {
            pb.move_to(pts[0].x, pts[0].y);
            for p in &pts[1..] {
                pb.line_to(p.x, p.y);
            }
        }
    }
    if let Some(path) = pb.finish() {
        if pts.len() == 1 {
            pixmap.fill_path(&path, &paint(color), FillRule::Winding, t, None);
        } else {
            pixmap.stroke_path(&path, &paint(color), &round_stroke(width), t, None);
        }
    }
}

/// Single path for an arrow. Every sub-polygon is emitted with the same
/// orientation so the non-zero fill is a true union (no holes where the
/// shaft runs under the head).
/// Curved arrow: tapered shaft outline + swept head (+ round tail tip), all
/// with the same orientation so the non-zero fill is a union.
pub fn curved_arrow_path(start: Pt, end: Pt, width: f32) -> Option<Path> {
    let g = crate::geometry::curved_arrow_geometry(start, end, width)?;
    let mut pb = PathBuilder::new();
    push_polygon(&mut pb, &g.shaft_outline());
    push_polygon(&mut pb, &g.head);
    let (tail, _, r) = g.spine[0];
    let cap: Vec<Pt> = (0..16)
        .map(|i| {
            let a = i as f32 / 16.0 * std::f32::consts::TAU;
            tail + Pt::new(a.cos(), a.sin()) * r
        })
        .collect();
    push_polygon(&mut pb, &cap);
    pb.finish()
}

pub fn arrow_path(start: Pt, end: Pt, width: f32) -> Option<Path> {
    let g = arrow_geometry(start, end, width)?;
    let mut pb = PathBuilder::new();
    push_polygon(&mut pb, &g.shaft);
    push_polygon(&mut pb, &g.head);
    let cap: Vec<Pt> = (0..24)
        .map(|i| {
            let a = i as f32 / 24.0 * std::f32::consts::TAU;
            g.tail + Pt::new(a.cos(), a.sin()) * g.tail_radius
        })
        .collect();
    push_polygon(&mut pb, &cap);
    pb.finish()
}

fn push_polygon(pb: &mut PathBuilder, pts: &[Pt]) {
    let area: f32 = pts
        .iter()
        .zip(pts.iter().cycle().skip(1))
        .map(|(a, b)| a.x * b.y - b.x * a.y)
        .sum();
    let mut it: Box<dyn Iterator<Item = &Pt>> = if area >= 0.0 {
        Box::new(pts.iter())
    } else {
        Box::new(pts.iter().rev())
    };
    if let Some(first) = it.next() {
        pb.move_to(first.x, first.y);
        for p in it {
            pb.line_to(p.x, p.y);
        }
        pb.close();
    }
}

pub fn rounded_rect_path(r: &Rect, radius: f32) -> Option<Path> {
    let (l, t, rr, b) = (r.min.x, r.min.y, r.max.x, r.max.y);
    if radius <= 0.01 {
        return Some(PathBuilder::from_rect(tiny_skia::Rect::from_ltrb(l, t, rr, b)?));
    }
    // Cubic approximation of quarter circles.
    let k = radius * 0.552_284_8;
    let mut pb = PathBuilder::new();
    pb.move_to(l + radius, t);
    pb.line_to(rr - radius, t);
    pb.cubic_to(rr - radius + k, t, rr, t + radius - k, rr, t + radius);
    pb.line_to(rr, b - radius);
    pb.cubic_to(rr, b - radius + k, rr - radius + k, b, rr - radius, b);
    pb.line_to(l + radius, b);
    pb.cubic_to(l + radius - k, b, l, b - radius + k, l, b - radius);
    pb.line_to(l, t + radius);
    pb.cubic_to(l, t + radius - k, l + radius - k, t, l + radius, t);
    pb.close();
    pb.finish()
}

/// Glyph outlines for a text annotation as a single path in image space.
pub fn text_path(s: &str, size: f32, pos: Pt) -> Option<Path> {
    let font = text::font();
    let layout = text::layout(s, size);
    let sf = font.as_scaled(layout.scale);
    let (hs, vs) = (sf.h_scale_factor(), sf.v_scale_factor());
    let mut pb = PathBuilder::new();
    for (id, origin) in &layout.glyphs {
        let Some(outline) = font.outline(*id) else {
            continue;
        };
        let o = pos + *origin;
        let tr = |p: ab_glyph::Point| (o.x + p.x * hs, o.y - p.y * vs);
        let mut last: Option<ab_glyph::Point> = None;
        for c in &outline.curves {
            let (start, end) = match c {
                OutlineCurve::Line(a, b) | OutlineCurve::Quad(a, _, b) | OutlineCurve::Cubic(a, _, _, b) => {
                    (*a, *b)
                }
            };
            if last != Some(start) {
                if last.is_some() {
                    pb.close();
                }
                let (x, y) = tr(start);
                pb.move_to(x, y);
            }
            match c {
                OutlineCurve::Line(_, b) => {
                    let (x, y) = tr(*b);
                    pb.line_to(x, y);
                }
                OutlineCurve::Quad(_, c1, b) => {
                    let ((x1, y1), (x, y)) = (tr(*c1), tr(*b));
                    pb.quad_to(x1, y1, x, y);
                }
                OutlineCurve::Cubic(_, c1, c2, b) => {
                    let ((x1, y1), (x2, y2), (x, y)) = (tr(*c1), tr(*c2), tr(*b));
                    pb.cubic_to(x1, y1, x2, y2, x, y);
                }
            }
            last = Some(end);
        }
        if last.is_some() {
            pb.close();
        }
    }
    pb.finish()
}

// ---------------------------------------------------------------------------
// Pixelation
// ---------------------------------------------------------------------------

/// Block-averaged colours for a pixelation region. Shared by the export
/// renderer and the interactive preview (which shows it as a tiny
/// nearest-filtered texture), so both obscure exactly the same pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct PixelGrid {
    /// Image-space origin of the grid (integer pixel position).
    pub origin: Pt,
    /// Block edge length in pixels (integer, >= 1).
    pub block: u32,
    pub cols: u32,
    pub rows: u32,
    /// Straight-alpha RGBA per block, row-major.
    pub colors: Vec<[u8; 4]>,
    /// The region actually obscured: rect ∩ image, rounded out to pixels.
    pub clip: Rect,
}

pub fn pixel_grid(image: &RgbaImage, rect: Rect, block: f32) -> Option<PixelGrid> {
    let (iw, ih) = image.dimensions();
    let bounds = Rect::from_points(Pt::ZERO, Pt::new(iw as f32, ih as f32));
    let clip = rect.intersect(&bounds)?;
    let block = (block.round() as u32).max(1);
    let ox = clip.min.x.floor() as u32;
    let oy = clip.min.y.floor() as u32;
    let ex = (clip.max.x.ceil() as u32).min(iw);
    let ey = (clip.max.y.ceil() as u32).min(ih);
    if ex <= ox || ey <= oy {
        return None;
    }
    let cols = (ex - ox).div_ceil(block);
    let rows = (ey - oy).div_ceil(block);
    let mut colors = Vec::with_capacity((cols * rows) as usize);
    let raw = image.as_raw();
    for r in 0..rows {
        let y0 = oy + r * block;
        let y1 = (y0 + block).min(ey);
        for c in 0..cols {
            let x0 = ox + c * block;
            let x1 = (x0 + block).min(ex);
            // Average in premultiplied space so transparent pixels don't
            // bleed their (invisible) colour.
            let (mut sr, mut sg, mut sb, mut sa, mut n) = (0u64, 0u64, 0u64, 0u64, 0u64);
            for y in y0..y1 {
                let row = (y * iw) as usize * 4;
                for x in x0..x1 {
                    let i = row + x as usize * 4;
                    let a = raw[i + 3] as u64;
                    sr += raw[i] as u64 * a;
                    sg += raw[i + 1] as u64 * a;
                    sb += raw[i + 2] as u64 * a;
                    sa += a;
                    n += 1;
                }
            }
            let avg = |v: u64| v.checked_div(sa).unwrap_or(0) as u8;
            let px = [avg(sr), avg(sg), avg(sb), (sa / n.max(1)) as u8];
            colors.push(px);
        }
    }
    // Rounded outward to whole pixels so no original pixel peeks through at
    // fractional edges.
    let clip = Rect {
        min: Pt::new(clip.min.x.floor(), clip.min.y.floor()),
        max: Pt::new(clip.max.x.ceil(), clip.max.y.ceil()),
    };
    Some(PixelGrid {
        origin: Pt::new(ox as f32, oy as f32),
        block,
        cols,
        rows,
        colors,
        clip,
    })
}

fn draw_pixelate(pixmap: &mut Pixmap, base: &RgbaImage, rect: Rect, block: f32) {
    let Some(grid) = pixel_grid(base, rect, block) else {
        return;
    };
    let mut p = Paint {
        blend_mode: BlendMode::Source,
        anti_alias: false,
        ..Paint::default()
    };
    let b = grid.block as f32;
    for r in 0..grid.rows {
        for c in 0..grid.cols {
            let [cr, cg, cb, ca] = grid.colors[(r * grid.cols + c) as usize];
            p.set_color_rgba8(cr, cg, cb, ca);
            let cell = Rect::from_min_size(
                Pt::new(grid.origin.x + c as f32 * b, grid.origin.y + r as f32 * b),
                b,
                b,
            );
            if let Some(cell) = cell.intersect(&grid.clip)
                && let Some(tr) = tiny_skia::Rect::from_ltrb(cell.min.x, cell.min.y, cell.max.x, cell.max.y)
            {
                pixmap.fill_rect(tr, &p, Transform::identity(), None);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Pixel format conversion
// ---------------------------------------------------------------------------

pub fn pixmap_from_image(img: &RgbaImage) -> Option<Pixmap> {
    let (w, h) = img.dimensions();
    let mut pm = Pixmap::new(w, h)?;
    for (dst, src) in pm
        .data_mut()
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(img.as_raw().as_chunks::<4>().0)
    {
        let a = src[3] as u16;
        if a == 255 {
            dst.copy_from_slice(src);
        } else {
            let m = |c: u8| ((c as u16 * a + 127) / 255) as u8;
            dst.copy_from_slice(&[m(src[0]), m(src[1]), m(src[2]), src[3]]);
        }
    }
    Some(pm)
}

pub fn image_from_pixmap(pm: &Pixmap) -> RgbaImage {
    let mut out = Vec::with_capacity(pm.data().len());
    for px in pm.data().as_chunks::<4>().0 {
        let a = px[3] as u16;
        match a {
            255 => out.extend_from_slice(px),
            0 => out.extend_from_slice(&[0, 0, 0, 0]),
            _ => {
                let d = |c: u8| ((c as u16 * 255 + a / 2) / a).min(255) as u8;
                out.extend_from_slice(&[d(px[0]), d(px[1]), d(px[2]), px[3]]);
            }
        }
    }
    RgbaImage::from_raw(pm.width(), pm.height(), out).expect("buffer size matches")
}

/// Composite onto an opaque background (for formats without alpha).
pub fn flatten_alpha(img: &RgbaImage, bg: [u8; 3]) -> image::RgbImage {
    let (w, h) = img.dimensions();
    let mut out = Vec::with_capacity((w * h * 3) as usize);
    for px in img.as_raw().as_chunks::<4>().0 {
        let a = px[3] as u32;
        for i in 0..3 {
            out.push(((px[i] as u32 * a + bg[i] as u32 * (255 - a) + 127) / 255) as u8);
        }
    }
    image::RgbImage::from_raw(w, h, out).expect("buffer size matches")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Document, PALETTE, Style};
    use image::Rgba as Px;

    fn red() -> Rgba {
        PALETTE[0].1
    }

    fn doc_with(shape: Shape) -> Document {
        let mut d = Document::new(200, 100, "original.png", None);
        d.add(shape);
        d
    }

    fn checker(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| {
            if (x / 2 + y / 2) % 2 == 0 {
                Px([255, 255, 255, 255])
            } else {
                Px([0, 0, 0, 255])
            }
        })
    }

    #[test]
    fn export_keeps_dimensions_and_original_untouched() {
        let img = RgbaImage::from_pixel(200, 100, Px([10, 20, 30, 255]));
        let d = doc_with(Shape::Arrow {
            start: Pt::new(10.0, 50.0),
            end: Pt::new(190.0, 50.0),
            style: Style {
                color: red(),
                width: 6.0,
            },
        });
        let out = flatten(&img, &d.annotations);
        assert_eq!(out.dimensions(), (200, 100));
        // The shaft is red, a far corner is untouched.
        assert_eq!(out.get_pixel(100, 50).0, red().0);
        assert_eq!(out.get_pixel(2, 2).0, [10, 20, 30, 255]);
        assert_eq!(img.get_pixel(100, 50).0, [10, 20, 30, 255]);
    }

    #[test]
    fn arrow_union_has_no_holes() {
        let img = RgbaImage::from_pixel(200, 60, Px([255, 255, 255, 255]));
        let d = doc_with(Shape::Arrow {
            start: Pt::new(10.0, 30.0),
            end: Pt::new(190.0, 30.0),
            style: Style {
                color: red(),
                width: 8.0,
            },
        });
        let out = flatten(&img, &d.annotations);
        // Along the axis, from the tail through the head to near the tip.
        for x in 12..186 {
            assert_eq!(out.get_pixel(x, 30).0, red().0, "gap at x={x}");
        }
    }

    #[test]
    fn transparency_is_preserved() {
        let img = RgbaImage::from_pixel(50, 50, Px([0, 0, 0, 0]));
        let half = RgbaImage::from_pixel(4, 4, Px([200, 100, 50, 128]));
        let d = doc_with(Shape::Rectangle {
            rect: Rect::from_points(Pt::new(10.0, 10.0), Pt::new(40.0, 40.0)),
            style: Style {
                color: red(),
                width: 4.0,
            },
        });
        let out = flatten(&img, &d.annotations);
        assert_eq!(out.get_pixel(0, 0).0[3], 0);
        assert_eq!(out.get_pixel(25, 25).0[3], 0);
        assert_eq!(out.get_pixel(25, 10).0, red().0);
        // Semi-transparent pixels survive a round trip through tiny-skia.
        let rt = flatten(&half, &[]);
        let p = rt.get_pixel(1, 1).0;
        assert_eq!(p[3], 128);
        assert!((p[0] as i32 - 200).abs() <= 2 && (p[2] as i32 - 50).abs() <= 2);
    }

    #[test]
    fn pixelation_really_obscures_pixels() {
        let img = checker(64, 64);
        let d = doc_with(Shape::Pixelate {
            rect: Rect::from_points(Pt::new(0.0, 0.0), Pt::new(32.0, 32.0)),
            block: 8.0,
        });
        let out = flatten(&img, &d.annotations);
        // Every 8x8 block inside the region is a single flat colour.
        for by in 0..4 {
            for bx in 0..4 {
                let first = out.get_pixel(bx * 8, by * 8).0;
                for y in 0..8 {
                    for x in 0..8 {
                        assert_eq!(out.get_pixel(bx * 8 + x, by * 8 + y).0, first);
                    }
                }
                // A white/black checker averages to mid-grey.
                assert!((first[0] as i32 - 127).abs() <= 2, "{first:?}");
            }
        }
        // Outside the region the checker is intact.
        assert_eq!(out.get_pixel(40, 40).0, img.get_pixel(40, 40).0);
    }

    #[test]
    fn pixel_grid_clips_to_image() {
        let img = checker(20, 20);
        let g = pixel_grid(
            &img,
            Rect::from_points(Pt::new(15.0, -5.0), Pt::new(40.0, 10.0)),
            4.0,
        )
        .unwrap();
        assert_eq!(g.origin, Pt::new(15.0, 0.0));
        assert_eq!((g.cols, g.rows), (2, 3));
        assert!(
            pixel_grid(
                &img,
                Rect::from_points(Pt::new(30.0, 30.0), Pt::new(40.0, 40.0)),
                4.0
            )
            .is_none()
        );
    }

    #[test]
    fn text_renders_glyphs() {
        let img = RgbaImage::from_pixel(200, 60, Px([255, 255, 255, 255]));
        let d = doc_with(Shape::Text {
            pos: Pt::new(5.0, 5.0),
            text: "Hi!".into(),
            color: PALETTE[7].1,
            size: 32.0,
        });
        let out = flatten(&img, &d.annotations);
        let dark = out.pixels().filter(|p| p.0[0] < 80).count();
        assert!(dark > 100, "expected glyph pixels, got {dark}");
        // Nothing drawn far to the right of the short label.
        assert!(
            out.pixels()
                .enumerate()
                .all(|(i, p)| (i as u32 % 200) < 120 || p.0 == [255, 255, 255, 255])
        );
    }

    #[test]
    fn scaled_render_for_thumbnails() {
        let full = RgbaImage::from_pixel(400, 200, Px([255, 255, 255, 255]));
        let d = doc_with(Shape::Ellipse {
            rect: Rect::from_points(Pt::new(100.0, 50.0), Pt::new(300.0, 150.0)),
            style: Style {
                color: red(),
                width: 8.0,
            },
        });
        let small = image::imageops::thumbnail(&full, 100, 50);
        let out = render_scaled(&small, &d.annotations, 0.25);
        assert_eq!(out.dimensions(), (100, 50));
        // Left-most point of the ellipse at (25, 25) in thumbnail space.
        assert!(out.get_pixel(25, 25).0[1] < 128);
    }

    #[test]
    fn jpeg_background_flattening() {
        let img = RgbaImage::from_pixel(2, 1, Px([0, 0, 0, 0]));
        let rgb = flatten_alpha(&img, [255, 255, 255]);
        assert_eq!(rgb.get_pixel(0, 0).0, [255, 255, 255]);
    }
}
