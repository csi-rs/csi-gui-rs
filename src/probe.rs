//! `csi-gui-rs probe`: the device pipeline without the GUI.
//!
//! Opens ports, identifies each board, applies a config, runs every ready
//! board for a few seconds under one shared session, and prints what came
//! back per device. It checks a setup over SSH or in CI with nothing but
//! the binary, exercising exactly the code the GUI uses
//! ([`crate::device::DeviceManager`]).

use crate::data::{PreprocessConfig, TICK_MS};
use crate::device::{DeviceManager, Notice};
use crate::protocol::config::{LogMode, WifiMode};
use crate::rerun_sink::{RerunSink, Transport};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

/// What to probe and how.
#[derive(Clone, Debug)]
pub struct ProbeOptions {
    /// Ports to open; empty with `auto_attach` means every ESP32 port.
    pub ports: Vec<String>,
    /// Attach every ESP32-looking port.
    pub auto_attach: bool,
    /// Simulated devices to add.
    pub simulate: usize,
    /// Baud for the ports.
    pub baud: u32,
    /// How long to collect.
    pub seconds: u64,
    /// Channel for every device (`None` keeps each one's saved config).
    pub channel: Option<u8>,
    /// Log mode for every device.
    pub log_mode: LogMode,
    /// Make the first two ready devices an ESP-NOW initiator / responder pair.
    pub esp_now_pair: bool,
    /// Write per-device JSONL recordings to the data dir.
    pub record: bool,
    /// How long to wait for every device to identify.
    pub identify_timeout: Duration,
}

impl Default for ProbeOptions {
    fn default() -> Self {
        Self {
            ports: Vec::new(),
            auto_attach: false,
            simulate: 0,
            baud: crate::shared::DEFAULT_BAUD,
            seconds: 5,
            channel: None,
            log_mode: LogMode::Serialized,
            esp_now_pair: false,
            record: false,
            identify_timeout: Duration::from_secs(15),
        }
    }
}

/// Per-device outcome.
#[derive(Clone, Debug)]
pub struct ProbeResult {
    /// Port.
    pub port: String,
    /// Final connection state, as text.
    pub state: String,
    /// `info` summary, if identified.
    pub firmware: Option<String>,
    /// MAC, if known.
    pub mac: Option<String>,
    /// Mode it ran in.
    pub mode: String,
    /// Frames read while collecting.
    pub frames: u64,
    /// Decoded packets with CSI.
    pub packets: u64,
    /// Decode rate, 0–100.
    pub decode_pct: f32,
    /// Frames lost (wire-v1 sequence gaps).
    pub lost: u64,
    /// Session id announced.
    pub session: u32,
    /// Subcarriers in the last packet.
    pub subcarriers: usize,
    /// Mean RSSI over the run.
    pub rssi: Option<i32>,
    /// Recording written, if any.
    pub recording: Option<String>,
    /// The device's last console lines, for diagnosing a failure.
    pub console_tail: Vec<String>,
}

impl ProbeResult {
    /// `true` when the device identified and delivered decodable CSI, or
    /// is an emitter (which captures none by design).
    pub fn ok(&self, emitter: bool) -> bool {
        self.firmware.is_some() && (emitter || (self.packets > 0 && self.decode_pct >= 95.0))
    }
}

