//! Pure geometry: points, rectangles, the view transform between image,
//! canvas and screen space, and shape helpers shared by the interactive
//! preview and the CPU export renderer.
//!
//! Coordinate spaces:
//! * **image space** – pixels of the original image, `(0,0)` = top-left pixel
//!   corner. All annotation geometry is stored in this space.
//! * **canvas space** – logical UI points relative to the top-left of the
//!   canvas widget. Pan/zoom live here.
//! * **screen space** – logical UI points relative to the window (what egui
//!   uses). Physical pixels = screen points × `pixels_per_point`.

use serde::{Deserialize, Serialize};
use std::ops::{Add, Mul, Sub};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Pt {
    pub x: f32,
    pub y: f32,
}

impl Pt {
    pub const ZERO: Pt = Pt { x: 0.0, y: 0.0 };

    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
    pub fn len(self) -> f32 {
        self.x.hypot(self.y)
    }
    pub fn dist(self, o: Pt) -> f32 {
        (self - o).len()
    }
    pub fn dot(self, o: Pt) -> f32 {
        self.x * o.x + self.y * o.y
    }
    /// Unit vector, or zero for a zero-length vector.
    pub fn normalized(self) -> Pt {
        let l = self.len();
        if l > f32::EPSILON {
            self * (1.0 / l)
        } else {
            Pt::ZERO
        }
    }
    /// Perpendicular vector (rotated 90° clockwise in a y-down system).
    pub fn perp(self) -> Pt {
        Pt::new(-self.y, self.x)
    }
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

impl Add for Pt {
    type Output = Pt;
    fn add(self, o: Pt) -> Pt {
        Pt::new(self.x + o.x, self.y + o.y)
    }
}
impl Sub for Pt {
    type Output = Pt;
    fn sub(self, o: Pt) -> Pt {
        Pt::new(self.x - o.x, self.y - o.y)
    }
}
impl Mul<f32> for Pt {
    type Output = Pt;
    fn mul(self, s: f32) -> Pt {
        Pt::new(self.x * s, self.y * s)
    }
}

/// Axis-aligned rectangle. Constructors normalise so `min <= max`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub min: Pt,
    pub max: Pt,
}

impl Rect {
    pub fn from_points(a: Pt, b: Pt) -> Self {
        Self {
            min: Pt::new(a.x.min(b.x), a.y.min(b.y)),
            max: Pt::new(a.x.max(b.x), a.y.max(b.y)),
        }
    }
    pub fn from_min_size(min: Pt, w: f32, h: f32) -> Self {
        Self::from_points(min, Pt::new(min.x + w, min.y + h))
    }
    pub fn width(&self) -> f32 {
        self.max.x - self.min.x
    }
    pub fn height(&self) -> f32 {
        self.max.y - self.min.y
    }
    pub fn center(&self) -> Pt {
        Pt::new((self.min.x + self.max.x) * 0.5, (self.min.y + self.max.y) * 0.5)
    }
    pub fn contains(&self, p: Pt) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }
    pub fn expand(&self, m: f32) -> Rect {
        Rect {
            min: Pt::new(self.min.x - m, self.min.y - m),
            max: Pt::new(self.max.x + m, self.max.y + m),
        }
    }
    pub fn translate(&self, d: Pt) -> Rect {
        Rect {
            min: self.min + d,
            max: self.max + d,
        }
    }
    pub fn union(&self, o: &Rect) -> Rect {
        Rect {
            min: Pt::new(self.min.x.min(o.min.x), self.min.y.min(o.min.y)),
            max: Pt::new(self.max.x.max(o.max.x), self.max.y.max(o.max.y)),
        }
    }
    pub fn intersect(&self, o: &Rect) -> Option<Rect> {
        let r = Rect {
            min: Pt::new(self.min.x.max(o.min.x), self.min.y.max(o.min.y)),
            max: Pt::new(self.max.x.min(o.max.x), self.max.y.min(o.max.y)),
        };
        (r.width() > 0.0 && r.height() > 0.0).then_some(r)
    }
    pub fn bounding(points: &[Pt]) -> Option<Rect> {
        let first = *points.first()?;
        Some(points.iter().fold(Rect::from_points(first, first), |r, p| {
            r.union(&Rect::from_points(*p, *p))
        }))
    }
}

