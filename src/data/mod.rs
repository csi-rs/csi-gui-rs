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

pub mod csi;
pub mod dsp;
pub mod history;
pub mod stats;

pub use csi::CsiData;
pub use history::{History, DEFAULT_HISTORY_CAP};
pub use stats::NetworkStats;
