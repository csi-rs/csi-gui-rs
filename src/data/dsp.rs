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

/// Remove multipath taps beyond `keep_bins` in the delay domain and
/// reconstruct subcarrier I/Q via inverse DFT.
pub fn filter_cir_delay(csi: &mut CsiData, keep_bins: usize) {
    let n = csi.subcarriers().min(64);
    if n == 0 {
        return;
    }
    let keep = keep_bins.clamp(1, n);

    let mut h_re = vec![0f32; n];
    let mut h_im = vec![0f32; n];
    for s in 0..n {
        let (i, q) = csi.iq(s);
        h_re[s] = i as f32;
        h_im[s] = q as f32;
    }

    let mut cir_re = vec![0f32; n];
    let mut cir_im = vec![0f32; n];
    for k in 0..n {
        let mut re = 0.0f32;
        let mut im = 0.0f32;
        for s in 0..n {
            let theta = TAU * (k as f32) * (s as f32) / (n as f32);
            let c = theta.cos();
            let st = theta.sin();
            re += h_re[s] * c - h_im[s] * st;
            im += h_re[s] * st + h_im[s] * c;
        }
        cir_re[k] = re / n as f32;
        cir_im[k] = im / n as f32;
    }
    for k in keep..n {
        cir_re[k] = 0.0;
        cir_im[k] = 0.0;
    }

    for s in 0..n {
        let mut re = 0.0f32;
        let mut im = 0.0f32;
        for k in 0..n {
            let theta = TAU * (k as f32) * (s as f32) / (n as f32);
            let c = theta.cos();
            let st = theta.sin();
            re += cir_re[k] * c + cir_im[k] * st;
            im += -cir_re[k] * st + cir_im[k] * c;
        }
        let base = 2 * s;
        if base + 1 < csi.csi_raw_data.len() {
            csi.csi_raw_data[base] = re.round() as i32;
            csi.csi_raw_data[base + 1] = im.round() as i32;
        }
    }
}

/// Phase → RGBA per EGUI_PORT_SPEC §11: `(c, 100, 255-c, 255)` where
/// `c = (phase + π) / (2π) · 255`. Used by Rerun's complex-scatter coloring.
pub fn phase_to_rgba(phase: f32) -> [u8; 4] {
    let normalized = ((phase + std::f32::consts::PI) / std::f32::consts::TAU).clamp(0.0, 1.0);
    let c = (normalized * 255.0).round() as u8;
    [c, 100, 255 - c, 255]
}