/// Mapping between image, canvas and screen space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewTransform {
    /// Logical points per image pixel.
    pub zoom: f32,
    /// Canvas-space position of the image origin.
    pub pan: Pt,
    /// Screen-space position of the canvas origin.
    pub canvas_min: Pt,
    /// Physical pixels per logical point (HiDPI scale).
    pub pixels_per_point: f32,
}

impl Default for ViewTransform {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan: Pt::ZERO,
            canvas_min: Pt::ZERO,
            pixels_per_point: 1.0,
        }
    }
}

pub const MIN_ZOOM_PERCENT: f32 = 2.0;
pub const MAX_ZOOM_PERCENT: f32 = 3200.0;

impl ViewTransform {
    pub fn image_to_canvas(&self, p: Pt) -> Pt {
        self.pan + p * self.zoom
    }
    pub fn canvas_to_image(&self, c: Pt) -> Pt {
        (c - self.pan) * (1.0 / self.zoom)
    }
    pub fn canvas_to_screen(&self, c: Pt) -> Pt {
        c + self.canvas_min
    }
    pub fn screen_to_canvas(&self, s: Pt) -> Pt {
        s - self.canvas_min
    }
    pub fn image_to_screen(&self, p: Pt) -> Pt {
        self.canvas_to_screen(self.image_to_canvas(p))
    }
    pub fn screen_to_image(&self, s: Pt) -> Pt {
        self.canvas_to_image(self.screen_to_canvas(s))
    }
    pub fn image_rect_to_screen(&self, r: Rect) -> Rect {
        Rect::from_points(self.image_to_screen(r.min), self.image_to_screen(r.max))
    }
    /// Converts a length in screen points to image pixels.
    pub fn screen_len_to_image(&self, l: f32) -> f32 {
        l / self.zoom
    }
    /// Zoom where one image pixel maps to one physical display pixel.
    pub fn actual_size_zoom(&self) -> f32 {
        1.0 / self.pixels_per_point
    }
    /// Zoom expressed relative to physical pixels (100 % = 1:1).
    pub fn zoom_percent(&self) -> f32 {
        self.zoom * self.pixels_per_point * 100.0
    }

    /// Zoom by `factor`, keeping the image point under `anchor` (canvas
    /// space) fixed on screen.
    pub fn zoom_about(&mut self, anchor: Pt, factor: f32) {
        let min = MIN_ZOOM_PERCENT / 100.0 / self.pixels_per_point;
        let max = MAX_ZOOM_PERCENT / 100.0 / self.pixels_per_point;
        let new_zoom = (self.zoom * factor).clamp(min, max);
        let img = self.canvas_to_image(anchor);
        self.zoom = new_zoom;
        self.pan = anchor - img * new_zoom;
    }

    /// Set a zoom level centred on the canvas centre.
    pub fn set_zoom_centered(&mut self, zoom: f32, canvas_size: Pt) {
        let anchor = canvas_size * 0.5;
        self.zoom_about(anchor, zoom / self.zoom);
    }

    /// Fit the image into the canvas with a margin, never upscaling beyond
    /// 1:1 physical pixels (small screenshots stay crisp).
    pub fn fit(&mut self, image_w: f32, image_h: f32, canvas_size: Pt, margin: f32) {
        let avail_w = (canvas_size.x - 2.0 * margin).max(1.0);
        let avail_h = (canvas_size.y - 2.0 * margin).max(1.0);
        let fit = (avail_w / image_w.max(1.0)).min(avail_h / image_h.max(1.0));
        self.zoom = fit.min(self.actual_size_zoom());
        self.center(image_w, image_h, canvas_size);
    }

    pub fn center(&mut self, image_w: f32, image_h: f32, canvas_size: Pt) {
        self.pan = Pt::new(
            (canvas_size.x - image_w * self.zoom) * 0.5,
            (canvas_size.y - image_h * self.zoom) * 0.5,
        );
    }

