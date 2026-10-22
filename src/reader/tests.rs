//! Reader tests against the simulated device in [`crate::sim`].

use super::*;
use crate::protocol::config::EspConfig;
use crate::sim::SimBackend;

fn wait_for(events: &Receiver<ReaderEvent>, mut pred: impl FnMut(&ReaderEvent) -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if let Ok(ev) = events.recv_timeout(Duration::from_millis(100)) {
            if pred(&ev) {
                return true;
            }
        }
    }
    false
}

fn connect(port: &str) -> (ReaderHandles, Arc<EspSharedState>, Arc<RawConsole>, DeviceInfo) {
    let shared = EspSharedState::handle();
    let console = Arc::new(RawConsole::new(500));
    let h = spawn_reader_with(SimBackend::default(), shared.clone(), console.clone());
    h.commands.send(EspCommand::OpenPort(port.into())).unwrap();
    let mut info = None;
    assert!(
        wait_for(&h.events, |ev| {
            if let ReaderEvent::Info(i) = ev {
                info = Some(i.clone());
            }
            matches!(ev, ReaderEvent::State(ConnState::Streaming))
        }),
        "device never reached Streaming"
    );
    (h, shared, console, info.unwrap())
}

fn collect(h: &ReaderHandles, n: usize) -> Vec<CsiData> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut out = Vec::new();
    while out.len() < n && Instant::now() < deadline {
        if let Ok(p) = h.packets.recv_timeout(Duration::from_millis(100)) {
            out.push(p);
        }
    }
    out
}

#[test]
fn identifies_and_streams_wire_v1() {
    let (h, shared, console, info) = connect("sim:0");
    assert_eq!(info.version, crate::sim::SIM_VERSION);
    assert_eq!(info.protocol, 4);
    assert_eq!(info.chip, ChipFamily::Esp32S3);

    let cfg = EspConfig::default();
    assert_eq!(cfg.log_mode, LogMode::Serialized);
    h.commands
        .send(EspCommand::ApplyConfig { cfg: Box::new(cfg.clone()), chip: info.chip })
        .unwrap();
    h.commands
        .send(EspCommand::Start {
            duration_sec: None,
            log_mode: cfg.log_mode,
            session: Some((0xC0FFEE, 1_800_000_000_000_000)),
        })
        .unwrap();

    let mut announced = None;
    assert!(wait_for(&h.events, |ev| {
        if let ReaderEvent::Session(s) = ev {
            announced = Some(s.clone());
            true
        } else {
            false
        }
    }));
    let s = announced.unwrap();
    assert_eq!(s.session_id, 0xC0FFEE);
    assert_eq!(s.epoch_unix_us, Some(1_800_000_000_000_000));

    let pkts = collect(&h, 20);
    assert_eq!(pkts.len(), 20, "expected 20 packets");
    for p in &pkts {
        assert!(p.is_decoded());
        assert_eq!(p.subcarriers(), 128);
        assert_eq!(p.session_id, 0xC0FFEE);
        assert_eq!(p.device, info.mac.clone().unwrap());
        assert_eq!(p.node_id, info.mac.clone().unwrap());
        assert_eq!(p.ppdu, "Ht");
        assert!(p.host_time_us > 0);
    }
    assert!(pkts.windows(2).all(|w| w[1].stream_seq == w[0].stream_seq + 1));
    assert_eq!(shared.frames_decode_failed.load(Ordering::Relaxed), 0);
    assert_eq!(shared.frames_lost.load(Ordering::Relaxed), 0);
    assert_eq!(shared.session_id.load(Ordering::Relaxed), 0xC0FFEE);
    // The CLI's own text reached the console, not the decoder.
    let lines: Vec<String> = console.snapshot(None).into_iter().map(|(_, l)| l).collect();
    assert!(lines.iter().any(|l| l.contains("Starting CSI collection")), "{lines:?}");

    h.commands.send(EspCommand::Stop).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert!(!shared.is_collecting.load(Ordering::Relaxed));
    h.commands.send(EspCommand::Shutdown).unwrap();
}

#[test]
fn streams_array_list_with_mac_tail() {
    let (h, _shared, _console, info) = connect("sim:2");
    let cfg = EspConfig {
        log_mode: LogMode::ArrayList,
        ..EspConfig::default()
    };
    h.commands
        .send(EspCommand::ApplyConfig { cfg: Box::new(cfg.clone()), chip: info.chip })
        .unwrap();
    h.commands
        .send(EspCommand::Start { duration_sec: None, log_mode: cfg.log_mode, session: None })
        .unwrap();
    let pkts = collect(&h, 5);
    assert_eq!(pkts.len(), 5);
    assert!(pkts.iter().all(|p| p.is_decoded() && p.mac == "5A:1E:FF:00:00:02"));
    h.commands.send(EspCommand::Shutdown).unwrap();
}

#[test]
fn timed_run_ends_by_itself() {
    let (h, shared, _console, info) = connect("sim:1");
    assert_eq!(info.chip, ChipFamily::Esp32C5);
    let cfg = EspConfig::default();
    h.commands
        .send(EspCommand::ApplyConfig { cfg: Box::new(cfg.clone()), chip: info.chip })
        .unwrap();
    h.commands
        .send(EspCommand::Start { duration_sec: Some(1), log_mode: cfg.log_mode, session: None })
        .unwrap();
    assert!(wait_for(&h.events, |ev| matches!(ev, ReaderEvent::CollectionEnded)));
    assert!(!shared.is_collecting.load(Ordering::Relaxed));
    h.commands.send(EspCommand::Shutdown).unwrap();
}

#[test]
fn banner_needs_line_start_and_terminator() {
    assert!(banner_match(b"noise\nESP-CSI-CLI/0.10.1\r\n", b"ESP-CSI-CLI/"));
    assert!(!banner_match(b"xESP-CSI-CLI/0.10.1\r\n", b"ESP-CSI-CLI/"));
    assert!(!banner_match(b"ESP-CSI-CLI/0.10", b"ESP-CSI-CLI/"));
}
