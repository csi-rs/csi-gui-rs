use crate::data::CsiData;
use crate::protocol::info::ChipFamily;

/// Stateful text-mode parser. Reads `key: value` metadata lines and a final
/// `[…]` payload line; resyncs on `mac:` if framing is lost.
///
/// Matches the `LogMode::Text` block emitted by `esp-csi-cli-rs`. Handles
/// both the classic-chip metadata block (sgi, ant, ampdu_cnt, …) and the
/// C5/C6 metadata block (dump_len, cur_bb_format, rxmatch*, …) — unknown
/// keys are absorbed silently so adding new fields upstream does not break
/// framing. The "Recieved at YYYY-MM-DD HH:MM:SS.mmm" header line
/// (misspelled intentionally; matches firmware) is forwarded verbatim and
/// ignored.
#[derive(Default)]
pub struct TextParser {
    in_packet: bool,
    accum: CsiData,
    expecting_payload: bool,
    line_count: u32,
}

impl TextParser {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Feed one line. Returns `Some(CsiData)` when a packet completes; `None`
    /// when more lines are needed or the line was metadata.
    pub fn feed_line(&mut self, line: &str) -> Option<CsiData> {
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            return None;
        }

        // safety valve — 64 lines without payload means we've lost framing
        if self.in_packet {
            self.line_count += 1;
            if self.line_count > 64 {
                self.reset();
            }
        }

        if let Some(rest) = line.strip_prefix("mac:") {
            // start (or restart) a packet
            self.in_packet = true;
            self.line_count = 0;
            self.expecting_payload = false;
            self.accum = CsiData::default();
            self.accum.mac = rest.trim().to_string();
            return None;
        }

        if !self.in_packet {
            return None;
        }

        if self.expecting_payload {
            // payload may be on its own line (`[a, b, ...]`).
            if line.trim_start().starts_with('[') {
                self.accum.csi_raw_data = parse_int_array(line);
                let out = std::mem::take(&mut self.accum);
                self.reset();
                return Some(out);
            }
            return None;
        }

        if let Some(rest) = line.strip_prefix("csi raw data:") {
            let rest = rest.trim();
            if rest.is_empty() {
                // Array on the next line (legacy format).
                self.expecting_payload = true;
                return None;
            }
            // Inline payload on the same line (current esp-csi-cli-rs format).
            self.accum.csi_raw_data = parse_int_array(rest);
            let out = std::mem::take(&mut self.accum);
            self.reset();
            return Some(out);
        }

        if let Some((k, v)) = line.split_once(':') {
            apply_kv(&mut self.accum, k.trim(), v.trim());
        }
        None
    }
}

/// Parse `[…]` (or `[[…]]`) into a flat `Vec<i32>`. Picks hex vs decimal
/// uniformly per array based on whether any token contains a hex-only digit
/// (A-F), so the current `[[D5, 0, 67, …]]` byte-hex format and the legacy
/// `[1, -2, 3, …]` decimal format both work — but "67" inside a hex array
/// resolves to 0x67=103, not 67.
fn parse_int_array(line: &str) -> Vec<i32> {
    let inner = line
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']');
    let tokens: Vec<&str> = inner
        .split(|c: char| c == ',' || c.is_whitespace())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    let any_hex_letter = tokens
        .iter()
        .any(|t| t.bytes().any(|b| matches!(b, b'a'..=b'f' | b'A'..=b'F')));
    let radix = if any_hex_letter { 16 } else { 10 };
    tokens.iter().filter_map(|t| parse_int_token(t, radix)).collect()
}

fn parse_int_token(s: &str, radix: u32) -> Option<i32> {
    if let Some(rest) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return u32::from_str_radix(rest, 16).ok().map(|v| v as i32);
    }
    if radix == 16 {
        u32::from_str_radix(s, 16).ok().map(|v| v as i32)
    } else {
        s.parse::<i32>().ok()
    }
}

