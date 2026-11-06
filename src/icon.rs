//! The application icon: a white annotation arrow on a pink rounded
//! square (original artwork). Every format is generated from the geometry
//! here: the runtime window icon, the Linux PNG/SVG icons, the Windows
//! `.ico` and the macOS iconset (`cargo run --example make_icons`).

use crate::geometry::{Pt, Rect, arrow_geometry};
use crate::render;
use image::RgbaImage;
use tiny_skia::{Color, FillRule, GradientStop, LinearGradient, Paint, Pixmap, SpreadMode, Transform};

/// Geometry is defined on a 1024-unit canvas and scaled, so every size has
/// identical proportions.
const REF: f32 = 1024.0;

/// Margin around the rounded square for Linux/Windows icons (fraction of
/// the canvas).
pub const MARGIN_DEFAULT: f32 = 0.047;
/// Apple's icon grid leaves ~10 % around the body.
pub const MARGIN_MACOS: f32 = 0.098;

// Pink around #F92672, lighter at the top.
const TOP: [u8; 3] = [255, 84, 146];
const BOTTOM: [u8; 3] = [234, 22, 98];

struct Geometry {
    body: Rect,
    radius: f32,
    arrow_start: Pt,
    arrow_end: Pt,
    arrow_width: f32,
}

fn geometry(margin: f32) -> Geometry {
    let m = REF * margin;
    let c = REF - 2.0 * m; // body size
    let at = |x: f32, y: f32| Pt::new(m + x * c, m + y * c);
    Geometry {
        body: Rect::from_points(Pt::new(m, m), Pt::new(REF - m, REF - m)),
        radius: c * 0.243,
        arrow_start: at(0.246, 0.754),
        arrow_end: at(0.787, 0.235),
        arrow_width: c * 0.094,
    }
}

/// Renders the icon at `size`×`size` pixels.
pub fn render(size: u32, margin: f32) -> RgbaImage {
    let g = geometry(margin);
    let mut pm = Pixmap::new(size, size).expect("icon size > 0");
    let t = Transform::from_scale(size as f32 / REF, size as f32 / REF);
    let body = render::rounded_rect_path(&g.body, g.radius).expect("icon body");
    // Gentle top-to-bottom gradient on the body.
    let mut paint = Paint {
        anti_alias: true,
        ..Paint::default()
    };
    paint.shader = LinearGradient::new(
        tiny_skia::Point::from_xy(0.0, g.body.min.y),
        tiny_skia::Point::from_xy(0.0, g.body.max.y),
        vec![
            GradientStop::new(0.0, Color::from_rgba8(TOP[0], TOP[1], TOP[2], 255)),
            GradientStop::new(1.0, Color::from_rgba8(BOTTOM[0], BOTTOM[1], BOTTOM[2], 255)),
        ],
        SpreadMode::Pad,
        Transform::identity(),
    )
    .expect("gradient");
    pm.fill_path(&body, &paint, FillRule::Winding, t, None);

    let mut paint = Paint {
        anti_alias: true,
        ..Paint::default()
    };
    if let Some(arrow) = render::arrow_path(g.arrow_start, g.arrow_end, g.arrow_width) {
        paint.set_color_rgba8(255, 255, 255, 255);
        pm.fill_path(&arrow, &paint, FillRule::Winding, t, None);
    }
    render::image_from_pixmap(&pm)
}

/// The same icon as a standalone SVG document.
pub fn svg(margin: f32) -> String {
    let g = geometry(margin);
    let a = arrow_geometry(g.arrow_start, g.arrow_end, g.arrow_width).expect("arrow");
    let poly = |pts: &[Pt]| {
        pts.iter()
            .map(|p| format!("{:.2},{:.2}", p.x, p.y))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let (x, y, w, h) = (g.body.min.x, g.body.min.y, g.body.width(), g.body.height());
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024">
  <defs>
    <linearGradient id="bg" x1="0" y1="{y:.2}" x2="0" y2="{y2:.2}" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="#{top}"/>
      <stop offset="1" stop-color="#{bottom}"/>
    </linearGradient>
  </defs>
  <rect x="{x:.2}" y="{y:.2}" width="{w:.2}" height="{h:.2}" rx="{r:.2}" fill="url(#bg)"/>
  <g fill="#fff">
    <polygon points="{shaft}"/>
    <polygon points="{head}"/>
    <circle cx="{cx:.2}" cy="{cy:.2}" r="{cr:.2}"/>
  </g>
</svg>
"##,
        r = g.radius,
        y2 = g.body.max.y,
        top = format_args!("{:02x}{:02x}{:02x}", TOP[0], TOP[1], TOP[2]),
        bottom = format_args!("{:02x}{:02x}{:02x}", BOTTOM[0], BOTTOM[1], BOTTOM[2]),
        shaft = poly(&a.shaft),
        head = poly(&a.head),
        cx = a.tail.x,
        cy = a.tail.y,
        cr = a.tail_radius,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_all_sizes() {
        for size in [16, 32, 48, 128, 512] {
            let img = render(size, MARGIN_DEFAULT);
            assert_eq!(img.dimensions(), (size, size));
            // Transparent corner, opaque pink body, white arrow in the middle.
            assert_eq!(img.get_pixel(0, 0).0[3], 0);
            // Brightest pixel around the centre (small sizes are antialiased).
            let c = (size / 2 - 1..=size / 2 + 1)
                .flat_map(|x| (size / 2 - 1..=size / 2 + 1).map(move |y| (x, y)))
                .map(|(x, y)| img.get_pixel(x, y).0)
                .max_by_key(|p| p[0] as u32 + p[1] as u32)
                .unwrap();
            assert!(c[0] > 220 && c[1] > 220, "arrow at centre: {c:?}");
        }
        let edge = render(128, MARGIN_DEFAULT).get_pixel(64, 12).0;
        assert_eq!(edge[3], 255);
    }

    #[test]
    fn svg_is_well_formed() {
        let s = svg(MARGIN_MACOS);
        assert!(s.starts_with("<svg") && s.trim_end().ends_with("</svg>"));
        assert_eq!(s.matches("<polygon").count(), 2);
    }
}
