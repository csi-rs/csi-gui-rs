//! `LogMode::Serialized` — COBS-framed postcard wire decoder.
//!
//! Each CSI packet arrives as one `0x00`-terminated COBS-encoded record.
//! Inside the record is a `postcard`-serialized `CSIDataPacket` whose
//! field order matches `esp-csi-rs/src/lib/csi.rs`. Two distinct
//! schemas exist on the wire — classic chips (ESP32 / C3 / S3) and the
//! HE chips (ESP32-C5 / C6). Both end with `csi_data: Vec<i8, 612>`.

use crate::data::CsiData;
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
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
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
    Undefined = 13,
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
        }
    }
}

/// ESP32-C5 `CSIDataPacket` mirror. See the firmware's HE-chip
/// `CSIDataPacket` for field semantics.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[allow(missing_docs)]
pub struct CsiPacketHeC5 {
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

/// ESP32-C6 `CSIDataPacket` mirror — adds `he_sigb_len` +
/// `cur_single_mpdu` after `dump_len`, and `rxmatch0` after
/// `rxmatch1`, on top of the C5 layout.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[allow(missing_docs)]
pub struct CsiPacketHeC6 {
    pub mac: [u8; 6],
    pub rssi: i32,
    pub timestamp: u32,
    pub rate: u32,
    pub noise_floor: i32,
    pub sig_len: u32,
    pub rx_state: u32,
    pub dump_len: u32,
    pub he_sigb_len: u32,
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

// Each argument is one decoded wire field shared by the HE-C5 and HE-C6
// packet layouts; grouping them into a struct would just duplicate those
// wire structs, so the flat signature is intentional.
#[allow(clippy::too_many_arguments)]
fn he_to_csidata(
    mac: [u8; 6],
    rssi: i32,
    rate: u32,
    noise_floor: i32,
    channel: u32,
    timestamp: u32,
    sig_len: u32,
    rx_state: u32,
    csi_data_len: u16,
    csi_data: Vec<i8>,
) -> CsiData {
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

impl From<CsiPacketHeC5> for CsiData {
    fn from(w: CsiPacketHeC5) -> Self {
        he_to_csidata(
            w.mac,
            w.rssi,
            w.rate,
            w.noise_floor,
            w.channel,
            w.timestamp,
            w.sig_len,
            w.rx_state,
            w.csi_data_len,
            w.csi_data,
        )
    }
}

impl From<CsiPacketHeC6> for CsiData {
    fn from(w: CsiPacketHeC6) -> Self {
        he_to_csidata(
            w.mac,
            w.rssi,
            w.rate,
            w.noise_floor,
            w.channel,
            w.timestamp,
            w.sig_len,
            w.rx_state,
            w.csi_data_len,
            w.csi_data,
        )
    }
}

fn format_mac(b: &[u8; 6]) -> String {
    format!(
        "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
        b[0], b[1], b[2], b[3], b[4], b[5]
    )
}

/// COBS-framed postcard: one packet per `0x00`-terminated chunk.
/// Tries the three known schemas (classic → C6 → C5) and returns the
/// first match. Returns `None` on COBS / postcard decode failure so
/// callers can bump the decode-failure counter.
///
/// The firmware ships each COBS byte through
/// `esp_println::Printer::write_char(b as char)`
/// (`esp-csi-rs/src/lib/logging/logging.rs:454`). That goes through the
/// default `fmt::Write::write_char` which UTF-8-encodes the char, so
/// every byte ≥ 0x80 leaves the device as a 2-byte sequence
/// (`0xC2 X` for `0x80..=0xBF`, `0xC3 X` for `0xC0..=0xFF`). We invert
/// that mapping (see `un_utf8` in this module) before handing the
/// bytes to the COBS decoder.
pub fn decode_cobs_packet(frame: &[u8]) -> Option<CsiData> {
    let unmangled = un_utf8(frame);
    let mut buf = vec![0u8; unmangled.len()];
    let n = cobs_decode(&unmangled, &mut buf)?;
    let body = &buf[..n];

    if let Ok(p) = postcard::from_bytes::<CsiPacketClassic>(body) {
        return Some(p.into());
    }
    if let Ok(p) = postcard::from_bytes::<CsiPacketHeC6>(body) {
        return Some(p.into());
    }
    if let Ok(p) = postcard::from_bytes::<CsiPacketHeC5>(body) {
        return Some(p.into());
    }
    None
}

/// Reverse the per-byte `write_char(b as char)` UTF-8 expansion the
/// firmware applies. `0xC2 X` decodes back to `X`; `0xC3 X` decodes
/// back to `X + 0x40`. Other bytes (0x00..=0x7F, except none should
/// appear unpaired) pass through.
///
/// Truncated trailing lead byte is dropped silently — the next 0x00
/// sentinel will discard the partial frame anyway.
fn un_utf8(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        let b = input[i];
        match b {
            0xC2 if i + 1 < input.len() => {
                out.push(input[i + 1]);
                i += 2;
            }
            0xC3 if i + 1 < input.len() => {
                out.push(input[i + 1].wrapping_add(0x40));
                i += 2;
            }
            _ => {
                out.push(b);
                i += 1;
            }
        }
    }
    out
}

/// Minimal COBS decoder (no allocation beyond the caller's buffer).
fn cobs_decode(src: &[u8], dst: &mut [u8]) -> Option<usize> {
    let mut s = 0usize;
    let mut d = 0usize;
    while s < src.len() {
        let code = src[s] as usize;
        if code == 0 {
            return None;
        }
        s += 1;
        for _ in 1..code {
            if s >= src.len() {
                return None;
            }
            if d >= dst.len() {
                return None;
            }
            dst[d] = src[s];
            d += 1;
            s += 1;
        }
        if code < 0xFF && s < src.len() {
            if d >= dst.len() {
                return None;
            }
            dst[d] = 0;
            d += 1;
        }
    }
    Some(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Re-apply the firmware's `write_char(b as char)` UTF-8 expansion
    /// so the test fixture matches what arrives on the wire.
    fn utf8_mangle(input: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(input.len() * 2);
        for &b in input {
            if b < 0x80 {
                out.push(b);
            } else if b < 0xC0 {
                out.push(0xC2);
                out.push(b);
            } else {
                out.push(0xC3);
                out.push(b - 0x40);
            }
        }
        out
    }

    fn cobs_encode(input: &[u8]) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::with_capacity(input.len() + 2 + input.len() / 254);
        let mut code_idx = out.len();
        out.push(1);
        let mut code: u8 = 1;
        for &b in input {
            if b == 0 {
                out[code_idx] = code;
                code_idx = out.len();
                out.push(1);
                code = 1;
            } else {
                out.push(b);
                code += 1;
                if code == 0xFF {
                    out[code_idx] = code;
                    code_idx = out.len();
                    out.push(1);
                    code = 1;
                }
            }
        }
        out[code_idx] = code;
        out.push(0);
        out
    }

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
        let payload = postcard::to_allocvec(&packet).unwrap();
        let frame = cobs_encode(&payload);
        let frame_body = &frame[..frame.len() - 1];
        // Apply the firmware's UTF-8 expansion so the test matches the
        // wire reality (this is what the reader's cobs_accum collects).
        let wire = utf8_mangle(frame_body);

        let pkt = decode_cobs_packet(&wire).expect("classic decode");
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
    fn un_utf8_round_trips_all_bytes() {
        // Every byte the firmware can output, after `write_char(b as char)`,
        // must round-trip back through `un_utf8`.
        let all: Vec<u8> = (0u8..=255u8).collect();
        let mangled = utf8_mangle(&all);
        assert_eq!(un_utf8(&mangled), all);
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
        let payload = postcard::to_allocvec(&packet).unwrap();
        let frame = cobs_encode(&payload);
        let frame_body = &frame[..frame.len() - 1];
        let wire = utf8_mangle(frame_body);
        let pkt = decode_cobs_packet(&wire).expect("classic w/ datetime");
        assert_eq!(pkt.mac, "01:02:03:04:05:06");
        assert_eq!(pkt.timestamp, 999_999);
        assert_eq!(pkt.csi_raw_data, vec![5, -6]);
    }
}
