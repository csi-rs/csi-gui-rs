use super::{title_bar, ViewCtx};
use egui::Color32;
use egui_plot::{Line, Plot, PlotPoints, Points};
use std::f32::consts::TAU;

/// Flat polar amplitude plot. Angle = subcarrier index (0..N around the
/// circle), radius = amplitude. No depth / tunnel — that was the old
/// pseudo-3D mode. Reference rings are drawn at fixed dB-ish multiples.
pub fn render(ui: &mut egui::Ui, ctx: &mut ViewCtx<'_>) {
    title_bar(ui, "Polar amplitude", ctx);

    let Some(stats) = ctx.history.resolve_anchor(ctx.view_anchor()) else {
        ui.centered_and_justified(|ui| ui.label("Waiting for CSI…"));
        return;
    };
    let Some(csi) = stats.csi.as_ref() else {
        ui.centered_and_justified(|ui| ui.label("No decoded packet."));
        return;
    };
    let n = csi.subcarriers();
    if n == 0 {
        ui.centered_and_justified(|ui| ui.label("Empty CSI payload."));
        return;
    }

    let max_amp = (0..n).map(|i| csi.amplitude(i)).fold(1.0f32, f32::max);

    Plot::new("polar_plot")
        .height(super::plot_height(ctx.plot_budget))
        .data_aspect(1.0)
        .x_axis_label("I")
        .y_axis_label("Q")
        .show_axes([false, false])
        .show_grid([false, false])
        .allow_zoom(true)
        .allow_drag(true)
        .show(ui, |plot_ui| {
            // Reference rings at 25%, 50%, 75%, 100% of max amplitude.
            for frac in [0.25, 0.5, 0.75, 1.0] {
                let r = frac * max_amp as f64;
                let ring: PlotPoints = (0..=128)
                    .map(|k| {
                        let theta = (k as f64 / 128.0) * TAU as f64;
                        [r * theta.cos(), r * theta.sin()]
                    })
                    .collect();
                plot_ui.line(
                    Line::new("", ring)
                        .color(Color32::from_gray(80))
                        .width(0.5_f32),
                );
            }

            // Main trace: connect the points around the circle, then close it.
            let mut pts: Vec<[f64; 2]> = (0..n)
                .map(|i| {
                    let theta = (i as f32 / n as f32) * TAU;
                    let r = csi.amplitude(i);
                    [(r * theta.cos()) as f64, (r * theta.sin()) as f64]
                })
                .collect();
            if let Some(first) = pts.first().copied() {
                pts.push(first);
            }
            plot_ui.line(
                Line::new("|H(k)|", PlotPoints::from(pts.clone()))
                    .color(Color32::from_rgb(110, 200, 255))
                    .width(1.6_f32),
            );
            plot_ui.points(
                Points::new("", PlotPoints::from(pts))
                    .color(Color32::from_rgb(110, 200, 255))
                    .radius(2.0_f32),
            );
        });
}
