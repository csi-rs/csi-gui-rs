//! Connected ESP32s: one [`DeviceSession`] per board, owned by a
//! [`DeviceManager`].
//!
//! Every session has its own reader thread, history, preprocessing context
//! and recorder, so N boards capture side by side; the views pick which
//! one(s) to draw. The manager adds and removes boards, follows them across
//! re-enumeration, remembers their settings ([`store`]) and runs the
//! multi-device actions (apply to all, start / stop all under one shared
//! measurement session).

pub mod discovery;
pub mod store;

use crate::console::RawConsole;
use crate::data::{CsiData, History, PreprocessConfig};
use crate::persistence::SessionRecorder;
use crate::protocol::command::EspCommand;
use crate::protocol::config::{EspConfig, WifiMode};
use crate::protocol::info::DeviceInfo;
use crate::protocol::serialized::SessionAnnounce;
use crate::reader::{ConnState, ReaderEvent, ReaderHandles, spawn_reader_with};
use crate::rerun_sink::RerunSink;
use crate::shared::EspSharedState;
use crate::sim::MixedBackend;
use discovery::PortCandidate;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use store::DeviceStore;

/// Packets kept per device as context for window-based preprocessing
/// filters. Spans the longest stage window exposed (moving-average
/// detrending caps at 128).
const PREPROCESS_CONTEXT: usize = 256;

/// Series colours handed to devices in turn: distinct in both light and
/// dark themes and for the common colour-vision deficiencies.
pub const DEVICE_COLORS: [egui::Color32; 8] = [
    egui::Color32::from_rgb(0x4C, 0x9A, 0xFF), // blue
    egui::Color32::from_rgb(0xFF, 0x9F, 0x1C), // orange
    egui::Color32::from_rgb(0x2E, 0xC4, 0x8D), // green
    egui::Color32::from_rgb(0xE8, 0x5D, 0x9A), // pink
    egui::Color32::from_rgb(0xA3, 0x7B, 0xFF), // violet
    egui::Color32::from_rgb(0x00, 0xB8, 0xD4), // cyan
    egui::Color32::from_rgb(0xD4, 0xC1, 0x00), // yellow
    egui::Color32::from_rgb(0xB0, 0x6A, 0x3B), // brown
];

/// Something the UI should tell the user about, as a toast.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    /// Routine: a device came up, a recording was saved.
    Info(String),
    /// Needs attention but nothing broke.
    Warn(String),
    /// Something failed.
    Error(String),
}

/// One connected board.
pub struct DeviceSession {
    /// Port the session reads (`/dev/ttyACM0`, `COM3`, `sim:0`).
    pub port: String,
    /// USB serial number of the port, used to follow the board when it
    /// re-enumerates under another name.
    pub usb_serial: Option<String>,
    /// User-given name; empty for none. See [`Self::label`].
    pub alias: String,
    /// Series colour in plots.
    pub color: egui::Color32,
    reader: ReaderHandles,
    /// Lock-free flags and counters shared with the reader thread.
    pub shared: Arc<EspSharedState>,
    /// The device's text output.
    pub console: Arc<RawConsole>,
    /// Connection state, as last reported by the reader.
    pub conn_state: ConnState,
    /// Identity from the last successful `info`.
    pub info: Option<DeviceInfo>,
    /// Editable config, pushed with [`Self::apply_config`].
    pub cfg: EspConfig,
    /// Validation error from the last apply, shown in the panel.
    pub config_error: Option<String>,
    /// Plot history: one averaged entry per UI tick.
    pub history: History,
    preprocess_buffer: VecDeque<CsiData>,
    /// Per-packet JSONL recorder; runs while collecting when recording is on.
    pub recorder: SessionRecorder,
    /// The last run announcement (wire v1).
    pub last_session: Option<SessionAnnounce>,
    /// Ticked in the device list, for "apply to selected" / "start selected".
    pub selected: bool,
    /// The user dismissed the firmware problem card for this state.
    pub problem_dismissed: bool,
    /// `false` for a read-only session holding an imported capture.
    pub live: bool,
    /// Frames counted at the previous tick, for the raw-fps estimate.
    frames_at_last_tick: u64,
    /// Tick at which the current run started (for "Start all" alignment).
    pub started_at: Option<Instant>,
    /// `Some` while the user wants this board collecting: set by
    /// [`Self::start`], cleared by [`Self::stop`] or a timed run ending.
    /// Holds the session id to rejoin. When the board drops out (unplug,
    /// reboot) and comes back, the run resumes under it.
    pub resume: Option<Option<u32>>,
}

