//! Persisted user preferences (`settings.json`).

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tool {
    Select,
    Arrow,
    Rectangle,
    Ellipse,
    Text,
    Freehand,
    Pixelate,
}

impl Tool {
    pub const ALL: [Tool; 7] = [
        Tool::Select,
        Tool::Arrow,
        Tool::Rectangle,
        Tool::Ellipse,
        Tool::Text,
        Tool::Freehand,
        Tool::Pixelate,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Arrow => "Arrow",
            Tool::Rectangle => "Rectangle",
            Tool::Ellipse => "Ellipse",
            Tool::Text => "Text",
            Tool::Freehand => "Pen",
            Tool::Pixelate => "Pixelate",
        }
    }

    /// Single-key shortcut (when not editing text).
    pub fn key_hint(self) -> &'static str {
        match self {
            Tool::Select => "V or hold Alt",
            Tool::Arrow => "A",
            Tool::Rectangle => "R",
            Tool::Ellipse => "E",
            Tool::Text => "T",
            Tool::Freehand => "P",
            Tool::Pixelate => "X",
        }
    }

    pub fn has_color(self) -> bool {
        !matches!(self, Tool::Select | Tool::Pixelate)
    }
}

/// Palette and size indices remembered per tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolStyle {
    pub color: usize,
    pub size: usize,
}

impl Default for ToolStyle {
    fn default() -> Self {
        ToolStyle { color: 0, size: 2 }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolStyles {
    pub arrow: ToolStyle,
    pub rectangle: ToolStyle,
    pub ellipse: ToolStyle,
    pub text: ToolStyle,
    pub freehand: ToolStyle,
    pub pixelate: ToolStyle,
}

impl ToolStyles {
    pub fn get(&self, t: Tool) -> Option<ToolStyle> {
        Some(match t {
            Tool::Select => return None,
            Tool::Arrow => self.arrow,
            Tool::Rectangle => self.rectangle,
            Tool::Ellipse => self.ellipse,
            Tool::Text => self.text,
            Tool::Freehand => self.freehand,
            Tool::Pixelate => self.pixelate,
        })
    }

    pub fn get_mut(&mut self, t: Tool) -> Option<&mut ToolStyle> {
        Some(match t {
            Tool::Select => return None,
            Tool::Arrow => &mut self.arrow,
            Tool::Rectangle => &mut self.rectangle,
            Tool::Ellipse => &mut self.ellipse,
            Tool::Text => &mut self.text,
            Tool::Freehand => &mut self.freehand,
            Tool::Pixelate => &mut self.pixelate,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemePref {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub last_document: Option<String>,
    /// Inner window size in logical points.
    pub window_size: Option<[f32; 2]>,
    pub tool: Tool,
    pub styles: ToolStyles,
    pub theme: ThemePref,
    pub history_open: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            last_document: None,
            window_size: None,
            tool: Tool::Arrow,
            styles: ToolStyles::default(),
            theme: ThemePref::System,
            history_open: false,
        }
    }
}
