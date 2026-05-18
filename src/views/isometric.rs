use super::{title_bar, ViewCtx};
use crate::data::dsp::compute_cir;
use egui::Color32;
use egui_plot::{Legend, Line, Plot, PlotPoints};

const DEPTH_FRAMES: usize = 30;

/// Channel Impulse Response (CIR) — flat 2D plot. Each visible packet is one
/// line: power per delay tap. Most recent packet is highlighted; older ones
/// fade toward the background.
pub fn render(ui: &mut egui::Ui, ctx: &mut ViewCtx<'_>) {
    title_bar(ui, "CIR — Power vs Delay", ctx);

    let window = ctx
        .history
        .window_ending_at(ctx.view_anchor(), DEPTH_FRAMES);
    if window.is_empty() {
        ui.centered_and_justified(|ui| ui.label("Waiting for CSI…"));
        return;
    }

    let cirs: Vec<Vec<f32>> = window
        .iter()
        .filter_map(|s| s.csi.as_ref())
        .map(compute_cir)
        .collect();

    if cirs.is_empty() {
        ui.centered_and_justified(|ui| ui.label("No decoded packets in window."));
        return;
    }

    let n = cirs.len();
    Plot::new("cir_plot")
        .legend(Legend::default())
        .x_axis_label("Delay tap")
        .y_axis_label("|h|")
        .show_axes([true, true])
        .show_grid([true, true])
        .allow_zoom(true)
        .allow_drag(true)
        .show(ui, |plot_ui| {
            for (idx, cir) in cirs.iter().enumerate() {
                let age = n - 1 - idx; // 0 = newest
                let pts: PlotPoints = cir
                    .iter()
                    .enumerate()
                    .map(|(k, y)| [k as f64, *y as f64])
                    .collect();
                let color = age_color(age, n);
                let label = if age == 0 { "latest" } else { "" };
                plot_ui.line(Line::new(pts).color(color).name(label));
            }
        });
}

fn age_color(age: usize, total: usize) -> Color32 {
    if age == 0 {
        return Color32::from_rgb(110, 200, 255);
    }
    let t = (age as f32 / total.max(1) as f32).clamp(0.0, 1.0);
    let r = (180.0 * (1.0 - t) + 70.0 * t) as u8;
    let g = (200.0 * (1.0 - t) + 80.0 * t) as u8;
    let b = (255.0 * (1.0 - t) + 90.0 * t) as u8;
    let alpha = ((1.0 - t) * 180.0 + 30.0).clamp(30.0, 220.0) as u8;
    Color32::from_rgba_premultiplied(
        (r as u16 * alpha as u16 / 255) as u8,
        (g as u16 * alpha as u16 / 255) as u8,
        (b as u16 * alpha as u16 / 255) as u8,
        alpha,
    )
}