impl DeviceSession {
    /// Open `port` at `baud`: spawn the reader thread and start identifying.
    pub fn open(port: String, baud: u32, history_size: usize, color: egui::Color32) -> Self {
        let shared = EspSharedState::handle_with_baud(baud);
        let console = Arc::new(RawConsole::new(2_000));
        let reader = spawn_reader_with(MixedBackend::default(), shared.clone(), console.clone());
        let _ = reader.commands.send(EspCommand::OpenPort(port.clone()));
        Self {
            port,
            usb_serial: None,
            alias: String::new(),
            color,
            reader,
            shared,
            console,
            conn_state: ConnState::Opening,
            info: None,
            cfg: EspConfig::default(),
            config_error: None,
            history: History::with_capacity(history_size),
            preprocess_buffer: VecDeque::with_capacity(PREPROCESS_CONTEXT),
            recorder: SessionRecorder::default(),
            last_session: None,
            selected: true,
            problem_dismissed: false,
            live: true,
            frames_at_last_tick: 0,
            started_at: None,
            resume: None,
        }
    }

    /// A read-only session holding an imported capture, for replay next to
    /// the live devices.
    pub fn imported(name: String, packets: Vec<CsiData>, color: egui::Color32) -> Self {
        let mut s = Self::open_detached(name, color);
        s.history = History::with_capacity(packets.len().max(1));
        s.history.load_from_packets(packets);
        s
    }

    fn open_detached(name: String, color: egui::Color32) -> Self {
        // A reader with no port: it idles until shut down.
        let shared = EspSharedState::handle();
        let console = Arc::new(RawConsole::new(16));
        let reader = spawn_reader_with(MixedBackend::default(), shared.clone(), console.clone());
        Self {
            alias: name.clone(),
            port: name,
            usb_serial: None,
            color,
            reader,
            shared,
            console,
            conn_state: ConnState::Disconnected,
            info: None,
            cfg: EspConfig::default(),
            config_error: None,
            history: History::new(),
            preprocess_buffer: VecDeque::new(),
            recorder: SessionRecorder::default(),
            last_session: None,
            selected: false,
            problem_dismissed: true,
            live: false,
            frames_at_last_tick: 0,
            started_at: None,
            resume: None,
        }
    }

    /// Stable identity: the board's MAC once known, else the port.
    pub fn id(&self) -> String {
        self.mac().unwrap_or_else(|| self.port.clone())
    }

    /// The board's MAC, from `info` or (native USB) the USB serial number.
    pub fn mac(&self) -> Option<String> {
        self.info.as_ref().and_then(|i| i.mac.clone()).or_else(|| {
            self.usb_serial
                .as_deref()
                .and_then(discovery::normalize_mac)
        })
    }

    /// Display name: the alias, else the short port name.
    pub fn label(&self) -> String {
        if !self.alias.is_empty() {
            return self.alias.clone();
        }
        self.port
            .rsplit('/')
            .next()
            .unwrap_or(&self.port)
            .to_string()
    }

    /// `true` when identified and running supported firmware.
    pub fn is_ready(&self) -> bool {
        self.live && self.conn_state == ConnState::Streaming
    }

    /// `true` while the firmware is in a collection run.
    pub fn is_collecting(&self) -> bool {
        self.shared.is_collecting.load(Ordering::Acquire)
    }

    /// `true` once the current config has been pushed to the device.
    pub fn config_applied(&self) -> bool {
        self.shared.is_config_applied()
    }

    /// Chip family, or `Unknown` before identification.
    pub fn chip(&self) -> crate::protocol::info::ChipFamily {
        self.info
            .as_ref()
            .map(|i| i.chip)
            .unwrap_or(crate::protocol::info::ChipFamily::Unknown)
    }

    /// Queue a command for the reader thread.
    pub fn send(&self, cmd: EspCommand) {
        let _ = self.reader.commands.send(cmd);
    }

    /// Validate the config for this board's chip and push it.
    pub fn apply_config(&mut self) -> Result<(), String> {
        let chip = self.chip();
        match self.cfg.validate_for(chip) {
            Ok(()) => {
                self.config_error = None;
                if self.info.is_some() {
                    self.send(EspCommand::ApplyConfig {
                        cfg: Box::new(self.cfg.clone()),
                        chip,
                    });
                }
                Ok(())
            }
            Err(e) => {
                self.config_error = Some(e.clone());
                Err(e)
            }
        }
    }