    /// Keep the image reachable: centre it along axes where it is smaller
    /// than the canvas, otherwise prevent panning beyond its edges (with a
    /// small overscroll margin).
    pub fn clamp_pan(&mut self, image_w: f32, image_h: f32, canvas_size: Pt, margin: f32) {
        let clamp_axis = |pan: f32, img: f32, canvas: f32| {
            let size = img * self.zoom;
            if size + 2.0 * margin <= canvas {
                (canvas - size) * 0.5
            } else {
                pan.clamp(canvas - size - margin, margin)
            }
        };
        self.pan = Pt::new(
            clamp_axis(self.pan.x, image_w, canvas_size.x),
            clamp_axis(self.pan.y, image_h, canvas_size.y),
        );
    }

    /// Round the pan to whole physical pixels so 100 % views are crisp.
    pub fn snap_to_pixels(&mut self) {
        let ppp = self.pixels_per_point;
        let snap = |v: f32, base: f32| ((v + base) * ppp).round() / ppp - base;
        self.pan = Pt::new(
            snap(self.pan.x, self.canvas_min.x),
            snap(self.pan.y, self.canvas_min.y),
        );
    }
}

// ---------------------------------------------------------------------------
// Shape helpers
// ---------------------------------------------------------------------------

/// Distance from `p` to the segment `a`–`b`.
pub fn dist_to_segment(p: Pt, a: Pt, b: Pt) -> f32 {
    let ab = b - a;
    let len2 = ab.dot(ab);
    if len2 <= f32::EPSILON {
        return p.dist(a);
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    p.dist(a + ab * t)
}

pub fn dist_to_polyline(p: Pt, pts: &[Pt]) -> f32 {
    match pts {
        [] => f32::INFINITY,
        [a] => p.dist(*a),
        _ => pts
            .windows(2)
            .map(|w| dist_to_segment(p, w[0], w[1]))
            .fold(f32::INFINITY, f32::min),
    }
}

/// Point-in-triangle test (either winding).
pub fn point_in_triangle(p: Pt, a: Pt, b: Pt, c: Pt) -> bool {
    let cross = |o: Pt, u: Pt, v: Pt| (u.x - o.x) * (v.y - o.y) - (u.y - o.y) * (v.x - o.x);
    let d1 = cross(p, a, b);
    let d2 = cross(p, b, c);
    let d3 = cross(p, c, a);
    let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(neg && pos)
}

/// Arrow outline split into convex pieces so egui (convex fills only) and
/// tiny-skia (path union) render identical geometry.
///
/// The head has swept-back barbs: `head` = tip, left barb, notch, right
/// barb (concave as a whole; egui draws it as two triangles). The shaft is
/// a convex pentagon that runs under the head all the way to the tip, which
/// hides the seam between the two head triangles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArrowGeometry {
    /// tail-left, tail-right, notch-right, tip, notch-left.
    pub shaft: [Pt; 5],
    /// tip, left barb, notch, right barb.
    pub head: [Pt; 4],
    /// Rounded tail cap.
    pub tail: Pt,
    pub tail_radius: f32,
}

impl ArrowGeometry {
    /// The two convex triangles making up the head.
    pub fn head_triangles(&self) -> [[Pt; 3]; 2] {
        let [tip, l, notch, r] = self.head;
        [[tip, l, notch], [tip, notch, r]]
    }
}

pub fn arrow_geometry(start: Pt, end: Pt, width: f32) -> Option<ArrowGeometry> {
    let v = end - start;
    let len = v.len();
    if len < 1.0e-3 || !len.is_finite() {
        return None;
    }
    let dir = v * (1.0 / len);
    let n = dir.perp();
    let w = width.max(0.5);

    let mut head_len = w * 4.0 + 7.0;
    // Short arrows get proportionally smaller heads.
    let max_head = len * 0.65;
    if head_len > max_head {
        head_len = max_head;
    }
    let head_half = head_len * 0.56;
    let back = end - dir * head_len;
    let notch = end - dir * (head_len * 0.78);
    let tail_half = (w * 0.32).max(0.35);
    let base_half = (w * 0.6).min(head_half * 0.5).max(tail_half);

    Some(ArrowGeometry {
        shaft: [
            start + n * tail_half,
            start - n * tail_half,
            notch - n * base_half,
            end,
            notch + n * base_half,
        ],
        head: [end, back + n * head_half, notch, back - n * head_half],
        tail: start,
        tail_radius: tail_half,
    })
}

