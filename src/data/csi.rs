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
/// values are 53 (LLTF active subcarriers), 64 (HT-LTF), and 128 (HT40);
/// wider capture formats push it higher. Never hard-code 64; derive it from
/// the live packet via [`subcarriers`](Self::subcarriers).
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
/// use csi_gui_rs::data::CsiData;
///
/// let pkt = CsiData {
///     csi_raw_data: vec![10, 0, -5, 3, 7, -2],
///     ..Default::default()
/// };
/// assert_eq!(pkt.subcarriers(), 3);
/// let (i, q) = pkt.iq(1);
/// assert_eq!((i, q), (-5, 3));
/// ```
// `serde(default)` on the container lets exports written before a field
// existed still import: a missing column reads as the field's default.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CsiData {
    /// Id of the device this packet came from (see
    /// [`crate::device::DeviceId`]). Empty for packets that were never
    /// attributed, e.g. imports of files written before 2.0.
    pub device: String,
    /// Host wall-clock time the packet was read off the port, in
    /// microseconds since the Unix epoch.
    pub host_time_us: u64,
    /// Base MAC of the node that delivered the frame (wire-v1 `node_id`),
    /// as `XX:XX:XX:XX:XX:XX`. Empty for text and pre-0.12 formats.
    pub node_id: String,
    /// Measurement session the frame belongs to (wire-v1 `session_id`).
    /// Devices started together with "Start all" share one id.
    pub session_id: u32,
    /// Per-node frame counter (wire-v1 `stream_seq`). A gap is a frame
    /// lost between the radio and the host.
    pub stream_seq: u32,
    /// 802.11 sequence number of the sounding frame, when known.
    pub sequence_number: u32,
    /// PPDU format, e.g. `"NonHt"`, `"Ht"`, `"HeSu"`. Empty when unknown.
    pub ppdu: String,
    /// Channel bandwidth in MHz, or `0` when the firmware did not say.
    pub bandwidth_mhz: u32,
    /// Source MAC address as `XX:XX:XX:XX:XX:XX` (uppercase hex).
    pub mac: String,
    /// Received signal strength indicator, in dBm.
    pub rssi: i32,
    /// PHY rate index reported by the firmware (only meaningful for
    /// legacy non-HT frames).
    pub rate: u32,
    /// RF noise floor, in dBm.
    pub noise_floor: i32,
    /// Primary channel the packet was captured on.
    pub channel: u32,
    /// Local timestamp in microseconds (firmware-relative clock, not
    /// wall-clock).
    pub timestamp: u64,
    /// Packet length including the FCS.
    pub sig_len: u32,
    /// Receive-state code. `0` = no error.
    pub rx_state: u32,
    /// Secondary channel: `0` = none, `1` = above, `2` = below.
    pub secondary_channel: u32,
    /// Guard interval: `0` = long GI, `1` = short GI.
    pub sgi: u32,
    /// Antenna index that captured this packet.
    pub ant: u32,
    /// Number of subframes aggregated into the parent A-MPDU.
    pub ampdu_cnt: u32,
    /// Signal mode: `0` = non-HT, `1` = HT (11n), `3` = VHT (11ac).
    pub sig_mode: u32,
    /// Modulation Coding Scheme index.
    pub mcs: u32,
    /// Channel bandwidth: `0` = 20 MHz, `1` = 40 MHz.
    pub cwb: u32,
    /// Channel-estimate smoothing recommendation: `1` = smooth across
    /// subcarriers, `0` = per-subcarrier independent.
    pub smoothing: u32,
    /// `0` indicates a sounding PPDU.
    pub not_sounding: u32,
    /// `0` = MPDU, `1` = AMPDU.
    pub aggregation: u32,
    /// `1` for STBC-encoded packets.
    pub stbc: u32,
    /// `1` for 11n LDPC-coded packets.
    pub fec_coding: u32,
    /// Reserved for the legacy text-format's duplicated `sig_len` slot;
    /// always zero from the binary decoders.
    pub sig_len_extra: u32,
    /// Length of `csi_raw_data` in bytes (matches `csi_raw_data.len()`).
    pub data_length: u32,
    /// Interleaved I/Q samples, `[I0, Q0, I1, Q1, …]`. Empty when the
    /// packet was metadata-only (parser failure or undecoded).
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
        let len = decoded
            .iter()
            .map(|p| p.csi_raw_data.len())
            .min()
            .unwrap_or(0);
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
            device: first.device.clone(),
            host_time_us: first.host_time_us,
            node_id: first.node_id.clone(),
            session_id: first.session_id,
            stream_seq: first.stream_seq,
            sequence_number: first.sequence_number,
            ppdu: first.ppdu.clone(),
            bandwidth_mhz: first.bandwidth_mhz,
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
