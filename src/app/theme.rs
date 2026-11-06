//! Visual styling: light/dark palettes and widget visuals.

use crate::settings::ThemePref;
use eframe::egui::{self, Color32, CornerRadius, Stroke, Theme, ThemePreference};

/// Brand pink, the same as the default annotation colour (#FF336C).
pub const ACCENT: Color32 = Color32::from_rgb(255, 51, 108);

/// Colours not covered by egui's `Visuals`.
#[derive(Clone, Copy)]
pub struct Palette {
    pub canvas: Color32,
    pub panel: Color32,
    pub separator: Color32,
    pub icon: Color32,
    pub icon_muted: Color32,
    pub hover: Color32,
    pub card: Color32,
    pub text_muted: Color32,
    pub checker_a: Color32,
    pub checker_b: Color32,
}

pub fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            canvas: Color32::from_rgb(22, 23, 26),
            panel: Color32::from_rgb(33, 34, 38),
            separator: Color32::from_rgb(50, 52, 58),
            icon: Color32::from_rgb(222, 224, 230),
            icon_muted: Color32::from_rgb(120, 124, 134),
            hover: Color32::from_rgb(48, 50, 57),
            card: Color32::from_rgb(40, 42, 47),
            text_muted: Color32::from_rgb(140, 144, 154),
            checker_a: Color32::from_rgb(58, 60, 66),
            checker_b: Color32::from_rgb(44, 46, 51),
        }
    } else {
        Palette {
            canvas: Color32::from_rgb(226, 228, 233),
            panel: Color32::from_rgb(249, 249, 251),
            separator: Color32::from_rgb(222, 223, 228),
            icon: Color32::from_rgb(48, 50, 58),
            icon_muted: Color32::from_rgb(160, 163, 172),
            hover: Color32::from_rgb(234, 235, 240),
            card: Color32::from_rgb(255, 255, 255),
            text_muted: Color32::from_rgb(110, 114, 124),
            checker_a: Color32::from_rgb(255, 255, 255),
            checker_b: Color32::from_rgb(230, 231, 235),
        }
    }
}

pub fn apply(ctx: &egui::Context, pref: ThemePref) {
    ctx.set_theme(match pref {
        ThemePref::System => ThemePreference::System,
        ThemePref::Light => ThemePreference::Light,
        ThemePref::Dark => ThemePreference::Dark,
    });
}

/// Installs customised visuals for both themes (once, at startup).
pub fn install(ctx: &egui::Context) {
    for theme in [Theme::Light, Theme::Dark] {
        let dark = theme == Theme::Dark;
        let p = palette(dark);
        ctx.style_mut_of(theme, |style| {
            let v = &mut style.visuals;
            v.panel_fill = p.panel;
            v.window_fill = p.card;
            v.extreme_bg_color = p.canvas;
            v.selection.bg_fill = ACCENT.linear_multiply(0.45);
            v.selection.stroke = Stroke::new(1.0, ACCENT);
            v.hyperlink_color = ACCENT;
            v.window_corner_radius = CornerRadius::same(10);
            v.menu_corner_radius = CornerRadius::same(8);
            v.window_stroke = Stroke::new(1.0, p.separator);
            for w in [
                &mut v.widgets.noninteractive,
                &mut v.widgets.inactive,
                &mut v.widgets.hovered,
                &mut v.widgets.active,
                &mut v.widgets.open,
            ] {
                w.corner_radius = CornerRadius::same(6);
            }
            v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.separator);
            v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
            v.widgets.inactive.bg_stroke = Stroke::NONE;
            v.widgets.hovered.weak_bg_fill = p.hover;
            v.widgets.hovered.bg_stroke = Stroke::NONE;
            v.widgets.active.weak_bg_fill = p.hover;
            style.spacing.item_spacing = egui::vec2(6.0, 6.0);
            style.spacing.button_padding = egui::vec2(10.0, 5.0);
            style.interaction.tooltip_delay = 0.35;
        });
    }
}
