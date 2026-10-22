//! `LogMode::Serialized` — COBS-framed postcard wire decoder.
//!
//! Every record on the serialized stream is one `0x00`-terminated COBS
//! frame. Three things can be inside:
//!
//! - **Wire v1** (esp-csi-rs 0.12+, esp-csi-cli-rs 0.9+): a postcard
//!   [`wire::Envelope`] followed by a [`wire::Body`], decoded with the
//!   vendored [`crate::protocol::wire`] module. A run opens with one
//!   `Body::Session` announcement, then `Body::Csi` measurements.
//! - **Legacy** (esp-csi-rs ≤ 0.11): a bare postcard `CSIDataPacket`,
//!   whose layout depends on the chip (classic vs ESP32-C5 vs ESP32-C6).
//!   Kept so firmware 0.7–0.8 still decodes.
//! - **Log text**: the firmware brackets log lines as `\0 <text> \0` so
//!   they arrive as their own frame. [`decode_frame`] hands them back as
//!   [`DecodedFrame::Text`] for the console instead of counting them as
//!   decode failures.

use crate::data::CsiData;
use crate::protocol::info::ChipFamily;
use crate::protocol::wire;
use serde::{Deserialize, Serialize};

/// Mirror of `esp_csi_rs::time::DateTime` (all `u64`, varint-encoded by
/// postcard). Wrapped in `Option<…>` on the wire, so a `None` is a
/// single `0x00` byte at that field position.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[allow(missing_docs)] // each field maps 1:1 to esp_csi_rs::time::DateTime
pub struct DateTimeWire {
    pub year: u64,
    pub month: u64,
    pub day: u64,
    pub hour: u64,
    pub minute: u64,
    pub second: u64,
    pub millisecond: u64,
}

/// Mirror of `esp_csi_rs::csi::RxCSIFmt`. `#[repr(u8)]` enum → one byte
/// on the wire (discriminant only; no fields). See the firmware's
/// `RxCSIFmt` documentation for the secondary-channel × signal-mode ×
/// bandwidth × STBC encoding behind each variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
#[allow(dead_code, missing_docs)]
pub enum RxCsiFmtWire {
    Bw20 = 0,
    HtBw20 = 1,
    HtBw20Stbc = 2,
    SecbBw20 = 3,
    SecbHtBw20 = 4,
    SecbHtBw20Stbc = 5,
    SecbHtBw40 = 6,
    SecbHtBw40Stbc = 7,
    SecaBw20 = 8,
    SecaHtBw20 = 9,
    SecaHtBw20Stbc = 10,
    SecaHtBw40 = 11,
    SecaHtBw40Stbc = 12,
    // C5/C6 (esp-csi-rs 0.8.0) added the formats at 13..=15; the
    // discriminants must match `esp_csi_rs::csi::RxCSIFmt` exactly or
    // postcard rejects the enum during decode and the packet is dropped.
    VhtBw20 = 13,
    /// Held by formats this build does not name. Kept so the ordinals stay
    /// aligned with the firmware and a packet carrying one still decodes.
    Reserved14 = 14,
    /// See [`Self::Reserved14`].
    Reserved15 = 15,
    Undefined = 16,
}

/// Classic-chip `CSIDataPacket` mirror — field declaration order
/// matches `esp_csi_rs::csi::CSIDataPacket` (the non-`esp32c5`,
/// non-`esp32c6` cfg branch). Field semantics are the firmware's;
/// see [the upstream
/// reference](https://docs.rs/esp-csi-rs/latest/esp_csi_rs/csi/struct.CSIDataPacket.html).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[allow(missing_docs)]
pub struct CsiPacketClassic {
    pub mac: [u8; 6],
    pub rssi: i32,
    pub timestamp: u32,
    pub rate: u32,
    pub sgi: u32,
    pub secondary_channel: u32,
    pub channel: u32,
    pub bandwidth: u32,
    pub antenna: u32,
    pub sig_mode: u32,
    pub mcs: u32,
    pub smoothing: u32,
    pub not_sounding: u32,
    pub aggregation: u32,
    pub stbc: u32,
    pub fec_coding: u32,
    pub ampdu_cnt: u32,
    pub noise_floor: i32,
    pub rx_state: u32,
    pub sig_len: u32,
    pub date_time: Option<DateTimeWire>,
    pub sequence_number: u16,
    pub data_format: RxCsiFmtWire,
    pub csi_data_len: u16,
    pub csi_data: Vec<i8>,
}

