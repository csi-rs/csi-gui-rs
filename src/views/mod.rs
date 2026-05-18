//! Visualization panes.
//!
//! Each submodule is one pane the user can drop into the tiling layout.
//! Every plot uses [`egui_plot`](https://docs.rs/egui_plot) for high-DPI
//! rendering with proper axes, zoom, pan, and tooltips. All panes are flat
//! 2D — there is no pseudo-3D camera state.
//!
//! - [`dashboard`] — firmware + collection summary, PPS / SNR / RSSI gauges.
//! - [`magnitude`] — `|H(k)|` vs subcarrier; linear or dB; traces or
//!   mean ± σ envelope.
//! - [`phase`] — `∠H(k)` vs subcarrier; raw / unwrapped / linear-detrended.
//! - [`polar`] — polar amplitude of the current frame.
//! - [`isometric`] — channel impulse response (CIR), aged-faded lines.
//! - [`spectrogram`] — subcarrier × time heatmap (phase-Δ or amplitude),
//!   continuous viridis colormap.
//! - [`evolution`] — one-scalar-per-packet time series (mean / std-dev /
//!   Δ|H| / RSSI) for spotting drift and motion over a long window.
//! - [`raw_scatter`] — true I/Q scatter (every sample, no binning).
//! - [`console`] — raw UART text capture.
//! - [`replay`] — timeline scrubber + play/pause that drives the global
//!   [`ReplayState`].

/// Colormaps shared by every heatmap pane.
pub mod colormap;
/// Raw UART text pane.
pub mod console;
/// Firmware/PPS/SNR/RSSI overview pane.
pub mod dashboard;
/// Long-window per-packet scalar-over-time pane.
pub mod evolution;
/// Channel impulse response pane (IDFT).
pub mod isometric;
/// `|H(k)|` vs subcarrier pane.
pub mod magnitude;
/// `∠H(k)` vs subcarrier pane.
pub mod phase;
/// Polar amplitude pane.
pub mod polar;
/// I/Q scatter pane.
pub mod raw_scatter;
/// Global replay scrubber pane.
pub mod replay;
/// Subcarrier × time heatmap pane.
pub mod spectrogram;
/// Per-pane persisted view knobs.
pub mod view_state;

use crate::console::RawConsole;
use crate::data::History;
use crate::layout::ViewType;
use crate::protocol::info::DeviceInfo;
use crate::shared::EspSharedState;
use crate::theme::Theme;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
pub use view_state::ViewState;

/// Global replay control.
///
/// `anchor = None` means "live, follow head". Setting `anchor = Some(id)`
/// freezes every pane to that historic packet — the change is observable
/// to all views via [`ViewCtx::view_anchor`]. `playing` auto-advances the
/// anchor at `speed ×` the source rate (TICK_MS = 100 ms, so 1.0× = 10
/// packets/sec).
///
/// Owned by `App`; mutated either by the Replay view's UI controls or by
/// the keyboard shortcut handler.
pub struct ReplayState {
    /// `None` follows head ("LIVE"); `Some(id)` freezes panes at that
    /// packet id from [`crate::data::History`].
    pub anchor: Option<u64>,
    /// Whether playback is currently auto-advancing the anchor.
    pub playing: bool,
    /// Playback speed multiplier; 1.0 matches live source rate.
    pub speed: f32,
    /// Wall-clock instant of the last advance — used to throttle.
    pub last_advance: Instant,
}

impl Default for ReplayState {
    fn default() -> Self {
        Self {
            anchor: None,
            playing: false,
            speed: 1.0,
            last_advance: Instant::now(),
        }
    }
}