/// The "curved" arrow: a gently bowed, tapered shaft (thin tail, thick
/// near the head) ending in a large swept-back head that follows the
/// curve's direction.
#[derive(Clone, Debug, PartialEq)]
pub struct CurvedArrowGeometry {
    /// Shaft centre line samples: (point, unit normal, half width).
    pub spine: Vec<(Pt, Pt, f32)>,
    /// tip, left barb, notch, right barb.
    pub head: [Pt; 4],
}

impl CurvedArrowGeometry {
    pub fn head_triangles(&self) -> [[Pt; 3]; 2] {
        let [tip, l, notch, r] = self.head;
        [[tip, l, notch], [tip, notch, r]]
    }

    /// Closed outline of the shaft: left edge forward, right edge back.
    pub fn shaft_outline(&self) -> Vec<Pt> {
        let left = self.spine.iter().map(|(p, n, h)| *p + *n * *h);
        let right = self.spine.iter().rev().map(|(p, n, h)| *p - *n * *h);
        left.chain(right).collect()
    }

    pub fn spine_points(&self) -> Vec<Pt> {
        self.spine.iter().map(|(p, _, _)| *p).collect()
    }
}

/// How far the shaft bows sideways, as a fraction of the arrow length.
const CURVE_BEND: f32 = 0.10;

pub fn curved_arrow_geometry(start: Pt, end: Pt, width: f32) -> Option<CurvedArrowGeometry> {
    let v = end - start;
    let len = v.len();
    if len < 1.0e-3 || !len.is_finite() {
        return None;
    }
    let w = width.max(0.5);
    // Quadratic Bézier start → end, control point pushed to one side.
    let ctrl = (start + end) * 0.5 + v.normalized().perp() * (len * CURVE_BEND);
    let at = |t: f32| {
        let u = 1.0 - t;
        start * (u * u) + ctrl * (2.0 * u * t) + end * (t * t)
    };
    let tangent = |t: f32| ((ctrl - start) * (1.0 - t) + (end - ctrl) * t).normalized();

    let head_len = (w * 5.2 + 10.0).min(len * 0.6);
    let head_half = head_len * 0.62;
    let dir = tangent(1.0);
    let n = dir.perp();
    let back = end - dir * head_len;
    let notch = end - dir * (head_len * 0.72);

    // Shaft runs from the tail until it is safely inside the head.
    let stop = head_len * 0.6;
    let steps = 48;
    let mut spine = Vec::with_capacity(steps + 1);
    let tail_half = (w * 0.18).max(0.3);
    let base_half = (w * 1.0).min(head_half * 0.42).max(tail_half);
    let mut t_stop = 1.0;
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        if at(t).dist(end) <= stop {
            t_stop = t;
            break;
        }
    }
    for i in 0..=steps {
        let t = t_stop * i as f32 / steps as f32;
        let k = t / t_stop.max(1e-6);
        // Ease-out taper: widens quickly, then evens out towards the head.
        let half = tail_half + (base_half - tail_half) * (1.0 - (1.0 - k) * (1.0 - k));
        spine.push((at(t), tangent(t).perp(), half));
    }
    Some(CurvedArrowGeometry {
        spine,
        head: [end, back + n * head_half, notch, back - n * head_half],
    })
}

/// Snap `end` so the vector from `start` is a multiple of 15°.
pub fn snap_angle(start: Pt, end: Pt) -> Pt {
    let v = end - start;
    let len = v.len();
    if len < f32::EPSILON {
        return end;
    }
    let step = std::f32::consts::PI / 12.0;
    let a = (v.y.atan2(v.x) / step).round() * step;
    start + Pt::new(a.cos(), a.sin()) * len
}

/// Make the rect spanned from `anchor` to `p` square.
pub fn square_from(anchor: Pt, p: Pt) -> Pt {
    let d = p - anchor;
    let s = d.x.abs().max(d.y.abs());
    anchor + Pt::new(s.copysign(d.x), s.copysign(d.y))
}

