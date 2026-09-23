//! Colormaps shared by every heatmap pane (Spectrogram, Magnitude
//! heatmap, Phase heatmap). Keeps the look consistent across views and
//! avoids duplicating the control-point tables.

use egui::Color32;

/// Perceptually-uniform sequential colormap (matplotlib's viridis). Use
/// for non-negative quantities like `|H(k)|` or `|Δφ|`.
pub fn viridis(t: f32) -> Color32 {
    const STOPS: &[(f32, [u8; 3])] = &[
        (0.00, [68, 1, 84]),
        (0.20, [72, 36, 117]),
        (0.40, [64, 67, 135]),
        (0.60, [52, 94, 141]),
        (0.80, [41, 121, 142]),
        (1.00, [253, 231, 37]),
    ];
    interpolate(STOPS, t)
}

/// Diverging colormap centered on 0 (cool-warm). `t ∈ [-1, 1]`; out-of-
/// range values clamp. Use for signed quantities like raw / detrended
/// phase where positive vs negative deviations carry independent
/// meaning.
pub fn coolwarm_signed(t: f32) -> Color32 {
    // 7 stops sampled from matplotlib's coolwarm, normalized to [-1, 1].
    const STOPS: &[(f32, [u8; 3])] = &[
        (-1.00, [59, 76, 192]),
        (-0.66, [98, 129, 226]),
        (-0.33, [156, 178, 247]),
        (0.00, [221, 221, 221]),
        (0.33, [243, 174, 142]),
        (0.66, [222, 110, 78]),
        (1.00, [180, 4, 38]),
    ];
    interpolate(STOPS, t)
}

fn interpolate(stops: &[(f32, [u8; 3])], t: f32) -> Color32 {
    let lo = stops.first().unwrap().0;
    let hi = stops.last().unwrap().0;
    let t = t.clamp(lo, hi);
    for w in stops.windows(2) {
        let (t0, c0) = w[0];
        let (t1, c1) = w[1];
        if t <= t1 {
            let f = ((t - t0) / (t1 - t0)).clamp(0.0, 1.0);
            let r = (c0[0] as f32 * (1.0 - f) + c1[0] as f32 * f) as u8;
            let g = (c0[1] as f32 * (1.0 - f) + c1[1] as f32 * f) as u8;
            let b = (c0[2] as f32 * (1.0 - f) + c1[2] as f32 * f) as u8;
            return Color32::from_rgb(r, g, b);
        }
    }
    let last = stops.last().unwrap().1;
    Color32::from_rgb(last[0], last[1], last[2])
}

/// Compact horizontal colormap swatch used as a plot-legend strip.
/// `map(t)` is called with `t ∈ [0, 1]` to colour each strip cell.
pub fn swatch_widget<F>(map: F) -> impl egui::Widget
where
    F: Fn(f32) -> Color32,
{
    move |ui: &mut egui::Ui| {
        let desired = egui::vec2(120.0, 12.0);
        let (rect, resp) = ui.allocate_exact_size(desired, egui::Sense::hover());
        let painter = ui.painter();
        let steps = 60;
        for i in 0..steps {
            let t0 = i as f32 / steps as f32;
            let t1 = (i + 1) as f32 / steps as f32;
            let x0 = rect.left() + t0 * rect.width();
            let x1 = rect.left() + t1 * rect.width();
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(x0, rect.top()),
                    egui::pos2(x1, rect.bottom()),
                ),
                0.0,
                map((t0 + t1) * 0.5),
            );
        }
        painter.rect_stroke(
            rect,
            1.0,
            egui::Stroke::new(0.5_f32, Color32::from_gray(120)),
            egui::StrokeKind::Middle,
        );
        resp
    }
}
