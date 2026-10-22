//! In-memory CSI data model.
//!
//! - [`CsiData`] — one decoded packet. Every wire-format parser normalizes
//!   into this struct, so downstream code never branches on the format.
//! - [`NetworkStats`] — one 100 ms tick of aggregated stats (PPS, RSSI,
//!   averaged CSI). Produced by [`History::push_tick`].
//! - [`History`] — bounded ring buffer of `NetworkStats` plus the
//!   per-packet raw history used for CSV export.
//! - [`dsp`] — shared DSP helpers (`compute_cir`, `phase_to_rgba`) usable
//!   by both the in-process views and the Rerun sink.

/// Per-packet CSI representation. See [`csi::CsiData`].
pub mod csi;
/// Shared DSP helpers (`compute_cir`, `phase_to_rgba`, …).
pub mod dsp;
/// Bounded history ring buffer keyed by per-tick id.
pub mod history;
/// Pluggable post-capture pre-visualisation processing pipeline.
pub mod preprocess;
/// Per-tick aggregate stats (PPS, RSSI, SNR, …).
pub mod stats;

pub use csi::CsiData;
pub use history::{DEFAULT_HISTORY_CAP, History, TICK_MS};
pub use preprocess::PreprocessConfig;
pub use stats::NetworkStats;
