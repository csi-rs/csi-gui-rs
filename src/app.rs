//! Top-level [`eframe::App`] implementation.
//!
//! [`App::new`] owns every long-lived resource — reader thread,
//! [`crate::data::History`], [`crate::rerun_sink::RerunSink`], all
//! overlays — and routes input + draws the UI each frame.

use crate::console::RawConsole;
use crate::data::{History, PreprocessConfig};
use crate::layout::{LayoutNode, MAX_PANES, SplitDirection, TilingManager, ViewType};
use crate::persistence;
use crate::protocol::{
    command::EspCommand,
    config::EspConfig,
    info::{DeviceInfo, SUPPORTED_PROTOCOL},
};
use crate::reader::{ConnState, ReaderHandles, enumerate_ports, spawn_reader};
use crate::rerun_sink::{self, RerunSink, Transport};
use crate::shared::EspSharedState;
use crate::theme::{Theme, ThemeVariant};
use crate::ui::{
    alert::Alert,
    esp_panel::{ConfigFieldBuffers, EspPanel},
    help::HelpOverlay,
    layout_picker::LayoutPicker,
    main_menu::{MainMenu, MainMenuItem},
    preprocess_overlay::{PreprocessOverlay, PreprocessOverlayResult},
    template_picker::{TemplatePicker, TemplatePickerResult},
    text_input::{TextPrompt, TextPromptPurpose, TextPromptResult},
    theme_picker::{ThemePicker, ThemePickerResult},
    view_selector::ViewSelector,
};
use crate::views::{ReplayState, ViewCtx, ViewStates, render_view};
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

/// Size of the rolling raw-packet buffer kept around so window-based
/// preprocessing filters have continuity context across the live stream.
/// Big enough to span the longest stage window we expose (moving-average
/// detrending caps at 128) without dragging memory unduly — each entry
/// is a few hundred bytes of CSI.
const PREPROCESS_CONTEXT: usize = 256;

/// Adwaita Sans Regular (SIL OFL 1.1) — vendored at
/// `assets/fonts/AdwaitaSans-Regular.ttf`. Bundled as a last-resort
/// fallback so Unicode arrows (← → ↑ ↓), play/stop glyphs (▶ ●), and
/// other symbols outside eframe's default-font coverage render
/// correctly instead of showing as the "missing glyph" tofu box.
const SYMBOL_FALLBACK_TTF: &[u8] =
    include_bytes!("../assets/fonts/AdwaitaSans-Regular.ttf");

/// Install [`SYMBOL_FALLBACK_TTF`] at the end of every default font
/// family so the existing Ubuntu / Hack fonts keep priority for ASCII
/// and only the gaps fall through.
fn install_symbol_fallback_font(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "symbol_fallback".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(SYMBOL_FALLBACK_TTF)),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push("symbol_fallback".to_owned());
    }
    ctx.set_fonts(fonts);
}

/// One connected ESP32: its reader thread, lock-free shared state, raw
/// console, connection status, identity, and the editable config pushed to
/// it. `App` holds a `Vec<DeviceSession>` so several boards can be
/// configured independently; the *active* one feeds the visualisation.
pub struct DeviceSession {
    pub port: String,
    pub reader: ReaderHandles,
    pub shared: Arc<EspSharedState>,
    pub console: Arc<RawConsole>,
    pub conn_state: ConnState,
    pub device: Option<DeviceInfo>,
    /// Editable config pushed to this device via `EspCommand::ApplyConfig`.
    pub cfg: EspConfig,
    /// Persistent channel-field edit buffer (see `ui::esp_panel`).
    pub channel_text: String,
    /// Persistent numeric-field edit buffers (see `ui::esp_panel`).
    pub field_bufs: ConfigFieldBuffers,
    /// Last config-validation error, shown inline in the panel.
    pub config_error: Option<String>,
    pub imported_recording: bool,
    pub firmware_warning_dismissed: bool,
}

impl DeviceSession {
    /// Open a serial port: spawn a dedicated reader thread and request
    /// identification. Mirrors the single-device setup in `App::new`.
    pub fn open(port: String, baud: u32) -> Self {
        let shared = EspSharedState::handle_with_baud(baud);
        let console = Arc::new(RawConsole::new(2_000));
        let reader = spawn_reader(shared.clone(), console.clone());
        if !port.is_empty() {
            let _ = reader.commands.send(EspCommand::OpenPort(port.clone()));
        }
        let cfg = EspConfig::default();
        Self {
            channel_text: cfg.channel.to_string(),
            field_bufs: ConfigFieldBuffers::from_cfg(&cfg),
            port,
            reader,
            shared,
            console,
            conn_state: ConnState::Disconnected,
            device: None,
            cfg,
            config_error: None,
            imported_recording: false,
            firmware_warning_dismissed: false,
        }
    }

    /// `true` when this device is identified and running supported firmware.
    pub fn is_ready(&self) -> bool {
        matches!(self.conn_state, ConnState::Streaming)
            && self
                .device
                .as_ref()
                .map(|d| d.protocol_supported())
                .unwrap_or(false)
    }

    /// Signal the reader thread to exit. Call before dropping the session.
    pub fn close(&self) {
        let _ = self.reader.commands.send(EspCommand::Shutdown);
    }
}

impl Drop for DeviceSession {
    fn drop(&mut self) {
        self.close();
    }
}

pub struct App {
    pub tiling: TilingManager,
    pub history: History,
    pub view_states: ViewStates,
    /// All connected devices. The `active` one feeds history/views.
    pub sessions: Vec<DeviceSession>,
    /// Index into `sessions` of the device whose stream is visualised and
    /// whose controls the ESP panel edits. `None` when no device is open.
    pub active: Option<usize>,
    /// Shared-state / console used by the views when no device is active,
    /// so `ViewCtx` and the status bar always have something to read.
    pub fallback_shared: Arc<EspSharedState>,
    pub fallback_console: Arc<RawConsole>,
    pub replay: ReplayState,
    pub rerun: RerunSink,

    pub theme_variant: ThemeVariant,
    pub theme: Theme,
    pub pending_theme_cycle: bool,

    pub help: HelpOverlay,
    pub main_menu: MainMenu,
    pub view_selector: ViewSelector,
    pub layout_picker: LayoutPicker,
    pub esp_panel: EspPanel,
    pub theme_picker: ThemePicker,
    pub preprocess_overlay: PreprocessOverlay,
    /// Modal alert used to surface import / export success and errors
    /// — much more discoverable than the log line we used to emit.
    pub alert: Alert,
    pub preprocess: PreprocessConfig,
    /// Rolling raw-packet buffer used as context for the live preprocess
    /// pipeline. Cleared whenever the user changes [`Self::preprocess`]
    /// so the new config doesn't see stale samples.
    preprocess_buffer: VecDeque<crate::data::CsiData>,
    /// Reused each pump tick to avoid allocating a fresh packet batch.
    packet_batch: Vec<crate::data::CsiData>,
    /// JSONL writer — auto-starts/stops with collection.
    recorder: persistence::SessionRecorder,
    /// When true, an active collection session streams JSONL to `exports/`.
    pub record_on_capture: bool,
    was_recording: bool,
    pub text_prompt: TextPrompt,
    pub template_picker: TemplatePicker,
    pub quit_confirm_open: bool,
    pub should_close: bool,

    pub available_ports: Vec<String>,
    pub last_port_scan: Instant,
    pub last_tick: Instant,
}

