use super::csi::CsiData;
use serde::{Deserialize, Serialize};

/// Side length of the cumulative `distribution_grid` (cells per axis).
pub const GRID_SIZE: usize = 24;
/// `(I, Q)` value range the distribution grid spans: `-GRID_RANGE..GRID_RANGE`.
pub const GRID_RANGE: i32 = 128;

/// One UI tick (100 ms) of network statistics. PPS is decoded packets only.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NetworkStats {
    /// Monotonic tick id. Stable for the lifetime of the [`crate::data::History`]
    /// buffer; survives even after this entry has been evicted.
    pub id: u64,
    /// Window-averaged RSSI in dBm.
    pub rssi: i32,
    /// Decoded packets-per-second across the rolling PPS window.
    pub pps: u64,
    /// `rssi - noise_floor`, in dB.
    pub snr: i32,
    /// Milliseconds since the [`crate::data::History`] buffer started.
    pub timestamp: u64,
    /// Window-averaged CSI vector — `None` when nothing decoded this tick.
    pub csi: Option<CsiData>,
    /// 24×24 cumulative I/Q-distribution heatmap built across the
    /// History buffer lifetime; used by the dashboard's density preview.
    #[serde(with = "grid_serde")]
    pub distribution_grid: [[f32; GRID_SIZE]; GRID_SIZE],
}

impl Default for NetworkStats {
    fn default() -> Self {
        Self {
            id: 0,
            rssi: -100,
            pps: 0,
            snr: 0,
            timestamp: 0,
            csi: None,
            distribution_grid: [[0.0; GRID_SIZE]; GRID_SIZE],
        }
    }
}

impl NetworkStats {
    /// Bin one packet's I/Q values into the cumulative distribution grid.
    pub fn accumulate_grid(grid: &mut [[f32; GRID_SIZE]; GRID_SIZE], pkt: &CsiData) {
        for s in 0..pkt.subcarriers() {
            let (i, q) = pkt.iq(s);
            let (gx, gy) = iq_to_bin(i, q);
            grid[gy][gx] += 1.0;
        }
    }
}

fn iq_to_bin(i: i32, q: i32) -> (usize, usize) {
    let clamp = |v: i32| -> usize {
        let v = v.clamp(-GRID_RANGE, GRID_RANGE - 1);
        let shifted = v + GRID_RANGE;
        let bin = (shifted as usize * GRID_SIZE) / (GRID_RANGE as usize * 2);
        bin.min(GRID_SIZE - 1)
    };
    (clamp(i), clamp(q))
}

mod grid_serde {
    use super::GRID_SIZE;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(
        grid: &[[f32; GRID_SIZE]; GRID_SIZE],
        s: S,
    ) -> Result<S::Ok, S::Error> {
        let flat: Vec<f32> = grid.iter().flatten().copied().collect();
        flat.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<[[f32; GRID_SIZE]; GRID_SIZE], D::Error> {
        let flat: Vec<f32> = Vec::deserialize(d)?;
        let mut grid = [[0.0; GRID_SIZE]; GRID_SIZE];
        for (idx, v) in flat.into_iter().enumerate().take(GRID_SIZE * GRID_SIZE) {
            grid[idx / GRID_SIZE][idx % GRID_SIZE] = v;
        }
        Ok(grid)
    }
}