    /// Start a run. `session` is the shared `(id, epoch_us)` from "Start
    /// all", or `None` to let the firmware pick a session id.
    pub fn start(&mut self, session: Option<(u32, u64)>) {
        self.resume = Some(session.map(|(id, _)| id));
        self.preprocess_buffer.clear();
        self.started_at = Some(Instant::now());
        self.send(EspCommand::Start {
            duration_sec: self.cfg.start_duration_sec,
            log_mode: self.cfg.log_mode,
            session,
        });
    }

    /// Stop the run (`q`).
    pub fn stop(&mut self) {
        self.resume = None;
        self.send(EspCommand::Stop);
    }

    /// Restart the run the user wanted after the board came back, rejoining
    /// its session (new epoch: the board's clock restarted). Returns `true`
    /// when a run was resumed.
    fn resume_run(&mut self) -> bool {
        let Some(id) = self.resume else {
            return false;
        };
        let session = id.map(|id| (id, crate::paths::unix_now_us()));
        self.start(session);
        true
    }

    /// Drain reader events. Returns notices for the UI, and whether the
    /// device just reached `Streaming` (the caller then pushes its config).
    fn drain_events(&mut self, notices: &mut Vec<Notice>) -> bool {
        let mut became_ready = false;
        while let Ok(ev) = self.reader.events.try_recv() {
            match ev {
                ReaderEvent::State(st) => {
                    if st != self.conn_state {
                        self.problem_dismissed = false;
                    }
                    match &st {
                        ConnState::Streaming => became_ready = true,
                        ConnState::Disconnected if self.conn_state == ConnState::Streaming => {
                            notices.push(Notice::Warn(format!("{} disconnected", self.label())))
                        }
                        ConnState::Incompatible(why) => {
                            notices.push(Notice::Error(format!("{}: {why}", self.label())))
                        }
                        _ => {}
                    }
                    self.conn_state = st;
                }
                ReaderEvent::Info(info) => self.info = Some(info),
                ReaderEvent::Session(s) => self.last_session = Some(s),
                ReaderEvent::CollectionEnded => {
                    self.resume = None;
                    notices.push(Notice::Info(format!("{}: run finished", self.label())))
                }
                ReaderEvent::Rebooted => notices.push(Notice::Warn(format!(
                    "{} rebooted by itself; reconnecting",
                    self.label()
                ))),
            }
        }
        became_ready
    }

    /// Drain packets into history (and recorder / Rerun) for UI tick `tick`.
    fn pump_packets(&mut self, tick: u64, preprocess: &PreprocessConfig, rerun: &mut RerunSink) {
        let mut batch: Vec<CsiData> = self.reader.packets.try_iter().collect();
        let frames = self.shared.frames_total.load(Ordering::Relaxed);
        let raw_delta = frames.saturating_sub(self.frames_at_last_tick);
        self.frames_at_last_tick = frames;
        if !self.live {
            return;
        }
        for p in &batch {
            rerun.log_packet(p);
        }
        if preprocess.is_active() && !batch.is_empty() {
            batch = self.preprocess(preprocess, batch);
        }
        if self.recorder.is_recording() {
            self.recorder.append_batch(&batch);
        }
        let label = self.id();
        if let Some(stats) = self.history.push_tick_at(tick, batch, raw_delta) {
            rerun.log_tick(&label, stats);
        }
    }

    /// Run the preprocessing pipeline over the rolling context so window
    /// filters have continuity; return only the new packets' outputs.
    /// History already committed is never rewritten.
    fn preprocess(&mut self, cfg: &PreprocessConfig, batch: Vec<CsiData>) -> Vec<CsiData> {
        let n_new = batch.len();
        self.preprocess_buffer.extend(batch);
        while self.preprocess_buffer.len() > PREPROCESS_CONTEXT {
            self.preprocess_buffer.pop_front();
        }
        let window: Vec<CsiData> = self.preprocess_buffer.iter().cloned().collect();
        let processed = cfg.apply(&window);
        let take = n_new.min(processed.len());
        processed[processed.len() - take..].to_vec()
    }

    /// Forget the preprocessing context (after the filters change).
    pub fn clear_preprocess_context(&mut self) {
        self.preprocess_buffer.clear();
    }

    /// Start or stop the recorder to match "record while collecting".
    fn sync_recording(&mut self, record: bool, notices: &mut Vec<Notice>) {
        let want = record && self.live && self.is_collecting();
        if want && !self.recorder.is_recording() {
            let session = self.shared.session_id.load(Ordering::Relaxed);
            let label = if session != 0 {
                format!("{}_{session:08x}", self.label())
            } else {
                self.label()
            };
            if let Err(e) = self.recorder.start(&crate::paths::exports_dir(), &label) {
                notices.push(Notice::Error(format!(
                    "{}: recording failed: {e}",
                    self.label()
                )));
            }
        } else if !want && self.recorder.is_recording() {
            if let Some(sum) = self.recorder.stop() {
                notices.push(Notice::Info(format!(
                    "{}: saved {} packets to {}",
                    self.label(),
                    sum.packets_written,
                    sum.path.display()
                )));
            } else if let Some(err) = self.recorder.last_error() {
                notices.push(Notice::Error(format!(
                    "{}: recording failed: {err}",
                    self.label()
                )));
            }
        }
    }