/// Points approximating an ellipse inscribed in `r`.
pub fn ellipse_points(r: &Rect, segments: usize) -> Vec<Pt> {
    let c = r.center();
    let (rx, ry) = (r.width() * 0.5, r.height() * 0.5);
    (0..segments)
        .map(|i| {
            let t = i as f32 / segments as f32 * std::f32::consts::TAU;
            Pt::new(c.x + rx * t.cos(), c.y + ry * t.sin())
        })
        .collect()
}

/// Corner radius used for rectangle annotations.
pub fn rect_corner_radius(r: &Rect, width: f32) -> f32 {
    (width * 1.6 + 2.0)
        .min(r.width() * 0.5)
        .min(r.height() * 0.5)
        .max(0.0)
}

/// Ramer–Douglas–Peucker polyline simplification.
pub fn simplify(points: &[Pt], epsilon: f32) -> Vec<Pt> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut stack = vec![(0usize, points.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let (mut idx, mut dmax) = (0, 0.0f32);
        for i in a + 1..b {
            let d = dist_to_segment(points[i], points[a], points[b]);
            if d > dmax {
                idx = i;
                dmax = d;
            }
        }
        if dmax > epsilon {
            keep[idx] = true;
            stack.push((a, idx));
            stack.push((idx, b));
        }
    }
    points
        .iter()
        .zip(keep)
        .filter_map(|(p, k)| k.then_some(*p))
        .collect()
}

/// Chaikin corner cutting; endpoints are preserved.
pub fn chaikin(points: &[Pt], iterations: usize) -> Vec<Pt> {
    let mut pts = points.to_vec();
    for _ in 0..iterations {
        if pts.len() < 3 {
            break;
        }
        let mut out = Vec::with_capacity(pts.len() * 2);
        out.push(pts[0]);
        for w in pts.windows(2) {
            let (a, b) = (w[0], w[1]);
            out.push(a * 0.75 + b * 0.25);
            out.push(a * 0.25 + b * 0.75);
        }
        out.push(*pts.last().unwrap());
        pts = out;
    }
    pts
}

