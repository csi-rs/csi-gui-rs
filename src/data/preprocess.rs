//! Per-window CSI preprocessing pipeline.
//!
//! Applied **at capture time** by `App::pump_reader` against a rolling
//! buffer of recently-received raw packets — the preprocessed output
//! lands in [`crate::data::History`], so any filter the user turns on
//! is baked into newly captured packets but never retroactively rewrites
//! packets already in the buffer.

use super::dsp::filter_cir_delay;
use super::CsiData;
use savgol_rs::{savgol_filter, SavGolInput};
use serde::{Deserialize, Serialize};
use std::f32::consts::FRAC_1_SQRT_2;

/// Top-level preprocessing configuration.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PreprocessConfig {
    #[serde(default)]
    pub interpolation: Interpolation,
    #[serde(default)]
    pub denoising: Denoising,
    #[serde(default)]
    pub detrending: Detrending,
    #[serde(default)]
    pub feature_scaling: FeatureScaling,
    #[serde(default)]
    pub params: PreprocessParams,
}

/// Parameters shared across stages.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PreprocessParams {
    #[serde(default = "default_filter_window")]
    pub filter_window: usize,
    #[serde(default = "default_sg_poly_order")]
    pub sg_poly_order: u32,
    #[serde(default = "default_hampel_sigma")]
    pub hampel_sigma: f32,
    #[serde(default = "default_butterworth_order")]
    pub butterworth_order: u32,
    #[serde(default = "default_butterworth_cutoff")]
    pub butterworth_cutoff: f32,
    #[serde(default = "default_dwt_levels")]
    pub dwt_levels: u32,
    #[serde(default = "default_dwt_threshold")]
    pub dwt_threshold: f32,
    #[serde(default = "default_dwt_soft_threshold")]
    pub dwt_soft_threshold: bool,
    #[serde(default = "default_fft_delay_keep_bins")]
    pub fft_delay_keep_bins: usize,
    pub moving_average_window: usize,
    pub quantization_levels: u32,
    pub scaling_per_packet: bool,
}

fn default_filter_window() -> usize {
    11
}
fn default_sg_poly_order() -> u32 {
    3
}
fn default_hampel_sigma() -> f32 {
    3.0
}
fn default_butterworth_order() -> u32 {
    2
}
fn default_butterworth_cutoff() -> f32 {
    0.1
}
fn default_dwt_levels() -> u32 {
    2
}
fn default_dwt_threshold() -> f32 {
    0.5
}
fn default_dwt_soft_threshold() -> bool {
    true
}
fn default_fft_delay_keep_bins() -> usize {
    16
}

impl Default for PreprocessParams {
    fn default() -> Self {
        Self {
            filter_window: default_filter_window(),
            sg_poly_order: default_sg_poly_order(),
            hampel_sigma: default_hampel_sigma(),
            butterworth_order: default_butterworth_order(),
            butterworth_cutoff: default_butterworth_cutoff(),
            dwt_levels: default_dwt_levels(),
            dwt_threshold: default_dwt_threshold(),
            dwt_soft_threshold: default_dwt_soft_threshold(),
            fft_delay_keep_bins: default_fft_delay_keep_bins(),
            moving_average_window: 16,
            quantization_levels: 16,
            scaling_per_packet: true,
        }
    }
}

/// Denoising / filter stage.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Denoising {
    #[default]
    Off,
    RollingMean,
    RollingMedian,
    SavitzkyGolay,
    Hampel,
    ButterworthLowPass,
    WaveletDwt,
    FftDelayFilter,
}