impl From<CsiPacketClassic> for CsiData {
    fn from(w: CsiPacketClassic) -> Self {
        let csi_i32 = w.csi_data.iter().map(|v| *v as i32).collect();
        CsiData {
            mac: format_mac(&w.mac),
            rssi: w.rssi,
            rate: w.rate,
            noise_floor: w.noise_floor,
            channel: w.channel,
            timestamp: w.timestamp as u64,
            sig_len: w.sig_len,
            rx_state: w.rx_state,
            secondary_channel: w.secondary_channel,
            sgi: w.sgi,
            ant: w.antenna,
            ampdu_cnt: w.ampdu_cnt,
            sig_mode: w.sig_mode,
            mcs: w.mcs,
            cwb: w.bandwidth,
            smoothing: w.smoothing,
            not_sounding: w.not_sounding,
            aggregation: w.aggregation,
            stbc: w.stbc,
            fec_coding: w.fec_coding,
            sig_len_extra: 0,
            data_length: w.csi_data_len as u32,
            csi_raw_data: csi_i32,
            ..CsiData::default()
        }
    }
}

/// ESP32-C5 `CSIDataPacket` mirror. See the firmware.s ESP32-C5/C6
/// `CSIDataPacket` for field semantics.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[allow(missing_docs)]
pub struct CsiPacketC5 {
    pub mac: [u8; 6],
    pub rssi: i32,
    pub timestamp: u32,
    pub rate: u32,
    pub noise_floor: i32,
    pub sig_len: u32,
    pub rx_state: u32,
    pub dump_len: u32,
    pub cur_bb_format: u32,
    pub rx_channel_estimate_info_vld: u32,
    pub rx_channel_estimate_len: u32,
    pub second: u32,
    pub channel: u32,
    pub is_group: u32,
    pub rxend_state: u32,
    pub rxmatch3: u32,
    pub rxmatch2: u32,
    pub rxmatch1: u32,
    pub date_time: Option<DateTimeWire>,
    pub sequence_number: u16,
    pub csi_data_len: u16,
    pub data_format: RxCsiFmtWire,
    pub csi_data: Vec<i8>,
}

/// ESP32-C6 `CSIDataPacket` mirror — adds `sigb_len` +
/// `cur_single_mpdu` after `dump_len`, and `rxmatch0` after
/// `rxmatch1`, on top of the C5 layout.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[allow(missing_docs)]
pub struct CsiPacketC6 {
    pub mac: [u8; 6],
    pub rssi: i32,
    pub timestamp: u32,
    pub rate: u32,
    pub noise_floor: i32,
    pub sig_len: u32,
    pub rx_state: u32,
    pub dump_len: u32,
    pub sigb_len: u32,
    pub cur_single_mpdu: u32,
    pub cur_bb_format: u32,
    pub rx_channel_estimate_info_vld: u32,
    pub rx_channel_estimate_len: u32,
    pub second: u32,
    pub channel: u32,
    pub is_group: u32,
    pub rxend_state: u32,
    pub rxmatch3: u32,
    pub rxmatch2: u32,
    pub rxmatch1: u32,
    pub rxmatch0: u32,
    pub date_time: Option<DateTimeWire>,
    pub sequence_number: u16,
    pub csi_data_len: u16,
    pub data_format: RxCsiFmtWire,
    pub csi_data: Vec<i8>,
}

// Each argument is one decoded wire field shared by the ESP32-C5 and ESP32-C6
// packet layouts; grouping them into a struct would just duplicate those
// wire structs, so the flat signature is intentional.
#[allow(clippy::too_many_arguments)]
fn c5c6_to_csidata(
    header: ([u8; 6], i32, u32, i32, u32, u32, u32, u32),
    csi_data_len: u16,
    csi_data: Vec<i8>,
) -> CsiData {
    let (mac, rssi, rate, noise_floor, channel, timestamp, sig_len, rx_state) = header;
    let csi_i32 = csi_data.iter().map(|v| *v as i32).collect();
    CsiData {
        mac: format_mac(&mac),
        rssi,
        rate,
        noise_floor,
        channel,
        timestamp: timestamp as u64,
        sig_len,
        rx_state,
        data_length: csi_data_len as u32,
        csi_raw_data: csi_i32,
        ..CsiData::default()
    }
}

impl From<CsiPacketC5> for CsiData {
    fn from(w: CsiPacketC5) -> Self {
        c5c6_to_csidata(
            (
                w.mac,
                w.rssi,
                w.rate,
                w.noise_floor,
                w.channel,
                w.timestamp,
                w.sig_len,
                w.rx_state,
            ),
            w.csi_data_len,
            w.csi_data,
        )
    }
}

