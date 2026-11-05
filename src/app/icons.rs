//! Hand-drawn vector icons (original artwork) painted with egui shapes,
//! plus the window icon rendered with tiny-skia.

use eframe::egui::{self, Color32, CornerRadius, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Select,
    Arrow,
    Rectangle,
    Ellipse,
    Text,
    Pen,
    Pixelate,
    Open,
    Paste,
    Copy,
    Export,
    Undo,
    Redo,
    History,
    Fit,
    ZoomIn,
    ZoomOut,
    ThemeSystem,
    ThemeLight,
    ThemeDark,
    Trash,
    Image,
}

/// Paints `icon` into `rect`. Icons are designed on a 24×24 grid.
pub fn paint(p: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let s = rect.width().min(rect.height()) / 24.0;
    let o = rect.center() - vec2(12.0 * s, 12.0 * s);
    let at = |x: f32, y: f32| -> Pos2 { o + vec2(x * s, y * s) };
    let w = (1.7 * s).max(1.2);
    let st = Stroke::new(w, color);
    let line = |pts: &[(f32, f32)]| {
        p.add(Shape::line(pts.iter().map(|&(x, y)| at(x, y)).collect(), st));
    };
    let rrect = |x0: f32, y0: f32, x1: f32, y1: f32, r: f32| {
        p.rect_stroke(
            Rect::from_min_max(at(x0, y0), at(x1, y1)),
            CornerRadius::same((r * s).round().clamp(0.0, 255.0) as u8),
            st,
            StrokeKind::Middle,
        );
    };
    let fill_poly = |pts: &[(f32, f32)]| {
        p.add(Shape::convex_polygon(
            pts.iter().map(|&(x, y)| at(x, y)).collect(),
            color,
            Stroke::NONE,
        ));
    };
    let arc = |cx: f32, cy: f32, r: f32, a0: f32, a1: f32| -> Vec<(f32, f32)> {
        (0..=20)
            .map(|i| {
                let a = (a0 + (a1 - a0) * i as f32 / 20.0).to_radians();
                (cx + r * a.cos(), cy + r * a.sin())
            })
            .collect()
    };

    match icon {
        Icon::Select => {
            let pts = [
                (6.5, 3.5),
                (6.5, 19.0),
                (10.6, 15.3),
                (13.4, 21.0),
                (15.9, 19.9),
                (13.2, 14.3),
                (18.6, 14.0),
            ];
            p.add(Shape::closed_line(
                pts.iter().map(|&(x, y)| at(x, y)).collect(),
                Stroke::new(w * 0.9, color),
            ));
            fill_poly(&[(6.5, 3.5), (6.5, 19.0), (10.6, 15.3), (18.6, 14.0)]);
        }
        Icon::Arrow => {
            line(&[(5.0, 19.0), (15.0, 9.0)]);
            fill_poly(&[(19.5, 4.5), (17.6, 12.6), (11.4, 6.4)]);
        }
        Icon::Rectangle => rrect(4.0, 6.0, 20.0, 18.0, 2.5),
        Icon::Ellipse => {
            p.add(Shape::ellipse_stroke(at(12.0, 12.0), vec2(8.5 * s, 6.5 * s), st));
        }
        Icon::Text => {
            let st2 = Stroke::new(w * 1.15, color);
            p.line_segment([at(6.0, 5.5), at(18.0, 5.5)], st2);
            p.line_segment([at(12.0, 5.5), at(12.0, 19.0)], st2);
            p.line_segment([at(9.5, 19.0), at(14.5, 19.0)], st);
        }
        Icon::Pen => {
            let pts: Vec<(f32, f32)> = (0..=24)
                .map(|i| {
                    let t = i as f32 / 24.0;
                    let x = 4.0 + 16.0 * t;
                    let y = 12.0 + 5.5 * (t * std::f32::consts::TAU * 1.1).sin() * (1.0 - 0.3 * t);
                    (x, y)
                })
                .collect();
            line(&pts);
        }
        Icon::Pixelate => {
            for row in 0..3 {
                for col in 0..3 {
                    let a = if (row + col) % 2 == 0 { 1.0 } else { 0.35 };
                    let (x, y) = (5.0 + col as f32 * 4.8, 5.0 + row as f32 * 4.8);
                    p.rect_filled(
                        Rect::from_min_size(at(x, y), vec2(4.4 * s, 4.4 * s)),
                        CornerRadius::same((0.8 * s) as u8),
                        color.gamma_multiply(a),
                    );
                }
            }
        }
        Icon::Open => {
            line(&[
                (3.5, 18.5),
                (3.5, 6.0),
                (9.0, 6.0),
                (11.0, 8.5),
                (20.5, 8.5),
                (20.5, 18.5),
                (3.5, 18.5),
            ]);
            line(&[(3.5, 11.5), (20.5, 11.5)]);
        }
        Icon::Paste => {
            rrect(5.5, 5.0, 18.5, 21.0, 2.0);
            p.rect_filled(
                Rect::from_min_max(at(9.0, 3.0), at(15.0, 7.0)),
                CornerRadius::same((1.2 * s) as u8),
                color,
            );
            line(&[(9.0, 12.0), (15.0, 12.0)]);
            line(&[(9.0, 16.0), (13.0, 16.0)]);
        }
        Icon::Copy => {
            rrect(9.0, 9.0, 20.0, 20.0, 2.0);
            line(&[
                (6.0, 15.0),
                (5.0, 15.0),
                (4.0, 14.0),
                (4.0, 5.0),
                (5.0, 4.0),
                (14.0, 4.0),
                (15.0, 5.0),
                (15.0, 6.0),
            ]);
        }
        Icon::Export => {
            line(&[(12.0, 3.5), (12.0, 14.5)]);
            line(&[(7.5, 10.0), (12.0, 14.5), (16.5, 10.0)]);
            line(&[(4.5, 15.0), (4.5, 19.5), (19.5, 19.5), (19.5, 15.0)]);
        }
        Icon::Undo | Icon::Redo => {
            let mirror = icon == Icon::Redo;
            let m = |(x, y): (f32, f32)| if mirror { (24.0 - x, y) } else { (x, y) };
            let mut pts = vec![m((5.5, 9.5)), m((14.0, 9.5))];
            pts.extend(arc(14.0, 14.0, 4.5, -90.0, 90.0).into_iter().map(m));
            pts.push(m((8.0, 18.5)));
            line(&pts);
            line(&[m((9.0, 6.0)), m((5.5, 9.5)), m((9.0, 13.0))]);
        }
        Icon::History => {
            p.circle_stroke(at(12.0, 12.0), 8.5 * s, st);
            line(&[(12.0, 7.0), (12.0, 12.0), (15.5, 14.0)]);
        }
        Icon::Fit => {
            for (sx, sy) in [(1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)] {
                let c = |x: f32, y: f32| (12.0 + sx * x, 12.0 + sy * y);
                line(&[c(-8.0, -3.5), c(-8.0, -8.0), c(-3.5, -8.0)]);
            }
        }
        Icon::ZoomIn | Icon::ZoomOut => {
            p.circle_stroke(at(10.5, 10.5), 6.5 * s, st);
            line(&[(15.3, 15.3), (20.0, 20.0)]);
            line(&[(7.5, 10.5), (13.5, 10.5)]);
            if icon == Icon::ZoomIn {
                line(&[(10.5, 7.5), (10.5, 13.5)]);
            }
        }
        Icon::ThemeLight => {
            p.circle_stroke(at(12.0, 12.0), 4.0 * s, st);
            for i in 0..8 {
                let a = (i as f32 * 45.0).to_radians();
                let (c, sn) = (a.cos(), a.sin());
                line(&[
                    (12.0 + c * 7.0, 12.0 + sn * 7.0),
                    (12.0 + c * 9.0, 12.0 + sn * 9.0),
                ]);
            }
        }
        Icon::ThemeDark => {
            // Crescent: outer arc of one circle and inner arc of an offset
            // circle, joined at their intersections.
            let mut pts = arc(12.0, 12.0, 8.0, 12.3, 264.4);
            pts.extend(arc(16.5, 8.0, 6.6, 216.9, 59.8));
            pts.push(pts[0]);
            line(&pts);
        }
        Icon::ThemeSystem => {
            p.circle_stroke(at(12.0, 12.0), 8.0 * s, st);
            let half: Vec<Pos2> = arc(12.0, 12.0, 8.0, -90.0, 90.0)
                .into_iter()
                .map(|(x, y)| at(x, y))
                .collect();
            p.add(Shape::convex_polygon(half, color, Stroke::NONE));
        }
        Icon::Trash => {
            line(&[(4.5, 6.5), (19.5, 6.5)]);
            line(&[(9.5, 6.5), (9.5, 4.0), (14.5, 4.0), (14.5, 6.5)]);
            line(&[(6.5, 6.5), (7.5, 20.0), (16.5, 20.0), (17.5, 6.5)]);
        }
        Icon::Image => {
            rrect(3.0, 4.5, 21.0, 19.5, 2.5);
            p.circle_filled(at(8.5, 9.5), 1.8 * s, color);
            line(&[(3.5, 17.5), (9.0, 12.5), (12.5, 15.5), (15.5, 12.0), (20.5, 17.0)]);
        }
    }
}