impl Denoising {
    pub const ALL: [Denoising; 8] = [
        Denoising::Off,
        Denoising::RollingMean,
        Denoising::RollingMedian,
        Denoising::SavitzkyGolay,
        Denoising::Hampel,
        Denoising::ButterworthLowPass,
        Denoising::WaveletDwt,
        Denoising::FftDelayFilter,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::RollingMean => "Rolling mean",
            Self::RollingMedian => "Rolling median",
            Self::SavitzkyGolay => "Savitzky–Golay",
            Self::Hampel => "Hampel (MAD)",
            Self::ButterworthLowPass => "Butterworth low-pass",
            Self::WaveletDwt => "Wavelet DWT (Haar)",
            Self::FftDelayFilter => "FFT delay filter",
        }
    }

    pub fn tooltip(self) -> &'static str {
        match self {
            Self::Off => "No denoising.",
            Self::RollingMean => "Smooth with a rolling average over the window.",
            Self::RollingMedian => "Robust smoother; handles anomalous spikes well.",
            Self::SavitzkyGolay => "Polynomial fit that preserves peaks and valleys.",
            Self::Hampel => "Replace outliers (e.g. quantization glitches) via MAD.",
            Self::ButterworthLowPass => "Low-pass IIR; removes slow drift and high-frequency noise.",
            Self::WaveletDwt => "Haar wavelet decomposition with coefficient thresholding.",
            Self::FftDelayFilter => "Zero long multipath taps in the CIR (per packet).",
        }
    }
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Detrending {
    #[default]
    Off,
    LeastSquaresBaseline,
    MovingAverageTrend,
}

impl Detrending {
    pub const ALL: [Detrending; 3] = [
        Detrending::Off,
        Detrending::LeastSquaresBaseline,
        Detrending::MovingAverageTrend,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::LeastSquaresBaseline => "Least-squares baseline",
            Self::MovingAverageTrend => "Moving-average trend",
        }
    }

    pub fn tooltip(self) -> &'static str {
        match self {
            Self::Off => "No detrending.",
            Self::LeastSquaresBaseline => "Subtract a per-subcarrier linear fit.",
            Self::MovingAverageTrend => "Subtract a rolling baseline for online drift removal.",
        }
    }
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Interpolation {
    #[default]
    Off,
    Linear,
    NearestNeighbor,
}

impl Interpolation {
    pub const ALL: [Interpolation; 3] = [
        Interpolation::Off,
        Interpolation::Linear,
        Interpolation::NearestNeighbor,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Linear => "Linear",
            Self::NearestNeighbor => "Nearest neighbour",
        }
    }

    pub fn tooltip(self) -> &'static str {
        match self {
            Self::Off => "No interpolation.",
            Self::Linear => "Fill missing subcarriers along a straight line between neighbours.",
            Self::NearestNeighbor => "Copy the closest valid subcarrier value.",
        }
    }
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FeatureScaling {
    #[default]
    Off,
    MaxMinNormalization,
    ZScoreStandardization,
    Quantization,
}

impl FeatureScaling {
    pub const ALL: [FeatureScaling; 4] = [
        FeatureScaling::Off,
        FeatureScaling::MaxMinNormalization,
        FeatureScaling::ZScoreStandardization,
        FeatureScaling::Quantization,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::MaxMinNormalization => "Max–Min",
            Self::ZScoreStandardization => "Z-score",
            Self::Quantization => "Quantize",
        }
    }

    pub fn tooltip(self) -> &'static str {
        match self {
            Self::Off => "No scaling.",
            Self::MaxMinNormalization => "Compress amplitudes into [0, 1] using window min/max.",
            Self::ZScoreStandardization => "Standardise to mean 0, std 1 (shifted non-negative).",
            Self::Quantization => "Bucket amplitudes to reduce precision and memory.",
        }
    }
}