fn apply_kv(c: &mut CsiData, k: &str, v: &str) {
    let parse_i = |s: &str| s.parse::<i64>().ok();
    let parse_u = |s: &str| s.parse::<u64>().ok();
    // The current esp-csi-cli-rs text format uses space-separated keys (e.g.
    // "noise floor", "sig len"); older variants use underscores. Normalize so
    // a single match arm covers both.
    let key_owned: String;
    let key = if k.contains(' ') {
        key_owned = k.replace(' ', "_");
        key_owned.as_str()
    } else {
        k
    };
    match key {
        "rssi" => {
            if let Some(x) = parse_i(v) {
                c.rssi = x as i32;
            }
        }
        "rate" => {
            if let Some(x) = parse_u(v) {
                c.rate = x as u32;
            }
        }
        "noise_floor" => {
            if let Some(x) = parse_i(v) {
                c.noise_floor = x as i32;
            }
        }
        "channel" => {
            if let Some(x) = parse_u(v) {
                c.channel = x as u32;
            }
        }
        "real_timestamp" | "local_timestamp" | "timestamp" => {
            if let Some(x) = parse_u(v) {
                c.timestamp = x;
            }
        }
        "sig_len" => {
            if let Some(x) = parse_u(v) {
                c.sig_len = x as u32;
            }
        }
        "rx_state" => {
            if let Some(x) = parse_u(v) {
                c.rx_state = x as u32;
            }
        }
        "secondary_channel" => {
            if let Some(x) = parse_u(v) {
                c.secondary_channel = x as u32;
            }
        }
        "sgi" => {
            if let Some(x) = parse_u(v) {
                c.sgi = x as u32;
            }
        }
        "ant" => {
            if let Some(x) = parse_u(v) {
                c.ant = x as u32;
            }
        }
        "ampdu_cnt" => {
            if let Some(x) = parse_u(v) {
                c.ampdu_cnt = x as u32;
            }
        }
        "sig_mode" => {
            if let Some(x) = parse_u(v) {
                c.sig_mode = x as u32;
            }
        }
        "mcs" => {
            if let Some(x) = parse_u(v) {
                c.mcs = x as u32;
            }
        }
        "bandwidth" | "cwb" => {
            if let Some(x) = parse_u(v) {
                c.cwb = x as u32;
            }
        }
        "smoothing" => {
            if let Some(x) = parse_u(v) {
                c.smoothing = x as u32;
            }
        }
        "not_sounding" => {
            if let Some(x) = parse_u(v) {
                c.not_sounding = x as u32;
            }
        }
        "aggregation" => {
            if let Some(x) = parse_u(v) {
                c.aggregation = x as u32;
            }
        }
        "stbc" => {
            if let Some(x) = parse_u(v) {
                c.stbc = x as u32;
            }
        }
        "fec_coding" => {
            if let Some(x) = parse_u(v) {
                c.fec_coding = x as u32;
            }
        }
        "len" | "data_length" => {
            if let Some(x) = parse_u(v) {
                c.data_length = x as u32;
            }
        }
        // ESP32-C5/C6 metadata block — absorbed so the parser does not lose
        // framing on HE-chip output. None of these map onto `CsiData` today,
        // but consuming them by name beats the safety-valve reset firing
        // after 64 unrecognised lines.
        "dump_len"
        | "he_sigb_len"
        | "cur_single_mpdu"
        | "cur_bb_format"
        | "rx_channel_estimate_info_vld"
        | "rx_channel_estimate_len"
        | "time_seconds"
        | "is_group"
        | "rxend_state"
        | "rxmatch0"
        | "rxmatch1"
        | "rxmatch2"
        | "rxmatch3"
        // Misc classic header bits we don't model.
        | "sequence_number" => {}
        _ => {}
    }
}

