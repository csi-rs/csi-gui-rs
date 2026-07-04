//! Overlay for toggling the
//! [`crate::data::preprocess::PreprocessConfig`] applied to every CSI
//! window before it reaches a view (or the CSV exporter).

use crate::data::preprocess::{
    Denoising, Detrending, FeatureScaling, Interpolation, PreprocessConfig,
};

#[derive(Clone, Debug)]
pub enum PreprocessOverlayResult {
    Apply(PreprocessConfig),
    Reset,
    Cancel,
    None,
}

#[derive(Default)]
pub struct PreprocessOverlay {
    pub open: bool,
    pub cfg: PreprocessConfig,
}

impl PreprocessOverlay {
    pub fn show(&mut self, ctx: &egui::Context) -> PreprocessOverlayResult {
        if !self.open {
            return PreprocessOverlayResult::None;
        }

        let mut result = PreprocessOverlayResult::None;
        egui::Window::new("Preprocessing")
            .collapsible(false)
            .resizable(true)
            .default_width(520.0)
            .default_height(560.0)
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(
                        "Filters are baked into newly captured packets. \
                         Packets already in the history buffer keep \
                         whatever processing they had when received.",
                    )
                    .weak()
                    .small(),
                );
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Active pipeline:").strong());
                    ui.label(
                        egui::RichText::new(self.cfg.summary())
                            .italics()
                            .color(egui::Color32::LIGHT_BLUE),
                    );
                });
                ui.separator();

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        stage_section(
                            ui,
                            "Interpolation",
                            "Fill missing subcarriers caused by pilot nulls or dropouts.",
                            |ui| {
                                enum_combo(
                                    ui,
                                    "interpolation",
                                    &mut self.cfg.interpolation,
                                    &Interpolation::ALL,
                                );
                            },
                        );
                        ui.separator();
                        stage_section(
                            ui,
                            "Denoising & filters",
                            "Remove noise and outliers across the rolling packet window.",
                            |ui| {
                                enum_combo(
                                    ui,
                                    "denoising",
                                    &mut self.cfg.denoising,
                                    &Denoising::ALL,
                                );
                                denoising_params(ui, &mut self.cfg);
                            },
                        );
                        ui.separator();
                        stage_section(
                            ui,
                            "Detrending",
                            "Remove long-term drift from environmental changes.",
                            |ui| {
                                enum_combo(
                                    ui,
                                    "detrending",
                                    &mut self.cfg.detrending,
                                    &Detrending::ALL,
                                );
                                if matches!(self.cfg.detrending, Detrending::MovingAverageTrend) {
                                    ui.horizontal(|ui| {
                                        ui.label("MA window:");
                                        ui.add(
                                            egui::Slider::new(
                                                &mut self.cfg.params.moving_average_window,
                                                2..=128,
                                            )
                                            .logarithmic(true),
                                        );
                                    });
                                }
                            },
                        );
                        ui.separator();
                        stage_section(
                            ui,
                            "Feature scaling",
                            "Normalise amplitudes for ML models or clearer plots.",
                            |ui| {
                                enum_combo(
                                    ui,
                                    "scaling",
                                    &mut self.cfg.feature_scaling,
                                    &FeatureScaling::ALL,
                                );
                                if !matches!(self.cfg.feature_scaling, FeatureScaling::Off) {
                                    ui.horizontal(|ui| {
                                        ui.checkbox(
                                            &mut self.cfg.params.scaling_per_packet,
                                            "per-packet",
                                        );
                                        ui.label(
                                            egui::RichText::new(
                                                "uncheck for per-subcarrier across the window",
                                            )
                                            .weak()
                                            .small(),
                                        );
                                    });
                                }
                                if matches!(
                                    self.cfg.feature_scaling,
                                    FeatureScaling::Quantization
                                ) {
                                    ui.horizontal(|ui| {
                                        ui.label("Levels:");
                                        ui.add(
                                            egui::Slider::new(
                                                &mut self.cfg.params.quantization_levels,
                                                2..=255,
                                            )
                                            .logarithmic(true),
                                        );
                                    });
                                }
                            },
                        );
                    });

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

fn stage_section(
    ui: &mut egui::Ui,
    title: &str,
    blurb: &str,
    body: impl FnOnce(&mut egui::Ui),
) {
    egui::CollapsingHeader::new(egui::RichText::new(title).strong())
        .default_open(true)
        .show(ui, |ui| {
            ui.label(egui::RichText::new(blurb).weak().small());
            ui.add_space(4.0);
            body(ui);
        });
}

