//! CSI evolution pane — single-curve time series of a derived scalar.
//!
//! The spectrogram + magnitude/phase heatmaps show *per-subcarrier*
//! evolution but compress time into a fixed-rows window. This pane
//! does the opposite: collapse each packet's CSI vector into one
//! scalar and plot that scalar vs packet step over a long history
//! window. Good for spotting slow drifts, motion bursts, and trends
//! that get lost inside a heatmap row.

use super::{ViewCtx, title_bar};
use egui::Color32;
use egui_plot::{Legend, Line, Plot, PlotPoints, VLine};

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum EvolutionMetric {
    /// Mean of `|H(k)|` across all subcarriers — overall channel
    /// strength per packet.
    #[default]
    MeanAmplitude,
    /// Standard deviation of `|H(k)|` across subcarriers — frequency
    /// selectivity per packet. Spikes when the channel becomes
    /// frequency-selective, which often coincides with motion.
    StdAmplitude,
    /// Σ over subcarriers of `||H[t](k)| − |H[t−1](k)||` — frame-to-
    /// frame amplitude change. The textbook CSI-based motion proxy.
    DeltaAmplitude,
    /// Reported RSSI (dBm) — useful sanity baseline against link-
    /// level signal strength.
    Rssi,
}

impl EvolutionMetric {
    fn label(self) -> &'static str {
        match self {
            Self::MeanAmplitude => "Mean |H|",
            Self::StdAmplitude => "Std |H|",
            Self::DeltaAmplitude => "Δ|H| (motion)",
            Self::Rssi => "RSSI",
        }
    }

    fn y_label(self) -> &'static str {
        match self {
            Self::MeanAmplitude => "mean |H(k)|",
            Self::StdAmplitude => "σ over subcarriers",
            Self::DeltaAmplitude => "Σ |Δ|H(k)||",
            Self::Rssi => "RSSI (dBm)",
        }
    }
}

pub fn render(ui: &mut egui::Ui, ctx: &mut ViewCtx<'_>) {
    title_bar(ui, "CSI evolution", ctx);

    let mut window = ctx
        .view_state
        .evolution_window
        .clamp(2, ctx.history.capacity().max(2));
    let mut metric = ctx.view_state.evolution_metric;

    ui.horizontal(|ui| {
        ui.label("Metric:");
        for m in [
            EvolutionMetric::MeanAmplitude,
            EvolutionMetric::StdAmplitude,
            EvolutionMetric::DeltaAmplitude,
            EvolutionMetric::Rssi,
        ] {
            if ui.selectable_label(metric == m, m.label()).clicked() {
                metric = m;
            }
        }
        ui.separator();
        ui.label("Window:");
        ui.add(egui::Slider::new(&mut window, 16..=2000).logarithmic(true));
        if ctx.devices.len() > 1 {
            ui.separator();
            ui.checkbox(&mut ctx.view_state.evolution_compare, "Compare devices")
                .on_hover_text("Overlay every device's curve, each in its colour");
        }
    });
    ctx.view_state.evolution_window = window;
    ctx.view_state.evolution_metric = metric;

    let compare = ctx.view_state.evolution_compare && ctx.devices.len() > 1;
    if compare {
        render_compare(ui, ctx, metric, window);
        return;
    }

    let stats = ctx.history.window_ending_at(ctx.view_anchor(), window);
    if stats.len() < 2 {
        ui.centered_and_justified(|ui| ui.label("Waiting for CSI…"));
        return;
    }

    let curve = compute_curve(metric, &stats);
    if curve.is_empty() {
        ui.centered_and_justified(|ui| ui.label("No decoded packets in window."));
        return;
    }

    // x = packet id within the window so the axis stays stable when
    // replay scrubbing moves the anchor.
    let x0 = stats.first().map(|s| s.id).unwrap_or(0) as f64;
    let pts: PlotPoints = curve
        .iter()
        .enumerate()
        .map(|(i, y)| [x0 + i as f64, *y as f64])
        .collect();

    // Visual marker: rolling-mean and "now" cursor.
    let mean: f32 = curve.iter().sum::<f32>() / curve.len().max(1) as f32;

    // Live-tail view: lock interaction so auto-bounds wins every
    // frame. Without this, the first user pan/zoom flips egui_plot
    // into "manual bounds" mode and the plot stops following the head
    // of the stream until the user keeps scrolling — exactly the
    // "screen stopped following packets" symptom.
    Plot::new("evolution_plot")
        .height(super::plot_height(ctx.plot_budget))
        .legend(Legend::default())
        .x_axis_label("packet step")
        .y_axis_label(metric.y_label())
        .show_axes([true, true])
        .show_grid([true, true])
        .auto_bounds([true, true])
        .allow_zoom(false)
        .allow_drag(false)
        .allow_boxed_zoom(false)
        .allow_scroll(false)
        .show(ui, |plot_ui| {
            plot_ui.line(
                Line::new(metric.label(), pts)
                    .color(ctx.device_color)
                    .width(1.6_f32),
            );
            // Horizontal-mean reference line via two-point polyline so
            // both axes stay finite under autoscale.
            let mean_pts: PlotPoints = vec![
                [x0, mean as f64],
                [x0 + (curve.len() - 1) as f64, mean as f64],
            ]
            .into();
            plot_ui.line(
                Line::new("mean", mean_pts)
                    .color(Color32::from_gray(120))
                    .width(0.8_f32),
            );
            if let Some(anchor) = ctx.view_anchor() {
                plot_ui.vline(
                    VLine::new("replay", anchor as f64).color(Color32::from_rgb(220, 180, 60)),
                );
            }
        });

    let last = curve.last().copied().unwrap_or(0.0);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("latest: {last:.3}"))
                .weak()
                .small(),
        );
        ui.separator();
        ui.label(
            egui::RichText::new(format!("mean: {mean:.3}"))
                .weak()
                .small(),
        );
    });
}

