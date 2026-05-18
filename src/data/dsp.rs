use super::CsiData;
use std::f32::consts::TAU;

/// Channel Impulse Response via per-packet O(N²) IDFT (N up to 64).
/// Moved here from `views/isometric.rs` so the Rerun sink can log the same
/// values the in-process CIR plot shows, without duplicating the math.
pub fn compute_cir(csi: &CsiData) -> Vec<f32> {
    let n = csi.subcarriers().min(64);
    if n == 0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(n);
    for k in 0..n {
        let mut re = 0.0f32;
        let mut im = 0.0f32;
        for s in 0..n {
            let (i, q) = csi.iq(s);
            let theta = TAU * (k as f32) * (s as f32) / (n as f32);
            let c = theta.cos();
            let st = theta.sin();
            re += i as f32 * c - q as f32 * st;
            im += i as f32 * st + q as f32 * c;
        }
        out.push((re * re + im * im).sqrt() / n as f32);
    }
    out
}

/// Phase → RGBA per EGUI_PORT_SPEC §11: `(c, 100, 255-c, 255)` where
/// `c = (phase + π) / (2π) · 255`. Used by Rerun's complex-scatter coloring.
pub fn phase_to_rgba(phase: f32) -> [u8; 4] {
    let normalized = ((phase + std::f32::consts::PI) / std::f32::consts::TAU).clamp(0.0, 1.0);
    let c = (normalized * 255.0).round() as u8;
    [c, 100, 255 - c, 255]
}
