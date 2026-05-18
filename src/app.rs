//! Top-level [`eframe::App`] implementation.
//!
//! [`App::new`] owns every long-lived resource — reader thread,
//! [`crate::data::History`], [`crate::rerun_sink::RerunSink`], all
//! overlays — and routes input + draws the UI each frame.

use crate::console::RawConsole;
use crate::data::History;
use crate::layout::{LayoutNode, SplitDirection, TilingManager, ViewType};
use crate::persistence;
use crate::protocol::{
    command::EspCommand,
    info::{DeviceInfo, SUPPORTED_PROTOCOL},
};
use crate::reader::{enumerate_ports, spawn_reader, ConnState, ReaderHandles};
use crate::rerun_sink::{self, RerunSink, Transport};
use crate::shared::EspSharedState;
use crate::theme::{Theme, ThemeVariant};
use crate::ui::{
    config_overlay::{ConfigOverlay, ConfigOverlayResult},
    help::HelpOverlay,
    main_menu::{MainMenu, MainMenuItem},
    template_picker::{TemplatePicker, TemplatePickerResult},
    text_input::{TextPrompt, TextPromptPurpose, TextPromptResult},
    view_selector::ViewSelector,
};
use crate::views::{render_view, ReplayState, ViewCtx, ViewStates};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct App {
    pub tiling: TilingManager,
    pub history: History,
    pub view_states: ViewStates,
    pub shared: Arc<EspSharedState>,
    pub console: Arc<RawConsole>,
    pub replay: ReplayState,
    pub rerun: RerunSink,
    pub reader: ReaderHandles,
    pub conn_state: ConnState,
    pub device: Option<DeviceInfo>,

    pub theme_variant: ThemeVariant,
    pub theme: Theme,
    pub pending_theme_cycle: bool,

    pub help: HelpOverlay,
    pub main_menu: MainMenu,
    pub view_selector: ViewSelector,
    pub config_overlay: ConfigOverlay,
    pub text_prompt: TextPrompt,
    pub template_picker: TemplatePicker,
    pub quit_confirm_open: bool,
    pub should_close: bool,

    pub port_to_open: String,
    pub available_ports: Vec<String>,
    pub last_port_scan: Instant,
    pub firmware_warning_dismissed: bool,
    pub last_tick: Instant,
}

