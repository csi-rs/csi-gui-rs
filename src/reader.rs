//! Serial reader thread.
//!
//! Owns the [`serialport`] handle, performs the DTR/RTS hardware reset,
//! identifies the firmware via the `info` command, and decodes incoming
//! CSI packets according to the current [`LogMode`]. All decoded packets
//! flow out through a bounded `crossbeam_channel::Receiver<CsiData>`.
//!
//! Most callers want [`spawn_reader`] — it returns a [`ReaderHandles`]
//! bundle with the packet receiver, command sender, connection-state
//! receiver, and device-info receiver.
//!
//! See [`crate::shared::EspSharedState`] for the lock-free state shared
//! between this thread and the UI thread.
//!
//! [`LogMode`]: crate::protocol::config::LogMode

use crate::console::RawConsole;
use crate::data::CsiData;
use crate::protocol::command::{render_apply_config, render_start, EspCommand};
use crate::protocol::config::LogMode;
use crate::protocol::info::{ChipFamily, DeviceInfo};
use crate::protocol::parse::{parse_array_list, parse_esp_csi_tool, TextParser};
use crate::protocol::serialized::decode_cobs_packet;
use crate::shared::EspSharedState;
use crossbeam_channel::{Receiver, Sender, TrySendError};
use serialport::SerialPort;
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Capacity of the bounded `packets` channel between the reader thread and
/// the UI. Oldest packets are silently dropped when the receiver lags.
pub const PACKET_CHANNEL_CAP: usize = 4096;

/// Maximum packets drained from the serial port in one batch before yielding.
pub const READER_BATCH: usize = 64;

/// Soft cap on contiguous packet decoding within a single reader-loop pass.
pub const YIELD_EVERY: usize = 256;

/// Current state of the connection to the ESP32, as seen by the reader.
///
/// Travels from the reader to the UI through the [`ReaderHandles::state`]
/// channel. Drives the status-bar label and the firmware-not-recognized
/// dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnState {
    /// No serial port open.
    Disconnected,
    /// DTR/RTS is being toggled to reset the chip.
    Resetting,
    /// Waiting up to 6 s for the firmware welcome banner.
    AwaitingBanner,
    /// Banner seen; the `info` command has been sent and we're parsing the
    /// response.
    Identifying,
    /// No `ESP-CSI-CLI/<version>` banner ever arrived. Firmware is either
    /// missing, on a different port, or boot was interrupted.
    UnknownFirmware,
    /// `info` reported a `protocol=` the GUI doesn't understand.
    UnsupportedProtocol(u32),
    /// Identified, streaming packets.
    Streaming,
    /// Open / read failure.
    Error(String),
}

/// Channel bundle returned by [`spawn_reader`]. Keep the bundle alive for as
/// long as you want the reader thread to live; dropping it leaks the thread
/// (sender disconnects but the thread blocks on `read_line` forever).
///
/// For a clean shutdown, send [`EspCommand::Shutdown`] before dropping.
pub struct ReaderHandles {
    /// Send commands to the reader (open port, start/stop, apply config…).
    pub commands: Sender<EspCommand>,
    /// Drain decoded CSI packets here. Bounded; oldest dropped on overflow.
    pub packets: Receiver<CsiData>,
    /// Connection-state transitions, one per change.
    pub state: Receiver<ConnState>,
    /// `DeviceInfo` is emitted once per successful identification.
    pub info: Receiver<DeviceInfo>,
    /// Thread handle. Take it with `handle.join.take().unwrap().join()` on
    /// shutdown if you want to wait for the reader to exit cleanly.
    pub join: Option<JoinHandle<()>>,
}