impl From<CsiPacketC6> for CsiData {
    fn from(w: CsiPacketC6) -> Self {
        c5c6_to_csidata(
            (
                w.mac,
                w.rssi,
                w.rate,
                w.noise_floor,
                w.channel,
                w.timestamp,
                w.sig_len,
                w.rx_state,
            ),
            w.csi_data_len,
            w.csi_data,
        )
    }
}

/// One decoded serialized-stream frame. See the module docs.
#[derive(Debug, Clone)]
pub enum DecodedFrame {
    /// A CSI measurement.
    Csi(Box<CsiData>),
    /// The announcement a wire-v1 node emits at the start of each run.
    Session(SessionAnnounce),
    /// A bracketed firmware log line.
    Text(String),
    /// A wire-v1 frame this GUI understands but cannot plot: a grouped
    /// (802.11bf) or variation report. Counted, not drawn.
    Unplottable,
}

/// The host-side view of a wire-v1 [`wire::SessionInfo`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionAnnounce {
    /// Base MAC of the announcing node.
    pub node_id: String,
    /// The session the run belongs to.
    pub session_id: u32,
    /// Chip name as the firmware reports it, e.g. `"esp32c5"`.
    pub chip: &'static str,
    /// esp-csi-rs version the firmware was built against.
    pub crate_version: [u8; 3],
    /// Wall-clock anchor set with `set-session --epoch`, if any.
    pub epoch_unix_us: Option<u64>,
}

/// Why a serialized frame did not decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameError {
    /// Neither the wire-v1 nor the legacy layout fit.
    Malformed,
    /// A wire frame from a newer format version, which this build can't
    /// read. Update the GUI.
    UnsupportedVersion(u8),
}

/// Largest frame treated as a log line.
const MAX_TEXT_FRAME: usize = 256;

/// `true` when `frame` reads as a firmware log line rather than binary:
/// short and entirely printable ASCII (plus tab / CR / LF / ESC). A real
/// CSI frame is hundreds of bytes and full of control bytes, so the two
/// never collide in practice. Same heuristic as csi-webserver-core.
pub fn is_text_frame(frame: &[u8]) -> bool {
    !frame.is_empty()
        && frame.len() <= MAX_TEXT_FRAME
        && frame
            .iter()
            .all(|&b| (0x20..0x7f).contains(&b) || matches!(b, b'\t' | b'\r' | b'\n' | 0x1b))
}

/// Decode one serialized-stream frame (the bytes before its `\0`).
///
/// Tries, in order: log text, wire v1, then the legacy layout for `chip`.
/// A frame whose first byte names another wire version is only reported
/// as [`FrameError::UnsupportedVersion`] when the legacy layout does not
/// decode either: a legacy frame starts with a MAC, whose first byte can
/// read as any version.
pub fn decode_frame(frame: &[u8], chip: ChipFamily) -> Result<DecodedFrame, FrameError> {
    if is_text_frame(frame) {
        let text = String::from_utf8_lossy(frame);
        return Ok(DecodedFrame::Text(text.trim_matches(['\r', '\n']).to_string()));
    }
    let mut owned = frame.to_vec();
    match wire::decode_cobs(&mut owned) {
        Ok((env, wire::Body::Csi(f))) => return Ok(from_wire(&env, f)),
        Ok((env, wire::Body::Session(info))) => {
            return Ok(DecodedFrame::Session(SessionAnnounce {
                node_id: format_mac(&env.node_id),
                session_id: env.session_id,
                chip: wire_chip_name(info.chip),
                crate_version: info.crate_version,
                epoch_unix_us: info.epoch_unix_us,
            }));
        }
        Err(wire::DecodeError::UnsupportedVersion(v)) => {
            return decode_cobs_packet(frame, chip)
                .map(|p| DecodedFrame::Csi(Box::new(p)))
                .ok_or(FrameError::UnsupportedVersion(v));
        }
        Err(_) => {}
    }
    decode_cobs_packet(frame, chip)
        .map(|p| DecodedFrame::Csi(Box::new(p)))
        .ok_or(FrameError::Malformed)
}

fn wire_chip_name(c: wire::Chip) -> &'static str {
    match c {
        wire::Chip::Esp32 => "esp32",
        wire::Chip::Esp32S2 => "esp32s2",
        wire::Chip::Esp32S3 => "esp32s3",
        wire::Chip::Esp32C3 => "esp32c3",
        wire::Chip::Esp32C5 => "esp32c5",
        wire::Chip::Esp32C6 => "esp32c6",
    }
}

fn opt_flag(b: Option<bool>) -> u32 {
    b.map(u32::from).unwrap_or(0)
}

