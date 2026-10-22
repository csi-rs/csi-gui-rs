//! Serial reader thread — one per device.
//!
//! Owns the port, identifies the firmware via `info`, and decodes incoming
//! CSI according to the current [`LogMode`]. Decoded packets flow out
//! through a bounded channel; connection changes, the device's identity
//! and run announcements flow out as [`ReaderEvent`]s.
//!
//! Most callers want [`spawn_reader`], which returns a [`ReaderHandles`]
//! bundle. [`spawn_reader_with`] takes a [`PortBackend`] instead of the
//! real serial ports, which is how the tests drive the reader without
//! hardware.
//!
//! ## Identification
//!
//! Boards reach the host two ways: through a USB-UART bridge (CP210x,
//! CH340), where DTR/RTS reset the chip, and through the chip's own
//! USB-Serial-JTAG (S3, C3, C5, C6), where opening the port resets
//! nothing and the firmware may still be streaming from an earlier run.
//! So the reader first *asks*: `q` to stop any run, a short drain, then
//! `info`. Only if that gets no answer does it pulse RTS and wait for the
//! `ESP-CSI-CLI/<version>` banner, then ask again.
//!
//! [`LogMode`]: crate::protocol::config::LogMode

use crate::console::RawConsole;
use crate::data::CsiData;
use crate::protocol::command::{
    EspCommand, render_apply_config, render_restart, render_set_session, render_start,
};
use crate::protocol::config::LogMode;
use crate::protocol::info::{ChipFamily, DeviceInfo};
use crate::protocol::parse::{TextParser, parse_array_list, parse_esp_csi_tool};
use crate::protocol::serialized::{DecodedFrame, FrameError, SessionAnnounce, decode_frame};
use crate::shared::EspSharedState;
use crossbeam_channel::{Receiver, Sender, TrySendError};
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Capacity of the bounded `packets` channel between the reader thread and
/// the UI. New packets are dropped (and counted) when the receiver lags.
pub const PACKET_CHANNEL_CAP: usize = 4096;

/// Maximum frames decoded in one pass before the reader checks for commands.
pub const READER_BATCH: usize = 256;

/// Largest all-printable chunk the serialized decoder treats as firmware
/// text rather than a binary frame. Bigger than a single log line because
/// the CLI's command echoes before a run aren't `\0`-bracketed and arrive
/// as one chunk.
const MAX_TEXT_CHUNK: usize = 4096;

/// What the firmware prints when a run ends, by `q` or by `--duration`.
const COLLECTION_COMPLETE: &[u8] = b"Collection complete";

/// Current state of the connection to the ESP32, as seen by the reader.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnState {
    /// No port open.
    Disconnected,
    /// Opening the port, or retrying after it went away.
    Opening,
    /// Asking the firmware who it is (`info`, and a reset if it stays quiet).
    Identifying,
    /// Nothing that answered like esp-csi-cli-rs: other firmware, a
    /// bootloader, or a board that didn't come out of reset.
    UnknownFirmware,
    /// esp-csi-cli-rs, but a build this GUI can't drive. The text says why.
    Incompatible(String),
    /// Identified; packets flow while collecting.
    Streaming,
    /// Open / read failure.
    Error(String),
}

impl ConnState {
    /// `true` for the transient states on the way to [`ConnState::Streaming`].
    pub fn is_busy(&self) -> bool {
        matches!(self, Self::Opening | Self::Identifying)
    }

    /// `true` for the states that need the user to do something.
    pub fn is_problem(&self) -> bool {
        matches!(
            self,
            Self::UnknownFirmware | Self::Incompatible(_) | Self::Error(_)
        )
    }
}

/// Something the reader tells the UI about, besides packets.
#[derive(Clone, Debug)]
pub enum ReaderEvent {
    /// The connection changed state.
    State(ConnState),
    /// The device identified itself (sent on every successful identify).
    Info(DeviceInfo),
    /// A wire-v1 run announcement: the first frame after `start`.
    Session(SessionAnnounce),
    /// The firmware ended a run on its own (`start --duration=N` elapsed).
    CollectionEnded,
}

