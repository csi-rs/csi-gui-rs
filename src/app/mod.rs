//! Top-level [`eframe::App`].
//!
//! [`App`] owns the [`DeviceManager`] (every connected board), the tiling
//! layout and per-pane view state, the overlays, and the Rerun sink. Each
//! frame it pumps the devices, routes input, and draws:
//!
//! - [`menu_bar`] and [`status_bar`] (top and bottom),
//! - [`device_panel`] (the right-hand device manager and config form),
//! - [`panes`] (the tiling area),
//! - [`overlays`] (modal pickers and prompts), [`input`] (shortcuts) and
//!   [`palette`] (the Ctrl+K command palette).

mod device_panel;
mod input;
mod menu_bar;
mod overlays;
mod palette;
mod panes;
mod status_bar;

use crate::console::RawConsole;
use crate::data::{History, PreprocessConfig, TICK_MS};
use crate::device::{DeviceManager, Notice};
use crate::layout::TilingManager;
use crate::persistence;
use crate::rerun_sink::{RerunSink, Transport};
use crate::shared::EspSharedState;
use crate::theme::{Theme, ThemeVariant};
use crate::ui::{
    alert::Alert, command_palette::CommandPalette, esp_panel::EspPanel, help::HelpOverlay,
    layout_picker::LayoutPicker, main_menu::MainMenu, preprocess_overlay::PreprocessOverlay,
    template_picker::TemplatePicker, text_input::TextPrompt, theme_picker::ThemePicker,
    toast::Toasts, view_selector::ViewSelector,
};
use crate::views::{ReplayState, ViewStates};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Adwaita Sans Regular (SIL OFL 1.1) — vendored at
/// `assets/fonts/AdwaitaSans-Regular.ttf`. Bundled as a last-resort
/// fallback so Unicode arrows (← → ↑ ↓), play/stop glyphs (▶ ●), and
/// other symbols outside eframe's default-font coverage render
/// correctly instead of showing as the "missing glyph" tofu box.
const SYMBOL_FALLBACK_TTF: &[u8] = include_bytes!("../../assets/fonts/AdwaitaSans-Regular.ttf");

/// eframe storage key for [`UiState`].
const STATE_KEY: &str = "csi-gui-rs/ui-state";

/// Install [`SYMBOL_FALLBACK_TTF`] at the end of every default font
/// family so the existing fonts keep priority for ASCII and only the gaps
/// fall through.
fn install_symbol_fallback_font(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "symbol_fallback".to_owned(),
        Arc::new(egui::FontData::from_static(SYMBOL_FALLBACK_TTF)),
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

/// How the app was launched (from the command line).
#[derive(Clone, Debug)]
pub struct AppOptions {
    /// Baud for new ports.
    pub baud: u32,
    /// Plot history window, in ticks.
    pub history_size: usize,
    /// Rerun transport at startup.
    pub rerun: Transport,
    /// Ports to open at startup.
    pub ports: Vec<String>,
    /// Attach every ESP32-looking port, now and as they appear.
    pub auto_attach: bool,
    /// Number of simulated devices to add.
    pub simulate: usize,
    /// Start every device once it is ready (unattended / kiosk use).
    pub start_all: bool,
}

impl Default for AppOptions {
    fn default() -> Self {
        Self {
            baud: crate::shared::DEFAULT_BAUD,
            history_size: crate::data::DEFAULT_HISTORY_CAP,
            rerun: Transport::Off,
            ports: Vec::new(),
            auto_attach: false,
            simulate: 0,
            start_all: false,
        }
    }
}

/// UI state restored across launches through eframe's storage: the last
/// workspace (unless a default template overrides it) and panel prefs.
#[derive(Serialize, Deserialize)]
#[serde(default)]
struct UiState {
    workspace: Option<persistence::Template>,
    panel_open: bool,
    auto_attach: bool,
    record_on_capture: bool,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            workspace: None,
            panel_open: true,
            auto_attach: true,
            record_on_capture: false,
        }
    }
}

/// The application.
pub struct App {
    /// Tiling layout.
    pub tiling: TilingManager,
    /// Per-pane view knobs (including each pane's device binding).
    pub view_states: ViewStates,
    /// Every connected board.
    pub devices: DeviceManager,
    /// Global replay anchor.
    pub replay: ReplayState,
    /// Optional Rerun sink.
    pub rerun: RerunSink,
    /// Current theme.
    pub theme_variant: ThemeVariant,
    /// Resolved theme colours.
    pub theme: Theme,
    /// Live preprocessing pipeline, applied to every device.
    pub preprocess: PreprocessConfig,