impl App {
    /// Construct the app and spawn the reader thread.
    ///
    /// - `baud` — serial baud rate. ESP-CSI-CLI defaults to 115200.
    /// - `history_size` — in-memory tick window for plots/replay (~100 ms per
    ///   tick). Full per-packet capture streams to JSONL on Start.
    /// - `rerun_transport` — Rerun sink mode. Use [`Transport::Off`] to
    ///   disable.
    ///
    /// Auto-connects to the first serial port [`enumerate_ports`] lists;
    /// the user can add more devices from the ESP panel.
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        baud: u32,
        history_size: usize,
        rerun_transport: Transport,
    ) -> Self {
        let rerun = RerunSink::new(rerun_transport);

        install_symbol_fallback_font(&cc.egui_ctx);

        let startup = persistence::load_startup_template().unwrap_or_else(|| {
            persistence::Template::from(crate::layout::TilingManager::default())
        });
        let theme_variant = ThemeVariant::from_u8(startup.tiling.theme_variant);
        let theme = Theme::from_variant(theme_variant);
        theme.apply_to(&cc.egui_ctx);

        let available_ports = enumerate_ports();
        // Open the first port as the initial device session (if any).
        let mut sessions = Vec::new();
        if let Some(port) = available_ports.first().cloned() {
            sessions.push(DeviceSession::open(port, baud));
        }
        let active = if sessions.is_empty() { None } else { Some(0) };

        Self {
            tiling: startup.tiling,
            history: History::with_capacity(history_size),
            view_states: startup.view_states,
            sessions,
            active,
            fallback_shared: EspSharedState::handle_with_baud(baud),
            fallback_console: Arc::new(RawConsole::new(1)),
            replay: ReplayState::default(),
            rerun,
            theme_variant,
            theme,
            pending_theme_cycle: false,
            help: HelpOverlay::default(),
            main_menu: MainMenu::default(),
            view_selector: ViewSelector::default(),
            layout_picker: LayoutPicker::default(),
            esp_panel: EspPanel::default(),
            theme_picker: ThemePicker::default(),
            preprocess_overlay: PreprocessOverlay::default(),
            alert: Alert::default(),
            preprocess: startup.preprocess,
            preprocess_buffer: VecDeque::with_capacity(PREPROCESS_CONTEXT),
            packet_batch: Vec::new(),
            recorder: persistence::SessionRecorder::default(),
            record_on_capture: false,
            was_recording: false,
            text_prompt: TextPrompt::default(),
            template_picker: TemplatePicker::default(),
            quit_confirm_open: false,
            should_close: false,
            available_ports,
            last_port_scan: Instant::now(),
            last_tick: Instant::now(),
        }
    }

    /// Active device session (the one feeding the views), if any.
    fn active_session(&self) -> Option<&DeviceSession> {
        self.active.and_then(|i| self.sessions.get(i))
    }

    /// Mutable active device session, if any.
    fn active_session_mut(&mut self) -> Option<&mut DeviceSession> {
        match self.active {
            Some(i) => self.sessions.get_mut(i),
            None => None,
        }
    }

    /// Shared state of the active device, or the fallback when none — so the
    /// status bar and `ViewCtx` always have something to read.
    fn cur_shared(&self) -> &Arc<EspSharedState> {
        self.active_session()
            .map(|s| &s.shared)
            .unwrap_or(&self.fallback_shared)
    }

    /// `true` when the active device is collecting.
    fn is_collecting(&self) -> bool {
        self.cur_shared().is_collecting.load(Ordering::Acquire)
    }

    /// Start/stop the JSONL session recorder when the user has enabled recording.
    fn sync_collection_recording(&mut self, imported: bool) {
        let should_record =
            self.record_on_capture && self.is_collecting() && !imported;
        if should_record && !self.was_recording {
            let label = self
                .active_session()
                .map(|s| s.port.clone())
                .unwrap_or_else(|| "session".into());
            self.start_session_recording(label);
        } else if !should_record && self.was_recording {
            if let Some(summary) = self.recorder.stop() {
                self.alert.open_info(
                    "Recording saved",
                    format!(
                        "Wrote {} packets to:\n{}",
                        summary.packets_written,
                        summary.path.display()
                    ),
                );
            } else if let Some(err) = self.recorder.last_error() {
                self.alert.open_error("Recording error", err.to_string());
            }
        }
        self.was_recording = should_record;
    }

    fn start_session_recording(&mut self, label: String) {
        match self.recorder.start(&label) {
            Ok(path) => log::info!("recording to {}", path.display()),
            Err(e) => self.alert.open_error("Recording failed", e.to_string()),
        }
    }

    fn pump_reader(&mut self) {
        // Drain state/info for EVERY session so each device's status stays
        // live. On a fresh entry into `Streaming` we push that device's full
        // config (the firmware boots in `LogMode::ArrayList`; the GUI default
        // is `Serialized`, so without a sync the two disagree).
        let mut flush_indices: Vec<usize> = Vec::new();
        for (i, s) in self.sessions.iter_mut().enumerate() {
            let prev = s.conn_state.clone();
            while let Ok(st) = s.reader.state.try_recv() {
                if s.conn_state != st {
                    s.firmware_warning_dismissed = false;
                }
                s.conn_state = st;
            }
            while let Ok(info) = s.reader.info.try_recv() {
                s.device = Some(info);
            }
            if !matches!(prev, ConnState::Streaming)
                && matches!(s.conn_state, ConnState::Streaming)
            {
                flush_indices.push(i);
            }
        }
        for i in flush_indices {
            self.flush_config_to_device(i);
        }

        if self.last_port_scan.elapsed() > Duration::from_secs(2) {
            self.available_ports = enumerate_ports();
            self.last_port_scan = Instant::now();
        }

        self.advance_replay();

        let imported = self
            .active_session()
            .map(|s| s.imported_recording)
            .unwrap_or(false);
        self.sync_collection_recording(imported);

        if self.last_tick.elapsed() < Duration::from_millis(100) {
            return;
        }

        // Non-active sessions: drain-and-discard so they can't back-pressure
        // into a permanent overflow while we're not visualising them.
        let active = self.active;
        for (i, s) in self.sessions.iter().enumerate() {
            if Some(i) != active {
                while s.reader.packets.try_recv().is_ok() {}
            }
        }

        // Active session: feed the history/views (unchanged behaviour).
        if let Some(ai) = active {
            let s = &self.sessions[ai];
            let raw_before = s.shared.frames_total.load(Ordering::Relaxed);
            self.packet_batch.clear();
            while let Ok(p) = s.reader.packets.try_recv() {
                self.packet_batch.push(p);
            }
            let raw_after = s.shared.frames_total.load(Ordering::Relaxed);
            let raw_delta = raw_after.saturating_sub(raw_before);
            let imported = s.imported_recording;

            for p in &self.packet_batch {
                self.rerun.log_packet(p);
            }
            // While replaying an imported recording we drop live packets so
            // they can't overwrite the history the user is inspecting.
            if !imported {
                let batch = std::mem::take(&mut self.packet_batch);
                let processed = self.preprocess_incoming(batch);
                if self.recorder.is_recording() {
                    self.recorder.append_batch(&processed);
                }
                if let Some(stats) = self.history.push_tick(processed, raw_delta) {
                    self.rerun.log_tick(stats);
                }
            }
        }
        self.last_tick = Instant::now();
    }

    /// Switch the visualised device. Clears the rolling history + preprocess
    /// buffer so the two devices' streams don't mix in the plots.
    fn set_active(&mut self, idx: Option<usize>) {
        if self.active == idx {
            return;
        }
        self.active = idx;
        self.history.clear();
        self.preprocess_buffer.clear();
    }

    /// Bake the active [`crate::data::PreprocessConfig`] into freshly
    /// captured packets. The pre-process buffer holds the most recent
    /// `PREPROCESS_CONTEXT` raw packets so window-based filters
    /// (least-squares baseline, moving-average detrending, per-window
    /// scaling) have signal continuity, while history rows already
    /// committed stay untouched — toggling a filter never rewrites the
    /// past.
    fn preprocess_incoming(&mut self, batch: Vec<crate::data::CsiData>) -> Vec<crate::data::CsiData> {
        if !self.preprocess.is_active() || batch.is_empty() {
            return batch;
        }
        let n_new = batch.len();
        for p in &batch {
            self.preprocess_buffer.push_back(p.clone());
        }
        while self.preprocess_buffer.len() > PREPROCESS_CONTEXT {
            self.preprocess_buffer.pop_front();
        }
        let raw_window: Vec<crate::data::CsiData> =
            self.preprocess_buffer.iter().cloned().collect();
        let processed = self.preprocess.apply(&raw_window);
        // Pipeline is cardinality-preserving; the tail `n_new` entries
        // correspond to the packets we just pushed.
        let take = n_new.min(processed.len());
        processed[processed.len() - take..].to_vec()
    }

    fn advance_replay(&mut self) {
        if !self.replay.playing {
            return;
        }
        let (Some(first), Some(last)) = (self.history.first_id(), self.history.last_id()) else {
            self.replay.playing = false;
            return;
        };
        // Source rate is one packet per ~TICK_MS (100ms), so 1.0× = 10 pkts/s.
        let speed = self.replay.speed.max(0.05);
        let step_ms = (100.0 / speed).max(8.0);
        let elapsed = self.replay.last_advance.elapsed().as_millis() as f32;
        if elapsed < step_ms {
            return;
        }
        let steps = (elapsed / step_ms).floor() as u64;
        let cur = self.replay.anchor.unwrap_or(first);
        let next = cur.saturating_add(steps);
        if next >= last {
            self.replay.anchor = None; // reached the head → snap to LIVE and stop
            self.replay.playing = false;
        } else {
            self.replay.anchor = Some(next.max(first));
        }
        self.replay.last_advance = Instant::now();
    }

    /// Push session `i`'s config to that device via `EspCommand::ApplyConfig`.
    /// Called automatically on every transition into [`ConnState::Streaming`]
    /// so the firmware's runtime state matches the GUI's view from the first
    /// packet — otherwise the firmware boots in its own defaults
    /// (`LogMode::ArrayList`, etc.) and the GUI parses the wrong wire format
    /// until the user clicks Apply.
    fn flush_config_to_device(&mut self, i: usize) {
        if let Some(s) = self.sessions.get(i) {
            if let Some(info) = s.device.as_ref() {
                let cfg = s.cfg.clone();
                let _ = s.reader.commands.send(EspCommand::ApplyConfig {
                    cfg,
                    chip: info.chip,
                });
            }
        }
    }

    /// Walk the user through a CSI-capture import.
    ///
    /// Pops a native file dialog filtered to the supported export
    /// extensions, validates the result against the current collection
    /// state, runs the dispatcher in [`persistence::import_history`],
    /// and surfaces success / failure in the modal [`Alert`] so the
    /// user actually sees what happened. After a successful load the
    /// reader's outgoing packet channel is drained so an in-flight
    /// packet from before the import can't slip into the freshly
    /// loaded history on the next pump tick.
    fn run_import_flow(&mut self) {
        if self.is_collecting() {
            self.alert.open_error(
                "Import unavailable",
                "Stop the live collection before importing a capture.",
            );
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .set_title("Import CSI capture")
            .add_filter("CSI captures", &["csv", "jsonl", "json", "ndjson", "parquet", "pq"])
            .add_filter("All files", &["*"])
            .pick_file()
        else {
            return; // User cancelled — nothing to do.
        };
        if !path.exists() {
            self.alert.open_error(
                "Import failed",
                format!("File not found:\n{}", path.display()),
            );
            return;
        }
        match persistence::import_history(&path) {
            Ok(pkts) if pkts.is_empty() => {
                self.alert.open_error(
                    "Import failed",
                    format!(
                        "No decoded packets in:\n{}\n\nThe file may be empty or in an \
                         unrecognised format. Supported extensions: .csv / .jsonl / .parquet.",
                        path.display()
                    ),
                );
            }
            Ok(pkts) => {
                let count = pkts.len();
                self.history.load_from_packets(pkts);
                self.preprocess_buffer.clear();
                if let Some(s) = self.active_session() {
                    while s.reader.packets.try_recv().is_ok() {}
                }
                self.replay.anchor = self.history.first_id();
                self.replay.playing = false;
                // Lock down the live pump while the imported recording
                // is the source of truth — the user can leave this
                // mode with the `R` key (snap to LIVE) or by starting
                // a new collection.
                if let Some(s) = self.active_session_mut() {
                    s.imported_recording = true;
                }
                self.alert.open_info(
                    "Import complete",
                    format!(
                        "Loaded {count} packets from\n{}\n\nReplay anchor set to packet 0. \
                         Live collection is locked until you press R or start a new session.",
                        path.display()
                    ),
                );
            }
            Err(e) => {
                self.alert.open_error(
                    "Import failed",
                    format!("{}\n\n{e}", path.display()),
                );
            }
        }
    }

    /// `true` when the reader has identified an ESP32 running a supported
    /// `esp-csi-cli-rs` build. Used to gate "Start Collection" so the user
    /// can't kick a session off against an absent or unrecognised device.
    /// Stop is always allowed (so the local "collecting" state can be
    /// cleared even if connectivity changes mid-session).
    fn cycle_theme(&mut self, ctx: &egui::Context) {
        self.theme_variant = self.theme_variant.next();
        self.theme = Theme::from_variant(self.theme_variant);
        self.tiling.theme_variant = self.theme_variant.as_u8();
        self.theme.apply_to(ctx);
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        let any_overlay = self.help.open
            || self.main_menu.open
            || self.view_selector.open
            || self.layout_picker.open
            || self.text_prompt.open
            || self.template_picker.open
            || self.theme_picker.open
            || self.preprocess_overlay.open
            || self.alert.open
            || self.quit_confirm_open;
        if any_overlay {
            if self.quit_confirm_open {
                self.handle_quit_confirm(ctx);
            }
            return;
        }

        if ctx.egui_wants_keyboard_input() {
            return;
        }

        let shift = ctx.input(|i| i.modifiers.shift);

        ctx.input(|i| {
            if i.key_pressed(egui::Key::Q) {
                self.quit_confirm_open = true;
            }
            if i.key_pressed(egui::Key::H) {
                self.help.open = !self.help.open;
            }
            if i.key_pressed(egui::Key::M) {
                self.main_menu.open = !self.main_menu.open;
            }
            if i.key_pressed(egui::Key::T) {
                self.pending_theme_cycle = true;
            }
            if i.key_pressed(egui::Key::Tab) {
                self.tiling.focus_next();
            }
            if i.key_pressed(egui::Key::Delete) {
                self.tiling.close_focused();
            }
            if i.key_pressed(egui::Key::Space) {
                self.tiling.toggle_fullscreen();
            }
            if i.key_pressed(egui::Key::Enter) {
                self.view_selector.open = true;
                self.view_selector.selected = 0;
            }
            if !shift && i.key_pressed(egui::Key::R) {
                // R = return all panes to LIVE. Also exits import-replay
                // mode so newly captured packets can flow into history
                // again.
                self.replay.anchor = None;
                self.replay.playing = false;
                if let Some(s) = self.active_session_mut() {
                    s.imported_recording = false;
                }
            }
            #[cfg(feature = "rerun")]
            if shift && i.key_pressed(egui::Key::R) {
                self.rerun.toggle_live();
            }
            #[cfg(feature = "rerun")]
            if shift && i.key_pressed(egui::Key::L) {
                self.rerun.toggle_rrd();
            }
            for (k, id) in [
                (egui::Key::Num1, 1),
                (egui::Key::Num2, 2),
                (egui::Key::Num3, 3),
                (egui::Key::Num4, 4),
                (egui::Key::Num5, 5),
                (egui::Key::Num6, 6),
                (egui::Key::Num7, 7),
                (egui::Key::Num8, 8),
                (egui::Key::Num9, 9),
                (egui::Key::Num0, 10),
            ] {
                if i.key_pressed(k) {
                    self.tiling.focus_by_id(id);
                }
            }

            if shift {
                // Split shortcuts must NOT auto-repeat: egui's
                // `key_pressed` is true on every OS-key-repeat event,
                // so holding Shift+Right would spam splits until
                // MAX_PANES capped out, leaving the user with N tiny
                // overlapping panes. Filter the event stream for the
                // initial press only (`repeat: false`).
                if key_first_pressed(i, egui::Key::ArrowLeft)
                    || key_first_pressed(i, egui::Key::ArrowRight)
                {
                    self.tiling.split(SplitDirection::Horizontal);
                }
                if key_first_pressed(i, egui::Key::ArrowUp)
                    || key_first_pressed(i, egui::Key::ArrowDown)
                {
                    self.tiling.split(SplitDirection::Vertical);
                }
            } else if let (Some(first), Some(last)) =
                (self.history.first_id(), self.history.last_id())
            {
                // Arrow keys scrub the global replay anchor regardless of which
                // pane is focused; that's the "global anchor" model.
                if last > first {
                    let cur = self.replay.anchor.unwrap_or(last);
                    if i.key_pressed(egui::Key::ArrowLeft) {
                        let next = cur.saturating_sub(1).max(first);
                        self.replay.anchor = Some(next);
                        self.replay.playing = false;
                    }
                    if i.key_pressed(egui::Key::ArrowRight) {
                        if cur >= last {
                            self.replay.anchor = None; // snap to LIVE
                        } else {
                            self.replay.anchor = Some(cur + 1);
                        }
                        self.replay.playing = false;
                    }
                }
            }
        });

        if self.pending_theme_cycle {
            self.cycle_theme(ctx);
            self.pending_theme_cycle = false;
        }
    }

    fn handle_quit_confirm(&mut self, ctx: &egui::Context) {
        let mut close = false;
        let mut confirm = false;
        egui::Window::new("Quit?")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("Really quit?");
                ui.horizontal(|ui| {
                    if ui.button("Yes").clicked() {
                        confirm = true;
                    }
                    if ui.button("No").clicked() {
                        close = true;
                    }
                });
                ctx.input(|i| {
                    if i.key_pressed(egui::Key::Y)
                        || i.key_pressed(egui::Key::Enter)
                        || i.key_pressed(egui::Key::Space)
                    {
                        confirm = true;
                    }
                    if i.key_pressed(egui::Key::N)
                        || i.key_pressed(egui::Key::Q)
                        || i.key_pressed(egui::Key::Escape)
                    {
                        close = true;
                    }
                });
            });
        if confirm {
            self.should_close = true;
        }
        if close || confirm {
            self.quit_confirm_open = false;
        }
    }

    fn handle_overlays(&mut self, ctx: &egui::Context) {
        self.help.show(ctx);
        self.alert.show(ctx);

        let is_collecting = self.is_collecting();

        if let Some(item) = self.main_menu.show(ctx, is_collecting) {
            match item {
                MainMenuItem::Theme => {
                    self.theme_picker.open = true;
                    self.theme_picker.variant = self.theme_variant;
                }
                MainMenuItem::SaveTemplate => {
                    self.text_prompt.open = true;
                    self.text_prompt.title = "Save Template".into();
                    self.text_prompt.label = "Template name:".into();
                    self.text_prompt.purpose = TextPromptPurpose::SaveTemplate;
                }
                MainMenuItem::LoadTemplate => {
                    self.template_picker.open = true;
                    self.template_picker.refresh();
                }
                MainMenuItem::ExportData => {
                    self.text_prompt.open = true;
                    self.text_prompt.title = "Export Data".into();
                    self.text_prompt.label = "Filename (timestamp + extension appended):".into();
                    self.text_prompt.purpose = TextPromptPurpose::ExportData;
                    self.text_prompt.export_format = persistence::ExportFormat::Csv;
                }
                MainMenuItem::ImportData => self.run_import_flow(),
                MainMenuItem::Preprocess => {
                    self.preprocess_overlay.cfg = self.preprocess.clone();
                    self.preprocess_overlay.open = true;
                }
                MainMenuItem::Close => {}
            }
        }

        if let Some(view) = self.view_selector.show(ctx) {
            self.tiling.set_view(view);
        }

        if let Some(preset) = self.layout_picker.show(ctx) {
            self.tiling.apply_preset(preset);
        }

        match self.theme_picker.show(ctx) {
            ThemePickerResult::Pick(variant) => {
                self.theme_variant = variant;
                self.theme = Theme::from_variant(variant);
                self.tiling.theme_variant = variant.as_u8();
                self.theme.apply_to(ctx);
            }
            ThemePickerResult::Cancel | ThemePickerResult::None => {}
        }

        match self.preprocess_overlay.show(ctx) {
            PreprocessOverlayResult::Apply(cfg) => {
                self.preprocess = cfg;
                // New filter settings should not "see" packets that
                // arrived before they were enabled — clear the rolling
                // context so the next batch starts fresh.
                self.preprocess_buffer.clear();
            }
            PreprocessOverlayResult::Reset => {
                self.preprocess = crate::data::PreprocessConfig::default();
                self.preprocess_buffer.clear();
            }
            PreprocessOverlayResult::Cancel | PreprocessOverlayResult::None => {}
        }

        match self.text_prompt.show(ctx) {
            TextPromptResult::Submit(name, TextPromptPurpose::SaveTemplate) => {
                // Bundle the layout, the active preprocessing pipeline,
                // and every per-pane view-state knob so reloading the
                // template restores the user's whole workspace, not
                // just the tiling skeleton.
                let tpl = persistence::Template {
                    tiling: self.tiling.clone(),
                    preprocess: self.preprocess.clone(),
                    view_states: self.view_states.clone(),
                };
                let _ = persistence::save_template(&name, &tpl);
            }
            TextPromptResult::SubmitExport(name, fmt) => {
                if self.recorder.is_recording() {
                    if let Some(path) = self.recorder.path() {
                        self.alert.open_info(
                            "Recording in progress",
                            format!(
                                "Per-packet JSONL is being written to:\n{}\n\n{} packets so far. \
                                 Stop collection to finalize the file.",
                                path.display(),
                                self.recorder.packets_written()
                            ),
                        );
                    }
                } else if let Some(rec) = self.recorder.last_recording() {
                    self.alert.open_info(
                        "Session recording",
                        format!(
                            "Full per-packet JSONL capture:\n{}\n\n{} packets. \
                             Use Import Data (Replay) to load this file.",
                            rec.path.display(),
                            rec.packets_written
                        ),
                    );
                } else if !self.history.raw_history.is_empty() {
                    let raw: Vec<_> = self.history.raw_history.iter().cloned().collect();
                    let stem = filename_stem_for_export(&name);
                    match persistence::export_history(&stem, &raw, fmt) {
                        Ok(path) => self.alert.open_info(
                            "Export complete",
                            format!("Wrote {}", path.display()),
                        ),
                        Err(e) => self.alert.open_error("Export failed", e.to_string()),
                    }
                } else {
                    self.alert.open_info(
                        "Nothing to export",
                        "Enable Record to JSONL during collection, \
                         or import a capture to re-export.",
                    );
                }
            }
            TextPromptResult::Submit(name, TextPromptPurpose::RerunConnect) => {
                self.rerun.set_transport(parse_host_port(&name));
            }
            TextPromptResult::Submit(name, TextPromptPurpose::RerunFile) => {
                let unix = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let trimmed = name.trim();
                let leaf = if trimmed.is_empty() {
                    format!("csi_{unix}.rrd")
                } else if trimmed.ends_with(".rrd") {
                    trimmed.to_string()
                } else {
                    format!("{trimmed}_{unix}.rrd")
                };
                self.rerun.set_transport(Transport::File {
                    path: std::path::PathBuf::from("logs").join(leaf),
                });
            }
            TextPromptResult::Submit(name, TextPromptPurpose::RerunExport) => {
                let unix = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let trimmed = name.trim();
                let leaf = if trimmed.is_empty() {
                    format!("history_{unix}.rrd")
                } else if trimmed.ends_with(".rrd") {
                    trimmed.to_string()
                } else {
                    format!("{trimmed}_{unix}.rrd")
                };
                let path = std::path::PathBuf::from("exports").join(leaf);
                if let Err(e) = rerun_sink::export_history_to_rrd(&self.history, &path) {
                    log::error!("rerun history export failed: {e}");
                }
            }
            // Submit-for-ExportData is routed through SubmitExport; the
            // path below should never fire but stays exhaustive for the
            // compiler.
            TextPromptResult::Submit(_, TextPromptPurpose::None)
            | TextPromptResult::Submit(_, TextPromptPurpose::ExportData)
            | TextPromptResult::Cancel
            | TextPromptResult::None => {}
        }

        match self.template_picker.show(ctx) {
            TemplatePickerResult::Load(path) => {
                if let Ok(tpl) = persistence::read_template(&path) {
                    self.tiling = tpl.tiling;
                    self.preprocess = tpl.preprocess;
                    self.preprocess_buffer.clear();
                    self.view_states = tpl.view_states;
                    self.theme_variant = ThemeVariant::from_u8(self.tiling.theme_variant);
                    self.theme = Theme::from_variant(self.theme_variant);
                    self.theme.apply_to(ctx);
                }
            }
            TemplatePickerResult::MakeDefault(name) => {
                let _ = persistence::set_default(&name);
                self.template_picker.refresh();
            }
            TemplatePickerResult::Cancel | TemplatePickerResult::None => {}
        }
    }

    fn render_tree(&mut self, ui: &mut egui::Ui) {
        if self.tiling.fullscreen {
            let focused = self.tiling.focused;
            let view = self.tiling.focused_view();
            self.render_pane(ui, focused, view);
            return;
        }
        let root = self.tiling.root.clone();
        self.render_node(ui, &root);
    }

    fn render_node(&mut self, ui: &mut egui::Ui, node: &LayoutNode) {
        match node {
            LayoutNode::Pane { id, view } => self.render_pane(ui, *id, *view),
            LayoutNode::Split {
                direction,
                ratio,
                children,
            } => {
                let rect = ui.available_rect_before_wrap();
                let frac = (*ratio as f32 / 100.0).clamp(0.10, 0.90);
                match direction {
                    SplitDirection::Horizontal => {
                        let split = rect.left() + rect.width() * frac;
                        let left =
                            egui::Rect::from_min_max(rect.min, egui::pos2(split, rect.bottom()));
                        let right =
                            egui::Rect::from_min_max(egui::pos2(split, rect.top()), rect.max);
                        if let Some(c) = children.first() {
                            self.render_child(ui, c, left);
                        }
                        if let Some(c) = children.get(1) {
                            self.render_child(ui, c, right);
                        }
                    }
                    SplitDirection::Vertical => {
                        let split = rect.top() + rect.height() * frac;
                        let top =
                            egui::Rect::from_min_max(rect.min, egui::pos2(rect.right(), split));
                        let bot =
                            egui::Rect::from_min_max(egui::pos2(rect.left(), split), rect.max);
                        if let Some(c) = children.first() {
                            self.render_child(ui, c, top);
                        }
                        if let Some(c) = children.get(1) {
                            self.render_child(ui, c, bot);
                        }
                    }
                }
            }
        }
    }

    /// Render one child subtree confined to `rect`. The clip rect is
    /// intersected with the parent's so a pane can never paint over its
    /// neighbour, even if its content is wider/taller than the column.
    fn render_child(&mut self, ui: &mut egui::Ui, node: &LayoutNode, rect: egui::Rect) {
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            ui.set_clip_rect(rect.intersect(ui.clip_rect()));
            self.render_node(ui, node);
        });
    }

    fn render_menu_bar(&mut self, ui: &mut egui::Ui) {
        let is_collecting = self.is_collecting();
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Save Template…").clicked() {
                    self.text_prompt.open = true;
                    self.text_prompt.title = "Save Template".into();
                    self.text_prompt.label = "Template name:".into();
                    self.text_prompt.purpose = TextPromptPurpose::SaveTemplate;
                    ui.close();
                }
                if ui.button("Load Template…").clicked() {
                    self.template_picker.open = true;
                    self.template_picker.refresh();
                    ui.close();
                }
                ui.separator();
                if ui.button("Export Data…").clicked() {
                    self.text_prompt.open = true;
                    self.text_prompt.title = "Export Data".into();
                    self.text_prompt.label =
                        "Filename (timestamp + extension appended):".into();
                    self.text_prompt.purpose = TextPromptPurpose::ExportData;
                    self.text_prompt.export_format = persistence::ExportFormat::Csv;
                    ui.close();
                }
                // Importing rewinds the history buffer with a captured
                // recording — block it while a live session is active
                // so the user doesn't accidentally clobber data they're
                // still collecting.
                let import_enabled = !is_collecting;
                let import_btn =
                    ui.add_enabled(import_enabled, egui::Button::new("Import Data… (Replay)"));
                let import_btn = if !import_enabled {
                    import_btn.on_disabled_hover_text("Stop the live collection first.")
                } else {
                    import_btn
                };
                if import_btn.clicked() {
                    self.run_import_flow();
                    ui.close();
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    self.quit_confirm_open = true;
                    ui.close();
                }
            });

            ui.menu_button("View", |ui| {
                if ui.button("Theme…").clicked() {
                    self.theme_picker.variant = self.theme_variant;
                    self.theme_picker.open = true;
                    ui.close();
                }
                if ui.button("Layouts…").clicked() {
                    self.layout_picker.open = true;
                    self.layout_picker.selected = 0;
                    ui.close();
                }
                if ui.button("Select View…").clicked() {
                    self.view_selector.open = true;
                    self.view_selector.selected = 0;
                    ui.close();
                }
                ui.separator();
                if ui.button("Fullscreen Focused").clicked() {
                    self.tiling.toggle_fullscreen();
                    ui.close();
                }
                if ui.button("Split Horizontal").clicked() {
                    self.tiling.split(SplitDirection::Horizontal);
                    ui.close();
                }
                if ui.button("Split Vertical").clicked() {
                    self.tiling.split(SplitDirection::Vertical);
                    ui.close();
                }
                if ui.button("Close Focused Pane").clicked() {
                    self.tiling.close_focused();
                    ui.close();
                }
            });

            #[cfg(feature = "rerun")]
            ui.menu_button("Rerun", |ui| {
                ui.label(
                    egui::RichText::new(format!("transport: {}", self.rerun.transport().label()))
                        .weak()
                        .small(),
                );
                ui.separator();
                if ui
                    .selectable_label(matches!(self.rerun.transport(), Transport::Off), "Off")
                    .clicked()
                {
                    self.rerun.set_transport(Transport::Off);
                    ui.close();
                }
                if ui
                    .selectable_label(
                        matches!(self.rerun.transport(), Transport::Spawn),
                        "Spawn viewer",
                    )
                    .clicked()
                {
                    self.rerun.set_transport(Transport::Spawn);
                    ui.close();
                }
                if ui.button("Connect to viewer…").clicked() {
                    self.text_prompt.open = true;
                    self.text_prompt.title = "Connect to Rerun".into();
                    self.text_prompt.label = "host:port (default 127.0.0.1:9876):".into();
                    self.text_prompt.buffer = "127.0.0.1:9876".into();
                    self.text_prompt.purpose = TextPromptPurpose::RerunConnect;
                    ui.close();
                }
                if ui.button("Record to file…").clicked() {
                    self.text_prompt.open = true;
                    self.text_prompt.title = "Rerun record to RRD".into();
                    self.text_prompt.label = "filename (relative to logs/):".into();
                    self.text_prompt.buffer = String::new();
                    self.text_prompt.purpose = TextPromptPurpose::RerunFile;
                    ui.close();
                }
                ui.separator();
                let live = self.rerun.is_live();
                if ui
                    .checkbox(&mut live.clone(), "Live streaming  [Shift+R]")
                    .clicked()
                {
                    self.rerun.toggle_live();
                    ui.close();
                }
                let rec = self.rerun.is_recording();
                if ui
                    .checkbox(&mut rec.clone(), "RRD recording  [Shift+L]")
                    .clicked()
                {
                    self.rerun.toggle_rrd();
                    ui.close();
                }
                ui.separator();
                if ui.button("Export history to RRD…").clicked() {
                    self.text_prompt.open = true;
                    self.text_prompt.title = "Export history to RRD".into();
                    self.text_prompt.label = "filename (relative to exports/):".into();
                    self.text_prompt.buffer = String::new();
                    self.text_prompt.purpose = TextPromptPurpose::RerunExport;
                    ui.close();
                }
                if let Some(err) = self.rerun.last_error() {
                    ui.separator();
                    ui.colored_label(egui::Color32::from_rgb(220, 80, 80), err);
                }
            });

            ui.menu_button("Help", |ui| {
                if ui.button("Keyboard Shortcuts").clicked() {
                    self.help.open = true;
                    ui.close();
                }
                if ui.button("Main Menu (M)").clicked() {
                    self.main_menu.open = true;
                    ui.close();
                }
            });

            // Right edge of the menu bar: the ESP-panel toggle (rightmost)
            // and a COMPACT connection indicator to its left. The full
            // status string is long ("Unknown firmware (no esp-csi-cli-rs
            // banner)") and would overrun the menu buttons when the ESP side
            // panel narrows the window, so only a short badge lives here —
            // the full text is on hover and in the bottom status bar.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let arrow = if self.esp_panel.open { "▸" } else { "◂" };
                if ui
                    .button(format!("{arrow} ESP"))
                    .on_hover_text("Toggle the ESP control panel")
                    .clicked()
                {
                    self.esp_panel.open = !self.esp_panel.open;
                }
                ui.separator();
                let state = self
                    .active_session()
                    .map(|s| s.conn_state.clone())
                    .unwrap_or(ConnState::Disconnected);
                let (color, short) = conn_state_short(&state);
                ui.colored_label(color, short)
                    .on_hover_text(conn_state_summary(&state).1);
            });
        });
    }

    fn render_status_bar(&mut self, ui: &mut egui::Ui) {
        let is_collecting = self.is_collecting();
        // Snapshot the active device's state up front so the closure below
        // doesn't fight the borrow checker over `self`.
        let n_devices = self.sessions.len();
        let dev_index = self.active.map(|i| i + 1).unwrap_or(0);
        let (port, state, device, log_mode_id, imported) = match self.active_session() {
            Some(s) => (
                s.port.clone(),
                s.conn_state.clone(),
                s.device.clone(),
                s.shared.log_mode_id.load(Ordering::Acquire),
                s.imported_recording,
            ),
            None => (
                String::new(),
                ConnState::Disconnected,
                None,
                self.fallback_shared.log_mode_id.load(Ordering::Acquire),
                false,
            ),
        };
        // Wrapped so the status items reflow onto a second line when the ESP
        // side panel narrows the window, rather than overrunning each other.
        ui.horizontal_wrapped(|ui| {
            if n_devices > 1 {
                ui.label(
                    egui::RichText::new(format!("Dev {dev_index}/{n_devices}"))
                        .weak()
                        .small(),
                );
                ui.separator();
            }
            ui.label(egui::RichText::new("Port").weak().small());
            let port_display = if port.is_empty() {
                "—".to_string()
            } else {
                port.clone()
            };
            ui.label(port_display);
            ui.separator();

            let (color, label) = conn_state_summary(&state);
            ui.colored_label(color, label);
            ui.separator();

            match &device {
                Some(info) => {
                    let mac = info
                        .mac
                        .as_deref()
                        .map(|m| format!(" · {m}"))
                        .unwrap_or_default();
                    ui.label(format!(
                        "{} {} · {} · proto {}{}",
                        info.name,
                        info.version,
                        info.chip.label(),
                        info.protocol,
                        mac,
                    ));
                }
                None => {
                    ui.colored_label(
                        egui::Color32::from_rgb(220, 80, 80),
                        "not identified",
                    );
                }
            }
            ui.separator();

            let log_mode = match log_mode_id {
                0 => "serialized",
                1 => "text",
                2 => "array-list",
                3 => "esp-csi-tool",
                _ => "?",
            };
            ui.label(format!("log: {log_mode}"));
            ui.separator();

            if is_collecting {
                ui.colored_label(
                    egui::Color32::from_rgb(80, 220, 110),
                    "collecting",
                );
            } else {
                ui.label(egui::RichText::new("idle").weak());
            }

            if let Some(anchor) = self.replay.anchor {
                ui.separator();
                let badge = if imported {
                    "REPLAY · imported"
                } else {
                    "replay"
                };
                ui.colored_label(
                    egui::Color32::from_rgb(220, 180, 60),
                    format!("{badge} @ #{anchor}"),
                );
                // Always-visible, accent-coloured exit-replay button so
                // the user doesn't have to remember the `R` shortcut.
                let exit_label = if imported { "Exit Replay" } else { "Live" };
                let resp = ui.add(
                    egui::Button::new(
                        egui::RichText::new(exit_label)
                            .color(egui::Color32::WHITE)
                            .strong(),
                    )
                    .fill(egui::Color32::from_rgb(160, 110, 30)),
                );
                if resp.clicked() {
                    self.replay.anchor = None;
                    self.replay.playing = false;
                    if let Some(s) = self.active_session_mut() {
                        s.imported_recording = false;
                    }
                }
            }

            let history_used = self.history.entries.len();
            let history_cap = self.history.capacity();
            ui.separator();
            ui.label(
                egui::RichText::new(format!("history {history_used}/{history_cap}"))
                    .weak()
                    .small(),
            );

            if self.preprocess.is_active() {
                ui.separator();
                ui.label(
                    egui::RichText::new(format!("preprocess: {}", self.preprocess.summary()))
                        .weak()
                        .small(),
                )
                .on_hover_text("M → Preprocessing… to edit filters");
            }

            if self.recorder.is_recording() {
                ui.separator();
                let n = self.recorder.packets_written();
                let path_hint = self
                    .recorder
                    .path()
                    .and_then(|p| p.file_name())
                    .and_then(|s| s.to_str())
                    .unwrap_or("…");
                ui.label(
                    egui::RichText::new(format!("record: {n} pkts → {path_hint}"))
                        .weak()
                        .small(),
                );
            }

            #[cfg(feature = "rerun")]
            {
                ui.separator();
                let (rcol, rtext) = self.rerun.status_summary();
                let resp = ui.colored_label(rcol, rtext);
                if let Some(err) = self.rerun.last_error() {
                    resp.on_hover_text(err);
                }
            }

            ui.separator();
            ui.label(
                egui::RichText::new("M menu · H help · Enter pick view")
                    .weak()
                    .small(),
            );
        });
    }

    fn render_firmware_warning(&mut self, ctx: &egui::Context) {
        let (state, port, dismissed) = match self.active_session() {
            Some(s) => (s.conn_state.clone(), s.port.clone(), s.firmware_warning_dismissed),
            None => return,
        };
        let needs_warning = matches!(
            &state,
            ConnState::UnknownFirmware | ConnState::UnsupportedProtocol(_)
        );
        if !needs_warning || dismissed {
            return;
        }
        let mut do_retry = false;
        let mut do_dismiss = false;
        let mut open_config_menu = false;
        egui::Window::new("ESP firmware not recognized")
            .collapsible(false)
            .resizable(false)
            .default_width(560.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, -40.0))
            .show(ctx, |ui| {
                match &state {
                    ConnState::UnknownFirmware => {
                        ui.colored_label(
                            egui::Color32::from_rgb(230, 80, 80),
                            "No esp-csi-cli-rs banner was received from this device after reset.",
                        );
                        ui.label(format!("Port: {}", port));
                        ui.add_space(4.0);
                        ui.label(
                            "This GUI resets the ESP via DTR/RTS on connect and expects the \
                             firmware to emit `ESP-CSI-CLI/<version>` as its first banner line. \
                             The device on this port either:",
                        );
                        ui.label("  • is not running esp-csi-cli-rs (please flash it), or");
                        ui.label(
                            "  • is on a different serial port (use the ESP panel's + Add menu), or",
                        );
                        ui.label("  • did not respond to the DTR/RTS reset — try Retry / Reset.");
                    }
                    ConnState::UnsupportedProtocol(p) => {
                        ui.colored_label(
                            egui::Color32::from_rgb(230, 80, 80),
                            format!(
                                "Device firmware reports protocol={}, but this GUI \
                                 understands protocol={}.",
                                p, SUPPORTED_PROTOCOL
                            ),
                        );
                        ui.label(
                            "Reflash the ESP32 with a matching esp-csi-cli-rs build, \
                             or update this GUI.",
                        );
                    }
                    _ => {}
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Retry / Reset").clicked() {
                        do_retry = true;
                    }
                    if ui.button("Pick another port…").clicked() {
                        open_config_menu = true;
                    }
                    if ui.button("Dismiss").clicked() {
                        do_dismiss = true;
                    }
                });
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(
                        "(banner regex: ^ESP-CSI-CLI/<version>$ — printed on every reset)",
                    )
                    .weak()
                    .small(),
                );
            });
        if do_retry {
            self.available_ports = enumerate_ports();
            if let Some(s) = self.active_session() {
                let _ = s.reader.commands.send(EspCommand::Reset);
            }
        }
        if do_dismiss {
            if let Some(s) = self.active_session_mut() {
                s.firmware_warning_dismissed = true;
            }
        }
        if open_config_menu {
            if let Some(s) = self.active_session_mut() {
                s.firmware_warning_dismissed = true;
            }
            self.esp_panel.open = true;
            self.available_ports = enumerate_ports();
        }
    }

    /// Draw the docked, collapsible ESP control panel (right side). This
    /// is the single home for every device action that used to live in the
    /// top `ESP` menu and the floating Configure modal: port selection,
    /// connection status, Start/Stop, the full `set-*` config grid, and the
    /// raw diagnostic commands.
    fn render_esp_panel(&mut self, ui: &mut egui::Ui) {
        let is_collecting = self.is_collecting();

        // The firmware locks the CLI while collecting (only `q` is
        // honoured), so `set-*` / `show-*` / raw commands would be silently
        // discarded. Disable those controls and explain why on hover.
        let cli_disabled_hint =
            "Stop the live collection first — the firmware CLI is locked while \
             IS_COLLECTING is true.";

        ui.add_space(4.0);
        ui.heading("ESP32 Devices");
        ui.separator();

        // ---- Device selector: pick / add / remove connected boards ----
        let mut select: Option<usize> = None;
        let mut add_port: Option<String> = None;
        let mut remove_active = false;
        let mut rescan = false;
        ui.horizontal_wrapped(|ui| {
            for (i, s) in self.sessions.iter().enumerate() {
                let (col, short) = conn_state_short(&s.conn_state);
                let chip = s.device.as_ref().map(|d| d.chip.label()).unwrap_or("?");
                let label =
                    egui::RichText::new(format!("{} · {} · {}", s.port, chip, short)).color(col);
                if ui
                    .selectable_label(self.active == Some(i), label)
                    .clicked()
                {
                    select = Some(i);
                }
            }
            let used: Vec<String> = self.sessions.iter().map(|s| s.port.clone()).collect();
            ui.menu_button("+ Add", |ui| {
                let free: Vec<String> = self
                    .available_ports
                    .iter()
                    .filter(|p| !used.contains(p))
                    .cloned()
                    .collect();
                if free.is_empty() {
                    ui.label(egui::RichText::new("(no free ports)").weak());
                }
                for p in free {
                    if ui.button(&p).clicked() {
                        add_port = Some(p);
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button("Rescan ports").clicked() {
                    rescan = true;
                    ui.close();
                }
            });
            if self.active.is_some()
                && ui
                    .button("✕")
                    .on_hover_text("Disconnect the selected device")
                    .clicked()
            {
                remove_active = true;
            }
        });
        if rescan {
            self.available_ports = enumerate_ports();
        }
        if let Some(i) = select {
            self.set_active(Some(i));
        }
        if let Some(p) = add_port {
            let baud = self.fallback_shared.baud.load(Ordering::Relaxed);
            self.sessions.push(DeviceSession::open(p, baud));
            self.set_active(Some(self.sessions.len() - 1));
        }
        if remove_active {
            if let Some(i) = self.active {
                self.sessions[i].close();
                self.sessions.remove(i);
                let new = if self.sessions.is_empty() {
                    None
                } else {
                    Some(i.min(self.sessions.len() - 1))
                };
                self.set_active(new);
            }
        }

        ui.separator();

        let Some(ai) = self.active else {
            ui.label("Add a device to begin.");
            return;
        };

        // Clone the command sender so we can send while mutably borrowing the
        // session for config edits. Snapshot display-only state up front.
        let cmd = self.sessions[ai].reader.commands.clone();
        let state = self.sessions[ai].conn_state.clone();
        let device = self.sessions[ai].device.clone();
        let device_ready = self.sessions[ai].is_ready();
        let imported = self.sessions[ai].imported_recording;

        // Intents applied after rendering (need `&mut self` at App level).
        let mut do_flush = false;
        let mut mark_boundary = false;
        let mut clear_imported = false;

        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::CollapsingHeader::new("Connection")
                .default_open(true)
                .show(ui, |ui| {
                    let (color, label) = conn_state_summary(&state);
                    ui.horizontal(|ui| {
                        ui.label("Status");
                        ui.colored_label(color, label);
                    });
                    match &device {
                        Some(info) => {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} {} · {} · proto {}",
                                    info.name,
                                    info.version,
                                    info.chip.label(),
                                    info.protocol,
                                ))
                                .weak()
                                .small(),
                            );
                            if let Some(mac) = info.mac.as_deref() {
                                ui.label(egui::RichText::new(format!("MAC {mac}")).weak().small());
                            }
                        }
                        None => {
                            ui.colored_label(
                                egui::Color32::from_rgb(220, 80, 80),
                                "not identified",
                            );
                        }
                    }
                    ui.horizontal(|ui| {
                        if ui
                            .button("Reset (DTR/RTS)")
                            .on_hover_text("Hard reset via serial control lines, then re-identify.")
                            .clicked()
                        {
                            clear_imported = true;
                            let _ = cmd.send(EspCommand::Reset);
                        }
                        if ui
                            .button("Restart")
                            .on_hover_text("Soft firmware reboot via the `restart` command.")
                            .clicked()
                        {
                            clear_imported = true;
                            let _ = cmd.send(EspCommand::Restart);
                        }
                    });
                });

            egui::CollapsingHeader::new("Capture")
                .default_open(true)
                .show(ui, |ui| {
                    ui.checkbox(
                        &mut self.record_on_capture,
                        "Record to JSONL (exports/)",
                    )
                    .on_hover_text(
                        "When enabled, post-preprocess packets are streamed to \
                         exports/<port>_<time>.jsonl for the duration of collection.",
                    );
                    ui.add_space(4.0);
                    let config_applied = self.sessions[ai].shared.is_config_applied();
                    let start_enabled =
                        is_collecting || (device_ready && !imported && config_applied);
                    let start_label = if is_collecting {
                        "■ Stop Collection"
                    } else {
                        "▶ Start Collection"
                    };
                    let resp = ui.add_enabled(start_enabled, egui::Button::new(start_label));
                    let resp = if !start_enabled {
                        let reason = if imported {
                            "Exit replay first (status-bar button or R key)."
                        } else if device_ready && !config_applied {
                            "Waiting for config sync — Save & Apply or wait for connect-time apply."
                        } else {
                            "Connect an ESP32 running esp-csi-cli-rs firmware first."
                        };
                        resp.on_disabled_hover_text(reason)
                    } else {
                        resp
                    };
                    if resp.clicked() {
                        if is_collecting {
                            let _ = cmd.send(EspCommand::Stop);
                        } else {
                            let dur = self.sessions[ai].cfg.start_duration_sec;
                            let log_mode = self.sessions[ai].cfg.log_mode;
                            let _ = cmd.send(EspCommand::Start {
                                duration_sec: dur,
                                log_mode,
                            });
                        }
                        mark_boundary = true;
                    }
                    if self.recorder.is_recording() {
                        if let Some(path) = self.recorder.path() {
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(format!(
                                    "Recording → {} ({} pkts)",
                                    path.file_name()
                                        .and_then(|s| s.to_str())
                                        .unwrap_or("…"),
                                    self.recorder.packets_written()
                                ))
                                .weak()
                                .small(),
                            );
                        }
                    } else if let Some(rec) = self.recorder.last_recording() {
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(format!(
                                "Last recording: {} ({} pkts)",
                                rec.path.file_name()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("…"),
                                rec.packets_written
                            ))
                            .weak()
                            .small(),
                        )
                        .on_hover_text(rec.path.display().to_string());
                    } else {
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(
                                "Enable \"Record to JSONL\" to save full captures under exports/",
                            )
                            .weak()
                            .small(),
                        );
                    }
                });

            egui::CollapsingHeader::new("Configuration")
                .default_open(true)
                .show(ui, |ui| {
                    if is_collecting {
                        ui.colored_label(
                            egui::Color32::from_rgb(220, 200, 60),
                            "Collection in progress — Stop before applying.",
                        );
                    }
                    let s = &mut self.sessions[ai];
                    if let Some(err) = s.config_error.clone() {
                        ui.colored_label(egui::Color32::from_rgb(230, 80, 80), err);
                    }
                    if !s.shared.is_config_applied() && !is_collecting {
                        ui.colored_label(
                            egui::Color32::from_rgb(220, 200, 60),
                            "Config not synced to device — Save & Apply or wait for connect.",
                        );
                    }
                    crate::ui::esp_panel::config_grid(
                        ui,
                        &mut s.cfg,
                        &mut s.channel_text,
                        &mut s.field_bufs,
                        &s.shared,
                    );
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button("Save & Apply").clicked() {
                            match crate::ui::esp_panel::commit_and_validate(
                                &mut s.cfg,
                                &s.channel_text,
                                &mut s.field_bufs,
                            ) {
                                Ok(()) => {
                                    s.config_error = None;
                                    s.field_bufs = ConfigFieldBuffers::from_cfg(&s.cfg);
                                    do_flush = true;
                                }
                                Err(e) => s.config_error = Some(e),
                            }
                        }
                        let resp = ui.add_enabled(
                            !is_collecting,
                            egui::Button::new("Reset to defaults"),
                        );
                        let resp = if is_collecting {
                            resp.on_disabled_hover_text(cli_disabled_hint)
                        } else {
                            resp
                        };
                        if resp.clicked() {
                            s.shared.mark_config_dirty();
                            let _ = cmd.send(EspCommand::Raw("reset-config".into()));
                        }
                    });
                });

            egui::CollapsingHeader::new("Advanced")
                .default_open(false)
                .show(ui, |ui| {
                    let raw_button = |ui: &mut egui::Ui, label: &str| -> egui::Response {
                        let resp = ui.add_enabled(!is_collecting, egui::Button::new(label));
                        if is_collecting {
                            resp.on_disabled_hover_text(cli_disabled_hint)
                        } else {
                            resp
                        }
                    };
                    if raw_button(ui, "show-config (raw)").clicked() {
                        let _ = cmd.send(EspCommand::Raw("show-config".into()));
                    }
                    if raw_button(ui, "show-stats (raw)").clicked() {
                        let _ = cmd.send(EspCommand::Raw("show-stats".into()));
                    }
                    if raw_button(ui, "info (raw)").clicked() {
                        let _ = cmd.send(EspCommand::Raw("info".into()));
                    }
                });
        });

        if clear_imported {
            self.sessions[ai].imported_recording = false;
        }
        if do_flush {
            self.flush_config_to_device(ai);
        }
        if mark_boundary {
            self.rerun.mark_session_boundary();
        }
    }

    fn render_pane(&mut self, ui: &mut egui::Ui, id: usize, view: ViewType) {
        let focused = id == self.tiling.focused;
        let border_color = if focused {
            self.theme.focused_border
        } else {
            self.theme.normal_border
        };
        let id_color = if focused {
            self.theme.focused_border
        } else {
            self.theme.text_highlight
        };
        let pane_count = self.tiling.pane_count();
        egui::Frame::default()
            .stroke(egui::Stroke::new(if focused { 2.0 } else { 1.0 }, border_color))
            .inner_margin(egui::Margin::same(6))
            .show(ui, |ui| {
                let resp = ui.interact(
                    ui.available_rect_before_wrap(),
                    egui::Id::new(("pane-bg", id)),
                    egui::Sense::click(),
                );
                if resp.clicked() {
                    self.tiling.focus_by_id(id);
                }

                // Guided control header: every keyboard action (pick view,
                // split, fullscreen, close) is a click here so users don't
                // have to memorise shortcuts. Intents are collected into
                // locals and applied after the closures return, to avoid
                // borrowing `self` mutably inside nested egui closures.
                let mut chosen_view: Option<ViewType> = None;
                let mut split_dir: Option<SplitDirection> = None;
                let mut do_fullscreen = false;
                let mut do_close = false;
                // `push_id` keeps the header's widget ids stable across egui's
                // sizing/paint passes; `horizontal_wrapped` lets the controls
                // reflow onto a second line in narrow panes instead of
                // overflowing sideways into the neighbouring pane.
                ui.push_id(("pane-header", id), |ui| {
                    ui.horizontal_wrapped(|ui| {
                        // Pane id badge — kept so the 1..9/0 focus shortcuts
                        // stay discoverable at a glance.
                        ui.label(
                            egui::RichText::new(format!("#{id}"))
                                .strong()
                                .size(16.0)
                                .color(id_color),
                        );
                        egui::ComboBox::from_id_salt(("pane-view", id))
                            .selected_text(view.label())
                            .width(90.0)
                            .show_ui(ui, |ui| {
                                for v in ViewType::ALL_SELECTABLE {
                                    if ui.selectable_label(v == view, v.label()).clicked() {
                                        chosen_view = Some(v);
                                    }
                                }
                            });
                        let can_split = pane_count < MAX_PANES;
                        if ui
                            .add_enabled(can_split, egui::Button::new("Cols").small())
                            .on_hover_text("Split into left/right columns")
                            .clicked()
                        {
                            split_dir = Some(SplitDirection::Horizontal);
                        }
                        if ui
                            .add_enabled(can_split, egui::Button::new("Rows").small())
                            .on_hover_text("Split into top/bottom rows")
                            .clicked()
                        {
                            split_dir = Some(SplitDirection::Vertical);
                        }
                        if ui
                            .button(egui::RichText::new("Full").small())
                            .on_hover_text("Toggle fullscreen for this pane (Space)")
                            .clicked()
                        {
                            do_fullscreen = true;
                        }
                        if ui
                            .add_enabled(pane_count > 1, egui::Button::new("×").small())
                            .on_hover_text("Close pane (Delete)")
                            .clicked()
                        {
                            do_close = true;
                        }
                    });
                });
                // Apply the collected intents. Focus the pane first so the
                // TilingManager operations (which act on the focused pane)
                // target this one.
                if let Some(v) = chosen_view {
                    self.tiling.focus_by_id(id);
                    self.tiling.set_view(v);
                }
                if let Some(dir) = split_dir {
                    self.tiling.focus_by_id(id);
                    self.tiling.split(dir);
                }
                if do_fullscreen {
                    self.tiling.focus_by_id(id);
                    self.tiling.toggle_fullscreen();
                }
                if do_close {
                    self.tiling.focus_by_id(id);
                    self.tiling.close_focused();
                }

                ui.separator();
                let body_h = ui.available_height().max(0.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), body_h),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false; 2])
                            .show(ui, |ui| {
                                let mut vs_clone = self
                                    .view_states
                                    .get_or_default(id)
                                    .clone();
                                let (shared, console, device) = match self.active {
                                    Some(i) => {
                                        let s = &self.sessions[i];
                                        (&s.shared, &s.console, s.device.as_ref())
                                    }
                                    None => {
                                        (&self.fallback_shared, &self.fallback_console, None)
                                    }
                                };
                                let mut ctx = ViewCtx {
                                    history: &self.history,
                                    shared,
                                    console,
                                    replay: &mut self.replay,
                                    device,
                                    theme: &self.theme,
                                    view_state: &mut vs_clone,
                                    focused,
                                    plot_budget: body_h,
                                };
                                render_view(view, ui, &mut ctx);
                                *self.view_states.get_or_default(id) = vs_clone;
                            });
                    },
                );
            });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // eframe 0.34 swapped `App::update(ctx)` → `App::ui(ui)`. Overlays,
        // keys, viewport commands, and the repaint scheduler still want a
        // `&Context`; panels now want a `&mut Ui` via `show_inside`.
        let ctx = ui.ctx().clone();
        self.pump_reader();
        self.handle_overlays(&ctx);
        self.render_firmware_warning(&ctx);
        self.handle_keys(&ctx);

        egui::Panel::top("menu_bar").show_inside(ui, |ui| {
            self.render_menu_bar(ui);
        });
        egui::Panel::bottom("status_bar").show_inside(ui, |ui| {
            self.render_status_bar(ui);
        });
        if self.esp_panel.open {
            egui::Panel::right("esp_panel_side")
                .resizable(true)
                .default_size(320.0)
                .show_inside(ui, |ui| {
                    self.render_esp_panel(ui);
                });
        }
        egui::CentralPanel::default().show_inside(ui, |ui| {
            self.render_tree(ui);
        });

        // 100ms heartbeat for data tick + repaint on close request.
        ctx.request_repaint_after(Duration::from_millis(100));
        if self.should_close {
            // Cleanly stop every reader thread before the window closes.
            for s in &self.sessions {
                s.close();
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn conn_state_summary(s: &ConnState) -> (egui::Color32, String) {
    match s {
        ConnState::Disconnected => (
            egui::Color32::from_rgb(180, 180, 180),
            "Disconnected".into(),
        ),
        ConnState::Resetting => (
            egui::Color32::from_rgb(220, 200, 60),
            "Resetting…".into(),
        ),
        ConnState::AwaitingBanner => (
            egui::Color32::from_rgb(220, 200, 60),
            "Awaiting banner…".into(),
        ),
        ConnState::Identifying => (
            egui::Color32::from_rgb(220, 200, 60),
            "Identifying…".into(),
        ),
        ConnState::UnknownFirmware => (
            egui::Color32::from_rgb(230, 80, 80),
            "Unknown firmware (no esp-csi-cli-rs banner)".into(),
        ),
        ConnState::UnsupportedProtocol(p) => (
            egui::Color32::from_rgb(230, 80, 80),
            format!("Unsupported protocol {p}"),
        ),
        ConnState::Streaming => (
            egui::Color32::from_rgb(80, 220, 110),
            "Streaming".into(),
        ),
        ConnState::Error(e) => (
            egui::Color32::from_rgb(230, 80, 80),
            format!("Error: {e}"),
        ),
    }
}

/// Compact connection badge for the menu-bar right edge. Same colours as
/// [`conn_state_summary`] but short strings that won't overrun the menu
/// buttons; the full text is shown on hover and in the bottom status bar.
fn conn_state_short(s: &ConnState) -> (egui::Color32, String) {
    let (color, _) = conn_state_summary(s);
    let short = match s {
        ConnState::Disconnected => "Disconnected".to_string(),
        ConnState::Resetting => "Resetting…".to_string(),
        ConnState::AwaitingBanner => "Banner…".to_string(),
        ConnState::Identifying => "Identifying…".to_string(),
        ConnState::UnknownFirmware => "Unknown fw".to_string(),
        ConnState::UnsupportedProtocol(p) => format!("proto {p} ✗"),
        ConnState::Streaming => "Streaming".to_string(),
        ConnState::Error(_) => "Error".to_string(),
    };
    (color, short)
}

// Convenience wrapper for `Receiver` re-exports the App uses.
pub fn _expose_receiver_type<T>(_r: crossbeam_channel::Receiver<T>) {}

/// Parse a "host:port" string into a `Transport::Connect`, falling back to
/// 127.0.0.1:9876 for any malformed pieces. Used by the Rerun→Connect prompt.
/// `true` only for the *initial* press of `key` this frame — OS
/// key-repeat events are filtered out. Used by the split-pane
/// shortcuts so holding Shift+Arrow can't spam `TilingManager::split`
/// faster than the user can release the key.
fn key_first_pressed(i: &egui::InputState, key: egui::Key) -> bool {
    i.events.iter().any(|e| {
        matches!(
            e,
            egui::Event::Key {
                key: k,
                pressed: true,
                repeat: false,
                ..
            } if *k == key
        )
    })
}

/// Trim and dedupe the user-typed export filename down to a stem
/// suitable for `export_history` (which appends `_<unix>` and the
/// format-specific extension). Empty / extension-only input falls
/// back to a safe default so we never emit an empty filename.
fn filename_stem_for_export(input: &str) -> String {
    let trimmed = input.trim();
    let path = std::path::Path::new(trimmed);
    path.file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "csi_export".to_string())
}

fn parse_host_port(s: &str) -> Transport {
    let s = s.trim();
    if s.is_empty() {
        return Transport::default_connect();
    }
    let mut parts = s.rsplitn(2, ':');
    let port_str = parts.next();
    let host_str = parts.next();
    let (host, port) = match (host_str, port_str) {
        (Some(h), Some(p)) => (h.to_string(), p.parse::<u16>().unwrap_or(9876)),
        (None, Some(p)) => {
            // No colon — interpret as host only or port only.
            if let Ok(port) = p.parse::<u16>() {
                ("127.0.0.1".into(), port)
            } else {
                (p.to_string(), 9876)
            }
        }
        _ => ("127.0.0.1".into(), 9876),
    };
    Transport::Connect { host, port }
}