impl PreprocessConfig {
    pub fn is_active(&self) -> bool {
        !matches!(self.interpolation, Interpolation::Off)
            || !matches!(self.denoising, Denoising::Off)
            || !matches!(self.detrending, Detrending::Off)
            || !matches!(self.feature_scaling, FeatureScaling::Off)
    }

    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if !matches!(self.interpolation, Interpolation::Off) {
            parts.push(self.interpolation.label().to_string());
        }
        if !matches!(self.denoising, Denoising::Off) {
            parts.push(self.denoising.label().to_string());
        }
        if !matches!(self.detrending, Detrending::Off) {
            parts.push(self.detrending.label().to_string());
        }
        if !matches!(self.feature_scaling, FeatureScaling::Off) {
            parts.push(self.feature_scaling.label().to_string());
        }
        if parts.is_empty() {
            "No active stages".to_string()
        } else {
            parts.join(" → ")
        }
    }

    pub fn apply(&self, packets: &[CsiData]) -> Vec<CsiData> {
        let mut out: Vec<CsiData> = packets.to_vec();
        if !self.is_active() {
            return out;
        }
        apply_interpolation(&mut out, self.interpolation);
        apply_denoising(&mut out, self.denoising, &self.params);
        apply_detrending(&mut out, self.detrending, &self.params);
        apply_feature_scaling(&mut out, self.feature_scaling, &self.params);
        out
    }
}

fn apply_interpolation(packets: &mut [CsiData], mode: Interpolation) {
    if matches!(mode, Interpolation::Off) {
        return;
    }
    for p in packets.iter_mut() {
        let n = p.subcarriers();
        if n == 0 {
            continue;
        }
        let amps: Vec<f32> = (0..n).map(|k| p.amplitude(k)).collect();
        let anchors: Vec<usize> = (0..n).filter(|&k| amps[k] > 1e-6).collect();
        if anchors.is_empty() || anchors.len() == n {
            continue;
        }
        let mut new_amps = amps.clone();
        for (k, slot) in new_amps.iter_mut().enumerate() {
            if *slot > 1e-6 {
                continue;
            }
            let lo = anchors.iter().rev().find(|&&a| a < k).copied();
            let hi = anchors.iter().find(|&&a| a > k).copied();
            *slot = match (lo, hi, mode) {
                (Some(l), Some(h), Interpolation::Linear) => {
                    let t = (k - l) as f32 / (h - l) as f32;
                    amps[l] * (1.0 - t) + amps[h] * t
                }
                (Some(l), Some(h), Interpolation::NearestNeighbor) => {
                    if k - l <= h - k {
                        amps[l]
                    } else {
                        amps[h]
                    }
                }
                (Some(l), None, _) => amps[l],
                (None, Some(h), _) => amps[h],
                _ => 0.0,
            };
        }
        rescale_packet_amplitudes(p, &new_amps);
    }
}

fn apply_denoising(packets: &mut [CsiData], mode: Denoising, params: &PreprocessParams) {
    if matches!(mode, Denoising::Off) {
        return;
    }
    if matches!(mode, Denoising::FftDelayFilter) {
        let keep = params.fft_delay_keep_bins.max(1);
        for p in packets.iter_mut() {
            filter_cir_delay(p, keep);
        }
        return;
    }
    if packets.len() < 2 {
        return;
    }
    let Some((n, np, mut amps)) = materialize_amplitude_matrix(packets) else {
        return;
    };
    let w = odd_window(params.filter_window, np);
    for k in 0..n {
        let series: Vec<f32> = (0..np).map(|t| amps[k * np + t]).collect();
        let filtered = match mode {
            Denoising::RollingMean => rolling_mean(&series, w),
            Denoising::RollingMedian => rolling_median(&series, w),
            Denoising::SavitzkyGolay => savitzky_golay(&series, w, params.sg_poly_order),
            Denoising::Hampel => hampel_filter(&series, w, params.hampel_sigma),
            Denoising::ButterworthLowPass => {
                butterworth_lowpass(&series, params.butterworth_order, params.butterworth_cutoff)
            }
            Denoising::WaveletDwt => haar_dwt_denoise(
                &series,
                params.dwt_levels as usize,
                params.dwt_threshold,
                params.dwt_soft_threshold,
            ),
            Denoising::Off | Denoising::FftDelayFilter => unreachable!(),
        };
        for (t, &v) in filtered.iter().enumerate().take(np) {
            amps[k * np + t] = v.max(0.0);
        }
    }
    write_amplitude_matrix(packets, &amps, n, np);
}

fn odd_window(requested: usize, max_len: usize) -> usize {
    let mut w = requested.max(3).min(max_len);
    if w % 2 == 0 {
        w -= 1;
    }
    w.max(3)
}

