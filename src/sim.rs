//! A simulated esp-csi-cli-rs device.
//!
//! [`SimBackend`] is a [`PortBackend`] whose ports (`sim:0`, `sim:1`, …)
//! answer like a board running esp-csi-cli-rs 0.10.1: `info`, the `set-*`
//! commands, `set-session`, `start [--duration]` and `q`. While collecting
//! it streams wire-v1 frames (serialized mode) or array-list lines,
//! encoded with the same vendored wire module the decoder uses.
//!
//! It exists for two reasons: the reader's tests run against it, and
//! `csi-gui-rs --simulate N` lets you try the whole GUI, including several
//! devices at once, with no hardware attached.

use crate::protocol::config::LogMode;
use crate::protocol::info::ChipFamily;
use crate::protocol::wire::{
    self, Bandwidth, Body, CsiFrame, CsiPayload, Envelope, LayoutId, PpduFormat, RxMeta, Secondary,
    SessionInfo, SourceKind, Stimulus, VendorRx,
};
use crate::reader::{Link, PortBackend};
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Port-name prefix of simulated devices.
pub const SIM_PREFIX: &str = "sim:";

/// The firmware version simulated devices report.
pub const SIM_VERSION: &str = "0.10.1";

/// Chips handed out to `sim:0`, `sim:1`, … in turn.
const CHIPS: [ChipFamily; 4] = [
    ChipFamily::Esp32S3,
    ChipFamily::Esp32C5,
    ChipFamily::Esp32,
    ChipFamily::Esp32C6,
];

/// `true` for a simulated port name.
pub fn is_sim_port(name: &str) -> bool {
    name.starts_with(SIM_PREFIX)
}

/// The names of `n` simulated ports.
pub fn sim_ports(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("{SIM_PREFIX}{i}")).collect()
}

/// [`PortBackend`] for simulated devices. Each port name gets its own
/// device, kept across re-opens like a real board.
type SharedDevice = Arc<Mutex<SimDevice>>;

#[derive(Default, Clone)]
pub struct SimBackend {
    devices: Arc<Mutex<Vec<(String, SharedDevice)>>>,
}

impl SimBackend {
    fn device(&self, name: &str) -> SharedDevice {
        let mut devs = self.devices.lock().unwrap();
        if let Some((_, d)) = devs.iter().find(|(n, _)| n == name) {
            return d.clone();
        }
        let idx: usize = name
            .strip_prefix(SIM_PREFIX)
            .and_then(|s| s.parse().ok())
            .unwrap_or(devs.len());
        let d = Arc::new(Mutex::new(SimDevice::new(idx)));
        devs.push((name.to_string(), d.clone()));
        d
    }
}

impl PortBackend for SimBackend {
    fn open(&self, name: &str, _baud: u32) -> io::Result<Box<dyn Link>> {
        if !is_sim_port(name) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "not a simulated port",
            ));
        }
        Ok(Box::new(SimLink(self.device(name))))
    }

    fn present(&self, name: &str) -> bool {
        is_sim_port(name)
    }
}

/// [`PortBackend`] that serves `sim:*` ports from a [`SimBackend`] and
/// everything else from the real serial ports.
#[derive(Default, Clone)]
pub struct MixedBackend {
    sim: SimBackend,
}

impl PortBackend for MixedBackend {
    fn open(&self, name: &str, baud: u32) -> io::Result<Box<dyn Link>> {
        if is_sim_port(name) {
            self.sim.open(name, baud)
        } else {
            crate::reader::SerialBackend.open(name, baud)
        }
    }

    fn present(&self, name: &str) -> bool {
        if is_sim_port(name) {
            true
        } else {
            crate::reader::SerialBackend.present(name)
        }
    }
}

struct SimLink(Arc<Mutex<SimDevice>>);

impl Read for SimLink {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        {
            let mut d = self.0.lock().unwrap();
            d.tick();
            if !d.out.is_empty() {
                let n = buf.len().min(d.out.len());
                for (b, v) in buf.iter_mut().zip(d.out.drain(..n)) {
                    *b = v;
                }
                return Ok(n);
            }
        }
        // Behave like a serial port with a read timeout.
        std::thread::sleep(Duration::from_millis(5));
        Err(io::Error::new(io::ErrorKind::TimedOut, "sim: no data"))
    }
}

