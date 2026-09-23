//! Lock-free state shared between the reader thread and the UI thread.
//!
//! Built entirely out of atomics so neither side ever blocks on the other
//! for flags and counters.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;

/// Default serial baud rate. Matches the [`esp-csi-cli-rs`] firmware
/// default (`espflash monitor` also defaults to this).
///
/// [`esp-csi-cli-rs`]: https://github.com/csi-rs/esp-csi-cli-rs
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
    /// `true` while the firmware is in a collection session. Set by the
    /// reader on `EspCommand::Start`, cleared on `Stop` / disconnect.
    pub is_collecting: AtomicBool,
    /// Latch the UI sets to request a DTR/RTS chip reset on the reader
    /// thread's next iteration. The reader clears it.
    pub should_reset: AtomicBool,
    /// Reserved latch for "kick off a `start` without going through the
    /// command channel". Currently unused; kept for parity with the TUI.
    pub should_start: AtomicBool,
    /// Internal tag identifying which [`crate::protocol::config::LogMode`]
    /// parser the reader should run. Updated when `set-log-mode` is sent.
    pub log_mode_id: AtomicU8,
    /// `true` after the reader finishes an [`crate::protocol::command::EspCommand::ApplyConfig`]
    /// pass. Cleared on reconnect / reset and when the user edits config in
    /// the ESP panel until the next Save & Apply.
    pub config_applied: AtomicBool,
    /// Monotonic counter bumped whenever the I/O pipeline is re-armed
    /// (log mode change, start/stop). Lets views invalidate stale state.
    pub io_generation: AtomicU64,
    /// All frames the reader has produced (decoded + decode-failed).
    pub frames_total: AtomicU64,
    /// Subset of `frames_total` where the parser returned no payload.
    /// Drives the dashboard's decode-rate %.
    pub frames_decode_failed: AtomicU64,
    /// `true` only while [`crate::reader::ConnState::Streaming`].
    pub connected: AtomicBool,
    /// Current serial baud rate. Mutated when the user reconnects with a
    /// different setting.
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
            // Firmware boots in ArrayList; updated when `set-log-mode` is applied.
            log_mode_id: AtomicU8::new(2),
            config_applied: AtomicBool::new(false),
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

    /// Zero `frames_total` + `frames_decode_failed`. Called by the
    /// reader on start/stop so each session begins from zero.
    pub fn reset_counters(&self) {
        self.frames_total.store(0, Ordering::Relaxed);
        self.frames_decode_failed.store(0, Ordering::Relaxed);
    }

    /// Increment and return the new `io_generation`. Views compare
    /// against a cached value to drop stale state across log-mode or
    /// session transitions.
    pub fn bump_io_generation(&self) -> u64 {
        self.io_generation.fetch_add(1, Ordering::AcqRel) + 1
    }

    /// Mark session config as edited but not yet pushed to the device.
    pub fn mark_config_dirty(&self) {
        self.config_applied.store(false, Ordering::Release);
    }

    /// `true` when the reader has finished applying the current config.
    pub fn is_config_applied(&self) -> bool {
        self.config_applied.load(Ordering::Acquire)
    }

    /// Fraction of frames that decoded successfully, in `[0.0, 1.0]`.
    /// Returns `1.0` when no frames have arrived yet.
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
