//! Per-window CSI preprocessing pipeline.
//!
//! Applied **at capture time** by `App::pump_reader` against a rolling
//! buffer of recently-received raw packets — the preprocessed output
//! lands in [`crate::data::History`], so any filter the user turns on
//! is baked into newly captured packets but never retroactively rewrites
//! packets already in the buffer.
//!
//! # Order of operations
//!
//! Stages run in a fixed order so each one sees the previous one's
//! output:
//!
//! 1. **Interpolation** — fill missing / zero subcarriers per packet.
//! 2. **Detrending** — remove per-subcarrier trends across the window.
//! 3. **Feature scaling** — normalise the resulting amplitudes.
//!
//! Each stage has an `Off` variant; with everything `Off` (the default)
//! the pipeline is a no-op clone of the input slice and
//! [`PreprocessConfig::is_active`] returns `false` so call sites can
//! skip the clone entirely.
//!
//! # Domain of operation
//!
//! The on-wire CSI payload is interleaved I/Q (`i32`). Every filter
//! works in **amplitude space** (`|H(k)| = √(I² + Q²)`) and then
//! rescales each subcarrier's I and Q proportionally so the
//! reconstructed packet keeps its phase. Subcarriers that arrive at
//! zero amplitude have no recoverable phase; the post-filter I/Q for
//! those subcarriers is `(amplitude, 0)`.

use super::CsiData;
use serde::{Deserialize, Serialize};

/// Top-level preprocessing configuration. Each stage is independent
/// and can be left `Off`; [`PreprocessConfig::is_active`] is the cheap
/// test call sites use to bypass the pipeline entirely.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PreprocessConfig {
    /// Detrending stage (Stage 2).
    #[serde(default)]
    pub detrending: Detrending,
    /// Interpolation stage (Stage 1).
    #[serde(default)]
    pub interpolation: Interpolation,
    /// Feature scaling stage (Stage 3).
    #[serde(default)]
    pub feature_scaling: FeatureScaling,
    /// Tunable knobs each stage may consult.
    #[serde(default)]
    pub params: PreprocessParams,
}

/// Parameters shared across stages. Each one is only consulted by the
/// stage(s) that name it in their doc string.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PreprocessParams {
    /// Window size in packets for [`Detrending::MovingAverageTrend`].
    pub moving_average_window: usize,
    /// Bucket count for [`FeatureScaling::Quantization`] (1..=255).
    pub quantization_levels: u32,
    /// `true` → Min-Max / Z-Score / Quantization normalise per-packet
    /// (across subcarriers). `false` → per-subcarrier (across packets).
    pub scaling_per_packet: bool,
}

impl Default for PreprocessParams {
    fn default() -> Self {
        Self {
            moving_average_window: 16,
            quantization_levels: 16,
            scaling_per_packet: true,
        }
    }
}

/// Detrending stage — remove low-frequency baselines from each
/// subcarrier's amplitude across the window.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Detrending {
    /// Pass-through.
    #[default]
    Off,
    /// Fit a per-subcarrier least-squares line to amplitude vs packet
    /// index, subtract the line. Removes both slope and constant
    /// offset.
    LeastSquaresBaseline,
    /// Per-subcarrier moving average across
    /// [`PreprocessParams::moving_average_window`] packets, subtracted
    /// from amplitude. Removes slow drift while preserving fast events.
    MovingAverageTrend,
}

/// Interpolation stage — fill packets that report `amplitude == 0`
/// for some subcarrier (pilot/DC nulls, sporadic dropouts).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Interpolation {
    /// Pass-through.
    #[default]
    Off,
    /// Linear interpolation between the nearest non-zero neighbours
    /// on each side. Edges fall back to nearest-neighbour.
    Linear,
    /// Each zero subcarrier inherits its nearest non-zero
    /// neighbour's amplitude.
    NearestNeighbor,
}

/// Feature scaling stage — re-range amplitudes for downstream models
/// or visualisations.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FeatureScaling {
    /// Pass-through.
    #[default]
    Off,
    /// Normalise amplitudes into `[0, 1]` via `(x − min) / (max − min)`.
    MaxMinNormalization,
    /// Standardise amplitudes to mean = 0, std = 1.
    ZScoreStandardization,
    /// Round amplitudes to
    /// [`PreprocessParams::quantization_levels`] equally-spaced
    /// buckets between the window's min and max.
    Quantization,
}