    help: HelpOverlay,
    main_menu: MainMenu,
    view_selector: ViewSelector,
    layout_picker: LayoutPicker,
    esp_panel: EspPanel,
    theme_picker: ThemePicker,
    preprocess_overlay: PreprocessOverlay,
    alert: Alert,
    text_prompt: TextPrompt,
    template_picker: TemplatePicker,
    palette: CommandPalette,
    toasts: Toasts,
    quit_confirm_open: bool,
    should_close: bool,

    last_tick: Instant,
    /// `--start-all`: start once the devices are ready (or after a timeout).
    pending_start: Option<Instant>,
    /// What panes read when there is no device at all.
    fallback_history: History,
    fallback_shared: Arc<EspSharedState>,
    fallback_console: Arc<RawConsole>,
}

impl App {
    /// Build the app: restore the workspace, then open the requested ports
    /// (and simulated devices).
    pub fn new(cc: &eframe::CreationContext<'_>, opts: AppOptions) -> Self {
        install_symbol_fallback_font(&cc.egui_ctx);

        let saved: UiState = cc
            .storage
            .and_then(|s| eframe::get_value(s, STATE_KEY))
            .unwrap_or_default();
        // A template the user marked as default wins over the last session.
        let workspace = persistence::load_startup_template()
            .or(saved.workspace)
            .unwrap_or_else(default_workspace);

        let theme_variant = ThemeVariant::from_u8(workspace.tiling.theme_variant);
        let theme = Theme::from_variant(theme_variant);
        theme.apply_to(&cc.egui_ctx);

        let mut devices = DeviceManager::new(opts.baud, opts.history_size);
        devices.auto_attach = opts.auto_attach || (saved.auto_attach && opts.ports.is_empty());
        devices.record_on_capture = saved.record_on_capture;
        devices.rescan();
        for p in &opts.ports {
            devices.add(p);
        }
        for p in crate::sim::sim_ports(opts.simulate) {
            devices.add(&p);
        }
        if devices.auto_attach {
            devices.attach_all();
        }

        let esp_panel = EspPanel {
            open: saved.panel_open,
            ..EspPanel::default()
        };

        Self {
            tiling: workspace.tiling,
            view_states: workspace.view_states,
            devices,
            replay: ReplayState::default(),
            rerun: RerunSink::new(opts.rerun),
            theme_variant,
            theme,
            preprocess: workspace.preprocess,
            help: HelpOverlay::default(),
            main_menu: MainMenu::default(),
            view_selector: ViewSelector::default(),
            layout_picker: LayoutPicker::default(),
            esp_panel,
            theme_picker: ThemePicker::default(),
            preprocess_overlay: PreprocessOverlay::default(),
            alert: Alert::default(),
            text_prompt: TextPrompt::default(),
            template_picker: TemplatePicker::default(),
            palette: CommandPalette::default(),
            toasts: Toasts::default(),
            quit_confirm_open: false,
            should_close: false,
            last_tick: Instant::now(),
            pending_start: opts.start_all.then(Instant::now),
            fallback_history: History::with_capacity(1),
            fallback_shared: EspSharedState::handle(),
            fallback_console: Arc::new(RawConsole::new(1)),
        }
    }

    /// Show `notices` as toasts.
    fn notify(&mut self, notices: impl IntoIterator<Item = Notice>) {
        for n in notices {
            self.toasts.push(n);
        }
    }

    /// The current workspace, as a template.
    fn workspace(&self) -> persistence::Template {
        persistence::Template {
            tiling: self.tiling.clone(),
            preprocess: self.preprocess.clone(),
            view_states: self.view_states.clone(),
        }
    }

    /// Switch theme.
    fn set_theme(&mut self, ctx: &egui::Context, v: ThemeVariant) {
        self.theme_variant = v;
        self.theme = Theme::from_variant(v);
        self.tiling.theme_variant = v.as_u8();
        self.theme.apply_to(ctx);
    }

