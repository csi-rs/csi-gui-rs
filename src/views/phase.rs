use super::colormap::{coolwarm_signed, swatch_widget, viridis};
use super::{subcarrier_x, subcarrier_x_bounds, title_bar, ViewCtx};
use egui::Color32;
use egui_plot::{HLine, Legend, Line, Plot, PlotPoints, Polygon, VLine};
use std::f32::consts::PI;

/// How the Phase pane preprocesses `∠H(k)` before plotting.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum PhaseMode {
    /// Raw `atan2(Q, I)` in (-π, π]. Has visible 2π jumps.
    Raw,
    /// Cumulative-diff unwrap. Removes ±2π discontinuities so the trace
    /// is continuous across subcarriers.
    #[default]
    Unwrapped,
    /// Unwrap then subtract a least-squares linear fit. Strips the timing-
    /// offset slope that otherwise dominates and reveals the per-subcarrier
    /// channel variations.
    Detrended,
}

/// How the Phase pane visualizes the window of recent packets.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum PhaseDisplay {
    /// One line per packet, colored by age.
    #[default]
    AllTraces,
    /// Mean across the window + ±1σ shaded envelope. Easier to spot drift
    /// vs jitter than a tangle of lines.
    MeanEnvelope,
    /// Subcarrier × time heatmap of the chosen phase mode. Raw / detrended
    /// use a diverging coolwarm colormap (centered on 0); unwrapped uses
    /// viridis on the raw value range — both modes pick sensible defaults
    /// automatically.
    Heatmap,
}