/// Spawn the reader thread.
///
/// `shared` holds the lock-free state (flags + counters) that both the
/// reader and the UI mutate. `console` is a rolling-line text buffer the
/// reader writes raw UART text into, used by the Console view.
///
/// # Example
///
/// ```no_run
/// # use std::sync::Arc;
/// # use csi_gui_rs::console::RawConsole;
/// # use csi_gui_rs::reader::spawn_reader;
/// # use csi_gui_rs::shared::EspSharedState;
/// let shared = EspSharedState::handle();
/// let console = Arc::new(RawConsole::new(2_000));
/// let handles = spawn_reader(shared, console);
/// // ... use handles.packets, handles.state, etc. ...
/// ```
pub fn spawn_reader(shared: Arc<EspSharedState>, console: Arc<RawConsole>) -> ReaderHandles {
    let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded::<EspCommand>();
    let (pkt_tx, pkt_rx) = crossbeam_channel::bounded::<CsiData>(PACKET_CHANNEL_CAP);
    let (state_tx, state_rx) = crossbeam_channel::unbounded::<ConnState>();
    let (info_tx, info_rx) = crossbeam_channel::unbounded::<DeviceInfo>();

    let join = std::thread::Builder::new()
        .name("csi-reader".into())
        .spawn(move || reader_main(shared, console, cmd_rx, pkt_tx, state_tx, info_tx))
        .expect("spawn reader thread");

    ReaderHandles {
        commands: cmd_tx,
        packets: pkt_rx,
        state: state_rx,
        info: info_rx,
        join: Some(join),
    }
}