fn materialize_amplitude_matrix(packets: &[CsiData]) -> Option<(usize, usize, Vec<f32>)> {
    let n = packets.first()?.subcarriers();
    if n == 0 {
        return None;
    }
    let np = packets.len();
    let mut amps = vec![0f32; n * np];
    for (t, p) in packets.iter().enumerate() {
        let lim = n.min(p.subcarriers());
        for k in 0..lim {
            amps[k * np + t] = p.amplitude(k);
        }
    }
    Some((n, np, amps))
}

fn write_amplitude_matrix(packets: &mut [CsiData], amps: &[f32], n: usize, np: usize) {
    for (t, p) in packets.iter_mut().enumerate() {
        let lim = n.min(p.subcarriers());
        let row: Vec<f32> = (0..lim).map(|k| amps[k * np + t]).collect();
        rescale_packet_amplitudes(p, &row);
    }
}

fn rolling_mean(xs: &[f32], w: usize) -> Vec<f32> {
    let n = xs.len();
    let half = w / 2;
    let mut out = vec![0f32; n];
    for (t, slot) in out.iter_mut().enumerate().take(n) {
        let lo = t.saturating_sub(half);
        let hi = (t + half + 1).min(n);
        let sum: f32 = xs[lo..hi].iter().sum();
        *slot = sum / (hi - lo) as f32;
    }
    out
}

fn rolling_median(xs: &[f32], w: usize) -> Vec<f32> {
    let n = xs.len();
    let half = w / 2;
    let mut out = vec![0f32; n];
    let mut buf = Vec::with_capacity(w);
    for (t, slot) in out.iter_mut().enumerate().take(n) {
        let lo = t.saturating_sub(half);
        let hi = (t + half + 1).min(n);
        buf.clear();
        buf.extend_from_slice(&xs[lo..hi]);
        buf.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mid = buf.len() / 2;
        *slot = if buf.len() % 2 == 0 {
            (buf[mid - 1] + buf[mid]) / 2.0
        } else {
            buf[mid]
        };
    }
    out
}

fn savitzky_golay(xs: &[f32], w: usize, poly_order: u32) -> Vec<f32> {
    if xs.len() < w {
        return xs.to_vec();
    }
    let order = poly_order.max(1).min(w as u32 - 1) as usize;
    let data64: Vec<f64> = xs.iter().map(|&x| x as f64).collect();
    let input = SavGolInput {
        data: &data64,
        window_length: w,
        poly_order: order,
        derivative: 0,
    };
    savgol_filter(&input)
        .map(|v| v.into_iter().map(|x| x as f32).collect())
        .unwrap_or_else(|_| xs.to_vec())
}

fn hampel_filter(xs: &[f32], w: usize, sigma: f32) -> Vec<f32> {
    let n = xs.len();
    let half = w / 2;
    let mut out = xs.to_vec();
    let thresh = sigma.max(0.1);
    for t in 0..n {
        let lo = t.saturating_sub(half);
        let hi = (t + half + 1).min(n);
        let window = &xs[lo..hi];
        let mut sorted: Vec<f32> = window.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let med = sorted[sorted.len() / 2];
        let mut dev: Vec<f32> = window.iter().map(|x| (x - med).abs()).collect();
        dev.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mad = dev[dev.len() / 2].max(1e-6);
        if (xs[t] - med).abs() > thresh * mad {
            out[t] = med;
        }
    }
    out
}

fn butterworth_lowpass(xs: &[f32], order: u32, cutoff: f32) -> Vec<f32> {
    let mut out = biquad_lowpass(xs, cutoff);
    if order >= 4 {
        out = biquad_lowpass(&out, cutoff);
    }
    out
}