/// Parse one packet's worth of `LogMode::ArrayList`.
///
/// Wire shape (classic, 22 scalars + payload):
/// ```text
/// [<seq>,<rssi>,<rate>,<nf>,<ch>,<ts>,<sig_len>,<rx_state>,
///  <secondary_channel>,<sgi>,<ant>,<ampdu_cnt>,<sig_mode>,<mcs>,<bandwidth>,
///  <smoothing>,<not_sounding>,<aggregation>,<stbc>,<fec_coding>,
///  <sig_len>,<csi_data_len>,[<v0>,<v1>,…,<vN-1>]]
/// ```
///
/// ESP32-C5 (21 scalars) and ESP32-C6 (24 scalars) substitute the
/// classic 802.11n metadata block with the HE block (`dump_len`,
/// `cur_bb_format`, `rxmatch*`, …). `sequence_number` is the first
/// scalar in every variant; the duplicated `sig_len` and `csi_data_len`
/// are always the last two scalars before the inner CSI array.
///
/// The chip family is used to decode the middle metadata; when
/// `ChipFamily::Unknown`, scalar count auto-detects (22→classic,
/// 21→C5, 24→C6).
pub fn parse_array_list(line: &str, chip: ChipFamily) -> Option<CsiData> {
    let line = line.trim();
    if !line.starts_with('[') {
        return None;
    }
    // The line ends `]]` — outer `]` is the very last byte; inner array
    // starts at the last `[` and ends at the second-to-last `]`.
    let inner_open = line.rfind('[')?;
    if inner_open == 0 {
        // Only one `[` — no outer wrapper.
        return None;
    }
    let inner_close = line[..line.len()].rfind(']')?;
    // The outer ']' is the very last char; the inner ']' is the one before it.
    let inner_end = line[..inner_close].rfind(']').map(|p| p + 1)?;
    let inner = &line[inner_open..inner_end];
    let csi = parse_int_array(inner);

    // Scalars: between '[' and the comma immediately before the inner array.
    let scalars_str = line[1..inner_open]
        .trim_end_matches(|c: char| c == ',' || c.is_whitespace());
    let scalars: Vec<&str> = scalars_str
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    let g_u = |i: usize| -> u64 { scalars.get(i).and_then(|s| s.parse().ok()).unwrap_or(0) };
    let g_i = |i: usize| -> i64 { scalars.get(i).and_then(|s| s.parse().ok()).unwrap_or(0) };

    // Detect layout. Prefer chip hint; otherwise rely on scalar count.
    let layout = match chip {
        ChipFamily::Esp32 | ChipFamily::Esp32C3 | ChipFamily::Esp32S3 => ArrayLayout::Classic,
        ChipFamily::Esp32C5 => ArrayLayout::HeC5,
        ChipFamily::Esp32C6 => ArrayLayout::HeC6,
        ChipFamily::Unknown => match scalars.len() {
            22 => ArrayLayout::Classic,
            21 => ArrayLayout::HeC5,
            24 => ArrayLayout::HeC6,
            _ => ArrayLayout::Classic,
        },
    };

    let mut c = CsiData::default();
    c.csi_raw_data = csi;

    // Fields 0..=7 are common across all chips.
    c.rssi = g_i(1) as i32;
    c.rate = g_u(2) as u32;
    c.noise_floor = g_i(3) as i32;
    c.channel = g_u(4) as u32;
    c.timestamp = g_u(5);
    c.sig_len = g_u(6) as u32;
    c.rx_state = g_u(7) as u32;

    match layout {
        ArrayLayout::Classic => {
            c.secondary_channel = g_u(8) as u32;
            c.sgi = g_u(9) as u32;
            c.ant = g_u(10) as u32;
            c.ampdu_cnt = g_u(11) as u32;
            c.sig_mode = g_u(12) as u32;
            c.mcs = g_u(13) as u32;
            c.cwb = g_u(14) as u32;
            c.smoothing = g_u(15) as u32;
            c.not_sounding = g_u(16) as u32;
            c.aggregation = g_u(17) as u32;
            c.stbc = g_u(18) as u32;
            c.fec_coding = g_u(19) as u32;
            // 20 is the duplicated sig_len; 21 is csi_data_len.
            c.data_length = g_u(21) as u32;
        }
        ArrayLayout::HeC5 => {
            // 8 dump_len, 9 cur_bb_format, 10 rx_channel_estimate_info_vld,
            // 11 rx_channel_estimate_len, 12 second, 13 channel(HE), 14 is_group,
            // 15 rxend_state, 16 rxmatch3, 17 rxmatch2, 18 rxmatch1,
            // 19 sig_len (dup), 20 csi_data_len.
            c.data_length = g_u(20) as u32;
        }
        ArrayLayout::HeC6 => {
            // 8 dump_len, 9 he_sigb_len, 10 cur_single_mpdu,
            // 11 cur_bb_format, 12 rx_channel_estimate_info_vld,
            // 13 rx_channel_estimate_len, 14 second, 15 channel(HE), 16 is_group,
            // 17 rxend_state, 18 rxmatch3, 19 rxmatch2, 20 rxmatch1,
            // 21 rxmatch0, 22 sig_len (dup), 23 csi_data_len.
            c.data_length = g_u(23) as u32;
        }
    }

    Some(c)
}

#[derive(Copy, Clone, Debug)]
enum ArrayLayout {
    Classic,
    HeC5,
    HeC6,
}