trait StageOption {
    fn label(self) -> &'static str;
    fn tooltip(self) -> &'static str;
}

impl StageOption for Interpolation {
    fn label(self) -> &'static str {
        Interpolation::label(self)
    }
    fn tooltip(self) -> &'static str {
        Interpolation::tooltip(self)
    }
}

impl StageOption for Denoising {
    fn label(self) -> &'static str {
        Denoising::label(self)
    }
    fn tooltip(self) -> &'static str {
        Denoising::tooltip(self)
    }
}

impl StageOption for Detrending {
    fn label(self) -> &'static str {
        Detrending::label(self)
    }
    fn tooltip(self) -> &'static str {
        Detrending::tooltip(self)
    }
}

impl StageOption for FeatureScaling {
    fn label(self) -> &'static str {
        FeatureScaling::label(self)
    }
    fn tooltip(self) -> &'static str {
        FeatureScaling::tooltip(self)
    }
}

fn enum_combo<T: Copy + PartialEq + StageOption>(
    ui: &mut egui::Ui,
    id: &str,
    value: &mut T,
    options: &[T],
) {
    let label = value.label();
    egui::ComboBox::from_id_salt(id)
        .selected_text(label)
        .width(280.0)
        .show_ui(ui, |ui| {
            for opt in options {
                let resp = ui
                    .selectable_value(value, *opt, opt.label())
                    .on_hover_text(opt.tooltip());
                let _ = resp;
            }
        });
}

fn denoising_params(ui: &mut egui::Ui, cfg: &mut PreprocessConfig) {
    match cfg.denoising {
        Denoising::Off => {}
        Denoising::RollingMean | Denoising::RollingMedian => {
            ui.horizontal(|ui| {
                ui.label("Window:");
                ui.add(
                    egui::Slider::new(&mut cfg.params.filter_window, 3..=128)
                        .logarithmic(true),
                );
            });
        }
        Denoising::SavitzkyGolay => {
            ui.horizontal(|ui| {
                ui.label("Window:");
                ui.add(
                    egui::Slider::new(&mut cfg.params.filter_window, 5..=51).step_by(2.0),
                );
            });
            ui.horizontal(|ui| {
                ui.label("Poly order:");
                ui.add(egui::Slider::new(&mut cfg.params.sg_poly_order, 1..=5));
            });
        }
        Denoising::Hampel => {
            ui.horizontal(|ui| {
                ui.label("Window:");
                ui.add(
                    egui::Slider::new(&mut cfg.params.filter_window, 3..=128)
                        .logarithmic(true),
                );
            });
            ui.horizontal(|ui| {
                ui.label("Sigma (MAD):");
                ui.add(egui::Slider::new(&mut cfg.params.hampel_sigma, 1.0..=10.0));
            });
        }
        Denoising::ButterworthLowPass => {
            ui.horizontal(|ui| {
                ui.label("Order:");
                ui.add(egui::Slider::new(&mut cfg.params.butterworth_order, 2..=4).step_by(2.0));
            });
            ui.horizontal(|ui| {
                ui.label("Cutoff (norm.):");
                ui.add(
                    egui::Slider::new(&mut cfg.params.butterworth_cutoff, 0.01..=0.49)
                        .logarithmic(true),
                );
            });
            ui.label(
                egui::RichText::new(
                    "Packet index is treated as uniform time (1 sample/packet).",
                )
                .weak()
                .small(),
            );
        }
        Denoising::WaveletDwt => {
            ui.horizontal(|ui| {
                ui.label("Levels:");
                ui.add(egui::Slider::new(&mut cfg.params.dwt_levels, 1..=6));
            });
            ui.horizontal(|ui| {
                ui.label("Threshold:");
                ui.add(
                    egui::Slider::new(&mut cfg.params.dwt_threshold, 0.01..=10.0)
                        .logarithmic(true),
                );
            });
            ui.checkbox(&mut cfg.params.dwt_soft_threshold, "Soft threshold");
        }
        Denoising::FftDelayFilter => {
            ui.horizontal(|ui| {
                ui.label("CIR taps kept:");
                ui.add(
                    egui::Slider::new(&mut cfg.params.fft_delay_keep_bins, 1..=64)
                        .logarithmic(true),
                );
            });
            ui.label(
                egui::RichText::new(
                    "Per-packet: zero long multipath delays in the CIR, then reconstruct.",
                )
                .weak()
                .small(),
            );
        }
    }
}