/// Run the probe. Progress goes to stderr; the returned results are what
/// the caller prints.
pub fn run(opts: &ProbeOptions) -> Vec<ProbeResult> {
    let mut m = DeviceManager::new(opts.baud, 2_000);
    m.record_on_capture = opts.record;
    m.rescan();
    for p in &opts.ports {
        m.add(p);
    }
    for p in crate::sim::sim_ports(opts.simulate) {
        m.add(&p);
    }
    if opts.auto_attach {
        m.attach_all();
    }
    if m.sessions.is_empty() {
        eprintln!("probe: no devices (pass ports, --auto-attach or --simulate N)");
        return Vec::new();
    }
    eprintln!("probe: opening {} device(s)…", m.sessions.len());
    let mut rerun = RerunSink::new(Transport::Off);
    let pre = PreprocessConfig::default();
    let log = |n: Vec<Notice>| {
        for n in n {
            match n {
                Notice::Info(s) => eprintln!("  · {s}"),
                Notice::Warn(s) => eprintln!("  ! {s}"),
                Notice::Error(s) => eprintln!("  ✗ {s}"),
            }
        }
    };

    // Identify.
    let deadline = Instant::now() + opts.identify_timeout;
    while Instant::now() < deadline {
        log(m.pump());
        let settled = m
            .sessions
            .iter()
            .all(|s| s.is_ready() && s.config_applied() || s.conn_state.is_problem());
        if settled {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    // Configure.
    let ready: Vec<usize> = (0..m.sessions.len())
        .filter(|&i| m.sessions[i].is_ready())
        .collect();
    for &i in &ready {
        let s = &mut m.sessions[i];
        s.cfg.log_mode = opts.log_mode;
        if let Some(ch) = opts.channel {
            s.cfg.channel = ch;
        }
        if opts.esp_now_pair && ready.len() >= 2 && i != ready[0] && i != ready[1] {
            // Everyone else listens to the pair.
            s.cfg.wifi_mode = WifiMode::Sniffer;
        }
        if let Err(e) = s.apply_config() {
            eprintln!("  ✗ {}: {e}", s.label());
        }
    }
    if opts.esp_now_pair && ready.len() >= 2 {
        match m.pair_esp_now(ready[0], ready[1]) {
            Ok(()) => eprintln!(
                "  · paired {} (initiator) ⇄ {} (responder)",
                m.sessions[ready[0]].label(),
                m.sessions[ready[1]].label()
            ),
            Err(e) => eprintln!("  ✗ pairing: {e}"),
        }
    }
    // Let the config land before starting.
    let applied_by = Instant::now() + Duration::from_secs(5);
    while Instant::now() < applied_by && !ready.iter().all(|&i| m.sessions[i].config_applied()) {
        log(m.pump());
        std::thread::sleep(Duration::from_millis(50));
    }
    std::thread::sleep(Duration::from_millis(300));

    // Collect.
    let (session, notices) = m.start_all(&ready);
    log(notices);
    let mut packets = vec![0u64; m.sessions.len()];
    let mut rssi_sum = vec![0i64; m.sessions.len()];
    let end = Instant::now() + Duration::from_secs(opts.seconds);
    let mut last_tick = Instant::now();
    while Instant::now() < end {
        log(m.pump());
        if last_tick.elapsed() >= Duration::from_millis(TICK_MS) {
            m.tick(&pre, &mut rerun);
            for (i, s) in m.sessions.iter().enumerate() {
                if let Some(t) = s.history.last().filter(|t| t.id == m.tick) {
                    packets[i] += 1;
                    rssi_sum[i] += t.rssi as i64;
                }
            }
            last_tick = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    // Totals before stopping (Stop resets the counters).
    let snapshot: Vec<(u64, u64, f32, u64, u32)> = m
        .sessions
        .iter()
        .map(|s| {
            let total = s.shared.frames_total.load(Ordering::Relaxed);
            let failed = s.shared.frames_decode_failed.load(Ordering::Relaxed);
            (
                total,
                total.saturating_sub(failed),
                s.shared.decode_rate() * 100.0,
                s.shared.frames_lost.load(Ordering::Relaxed),
                s.shared.session_id.load(Ordering::Relaxed),
            )
        })
        .collect();
    m.stop_all(&ready);
    let stop_by = Instant::now() + Duration::from_secs(2);
    while Instant::now() < stop_by {
        log(m.pump());
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = session;

    let results = m
        .sessions
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let (frames, decoded, pct, lost, sid) = snapshot[i];
            ProbeResult {
                port: s.port.clone(),
                state: format!("{:?}", s.conn_state),
                firmware: s.info.as_ref().map(|i| i.summary()),
                mac: s.mac(),
                mode: s.cfg.wifi_mode.cli().to_string(),
                frames,
                packets: decoded,
                decode_pct: pct,
                lost,
                session: sid,
                subcarriers: s
                    .history
                    .last()
                    .and_then(|t| t.csi.as_ref())
                    .map(|c| c.subcarriers())
                    .unwrap_or(0),
                rssi: (packets[i] > 0).then(|| (rssi_sum[i] / packets[i] as i64) as i32),
                recording: s
                    .recorder
                    .last_recording()
                    .map(|r| r.path.display().to_string()),
                console_tail: s
                    .console
                    .snapshot(Some(60))
                    .into_iter()
                    .map(|(_, l)| l)
                    .collect(),
            }
        })
        .collect();
    m.shutdown();
    results
}

/// Print `results` as a table; returns `true` when every device passed.
pub fn report(results: &[ProbeResult], seconds: u64) -> bool {
    println!(
        "{:<16} {:<18} {:<34} {:<20} {:>7} {:>7} {:>6} {:>5} {:>8} {:>4} {:>5}",
        "port", "mac", "firmware", "mode", "frames", "pps", "dec%", "lost", "session", "sc", "rssi"
    );
    let mut all_ok = !results.is_empty();
    for r in results {
        let emitter = r.mode.contains("emitter") || r.mode == "esp-now-simplex-source";
        let ok = r.ok(emitter);
        all_ok &= ok;
        println!(
            "{:<16} {:<18} {:<34} {:<20} {:>7} {:>7.1} {:>6.1} {:>5} {:>8} {:>4} {:>5}  {}",
            r.port,
            r.mac.as_deref().unwrap_or("-"),
            r.firmware.as_deref().unwrap_or(&r.state),
            r.mode,
            r.frames,
            r.packets as f64 / seconds.max(1) as f64,
            r.decode_pct,
            r.lost,
            if r.session == 0 {
                "-".to_string()
            } else {
                format!("{:08x}", r.session)
            },
            r.subcarriers,
            r.rssi.map(|v| v.to_string()).unwrap_or_else(|| "-".into()),
            if ok { "OK" } else { "FAIL" }
        );
        if let Some(p) = &r.recording {
            println!("    recording: {p}");
        }
        if !ok {
            for l in &r.console_tail {
                println!("    | {l}");
            }
        }
    }
    let sessions: std::collections::BTreeSet<u32> = results
        .iter()
        .filter(|r| r.session != 0)
        .map(|r| r.session)
        .collect();
    if sessions.len() == 1 && results.iter().filter(|r| r.session != 0).count() > 1 {
        println!(
            "all collecting devices share session {:08x}",
            sessions.iter().next().unwrap()
        );
    }
    all_ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probes_simulated_devices() {
        let dir = std::env::temp_dir().join(format!("csi-gui-rs-probe-{}", std::process::id()));
        crate::paths::set_config_dir_override(dir.join("config"));
        crate::paths::set_data_dir_override(dir.join("data"));
        let results = run(&ProbeOptions {
            simulate: 2,
            seconds: 2,
            record: true,
            ..ProbeOptions::default()
        });
        assert_eq!(results.len(), 2);
        for r in &results {
            assert!(r.firmware.is_some(), "{r:?}");
            assert!(r.packets > 0, "{r:?}");
            assert!(r.decode_pct > 99.0, "{r:?}");
            assert!(r.recording.is_some(), "{r:?}");
        }
        assert_eq!(results[0].session, results[1].session);
        assert!(report(&results, 2));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