/// 26-column CSV per packet (Hernandez ESP32-CSI-Tool). Last column is the
/// `[…]` array; separator inside the array may be space or comma.
pub fn parse_esp_csi_tool(line: &str) -> Option<CsiData> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }

    // Pull off the trailing array first since it contains commas.
    let lb = line.find('[')?;
    let rb = line.rfind(']')?;
    if rb <= lb {
        return None;
    }
    let csv_part = line[..lb].trim_end_matches(|c: char| c == ',' || c.is_whitespace());
    let arr_part = &line[lb..=rb];
    let csi_raw = parse_int_array(arr_part);

    let mut fields = csv_part.split(',').map(|s| s.trim());
    let mut c = CsiData::default();
    let _type = fields.next();
    let _id = fields.next();
    c.mac = fields.next().unwrap_or("").to_string();
    c.rssi = fields.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    c.rate = fields.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    c.sig_mode = fields.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    c.mcs = fields.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    c.cwb = fields.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    c.smoothing = fields.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    c.not_sounding = fields.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    c.aggregation = fields.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    c.stbc = fields.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    c.fec_coding = fields.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    c.sgi = fields.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    c.noise_floor = fields.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    c.ampdu_cnt = fields.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    c.channel = fields.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    c.secondary_channel = fields.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let _local_ts: u64 = fields.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    c.ant = fields.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    c.sig_len = fields.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    c.rx_state = fields.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let _real_time_set = fields.next();
    c.timestamp = fields.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    c.data_length = fields.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    c.csi_raw_data = csi_raw;
    Some(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The packet body from the user's actual firmware output. Reproduces the
    // current esp-csi-cli-rs text format: space-separated keys, inline
    // `[[hex]]` payload on the same line as `csi raw data:`.
    const PACKET_LINES: &[&str] = &[
        "mac: B0:47:E9:AE:45:98",
        "sequence number: 4063",
        "rssi: -54",
        "rate: 12",
        "noise floor: -97",
        "channel: 1",
        "timestamp: 68379926",
        "sig len: 128",
        "rx state: 0",
        "data length: 106",
        "csi raw data: [[D5, 0, 67, 0, E2, 0, 52, 0, 0, 0]]",
    ];

    #[test]
    fn parses_current_firmware_text_format() {
        let mut p = TextParser::default();
        let mut emitted = None;
        for line in PACKET_LINES {
            if let Some(pkt) = p.feed_line(line) {
                emitted = Some(pkt);
            }
        }
        let pkt = emitted.expect("parser should emit a packet on csi raw data line");
        assert_eq!(pkt.mac, "B0:47:E9:AE:45:98");
        assert_eq!(pkt.rssi, -54);
        assert_eq!(pkt.noise_floor, -97);
        assert_eq!(pkt.channel, 1);
        assert_eq!(pkt.timestamp, 68_379_926);
        assert_eq!(pkt.sig_len, 128);
        assert_eq!(pkt.rx_state, 0);
        assert_eq!(pkt.data_length, 106);
        assert!(pkt.is_decoded());
        // Hex values: D5→213, 67→103, E2→226, 52→82, plus zero padding.
        assert_eq!(pkt.csi_raw_data, vec![213, 0, 103, 0, 226, 0, 82, 0, 0, 0]);
    }

    #[test]
    fn parses_array_list_classic_layout() {
        // Classic: 22 scalars, then `[<csi>]`, terminated `]]`.
        let line = "[4063,-54,12,-97,1,68379926,128,0,0,1,0,0,0,0,0,1,1,0,0,0,128,10,[1,2,-3,4,5,6,-7,8,9,-10]]";
        let pkt = parse_array_list(line, ChipFamily::Esp32).expect("classic");
        assert_eq!(pkt.rssi, -54);
        assert_eq!(pkt.rate, 12);
        assert_eq!(pkt.noise_floor, -97);
        assert_eq!(pkt.channel, 1);
        assert_eq!(pkt.timestamp, 68_379_926);
        assert_eq!(pkt.sig_len, 128);
        assert_eq!(pkt.rx_state, 0);
        assert_eq!(pkt.secondary_channel, 0);
        assert_eq!(pkt.sgi, 1);
        assert_eq!(pkt.ampdu_cnt, 0);
        assert_eq!(pkt.smoothing, 1);
        assert_eq!(pkt.not_sounding, 1);
        assert_eq!(pkt.data_length, 10);
        assert_eq!(pkt.csi_raw_data, vec![1, 2, -3, 4, 5, 6, -7, 8, 9, -10]);
    }

    #[test]
    fn parses_array_list_he_c5_layout() {
        // C5: 21 scalars (no he_sigb_len, no cur_single_mpdu, no rxmatch0).
        let line = "[100,-60,11,-95,36,12345,256,0,512,0,0,0,5,36,0,0,1,2,3,256,4,[10,20,30,40]]";
        let pkt = parse_array_list(line, ChipFamily::Esp32C5).expect("c5");
        assert_eq!(pkt.rssi, -60);
        assert_eq!(pkt.channel, 36);
        assert_eq!(pkt.timestamp, 12345);
        assert_eq!(pkt.sig_len, 256);
        assert_eq!(pkt.data_length, 4);
        assert_eq!(pkt.csi_raw_data, vec![10, 20, 30, 40]);
    }

    #[test]
    fn parses_array_list_he_c6_layout() {
        // C6: 24 scalars (extra he_sigb_len + cur_single_mpdu + rxmatch0).
        let line = "[7,-70,13,-92,40,99,512,0,1024,8,1,0,0,0,7,40,1,0,1,2,3,4,512,6,[1,2,3,4,5,6]]";
        let pkt = parse_array_list(line, ChipFamily::Esp32C6).expect("c6");
        assert_eq!(pkt.rssi, -70);
        assert_eq!(pkt.channel, 40);
        assert_eq!(pkt.sig_len, 512);
        assert_eq!(pkt.data_length, 6);
        assert_eq!(pkt.csi_raw_data, vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn array_list_autodetects_when_chip_unknown() {
        let line = "[4063,-54,12,-97,1,68379926,128,0,0,1,0,0,0,0,0,1,1,0,0,0,128,4,[1,-2,3,-4]]";
        let pkt = parse_array_list(line, ChipFamily::Unknown).expect("auto");
        assert_eq!(pkt.csi_raw_data, vec![1, -2, 3, -4]);
    }

    #[test]
    fn text_skips_received_at_header() {
        // `LogMode::Text` prefixes the packet with `Recieved at …` (sic) when
        // the firmware has an NTP DateTime attached. The line precedes
        // `mac:` and has colons inside (HH:MM:SS), so it must not confuse
        // the parser.
        let mut p = TextParser::default();
        let lines = [
            "Recieved at 2026-05-18 12:34:56.789",
            "mac: 01:02:03:04:05:06",
            "rssi: -50",
            "csi raw data: [[1, 2, 3]]",
        ];
        let mut emitted = None;
        for l in lines {
            if let Some(pkt) = p.feed_line(l) {
                emitted = Some(pkt);
            }
        }
        let pkt = emitted.expect("packet should still complete after 'Recieved at' header");
        assert_eq!(pkt.mac, "01:02:03:04:05:06");
        assert_eq!(pkt.rssi, -50);
        assert_eq!(pkt.csi_raw_data, vec![1, 2, 3]);
    }

    #[test]
    fn text_absorbs_c5_c6_metadata_block() {
        // ESP32-C5/C6 emit the HE metadata block instead of 802.11n fields.
        // Unknown keys are ignored without resetting the parser.
        let mut p = TextParser::default();
        let lines = [
            "mac: AA:BB:CC:DD:EE:FF",
            "sequence number: 7",
            "rssi: -65",
            "rate: 11",
            "noise floor: -90",
            "channel: 36",
            "timestamp: 999",
            "sig len: 256",
            "rx state: 0",
            "dump len: 128",
            "cur bb format: 1",
            "rx channel estimate info vld: 1",
            "rx channel estimate len: 64",
            "time seconds: 5",
            "channel: 36",
            "is group: 0",
            "rxend state: 0",
            "rxmatch3: 0",
            "rxmatch2: 0",
            "rxmatch1: 0",
            "sig_len: 256",
            "data length: 8",
            "csi raw data: [[1, 2, 3, 4, 5, 6, 7, 8]]",
        ];
        let mut emitted = None;
        for l in lines {
            if let Some(pkt) = p.feed_line(l) {
                emitted = Some(pkt);
            }
        }
        let pkt = emitted.expect("C5/C6 text block should still produce a packet");
        assert_eq!(pkt.rssi, -65);
        assert_eq!(pkt.channel, 36);
        assert_eq!(pkt.timestamp, 999);
        assert_eq!(pkt.data_length, 8);
        assert_eq!(pkt.csi_raw_data.len(), 8);
    }

    #[test]
    fn parses_decimal_payload_legacy() {
        let mut p = TextParser::default();
        let lines = ["mac: AA:BB:CC:DD:EE:FF", "csi raw data:", "[1, 2, -3, 4]"];
        let mut emitted = None;
        for l in lines {
            if let Some(pkt) = p.feed_line(l) {
                emitted = Some(pkt);
            }
        }
        let pkt = emitted.expect("legacy split-line payload still works");
        assert_eq!(pkt.csi_raw_data, vec![1, 2, -3, 4]);
    }
}