/// Window icon: an accent-coloured rounded square with a white arrow.
pub fn app_icon() -> egui::IconData {
    use crate::geometry::Pt;
    use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Rect as SkRect, Transform};
    const N: u32 = 128;
    let mut pm = Pixmap::new(N, N).expect("icon pixmap");
    let n = N as f32;
    let bg = crate::render::rounded_rect_path(
        &crate::geometry::Rect::from_points(Pt::new(6.0, 6.0), Pt::new(n - 6.0, n - 6.0)),
        n * 0.22,
    )
    .expect("icon path");
    let mut paint = Paint {
        anti_alias: true,
        ..Paint::default()
    };
    paint.set_color_rgba8(92, 104, 245, 255);
    pm.fill_path(&bg, &paint, FillRule::Winding, Transform::identity(), None);
    // Soft highlight on the upper half.
    paint.set_color_rgba8(255, 255, 255, 28);
    if let Some(r) = SkRect::from_ltrb(6.0, 6.0, n - 6.0, n * 0.5) {
        let mut mask = PathBuilder::new();
        mask.push_rect(r);
        if let Some(hl) = mask.finish() {
            let mut clip = tiny_skia::Mask::new(N, N).expect("mask");
            clip.fill_path(&bg, FillRule::Winding, true, Transform::identity());
            pm.fill_path(&hl, &paint, FillRule::Winding, Transform::identity(), Some(&clip));
        }
    }
    if let Some(arrow) = crate::render::arrow_path(
        Pt::new(n * 0.27, n * 0.73),
        Pt::new(n * 0.76, n * 0.26),
        n * 0.085,
    ) {
        paint.set_color_rgba8(255, 255, 255, 255);
        pm.fill_path(&arrow, &paint, FillRule::Winding, Transform::identity(), None);
    }
    let rgba = crate::render::image_from_pixmap(&pm).into_raw();
    egui::IconData {
        rgba,
        width: N,
        height: N,
    }
}