fn biquad_lowpass(xs: &[f32], cutoff: f32) -> Vec<f32> {
    if xs.is_empty() {
        return Vec::new();
    }
    let f = cutoff.clamp(0.001, 0.49);
    let w0 = std::f32::consts::PI * f;
    let cos_w0 = w0.cos();
    let sin_w0 = w0.sin();
    let alpha = sin_w0 / (2.0 * FRAC_1_SQRT_2);
    let b0 = (1.0 - cos_w0) / 2.0;
    let b1 = 1.0 - cos_w0;
    let b2 = (1.0 - cos_w0) / 2.0;
    let a0 = 1.0 + alpha;
    let a1 = -2.0 * cos_w0;
    let a2 = 1.0 - alpha;
    let b0 = b0 / a0;
    let b1 = b1 / a0;
    let b2 = b2 / a0;
    let a1 = a1 / a0;
    let a2 = a2 / a0;

    let mut out = vec![0f32; xs.len()];
    let mut z1 = 0.0f32;
    let mut z2 = 0.0f32;
    for (i, &x) in xs.iter().enumerate() {
        let y = b0 * x + z1;
        z1 = b1 * x - a1 * y + z2;
        z2 = b2 * x - a2 * y;
        out[i] = y;
    }
    out
}

fn soft_threshold(x: f32, t: f32) -> f32 {
    if x.abs() <= t {
        0.0
    } else if x > 0.0 {
        x - t
    } else {
        x + t
    }
}

fn haar_dwt_denoise(signal: &[f32], levels: usize, threshold: f32, soft: bool) -> Vec<f32> {
    let n = signal.len();
    if n < 2 {
        return signal.to_vec();
    }
    let mut len = 1usize;
    while len < n {
        len *= 2;
    }
    let mut data = signal.to_vec();
    data.resize(len, *signal.last().unwrap_or(&0.0));
    let max_levels = len.trailing_zeros() as usize;
    let levels = levels.clamp(1, max_levels.max(1));
    let mut details: Vec<Vec<f32>> = Vec::with_capacity(levels);

    for _ in 0..levels {
        let half = data.len() / 2;
        if half == 0 {
            break;
        }
        let mut avg = vec![0f32; half];
        let mut diff = vec![0f32; half];
        for i in 0..half {
            avg[i] = (data[2 * i] + data[2 * i + 1]) * FRAC_1_SQRT_2;
            diff[i] = (data[2 * i] - data[2 * i + 1]) * FRAC_1_SQRT_2;
        }
        for d in &mut diff {
            *d = if soft {
                soft_threshold(*d, threshold)
            } else if d.abs() < threshold {
                0.0
            } else {
                *d
            };
        }
        details.push(diff);
        data = avg;
    }

    for level in (0..details.len()).rev() {
        let diff = &details[level];
        let half = diff.len();
        let mut merged = vec![0f32; half * 2];
        for i in 0..half {
            merged[2 * i] = (data[i] + diff[i]) * FRAC_1_SQRT_2;
            merged[2 * i + 1] = (data[i] - diff[i]) * FRAC_1_SQRT_2;
        }
        data = merged;
    }
    data.truncate(n);
    data
}

fn apply_detrending(packets: &mut [CsiData], mode: Detrending, params: &PreprocessParams) {
    if matches!(mode, Detrending::Off) || packets.len() < 2 {
        return;
    }
    let Some((n, np, mut amps)) = materialize_amplitude_matrix(packets) else {
        return;
    };

    match mode {
        Detrending::LeastSquaresBaseline => {
            let nf = np as f32;
            let sum_x: f32 = (0..np).map(|t| t as f32).sum();
            let sum_xx: f32 = (0..np).map(|t| (t * t) as f32).sum();
            let denom = nf * sum_xx - sum_x * sum_x;
            if denom.abs() < 1e-9 {
                return;
            }
            for k in 0..n {
                let mut sum_y = 0f32;
                let mut sum_xy = 0f32;
                for t in 0..np {
                    let y = amps[k * np + t];
                    sum_y += y;
                    sum_xy += t as f32 * y;
                }
                let slope = (nf * sum_xy - sum_x * sum_y) / denom;
                let intercept = (sum_y - slope * sum_x) / nf;
                for t in 0..np {
                    let trend = slope * t as f32 + intercept;
                    amps[k * np + t] = (amps[k * np + t] - trend).max(0.0);
                }
            }
        }
        Detrending::MovingAverageTrend => {
            let w = params.moving_average_window.max(1).min(np);
            for k in 0..n {
                for t in 0..np {
                    let lo = t.saturating_sub(w / 2);
                    let hi = (t + w / 2 + 1).min(np);
                    let mean: f32 = (lo..hi).map(|u| amps[k * np + u]).sum::<f32>()
                        / (hi - lo) as f32;
                    amps[k * np + t] = (amps[k * np + t] - mean).max(0.0);
                }
            }
        }
        Detrending::Off => unreachable!(),
    }

    write_amplitude_matrix(packets, &amps, n, np);
}