/// Turn raw pointer samples into a smooth, compact stroke.
pub fn smooth_stroke(raw: &[Pt], width: f32) -> Vec<Pt> {
    let eps = (width * 0.15).clamp(0.3, 2.0);
    chaikin(&simplify(raw, eps), 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: Pt, b: Pt) -> bool {
        a.dist(b) < 1e-3
    }

    #[test]
    fn view_roundtrip() {
        let v = ViewTransform {
            zoom: 0.37,
            pan: Pt::new(12.5, -40.0),
            canvas_min: Pt::new(52.0, 44.0),
            pixels_per_point: 2.0,
        };
        for p in [Pt::new(0.0, 0.0), Pt::new(1920.0, 1080.0), Pt::new(13.3, 999.9)] {
            assert!(approx(v.screen_to_image(v.image_to_screen(p)), p));
            assert!(approx(v.canvas_to_image(v.image_to_canvas(p)), p));
        }
    }

    #[test]
    fn zoom_about_keeps_anchor_fixed() {
        let mut v = ViewTransform {
            zoom: 0.5,
            pan: Pt::new(10.0, 20.0),
            ..Default::default()
        };
        let anchor = Pt::new(300.0, 200.0);
        let before = v.canvas_to_image(anchor);
        v.zoom_about(anchor, 2.5);
        assert!((v.zoom - 1.25).abs() < 1e-6);
        assert!(approx(v.canvas_to_image(anchor), before));
    }

    #[test]
    fn fit_respects_aspect_and_never_upscales() {
        let mut v = ViewTransform {
            pixels_per_point: 2.0,
            ..Default::default()
        };
        v.fit(4000.0, 1000.0, Pt::new(1000.0, 800.0), 0.0);
        assert!((v.zoom - 0.25).abs() < 1e-6);
        assert!((v.pan.y - (800.0 - 250.0) / 2.0).abs() < 1e-4);
        // A tiny image is shown at 1:1 physical pixels, not stretched.
        v.fit(100.0, 50.0, Pt::new(1000.0, 800.0), 0.0);
        assert!((v.zoom - 0.5).abs() < 1e-6);
        assert!((v.zoom_percent() - 100.0).abs() < 1e-3);
    }

    #[test]
    fn clamp_pan_centres_small_images() {
        let mut v = ViewTransform {
            zoom: 1.0,
            pan: Pt::new(-500.0, 900.0),
            ..Default::default()
        };
        v.clamp_pan(100.0, 100.0, Pt::new(400.0, 300.0), 10.0);
        assert!(approx(v.pan, Pt::new(150.0, 100.0)));
    }

    #[test]
    fn segment_distance() {
        let a = Pt::new(0.0, 0.0);
        let b = Pt::new(10.0, 0.0);
        assert_eq!(dist_to_segment(Pt::new(5.0, 3.0), a, b), 3.0);
        assert_eq!(dist_to_segment(Pt::new(-4.0, 3.0), a, b), 5.0);
        assert_eq!(dist_to_segment(Pt::new(1.0, 1.0), a, a), 2f32.sqrt());
    }

    #[test]
    fn arrow_geometry_is_proportional() {
        let g = arrow_geometry(Pt::new(0.0, 0.0), Pt::new(200.0, 0.0), 6.0).unwrap();
        assert!(approx(g.head[0], Pt::new(200.0, 0.0)));
        let head_len = 200.0 - g.head[1].x;
        assert!(head_len > 6.0 * 3.0 && head_len < 6.0 * 6.0);
        // Barbs sweep back behind the notch.
        assert!(g.head[2].x > g.head[1].x);
        // Symmetric about the axis.
        assert!((g.head[1].y + g.head[3].y).abs() < 1e-4);
        // Short arrows shrink the head instead of overshooting the tail.
        let g = arrow_geometry(Pt::new(0.0, 0.0), Pt::new(10.0, 0.0), 6.0).unwrap();
        assert!(g.head[1].x >= 3.49);
        assert!(arrow_geometry(Pt::new(1.0, 1.0), Pt::new(1.0, 1.0), 4.0).is_none());
    }

    #[test]
    fn curved_arrow_tapers_and_bows() {
        let g = curved_arrow_geometry(Pt::new(0.0, 0.0), Pt::new(300.0, 0.0), 8.0).unwrap();
        let first = g.spine.first().unwrap();
        let last = g.spine.last().unwrap();
        assert!(first.2 < last.2, "shaft gets thicker towards the head");
        // The middle of the shaft is pushed to one side of the straight line.
        let mid = g.spine[g.spine.len() / 2].0;
        assert!(mid.y.abs() > 10.0, "bowed: {mid:?}");
        assert!(approx(g.head[0], Pt::new(300.0, 0.0)));
        assert_eq!(g.shaft_outline().len(), g.spine.len() * 2);
        assert!(curved_arrow_geometry(Pt::ZERO, Pt::ZERO, 4.0).is_none());
    }

    #[test]
    fn snapping() {
        let e = snap_angle(Pt::ZERO, Pt::new(100.0, 4.0));
        assert!(e.y.abs() < 1e-3);
        let s = square_from(Pt::ZERO, Pt::new(-30.0, 10.0));
        assert!(approx(s, Pt::new(-30.0, 30.0)));
    }

    #[test]
    fn simplify_and_smooth() {
        let line: Vec<Pt> = (0..50).map(|i| Pt::new(i as f32, 0.0)).collect();
        assert_eq!(simplify(&line, 0.5).len(), 2);
        let pts = vec![Pt::new(0.0, 0.0), Pt::new(10.0, 10.0), Pt::new(20.0, 0.0)];
        let s = chaikin(&pts, 2);
        assert_eq!(s.first(), pts.first());
        assert_eq!(s.last(), pts.last());
        assert!(s.len() > pts.len());
    }

    #[test]
    fn triangle_containment() {
        let (a, b, c) = (Pt::new(0.0, 0.0), Pt::new(10.0, 0.0), Pt::new(0.0, 10.0));
        assert!(point_in_triangle(Pt::new(2.0, 2.0), a, b, c));
        assert!(point_in_triangle(Pt::new(2.0, 2.0), a, c, b));
        assert!(!point_in_triangle(Pt::new(8.0, 8.0), a, b, c));
    }
}