    /// Ask the reader thread to exit.
    pub fn close(&mut self) {
        self.recorder.stop();
        self.send(EspCommand::Shutdown);
    }

    /// Re-point the session at `port` (the board re-enumerated there).
    fn reopen(&mut self, port: String) {
        self.port = port.clone();
        self.send(EspCommand::OpenPort(port));
    }
}

impl Drop for DeviceSession {
    fn drop(&mut self) {
        self.close();
    }
}

/// Every connected board, plus the multi-device actions.
pub struct DeviceManager {
    /// Sessions, in the order they were added.
    pub sessions: Vec<DeviceSession>,
    /// Index of the device the ESP panel edits and that panes without an
    /// explicit device follow.
    pub active: Option<usize>,
    /// Remembered aliases and configs.
    pub store: DeviceStore,
    /// Last port scan.
    pub ports: Vec<PortCandidate>,
    last_scan: Instant,
    /// Attach every new ESP32-looking port as it appears.
    pub auto_attach: bool,
    /// Record per-packet JSONL while collecting.
    pub record_on_capture: bool,
    /// Baud for newly opened ports.
    pub baud: u32,
    /// History window for new sessions.
    pub history_size: usize,
    /// Global tick counter: every history is pushed with the same id.
    pub tick: u64,
    /// Ports the user removed; auto-attach leaves them alone.
    ignored: Vec<String>,
}

impl DeviceManager {
    /// An empty manager; load remembered settings from the config dir.
    pub fn new(baud: u32, history_size: usize) -> Self {
        Self {
            sessions: Vec::new(),
            active: None,
            store: DeviceStore::load(&crate::paths::devices_file()),
            ports: Vec::new(),
            last_scan: Instant::now() - Duration::from_secs(60),
            auto_attach: false,
            record_on_capture: false,
            baud,
            history_size,
            tick: 0,
            ignored: Vec::new(),
        }
    }

    /// The active session.
    pub fn active(&self) -> Option<&DeviceSession> {
        self.active.and_then(|i| self.sessions.get(i))
    }

    /// The active session, mutably.
    pub fn active_mut(&mut self) -> Option<&mut DeviceSession> {
        match self.active {
            Some(i) => self.sessions.get_mut(i),
            None => None,
        }
    }

