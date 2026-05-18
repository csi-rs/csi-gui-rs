//! Overlay for toggling the
//! [`crate::data::preprocess::PreprocessConfig`] applied to every CSI
//! window before it reaches a view (or the CSV exporter).
//!
//! The overlay is owned by `App`; opening it does not block the rest of
//! the GUI. Each stage is independent and exposes its tunables inline.

use crate::data::preprocess::{Detrending, FeatureScaling, Interpolation, PreprocessConfig};

/// Possible results of running [`PreprocessOverlay::show`] for one
/// frame. Used by `App` to decide whether to update its stored config.
#[derive(Clone, Debug)]
pub enum PreprocessOverlayResult {
    /// User confirmed; persist `cfg`.
    Apply(PreprocessConfig),
    /// User reset to defaults (no-op pipeline).
    Reset,
    /// User dismissed without changing anything.
    Cancel,
    /// Overlay was not open this frame.
    None,
}

/// Modal-overlay state for the preprocessing menu.
#[derive(Default)]
pub struct PreprocessOverlay {
    /// `true` while the overlay window is rendered.
    pub open: bool,
    /// Working copy edited inside the overlay. Copied from the
    /// caller's authoritative config when the overlay is opened.
    pub cfg: PreprocessConfig,
    prev_open: bool,
}

impl PreprocessOverlay {
    /// Render the overlay this frame. The caller copies the result via
    /// [`PreprocessOverlayResult`].
    pub fn show(&mut self, ctx: &egui::Context) -> PreprocessOverlayResult {
        if !self.open {
            self.prev_open = false;
            return PreprocessOverlayResult::None;
        }
        // First frame after open(): nothing to do; the caller is
        // responsible for seeding `self.cfg`.
        self.prev_open = true;

        let mut result = PreprocessOverlayResult::None;
        egui::Window::new("Preprocessing")
            .collapsible(false)
            .resizable(false)
            .default_width(420.0)
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(
                        "Filters are baked into newly captured packets. \
                         Packets already in the history buffer keep \
                         whatever processing they had when received — \
                         changing the settings here does not retro-apply.",
                    )
                    .weak()
                    .small(),
                );
                ui.separator();

                interpolation_row(ui, &mut self.cfg);
                ui.separator();
                detrending_row(ui, &mut self.cfg);
                ui.separator();
                feature_scaling_row(ui, &mut self.cfg);

                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Apply").clicked() {
                        result = PreprocessOverlayResult::Apply(self.cfg.clone());
                        self.open = false;
                    }
                    if ui.button("Reset").clicked() {
                        self.cfg = PreprocessConfig::default();
                        result = PreprocessOverlayResult::Reset;
                    }
                    if ui.button("Cancel").clicked() {
                        result = PreprocessOverlayResult::Cancel;
                        self.open = false;
                    }
                });

                ctx.input(|i| {
                    if i.key_pressed(egui::Key::Escape) {
                        result = PreprocessOverlayResult::Cancel;
                        self.open = false;
                    }
                });
            });
        result
    }
}

fn interpolation_row(ui: &mut egui::Ui, cfg: &mut PreprocessConfig) {
    ui.label(egui::RichText::new("Interpolation").strong());
    ui.horizontal_wrapped(|ui| {
        for (variant, label) in [
            (Interpolation::Off, "Off"),
            (Interpolation::Linear, "Linear"),
            (Interpolation::NearestNeighbor, "Nearest neighbour"),
        ] {
            if ui
                .selectable_label(cfg.interpolation == variant, label)
                .clicked()
            {
                cfg.interpolation = variant;
            }
        }
    });
}

fn detrending_row(ui: &mut egui::Ui, cfg: &mut PreprocessConfig) {
    ui.label(egui::RichText::new("Detrending").strong());
    ui.horizontal_wrapped(|ui| {
        for (variant, label) in [
            (Detrending::Off, "Off"),
            (Detrending::LeastSquaresBaseline, "Least-squares baseline"),
            (Detrending::MovingAverageTrend, "Moving-average trend"),
        ] {
            if ui
                .selectable_label(cfg.detrending == variant, label)
                .clicked()
            {
                cfg.detrending = variant;
            }
        }
    });
    if matches!(cfg.detrending, Detrending::MovingAverageTrend) {
        ui.horizontal(|ui| {
            ui.label("MA window:");
            ui.add(
                egui::Slider::new(&mut cfg.params.moving_average_window, 2..=128)
                    .logarithmic(true),
            );
        });
    }
}

fn feature_scaling_row(ui: &mut egui::Ui, cfg: &mut PreprocessConfig) {
    ui.label(egui::RichText::new("Feature scaling").strong());
    ui.horizontal_wrapped(|ui| {
        for (variant, label) in [
            (FeatureScaling::Off, "Off"),
            (FeatureScaling::MaxMinNormalization, "Max-Min"),
            (FeatureScaling::ZScoreStandardization, "Z-score"),
            (FeatureScaling::Quantization, "Quantize"),
        ] {
            if ui
                .selectable_label(cfg.feature_scaling == variant, label)
                .clicked()
            {
                cfg.feature_scaling = variant;
            }
        }
    });
    if !matches!(cfg.feature_scaling, FeatureScaling::Off) {
        ui.horizontal(|ui| {
            ui.checkbox(&mut cfg.params.scaling_per_packet, "per-packet");
            ui.label(
                egui::RichText::new("uncheck for per-subcarrier across the window")
                    .weak()
                    .small(),
            );
        });
    }
    if matches!(cfg.feature_scaling, FeatureScaling::Quantization) {
        ui.horizontal(|ui| {
            ui.label("Levels:");
            ui.add(
                egui::Slider::new(&mut cfg.params.quantization_levels, 2..=255)
                    .logarithmic(true),
            );
        });
    }
}
