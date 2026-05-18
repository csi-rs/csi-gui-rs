//! Lock-free state shared between the reader thread and the UI thread.
//!
//! Built entirely out of atomics so neither side ever blocks on the other
//! for flags and counters.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;

/// Default serial baud rate. Matches the [`esp-csi-cli-rs`] firmware
/// default (`espflash monitor` also defaults to this).
///
/// [`esp-csi-cli-rs`]: https://github.com/connectedmotion-io/esp-csi-cli-rs
pub const DEFAULT_BAUD: u32 = 115_200;

/// Lock-free state shared between the reader thread and the UI thread.
///
/// Read by the UI for status display and configuration; written by the
/// reader for counters and connection state. The fields are deliberately
/// `pub` so views can read them without going through accessors.
///
/// Construct with [`EspSharedState::handle`] (default baud) or
/// [`EspSharedState::handle_with_baud`].
pub struct EspSharedState {
    pub is_collecting: AtomicBool,
    pub should_reset: AtomicBool,
    pub should_start: AtomicBool,
    pub log_mode_id: AtomicU8,
    pub io_generation: AtomicU64,
    pub frames_total: AtomicU64,
    pub frames_decode_failed: AtomicU64,
    pub connected: AtomicBool,
    pub baud: AtomicU32,
}

impl Default for EspSharedState {
    fn default() -> Self {
        Self::new()
    }
}

impl EspSharedState {
    /// Construct with the default baud rate ([`DEFAULT_BAUD`]).
    pub fn new() -> Self {
        Self::with_baud(DEFAULT_BAUD)
    }

    /// Construct with a specific baud rate.
    pub fn with_baud(baud: u32) -> Self {
        Self {
            is_collecting: AtomicBool::new(false),
            should_reset: AtomicBool::new(false),
            should_start: AtomicBool::new(false),
            log_mode_id: AtomicU8::new(1), // default Text
            io_generation: AtomicU64::new(0),
            frames_total: AtomicU64::new(0),
            frames_decode_failed: AtomicU64::new(0),
            connected: AtomicBool::new(false),
            baud: AtomicU32::new(baud),
        }
    }

    /// Convenience: `Arc::new(Self::new())`.
    pub fn handle() -> Arc<Self> {
        Arc::new(Self::new())
    }

    /// Convenience: `Arc::new(Self::with_baud(baud))`.
    pub fn handle_with_baud(baud: u32) -> Arc<Self> {
        Arc::new(Self::with_baud(baud))
    }

    pub fn reset_counters(&self) {
        self.frames_total.store(0, Ordering::Relaxed);
        self.frames_decode_failed.store(0, Ordering::Relaxed);
    }

    pub fn bump_io_generation(&self) -> u64 {
        self.io_generation.fetch_add(1, Ordering::AcqRel) + 1
    }

    pub fn decode_rate(&self) -> f32 {
        let total = self.frames_total.load(Ordering::Relaxed);
        if total == 0 {
            return 1.0;
        }
        let failed = self.frames_decode_failed.load(Ordering::Relaxed);
        let ok = total.saturating_sub(failed);
        (ok as f32) / (total as f32)
    }
}