impl PreprocessConfig {
    /// `true` when at least one stage is doing work. Cheap to call —
    /// designed so the live-ingest path can early-return on the no-op
    /// case without cloning the buffer.
    pub fn is_active(&self) -> bool {
        !matches!(self.detrending, Detrending::Off)
            || !matches!(self.interpolation, Interpolation::Off)
            || !matches!(self.feature_scaling, FeatureScaling::Off)
    }

    /// Run the full pipeline. Returns a new owned `Vec`; the original
    /// slice is never mutated. Cardinality is preserved — output
    /// length always equals input length.
    pub fn apply(&self, packets: &[CsiData]) -> Vec<CsiData> {
        let mut out: Vec<CsiData> = packets.to_vec();
        if !self.is_active() {
            return out;
        }
        apply_interpolation(&mut out, self.interpolation);
        apply_detrending(&mut out, self.detrending, &self.params);
        apply_feature_scaling(&mut out, self.feature_scaling, &self.params);
        out
    }
}

// ---------------------------------------------------------------------
// Stage 1 — Interpolation
// ---------------------------------------------------------------------

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
        // Indices with non-zero amplitude — the "anchors" we interpolate
        // between. If everything is zero, leave the packet untouched.
        let anchors: Vec<usize> = (0..n).filter(|&k| amps[k] > 1e-6).collect();
        if anchors.is_empty() || anchors.len() == n {
            continue;
        }
        let mut new_amps = amps.clone();
        for k in 0..n {
            if new_amps[k] > 1e-6 {
                continue;
            }
            let lo = anchors.iter().rev().find(|&&a| a < k).copied();
            let hi = anchors.iter().find(|&&a| a > k).copied();
            new_amps[k] = match (lo, hi, mode) {
                (Some(l), Some(h), Interpolation::Linear) => {
                    let t = (k - l) as f32 / (h - l) as f32;
                    amps[l] * (1.0 - t) + amps[h] * t
                }
                (Some(l), Some(h), Interpolation::NearestNeighbor) => {
                    if k - l <= h - k { amps[l] } else { amps[h] }
                }
                (Some(l), None, _) => amps[l],
                (None, Some(h), _) => amps[h],
                _ => 0.0,
            };
        }
        rescale_packet_amplitudes(p, &new_amps);
    }
}

// ---------------------------------------------------------------------
// Stage 2 — Detrending
// ---------------------------------------------------------------------