fn reader_main(
    shared: Arc<EspSharedState>,
    console: Arc<RawConsole>,
    cmd_rx: Receiver<EspCommand>,
    pkt_tx: Sender<CsiData>,
    state_tx: Sender<ConnState>,
    info_tx: Sender<DeviceInfo>,
) {
    let mut state = Ctx {
        shared,
        console,
        port_name: None,
        port: None,
        device: None,
        chip: ChipFamily::Unknown,
        text: TextParser::default(),
        cobs_accum: Vec::with_capacity(8192),
        connection: ConnState::Disconnected,
        state_tx,
        info_tx,
        pkt_tx,
    };

    let mut last_command_check = Instant::now();
    let mut last_presence_check = Instant::now();

    loop {
        // 1) Process commands first.
        while let Ok(cmd) = cmd_rx.try_recv() {
            if matches!(cmd, EspCommand::Shutdown) {
                return;
            }
            state.handle_command(cmd);
            last_command_check = Instant::now();
        }

        // 2) If not connected, try to (re)open.
        if state.port.is_none() {
            if state.port_name.is_some() {
                state.try_open();
            } else {
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
        }

        // 3) Honor reset request.
        if state.shared.should_reset.swap(false, Ordering::AcqRel) {
            state.reset_and_identify();
        }

        // 4) Honor start request.
        if state.shared.should_start.swap(false, Ordering::AcqRel) {
            state.send_line("start");
            state.shared.is_collecting.store(true, Ordering::Release);
        }

        // 5) Drain serial frames.
        let mut produced = 0usize;
        let mut io_err = false;
        for _ in 0..READER_BATCH {
            match state.read_one_frame() {
                ReadOutcome::Frame => {
                    produced += 1;
                    if produced >= YIELD_EVERY {
                        break;
                    }
                }
                ReadOutcome::Idle => break,
                ReadOutcome::IoError => {
                    io_err = true;
                    break;
                }
            }
        }

        // 6) Hard-disconnect detection. An IO error from a read is the fast
        //    signal; a periodic check against the OS port list is the
        //    fallback when the driver returns timeouts instead of errors.
        if state.port.is_some()
            && (io_err || last_presence_check.elapsed() > Duration::from_millis(1_000))
        {
            last_presence_check = Instant::now();
            if let Some(name) = state.port_name.as_ref() {
                let present = enumerate_ports().iter().any(|p| p == name);
                if !present {
                    state.handle_disconnect();
                }
            }
        }

        if produced == 0 && last_command_check.elapsed() > Duration::from_millis(20) {
            std::thread::sleep(Duration::from_millis(5));
            last_command_check = Instant::now();
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum ReadOutcome {
    /// A frame was produced (or a non-empty serialized byte was consumed).
    Frame,
    /// Read timed out / no data — try again later.
    Idle,
    /// Underlying read returned an error that isn't a timeout. Likely a hard
    /// disconnect.
    IoError,
}

struct Ctx {
    shared: Arc<EspSharedState>,
    console: Arc<RawConsole>,
    port_name: Option<String>,
    port: Option<BufReader<Box<dyn SerialPort>>>,
    device: Option<DeviceInfo>,
    chip: ChipFamily,
    text: TextParser,
    cobs_accum: Vec<u8>,
    connection: ConnState,
    state_tx: Sender<ConnState>,
    info_tx: Sender<DeviceInfo>,
    pkt_tx: Sender<CsiData>,
}

impl Ctx {
    fn set_state(&mut self, s: ConnState) {
        if self.connection != s {
            self.connection = s.clone();
            self.shared
                .connected
                .store(matches!(s, ConnState::Streaming), Ordering::Release);
            let _ = self.state_tx.send(s);
        }
    }

    fn handle_command(&mut self, cmd: EspCommand) {
        match cmd {
            EspCommand::OpenPort(p) => {
                self.port_name = Some(p);
                self.port = None;
            }
            EspCommand::Reset => {
                self.shared.should_reset.store(true, Ordering::Release);
            }
            EspCommand::Start { duration_sec } => {
                // Reset parser/counter state BEFORE sending `start`. The
                // previous Stop may have left bytes in the serial input
                // and cobs_accum / TextParser may carry partial framing.
                // Without this, the first session after a restart loses
                // packets (or all of them, if the firmware never emits a
                // 0x00 sentinel before a new full COBS frame arrives).
                self.shared.reset_counters();
                self.drain_echo();
                self.text.reset();
                self.cobs_accum.clear();
                self.send_line(&render_start(duration_sec));
                self.shared.is_collecting.store(true, Ordering::Release);
                self.shared.bump_io_generation();
                // Give the firmware a moment to echo the command + emit
                // any header lines, then drain them so they don't get
                // interpreted as the first CSI packet.
                std::thread::sleep(Duration::from_millis(20));
                self.drain_echo();
                self.text.reset();
                self.cobs_accum.clear();
            }
            EspCommand::Stop => {
                self.send_byte(b'q');
                self.shared.is_collecting.store(false, Ordering::Release);
                // Drain the trailing CSI packet + any post-stop firmware
                // output ("DONE-COLLECTION", CLI prompt, etc.) so a
                // subsequent Start sees an empty pipe.
                std::thread::sleep(Duration::from_millis(50));
                self.drain_echo();
                self.text.reset();
                self.cobs_accum.clear();
                self.shared.reset_counters();
                self.shared.bump_io_generation();
            }
            EspCommand::ApplyConfig { cfg, chip } => {
                if self.shared.is_collecting.load(Ordering::Acquire) {
                    log::warn!("ApplyConfig ignored: collection in progress");
                    return;
                }
                let cmds = render_apply_config(&cfg, chip);
                for (i, line) in cmds.iter().enumerate() {
                    self.send_line(line);
                    let is_log_mode = line.starts_with("set-log-mode");
                    let pause = if is_log_mode { 50 } else { 20 };
                    std::thread::sleep(Duration::from_millis(pause));
                    self.drain_echo();
                    if is_log_mode {
                        self.shared
                            .log_mode_id
                            .store(cfg.log_mode.id(), Ordering::Release);
                        self.shared.bump_io_generation();
                        self.text.reset();
                        self.cobs_accum.clear();
                    }
                    let _ = i;
                }
            }
            EspCommand::Raw(s) => {
                // Firmware CLI is locked while collecting (only `q` /
                // `Q` is honoured); anything else is silently discarded
                // on the device. Drop the command here too so it
                // doesn't clog the wire — the UI already greys out
                // raw-command buttons during a session, but a stale
                // command queued from before Start could still fire.
                if self.shared.is_collecting.load(Ordering::Acquire) {
                    log::debug!("raw command ignored while collecting: {s}");
                } else {
                    self.send_line(&s);
                }
            }
            EspCommand::Shutdown => {}
        }
    }

    fn try_open(&mut self) {
        let Some(name) = self.port_name.clone() else {
            return;
        };
        let baud = self.shared.baud.load(Ordering::Relaxed);
        self.set_state(ConnState::Resetting);
        match serialport::new(&name, baud)
            .timeout(Duration::from_millis(100))
            .open()
        {
            Ok(p) => {
                self.port = Some(BufReader::with_capacity(16 * 1024, p));
                self.reset_and_identify();
            }
            Err(e) => {
                self.set_state(ConnState::Error(format!("open {name}: {e}")));
                std::thread::sleep(Duration::from_millis(2000));
            }
        }
    }

    fn reset_and_identify(&mut self) {
        self.text.reset();
        self.cobs_accum.clear();
        if let Some(reader) = self.port.as_mut() {
            let port = reader.get_mut();
            // Hard-reset-to-firmware: RTS=True alone pulls EN low (assert reset),
            // DTR stays low so the 2-transistor auto-reset circuit doesn't drag
            // GPIO0 low and trap the chip in the download bootloader on release.
            let _ = port.write_data_terminal_ready(false);
            let _ = port.write_request_to_send(true);
            std::thread::sleep(Duration::from_millis(100));
            let _ = port.write_request_to_send(false);
        }
        self.set_state(ConnState::AwaitingBanner);
        if self.await_banner() {
            self.set_state(ConnState::Identifying);
            if let Some(info) = self.fetch_info() {
                self.chip = info.chip;
                if info.protocol_supported() {
                    let _ = self.info_tx.send(info.clone());
                    self.device = Some(info);
                    self.set_state(ConnState::Streaming);
                } else {
                    self.set_state(ConnState::UnsupportedProtocol(info.protocol));
                }
            } else {
                self.set_state(ConnState::Error("info parse failed".into()));
            }
        } else {
            self.set_state(ConnState::UnknownFirmware);
        }
    }

    /// Wait for `ESP-CSI-CLI/<version>` after reset. Uses raw `read()` rather
    /// than `read_line` so non-UTF-8 ROM-bootloader noise doesn't trip the
    /// UTF-8 check and silently roll back bytes. Also forwards each `\n`-
    /// terminated UTF-8 line to the RawConsole for visibility.
    fn await_banner(&mut self) -> bool {
        let deadline = Instant::now() + Duration::from_millis(6_000);
        let console = self.console.clone();
        let Some(reader) = self.port.as_mut() else {
            return false;
        };
        let needle: &[u8] = b"ESP-CSI-CLI/";
        let mut acc: Vec<u8> = Vec::with_capacity(4096);
        let mut buf = [0u8; 256];
        let mut line_start = 0usize;
        while Instant::now() < deadline {
            match reader.read(&mut buf) {
                Ok(0) => return false,
                Ok(n) => {
                    acc.extend_from_slice(&buf[..n]);
                    // Forward any newly completed lines (best-effort UTF-8).
                    while let Some(rel) = acc[line_start..].iter().position(|&b| b == b'\n') {
                        let end = line_start + rel;
                        if let Ok(s) = std::str::from_utf8(&acc[line_start..end]) {
                            console.push(s);
                        }
                        line_start = end + 1;
                    }
                    if banner_match(&acc, needle) {
                        return true;
                    }
                    if acc.len() > 16 * 1024 {
                        let drain = acc.len().saturating_sub(1024);
                        let drain = drain.min(line_start);
                        acc.drain(..drain);
                        line_start -= drain;
                    }
                }
                Err(_) => continue, // timeout / transient — retry until deadline
            }
        }
        false
    }

    fn fetch_info(&mut self) -> Option<DeviceInfo> {
        self.send_line("info");
        let console = self.console.clone();
        let Some(reader) = self.port.as_mut() else {
            return None;
        };
        let mut acc = String::new();
        let deadline = Instant::now() + Duration::from_millis(2_000);
        let mut line = String::new();
        while Instant::now() < deadline {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    let trimmed = line.trim_end_matches(['\r', '\n']);
                    if !trimmed.is_empty() {
                        console.push(trimmed);
                    }
                    if trimmed == "END-INFO" {
                        return Some(DeviceInfo::parse(&acc));
                    }
                    acc.push_str(&line);
                }
                Err(_) => continue,
            }
        }
        None
    }

    fn drain_echo(&mut self) {
        let Some(reader) = self.port.as_mut() else { return; };
        // Consume whatever is buffered inside the BufReader first — these
        // bytes have already been read off the port and `port.read()`
        // would not see them.
        let buffered = reader.buffer().len();
        if buffered > 0 {
            reader.consume(buffered);
        }
        let port = reader.get_mut();
        let mut tmp = [0u8; 256];
        let _ = port.set_timeout(Duration::from_millis(10));
        loop {
            match port.read(&mut tmp) {
                Ok(0) | Err(_) => break,
                Ok(_) => continue,
            }
        }
        let _ = port.set_timeout(Duration::from_millis(100));
    }

    fn send_line(&mut self, line: &str) {
        self.console.push(format!("» {line}"));
        let Some(reader) = self.port.as_mut() else { return; };
        let port = reader.get_mut();
        let mut buf = line.as_bytes().to_vec();
        buf.extend_from_slice(b"\r\n");
        let _ = port.write_all(&buf);
        let _ = port.flush();
    }

    fn send_byte(&mut self, b: u8) {
        let Some(reader) = self.port.as_mut() else { return; };
        let port = reader.get_mut();
        let _ = port.write_all(&[b]);
        let _ = port.flush();
    }

    fn handle_disconnect(&mut self) {
        self.port = None;
        self.device = None;
        self.text.reset();
        self.cobs_accum.clear();
        self.shared.is_collecting.store(false, Ordering::Release);
        self.set_state(ConnState::Disconnected);
    }

    /// Read one packet (line, CSV, or COBS-framed blob) per current log mode.
    fn read_one_frame(&mut self) -> ReadOutcome {
        let mode = match self.shared.log_mode_id.load(Ordering::Acquire) {
            0 => LogMode::Serialized,
            1 => LogMode::Text,
            2 => LogMode::ArrayList,
            3 => LogMode::EspCsiTool,
            _ => LogMode::Text,
        };

        let Some(reader) = self.port.as_mut() else {
            return ReadOutcome::Idle;
        };

        match mode {
            LogMode::Serialized => {
                let mut byte = [0u8; 1];
                match reader.read_exact(&mut byte) {
                    Ok(()) => {}
                    Err(e) => return classify_read_err(&e),
                }
                if byte[0] == 0 {
                    let pkt = if let Some(p) = decode_cobs_packet(&self.cobs_accum) {
                        p
                    } else {
                        self.shared
                            .frames_decode_failed
                            .fetch_add(1, Ordering::Relaxed);
                        CsiData::default()
                    };
                    self.shared.frames_total.fetch_add(1, Ordering::Relaxed);
                    let _ = try_push(&self.pkt_tx, pkt);
                    self.cobs_accum.clear();
                    ReadOutcome::Frame
                } else {
                    self.cobs_accum.push(byte[0]);
                    if self.cobs_accum.len() > 16 * 1024 {
                        self.cobs_accum.clear();
                    }
                    ReadOutcome::Frame
                }
            }
            LogMode::Text => {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => ReadOutcome::Idle,
                    Ok(_) => {
                        self.console.push(line.trim_end_matches(['\r', '\n']));
                        if let Some(pkt) = self.text.feed_line(&line) {
                            self.shared.frames_total.fetch_add(1, Ordering::Relaxed);
                            if !pkt.is_decoded() {
                                self.shared
                                    .frames_decode_failed
                                    .fetch_add(1, Ordering::Relaxed);
                            }
                            let _ = try_push(&self.pkt_tx, pkt);
                            ReadOutcome::Frame
                        } else {
                            ReadOutcome::Idle
                        }
                    }
                    Err(e) => classify_read_err(&e),
                }
            }
            LogMode::EspCsiTool => {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => ReadOutcome::Idle,
                    Ok(_) => {
                        self.console.push(line.trim_end_matches(['\r', '\n']));
                        self.shared.frames_total.fetch_add(1, Ordering::Relaxed);
                        if let Some(pkt) = parse_esp_csi_tool(&line) {
                            let _ = try_push(&self.pkt_tx, pkt);
                            ReadOutcome::Frame
                        } else {
                            self.shared
                                .frames_decode_failed
                                .fetch_add(1, Ordering::Relaxed);
                            ReadOutcome::Frame
                        }
                    }
                    Err(e) => classify_read_err(&e),
                }
            }
            LogMode::ArrayList => {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => ReadOutcome::Idle,
                    Ok(_) => {
                        self.console.push(line.trim_end_matches(['\r', '\n']));
                        self.shared.frames_total.fetch_add(1, Ordering::Relaxed);
                        let chip = self.chip;
                        let pkt = parse_array_list(&line, chip).unwrap_or_else(|| {
                            self.shared
                                .frames_decode_failed
                                .fetch_add(1, Ordering::Relaxed);
                            CsiData::default()
                        });
                        let _ = try_push(&self.pkt_tx, pkt);
                        ReadOutcome::Frame
                    }
                    Err(e) => classify_read_err(&e),
                }
            }
        }
    }
}

