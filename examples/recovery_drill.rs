//! Recovery drill: run boards through the same `DeviceManager` the GUI
//! uses, fire scheduled faults, and print a one-line status per second so
//! you can see each board drop out and come back.
//!
//! ```text
//! recovery_drill [--seconds=N] [--pair] [--at=T:ACTION:DEV]... PORT...
//!
//!   --pair            make the first two ports an ESP-NOW initiator / responder
//!   --at=T:ACTION:DEV at T seconds, do ACTION on device index DEV:
//!                       start | stop | reset (DTR/RTS) | restart (soft reboot)
//!                       | overflow (12 commands in one write: overruns a UART FIFO)
//!   --sh=T=COMMAND    at T seconds, run COMMAND in a shell (e.g. a power cycle)
//! ```
//!
//! Physical faults go through `--sh`, e.g.
//! `--sh='10=sudo uhubctl -l 1-1.3.1 -p 2 -a cycle'`.

use csi_gui_rs::data::{PreprocessConfig, TICK_MS};
use csi_gui_rs::device::{DeviceManager, Notice};
use csi_gui_rs::protocol::command::EspCommand;
use csi_gui_rs::protocol::config::WifiMode;
use csi_gui_rs::rerun_sink::{RerunSink, Transport};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

fn main() {
    let mut seconds = 60u64;
    let mut pair = false;
    let mut plan: Vec<(u64, String, usize)> = Vec::new();
    let mut ports = Vec::new();
    for a in std::env::args().skip(1) {
        if let Some(v) = a.strip_prefix("--seconds=") {
            seconds = v.parse().expect("--seconds");
        } else if a == "--pair" {
            pair = true;
        } else if let Some(v) = a.strip_prefix("--sh=") {
            let (t, cmd) = v.split_once('=').expect("--sh=T=COMMAND");
            plan.push((t.parse().unwrap(), format!("sh {cmd}"), 0));
        } else if let Some(v) = a.strip_prefix("--at=") {
            let p: Vec<&str> = v.split(':').collect();
            plan.push((
                p[0].parse().unwrap(),
                p[1].to_string(),
                p[2].parse().unwrap(),
            ));
        } else {
            ports.push(a);
        }
    }

    let t0 = Instant::now();
    let stamp = || format!("[{:6.1}s]", t0.elapsed().as_secs_f32());
    let mut m = DeviceManager::new(115_200, 1_000);
    m.rescan();
    for p in &ports {
        m.add(p);
    }
    let mut rerun = RerunSink::new(Transport::Off);
    let pre = PreprocessConfig::default();
    let log = |t: String, n: Vec<Notice>| {
        for n in n {
            println!("{t} {n:?}");
        }
    };

    // Identify and configure.
    while t0.elapsed() < Duration::from_secs(20)
        && !m
            .sessions
            .iter()
            .all(|s| s.is_ready() && s.config_applied())
    {
        log(stamp(), m.pump());
        std::thread::sleep(Duration::from_millis(50));
    }
    for s in &mut m.sessions {
        s.cfg.channel = 6;
        if pair {
            s.cfg.wifi_mode = WifiMode::Sniffer;
        }
        let _ = s.apply_config();
    }
    if pair && m.sessions.len() >= 2 {
        m.pair_esp_now(0, 1).expect("pair");
    }
    std::thread::sleep(Duration::from_millis(1500));
    log(stamp(), m.pump());
    let targets = m.targets();
    let (_, n) = m.start_all(&targets);
    log(stamp(), n);

    let start = Instant::now();
    let mut last_tick = Instant::now();
    let mut last_print = Instant::now();
    let mut prev: Vec<u64> = vec![0; m.sessions.len()];
    let mut done = vec![false; plan.len()];
    while start.elapsed() < Duration::from_secs(seconds) {
        log(stamp(), m.pump());
        if last_tick.elapsed() >= Duration::from_millis(TICK_MS) {
            m.tick(&pre, &mut rerun);
            last_tick = Instant::now();
        }
        for (k, (at, action, dev)) in plan.iter().enumerate() {
            if done[k] || start.elapsed() < Duration::from_secs(*at) {
                continue;
            }
            done[k] = true;
            if let Some(cmd) = action.strip_prefix("sh ") {
                println!("{} >>> {cmd}", stamp());
                let _ = std::process::Command::new("sh").arg("-c").arg(cmd).spawn();
                continue;
            }
            let s = &mut m.sessions[*dev];
            println!("{} >>> {action} {}", stamp(), s.label());
            match action.as_str() {
                "start" => s.start(None),
                "stop" => s.stop(),
                "reset" => s.send(EspCommand::Reset),
                "restart" => s.send(EspCommand::Restart),
                // A dozen commands in one write: the firmware is still printing
                // the first reply when the rest overflows its RX FIFO.
                "overflow" => s.send(EspCommand::Raw(
                    ["set-wifi --mode=sniffer"; 12].join("\r\n"),
                )),
                other => println!("unknown action {other}"),
            }
        }
        if last_print.elapsed() >= Duration::from_secs(1) {
            last_print = Instant::now();
            let line: Vec<String> = m
                .sessions
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let f = s.shared.frames_total.load(Ordering::Relaxed);
                    let d = f.saturating_sub(prev[i]);
                    prev[i] = f;
                    let st = match &s.conn_state {
                        csi_gui_rs::reader::ConnState::Streaming if s.is_collecting() => {
                            "RUN".to_string()
                        }
                        csi_gui_rs::reader::ConnState::Streaming => "idle".to_string(),
                        other => format!("{other:?}").chars().take(14).collect(),
                    };
                    format!("{:>10} {:<14}{:>5}/s", s.label(), st, d)
                })
                .collect();
            println!("{} {}", stamp(), line.join(" |"));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    m.stop_all(&targets);
    std::thread::sleep(Duration::from_millis(500));
    for s in &m.sessions {
        println!("--- console {} ({:?})", s.label(), s.conn_state);
        for (ms, l) in s.console.snapshot(Some(160)) {
            println!("  | {:8.2} {l}", ms as f64 / 1000.0);
        }
    }
    m.shutdown();
}
