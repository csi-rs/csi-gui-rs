use super::{ViewCtx, title_bar};
use egui::Color32;
use egui_plot::{Legend, Plot, PlotPoints, Points};

/// I/Q scatter density — every subcarrier of every packet in the window is
/// a real point. No binning, no fixed-resolution grid. Density is conveyed
/// by alpha overlap (dense regions appear bright). The data-layer grid in
/// `NetworkStats::distribution_grid` is bypassed because its 24×24 / ±128
/// quantization throws away too much detail to be useful for analysis.
pub fn render(ui: &mut egui::Ui, ctx: &mut ViewCtx<'_>) {
    title_bar(ui, "I/Q scatter", ctx);

    let window_packets = ctx
        .view_state
        .iq_window
        .clamp(1, ctx.history.capacity().max(1));

    let window = ctx
        .history
        .window_ending_at(ctx.view_anchor(), window_packets);
    if window.is_empty() {
        ui.centered_and_justified(|ui| ui.label("Waiting for CSI…"));
        return;
    }

    // Controls row.
    let mut requested = window_packets;
    ui.horizontal(|ui| {
        ui.label("Packets in window:");
        ui.add(egui::Slider::new(&mut requested, 1..=400).logarithmic(true));
        ui.separator();
        let total_subcarriers: usize = window
            .iter()
            .filter_map(|s| s.csi.as_ref().map(|c| c.subcarriers()))
            .sum();
        ui.label(
            egui::RichText::new(format!(
                "{total_subcarriers} points · {} packets",
                window.len()
            ))
            .weak()
            .small(),
        );
    });
    if requested != window_packets {
        ctx.view_state.iq_window = requested;
    }

    // Walk the window twice: once to find the data extent (so the plot
    // auto-fits the actual cloud, not the legacy ±128 grid), once to emit
    // the points. Cheap because windows are bounded.
    let mut min_i = i32::MAX;
    let mut max_i = i32::MIN;
    let mut min_q = i32::MAX;
    let mut max_q = i32::MIN;
    let mut total_pts = 0usize;
    for s in &window {
        let Some(csi) = s.csi.as_ref() else { continue };
        for k in 0..csi.subcarriers() {
            let (i, q) = csi.iq(k);
            min_i = min_i.min(i);
            max_i = max_i.max(i);
            min_q = min_q.min(q);
            max_q = max_q.max(q);
            total_pts += 1;
        }
    }
    if total_pts == 0 {
        ui.centered_and_justified(|ui| ui.label("No decoded I/Q samples in window."));
        return;
    }
    let span_i = (max_i - min_i).max(1);
    let span_q = (max_q - min_q).max(1);
    let pad_i = (span_i / 20).max(1) as f64;
    let pad_q = (span_q / 20).max(1) as f64;

    // Color/alpha intent:
    //   - Many overlapping points → bright/saturated (density emerges naturally).
    //   - Newest packets slightly brighter than oldest, so motion is visible.
    let n_pkts = window.len().max(1);

    Plot::new("iq_scatter")
        .height(super::plot_height(ctx.plot_budget))
        .data_aspect(1.0)
        .x_axis_label("I")
        .y_axis_label("Q")
        .include_x(min_i as f64 - pad_i)
        .include_x(max_i as f64 + pad_i)
        .include_y(min_q as f64 - pad_q)
        .include_y(max_q as f64 + pad_q)
        .show_axes([true, true])
        .show_grid([true, true])
        .legend(Legend::default())
        .allow_zoom(true)
        .allow_drag(true)
        .allow_boxed_zoom(true)
        .show(ui, |plot_ui| {
            // We bucket points into a few age groups so we can submit a few
            // Points layers (egui_plot's per-batch alpha blends them, giving
            // the density effect). Doing it per-point would be a draw call
            // tsunami; doing it as a single batch loses the age coloring.
            const AGE_BUCKETS: usize = 5;
            let mut buckets: Vec<Vec<[f64; 2]>> = (0..AGE_BUCKETS).map(|_| Vec::new()).collect();
            for (idx, stats) in window.iter().enumerate() {
                let Some(csi) = stats.csi.as_ref() else {
                    continue;
                };
                let age = n_pkts - 1 - idx;
                let bucket = (age * AGE_BUCKETS / n_pkts).min(AGE_BUCKETS - 1);
                let bucket_vec = &mut buckets[bucket];
                for k in 0..csi.subcarriers() {
                    let (i, q) = csi.iq(k);
                    bucket_vec.push([i as f64, q as f64]);
                }
            }
            // Older buckets first so newer points draw on top.
            for (b, pts) in buckets.into_iter().enumerate().rev() {
                if pts.is_empty() {
                    continue;
                }
                let color = bucket_color(b, AGE_BUCKETS);
                let name = if b == 0 { "newest" } else { "" };
                plot_ui.points(
                    Points::new(name, PlotPoints::from(pts))
                        .color(color)
                        .radius(1.6_f32),
                );
            }
        });
}

/// Newest bucket gets the strongest, most opaque color; older ones fade.
fn bucket_color(bucket: usize, total: usize) -> Color32 {
    let t = bucket as f32 / (total - 1).max(1) as f32;
    let alpha = (130.0 * (1.0 - t) + 25.0).round() as u8;
    let r = (90.0 + 130.0 * (1.0 - t)) as u8;
    let g = (180.0 + 50.0 * (1.0 - t)) as u8;
    let b = (240.0 + 15.0 * (1.0 - t)).min(255.0) as u8;
    Color32::from_rgba_premultiplied(
        (r as u16 * alpha as u16 / 255) as u8,
        (g as u16 * alpha as u16 / 255) as u8,
        (b as u16 * alpha as u16 / 255) as u8,
        alpha,
    )
}