/// Flatten a wire-v1 measurement into [`CsiData`]. Mirrors
/// `csi_webserver_core::csi::from_wire`, so a capture reads the same in
/// both tools.
fn from_wire(env: &wire::Envelope, f: wire::CsiFrame) -> DecodedFrame {
    let m = &f.meta;
    let bytes = match &f.payload {
        wire::CsiPayload::EspRaw { bytes, .. } => bytes,
        _ => return DecodedFrame::Unplottable,
    };
    let forty = m.bandwidth == Some(wire::Bandwidth::Mhz40);
    let mut d = CsiData {
        node_id: format_mac(&env.node_id),
        session_id: env.session_id,
        stream_seq: env.stream_seq,
        mac: f.transmitter().map(|a| format_mac(&a)).unwrap_or_default(),
        rssi: m.rssi as i32,
        noise_floor: m.noise_floor as i32,
        channel: m.channel as u32,
        timestamp: m.timestamp_us,
        sig_len: m.sig_len as u32,
        rx_state: m.rx_state as u32,
        secondary_channel: match m.secondary {
            wire::Secondary::Above => 1,
            wire::Secondary::Below => 2,
            _ => 0,
        },
        sgi: opt_flag(m.sgi),
        stbc: opt_flag(m.stbc),
        ant: m.antenna.unwrap_or(0) as u32,
        mcs: m.mcs.unwrap_or(0) as u32,
        cwb: forty as u32,
        bandwidth_mhz: m.bandwidth.map(|b| b.mhz() as u32).unwrap_or(0),
        not_sounding: opt_flag(m.not_sounding),
        aggregation: opt_flag(m.aggregation),
        sequence_number: m.frame_seq.unwrap_or(0) as u32,
        ppdu: format!("{:?}", m.ppdu),
        data_length: bytes.len() as u32,
        csi_raw_data: bytes.iter().map(|v| *v as i32).collect(),
        ..CsiData::default()
    };
    match m.vendor {
        wire::VendorRx::EspClassic {
            rate,
            sig_mode,
            smoothing,
            fec_ldpc,
            ampdu_cnt,
        } => {
            d.rate = rate as u32;
            d.sig_mode = sig_mode as u32;
            d.smoothing = smoothing as u32;
            d.fec_coding = fec_ldpc as u32;
            d.ampdu_cnt = ampdu_cnt as u32;
        }
        wire::VendorRx::EspHe { rate, .. } => d.rate = rate as u32,
        wire::VendorRx::None => {}
    }
    DecodedFrame::Csi(Box::new(d))
}

fn format_mac(b: &[u8; 6]) -> String {
    format!(
        "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
        b[0], b[1], b[2], b[3], b[4], b[5]
    )
}

