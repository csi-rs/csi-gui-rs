use super::csi::CsiData;
use super::stats::{NetworkStats, GRID_SIZE};
use std::collections::VecDeque;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Default in-memory **tick** window capacity (per [`History::new`]).
///
/// Override via [`History::with_capacity`] or the binary's `--history-size`
/// flag. At 10 Hz ticks this is ~16 minutes of scrubber range by default.
/// Per-packet captures stream to JSONL during collection — not held here.
pub const DEFAULT_HISTORY_CAP: usize = 10_000;

/// UI aggregation tick, in milliseconds. The reader thread runs at hardware
/// rate; the App thread folds packets into a per-tick [`NetworkStats`]
/// every `TICK_MS` ms.
pub const TICK_MS: u64 = 100;

/// Number of ticks the PPS rolling-average window covers.
pub const PPS_WINDOW_TICKS: usize = 10;

/// Bounded ring buffer of per-tick aggregates for live plots and replay.
///
/// `History` grows at 10 Hz (one [`NetworkStats`] per [`TICK_MS`]) regardless
/// of how fast packets are arriving. Full per-packet captures are written to
/// disk when optional recording is enabled ([`crate::persistence::SessionRecorder`]); only
/// tick aggregates live in RAM.
pub struct History {
    pub entries: VecDeque<NetworkStats>,
    /// Populated only by [`Self::load_from_packets`] (import/replay). Live
    /// capture does not retain per-packet data here.
    pub raw_history: VecDeque<CsiData>,
    grid: [[f32; GRID_SIZE]; GRID_SIZE],
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
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_HISTORY_CAP)
    }

    pub fn with_capacity(cap: usize) -> Self {
        let cap = cap.max(1);
        Self {
            entries: VecDeque::with_capacity(cap),
            raw_history: VecDeque::new(),
            grid: [[0.0; GRID_SIZE]; GRID_SIZE],
            pps_window: VecDeque::with_capacity(PPS_WINDOW_TICKS),
            next_id: 0,
            started: Instant::now(),
            frames_decoded_total: 0,
            raw_fps_smoothed: 0.0,
            cap,
        }
    }

    pub fn capacity(&self) -> usize {
        self.cap
    }

    pub fn first_id(&self) -> Option<u64> {
        self.entries.front().map(|s| s.id)
    }

    pub fn last_id(&self) -> Option<u64> {
        self.entries.back().map(|s| s.id)
    }

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

    pub fn load_from_packets(&mut self, packets: Vec<CsiData>) {
        self.clear();
        for p in packets.into_iter().filter(|p| p.is_decoded()) {
            NetworkStats::accumulate_grid(&mut self.grid, &p);
            if self.raw_history.len() == self.cap {
                self.raw_history.pop_front();
            }
            self.raw_history.push_back(p.clone());

            let rssi = p.rssi;
            let nf = p.noise_floor;
            let snr = rssi - nf;
            let stats = NetworkStats {
                id: self.next_id,
                rssi,
                pps: 0,
                snr,
                timestamp: self.next_id * TICK_MS,
                csi: Some(p),
                distribution_grid: [[0.0; GRID_SIZE]; GRID_SIZE],
            };
            self.next_id += 1;
            if self.entries.len() == self.cap {
                self.entries.pop_front();
            }
            self.entries.push_back(stats);
        }
        self.frames_decoded_total = self.entries.len() as u64;
    }

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
            distribution_grid: [[0.0; GRID_SIZE]; GRID_SIZE],
        };
        self.next_id += 1;

        if self.entries.len() == self.cap {
            self.entries.pop_front();
        }
        self.entries.push_back(stats);
        self.entries.back()
    }

    pub fn last(&self) -> Option<&NetworkStats> {
        self.entries.back()
    }

    pub fn get(&self, id: u64) -> Option<&NetworkStats> {
        self.entries
            .binary_search_by(|s| s.id.cmp(&id))
            .ok()
            .map(|i| &self.entries[i])
    }

    pub fn resolve_anchor(&self, anchor: Option<u64>) -> Option<&NetworkStats> {
        match anchor {
            Some(id) => self.get(id),
            None => self.last(),
        }
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_packet(seed: i32) -> CsiData {
        CsiData {
            csi_raw_data: vec![seed, 0],
            data_length: 2,
            ..Default::default()
        }
    }

    #[test]
    fn push_tick_does_not_retain_raw_packets() {
        let mut h = History::with_capacity(32);
        let batch: Vec<CsiData> = (0..20).map(make_packet).collect();
        h.push_tick(batch, 20);
        assert!(h.raw_history.is_empty());
        assert_eq!(h.entries.len(), 1);
    }

    #[test]
    fn get_uses_binary_search() {
        let mut h = History::with_capacity(100);
        for i in 0..50 {
            h.push_tick(vec![make_packet(i)], 1);
        }
        assert!(h.get(25).is_some());
        assert!(h.get(999).is_none());
    }
}