impl Write for SimLink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().input(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Link for SimLink {
    fn pulse_reset(&mut self) {
        let mut d = self.0.lock().unwrap();
        d.collecting = false;
        d.out.clear();
        d.banner();
    }
}

/// One simulated board.
struct SimDevice {
    chip: ChipFamily,
    mac: [u8; 6],
    /// The transmitter it "hears".
    peer: [u8; 6],
    out: VecDeque<u8>,
    line: Vec<u8>,
    log_mode: LogMode,
    channel: u8,
    periodicity_hz: u32,
    collecting: bool,
    run_end: Option<Instant>,
    next_frame: Instant,
    session_id: u32,
    pending_session: Option<(u32, Option<u64>)>,
    epoch: Option<u64>,
    stream_seq: u32,
    frame_seq: u16,
    boot: Instant,
}

impl SimDevice {
    fn new(idx: usize) -> Self {
        let i = idx as u8;
        Self {
            chip: CHIPS[idx % CHIPS.len()],
            mac: [0x5A, 0x1E, 0x00, 0x00, 0x00, i],
            peer: [0x5A, 0x1E, 0xFF, 0x00, 0x00, i],
            out: VecDeque::new(),
            line: Vec::new(),
            // Firmware boots in text; the GUI switches it.
            log_mode: LogMode::Text,
            channel: 6,
            periodicity_hz: 100,
            collecting: false,
            run_end: None,
            next_frame: Instant::now(),
            session_id: 0x51A0_0000 | idx as u32,
            pending_session: None,
            epoch: None,
            stream_seq: 0,
            frame_seq: 0,
            boot: Instant::now(),
        }
    }

    fn print(&mut self, s: &str) {
        self.out.extend(s.as_bytes());
    }

    fn banner(&mut self) {
        let mac = fmt_mac(&self.mac);
        self.print(&format!("ESP-CSI-CLI/{SIM_VERSION}\r\nmac={mac}\r\n> "));
    }

    fn input(&mut self, bytes: &[u8]) {
        for &b in bytes {
            if self.collecting {
                // Only `q` is honoured while collecting.
                if b == b'q' || b == b'Q' {
                    self.stop("\r\nStopping...\r\n");
                }
                continue;
            }
            match b {
                b'\r' | b'\n' => {
                    let line = String::from_utf8_lossy(&self.line).trim().to_string();
                    self.line.clear();
                    if !line.is_empty() {
                        self.command(&line);
                    }
                }
                _ => self.line.push(b),
            }
        }
    }

    fn command(&mut self, line: &str) {
        self.print(&format!("{line}\r\n"));
        let mut words = line.split_whitespace();
        let cmd = words.next().unwrap_or("");
        let arg = |name: &str| {
            line.split_whitespace()
                .find_map(|w| w.strip_prefix(&format!("--{name}=")).map(str::to_string))
        };
        match cmd {
            "info" => {
                let mac = fmt_mac(&self.mac);
                let chip = self.chip.label();
                self.print(&format!(
                    "ESP-CSI-CLI/{SIM_VERSION}\r\nname=esp-csi-cli-rs\r\nversion={SIM_VERSION}\r\n\
                     chip={chip}\r\nprotocol=4\r\nmac={mac}\r\nlog=text\r\ntransport=sim\r\n\
                     baud=115200\r\nfeatures=statistics\r\nEND-INFO\r\n"
                ));
            }
            "set-log-mode" => {
                self.log_mode = match arg("mode").as_deref() {
                    Some("serialized") => LogMode::Serialized,
                    Some("array-list") => LogMode::ArrayList,
                    Some("esp-csi-tool") => LogMode::EspCsiTool,
                    _ => LogMode::Text,
                };
            }
            "set-wifi" => {
                if let Some(ch) = arg("set-channel").and_then(|c| c.parse().ok()) {
                    self.channel = ch;
                }
            }
            "set-measurement" => {
                if let Some(hz) = arg("periodicity-hz").and_then(|c| c.parse().ok()) {
                    self.periodicity_hz = hz;
                }
            }
            "set-session" => {
                let id = arg("id").and_then(|v| parse_u32(&v));
                let epoch = arg("epoch").and_then(|v| v.parse().ok());
                if let Some(id) = id {
                    self.pending_session = Some((id, epoch));
                }
            }
            "start" => {
                let dur: Option<u64> = arg("duration").and_then(|v| v.parse().ok());
                match dur {
                    Some(d) => self.print(&format!("Starting CSI collection for {d}s...\r\n")),
                    None => self.print("Starting CSI collection indefinitely...\r\n"),
                }
                self.collecting = true;
                self.run_end = dur.map(|d| Instant::now() + Duration::from_secs(d));
                self.next_frame = Instant::now();
                self.stream_seq = 0;
                match self.pending_session.take() {
                    Some((id, epoch)) => {
                        self.session_id = id;
                        self.epoch = epoch;
                    }
                    None => {
                        self.session_id =
                            self.session_id.wrapping_mul(2_654_435_761).wrapping_add(1);
                        self.epoch = None;
                    }
                }
                if self.log_mode == LogMode::Serialized {
                    let info = SessionInfo::new(
                        wire_chip(self.chip),
                        [0, 13, 0],
                        self.now_us(),
                        self.epoch,
                    );
                    self.emit_wire(Body::Session(info));
                }
            }
            "restart" => {
                self.print("Restarting...\r\n");
                self.banner();
                return;
            }
            "help" | "show-config" | "show-stats" | "reset-config" | "version" => {}
            c if c.starts_with("set-") => {}
            _ => self.print(&format!("Invalid command: {cmd}\r\n")),
        }
        self.print("> ");
    }

    fn stop(&mut self, prefix: &str) {
        self.collecting = false;
        self.run_end = None;
        self.print(prefix);
        self.print("\r\x1b[2KCollection complete.\r\n> ");
    }

    fn now_us(&self) -> u64 {
        self.boot.elapsed().as_micros() as u64
    }

    /// Emit whatever frames are due.
    fn tick(&mut self) {
        if !self.collecting {
            return;
        }
        let now = Instant::now();
        if self.run_end.is_some_and(|end| now >= end) {
            self.stop("");
            return;
        }
        let period =
            Duration::from_micros(1_000_000 / u64::from(self.periodicity_hz.clamp(1, 1000)));
        // Cap the catch-up so a long stall doesn't flood the reader.
        let mut budget = 32;
        while self.next_frame <= now && budget > 0 {
            self.next_frame += period;
            budget -= 1;
            self.emit_measurement();
        }
        if budget == 0 {
            self.next_frame = now + period;
        }
    }

    /// The simulated channel: a smooth per-subcarrier response with a slow
    /// "breathing" ripple and a moving notch, distinct per device.
    fn iq(&self, n_sc: usize) -> Vec<i8> {
        let t = self.boot.elapsed().as_secs_f32();
        let dev = self.mac[5] as f32;
        let mut out = Vec::with_capacity(n_sc * 2);
        for k in 0..n_sc {
            let x = k as f32 / n_sc as f32;
            let notch =
                1.0 - 0.6 * (-((x - 0.5 - 0.3 * (t * 0.4 + dev).sin()) * 12.0).powi(2)).exp();
            let amp =
                (18.0 + 8.0 * (x * 6.0 + dev).sin() + 3.0 * (t * 1.7 + x * 3.0).sin()) * notch;
            let phase = x * 9.0 + t * 0.8 + dev;
            // Firmware order: imaginary, then real.
            out.push((amp * phase.sin()).round().clamp(-127.0, 127.0) as i8);
            out.push((amp * phase.cos()).round().clamp(-127.0, 127.0) as i8);
        }
        out
    }

    fn emit_measurement(&mut self) {
        self.frame_seq = self.frame_seq.wrapping_add(1);
        let t = self.boot.elapsed().as_secs_f32();
        let rssi = (-48.0 - 6.0 * (t * 0.3 + self.mac[5] as f32).sin()) as i8;
        let (layout, n_sc, vendor) = if self.chip.is_c5c6() {
            (
                if self.chip == ChipFamily::Esp32C5 {
                    LayoutId::C5Ht20None
                } else {
                    LayoutId::Unknown
                },
                57,
                VendorRx::EspHe {
                    rate: 0,
                    cur_bb_format: 1,
                    estimate_valid: true,
                    estimate_len: 114,
                    dump_len: 114,
                    is_group: false,
                    rxend_state: 0,
                    rxmatch: 0,
                    he_siga1: 0,
                    he_siga2: 0,
                    sigb_len: 0,
                    single_mpdu: true,
                },
            )
        } else {
            (
                LayoutId::ClassicNoneHt20,
                128,
                VendorRx::EspClassic {
                    rate: 0x80,
                    sig_mode: 1,
                    smoothing: true,
                    fec_ldpc: false,
                    ampdu_cnt: 0,
                },
            )
        };
        let bytes = self.iq(n_sc);
        match self.log_mode {
            LogMode::Serialized => {
                let meta = RxMeta {
                    timestamp_us: self.now_us(),
                    rssi,
                    noise_floor: -95,
                    channel: self.channel,
                    secondary: Secondary::None,
                    bandwidth: Some(Bandwidth::Mhz20),
                    ppdu: PpduFormat::Ht,
                    mcs: Some(0),
                    stbc: Some(false),
                    sgi: Some(false),
                    n_rx: 1,
                    n_ss: Some(1),
                    antenna: Some(0),
                    sig_len: 100,
                    rx_state: 0,
                    not_sounding: Some(false),
                    aggregation: Some(false),
                    frame_seq: Some(self.frame_seq),
                    vendor,
                };
                let payload = CsiPayload::EspRaw {
                    chip: wire_chip(self.chip),
                    layout,
                    first_word_invalid: false,
                    bytes: heapless::Vec::from_slice(&bytes).unwrap_or_default(),
                };
                let frame = CsiFrame::new(meta, Stimulus::Ambient { ta: self.peer }, None, payload);
                self.emit_wire(Body::Csi(frame));
            }
            LogMode::ArrayList => {
                let vals: Vec<String> = bytes.iter().map(|v| v.to_string()).collect();
                let ts = (self.now_us() & 0xFFFF_FFFF) as u32;
                let line = format!(
                    "[{},{rssi},11,-95,{},{ts},100,0,0,0,0,0,1,0,0,1,0,0,0,0,100,{},[{}],{}]\r\n",
                    self.frame_seq,
                    self.channel,
                    bytes.len(),
                    vals.join(","),
                    fmt_mac(&self.peer).to_ascii_lowercase(),
                );
                self.print(&line);
            }
            // Text and esp-csi-tool aren't simulated; a real host should use
            // serialized anyway.
            LogMode::Text | LogMode::EspCsiTool => {}
        }
    }

    fn emit_wire(&mut self, body: Body) {
        let env = Envelope::new(
            self.mac,
            self.session_id,
            SourceKind::EspVendor,
            self.stream_seq,
        );
        self.stream_seq = self.stream_seq.wrapping_add(1);
        let mut buf = vec![0u8; wire::MAX_ENCODED_LEN];
        if let Ok(enc) = wire::encode_cobs(&env, &body, &mut buf) {
            // `to_slice_cobs` already ends the frame with its `\0`.
            let enc = enc.to_vec();
            self.out.extend(enc);
        }
    }
}

fn wire_chip(c: ChipFamily) -> wire::Chip {
    match c {
        ChipFamily::Esp32C3 => wire::Chip::Esp32C3,
        ChipFamily::Esp32C5 => wire::Chip::Esp32C5,
        ChipFamily::Esp32C6 => wire::Chip::Esp32C6,
        ChipFamily::Esp32S3 => wire::Chip::Esp32S3,
        ChipFamily::Esp32 | ChipFamily::Unknown => wire::Chip::Esp32,
    }
}

fn fmt_mac(b: &[u8; 6]) -> String {
    format!(
        "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
        b[0], b[1], b[2], b[3], b[4], b[5]
    )
}

fn parse_u32(s: &str) -> Option<u32> {
    match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(hex) => u32::from_str_radix(hex, 16).ok(),
        None => s.parse().ok(),
    }
}