fn mean_and_std(xs: &[f32]) -> (f32, f32) {
    if xs.is_empty() {
        return (0.0, 0.0);
    }
    let n = xs.len() as f32;
    let m = xs.iter().sum::<f32>() / n;
    let var = xs.iter().map(|x| (x - m).powi(2)).sum::<f32>() / n;
    (m, var.sqrt())
}

fn apply_feature_scaling(
    packets: &mut [CsiData],
    mode: FeatureScaling,
    params: &PreprocessParams,
) {
    if matches!(mode, FeatureScaling::Off) || packets.is_empty() {
        return;
    }
    if params.scaling_per_packet {
        for p in packets.iter_mut() {
            let n = p.subcarriers();
            if n == 0 {
                continue;
            }
            let amps: Vec<f32> = (0..n).map(|k| p.amplitude(k)).collect();
            let new_amps = scale_vector(&amps, mode, params);
            rescale_packet_amplitudes(p, &new_amps);
        }
    } else {
        let n = packets[0].subcarriers();
        let np = packets.len();
        if n == 0 {
            return;
        }
        for k in 0..n {
            let series: Vec<f32> = packets
                .iter()
                .map(|p| {
                    if k < p.subcarriers() {
                        p.amplitude(k)
                    } else {
                        0.0
                    }
                })
                .collect();
            let scaled = scale_vector(&series, mode, params);
            for t in 0..np {
                if k < packets[t].subcarriers() {
                    let mut row: Vec<f32> = (0..packets[t].subcarriers())
                        .map(|j| packets[t].amplitude(j))
                        .collect();
                    row[k] = scaled[t];
                    rescale_packet_amplitudes(&mut packets[t], &row);
                }
            }
        }
    }
}

fn scale_vector(xs: &[f32], mode: FeatureScaling, params: &PreprocessParams) -> Vec<f32> {
    match mode {
        FeatureScaling::Off => xs.to_vec(),
        FeatureScaling::MaxMinNormalization => {
            let (lo, hi) = xs
                .iter()
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), &v| {
                    (a.min(v), b.max(v))
                });
            let span = (hi - lo).max(1e-6);
            xs.iter().map(|x| ((x - lo) / span) * 255.0).collect()
        }
        FeatureScaling::ZScoreStandardization => {
            let (m, sd) = mean_and_std(xs);
            if sd < 1e-6 {
                return vec![0.0; xs.len()];
            }
            xs.iter().map(|x| ((x - m) / sd + 4.0) * 16.0).collect()
        }
        FeatureScaling::Quantization => {
            let levels = params.quantization_levels.clamp(1, 255) as f32;
            let (lo, hi) = xs
                .iter()
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), &v| {
                    (a.min(v), b.max(v))
                });
            let span = (hi - lo).max(1e-6);
            xs.iter()
                .map(|x| {
                    let n = ((x - lo) / span * (levels - 1.0)).round();
                    lo + (n / (levels - 1.0)) * span
                })
                .collect()
        }
    }
}