impl App {
    /// Construct the app and spawn the reader thread.
    ///
    /// - `baud` — serial baud rate. ESP-CSI-CLI defaults to 115200.
    /// - `history_size` — ring-buffer capacity (number of per-tick stats
    ///   to retain).
    /// - `rerun_transport` — Rerun sink mode. Use [`Transport::Off`] to
    ///   disable.
    ///
    /// Auto-connects to the first serial port [`enumerate_ports`] lists.
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        baud: u32,
        history_size: usize,
        rerun_transport: Transport,
    ) -> Self {
        let shared = EspSharedState::handle_with_baud(baud);
        let console = Arc::new(RawConsole::new(2_000));
        let reader = spawn_reader(shared.clone(), console.clone());
        let rerun = RerunSink::new(rerun_transport);

        let tiling = persistence::load_startup_template().unwrap_or_default();
        let theme_variant = ThemeVariant::from_u8(tiling.theme_variant);
        let theme = Theme::from_variant(theme_variant);
        theme.apply_to(&cc.egui_ctx);

        let available_ports = enumerate_ports();
        let port = available_ports.first().cloned().unwrap_or_default();
        if !port.is_empty() {
            let _ = reader.commands.send(EspCommand::OpenPort(port.clone()));
        }

        Self {
            tiling,
            history: History::with_capacity(history_size),
            view_states: ViewStates::default(),
            shared,
            console,
            replay: ReplayState::default(),
            rerun,
            reader,
            conn_state: ConnState::Disconnected,
            device: None,
            theme_variant,
            theme,
            pending_theme_cycle: false,
            help: HelpOverlay::default(),
            main_menu: MainMenu::default(),
            view_selector: ViewSelector::default(),
            config_overlay: ConfigOverlay::default(),
            text_prompt: TextPrompt::default(),
            template_picker: TemplatePicker::default(),
            quit_confirm_open: false,
            should_close: false,
            port_to_open: port,
            available_ports,
            last_port_scan: Instant::now(),
            firmware_warning_dismissed: false,
            last_tick: Instant::now(),
        }
    }

    fn pump_reader(&mut self) {
        // Track the pre-drain state so we can detect a fresh entry into
        // `Streaming` and push the full config to the device — the
        // firmware's default `LogMode` is `ArrayList`, the GUI's default
        // is `Serialized`, and without an explicit sync the two sides
        // disagree until the user opens the config overlay and clicks
        // Apply.
        let prev_state = self.conn_state.clone();
        while let Ok(s) = self.reader.state.try_recv() {
            if self.conn_state != s {
                self.firmware_warning_dismissed = false;
            }
            self.conn_state = s;
        }
        while let Ok(info) = self.reader.info.try_recv() {
            self.device = Some(info);
        }
        if !matches!(prev_state, ConnState::Streaming)
            && matches!(self.conn_state, ConnState::Streaming)
        {
            self.flush_config_to_device();
        }

        if self.last_port_scan.elapsed() > Duration::from_secs(2) {
            self.available_ports = enumerate_ports();
            self.last_port_scan = Instant::now();
        }

        self.advance_replay();

        if self.last_tick.elapsed() < Duration::from_millis(100) {
            return;
        }
        let raw_before = self.shared.frames_total.load(Ordering::Relaxed);
        let mut batch = Vec::new();
        while let Ok(p) = self.reader.packets.try_recv() {
            self.rerun.log_packet(&p);
            batch.push(p);
        }
        let raw_after = self.shared.frames_total.load(Ordering::Relaxed);
        let raw_delta = raw_after.saturating_sub(raw_before);
        if let Some(stats) = self.history.push_tick(batch, raw_delta) {
            self.rerun.log_tick(stats);
        }
        self.last_tick = Instant::now();
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

    /// Push the entire `config_overlay.cfg` to the device via
    /// `EspCommand::ApplyConfig`. Called automatically on every
    /// transition into [`ConnState::Streaming`] so the firmware's
    /// runtime state matches the GUI's view from the first packet —
    /// otherwise the firmware boots in its own defaults (`LogMode::ArrayList`,
    /// etc.) and the GUI parses the wrong wire format until the user
    /// clicks Apply.
    fn flush_config_to_device(&mut self) {
        if let Some(info) = self.device.as_ref() {
            let cfg = self.config_overlay.cfg.clone();
            let _ = self.reader.commands.send(EspCommand::ApplyConfig {
                cfg,
                chip: info.chip,
            });
        }
    }

    /// `true` when the reader has identified an ESP32 running a supported
    /// `esp-csi-cli-rs` build. Used to gate "Start Collection" so the user
    /// can't kick a session off against an absent or unrecognised device.
    /// Stop is always allowed (so the local "collecting" state can be
    /// cleared even if connectivity changes mid-session).
    fn is_device_ready(&self) -> bool {
        matches!(self.conn_state, ConnState::Streaming)
            && self
                .device
                .as_ref()
                .map(|d| d.protocol_supported())
                .unwrap_or(false)
    }

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
            || self.config_overlay.open
            || self.text_prompt.open
            || self.template_picker.open
            || self.quit_confirm_open;
        if any_overlay {
            if self.quit_confirm_open {
                self.handle_quit_confirm(ctx);
            }
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
                // R = return all panes to LIVE.
                self.replay.anchor = None;
                self.replay.playing = false;
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
                if i.key_pressed(egui::Key::ArrowLeft) || i.key_pressed(egui::Key::ArrowRight) {
                    self.tiling.split(SplitDirection::Horizontal);
                }
                if i.key_pressed(egui::Key::ArrowUp) || i.key_pressed(egui::Key::ArrowDown) {
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

        let is_collecting = self.shared.is_collecting.load(Ordering::Acquire);
        let device_ready = self.is_device_ready();

        if let Some(item) = self.main_menu.show(ctx, is_collecting, device_ready) {
            match item {
                MainMenuItem::Theme => self.cycle_theme(ctx),
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
                    self.text_prompt.label = "Export name (timestamp appended):".into();
                    self.text_prompt.purpose = TextPromptPurpose::ExportData;
                }
                MainMenuItem::ResetEsp => {
                    let _ = self.reader.commands.send(EspCommand::Reset);
                }
                MainMenuItem::ConfigureEsp => {
                    self.config_overlay.open = true;
                    self.config_overlay.is_collecting = is_collecting;
                }
                MainMenuItem::StartStop => {
                    if is_collecting {
                        let _ = self.reader.commands.send(EspCommand::Stop);
                        self.rerun.mark_session_boundary();
                    } else if device_ready {
                        let dur = self.config_overlay.cfg.start_duration_sec;
                        let _ = self
                            .reader
                            .commands
                            .send(EspCommand::Start { duration_sec: dur });
                        self.rerun.mark_session_boundary();
                    }
                }
                MainMenuItem::Close => {}
            }
        }

        if let Some(view) = self.view_selector.show(ctx) {
            self.tiling.set_view(view);
        }

        match self.config_overlay.show(ctx) {
            ConfigOverlayResult::Apply(cfg) => {
                if let Some(info) = &self.device {
                    let _ = self.reader.commands.send(EspCommand::ApplyConfig {
                        cfg,
                        chip: info.chip,
                    });
                }
            }
            ConfigOverlayResult::Cancel | ConfigOverlayResult::None => {}
        }

        match self.text_prompt.show(ctx) {
            TextPromptResult::Submit(name, TextPromptPurpose::SaveTemplate) => {
                let _ = persistence::save_template(&name, &self.tiling);
            }
            TextPromptResult::Submit(name, TextPromptPurpose::ExportData) => {
                let raw: Vec<_> = self.history.raw_history.iter().cloned().collect();
                let _ = persistence::export_history_csv(&name, &raw);
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
            TextPromptResult::Submit(_, TextPromptPurpose::None)
            | TextPromptResult::Cancel
            | TextPromptResult::None => {}
        }

        match self.template_picker.show(ctx) {
            TemplatePickerResult::Load(path) => {
                if let Ok(tm) = persistence::read_template(&path) {
                    self.tiling = tm;
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
                            ui.scope_builder(egui::UiBuilder::new().max_rect(left), |ui| {
                                self.render_node(ui, c);
                            });
                        }
                        if let Some(c) = children.get(1) {
                            ui.scope_builder(egui::UiBuilder::new().max_rect(right), |ui| {
                                self.render_node(ui, c);
                            });
                        }
                    }
                    SplitDirection::Vertical => {
                        let split = rect.top() + rect.height() * frac;
                        let top =
                            egui::Rect::from_min_max(rect.min, egui::pos2(rect.right(), split));
                        let bot =
                            egui::Rect::from_min_max(egui::pos2(rect.left(), split), rect.max);
                        if let Some(c) = children.first() {
                            ui.scope_builder(egui::UiBuilder::new().max_rect(top), |ui| {
                                self.render_node(ui, c);
                            });
                        }
                        if let Some(c) = children.get(1) {
                            ui.scope_builder(egui::UiBuilder::new().max_rect(bot), |ui| {
                                self.render_node(ui, c);
                            });
                        }
                    }
                }
            }
        }
    }

    fn render_menu_bar(&mut self, ui: &mut egui::Ui) {
        let is_collecting = self.shared.is_collecting.load(Ordering::Acquire);
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
                if ui.button("Export Data…").clicked() {
                    self.text_prompt.open = true;
                    self.text_prompt.title = "Export Data".into();
                    self.text_prompt.label = "Export name (timestamp appended):".into();
                    self.text_prompt.purpose = TextPromptPurpose::ExportData;
                    ui.close();
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    self.quit_confirm_open = true;
                    ui.close();
                }
            });

            ui.menu_button("ESP", |ui| {
                ui.menu_button("Port", |ui| {
                    if self.available_ports.is_empty() {
                        ui.label(
                            egui::RichText::new("(no serial ports detected)")
                                .color(egui::Color32::from_rgb(220, 80, 80)),
                        );
                    } else {
                        for port in self.available_ports.clone() {
                            let current = port == self.port_to_open;
                            let label = if current {
                                format!("● {port}")
                            } else {
                                format!("   {port}")
                            };
                            if ui.button(label).clicked() {
                                self.port_to_open = port.clone();
                                let _ = self
                                    .reader
                                    .commands
                                    .send(EspCommand::OpenPort(port));
                                self.firmware_warning_dismissed = false;
                                ui.close();
                            }
                        }
                    }
                    ui.separator();
                    if ui.button("Rescan ports").clicked() {
                        self.available_ports = enumerate_ports();
                    }
                });
                if ui.button("Reset Device (DTR/RTS)").clicked() {
                    let _ = self.reader.commands.send(EspCommand::Reset);
                    ui.close();
                }
                ui.separator();
                if ui.button("Configure…").clicked() {
                    self.config_overlay.open = true;
                    self.config_overlay.is_collecting = is_collecting;
                    ui.close();
                }
                if ui.button("Reset Config to defaults").clicked() {
                    let _ = self
                        .reader
                        .commands
                        .send(EspCommand::Raw("reset-config".into()));
                    ui.close();
                }
                ui.separator();
                let start_label = if is_collecting {
                    "Stop Collection"
                } else {
                    "Start Collection"
                };
                let device_ready = self.is_device_ready();
                let start_enabled = is_collecting || device_ready;
                let resp = ui.add_enabled(start_enabled, egui::Button::new(start_label));
                let resp = if !start_enabled {
                    resp.on_disabled_hover_text(
                        "Connect an ESP32 running esp-csi-cli-rs firmware first.",
                    )
                } else {
                    resp
                };
                if resp.clicked() {
                    if is_collecting {
                        let _ = self.reader.commands.send(EspCommand::Stop);
                    } else {
                        let dur = self.config_overlay.cfg.start_duration_sec;
                        let _ = self
                            .reader
                            .commands
                            .send(EspCommand::Start { duration_sec: dur });
                    }
                    self.rerun.mark_session_boundary();
                    ui.close();
                }
                ui.separator();
                if ui.button("show-config (raw)").clicked() {
                    let _ = self
                        .reader
                        .commands
                        .send(EspCommand::Raw("show-config".into()));
                    ui.close();
                }
                if ui.button("show-stats (raw)").clicked() {
                    let _ = self
                        .reader
                        .commands
                        .send(EspCommand::Raw("show-stats".into()));
                    ui.close();
                }
                if ui.button("info (raw)").clicked() {
                    let _ = self.reader.commands.send(EspCommand::Raw("info".into()));
                    ui.close();
                }
            });

            ui.menu_button("View", |ui| {
                if ui.button("Cycle Theme").clicked() {
                    self.pending_theme_cycle = true;
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

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (color, text) = conn_state_summary(&self.conn_state);
                ui.colored_label(color, text);
            });
        });
    }

    fn render_status_bar(&mut self, ui: &mut egui::Ui) {
        let is_collecting = self.shared.is_collecting.load(Ordering::Acquire);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Port").weak().small());
            let port_display = if self.port_to_open.is_empty() {
                "—".to_string()
            } else {
                self.port_to_open.clone()
            };
            ui.label(port_display);
            ui.separator();

            let (color, label) = conn_state_summary(&self.conn_state);
            ui.colored_label(color, label);
            ui.separator();

            match &self.device {
                Some(info) => {
                    ui.label(format!(
                        "{} {} · {} · proto {}",
                        info.name,
                        info.version,
                        info.chip.label(),
                        info.protocol,
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

            let log_mode_id = self.shared.log_mode_id.load(Ordering::Acquire);
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
                ui.colored_label(
                    egui::Color32::from_rgb(220, 180, 60),
                    format!("replay @ #{anchor}"),
                );
                if ui.small_button("Live").clicked() {
                    self.replay.anchor = None;
                    self.replay.playing = false;
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

            #[cfg(feature = "rerun")]
            {
                ui.separator();
                let (rcol, rtext) = self.rerun.status_summary();
                let resp = ui.colored_label(rcol, rtext);
                if let Some(err) = self.rerun.last_error() {
                    resp.on_hover_text(err);
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new("M menu · H help · Enter pick view")
                        .weak()
                        .small(),
                );
            });
        });
    }

    fn render_firmware_warning(&mut self, ctx: &egui::Context) {
        let needs_warning = matches!(
            &self.conn_state,
            ConnState::UnknownFirmware | ConnState::UnsupportedProtocol(_)
        );
        if !needs_warning || self.firmware_warning_dismissed {
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
                match &self.conn_state {
                    ConnState::UnknownFirmware => {
                        ui.colored_label(
                            egui::Color32::from_rgb(230, 80, 80),
                            "No esp-csi-cli-rs banner was received from this device after reset.",
                        );
                        ui.label(format!("Port: {}", self.port_to_open));
                        ui.add_space(4.0);
                        ui.label(
                            "This GUI resets the ESP via DTR/RTS on connect and expects the \
                             firmware to emit `ESP-CSI-CLI/<version>` as its first banner line. \
                             The device on this port either:",
                        );
                        ui.label("  • is not running esp-csi-cli-rs (please flash it), or");
                        ui.label("  • is on a different serial port (try the ESP → Port menu), or");
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
            let _ = self.reader.commands.send(EspCommand::Reset);
        }
        if do_dismiss {
            self.firmware_warning_dismissed = true;
        }
        if open_config_menu {
            self.firmware_warning_dismissed = true;
            self.available_ports = enumerate_ports();
        }
    }

    fn render_pane(&mut self, ui: &mut egui::Ui, id: usize, view: ViewType) {
        let focused = id == self.tiling.focused;
        let border_color = if focused {
            self.theme.focused_border
        } else {
            self.theme.normal_border
        };
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
                ui.label(
                    egui::RichText::new(format!("#{id}"))
                        .weak()
                        .small(),
                );

                let mut vs_clone = self
                    .view_states
                    .get_or_default(id)
                    .clone();
                let mut ctx = ViewCtx {
                    history: &self.history,
                    shared: &self.shared,
                    console: &self.console,
                    replay: &mut self.replay,
                    device: self.device.as_ref(),
                    theme: &self.theme,
                    view_state: &mut vs_clone,
                    focused,
                };
                render_view(view, ui, &mut ctx);
                *self.view_states.get_or_default(id) = vs_clone;
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
        egui::CentralPanel::default().show_inside(ui, |ui| {
            self.render_tree(ui);
        });

        // 100ms heartbeat for data tick + repaint on close request.
        ctx.request_repaint_after(Duration::from_millis(100));
        if self.should_close {
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

// Convenience wrapper for `Receiver` re-exports the App uses.
pub fn _expose_receiver_type<T>(_r: crossbeam_channel::Receiver<T>) {}

/// Parse a "host:port" string into a `Transport::Connect`, falling back to
/// 127.0.0.1:9876 for any malformed pieces. Used by the Rerun→Connect prompt.
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