/// Decode one COBS-framed postcard CSI frame into [`CsiData`].
///
/// `frame` is the buffer accumulated up to (not including) the `\0` COBS
/// terminator. `postcard::take_from_bytes_cobs` decodes in place and
/// tolerates trailing pad bytes after the encoded struct, mirroring
/// csi-webserver-rs and the firmware's length-tolerant decode path.
pub fn decode_cobs_packet(frame: &[u8], chip: ChipFamily) -> Option<CsiData> {
    let mut owned = frame.to_vec();
    let decoded = match chip {
        ChipFamily::Esp32C5 => {
            let (p, _) = postcard::take_from_bytes_cobs::<CsiPacketC5>(&mut owned).ok()?;
            p.into()
        }
        ChipFamily::Esp32C6 => {
            let (p, _) = postcard::take_from_bytes_cobs::<CsiPacketC6>(&mut owned).ok()?;
            p.into()
        }
        _ => {
            let (p, _) = postcard::take_from_bytes_cobs::<CsiPacketClassic>(&mut owned).ok()?;
            p.into()
        }
    };
    Some(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_classic_packet_no_datetime() {
        let packet = CsiPacketClassic {
            mac: [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF],
            rssi: -55,
            timestamp: 123_456,
            rate: 11,
            sgi: 1,
            secondary_channel: 0,
            channel: 6,
            bandwidth: 0,
            antenna: 0,
            sig_mode: 0,
            mcs: 0,
            smoothing: 0,
            not_sounding: 0,
            aggregation: 0,
            stbc: 0,
            fec_coding: 0,
            ampdu_cnt: 0,
            noise_floor: -90,
            rx_state: 0,
            sig_len: 128,
            date_time: None,
            sequence_number: 42,
            data_format: RxCsiFmtWire::HtBw20,
            csi_data_len: 4,
            csi_data: vec![1, -2, 3, -4],
        };
        let mut buf = vec![0u8; 1024];
        let cobs = postcard::to_slice_cobs(&packet, &mut buf).unwrap();
        let body = cobs.strip_suffix(&[0]).unwrap_or(cobs);

        let pkt = decode_cobs_packet(body, ChipFamily::Esp32).expect("classic decode");
        assert_eq!(pkt.mac, "AA:BB:CC:DD:EE:FF");
        assert_eq!(pkt.rssi, -55);
        assert_eq!(pkt.channel, 6);
        assert_eq!(pkt.timestamp, 123_456);
        assert_eq!(pkt.noise_floor, -90);
        assert_eq!(pkt.sig_len, 128);
        assert_eq!(pkt.csi_raw_data, vec![1, -2, 3, -4]);
        assert_eq!(pkt.data_length, 4);
    }

    #[test]
    fn decodes_classic_packet_with_datetime() {
        let packet = CsiPacketClassic {
            mac: [0x01, 0x02, 0x03, 0x04, 0x05, 0x06],
            rssi: -42,
            timestamp: 999_999,
            rate: 6,
            sgi: 0,
            secondary_channel: 0,
            channel: 11,
            bandwidth: 0,
            antenna: 0,
            sig_mode: 1,
            mcs: 7,
            smoothing: 1,
            not_sounding: 1,
            aggregation: 0,
            stbc: 0,
            fec_coding: 0,
            ampdu_cnt: 0,
            noise_floor: -95,
            rx_state: 0,
            sig_len: 64,
            date_time: Some(DateTimeWire {
                year: 2026,
                month: 5,
                day: 18,
                hour: 12,
                minute: 34,
                second: 56,
                millisecond: 789,
            }),
            sequence_number: 7,
            data_format: RxCsiFmtWire::Bw20,
            csi_data_len: 2,
            csi_data: vec![5, -6],
        };
        let mut buf = vec![0u8; 1024];
        let cobs = postcard::to_slice_cobs(&packet, &mut buf).unwrap();
        let body = cobs.strip_suffix(&[0]).unwrap_or(cobs);
        let pkt = decode_cobs_packet(body, ChipFamily::Esp32).expect("classic w/ datetime");
        assert_eq!(pkt.mac, "01:02:03:04:05:06");
        assert_eq!(pkt.timestamp, 999_999);
        assert_eq!(pkt.csi_raw_data, vec![5, -6]);
    }

    /// Locks in the `RxCsiFmtWire` fix: a C6 packet whose `data_format` is one
    /// of the values added in esp-csi-rs 0.8.0 (discriminant 14) must
    /// postcard-roundtrip. Before the enum was extended, 14 didn't exist and
    /// the packet would fail to decode.
    #[test]
    fn c6_reserved_data_format_round_trips() {
        let packet = CsiPacketC6 {
            mac: [0x11, 0x22, 0x33, 0x44, 0x55, 0x66],
            rssi: -48,
            timestamp: 55_555,
            rate: 0,
            noise_floor: -92,
            sig_len: 200,
            rx_state: 0,
            dump_len: 64,
            sigb_len: 0,
            cur_single_mpdu: 1,
            cur_bb_format: 3,
            rx_channel_estimate_info_vld: 1,
            rx_channel_estimate_len: 128,
            second: 0,
            channel: 149,
            is_group: 0,
            rxend_state: 0,
            rxmatch3: 0,
            rxmatch2: 0,
            rxmatch1: 0,
            rxmatch0: 0,
            date_time: None,
            sequence_number: 9,
            csi_data_len: 3,
            data_format: RxCsiFmtWire::Reserved14,
            csi_data: vec![7, -8, 9],
        };
        // Struct-level roundtrip proves the new discriminant (de)serializes.
        let payload = postcard::to_allocvec(&packet).unwrap();
        let decoded: CsiPacketC6 = postcard::from_bytes(&payload).unwrap();
        assert_eq!(decoded.data_format, RxCsiFmtWire::Reserved14);

        // And through the full COBS wire path used by the reader.
        let mut buf = vec![0u8; 1024];
        let cobs = postcard::to_slice_cobs(&packet, &mut buf).unwrap();
        let body = cobs.strip_suffix(&[0]).unwrap_or(cobs);
        let pkt = decode_cobs_packet(body, ChipFamily::Esp32C6).expect("C6 reserved-format decode");
        assert_eq!(pkt.mac, "11:22:33:44:55:66");
        assert_eq!(pkt.channel, 149);
        assert_eq!(pkt.csi_raw_data, vec![7, -8, 9]);
        assert_eq!(pkt.data_length, 3);
    }
}