    /// Index of the session with id (MAC or port) `id`.
    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.sessions
            .iter()
            .position(|s| s.id() == id || s.port == id)
    }

    /// `true` when `port` already has a session.
    pub fn has_port(&self, port: &str) -> bool {
        self.sessions.iter().any(|s| s.port == port)
    }

    fn next_color(&self) -> egui::Color32 {
        DEVICE_COLORS
            .iter()
            .copied()
            .find(|c| !self.sessions.iter().any(|s| s.color == *c))
            .unwrap_or(DEVICE_COLORS[self.sessions.len() % DEVICE_COLORS.len()])
    }

    /// Open a session on `port` (no-op if one exists). Returns its index.
    pub fn add(&mut self, port: &str) -> usize {
        if let Some(i) = self.sessions.iter().position(|s| s.port == port) {
            return i;
        }
        self.ignored.retain(|p| p != port);
        let color = self.next_color();
        let mut s = DeviceSession::open(port.to_string(), self.baud, self.history_size, color);
        s.usb_serial = self
            .ports
            .iter()
            .find(|p| p.name == port)
            .and_then(|p| p.serial.clone());
        // A native-USB board names itself before it answers: restore its
        // settings now so the panel shows them straight away.
        if let Some(mac) = s.mac()
            && let Some(stored) = self.store.get(&mac)
        {
            s.alias = stored.alias.clone();
            s.cfg = stored.config.clone();
        }
        self.sessions.push(s);
        let i = self.sessions.len() - 1;
        if self.active.is_none() {
            self.active = Some(i);
        }
        i
    }

    /// Add a read-only session holding an imported capture. Returns its index.
    pub fn add_imported(&mut self, name: String, packets: Vec<CsiData>) -> usize {
        let color = self.next_color();
        self.sessions
            .push(DeviceSession::imported(name, packets, color));
        self.sessions.len() - 1
    }

    /// Close and remove session `i`.
    pub fn remove(&mut self, i: usize) {
        if i >= self.sessions.len() {
            return;
        }
        self.remember(i);
        let mut s = self.sessions.remove(i);
        if s.live {
            self.ignored.push(s.port.clone());
        }
        s.close();
        self.active = match self.active {
            _ if self.sessions.is_empty() => None,
            Some(a) if a > i => Some(a - 1),
            Some(a) if a == i => Some(i.min(self.sessions.len() - 1)),
            other => other,
        };
    }

    /// Save session `i`'s alias and config under its MAC.
    pub fn remember(&mut self, i: usize) {
        let Some(s) = self.sessions.get(i) else {
            return;
        };
        let Some(mac) = s.mac() else { return };
        if !s.live {
            return;
        }
        self.store.remember(&mac, &s.alias, &s.cfg);
        if let Err(e) = self.store.save(&crate::paths::devices_file()) {
            log::warn!("saving device settings: {e}");
        }
    }

    /// Rescan ports now (normally every 2 s from [`Self::pump`]).
    pub fn rescan(&mut self) {
        self.ports = discovery::scan();
        self.last_scan = Instant::now();
    }

    /// Ports that look like ESP32s and have no session yet.
    pub fn free_ports(&self) -> Vec<&PortCandidate> {
        self.ports
            .iter()
            .filter(|p| !self.has_port(&p.name))
            .collect()
    }

    /// Attach every free ESP32-looking port. Returns how many were added.
    pub fn attach_all(&mut self) -> usize {
        let free: Vec<String> = self
            .ports
            .iter()
            .filter(|p| p.is_likely_esp() && !self.has_port(&p.name))
            .map(|p| p.name.clone())
            .collect();
        for p in &free {
            self.add(p);
        }
        free.len()
    }

    /// Per-frame work: rescan ports, follow moved boards, drain every
    /// session's events and (every 100 ms) packets. Returns notices for
    /// toasts.
    pub fn pump(&mut self) -> Vec<Notice> {
        let mut notices = Vec::new();

        if self.last_scan.elapsed() > Duration::from_secs(2) {
            self.rescan();
            self.follow_moved_boards(&mut notices);
            if self.auto_attach {
                let new: Vec<String> = self
                    .ports
                    .iter()
                    .filter(|p| {
                        p.is_likely_esp()
                            && !self.has_port(&p.name)
                            && !self.ignored.contains(&p.name)
                    })
                    .map(|p| p.name.clone())
                    .collect();
                for p in new {
                    notices.push(Notice::Info(format!("attached {p}")));
                    self.add(&p);
                }
            }
        }

        let mut ready = Vec::new();
        for (i, s) in self.sessions.iter_mut().enumerate() {
            if s.drain_events(&mut notices) {
                ready.push(i);
            }
            s.sync_recording(self.record_on_capture, &mut notices);
        }
        for i in ready {
            self.on_ready(i, &mut notices);
        }
        notices
    }

    /// Advance the global tick and move every session's packets into its
    /// history. Call every [`crate::data::history::TICK_MS`].
    pub fn tick(&mut self, preprocess: &PreprocessConfig, rerun: &mut RerunSink) {
        self.tick += 1;
        let tick = self.tick;
        for s in &mut self.sessions {
            s.pump_packets(tick, preprocess, rerun);
        }
    }

    /// A session just identified: restore its remembered settings (keyed
    /// by the MAC `info` reported) and push its config so the firmware
    /// runs what the panel shows.
    fn on_ready(&mut self, i: usize, notices: &mut Vec<Notice>) {
        let store = &self.store;
        let Some(s) = self.sessions.get_mut(i) else {
            return;
        };
        // After a re-enumeration the old port name can briefly hold another
        // board. Don't push this session's config onto it: the port follow
        // in `follow_moved_boards` moves the session to its own board.
        let expected = s.usb_serial.as_deref().and_then(discovery::normalize_mac);
        let actual = s.info.as_ref().and_then(|i| i.mac.clone());
        if let (Some(e), Some(a)) = (&expected, &actual)
            && e != a
        {
            notices.push(Notice::Warn(format!(
                "{} is now another board ({a}); waiting for {e} to reappear",
                s.label()
            )));
            return;
        }
        if let Some(stored) = s.mac().and_then(|m| store.get(&m)) {
            if s.alias.is_empty() {
                s.alias = stored.alias.clone();
            }
            if !s.config_applied() && s.cfg == EspConfig::default() {
                s.cfg = stored.config.clone();
            }
        }
        let summary = s.info.as_ref().map(DeviceInfo::summary).unwrap_or_default();
        notices.push(Notice::Info(format!("{} ready — {summary}", s.label())));
        if let Some(info) = &s.info
            && info.protocol_outdated()
        {
            notices.push(Notice::Warn(format!(
                "{} runs protocol {}; some settings need esp-csi-cli-rs 0.10+",
                s.label(),
                info.protocol
            )));
        }
        match s.apply_config() {
            // The reader handles commands in order, so `start` lands after
            // the config.
            Ok(()) if s.resume_run() => {
                notices.push(Notice::Info(format!("{}: run resumed", s.label())));
            }
            Ok(()) => {}
            Err(e) => notices.push(Notice::Warn(format!(
                "{}: config not applied: {e}",
                s.label()
            ))),
        }
    }

    /// Keep each session on its board. A board is known by its USB serial
    /// number (the MAC, on native USB); when it shows up under another
    /// port name — it re-enumerated, or two boards came back in a
    /// different order and swapped `/dev` names — the session re-opens it
    /// there. A board that vanished is only chased once it reappears, so
    /// one mid-reboot isn't disturbed.
    fn follow_moved_boards(&mut self, notices: &mut Vec<Notice>) {
        let mut moves = Vec::new();
        for (i, s) in self.sessions.iter().enumerate() {
            if !s.live || crate::sim::is_sim_port(&s.port) {
                continue;
            }
            let Some(serial) = s.usb_serial.as_deref() else {
                continue;
            };
            let here = self.ports.iter().find(|p| p.name == s.port);
            if here.is_some_and(|p| p.serial.as_deref() == Some(serial)) {
                continue; // still where it was
            }
            if let Some(new) = discovery::find_moved(&self.ports, serial, &s.port) {
                moves.push((i, new.to_string()));
            }
        }
        for (i, new) in moves {
            let s = &mut self.sessions[i];
            notices.push(Notice::Info(format!("{} moved to {new}", s.label())));
            // If another session holds `new` it is moving away too (a
            // swap); the reader retries the open until the port is free.
            s.reopen(new);
        }
    }

    /// Indices of the sessions an action applies to: the ticked ones, or
    /// every ready one when none is ticked.
    pub fn targets(&self) -> Vec<usize> {
        let ticked: Vec<usize> = (0..self.sessions.len())
            .filter(|&i| self.sessions[i].selected && self.sessions[i].live)
            .collect();
        if ticked.is_empty() {
            (0..self.sessions.len())
                .filter(|&i| self.sessions[i].live)
                .collect()
        } else {
            ticked
        }
    }

    /// Copy session `from`'s config into `to` (keeping each board's own
    /// peer MAC), then apply them.
    pub fn copy_config(&mut self, from: usize, to: &[usize]) -> Vec<Notice> {
        let Some(src) = self.sessions.get(from).map(|s| s.cfg.clone()) else {
            return Vec::new();
        };
        let mut notices = Vec::new();
        for &i in to {
            if i == from {
                continue;
            }
            let s = &mut self.sessions[i];
            let peer = std::mem::take(&mut s.cfg.peer_mac);
            s.cfg = src.clone();
            s.cfg.peer_mac = peer;
            match s.apply_config() {
                Ok(()) => {}
                Err(e) => notices.push(Notice::Error(format!("{}: {e}", s.label()))),
            }
        }
        for &i in to {
            self.remember(i);
        }
        notices
    }

    /// Validate and apply each target's own config.
    pub fn apply_all(&mut self, targets: &[usize]) -> Vec<Notice> {
        let mut notices = Vec::new();
        for &i in targets {
            let s = &mut self.sessions[i];
            if let Err(e) = s.apply_config() {
                notices.push(Notice::Error(format!("{}: {e}", s.label())));
            }
        }
        for &i in targets {
            self.remember(i);
        }
        notices
    }

    /// Pair two boards as an ESP-NOW initiator / responder on one channel,
    /// each with the other's MAC as its peer, and apply.
    pub fn pair_esp_now(&mut self, initiator: usize, responder: usize) -> Result<(), String> {
        if initiator == responder {
            return Err("pick two different devices".into());
        }
        let (Some(a), Some(b)) = (
            self.sessions.get(initiator).and_then(DeviceSession::mac),
            self.sessions.get(responder).and_then(DeviceSession::mac),
        ) else {
            return Err("both devices must be identified first".into());
        };
        let channel = self.sessions[initiator].cfg.channel;
        for (i, mode, peer) in [
            (initiator, WifiMode::EspNowInitiator, b),
            (responder, WifiMode::EspNowResponder, a),
        ] {
            let s = &mut self.sessions[i];
            s.cfg.wifi_mode = mode;
            s.cfg.peer_mac = peer.to_ascii_lowercase();
            s.cfg.channel = channel;
            s.apply_config()?;
        }
        self.remember(initiator);
        self.remember(responder);
        Ok(())
    }

    /// Start every target that is ready, under one new measurement session
    /// (same id, same epoch) so their frames line up. Returns the session id.
    pub fn start_all(&mut self, targets: &[usize]) -> (u32, Vec<Notice>) {
        let epoch = crate::paths::unix_now_us();
        // Any non-zero id unlikely to repeat: the low bits of the epoch.
        let id = (epoch as u32) | 1;
        let mut notices = Vec::new();
        let mut started = 0;
        for &i in targets {
            let s = &mut self.sessions[i];
            if !s.is_ready() || s.is_collecting() {
                continue;
            }
            if !s.config_applied() {
                notices.push(Notice::Warn(format!(
                    "{}: config not applied yet, skipped",
                    s.label()
                )));
                continue;
            }
            s.start(Some((id, epoch)));
            started += 1;
        }
        if started > 0 {
            notices.push(Notice::Info(format!(
                "started {started} device{} · session 0x{id:08x}",
                if started == 1 { "" } else { "s" }
            )));
        }
        (id, notices)
    }

    /// Stop every target that is collecting.
    pub fn stop_all(&mut self, targets: &[usize]) {
        for &i in targets {
            let s = &mut self.sessions[i];
            if s.is_collecting() {
                s.stop();
            }
        }
    }

    /// `true` when any session is collecting.
    pub fn any_collecting(&self) -> bool {
        self.sessions.iter().any(DeviceSession::is_collecting)
    }

    /// Replay bounds across every history: (oldest first id, newest last id).
    pub fn tick_bounds(&self) -> Option<(u64, u64)> {
        let firsts = self.sessions.iter().filter_map(|s| s.history.first_id());
        let lasts = self.sessions.iter().filter_map(|s| s.history.last_id());
        Some((firsts.min()?, lasts.max()?))
    }

    /// Clear every session's preprocessing context.
    pub fn clear_preprocess_context(&mut self) {
        for s in &mut self.sessions {
            s.clear_preprocess_context();
        }
    }

    /// Save every session's settings and stop every reader.
    pub fn shutdown(&mut self) {
        for i in 0..self.sessions.len() {
            self.remember(i);
        }
        for s in &mut self.sessions {
            s.close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rerun_sink::Transport;

    fn wait_ready(m: &mut DeviceManager, n: usize) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            m.pump();
            if m.sessions
                .iter()
                .filter(|s| s.is_ready() && s.config_applied())
                .count()
                == n
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("devices never became ready");
    }

    /// Two simulated boards start under one session and fill their own
    /// histories; nothing from one lands in the other.
    #[test]
    fn two_devices_capture_side_by_side() {
        let dir = std::env::temp_dir().join(format!("csi-gui-rs-mgr-{}", std::process::id()));
        crate::paths::set_config_dir_override(dir.join("config"));
        crate::paths::set_data_dir_override(dir.join("data"));
        let mut rerun = RerunSink::new(Transport::Off);
        let mut m = DeviceManager::new(115_200, 1000);
        m.add("sim:30");
        m.add("sim:31");
        wait_ready(&mut m, 2);

        let targets = m.targets();
        assert_eq!(targets, vec![0, 1]);
        let (id, _) = m.start_all(&targets);
        let pre = PreprocessConfig::default();
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline && m.sessions.iter().any(|s| s.history.entries.len() < 5) {
            m.pump();
            m.tick(&pre, &mut rerun);
            std::thread::sleep(Duration::from_millis(100));
        }
        for s in &m.sessions {
            assert!(s.history.entries.len() >= 5, "{} has no history", s.label());
            let last = s.history.last().unwrap().csi.as_ref().unwrap();
            assert_eq!(last.device, s.id(), "packet attributed to the wrong device");
            assert_eq!(last.session_id, id, "devices didn't share the session");
        }
        // Different chips, different subcarrier counts: proof the
        // histories didn't mix.
        let n0 = m.sessions[0]
            .history
            .last()
            .unwrap()
            .csi
            .as_ref()
            .unwrap()
            .subcarriers();
        let n1 = m.sessions[1]
            .history
            .last()
            .unwrap()
            .csi
            .as_ref()
            .unwrap()
            .subcarriers();
        assert_ne!(n0, n1);
        m.stop_all(&targets);
        m.shutdown();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Pump `m` (with ticks) until `pred` holds or 15 s pass; returns the
    /// notices seen.
    fn pump_until(
        m: &mut DeviceManager,
        mut pred: impl FnMut(&DeviceManager) -> bool,
    ) -> Vec<Notice> {
        let pre = PreprocessConfig::default();
        let mut rerun = RerunSink::new(Transport::Off);
        let mut seen = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            seen.extend(m.pump());
            m.tick(&pre, &mut rerun);
            if pred(m) {
                return seen;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("condition not reached; notices: {seen:?}");
    }

    fn collecting_manager(port: &str) -> (DeviceManager, u32) {
        let dir = std::env::temp_dir().join(format!("csi-gui-rs-recov-{}", std::process::id()));
        crate::paths::set_config_dir_override(dir.join("config"));
        crate::paths::set_data_dir_override(dir.join("data"));
        let mut m = DeviceManager::new(115_200, 1000);
        m.add(port);
        pump_until(&mut m, |m| {
            m.sessions[0].is_ready() && m.sessions[0].config_applied()
        });
        let (id, _) = m.start_all(&[0]);
        pump_until(&mut m, |m| {
            m.sessions[0].shared.frames_total.load(Ordering::Relaxed) > 20
        });
        (m, id)
    }

    fn frames(m: &DeviceManager) -> u64 {
        m.sessions[0].shared.frames_total.load(Ordering::Relaxed)
    }

    /// A board that reboots mid-run (panic, watchdog) is re-identified,
    /// re-configured, and resumes under the same session.
    #[test]
    fn reboot_mid_run_resumes_the_session() {
        let port = "sim:40";
        let (mut m, id) = collecting_manager(port);
        crate::sim::SimControl::of(port).reboot();
        // Wait for the reboot to be noticed and the run to come back.
        let mut seen = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(15);
        let pre = PreprocessConfig::default();
        let mut rerun = RerunSink::new(Transport::Off);
        while Instant::now() < deadline
            && !seen
                .iter()
                .any(|n| matches!(n, Notice::Info(s) if s.contains("resumed")))
        {
            seen.extend(m.pump());
            m.tick(&pre, &mut rerun);
            std::thread::sleep(Duration::from_millis(20));
        }
        seen.extend(pump_until(&mut m, |m| {
            m.sessions[0].is_collecting()
                && frames(m) > 20
                && m.sessions[0].shared.session_id.load(Ordering::Relaxed) == id
        }));
        assert!(
            seen.iter()
                .any(|n| matches!(n, Notice::Warn(s) if s.contains("rebooted"))),
            "{seen:?}"
        );
        assert!(
            seen.iter()
                .any(|n| matches!(n, Notice::Info(s) if s.contains("resumed"))),
            "{seen:?}"
        );
        let last = m.sessions[0].history.last().unwrap().csi.clone().unwrap();
        assert_eq!(last.session_id, id, "rejoined the original session");
        m.shutdown();
    }

    /// Unplug mid-run: the session shows offline (not an error) while the
    /// board is gone, and resumes when it is plugged back in.
    #[test]
    fn unplug_and_replug_resumes() {
        let port = "sim:41";
        let (mut m, id) = collecting_manager(port);
        let ctl = crate::sim::SimControl::of(port);
        ctl.unplug();
        pump_until(&mut m, |m| {
            m.sessions[0].conn_state == ConnState::Disconnected
        });
        // Stays offline, not "error", for as long as it's away.
        for _ in 0..40 {
            m.pump();
            assert!(
                !m.sessions[0].conn_state.is_problem(),
                "{:?}",
                m.sessions[0].conn_state
            );
            std::thread::sleep(Duration::from_millis(25));
        }
        ctl.plug();
        pump_until(&mut m, |m| {
            m.sessions[0].is_collecting()
                && m.sessions[0].shared.session_id.load(Ordering::Relaxed) == id
                && frames(m) > 10
        });
        m.shutdown();
    }

    /// A run the user stopped stays stopped across a reboot.
    #[test]
    fn stopped_run_is_not_resumed() {
        let port = "sim:42";
        let (mut m, _) = collecting_manager(port);
        m.stop_all(&[0]);
        pump_until(&mut m, |m| !m.sessions[0].is_collecting());
        crate::sim::SimControl::of(port).reboot();
        pump_until(&mut m, |m| {
            m.sessions[0].is_ready() && m.sessions[0].config_applied()
        });
        for _ in 0..50 {
            m.pump();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!m.sessions[0].is_collecting());
        assert!(!crate::sim::SimControl::of(port).collecting());
        m.shutdown();
    }
}