fn classify_read_err(e: &std::io::Error) -> ReadOutcome {
    use std::io::ErrorKind::*;
    match e.kind() {
        // Transient — normal idle behavior with our 100ms read timeout.
        TimedOut | Interrupted | WouldBlock | InvalidData => ReadOutcome::Idle,
        _ => ReadOutcome::IoError,
    }
}

/// Bounded channel: drop oldest by skipping `Full` rather than blocking.
fn try_push(tx: &Sender<CsiData>, p: CsiData) -> Result<(), ()> {
    match tx.try_send(p) {
        Ok(_) => Ok(()),
        Err(TrySendError::Full(_)) => Ok(()),
        Err(TrySendError::Disconnected(_)) => Err(()),
    }
}

/// List the serial port names currently visible to the OS.
///
/// Returns paths like `/dev/ttyUSB0` on Linux, `COM3` on Windows, or
/// `/dev/cu.usbserial-…` on macOS. Used both at startup for auto-port pick
/// and periodically to detect hot-unplugs.
pub fn enumerate_ports() -> Vec<String> {
    serialport::available_ports()
        .map(|ps| ps.into_iter().map(|p| p.port_name).collect())
        .unwrap_or_default()
}

/// Look for `needle` anywhere in `acc` such that it sits at the start of a
/// line (acc start or preceded by `\n`) and is followed by ≥1 non-whitespace
/// byte then a line terminator. Equivalent to the old `^ESP-CSI-CLI/\S+$`
/// regex but without UTF-8 validation, so we can match through binary
/// boot-loader garbage that read_line would have errored on.
fn banner_match(acc: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || acc.len() < needle.len() {
        return false;
    }
    let mut i = 0usize;
    while i + needle.len() <= acc.len() {
        let Some(rel) = acc[i..].windows(needle.len()).position(|w| w == needle) else {
            return false;
        };
        let pos = i + rel;
        let at_line_start = pos == 0 || acc[pos - 1] == b'\n';
        if at_line_start {
            let after = &acc[pos + needle.len()..];
            if let Some(end_rel) = after.iter().position(|&b| b == b'\n' || b == b'\r') {
                let version = &after[..end_rel];
                if !version.is_empty()
                    && version.iter().all(|&b| !b.is_ascii_whitespace())
                {
                    return true;
                }
            }
            // Line not terminated yet — wait for the rest in a later read.
        }
        i = pos + 1;
    }
    false
}