/// Channel bundle returned by [`spawn_reader`]. Send
/// [`EspCommand::Shutdown`] (or drop the [`crate::device::DeviceSession`]
/// that owns it) for a clean exit.
pub struct ReaderHandles {
    /// Send commands to the reader (open port, start/stop, apply config…).
    pub commands: Sender<EspCommand>,
    /// Drain decoded CSI packets here. Bounded; new packets are dropped on
    /// overflow and counted in [`EspSharedState::frames_dropped`].
    pub packets: Receiver<CsiData>,
    /// State changes, identity and run announcements, in order.
    pub events: Receiver<ReaderEvent>,
    /// Thread handle.
    pub join: Option<JoinHandle<()>>,
}

/// A byte stream to a device: the serial port, or a test double.
pub trait Link: Read + Write + Send {
    /// Pulse the chip's reset line (EN) without trapping it in the
    /// bootloader. A no-op where the link has no control lines.
    fn pulse_reset(&mut self) {}
}

impl Link for Box<dyn serialport::SerialPort> {
    fn pulse_reset(&mut self) {
        // RTS alone pulls EN low; DTR stays low so the two-transistor
        // auto-reset circuit doesn't drag GPIO0 low and trap the chip in
        // the download bootloader on release.
        let _ = self.write_data_terminal_ready(false);
        let _ = self.write_request_to_send(true);
        std::thread::sleep(Duration::from_millis(100));
        let _ = self.write_request_to_send(false);
    }
}

/// Opens [`Link`]s and says whether a port still exists. [`SerialBackend`]
/// is the real one.
pub trait PortBackend: Send + 'static {
    /// Open `name` at `baud`. Reads must time out (≈100 ms) rather than block.
    fn open(&self, name: &str, baud: u32) -> std::io::Result<Box<dyn Link>>;
    /// `false` once the port has disappeared (unplugged).
    fn present(&self, name: &str) -> bool;
}

/// The OS serial ports, via the `serialport` crate.
pub struct SerialBackend;

impl PortBackend for SerialBackend {
    fn open(&self, name: &str, baud: u32) -> std::io::Result<Box<dyn Link>> {
        let port = serialport::new(name, baud)
            .timeout(Duration::from_millis(100))
            .open()
            .map_err(std::io::Error::other)?;
        Ok(Box::new(port))
    }

    fn present(&self, name: &str) -> bool {
        enumerate_ports().iter().any(|p| p == name)
    }
}

/// Spawn a reader thread for the OS serial ports.
///
/// `shared` holds the lock-free flags and counters both threads touch;
/// `console` collects the device's text output for the Console view.
///
/// # Example
///
/// ```no_run
/// # use std::sync::Arc;
/// # use csi_gui_rs::console::RawConsole;
/// # use csi_gui_rs::reader::spawn_reader;
/// # use csi_gui_rs::shared::EspSharedState;
/// # use csi_gui_rs::protocol::EspCommand;
/// let handles = spawn_reader(EspSharedState::handle(), Arc::new(RawConsole::new(2_000)));
/// handles.commands.send(EspCommand::OpenPort("/dev/ttyACM0".into())).unwrap();
/// ```
pub fn spawn_reader(shared: Arc<EspSharedState>, console: Arc<RawConsole>) -> ReaderHandles {
    spawn_reader_with(SerialBackend, shared, console)
}

