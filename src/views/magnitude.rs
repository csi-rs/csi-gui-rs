use super::colormap::{swatch_widget, viridis};
use super::{subcarrier_x, subcarrier_x_bounds, title_bar, ViewCtx};
use egui::Color32;
use egui_plot::{Legend, Line, Plot, PlotPoints, Polygon, VLine};

/// Y-axis scale for the Magnitude pane.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum MagnitudeScale {
    /// Linear `|H(k)|`.
    #[default]
    Linear,
    /// 20·log10(|H|). Standard in RF/sensing — small fade events show up
    /// proportionally instead of being dwarfed by the strong subcarriers.
    Db,
}

/// How the Magnitude pane visualizes the window of recent packets.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum MagnitudeDisplay {
    /// One line per packet, colored by age.
    #[default]
    AllTraces,
    /// Window mean ± 1σ envelope.
    MeanEnvelope,
    /// Subcarrier × time heatmap of |H(k)| (or its dB-scaled value)
    /// across the analysis window. Newest packet at the bottom.
    Heatmap,
}

/// Channel magnitude response: |H(k)| vs subcarrier k.
pub fn render(ui: &mut egui::Ui, ctx: &mut ViewCtx<'_>) {
    title_bar(ui, "Magnitude vs subcarrier", ctx);

    let mut window_size = ctx
        .view_state
        .magnitude_window
        .clamp(1, ctx.history.capacity().max(1));
    let mut scale = ctx.view_state.magnitude_scale;
    let mut display = ctx.view_state.magnitude_display;

    ui.horizontal(|ui| {
        ui.label("Scale:");
        if ui
            .selectable_label(scale == MagnitudeScale::Linear, "Linear")
            .clicked()
        {
            scale = MagnitudeScale::Linear;
        }
        if ui
            .selectable_label(scale == MagnitudeScale::Db, "dB")
            .on_hover_text("20·log10(|H|)")
            .clicked()
        {
            scale = MagnitudeScale::Db;
        }
        ui.separator();
        ui.label("Display:");
        if ui
            .selectable_label(display == MagnitudeDisplay::AllTraces, "Traces")
            .clicked()
        {
            display = MagnitudeDisplay::AllTraces;
        }
        if ui
            .selectable_label(display == MagnitudeDisplay::MeanEnvelope, "Mean ± σ")
            .clicked()
        {
            display = MagnitudeDisplay::MeanEnvelope;
        }
        if ui
            .selectable_label(display == MagnitudeDisplay::Heatmap, "Heatmap")
            .on_hover_text("Subcarrier × time heatmap of the window")
            .clicked()
        {
            display = MagnitudeDisplay::Heatmap;
        }
        ui.separator();
        ui.label("Window:");
        ui.add(egui::Slider::new(&mut window_size, 1..=400).logarithmic(true));
    });
    ctx.view_state.magnitude_window = window_size;
    ctx.view_state.magnitude_scale = scale;
    ctx.view_state.magnitude_display = display;

    let window = ctx.history.window_ending_at(ctx.view_anchor(), window_size);
    if window.is_empty() {
        ui.centered_and_justified(|ui| ui.label("Waiting for CSI…"));
        return;
    }

    let subcarriers = window
        .iter()
        .filter_map(|s| s.csi.as_ref().map(|c| c.subcarriers()))
        .max()
        .unwrap_or(0);
    if subcarriers == 0 {
        ui.centered_and_justified(|ui| ui.label("No decoded packets in window."));
        return;
    }

    // Compute the per-packet trace in the chosen scale. We skip packets with
    // a different subcarrier count rather than padding — the firmware shouldn't
    // mix HT20 and HT40 in the same stream during one analysis window.
    let traces: Vec<Vec<f32>> = window
        .iter()
        .filter_map(|s| s.csi.as_ref())
        .filter(|c| c.subcarriers() == subcarriers)
        .map(|csi| {
            (0..subcarriers)
                .map(|k| convert(csi.amplitude(k), scale))
                .collect()
        })
        .collect();

    if traces.is_empty() {
        ui.centered_and_justified(|ui| ui.label("Subcarrier counts varied — try a smaller window."));
        return;
    }

    let y_label = match scale {
        MagnitudeScale::Linear => "|H(k)|",
        MagnitudeScale::Db => "|H(k)| (dB)",
    };
    let (x_lo, x_hi) = subcarrier_x_bounds(subcarriers);

    if matches!(display, MagnitudeDisplay::Heatmap) {
        render_heatmap(ui, &traces, subcarriers, scale, x_lo, x_hi, ctx.plot_budget);
        return;
    }

    Plot::new("magnitude_plot")
        .height(super::plot_height(ctx.plot_budget))
        .legend(Legend::default())
        .x_axis_label("Subcarrier (DC = 0, − below, + above)")
        .y_axis_label(y_label)
        .include_x(x_lo)
        .include_x(x_hi)
        .show_axes([true, true])
        .show_grid([true, true])
        .allow_zoom(true)
        .allow_drag(true)
        .allow_boxed_zoom(true)
        .show(ui, |plot_ui| {
            plot_ui.vline(VLine::new("DC", 0.0).color(Color32::from_gray(70)));
            match display {
                MagnitudeDisplay::AllTraces => {
                    let n = traces.len();
                    for (idx, trace) in traces.iter().enumerate() {
                        let age = n - 1 - idx;
                        let pts: PlotPoints = trace
                            .iter()
                            .enumerate()
                            .map(|(k, y)| [subcarrier_x(k, subcarriers), *y as f64])
                            .collect();
                        let name = if age == 0 { "latest" } else { "" };
                        plot_ui.line(Line::new(name, pts).color(age_color(age, n)));
                    }
                }
                MagnitudeDisplay::MeanEnvelope => {
                    let (mean, std) = mean_and_std(&traces, subcarriers);
                    let mut poly_pts: Vec<[f64; 2]> = (0..subcarriers)
                        .map(|k| {
                            [subcarrier_x(k, subcarriers), (mean[k] + std[k]) as f64]
                        })
                        .collect();
                    poly_pts.extend(
                        (0..subcarriers)
                            .rev()
                            .map(|k| {
                                [subcarrier_x(k, subcarriers), (mean[k] - std[k]) as f64]
                            }),
                    );
                    plot_ui.polygon(
                        Polygon::new("±1σ", PlotPoints::from(poly_pts))
                            .fill_color(Color32::from_rgba_premultiplied(80, 130, 200, 50))
                            .stroke((0.0, Color32::TRANSPARENT)),
                    );
                    let mean_pts: PlotPoints = mean
                        .iter()
                        .enumerate()
                        .map(|(k, y)| [subcarrier_x(k, subcarriers), *y as f64])
                        .collect();
                    plot_ui.line(
                        Line::new("mean", mean_pts)
                            .color(Color32::from_rgb(110, 200, 255))
                            .width(1.8),
                    );
                }
                // Handled above; left here so adding a new variant fails to
                // compile if it doesn't update one of the two render paths.
                MagnitudeDisplay::Heatmap => {}
            }
        });
}

