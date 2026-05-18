use serde::{Deserialize, Serialize};

/// On-the-wire and in-memory representation of a single CSI packet.
///
/// Every wire-format parser (text, COBS-serialized, array-list, esp-csi-tool
/// CSV) normalizes into this struct, so consumers never have to know how
/// the bytes arrived.
///
/// # Layout
///
/// `csi_raw_data` is interleaved I/Q: `[I0, Q0, I1, Q1, ...]`. The number of
/// subcarriers is `csi_raw_data.len() / 2` and is **dynamic** — common
/// values are 53 (LLTF active subcarriers), 64 (HT-LTF), 128 (HT40), and
/// 256 (HE20). Never hard-code 64; derive it from the live packet via
/// [`subcarriers`](Self::subcarriers).
///
/// The k-th array entry corresponds to physical subcarrier `k − N/2` —
/// firmware emits in natural order with DC near the middle. Use
/// [`crate::views::subcarrier_x`] to compute the centered x-coordinate
/// when plotting.
///
/// # When `csi_raw_data` is empty
///
/// Parsers that fail to decode the array (mismatched format, truncated
/// frame, COBS decode error) still emit a `CsiData` with metadata so the
/// raw-fps counter stays accurate. Call [`is_decoded`](Self::is_decoded)
/// before drawing.
///
/// # Example
///
/// ```
/// use esp_csi_egui::data::CsiData;
///
/// let pkt = CsiData {
///     csi_raw_data: vec![10, 0, -5, 3, 7, -2],
///     ..Default::default()
/// };
/// assert_eq!(pkt.subcarriers(), 3);
/// let (i, q) = pkt.iq(1);
/// assert_eq!((i, q), (-5, 3));
/// ```
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CsiData {
    pub mac: String,
    pub rssi: i32,
    pub rate: u32,
    pub noise_floor: i32,
    pub channel: u32,
    pub timestamp: u64,
    pub sig_len: u32,
    pub rx_state: u32,
    pub secondary_channel: u32,
    pub sgi: u32,
    pub ant: u32,
    pub ampdu_cnt: u32,
    pub sig_mode: u32,
    pub mcs: u32,
    pub cwb: u32,
    pub smoothing: u32,
    pub not_sounding: u32,
    pub aggregation: u32,
    pub stbc: u32,
    pub fec_coding: u32,
    pub sig_len_extra: u32,
    pub data_length: u32,
    pub csi_raw_data: Vec<i32>,
}

impl CsiData {
    /// `true` if the parser successfully extracted the per-subcarrier I/Q
    /// payload. Metadata-only packets (e.g. CLI command responses, decode
    /// failures) return `false`.
    pub fn is_decoded(&self) -> bool {
        !self.csi_raw_data.is_empty()
    }

    /// Number of subcarriers in this packet. Always `csi_raw_data.len() / 2`.
    pub fn subcarriers(&self) -> usize {
        self.csi_raw_data.len() / 2
    }

    /// `(I, Q)` for subcarrier `idx`, or `(0, 0)` when out of range.
    pub fn iq(&self, idx: usize) -> (i32, i32) {
        let base = idx * 2;
        if base + 1 >= self.csi_raw_data.len() {
            (0, 0)
        } else {
            (self.csi_raw_data[base], self.csi_raw_data[base + 1])
        }
    }

    /// `|H(k)|` for subcarrier `idx`. Returns 0.0 when out of range.
    pub fn amplitude(&self, idx: usize) -> f32 {
        let (i, q) = self.iq(idx);
        ((i as f32).powi(2) + (q as f32).powi(2)).sqrt()
    }

    /// `∠H(k)` for subcarrier `idx`, wrapped to `(-π, π]`. Returns 0.0 when
    /// out of range. Use [`crate::data::dsp`] (or the Phase view's
    /// `Unwrapped` / `Detrended` modes) to remove the wrap if needed.
    pub fn phase(&self, idx: usize) -> f32 {
        let (i, q) = self.iq(idx);
        (q as f32).atan2(i as f32)
    }

    /// Element-wise mean across the decoded packets in `packets`. Returns
    /// `None` if no packet in the slice has been decoded. The returned
    /// packet's metadata is copied from the first decoded source (MAC,
    /// channel, rate, etc.); RSSI and noise floor are averaged.
    pub fn average(packets: &[CsiData]) -> Option<CsiData> {
        let decoded: Vec<&CsiData> = packets.iter().filter(|p| p.is_decoded()).collect();
        if decoded.is_empty() {
            return None;
        }
        let len = decoded.iter().map(|p| p.csi_raw_data.len()).min().unwrap_or(0);
        let mut sum = vec![0i64; len];
        for p in &decoded {
            for (i, v) in p.csi_raw_data.iter().take(len).enumerate() {
                sum[i] += *v as i64;
            }
        }
        let n = decoded.len() as i64;
        let avg: Vec<i32> = sum.iter().map(|s| (s / n) as i32).collect();

        let rssi = (decoded.iter().map(|p| p.rssi as i64).sum::<i64>() / n) as i32;
        let nf = (decoded.iter().map(|p| p.noise_floor as i64).sum::<i64>() / n) as i32;
        let first = decoded[0];
        Some(CsiData {
            mac: first.mac.clone(),
            rssi,
            rate: first.rate,
            noise_floor: nf,
            channel: first.channel,
            timestamp: first.timestamp,
            sig_len: first.sig_len,
            rx_state: first.rx_state,
            secondary_channel: first.secondary_channel,
            sgi: first.sgi,
            ant: first.ant,
            ampdu_cnt: first.ampdu_cnt,
            sig_mode: first.sig_mode,
            mcs: first.mcs,
            cwb: first.cwb,
            smoothing: first.smoothing,
            not_sounding: first.not_sounding,
            aggregation: first.aggregation,
            stbc: first.stbc,
            fec_coding: first.fec_coding,
            sig_len_extra: first.sig_len_extra,
            data_length: first.data_length,
            csi_raw_data: avg,
        })
    }
}
