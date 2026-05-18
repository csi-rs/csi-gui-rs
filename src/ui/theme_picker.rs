//! Modal picker for the active GUI theme.
//!
//! Replaces the old "cycle on every M-menu click" UX. The picker
//! shows every available [`crate::theme::ThemeVariant`] and lets the
//! user click the one they want.

use crate::theme::ThemeVariant;

/// Possible results returned from [`ThemePicker::show`] each frame.
#[derive(Clone, Copy, Debug)]
pub enum ThemePickerResult {
    /// Overlay was not open.
    None,
    /// User picked a variant; caller should apply it.
    Pick(ThemeVariant),
    /// User dismissed without changing anything.
    Cancel,
}

/// Modal-overlay state for the theme picker.
pub struct ThemePicker {
    /// `true` while the overlay window is rendered.
    pub open: bool,
    /// Currently-selected variant — initialised by the caller before
    /// opening so the modal shows the active theme highlighted.
    pub variant: ThemeVariant,
}

impl Default for ThemePicker {
    fn default() -> Self {
        Self {
            open: false,
            variant: ThemeVariant::Dark,
        }
    }
}

impl ThemePicker {
    /// Render the picker this frame.
    pub fn show(&mut self, ctx: &egui::Context) -> ThemePickerResult {
        if !self.open {
            return ThemePickerResult::None;
        }
        let mut result = ThemePickerResult::None;
        let mut close = false;
        egui::Window::new("Theme")
            .collapsible(false)
            .resizable(false)
            .default_width(240.0)
            .show(ctx, |ui| {
                for variant in ThemeVariant::ALL {
                    let selected = variant == self.variant;
                    if ui
                        .selectable_label(selected, variant.label())
                        .clicked()
                    {
                        self.variant = variant;
                        result = ThemePickerResult::Pick(variant);
                        close = true;
                    }
                }
                ui.separator();
                if ui.button("Cancel").clicked() {
                    result = ThemePickerResult::Cancel;
                    close = true;
                }
                ctx.input(|i| {
                    if i.key_pressed(egui::Key::Escape) {
                        result = ThemePickerResult::Cancel;
                        close = true;
                    }
                });
            });
        if close {
            self.open = false;
        }
        result
    }
}