fn apply_detrending(
    packets: &mut [CsiData],
    mode: Detrending,
    params: &PreprocessParams,
) {
    if matches!(mode, Detrending::Off) || packets.len() < 2 {
        return;
    }
    let Some(n) = packets.first().map(|p| p.subcarriers()) else {
        return;
    };
    if n == 0 {
        return;
    }
    let np = packets.len();

    // Materialise the (subcarrier, packet) amplitude matrix once so the
    // detrender can iterate it without paying the sqrt in the hot loop.
    let mut amps = vec![0f32; n * np];
    for (t, p) in packets.iter().enumerate() {
        let lim = n.min(p.subcarriers());
        for k in 0..lim {
            amps[k * np + t] = p.amplitude(k);
        }
    }

    match mode {
        Detrending::LeastSquaresBaseline => {
            // For each subcarrier, fit y = a·t + b to its amplitude
            // trace; replace y with y - (a·t + b).
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
            // Centered moving-average; clamp window at the edges.
            for k in 0..n {
                let mut new_row = vec![0f32; np];
                for t in 0..np {
                    let lo = t.saturating_sub(w / 2);
                    let hi = (t + w / 2 + 1).min(np);
                    let mut sum = 0f32;
                    for u in lo..hi {
                        sum += amps[k * np + u];
                    }
                    let mean = sum / (hi - lo) as f32;
                    new_row[t] = (amps[k * np + t] - mean).max(0.0);
                }
                for t in 0..np {
                    amps[k * np + t] = new_row[t];
                }
            }
        }
        Detrending::Off => unreachable!(),
    }

    for (t, p) in packets.iter_mut().enumerate() {
        let lim = n.min(p.subcarriers());
        let row: Vec<f32> = (0..lim).map(|k| amps[k * np + t]).collect();
        rescale_packet_amplitudes(p, &row);
    }
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

// ---------------------------------------------------------------------
// Stage 3 — Feature scaling
// ---------------------------------------------------------------------

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

fn scale_vector(
    xs: &[f32],
    mode: FeatureScaling,
    params: &PreprocessParams,
) -> Vec<f32> {
    match mode {
        FeatureScaling::Off => xs.to_vec(),
        FeatureScaling::MaxMinNormalization => {
            let (lo, hi) = xs.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), &v| {
                (a.min(v), b.max(v))
            });
            let span = (hi - lo).max(1e-6);
            // Scale to [0, 255]; rescale_packet_amplitudes treats these
            // as the new amplitudes and renormalises I/Q to match.
            xs.iter().map(|x| ((x - lo) / span) * 255.0).collect()
        }
        FeatureScaling::ZScoreStandardization => {
            let (m, sd) = mean_and_std(xs);
            if sd < 1e-6 {
                return vec![0.0; xs.len()];
            }
            // Shift z-scores into a strictly non-negative band so the
            // post-scaling amplitudes are still valid `|H|` values.
            xs.iter().map(|x| ((x - m) / sd + 4.0) * 16.0).collect()
        }
        FeatureScaling::Quantization => {
            let levels = params.quantization_levels.clamp(1, 255) as f32;
            let (lo, hi) = xs.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), &v| {
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

// ---------------------------------------------------------------------
// Shared helper: rewrite a packet's I/Q so each subcarrier ends up with
// the requested amplitude while keeping its phase.
// ---------------------------------------------------------------------

fn rescale_packet_amplitudes(p: &mut CsiData, new_amps: &[f32]) {
    let n = p.subcarriers().min(new_amps.len());
    for k in 0..n {
        let old = p.amplitude(k);
        let base = 2 * k;
        if base + 1 >= p.csi_raw_data.len() {
            break;
        }
        let new_amp = new_amps[k].max(0.0);
        if old < 1e-6 {
            // Lost phase — drop the new amplitude onto I.
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
        let mut p = CsiData::default();
        p.data_length = iq.len() as u32;
        p.csi_raw_data = iq;
        p
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
    fn linear_interpolation_fills_zero_subcarrier() {
        let mut cfg = PreprocessConfig::default();
        cfg.interpolation = Interpolation::Linear;
        // Subcarrier 1 is zero; interpolation should fill it between
        // the amplitudes of subcarriers 0 and 2.
        let p = make_packet(&[(10, 0), (0, 0), (30, 0)]);
        let out = cfg.apply(&[p]);
        let mid_amp = out[0].amplitude(1);
        assert!((mid_amp - 20.0).abs() < 1.5, "got {mid_amp}");
    }

    #[test]
    fn nearest_neighbor_picks_closest_anchor() {
        let mut cfg = PreprocessConfig::default();
        cfg.interpolation = Interpolation::NearestNeighbor;
        // Anchors at indices 0 and 3. Subcarrier 2 is distance 2 from
        // the lower anchor and distance 1 from the upper one, so NN
        // should pick the upper amplitude (40).
        let p = make_packet(&[(10, 0), (0, 0), (0, 0), (40, 0)]);
        let out = cfg.apply(&[p]);
        assert!(
            (out[0].amplitude(2) - 40.0).abs() < 1.0,
            "expected upper anchor's amplitude, got {}",
            out[0].amplitude(2)
        );
    }

    #[test]
    fn pipeline_preserves_cardinality() {
        let mut cfg = PreprocessConfig::default();
        cfg.interpolation = Interpolation::Linear;
        cfg.feature_scaling = FeatureScaling::MaxMinNormalization;
        let pkts: Vec<CsiData> = (0..8)
            .map(|i| make_packet(&[(i as i32, 0), (i as i32, 0)]))
            .collect();
        let out = cfg.apply(&pkts);
        assert_eq!(
            out.len(),
            pkts.len(),
            "live-friendly pipeline must keep one output per input"
        );
    }

    #[test]
    fn max_min_normalisation_brings_amps_into_range() {
        let mut cfg = PreprocessConfig::default();
        cfg.feature_scaling = FeatureScaling::MaxMinNormalization;
        cfg.params.scaling_per_packet = true;
        let p = make_packet(&[(10, 0), (50, 0), (90, 0)]);
        let out = cfg.apply(&[p]);
        // Per-packet min-max maps the smallest amplitude to 0 and the
        // largest to 255.
        let a0 = out[0].amplitude(0);
        let a2 = out[0].amplitude(2);
        assert!(a0 < 2.0, "min mapped to ~0, got {a0}");
        assert!((a2 - 255.0).abs() < 2.0, "max mapped to ~255, got {a2}");
    }
}