/// Render the Phase pane: per-packet `∠H(k)` traces, mean envelope,
/// or subcarrier × time heatmap of the chosen [`PhaseMode`].
pub fn render(ui: &mut egui::Ui, ctx: &mut ViewCtx<'_>) {
    title_bar(ui, "Phase vs subcarrier", ctx);

    let mut window_size = ctx
        .view_state
        .phase_window
        .clamp(1, ctx.history.capacity().max(1));
    let mut mode = ctx.view_state.phase_mode;
    let mut display = ctx.view_state.phase_display;

    ui.horizontal(|ui| {
        ui.label("Mode:");
        if ui
            .selectable_label(mode == PhaseMode::Raw, "Raw")
            .on_hover_text("Wrapped atan2(Q, I)")
            .clicked()
        {
            mode = PhaseMode::Raw;
        }
        if ui
            .selectable_label(mode == PhaseMode::Unwrapped, "Unwrapped")
            .on_hover_text("Continuous unwrap across subcarriers")
            .clicked()
        {
            mode = PhaseMode::Unwrapped;
        }
        if ui
            .selectable_label(mode == PhaseMode::Detrended, "Detrended")
            .on_hover_text("Unwrapped minus least-squares linear fit")
            .clicked()
        {
            mode = PhaseMode::Detrended;
        }
        ui.separator();
        ui.label("Display:");
        if ui
            .selectable_label(display == PhaseDisplay::AllTraces, "Traces")
            .clicked()
        {
            display = PhaseDisplay::AllTraces;
        }
        if ui
            .selectable_label(display == PhaseDisplay::MeanEnvelope, "Mean ± σ")
            .clicked()
        {
            display = PhaseDisplay::MeanEnvelope;
        }
        if ui
            .selectable_label(display == PhaseDisplay::Heatmap, "Heatmap")
            .on_hover_text("Subcarrier × time heatmap of the window")
            .clicked()
        {
            display = PhaseDisplay::Heatmap;
        }
        ui.separator();
        ui.label("Window:");
        ui.add(egui::Slider::new(&mut window_size, 1..=400).logarithmic(true));
    });
    ctx.view_state.phase_window = window_size;
    ctx.view_state.phase_mode = mode;
    ctx.view_state.phase_display = display;

    let window = ctx.history.window_ending_at(ctx.view_anchor(), window_size);
    if window.is_empty() {
        ui.centered_and_justified(|ui| ui.label("Waiting for CSI…"));
        return;
    }

    // Build per-packet processed phase traces.
    let subcarriers = window
        .iter()
        .filter_map(|s| s.csi.as_ref().map(|c| c.subcarriers()))
        .max()
        .unwrap_or(0);
    if subcarriers == 0 {
        ui.centered_and_justified(|ui| ui.label("No decoded packets in window."));
        return;
    }

    let traces: Vec<Vec<f32>> = window
        .iter()
        .filter_map(|s| s.csi.as_ref())
        .filter(|c| c.subcarriers() == subcarriers)
        .map(|csi| {
            let raw: Vec<f32> = (0..subcarriers).map(|k| csi.phase(k)).collect();
            match mode {
                PhaseMode::Raw => raw,
                PhaseMode::Unwrapped => unwrap_phase(&raw),
                PhaseMode::Detrended => {
                    let u = unwrap_phase(&raw);
                    detrend(&u)
                }
            }
        })
        .collect();

    if traces.is_empty() {
        ui.centered_and_justified(|ui| ui.label("Subcarrier counts varied — try a smaller window."));
        return;
    }

    let (x_lo, x_hi) = subcarrier_x_bounds(subcarriers);

    if matches!(display, PhaseDisplay::Heatmap) {
        render_heatmap(ui, &traces, subcarriers, mode, x_lo, x_hi);
        return;
    }

    let mut plot = Plot::new("phase_plot")
        .legend(Legend::default())
        .x_axis_label("Subcarrier (DC = 0, − below, + above)")
        .y_axis_label(match mode {
            PhaseMode::Raw => "∠H(k) (rad, wrapped)",
            PhaseMode::Unwrapped => "∠H(k) (rad, unwrapped)",
            PhaseMode::Detrended => "∠H(k) − linear fit (rad)",
        })
        .include_x(x_lo)
        .include_x(x_hi)
        .show_axes([true, true])
        .show_grid([true, true])
        .allow_zoom(true)
        .allow_drag(true)
        .allow_boxed_zoom(true);
    if mode == PhaseMode::Raw {
        plot = plot.include_y(-PI as f64).include_y(PI as f64);
    }

    plot.show(ui, |plot_ui| {
        plot_ui.vline(VLine::new("DC", 0.0).color(Color32::from_gray(70)));
        match mode {
            PhaseMode::Raw => {
                plot_ui.hline(HLine::new("", 0.0).color(Color32::from_gray(80)));
                plot_ui.hline(HLine::new("", PI as f64).color(Color32::from_gray(60)));
                plot_ui.hline(HLine::new("", -PI as f64).color(Color32::from_gray(60)));
            }
            _ => {
                plot_ui.hline(HLine::new("", 0.0).color(Color32::from_gray(80)));
            }
        }

        match display {
            PhaseDisplay::AllTraces => {
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
            PhaseDisplay::MeanEnvelope => {
                let (mean, std) = mean_and_std(&traces, subcarriers);
                let mut poly_pts: Vec<[f64; 2]> = (0..subcarriers)
                    .map(|k| [subcarrier_x(k, subcarriers), (mean[k] + std[k]) as f64])
                    .collect();
                poly_pts.extend(
                    (0..subcarriers)
                        .rev()
                        .map(|k| [subcarrier_x(k, subcarriers), (mean[k] - std[k]) as f64]),
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
            PhaseDisplay::Heatmap => {}
        }
    });
}

fn render_heatmap(
    ui: &mut egui::Ui,
    traces: &[Vec<f32>],
    subcarriers: usize,
    mode: PhaseMode,
    x_lo: f64,
    x_hi: f64,
) {
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
    let x_offset = -(subcarriers as f64 / 2.0).floor();

    // Raw and Detrended use a diverging colormap centered on 0 — the
    // sign of the value is meaningful. Unwrapped trends monotonically
    // with subcarrier so a sequential colormap reads better; we map it
    // to [vmin, vmax] -> [0, 1].
    let signed = matches!(mode, PhaseMode::Raw | PhaseMode::Detrended);
    let span_signed = vmin.abs().max(vmax.abs()).max(1e-6);
    let span_seq = (vmax - vmin).max(1e-6);

    Plot::new("phase_heatmap")
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
                    let color = if signed {
                        coolwarm_signed((v / span_signed).clamp(-1.0, 1.0))
                    } else {
                        viridis(((v - vmin) / span_seq).clamp(0.0, 1.0))
                    };
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
        if signed {
            let bound = span_signed;
            ui.label(egui::RichText::new(format!("-{bound:.2}")).weak().small());
            ui.add(swatch_widget(|t| coolwarm_signed(2.0 * t - 1.0)));
            ui.label(egui::RichText::new(format!("+{bound:.2}")).weak().small());
        } else {
            ui.label(egui::RichText::new(format!("{vmin:.2}")).weak().small());
            ui.add(swatch_widget(viridis));
            ui.label(egui::RichText::new(format!("{vmax:.2}")).weak().small());
        }
        ui.label(egui::RichText::new("rad").weak().small());
    });
}

fn unwrap_phase(raw: &[f32]) -> Vec<f32> {
    if raw.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(raw.len());
    out.push(raw[0]);
    for i in 1..raw.len() {
        let mut diff = raw[i] - raw[i - 1];
        while diff > PI {
            diff -= 2.0 * PI;
        }
        while diff <= -PI {
            diff += 2.0 * PI;
        }
        let acc = out[i - 1] + diff;
        out.push(acc);
    }
    out
}

fn detrend(unwrapped: &[f32]) -> Vec<f32> {
    let n = unwrapped.len();
    if n < 2 {
        return unwrapped.to_vec();
    }
    let nf = n as f32;
    let sum_x: f32 = (0..n).map(|i| i as f32).sum();
    let sum_y: f32 = unwrapped.iter().sum();
    let sum_xx: f32 = (0..n).map(|i| (i * i) as f32).sum();
    let sum_xy: f32 = unwrapped
        .iter()
        .enumerate()
        .map(|(i, y)| i as f32 * y)
        .sum();
    let denom = nf * sum_xx - sum_x * sum_x;
    if denom.abs() < 1e-9 {
        return unwrapped.to_vec();
    }
    let slope = (nf * sum_xy - sum_x * sum_y) / denom;
    let intercept = (sum_y - slope * sum_x) / nf;
    unwrapped
        .iter()
        .enumerate()
        .map(|(i, y)| y - (slope * i as f32 + intercept))
        .collect()
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