/// [`spawn_reader`] over any [`PortBackend`].
pub fn spawn_reader_with(
    backend: impl PortBackend,
    shared: Arc<EspSharedState>,
    console: Arc<RawConsole>,
) -> ReaderHandles {
    let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded::<EspCommand>();
    let (pkt_tx, pkt_rx) = crossbeam_channel::bounded::<CsiData>(PACKET_CHANNEL_CAP);
    let (ev_tx, ev_rx) = crossbeam_channel::unbounded::<ReaderEvent>();

    let join = std::thread::Builder::new()
        .name("csi-reader".into())
        .spawn(move || {
            let mut r = Reader {
                backend: Box::new(backend),
                shared,
                console,
                port_name: None,
                port: None,
                next_open: Instant::now(),
                open_failures: 0,
                chip: ChipFamily::Unknown,
                device_id: String::new(),
                text: TextParser::default(),
                frame: Vec::with_capacity(8192),
                last_seq: None,
                connection: ConnState::Disconnected,
                events: ev_tx,
                packets: pkt_tx,
            };
            r.run(cmd_rx);
        })
        .expect("spawn reader thread");

    ReaderHandles {
        commands: cmd_tx,
        packets: pkt_rx,
        events: ev_rx,
        join: Some(join),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReadOutcome {
    /// Something was consumed; read again.
    Progress,
    /// Nothing available within the read timeout.
    Idle,
    /// The link failed (unplugged, or the driver gave up).
    IoError,
}

struct Reader {
    backend: Box<dyn PortBackend>,
    shared: Arc<EspSharedState>,
    console: Arc<RawConsole>,
    port_name: Option<String>,
    port: Option<BufReader<Box<dyn Link>>>,
    /// Earliest time to try opening the port again after a failure.
    next_open: Instant,
    open_failures: u32,
    chip: ChipFamily,
    /// Stamped on every packet: the device's MAC, or the port name until
    /// the firmware has said which MAC it has.
    device_id: String,
    text: TextParser,
    /// Bytes of the serialized frame (or idle text line) being assembled.
    /// Survives read timeouts, so a frame split across two reads is kept.
    frame: Vec<u8>,
    /// Last wire-v1 `stream_seq` seen this run, for gap counting.
    last_seq: Option<u32>,
    connection: ConnState,
    events: Sender<ReaderEvent>,
    packets: Sender<CsiData>,
}

impl Reader {
    fn run(&mut self, cmd_rx: Receiver<EspCommand>) {
        let mut last_presence_check = Instant::now();
        loop {
            while let Ok(cmd) = cmd_rx.try_recv() {
                if matches!(cmd, EspCommand::Shutdown) {
                    return;
                }
                self.handle_command(cmd);
            }

            if self.port.is_none() {
                if self.port_name.is_some() && Instant::now() >= self.next_open {
                    self.try_open();
                } else {
                    // Short sleep: keep commands (Shutdown, a new OpenPort)
                    // responsive while waiting out the retry backoff.
                    std::thread::sleep(Duration::from_millis(50));
                }
                continue;
            }

            let mut io_err = false;
            let mut progressed = false;
            if self.connection == ConnState::Streaming {
                for _ in 0..READER_BATCH {
                    match self.read_one() {
                        ReadOutcome::Progress => progressed = true,
                        ReadOutcome::Idle => break,
                        ReadOutcome::IoError => {
                            io_err = true;
                            break;
                        }
                    }
                }
            } else {
                std::thread::sleep(Duration::from_millis(50));
            }

            // An IO error is the fast unplug signal; the periodic port-list
            // check catches drivers that only ever time out.
            if io_err || last_presence_check.elapsed() > Duration::from_secs(1) {
                last_presence_check = Instant::now();
                let gone = self
                    .port_name
                    .as_deref()
                    .is_some_and(|n| !self.backend.present(n));
                if io_err || gone {
                    self.handle_disconnect(io_err);
                }
            }

            if !progressed {
                std::thread::yield_now();
            }
        }
    }

    fn set_state(&mut self, s: ConnState) {
        if self.connection != s {
            self.connection = s.clone();
            self.shared
                .connected
                .store(matches!(s, ConnState::Streaming), Ordering::Release);
            let _ = self.events.send(ReaderEvent::State(s));
        }
    }

    fn collecting(&self) -> bool {
        self.shared.is_collecting.load(Ordering::Acquire)
    }

    fn reset_parsers(&mut self) {
        self.text.reset();
        self.frame.clear();
        self.last_seq = None;
    }

    fn handle_command(&mut self, cmd: EspCommand) {
        match cmd {
            EspCommand::OpenPort(p) => {
                self.port_name = Some(p.clone());
                self.port = None;
                self.next_open = Instant::now();
                self.open_failures = 0;
                self.device_id = p;
                self.shared.config_applied.store(false, Ordering::Release);
            }
            EspCommand::Reset => {
                if self.port.is_some() {
                    self.identify(true);
                }
            }
            EspCommand::Start {
                duration_sec,
                log_mode,
                session,
            } => {
                self.shared.reset_counters();
                self.reset_parsers();
                self.sync_log_mode(log_mode);
                if let Some((id, epoch)) = session {
                    self.send_command(&render_set_session(id, Some(epoch)));
                }
                self.send_line(&render_start(duration_sec));
                self.shared.is_collecting.store(true, Ordering::Release);
                self.shared.bump_io_generation();
            }
            EspCommand::Stop => {
                self.send_bytes(b"q");
                self.shared.is_collecting.store(false, Ordering::Release);
                self.reset_parsers();
                self.shared.bump_io_generation();
            }
            EspCommand::ApplyConfig { cfg, chip } => {
                if self.collecting() {
                    log::warn!("ApplyConfig ignored: collection in progress");
                    return;
                }
                for line in render_apply_config(&cfg, chip) {
                    self.send_command(&line);
                    if line.starts_with("set-log-mode") {
                        self.shared
                            .log_mode_id
                            .store(cfg.log_mode.id(), Ordering::Release);
                        self.shared.bump_io_generation();
                        self.reset_parsers();
                    }
                }
                self.shared.config_applied.store(true, Ordering::Release);
            }
            EspCommand::Raw(s) => {
                // The firmware CLI only honours `q` while collecting; drop
                // anything else rather than clog the wire.
                if self.collecting() {
                    log::debug!("raw command ignored while collecting: {s}");
                } else {
                    self.send_command(&s);
                }
            }
            EspCommand::Restart => {
                if self.collecting() {
                    self.send_bytes(b"q");
                    self.shared.is_collecting.store(false, Ordering::Release);
                }
                self.send_line(&render_restart());
                // A native-USB board re-enumerates on reboot: the read
                // errors out and the open loop picks it back up. A
                // UART-bridged one stays open: wait for its banner.
                if self.port.is_some() {
                    std::thread::sleep(Duration::from_millis(300));
                    self.identify(false);
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
        self.set_state(ConnState::Opening);
        match self.backend.open(&name, baud) {
            Ok(link) => {
                self.open_failures = 0;
                self.port = Some(BufReader::with_capacity(16 * 1024, link));
                self.identify(false);
            }
            Err(e) => {
                // Back off 0.5 s, 1 s, 2 s … capped at 5 s, without blocking
                // the command loop.
                self.open_failures = self.open_failures.saturating_add(1);
                let backoff = 250u64 << self.open_failures.min(5);
                self.next_open = Instant::now() + Duration::from_millis(backoff.min(5_000));
                self.set_state(ConnState::Error(format!("open {name}: {e}")));
            }
        }
    }

    /// Find out what's on the port. See the module docs for the sequence.
    /// `hard_reset` skips the polite ask and goes straight to RTS.
    fn identify(&mut self, hard_reset: bool) {
        self.reset_parsers();
        self.shared.is_collecting.store(false, Ordering::Release);
        self.shared.config_applied.store(false, Ordering::Release);
        self.set_state(ConnState::Identifying);

        let mut info = None;
        if !hard_reset {
            self.send_bytes(b"q\r\n");
            self.drain(Duration::from_millis(500));
            info = self.fetch_info(Duration::from_millis(2_000));
        }
        if info.is_none() {
            if let Some(port) = self.port.as_mut() {
                port.get_mut().pulse_reset();
            }
            if self.await_banner(Duration::from_secs(6)) {
                // Let the CLI finish booting before talking to it.
                self.drain(Duration::from_millis(200));
                info = self.fetch_info(Duration::from_millis(3_000));
            }
        }

        let Some(info) = info else {
            self.set_state(ConnState::UnknownFirmware);
            return;
        };
        self.chip = info.chip;
        if let Some(mac) = &info.mac {
            self.device_id = mac.clone();
        }
        // Whatever the board was told before, it boots (and `q` leaves it)
        // in its own log mode; the UI re-applies its config on `Streaming`.
        let _ = self.events.send(ReaderEvent::Info(info.clone()));
        match info.check() {
            Ok(()) => self.set_state(ConnState::Streaming),
            Err(why) => self.set_state(ConnState::Incompatible(why.to_string())),
        }
    }

    /// Read and discard for `dur` (forwarding text to the console).
    fn drain(&mut self, dur: Duration) {
        let deadline = Instant::now() + dur;
        let mut buf = [0u8; 1024];
        while Instant::now() < deadline {
            let Some(port) = self.port.as_mut() else {
                return;
            };
            match port.read(&mut buf) {
                Ok(0) => return,
                Ok(n) => push_printable(&self.console, &buf[..n]),
                Err(e) if is_transient(&e) => {}
                Err(_) => return,
            }
        }
    }

    /// Wait for `ESP-CSI-CLI/<version>` at the start of a line. Uses raw
    /// reads so ROM-bootloader noise that isn't UTF-8 can't trip anything.
    fn await_banner(&mut self, dur: Duration) -> bool {
        let deadline = Instant::now() + dur;
        let mut acc: Vec<u8> = Vec::with_capacity(4096);
        let mut buf = [0u8; 256];
        while Instant::now() < deadline {
            let Some(port) = self.port.as_mut() else {
                return false;
            };
            match port.read(&mut buf) {
                Ok(0) => return false,
                Ok(n) => {
                    acc.extend_from_slice(&buf[..n]);
                    if banner_match(&acc, b"ESP-CSI-CLI/") {
                        push_printable(&self.console, &acc);
                        return true;
                    }
                    if acc.len() > 16 * 1024 {
                        acc.drain(..acc.len() - 1024);
                    }
                }
                Err(e) if is_transient(&e) => {}
                Err(_) => return false,
            }
        }
        push_printable(&self.console, &acc);
        false
    }

    /// Send `info` and collect `key=value` lines up to `END-INFO`.
    fn fetch_info(&mut self, dur: Duration) -> Option<DeviceInfo> {
        // The leading CRLF terminates whatever half-line the CLI holds.
        self.send_line("\r\ninfo");
        let deadline = Instant::now() + dur;
        let mut acc = String::new();
        let mut line = Vec::new();
        while Instant::now() < deadline {
            let port = self.port.as_mut()?;
            match port.read_until(b'\n', &mut line) {
                Ok(0) => return None,
                Ok(_) => {
                    if line.last() != Some(&b'\n') {
                        continue; // timed out mid-line; keep accumulating
                    }
                    let text = String::from_utf8_lossy(&line);
                    let trimmed = text.trim_matches(['\r', '\n', '\0', ' ']);
                    // The prompt can prefix the first response line.
                    let trimmed = trimmed.trim_start_matches("> ");
                    if !trimmed.is_empty() {
                        self.console.push(trimmed);
                    }
                    if trimmed == "END-INFO" {
                        let info = DeviceInfo::parse(&acc);
                        // `protocol=` is what marks a real esp-csi-cli-rs reply.
                        return (info.protocol > 0 || !info.version.is_empty()).then_some(info);
                    }
                    acc.push_str(trimmed);
                    acc.push('\n');
                    line.clear();
                }
                Err(e) if is_transient(&e) => {}
                Err(_) => return None,
            }
        }
        None
    }

    /// Push `set-log-mode` to the device and align the reader's parser.
    fn sync_log_mode(&mut self, log_mode: LogMode) {
        self.send_command(&format!("set-log-mode --mode={}", log_mode.cli()));
        self.shared
            .log_mode_id
            .store(log_mode.id(), Ordering::Release);
        self.shared.bump_io_generation();
        self.reset_parsers();
    }

    /// Send a CLI command and wait (up to 500 ms) for the `> ` prompt that
    /// ends its reply, forwarding the reply to the console.
    ///
    /// Pacing matters on UART-bridged boards: the classic ESP32's RX FIFO
    /// is 128 bytes, and esp-csi-cli-rs 0.10.1 panics on an RX overflow
    /// (`Rx(FifoOverflowed)`) and reboots if the host sends a config's
    /// worth of lines back to back.
    fn send_command(&mut self, line: &str) {
        self.send_line(line);
        let deadline = Instant::now() + Duration::from_millis(500);
        let mut acc: Vec<u8> = Vec::new();
        let mut buf = [0u8; 512];
        while Instant::now() < deadline {
            let Some(port) = self.port.as_mut() else {
                return;
            };
            match port.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    acc.extend_from_slice(&buf[..n]);
                    if acc.ends_with(b"> ") || acc.ends_with(b">") {
                        break;
                    }
                }
                Err(e) if is_transient(&e) => {}
                Err(_) => break,
            }
        }
        if contains(&acc, b"ESP-CSI-CLI/") || contains(&acc, b"PANIC") {
            log::warn!("{}: device rebooted while configuring", self.device_id);
        }
        push_printable(&self.console, &acc);
    }

    fn send_line(&mut self, line: &str) {
        let shown = line.trim_start_matches(['\r', '\n']);
        self.console.push(format!("» {shown}"));
        let mut buf = line.as_bytes().to_vec();
        buf.extend_from_slice(b"\r\n");
        self.send_bytes(&buf);
    }

    fn send_bytes(&mut self, bytes: &[u8]) {
        let Some(port) = self.port.as_mut() else {
            return;
        };
        let port = port.get_mut();
        let _ = port.write_all(bytes);
        let _ = port.flush();
    }

    fn handle_disconnect(&mut self, io_err: bool) {
        if io_err {
            log::info!(
                "{}: port went away",
                self.port_name.as_deref().unwrap_or("?")
            );
        }
        self.port = None;
        self.reset_parsers();
        self.shared.is_collecting.store(false, Ordering::Release);
        self.next_open = Instant::now() + Duration::from_millis(500);
        self.set_state(ConnState::Disconnected);
    }

    /// The firmware ended the run by itself.
    fn collection_ended(&mut self) {
        if self.collecting() {
            self.shared.is_collecting.store(false, Ordering::Release);
            self.reset_parsers();
            let _ = self.events.send(ReaderEvent::CollectionEnded);
        }
    }

    fn push_packet(&mut self, mut p: CsiData) {
        if !p.is_decoded() {
            self.shared
                .frames_decode_failed
                .fetch_add(1, Ordering::Relaxed);
        }
        p.device.clone_from(&self.device_id);
        p.host_time_us = crate::paths::unix_now_us();
        match self.packets.try_send(p) {
            Ok(()) | Err(TrySendError::Disconnected(_)) => {}
            Err(TrySendError::Full(_)) => {
                self.shared.frames_dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Read one frame (serialized) or line (text modes, and any mode while
    /// idle, when the firmware only prints CLI text).
    fn read_one(&mut self) -> ReadOutcome {
        let mode = LogMode::from_id(self.shared.log_mode_id.load(Ordering::Acquire));
        let collecting = self.collecting();
        let delim = if collecting && mode == LogMode::Serialized {
            b'\0'
        } else {
            b'\n'
        };

        let Some(port) = self.port.as_mut() else {
            return ReadOutcome::Idle;
        };
        match port.read_until(delim, &mut self.frame) {
            Ok(0) => return ReadOutcome::Idle,
            Ok(_) if self.frame.last() == Some(&delim) => {}
            // Short read (the timeout hit mid-frame): keep the bytes.
            Ok(_) => return ReadOutcome::Progress,
            Err(e) if is_transient(&e) => {
                // A timed run that ends in serialized mode leaves
                // "Collection complete." unterminated: no `\0` follows.
                if collecting && contains(&self.frame, COLLECTION_COMPLETE) {
                    push_printable(&self.console, &self.frame);
                    self.collection_ended();
                }
                return ReadOutcome::Idle;
            }
            Err(_) => return ReadOutcome::IoError,
        }
        self.frame.pop(); // the delimiter
        let frame = std::mem::take(&mut self.frame);

        if !collecting {
            push_printable(&self.console, &frame);
            // A banner while idle means the board rebooted (a panic, a
            // watchdog, the reset button): its config is gone. Re-identify
            // so the UI sees it come back and re-applies the config.
            if frame.starts_with(b"ESP-CSI-CLI/") || contains(&frame, b"\nESP-CSI-CLI/") {
                self.frame = frame;
                self.frame.clear();
                log::info!("{}: device rebooted; re-identifying", self.device_id);
                self.drain(Duration::from_millis(300));
                self.identify(false);
                return ReadOutcome::Progress;
            }
        } else if delim == b'\0' {
            self.handle_serialized(&frame);
        } else {
            self.handle_text_line(mode, &frame);
        }
        self.frame = frame;
        self.frame.clear();
        ReadOutcome::Progress
    }

    fn handle_serialized(&mut self, frame: &[u8]) {
        if frame.is_empty() {
            return;
        }
        if frame.len() <= MAX_TEXT_CHUNK && is_printable(frame) {
            push_printable(&self.console, frame);
            if contains(frame, COLLECTION_COMPLETE) {
                self.collection_ended();
            }
            return;
        }
        self.shared.frames_total.fetch_add(1, Ordering::Relaxed);
        let mut decoded = decode_frame(frame, self.chip);
        if decoded.is_err() {
            // The CLI's echo of the last command isn't `\0`-terminated, so it
            // can arrive glued to the front of the first frame of a run.
            // Retry after each line break in a printable prefix.
            if let Some((text, rest)) = split_text_prefix(frame, self.chip) {
                push_printable(&self.console, text);
                decoded = Ok(rest);
            }
        }
        match decoded {
            Ok(DecodedFrame::Csi(p)) => {
                if p.node_id.is_empty() {
                    self.last_seq = None;
                } else {
                    if let Some(prev) = self.last_seq {
                        let gap = p.stream_seq.wrapping_sub(prev).wrapping_sub(1);
                        // A huge "gap" is a counter reset, not loss.
                        if gap > 0 && gap < 1 << 20 {
                            self.shared
                                .frames_lost
                                .fetch_add(gap as u64, Ordering::Relaxed);
                        }
                    }
                    self.last_seq = Some(p.stream_seq);
                }
                self.push_packet(*p);
            }
            Ok(DecodedFrame::Session(s)) => {
                // Not a measurement: don't count it towards the decode rate.
                self.shared.frames_total.fetch_sub(1, Ordering::Relaxed);
                self.shared
                    .session_id
                    .store(s.session_id, Ordering::Relaxed);
                self.last_seq = None;
                self.console.push(format!(
                    "session 0x{:08x} from {} ({}, esp-csi-rs {}.{}.{})",
                    s.session_id,
                    s.node_id,
                    s.chip,
                    s.crate_version[0],
                    s.crate_version[1],
                    s.crate_version[2]
                ));
                let _ = self.events.send(ReaderEvent::Session(s));
            }
            Ok(DecodedFrame::Text(t)) => {
                self.shared.frames_total.fetch_sub(1, Ordering::Relaxed);
                self.console.push(t);
            }
            Ok(DecodedFrame::Unplottable) => {
                self.shared
                    .frames_unplottable
                    .fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => {
                self.shared
                    .frames_decode_failed
                    .fetch_add(1, Ordering::Relaxed);
                if let FrameError::UnsupportedVersion(v) = e {
                    log::warn!("wire format v{v} is newer than this GUI understands");
                }
            }
        }
    }

    fn handle_text_line(&mut self, mode: LogMode, line: &[u8]) {
        let line = String::from_utf8_lossy(line);
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if contains(trimmed.as_bytes(), COLLECTION_COMPLETE) {
            self.console.push(trimmed);
            self.collection_ended();
            return;
        }
        let pkt = match mode {
            LogMode::Text => {
                self.console.push(trimmed);
                match self.text.feed_line(&line) {
                    Some(p) => p,
                    None => return,
                }
            }
            LogMode::ArrayList => {
                if !trimmed.starts_with('[') {
                    self.console.push(trimmed);
                    return;
                }
                parse_array_list(trimmed, self.chip).unwrap_or_default()
            }
            LogMode::EspCsiTool => {
                if !trimmed.starts_with("CSI_DATA") {
                    self.console.push(trimmed);
                    return;
                }
                parse_esp_csi_tool(trimmed).unwrap_or_default()
            }
            // The serialized decoder never reads lines while collecting.
            LogMode::Serialized => return,
        };
        self.shared.frames_total.fetch_add(1, Ordering::Relaxed);
        self.push_packet(pkt);
    }
}

/// Find a printable, newline-terminated prefix after which the rest of
/// `frame` decodes. Returns the prefix and the decoded remainder.
fn split_text_prefix(frame: &[u8], chip: ChipFamily) -> Option<(&[u8], DecodedFrame)> {
    let printable_len = frame
        .iter()
        .position(|&c| !((0x20..0x7f).contains(&c) || matches!(c, b'\t' | b'\r' | b'\n' | 0x1b)))
        .unwrap_or(frame.len());
    // Cut after a `\n`, or after the `> ` prompt the CLI prints last.
    let mut cuts: Vec<usize> = frame[..printable_len]
        .iter()
        .enumerate()
        .filter(|(_, c)| **c == b'\n')
        .map(|(i, _)| i + 1)
        .collect();
    if frame[..printable_len].ends_with(b"> ") {
        cuts.push(printable_len);
    }
    cuts.into_iter().rev().find_map(|cut| {
        let rest = &frame[cut..];
        if rest.is_empty() {
            return None;
        }
        decode_frame(rest, chip).ok().map(|d| (&frame[..cut], d))
    })
}

fn is_transient(e: &std::io::Error) -> bool {
    use std::io::ErrorKind::*;
    matches!(e.kind(), TimedOut | Interrupted | WouldBlock)
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// Printable ASCII plus tab / CR / LF / ESC (the CLI's line-clear code).
fn is_printable(b: &[u8]) -> bool {
    b.iter()
        .all(|&c| (0x20..0x7f).contains(&c) || matches!(c, b'\t' | b'\r' | b'\n' | 0x1b))
}

/// Forward the readable lines in `bytes` to the console, skipping binary.
fn push_printable(console: &RawConsole, bytes: &[u8]) {
    for line in bytes.split(|&b| b == b'\n') {
        let line: Vec<u8> = line
            .iter()
            .copied()
            .filter(|&c| c != b'\r' && c != 0)
            .collect();
        // Drop the CLI's ESC[2K line-clear so the console shows plain text.
        let text = String::from_utf8_lossy(&line).replace("\x1b[2K", "");
        let text = text.trim();
        if !text.is_empty() && is_printable(text.as_bytes()) {
            console.push(text.to_string());
        }
    }
}

/// List the serial port names currently visible to the OS.
///
/// Returns paths like `/dev/ttyUSB0` on Linux, `COM3` on Windows, or
/// `/dev/cu.usbserial-…` on macOS.
pub fn enumerate_ports() -> Vec<String> {
    serialport::available_ports()
        .map(|ps| ps.into_iter().map(|p| p.port_name).collect())
        .unwrap_or_default()
}

/// Look for `needle` at the start of a line (buffer start or after `\n`)
/// followed by ≥1 non-whitespace byte and a line terminator — the
/// `^ESP-CSI-CLI/\S+$` banner, matched without UTF-8 validation so it can
/// be found through bootloader garbage.
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
                if !version.is_empty() && version.iter().all(|&b| !b.is_ascii_whitespace()) {
                    return true;
                }
            }
        }
        i = pos + 1;
    }
    false
}

#[cfg(test)]
mod tests;