/// Every device's curve on one plot, x = global tick id so the devices
/// line up in time.
fn render_compare(
    ui: &mut egui::Ui,
    ctx: &mut ViewCtx<'_>,
    metric: EvolutionMetric,
    window: usize,
) {
    let anchor = ctx.view_anchor();
    let curves: Vec<(String, Color32, Vec<[f64; 2]>)> = ctx
        .devices
        .iter()
        .filter_map(|d| {
            let stats = d.history.window_ending_at(anchor, window);
            if stats.len() < 2 {
                return None;
            }
            let ys = compute_curve(metric, &stats);
            let pts = stats
                .iter()
                .zip(ys)
                .map(|(s, y)| [s.id as f64, y as f64])
                .collect();
            Some((d.label.clone(), d.color, pts))
        })
        .collect();
    if curves.is_empty() {
        ui.centered_and_justified(|ui| ui.label("Waiting for CSI…"));
        return;
    }
    Plot::new("evolution_compare_plot")
        .height(super::plot_height(ctx.plot_budget))
        .legend(Legend::default())
        .x_axis_label("tick (100 ms)")
        .y_axis_label(metric.y_label())
        .auto_bounds([true, true])
        .allow_zoom(false)
        .allow_drag(false)
        .allow_boxed_zoom(false)
        .allow_scroll(false)
        .show(ui, |plot_ui| {
            for (label, color, pts) in curves {
                plot_ui.line(
                    Line::new(label, PlotPoints::from(pts))
                        .color(color)
                        .width(1.6_f32),
                );
            }
            if let Some(a) = anchor {
                plot_ui
                    .vline(VLine::new("replay", a as f64).color(Color32::from_rgb(220, 180, 60)));
            }
        });
}

/// Reduce one window of `NetworkStats` to a single scalar per packet
/// for the chosen metric. Packets without decoded CSI carry the most
/// recent valid value forward so the curve stays continuous instead
/// of leaving NaN gaps.
fn compute_curve(metric: EvolutionMetric, stats: &[&crate::data::NetworkStats]) -> Vec<f32> {
    let mut out = Vec::with_capacity(stats.len());
    let mut last = 0.0f32;
    let mut prev_amps: Option<Vec<f32>> = None;

    for s in stats {
        let val = match metric {
            EvolutionMetric::Rssi => s.rssi as f32,
            EvolutionMetric::MeanAmplitude
            | EvolutionMetric::StdAmplitude
            | EvolutionMetric::DeltaAmplitude => {
                let Some(csi) = s.csi.as_ref() else {
                    out.push(last);
                    continue;
                };
                let n = csi.subcarriers();
                if n == 0 {
                    out.push(last);
                    continue;
                }
                let amps: Vec<f32> = (0..n).map(|k| csi.amplitude(k)).collect();
                let v = match metric {
                    EvolutionMetric::MeanAmplitude => amps.iter().sum::<f32>() / n as f32,
                    EvolutionMetric::StdAmplitude => {
                        let mean = amps.iter().sum::<f32>() / n as f32;
                        let var: f32 =
                            amps.iter().map(|a| (a - mean).powi(2)).sum::<f32>() / n as f32;
                        var.sqrt()
                    }
                    EvolutionMetric::DeltaAmplitude => match prev_amps.as_ref() {
                        Some(prev) if prev.len() == amps.len() => amps
                            .iter()
                            .zip(prev.iter())
                            .map(|(a, p)| (a - p).abs())
                            .sum::<f32>(),
                        _ => 0.0,
                    },
                    EvolutionMetric::Rssi => unreachable!(),
                };
                prev_amps = Some(amps);
                v
            }
        };
        last = val;
        out.push(val);
    }
    out
}