fn rescale_packet_amplitudes(p: &mut CsiData, new_amps: &[f32]) {
    let n = p.subcarriers().min(new_amps.len());
    for (k, &target) in new_amps.iter().enumerate().take(n) {
        let old = p.amplitude(k);
        let base = 2 * k;
        if base + 1 >= p.csi_raw_data.len() {
            break;
        }
        let new_amp = target.max(0.0);
        if old < 1e-6 {
            p.csi_raw_data[base] = new_amp.round() as i32;
            p.csi_raw_data[base + 1] = 0;
        } else {
            let scale = new_amp / old;
            p.csi_raw_data[base] = (p.csi_raw_data[base] as f32 * scale).round() as i32;
            p.csi_raw_data[base + 1] = (p.csi_raw_data[base + 1] as f32 * scale).round() as i32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_packet(amps: &[(i32, i32)]) -> CsiData {
        let mut iq = Vec::with_capacity(amps.len() * 2);
        for (i, q) in amps {
            iq.push(*i);
            iq.push(*q);
        }
        CsiData {
            data_length: iq.len() as u32,
            csi_raw_data: iq,
            ..Default::default()
        }
    }

    fn make_series_packets(values: &[f32]) -> Vec<CsiData> {
        values
            .iter()
            .map(|&v| make_packet(&[(v.round() as i32, 0)]))
            .collect()
    }

    #[test]
    fn no_op_pipeline_clones_input() {
        let cfg = PreprocessConfig::default();
        assert!(!cfg.is_active());
        let input = vec![make_packet(&[(3, 4), (6, 8)])];
        let out = cfg.apply(&input);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].csi_raw_data, input[0].csi_raw_data);
    }

    #[test]
    fn summary_lists_active_stages() {
        let cfg = PreprocessConfig {
            interpolation: Interpolation::Linear,
            denoising: Denoising::Hampel,
            detrending: Detrending::MovingAverageTrend,
            feature_scaling: FeatureScaling::ZScoreStandardization,
            ..Default::default()
        };
        let s = cfg.summary();
        assert!(s.contains("Linear"));
        assert!(s.contains("Hampel"));
    }

    #[test]
    fn linear_interpolation_fills_zero_subcarrier() {
        let cfg = PreprocessConfig {
            interpolation: Interpolation::Linear,
            ..Default::default()
        };
        let p = make_packet(&[(10, 0), (0, 0), (30, 0)]);
        let out = cfg.apply(&[p]);
        let mid_amp = out[0].amplitude(1);
        assert!((mid_amp - 20.0).abs() < 1.5, "got {mid_amp}");
    }

    #[test]
    fn nearest_neighbor_picks_closest_anchor() {
        let cfg = PreprocessConfig {
            interpolation: Interpolation::NearestNeighbor,
            ..Default::default()
        };
        let p = make_packet(&[(10, 0), (0, 0), (0, 0), (40, 0)]);
        let out = cfg.apply(&[p]);
        assert!(
            (out[0].amplitude(2) - 40.0).abs() < 1.0,
            "expected upper anchor's amplitude, got {}",
            out[0].amplitude(2)
        );
    }

    #[test]
    fn rolling_mean_smooths_spike() {
        let mut cfg = PreprocessConfig {
            denoising: Denoising::RollingMean,
            ..Default::default()
        };
        cfg.params.filter_window = 5;
        let pkts = make_series_packets(&[10.0, 10.0, 100.0, 10.0, 10.0]);
        let out = cfg.apply(&pkts);
        let mid = out[2].amplitude(0);
        assert!(mid < 50.0, "spike should be smoothed, got {mid}");
    }

    #[test]
    fn rolling_median_rejects_outlier() {
        let mut cfg = PreprocessConfig {
            denoising: Denoising::RollingMedian,
            ..Default::default()
        };
        cfg.params.filter_window = 5;
        let pkts = make_series_packets(&[10.0, 10.0, 100.0, 10.0, 10.0]);
        let out = cfg.apply(&pkts);
        let mid = out[2].amplitude(0);
        assert!((mid - 10.0).abs() < 5.0, "median should reject spike, got {mid}");
    }

    #[test]
    fn hampel_replaces_outlier() {
        let mut cfg = PreprocessConfig {
            denoising: Denoising::Hampel,
            ..Default::default()
        };
        cfg.params.filter_window = 5;
        cfg.params.hampel_sigma = 2.0;
        let pkts = make_series_packets(&[10.0, 10.0, 200.0, 10.0, 10.0]);
        let out = cfg.apply(&pkts);
        let mid = out[2].amplitude(0);
        assert!(mid < 50.0, "Hampel should replace outlier, got {mid}");
    }

    #[test]
    fn savitzky_golay_preserves_trend() {
        let mut cfg = PreprocessConfig {
            denoising: Denoising::SavitzkyGolay,
            ..Default::default()
        };
        cfg.params.filter_window = 5;
        cfg.params.sg_poly_order = 2;
        let pkts = make_series_packets(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]);
        let out = cfg.apply(&pkts);
        assert_eq!(out.len(), pkts.len());
        assert!(out[6].amplitude(0) > out[0].amplitude(0));
    }

    #[test]
    fn butterworth_lowpass_reduces_high_frequency() {
        let mut cfg = PreprocessConfig {
            denoising: Denoising::ButterworthLowPass,
            ..Default::default()
        };
        cfg.params.butterworth_cutoff = 0.05;
        let mut values = Vec::new();
        for i in 0..32 {
            let t = i as f32;
            values.push(10.0 + (t * 0.5).sin() * 5.0 + (t * 3.0).sin() * 20.0);
        }
        let pkts = make_series_packets(&values);
        let out = cfg.apply(&pkts);
        let var_in: f32 = values.iter().map(|x| (x - 10.0).powi(2)).sum();
        let amps: Vec<f32> = out.iter().map(|p| p.amplitude(0)).collect();
        let mean: f32 = amps.iter().sum::<f32>() / amps.len() as f32;
        let var_out: f32 = amps.iter().map(|x| (x - mean).powi(2)).sum();
        assert!(var_out < var_in, "low-pass should reduce variance");
    }

    #[test]
    fn wavelet_dwt_runs_without_panic() {
        let mut cfg = PreprocessConfig {
            denoising: Denoising::WaveletDwt,
            ..Default::default()
        };
        cfg.params.dwt_levels = 2;
        let pkts = make_series_packets(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]);
        let out = cfg.apply(&pkts);
        assert_eq!(out.len(), pkts.len());
    }

    #[test]
    fn fft_delay_filter_preserves_cardinality() {
        let mut cfg = PreprocessConfig {
            denoising: Denoising::FftDelayFilter,
            ..Default::default()
        };
        cfg.params.fft_delay_keep_bins = 8;
        let pkts = vec![make_packet(&[(10, 5), (20, 0), (30, -3)])];
        let out = cfg.apply(&pkts);
        assert_eq!(out.len(), 1);
        assert!(out[0].amplitude(0) > 0.0);
    }

    #[test]
    fn pipeline_preserves_cardinality() {
        let cfg = PreprocessConfig {
            interpolation: Interpolation::Linear,
            denoising: Denoising::RollingMean,
            feature_scaling: FeatureScaling::MaxMinNormalization,
            ..Default::default()
        };
        let pkts: Vec<CsiData> = (0..8)
            .map(|i: i32| make_packet(&[(i, 0), (i, 0)]))
            .collect();
        let out = cfg.apply(&pkts);
        assert_eq!(out.len(), pkts.len());
    }

    #[test]
    fn max_min_normalisation_brings_amps_into_range() {
        let mut cfg = PreprocessConfig {
            feature_scaling: FeatureScaling::MaxMinNormalization,
            ..Default::default()
        };
        cfg.params.scaling_per_packet = true;
        let p = make_packet(&[(10, 0), (50, 0), (90, 0)]);
        let out = cfg.apply(&[p]);
        let a0 = out[0].amplitude(0);
        let a2 = out[0].amplitude(2);
        assert!(a0 < 2.0, "min mapped to ~0, got {a0}");
        assert!((a2 - 255.0).abs() < 2.0, "max mapped to ~255, got {a2}");
    }
}
