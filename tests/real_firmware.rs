//! Decode captures recorded from real esp-csi-cli-rs 0.10.1 boards
//! (`tests/fixtures/`, an ESP32-S3 sniffing on channel 11). These guard
//! the wire-v1 decoder and the array-list parser against the firmware as
//! it actually behaves, CLI echoes and prompts included.

use csi_gui_rs::protocol::info::ChipFamily;
use csi_gui_rs::protocol::parse::parse_array_list;
use csi_gui_rs::protocol::serialized::{DecodedFrame, decode_frame, is_text_frame};

#[test]
fn serialized_capture_decodes_completely() {
    let raw = include_bytes!("fixtures/serialized_sniffer_esp32s3.bin");
    let (mut csi, mut sessions, mut text, mut failed) = (0, 0, 0, 0);
    let mut seqs = Vec::new();
    for chunk in raw.split(|&b| b == 0).filter(|c| !c.is_empty()) {
        if is_text_frame(chunk) || chunk.iter().all(|b| b.is_ascii()) && chunk.len() > 256 {
            text += 1;
            continue;
        }
        match decode_frame(chunk, ChipFamily::Esp32S3) {
            Ok(DecodedFrame::Csi(p)) => {
                csi += 1;
                assert!(p.is_decoded());
                assert_eq!(p.channel, 11);
                assert!(!p.node_id.is_empty());
                assert!(p.subcarriers() > 0);
                seqs.push(p.stream_seq);
            }
            Ok(DecodedFrame::Session(s)) => {
                sessions += 1;
                assert_eq!(s.chip, "esp32s3");
            }
            Ok(DecodedFrame::Text(_)) => text += 1,
            Ok(DecodedFrame::Unplottable) => {}
            Err(_) => failed += 1,
        }
    }
    assert_eq!(failed, 0, "every binary frame must decode");
    assert_eq!(sessions, 1, "one run announcement");
    assert!(csi >= 10, "only {csi} CSI frames");
    assert!(text > 0);
    assert!(
        seqs.windows(2).all(|w| w[1] > w[0]),
        "stream_seq increases: {seqs:?}"
    );
}

#[test]
fn array_list_capture_parses() {
    let raw = include_str!("fixtures/arraylist_sniffer_esp32s3.txt");
    let rows: Vec<_> = raw
        .lines()
        .map(|l| l.trim_start_matches("> "))
        .filter(|l| l.starts_with('['))
        .map(|l| parse_array_list(l, ChipFamily::Esp32S3).expect("row parses"))
        .collect();
    assert!(rows.len() >= 10, "only {} rows", rows.len());
    for r in &rows {
        assert!(r.is_decoded());
        assert_eq!(r.mac.len(), 17, "transmitter MAC tail: {:?}", r.mac);
        assert_eq!(r.channel, 11);
    }
}