fn render_heatmap(
    ui: &mut egui::Ui,
    traces: &[Vec<f32>],
    subcarriers: usize,
    scale: MagnitudeScale,
    x_lo: f64,
    x_hi: f64,
    plot_budget: f32,
) {
    // Trace ordering: oldest at index 0, newest last. Render the newest
    // row at the bottom so the heatmap reads top-to-bottom = old-to-new,
    // matching the spectrogram pane.
    let rows = traces.len();
    let mut vmin = f32::INFINITY;
    let mut vmax = f32::NEG_INFINITY;
    for trace in traces {
        for &v in trace {
            if v < vmin {
                vmin = v;
            }
            if v > vmax {
                vmax = v;
            }
        }
    }
    if !vmin.is_finite() || !vmax.is_finite() {
        ui.centered_and_justified(|ui| ui.label("No finite samples to render."));
        return;
    }
    let span = (vmax - vmin).max(1e-6);
    let x_offset = -(subcarriers as f64 / 2.0).floor();

    Plot::new("magnitude_heatmap")
        .height(super::plot_height(plot_budget))
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
            for (t, trace) in traces.iter().enumerate() {
                for (s, &v) in trace.iter().enumerate() {
                    let n = ((v - vmin) / span).clamp(0.0, 1.0);
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

    ui.horizontal(|ui| {
        let (lo_text, hi_text) = match scale {
            MagnitudeScale::Linear => (format!("{vmin:.1}"), format!("{vmax:.1}")),
            MagnitudeScale::Db => (format!("{vmin:.1} dB"), format!("{vmax:.1} dB")),
        };
        ui.label(egui::RichText::new(lo_text).weak().small());
        ui.add(swatch_widget(viridis));
        ui.label(egui::RichText::new(hi_text).weak().small());
    });
}

fn convert(linear: f32, scale: MagnitudeScale) -> f32 {
    match scale {
        MagnitudeScale::Linear => linear,
        // Floor at -120 dB so a zero-valued subcarrier doesn't blow up the
        // axis. (Real data won't hit this — i^2 + q^2 = 0 means no signal.)
        MagnitudeScale::Db => {
            if linear <= 1e-6 {
                -120.0
            } else {
                20.0 * linear.log10()
            }
        }
    }
}

fn mean_and_std(traces: &[Vec<f32>], n_sub: usize) -> (Vec<f32>, Vec<f32>) {
    let n = traces.len() as f32;
    let mut mean = vec![0.0f32; n_sub];
    for t in traces {
        for k in 0..n_sub {
            mean[k] += t[k];
        }
    }
    for v in &mut mean {
        *v /= n;
    }
    let mut var = vec![0.0f32; n_sub];
    for t in traces {
        for k in 0..n_sub {
            let d = t[k] - mean[k];
            var[k] += d * d;
        }
    }
    let std: Vec<f32> = var.iter().map(|v| (v / n).sqrt()).collect();
    (mean, std)
}

fn age_color(age: usize, total: usize) -> Color32 {
    if age == 0 {
        return Color32::from_rgb(110, 200, 255);
    }
    let t = (age as f32 / total.max(1) as f32).clamp(0.0, 1.0);
    let alpha = ((1.0 - t) * 160.0 + 30.0).clamp(30.0, 200.0) as u8;
    let r = (180.0 * (1.0 - t) + 90.0 * t) as u8;
    let g = (200.0 * (1.0 - t) + 100.0 * t) as u8;
    let b = (255.0 * (1.0 - t) + 110.0 * t) as u8;
    Color32::from_rgba_premultiplied(
        (r as u16 * alpha as u16 / 255) as u8,
        (g as u16 * alpha as u16 / 255) as u8,
        (b as u16 * alpha as u16 / 255) as u8,
        alpha,
    )
}