/// Borrow bundle every view receives from `App::render_pane`. Aliases
/// what each pane needs without giving it ownership of `App`.
pub struct ViewCtx<'a> {
    /// Read-only access to the per-tick history.
    pub history: &'a History,
    /// Read-only access to the shared lock-free state (counters, flags).
    pub shared: &'a Arc<EspSharedState>,
    /// Raw UART text capture — only the Console view reads it.
    pub console: &'a Arc<RawConsole>,
    /// Global replay control. Mutable so Replay-related views can drive it.
    pub replay: &'a mut ReplayState,
    /// Last-known firmware identification, or `None` if not identified.
    pub device: Option<&'a DeviceInfo>,
    /// Active color theme.
    pub theme: &'a Theme,
    /// Per-pane state (window sizes, mode selectors, etc.).
    pub view_state: &'a mut ViewState,
    /// `true` when this pane has keyboard/input focus.
    pub focused: bool,
}

impl ViewCtx<'_> {
    /// What every live view should treat as "now" — either the replay anchor
    /// (if set) or the actual head of history.
    pub fn view_anchor(&self) -> Option<u64> {
        self.replay.anchor
    }
}

/// Map an array position `k ∈ 0..n` to its physical subcarrier index
/// relative to DC.
///
/// The firmware emits CSI samples in natural order (lowest physical
/// subcarrier first), so `array[0]` is the most-negative subcarrier and
/// DC sits near the middle. For odd `n` (e.g. 53 active LLTF subcarriers,
/// −26..+26) integer division places DC at exactly `array[n/2]`; for even
/// `n` (e.g. 64-subcarrier HT-LTF, −32..+31) DC lands at `array[n/2]` with
/// one extra positive bin.
///
/// Used by the Phase / Magnitude / Spectrogram views to center the x-axis
/// on DC instead of plotting against array index.
#[inline]
pub fn subcarrier_x(k: usize, n: usize) -> f64 {
    k as f64 - (n / 2) as f64
}

/// Inclusive `(min, max)` bounds of [`subcarrier_x`] for a given total `n`.
/// Returns `(0.0, 0.0)` when `n == 0`.
#[inline]
pub fn subcarrier_x_bounds(n: usize) -> (f64, f64) {
    if n == 0 {
        return (0.0, 0.0);
    }
    let half = (n / 2) as f64;
    (-half, (n - 1) as f64 - half)
}

/// Container of per-pane [`ViewState`] keyed by pane id.
#[derive(Default)]
pub struct ViewStates {
    /// One `ViewState` per layout pane id.
    pub per_pane: HashMap<usize, ViewState>,
}

impl ViewStates {
    /// Fetch the [`ViewState`] for `pane_id`, inserting the default
    /// when the pane is new.
    pub fn get_or_default(&mut self, pane_id: usize) -> &mut ViewState {
        self.per_pane.entry(pane_id).or_default()
    }
}

/// Dispatch to the renderer for `view`. Called by `App::render_pane` once
/// per visible pane every frame.
pub fn render_view(view: ViewType, ui: &mut egui::Ui, ctx: &mut ViewCtx<'_>) {
    match view {
        ViewType::Empty => empty(ui),
        ViewType::Dashboard => dashboard::render(ui, ctx),
        ViewType::Polar => polar::render(ui, ctx),
        ViewType::Isometric => isometric::render(ui, ctx),
        ViewType::Spectrogram => spectrogram::render(ui, ctx),
        ViewType::Phase => phase::render(ui, ctx),
        ViewType::Magnitude => magnitude::render(ui, ctx),
        ViewType::Evolution => evolution::render(ui, ctx),
        ViewType::RawScatter => raw_scatter::render(ui, ctx),
        ViewType::Console => console::render(ui, ctx),
        ViewType::Replay => replay::render(ui, ctx),
    }
}

fn empty(ui: &mut egui::Ui) {
    ui.centered_and_justified(|ui| {
        ui.label(
            egui::RichText::new("Empty pane — press Enter to choose a view")
                .weak()
                .italics(),
        );
    });
}

pub fn title_bar(ui: &mut egui::Ui, title: &str, ctx: &ViewCtx<'_>) {
    let replay = ctx.replay.anchor.is_some();
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(title).heading());
        if replay {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.colored_label(
                    egui::Color32::from_rgb(220, 180, 60),
                    egui::RichText::new("REPLAY").strong().small(),
                );
            });
        }
    });
    ui.separator();
}
