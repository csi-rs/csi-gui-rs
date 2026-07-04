use super::colormap::{swatch_widget, viridis};
use super::{subcarrier_x_bounds, title_bar, ViewCtx};
use egui_plot::{Plot, PlotPoints, Polygon};
use std::f32::consts::PI;

/// What the Spectrogram pane colormaps each cell on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SpectrogramMetric {
    /// `|Δφ|` between consecutive packets (motion-sensitive).
    #[default]
    PhaseDelta,
    /// Per-cell `|H(k)|` amplitude (channel-strength-sensitive).
    Amplitude,
}

impl SpectrogramMetric {
    fn label(self) -> &'static str {
        match self {
            Self::PhaseDelta => "|Δφ|",
            Self::Amplitude => "|H(k)|",
        }
    }
}

/// Time × subcarrier heatmap. Continuous viridis-like ramp instead of the
/// old 5-step buckets — small variations are visible because every cell
/// gets a unique color, not a bucket.
pub fn render(ui: &mut egui::Ui, ctx: &mut ViewCtx<'_>) {
    title_bar(ui, "Spectrogram", ctx);

    let mut window_packets = ctx
        .view_state
        .spectrogram_window
        .clamp(2, ctx.history.capacity().max(2));
    let mut metric = ctx.view_state.spectrogram_metric;

    ui.horizontal(|ui| {
        ui.label("Metric:");
        if ui
            .selectable_label(metric == SpectrogramMetric::PhaseDelta, "Phase Δ")
            .clicked()
        {
            metric = SpectrogramMetric::PhaseDelta;
        }
        if ui
            .selectable_label(metric == SpectrogramMetric::Amplitude, "Amplitude")
            .clicked()
        {
            metric = SpectrogramMetric::Amplitude;
        }
        ui.separator();
        ui.label("Window:");
        ui.add(egui::Slider::new(&mut window_packets, 2..=600).logarithmic(true));
    });
    ctx.view_state.spectrogram_window = window_packets;
    ctx.view_state.spectrogram_metric = metric;

    let window = ctx
        .history
        .window_ending_at(ctx.view_anchor(), window_packets);
    if window.len() < 2 {
        ui.centered_and_justified(|ui| ui.label("Need ≥ 2 packets…"));
        return;
    }

    let subcarriers = window
        .iter()
        .filter_map(|s| s.csi.as_ref().map(|c| c.subcarriers()))
        .max()
        .unwrap_or(64)
        .max(1);

    // Two passes: first compute the values into a flat buffer + max so we
    // can normalize once. Second pass emits polygons with continuous color.
    let rows = match metric {
        SpectrogramMetric::PhaseDelta => window.len().saturating_sub(1),
        SpectrogramMetric::Amplitude => window.len(),
    };
    if rows == 0 {
        ui.centered_and_justified(|ui| ui.label("Need more data."));
        return;
    }

    let mut values = vec![f32::NAN; rows * subcarriers];
    let mut max_val = 0f32;
    match metric {
        SpectrogramMetric::PhaseDelta => {
            for (t, pair) in window.windows(2).enumerate() {
                let (Some(prev), Some(cur)) = (pair[0].csi.as_ref(), pair[1].csi.as_ref()) else {
                    continue;
                };
                let limit = subcarriers.min(prev.subcarriers()).min(cur.subcarriers());
                for s in 0..limit {
                    let mut d = cur.phase(s) - prev.phase(s);
                    while d > PI {
                        d -= 2.0 * PI;
                    }
                    while d <= -PI {
                        d += 2.0 * PI;
                    }
                    let v = d.abs();
                    values[t * subcarriers + s] = v;
                    if v > max_val {
                        max_val = v;
                    }
                }
            }
            if max_val < 1e-6 {
                max_val = PI; // sane default so we don't divide by ~0
            }
        }
        SpectrogramMetric::Amplitude => {
            for (t, stats) in window.iter().enumerate() {
                let Some(csi) = stats.csi.as_ref() else { continue };
                let limit = subcarriers.min(csi.subcarriers());
                for s in 0..limit {
                    let v = csi.amplitude(s);
                    values[t * subcarriers + s] = v;
                    if v > max_val {
                        max_val = v;
                    }
                }
            }
            if max_val < 1e-6 {
                max_val = 1.0;
            }
        }
    }

    let (x_lo, x_hi) = subcarrier_x_bounds(subcarriers);
    // Cells are 1-unit wide; shift array index k to centered axis x = k - n/2.
    let x_offset = -(subcarriers as f64 / 2.0).floor();

    Plot::new("spectrogram_plot")
        .height(super::plot_height(ctx.plot_budget))
        .x_axis_label("Subcarrier (DC = 0)")
        .y_axis_label("packet step (older →)")
        .include_x(x_lo)
        .include_x(x_hi + 1.0)
        .include_y(0.0)
        .include_y(rows as f64)
        .show_axes([true, true])
        .show_grid([false, false])
        .allow_zoom(true)
        .allow_drag(true)
        .allow_boxed_zoom(true)
        .show(ui, |plot_ui| {
            for t in 0..rows {
                for s in 0..subcarriers {
                    let v = values[t * subcarriers + s];
                    if v.is_nan() {
                        continue;
                    }
                    let n = (v / max_val).clamp(0.0, 1.0);
                    let color = viridis(n);
                    let x = s as f64 + x_offset;
                    let y = t as f64;
                    let cell: PlotPoints = vec![
                        [x, y],
                        [x + 1.0, y],
                        [x + 1.0, y + 1.0],
                        [x, y + 1.0],
                    ]
                    .into();
                    plot_ui.polygon(Polygon::new("", cell).fill_color(color).stroke((0.0, color)));
                }
            }
        });

    // Footer with the colormap legend (compact swatch + value range).
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("{} range: 0", metric.label()))
                .weak()
                .small(),
        );
        ui.add(swatch_widget(viridis));
        ui.label(
            egui::RichText::new(format_max(max_val, metric))
                .weak()
                .small(),
        );
    });
}

fn format_max(v: f32, m: SpectrogramMetric) -> String {
    match m {
        SpectrogramMetric::PhaseDelta => format!("{v:.2} rad"),
        SpectrogramMetric::Amplitude => format!("{v:.1}"),
    }
}
