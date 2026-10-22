//! Built-in color themes. Cycle with `T` in the running GUI.

use egui::Color32;
use serde::{Deserialize, Serialize};

/// One of the five built-in color schemes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeVariant {
    Dark,
    Light,
    Nordic,
    Gruvbox,
    Catppuccin,
}

impl ThemeVariant {
    pub const ALL: [ThemeVariant; 5] = [
        ThemeVariant::Dark,
        ThemeVariant::Light,
        ThemeVariant::Nordic,
        ThemeVariant::Gruvbox,
        ThemeVariant::Catppuccin,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ThemeVariant::Dark => "Dark",
            ThemeVariant::Light => "Light",
            ThemeVariant::Nordic => "Nordic",
            ThemeVariant::Gruvbox => "Gruvbox",
            ThemeVariant::Catppuccin => "Catppuccin",
        }
    }

    pub fn next(self) -> ThemeVariant {
        let i = Self::ALL.iter().position(|t| *t == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    pub fn from_u8(v: u8) -> ThemeVariant {
        Self::ALL[(v as usize) % Self::ALL.len()]
    }

    pub fn as_u8(self) -> u8 {
        Self::ALL.iter().position(|t| *t == self).unwrap_or(0) as u8
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub variant: ThemeVariant,
    pub bg: Color32,
    pub fg: Color32,
    pub focused_border: Color32,
    pub normal_border: Color32,
    pub text_highlight: Color32,
    pub text_normal: Color32,
    pub sidebar_selected_bg: Color32,
    pub sidebar_normal_bg: Color32,
    pub gauge_color: Color32,
}

impl Theme {
    pub fn from_variant(v: ThemeVariant) -> Theme {
        match v {
            ThemeVariant::Dark => Theme {
                variant: v,
                bg: Color32::from_rgb(20, 22, 26),
                fg: Color32::from_rgb(230, 230, 230),
                focused_border: Color32::from_rgb(180, 200, 255),
                normal_border: Color32::from_rgb(60, 60, 70),
                text_highlight: Color32::from_rgb(255, 220, 120),
                text_normal: Color32::from_rgb(200, 200, 210),
                sidebar_selected_bg: Color32::from_rgb(45, 60, 90),
                sidebar_normal_bg: Color32::from_rgb(30, 32, 36),
                gauge_color: Color32::from_rgb(120, 200, 255),
            },
            ThemeVariant::Light => Theme {
                variant: v,
                bg: Color32::from_rgb(250, 250, 250),
                fg: Color32::from_rgb(30, 30, 30),
                focused_border: Color32::from_rgb(40, 100, 200),
                normal_border: Color32::from_rgb(190, 190, 190),
                text_highlight: Color32::from_rgb(180, 80, 0),
                text_normal: Color32::from_rgb(60, 60, 60),
                sidebar_selected_bg: Color32::from_rgb(210, 220, 240),
                sidebar_normal_bg: Color32::from_rgb(240, 240, 240),
                gauge_color: Color32::from_rgb(60, 130, 220),
            },
            ThemeVariant::Nordic => Theme {
                variant: v,
                bg: Color32::from_rgb(46, 52, 64),
                fg: Color32::from_rgb(216, 222, 233),
                focused_border: Color32::from_rgb(136, 192, 208),
                normal_border: Color32::from_rgb(76, 86, 106),
                text_highlight: Color32::from_rgb(143, 188, 187),
                text_normal: Color32::from_rgb(216, 222, 233),
                sidebar_selected_bg: Color32::from_rgb(67, 76, 94),
                sidebar_normal_bg: Color32::from_rgb(59, 66, 82),
                gauge_color: Color32::from_rgb(136, 192, 208),
            },
            ThemeVariant::Gruvbox => Theme {
                variant: v,
                bg: Color32::from_rgb(40, 40, 40),
                fg: Color32::from_rgb(235, 219, 178),
                focused_border: Color32::from_rgb(254, 128, 25),
                normal_border: Color32::from_rgb(80, 73, 69),
                text_highlight: Color32::from_rgb(250, 189, 47),
                text_normal: Color32::from_rgb(213, 196, 161),
                sidebar_selected_bg: Color32::from_rgb(60, 56, 54),
                sidebar_normal_bg: Color32::from_rgb(50, 48, 47),
                gauge_color: Color32::from_rgb(254, 128, 25),
            },
            ThemeVariant::Catppuccin => Theme {
                variant: v,
                bg: Color32::from_rgb(30, 30, 46),
                fg: Color32::from_rgb(205, 214, 244),
                focused_border: Color32::from_rgb(203, 166, 247),
                normal_border: Color32::from_rgb(69, 71, 90),
                text_highlight: Color32::from_rgb(245, 194, 231),
                text_normal: Color32::from_rgb(205, 214, 244),
                sidebar_selected_bg: Color32::from_rgb(49, 50, 68),
                sidebar_normal_bg: Color32::from_rgb(36, 39, 58),
                gauge_color: Color32::from_rgb(203, 166, 247),
            },
        }
    }

    pub fn apply_to(&self, ctx: &egui::Context) {
        let mut style = (*ctx.global_style()).clone();
        let mut visuals = if matches!(self.variant, ThemeVariant::Light) {
            egui::Visuals::light()
        } else {
            egui::Visuals::dark()
        };
        visuals.panel_fill = self.bg;
        visuals.window_fill = self.bg;
        visuals.override_text_color = Some(self.fg);
        visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, self.text_normal);
        style.visuals = visuals;
        ctx.set_global_style(style);
    }
}
