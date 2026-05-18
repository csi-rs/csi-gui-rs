use super::csi::CsiData;
use super::stats::{NetworkStats, GRID_SIZE};
use std::collections::VecDeque;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Default in-memory packet history capacity (per [`History::new`]).
///
/// Override via [`History::with_capacity`] or the binary's `--history-size`
/// flag. At 100 PPS this caps memory growth to ~10 minutes by default.
pub const DEFAULT_HISTORY_CAP: usize = 10_000;

/// UI aggregation tick, in milliseconds. The reader thread runs at hardware
/// rate; the App thread folds packets into a per-tick [`NetworkStats`]
/// every `TICK_MS` ms.
pub const TICK_MS: u64 = 100;

/// Number of ticks the PPS rolling-average window covers.
pub const PPS_WINDOW_TICKS: usize = 10;

/// Bounded ring buffer of per-tick aggregates plus a per-packet raw history
/// used for CSV export.
///
/// `History` grows at 10 Hz (one [`NetworkStats`] per [`TICK_MS`]) regardless
/// of how fast packets are arriving. The internal [`VecDeque`]s are capped at
/// `cap` (see [`Self::with_capacity`]); oldest entries are dropped first.
///
/// IDs in `NetworkStats` are monotonically increasing and stable for the
/// lifetime of the buffer — once a packet has an id it keeps it, even if it
/// later falls off the back. This is what makes the global replay anchor
/// (`Option<u64>`) work.
///
/// # Example
///
/// ```
/// use csi_gui_rs::data::{CsiData, History};
///
/// let mut h = History::with_capacity(1_000);
/// let mut pkt = CsiData::default();
/// pkt.csi_raw_data = vec![1, 2, 3, 4];     // 2 subcarriers, gets decoded
/// h.push_tick(vec![pkt], 1);
/// assert_eq!(h.entries.len(), 1);
/// assert_eq!(h.last_id(), Some(0));
/// ```
pub struct History {
    pub entries: VecDeque<NetworkStats>,
    pub raw_history: VecDeque<CsiData>,
    pub grid: [[f32; GRID_SIZE]; GRID_SIZE],
    pps_window: VecDeque<u64>,
    next_id: u64,
    started: Instant,
    pub frames_decoded_total: u64,
    pub raw_fps_smoothed: f32,
    cap: usize,
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}

impl History {
    /// Construct a `History` with the default capacity ([`DEFAULT_HISTORY_CAP`]).
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_HISTORY_CAP)
    }

    /// Construct a `History` capped at `cap` entries. A `cap` of 0 is
    /// promoted to 1 (so `entries.back()` is always meaningful once data
    /// arrives).
    pub fn with_capacity(cap: usize) -> Self {
        let cap = cap.max(1);
        Self {
            entries: VecDeque::with_capacity(cap),
            raw_history: VecDeque::with_capacity(cap),
            grid: [[0.0; GRID_SIZE]; GRID_SIZE],
            pps_window: VecDeque::with_capacity(PPS_WINDOW_TICKS),
            next_id: 0,
            started: Instant::now(),
            frames_decoded_total: 0,
            raw_fps_smoothed: 0.0,
            cap,
        }
    }

    /// Configured ring-buffer capacity.
    pub fn capacity(&self) -> usize {
        self.cap
    }

    /// Smallest id still resident in the buffer, or `None` if empty.
    pub fn first_id(&self) -> Option<u64> {
        self.entries.front().map(|s| s.id)
    }

    /// Largest id resident in the buffer (i.e. the head — "now"), or `None`
    /// if empty.
    pub fn last_id(&self) -> Option<u64> {
        self.entries.back().map(|s| s.id)
    }

    /// Drop all history and reset the id counter to 0.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.raw_history.clear();
        self.grid = [[0.0; GRID_SIZE]; GRID_SIZE];
        self.pps_window.clear();
        self.next_id = 0;
        self.frames_decoded_total = 0;
        self.raw_fps_smoothed = 0.0;
        self.started = Instant::now();
    }

    /// Push one 100 ms tick of packets.
    ///
    /// `raw_frames_delta` is `EspSharedState::frames_total` delta this tick,
    /// used only to compute the diagnostic `raw_fps`.
    pub fn push_tick(&mut self, batch: Vec<CsiData>, raw_frames_delta: u64) -> Option<&NetworkStats> {
        let decoded: Vec<CsiData> = batch.into_iter().filter(|p| p.is_decoded()).collect();
        let decoded_count = decoded.len();
        self.frames_decoded_total += decoded_count as u64;

        let raw_fps_new = (raw_frames_delta as f32) * 1000.0 / (TICK_MS as f32);
        self.raw_fps_smoothed = (self.raw_fps_smoothed * 3.0 + raw_fps_new) / 4.0;

        let instant_pps = (decoded_count as u64) * 1000 / TICK_MS;
        if self.pps_window.len() == PPS_WINDOW_TICKS {
            self.pps_window.pop_front();
        }
        self.pps_window.push_back(instant_pps);
        let pps = if self.pps_window.is_empty() {
            0
        } else {
            self.pps_window.iter().sum::<u64>() / self.pps_window.len() as u64
        };

        if decoded.is_empty() {
            return None;
        }

        for p in &decoded {
            NetworkStats::accumulate_grid(&mut self.grid, p);
            if self.raw_history.len() == self.cap {
                self.raw_history.pop_front();
            }
            self.raw_history.push_back(p.clone());
        }

        let avg = CsiData::average(&decoded);
        let rssi = avg.as_ref().map(|c| c.rssi).unwrap_or(-100);
        let nf = avg.as_ref().map(|c| c.noise_floor).unwrap_or(0);
        let snr = rssi - nf;

        let timestamp_ms = self.started.elapsed().as_millis() as u64;
        let _ = SystemTime::now().duration_since(UNIX_EPOCH);

        let stats = NetworkStats {
            id: self.next_id,
            rssi,
            pps,
            snr,
            timestamp: timestamp_ms,
            csi: avg,
            distribution_grid: self.grid,
        };
        self.next_id += 1;

        if self.entries.len() == self.cap {
            self.entries.pop_front();
        }
        self.entries.push_back(stats);
        self.entries.back()
    }

    /// Most-recent tick aggregate ("LIVE"), or `None` if empty.
    pub fn last(&self) -> Option<&NetworkStats> {
        self.entries.back()
    }

    /// Look up a specific tick by id. `O(N)`; fine for replay scrubbing.
    pub fn get(&self, id: u64) -> Option<&NetworkStats> {
        self.entries.iter().find(|s| s.id == id)
    }

    /// Resolve a per-pane anchor — `None` means "live" (head).
    pub fn resolve_anchor(&self, anchor: Option<u64>) -> Option<&NetworkStats> {
        match anchor {
            Some(id) => self.get(id),
            None => self.last(),
        }
    }

    /// `count` most recent entries up to and including `anchor` (or head).
    pub fn window_ending_at(&self, anchor: Option<u64>, count: usize) -> Vec<&NetworkStats> {
        let end_id = match anchor {
            Some(id) => id,
            None => match self.last() {
                Some(s) => s.id,
                None => return Vec::new(),
            },
        };
        self.entries
            .iter()
            .filter(|s| s.id <= end_id)
            .rev()
            .take(count)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }
}