    /// Replace the workspace with `tpl`.
    fn load_workspace(&mut self, ctx: &egui::Context, tpl: persistence::Template) {
        self.tiling = tpl.tiling;
        self.preprocess = tpl.preprocess;
        self.view_states = tpl.view_states;
        self.devices.clear_preprocess_context();
        let v = ThemeVariant::from_u8(self.tiling.theme_variant);
        self.set_theme(ctx, v);
    }

    /// Step the replay anchor while playing. Bounds come from the active
    /// device's history.
    fn advance_replay(&mut self) {
        if !self.replay.playing {
            return;
        }
        let bounds = self
            .devices
            .active()
            .and_then(|s| Some((s.history.first_id()?, s.history.last_id()?)));
        let Some((first, last)) = bounds else {
            self.replay.playing = false;
            return;
        };
        // One tick per 100 ms at 1.0×.
        let speed = self.replay.speed.max(0.05);
        let step_ms = (TICK_MS as f32 / speed).max(8.0);
        let elapsed = self.replay.last_advance.elapsed().as_millis() as f32;
        if elapsed < step_ms {
            return;
        }
        let steps = (elapsed / step_ms).floor() as u64;
        let next = self.replay.anchor.unwrap_or(first).saturating_add(steps);
        if next >= last {
            self.replay.anchor = None; // reached the head: back to LIVE
            self.replay.playing = false;
        } else {
            self.replay.anchor = Some(next.max(first));
        }
        self.replay.last_advance = Instant::now();
    }

    /// `--start-all`: once every live device is ready with its config
    /// applied (or 20 s have passed and at least one is), start them.
    fn maybe_start_all(&mut self) {
        let Some(since) = self.pending_start else {
            return;
        };
        let live: Vec<_> = self.devices.sessions.iter().filter(|s| s.live).collect();
        let ready = live
            .iter()
            .filter(|s| s.is_ready() && s.config_applied())
            .count();
        let settled = !live.is_empty()
            && live
                .iter()
                .all(|s| (s.is_ready() && s.config_applied()) || s.conn_state.is_problem());
        if settled || (ready > 0 && since.elapsed() > Duration::from_secs(20)) {
            self.pending_start = None;
            // Give the last config line a moment to land.
            std::thread::sleep(Duration::from_millis(300));
            self.toggle_run_all();
        }
    }

    /// Back to the live head.
    fn go_live(&mut self) {
        self.replay.anchor = None;
        self.replay.playing = false;
    }
}

/// The layout a first launch gets: magnitude and the devices table on
/// top, phase and evolution below.
fn default_workspace() -> persistence::Template {
    persistence::BUILTIN_TEMPLATES
        .iter()
        .find(|(n, _)| *n == "showcase")
        .and_then(|(_, json)| persistence::parse_template(json).ok())
        .unwrap_or_else(|| TilingManager::default().into())
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        let notices = self.devices.pump();
        self.notify(notices);
        if self.last_tick.elapsed() >= Duration::from_millis(TICK_MS) {
            self.devices.tick(&self.preprocess, &mut self.rerun);
            self.last_tick = Instant::now();
        }
        self.advance_replay();
        self.maybe_start_all();

        self.handle_overlays(&ctx);
        self.handle_keys(&ctx);

        egui::Panel::top("menu_bar").show_inside(ui, |ui| self.render_menu_bar(ui));
        egui::Panel::bottom("status_bar").show_inside(ui, |ui| self.render_status_bar(ui));
        if self.esp_panel.open {
            egui::Panel::right("device_panel")
                .resizable(true)
                .default_size(360.0)
                .size_range(280.0..=640.0)
                .show_inside(ui, |ui| self.render_device_panel(ui));
        }
        egui::CentralPanel::default().show_inside(ui, |ui| self.render_panes(ui));

        self.show_palette(&ctx);
        self.toasts.show(&ctx);

        ctx.request_repaint_after(Duration::from_millis(TICK_MS));
        if self.should_close {
            self.devices.shutdown();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let state = UiState {
            workspace: Some(self.workspace()),
            panel_open: self.esp_panel.open,
            auto_attach: self.devices.auto_attach,
            record_on_capture: self.devices.record_on_capture,
        };
        eframe::set_value(storage, STATE_KEY, &state);
        for i in 0..self.devices.sessions.len() {
            self.devices.remember(i);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.devices.shutdown();
    }
}
