//! Optional [Rerun](https://rerun.io) visualization sink.
//!
//! Logs every decoded CSI packet to an out-of-process Rerun viewer (or
//! `.rrd` file). This is additive to the in-process egui views — when
//! enabled, both update simultaneously.
//!
//! # Stable across the `rerun` Cargo feature
//!
//! The full public API ([`RerunSink`], [`Transport`], [`export_history_to_rrd`])
//! exists in both feature configurations. With the feature off, every
//! method is a no-op. Call sites therefore never need `#[cfg(feature = "rerun")]`
//! blocks; they just call the sink and the build profile decides whether
//! anything happens. This is intentional — it keeps `app.rs` and `main.rs`
//! readable.
//!
//! # Transports
//!
//! Pick one with the `--rerun=<mode>` CLI flag or by calling
//! [`RerunSink::set_transport`] at runtime via the Rerun menu:
//!
//! - [`Transport::Off`] — disabled (default).
//! - [`Transport::Spawn`] — launch a local viewer via the `rerun` CLI
//!   (`cargo install rerun-cli` once).
//! - [`Transport::Connect`] — connect to an already-running viewer.
//! - [`Transport::File`] — save to an `.rrd` file for offline playback.
//!
//! Transports are mutually exclusive — pick one at a time. Switch with
//! [`RerunSink::set_transport`].
//!
//! # Entity layout
//!
//! Every device logs under its own root, `csi/<device>/`, where `<device>`
//! is its MAC with `:` replaced by `-` (see [`entity_root`]), so several
//! boards sit side by side in one viewer.
//!
//! Per packet (timeline `frame_idx`, counted per device):
//! - `csi/<device>/bar_amplitude` — `BarChart` of `|H(k)|`
//! - `csi/<device>/complex_scatter` — `Points3D` `(I, Q, |H|)`, color by phase
//! - `csi/<device>/cir` — `BarChart` of channel impulse response amplitude
//! - `csi/<device>/heatmap` — rolling u8 `Tensor[H, W]` (up to 500 rows)
//! - `csi/<device>/scalars/rssi`, `…/scalars/noise_floor`
//!
//! Per UI tick (timeline `tick_idx`, shared by all devices):
//! - `csi/<device>/scalars/pps`, `…/scalars/snr`, `…/scalars/tick_rssi`
//!
//! # Example
//!
//! ```no_run
//! use csi_gui_rs::rerun_sink::{RerunSink, Transport};
//! use csi_gui_rs::data::CsiData;
//!
//! // Spawn a viewer and forward one packet:
//! let mut sink = RerunSink::new(Transport::Spawn);
//! let pkt = CsiData::default();
//! sink.log_packet(&pkt);   // no-op without the `rerun` feature
//! ```

use crate::data::{CsiData, History, NetworkStats};
use std::path::{Path, PathBuf};

#[cfg(feature = "rerun")]
use crate::data::dsp;
#[cfg(feature = "rerun")]
use std::collections::VecDeque;

#[cfg(feature = "rerun")]
const ROLLING_HEATMAP_ROWS: usize = 500;

/// Where the sink should send its data. `Off` is the default.
#[derive(Clone, Debug, Default)]
pub enum Transport {
    /// Sink is disabled — `log_packet` / `log_tick` are no-ops.
    #[default]
    Off,
    /// Launch a local viewer (`rerun` CLI) and stream into it.
    Spawn,
    /// Connect over TCP to an already-running viewer.
    Connect {
        /// Hostname or IP address.
        host: String,
        /// TCP port (default 9876).
        port: u16,
    },
    /// Save the stream to an `.rrd` file at `path`. Parent directory is
    /// created on demand.
    File {
        /// Output path.
        path: PathBuf,
    },
}

impl Transport {
    /// Short human label for the status bar / menu (e.g. `"spawn"`,
    /// `"connect:127.0.0.1:9876"`, `"file:logs/run.rrd"`).
    pub fn label(&self) -> String {
        match self {
            Self::Off => "off".into(),
            Self::Spawn => "spawn".into(),
            Self::Connect { host, port } => format!("connect:{host}:{port}"),
            Self::File { path } => format!("file:{}", path.display()),
        }
    }

    /// `Transport::Connect { host: "127.0.0.1", port: 9876 }` — what
    /// `--rerun=connect` (no host/port suffix) resolves to.
    pub fn default_connect() -> Self {
        Self::Connect {
            host: "127.0.0.1".into(),
            port: 9876,
        }
    }
}

/// Rerun visualization sink.
///
/// Held by `App`. See the [module docs](self) for the entity layout and
/// transport semantics. Drop the sink (or call [`flush_and_close`]) before
/// process exit to make sure file-mode recordings are flushed to disk.
///
/// [`flush_and_close`]: Self::flush_and_close
pub struct RerunSink {
    transport: Transport,
    live_streaming: bool,
    rrd_recording: bool,
    last_error: Option<String>,

    #[cfg(feature = "rerun")]
    rec: Option<rerun::RecordingStream>,
    /// Rolling heatmap and frame counter, per device root.
    #[cfg(feature = "rerun")]
    devices: std::collections::HashMap<String, DeviceStream>,
}

/// Per-device rolling state.
#[cfg(feature = "rerun")]
#[derive(Default)]
struct DeviceStream {
    heatmap_buf: VecDeque<Vec<u8>>,
    heatmap_width: usize,
    rolling_max: f32,
    frame_idx: u64,
}

/// Entity-path root for a device id: `csi/A0-85-E3-F5-48-90`. Packets
/// with no device (imports from before 2.0) log under plain `csi`.
pub fn entity_root(device: &str) -> String {
    if device.is_empty() {
        "csi".to_string()
    } else {
        format!("csi/{}", crate::paths::sanitize_file_stem(device))
    }
}

impl RerunSink {
    /// Construct a sink with the given transport. Never panics; if the
    /// transport fails to initialize (`rerun` viewer not on PATH, port in
    /// use, etc.) the sink reports the failure via [`last_error`] and
    /// behaves like [`Transport::Off`] until [`set_transport`] is called
    /// again.
    ///
    /// [`last_error`]: Self::last_error
    /// [`set_transport`]: Self::set_transport
    pub fn new(transport: Transport) -> Self {
        let mut sink = Self {
            transport: Transport::Off,
            live_streaming: false,
            rrd_recording: false,
            last_error: None,
            #[cfg(feature = "rerun")]
            rec: None,
            #[cfg(feature = "rerun")]
            devices: std::collections::HashMap::new(),
        };
        sink.set_transport(transport);
        sink
    }

    /// Current transport mode.
    pub fn transport(&self) -> &Transport {
        &self.transport
    }

    /// Last error message (if any), e.g. `"spawn failed: rerun not found"`.
    /// Cleared by any successful [`set_transport`](Self::set_transport) /
    /// [`toggle_live`](Self::toggle_live) / [`toggle_rrd`](Self::toggle_rrd).
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// `true` when the sink is forwarding decoded packets to a viewer or
    /// file. Becomes `false` while paused (Shift+R) or when transport is
    /// [`Transport::Off`].
    pub fn is_live(&self) -> bool {
        self.live_streaming && !matches!(self.transport, Transport::Off)
    }

    /// `true` when transport is [`Transport::File`] and recording is enabled.
    /// Toggle with Shift+L or [`toggle_rrd`](Self::toggle_rrd).
    pub fn is_recording(&self) -> bool {
        self.rrd_recording && matches!(self.transport, Transport::File { .. })
    }

    /// Pause or resume packet forwarding without dropping the stream. The
    /// existing connection/file is preserved.
    pub fn toggle_live(&mut self) {
        if matches!(self.transport, Transport::Off) {
            self.last_error =
                Some("Rerun transport is Off — select Spawn/Connect/File first.".into());
            return;
        }
        self.live_streaming = !self.live_streaming;
        self.last_error = None;
    }

    /// RRD recording only toggles a flag; the actual file-mode stream is
    /// already created when `transport == File`. Live streaming gates whether
    /// per-packet payload reaches the stream.
    pub fn toggle_rrd(&mut self) {
        if !matches!(self.transport, Transport::File { .. }) {
            self.last_error = Some(
                "RRD recording is only valid when transport = File. Pick \
                 Record-to-file… in the Rerun menu first."
                    .into(),
            );
            return;
        }
        self.rrd_recording = !self.rrd_recording;
        self.last_error = None;
    }

    /// Reset per-session rolling state (heatmap window, rolling max) when
    /// the user (re)starts collection. The underlying recording stream is
    /// kept open; only the in-process aggregation buffers are cleared so
    /// the heatmap doesn't mix the new session's amplitudes with stale
    /// rows from the previous one.
    ///
    /// `frame_idx` keeps incrementing monotonically — the Rerun viewer's
    /// timeline is sequence-based and going backwards causes data to be
    /// hidden or overwritten.
    pub fn mark_session_boundary(&mut self) {
        #[cfg(feature = "rerun")]
        for dev in self.devices.values_mut() {
            dev.heatmap_buf.clear();
            dev.heatmap_width = 0;
            dev.rolling_max = 0.0;
        }
    }

    /// Drop any open stream and open a fresh one matching the new transport.
    ///
    /// On success, the per-packet `frame_idx` timeline resets to 0 so the
    /// viewer's time panel starts cleanly. On failure, the sink reverts to
    /// [`Transport::Off`]-equivalent behavior and the reason is available
    /// via [`last_error`](Self::last_error).
    pub fn set_transport(&mut self, t: Transport) {
        self.flush_and_close();
        self.transport = t.clone();
        self.last_error = None;
        self.live_streaming = false;
        self.rrd_recording = false;
        #[cfg(feature = "rerun")]
        {
            self.devices.clear();
            self.rec = match &t {
                Transport::Off => None,
                Transport::Spawn => match build_spawn() {
                    Ok(s) => {
                        self.live_streaming = true;
                        Some(s)
                    }
                    Err(e) => {
                        self.last_error = Some(format!("spawn failed: {e}"));
                        None
                    }
                },
                Transport::Connect { host, port } => match build_connect(host, *port) {
                    Ok(s) => {
                        self.live_streaming = true;
                        Some(s)
                    }
                    Err(e) => {
                        self.last_error = Some(format!("connect failed: {e}"));
                        None
                    }
                },
                Transport::File { path } => match build_file(path) {
                    Ok(s) => {
                        self.rrd_recording = true;
                        // file mode also gates on live_streaming so the user can
                        // pause recording without re-opening the file.
                        self.live_streaming = true;
                        Some(s)
                    }
                    Err(e) => {
                        self.last_error = Some(format!("save failed: {e}"));
                        None
                    }
                },
            };
        }
    }

    /// Forward one decoded packet to the active Rerun stream.
    ///
    /// No-op when:
    /// - The sink is paused ([`is_live`](Self::is_live) returns `false`), or
    /// - `p` is a metadata-only stub ([`CsiData::is_decoded`] is `false`), or
    /// - The crate was compiled without `--features rerun`.
    ///
    /// Logs all five per-packet entities (`bar_amplitude`, `complex_scatter`,
    /// `cir`, `heatmap`, scalars) on the `frame_idx` timeline.
    pub fn log_packet(&mut self, p: &CsiData) {
        #[cfg(feature = "rerun")]
        {
            if !self.live_streaming || !p.is_decoded() {
                return;
            }
            let n = p.subcarriers();
            if n == 0 || self.rec.is_none() {
                return;
            }
            // Pre-compute everything that needs `&mut self` (heatmap rolling
            // state) before borrowing `self.rec` as `&` for log calls.
            let amps_f32: Vec<f32> = (0..n).map(|k| p.amplitude(k)).collect();
            let root = entity_root(&p.device);
            let dev = self.devices.entry(root.clone()).or_default();
            dev.update_heatmap(&amps_f32, n);
            let tensor = dev.build_heatmap_tensor();
            let frame_idx = dev.frame_idx;
            dev.frame_idx = dev.frame_idx.wrapping_add(1);

            let rec = self.rec.clone().expect("checked is_none above");
            rec.set_time_sequence("frame_idx", frame_idx as i64);

            // 1. bar_amplitude — per-subcarrier |H(k)|.
            let amps: Vec<f64> = amps_f32.iter().map(|&v| v as f64).collect();
            let _ = rec.log(format!("{root}/bar_amplitude"), &rerun::BarChart::new(amps));

            // 2. csi/complex_scatter — (I, Q, amplitude) with phase-colored points.
            let positions: Vec<[f32; 3]> = (0..n)
                .map(|k| {
                    let (i, q) = p.iq(k);
                    [i as f32, q as f32, amps_f32[k]]
                })
                .collect();
            let colors: Vec<[u8; 4]> = (0..n).map(|k| dsp::phase_to_rgba(p.phase(k))).collect();
            let _ = rec.log(
                format!("{root}/complex_scatter"),
                &rerun::Points3D::new(positions).with_colors(colors),
            );

            // 3. cir — per-tap impulse response (free, since we have IDFT).
            let cir = dsp::compute_cir(p);
            if !cir.is_empty() {
                let cir_f64: Vec<f64> = cir.iter().map(|&v| v as f64).collect();
                let _ = rec.log(format!("{root}/cir"), &rerun::BarChart::new(cir_f64));
            }

            // 4. heatmap — rolling normalized u8 tensor [rows, n].
            if let Some(tensor) = tensor {
                let _ = rec.log(format!("{root}/heatmap"), &tensor);
            }

            // 5. scalars — per-packet RSSI/noise floor.
            let _ = rec.log(
                format!("{root}/scalars/rssi"),
                &rerun::Scalars::new([p.rssi as f64]),
            );
            let _ = rec.log(
                format!("{root}/scalars/noise_floor"),
                &rerun::Scalars::new([p.noise_floor as f64]),
            );
        }
        #[cfg(not(feature = "rerun"))]
        {
            let _ = p;
        }
    }

    /// Forward one device's per-tick stats aggregate (PPS, SNR, RSSI),
    /// produced by [`History::push_tick_at`]. Uses the shared `tick_idx`
    /// timeline.
    ///
    /// [`History::push_tick_at`]: crate::data::History::push_tick_at
    pub fn log_tick(&mut self, device: &str, stats: &NetworkStats) {
        #[cfg(feature = "rerun")]
        {
            if !self.live_streaming {
                return;
            }
            let Some(rec) = self.rec.as_ref() else { return };
            let root = entity_root(device);
            rec.set_time_sequence("tick_idx", stats.id as i64);
            let _ = rec.log(
                format!("{root}/scalars/pps"),
                &rerun::Scalars::new([stats.pps as f64]),
            );
            let _ = rec.log(
                format!("{root}/scalars/snr"),
                &rerun::Scalars::new([stats.snr as f64]),
            );
            let _ = rec.log(
                format!("{root}/scalars/tick_rssi"),
                &rerun::Scalars::new([stats.rssi as f64]),
            );
        }
        #[cfg(not(feature = "rerun"))]
        {
            let _ = (device, stats);
        }
    }

    /// Flush any pending writes and drop the underlying stream. Called
    /// automatically by `Drop`; call it explicitly only if you want to make
    /// sure a file recording is durable before checking it externally.
    pub fn flush_and_close(&mut self) {
        #[cfg(feature = "rerun")]
        {
            if let Some(rec) = self.rec.take() {
                let _ = rec.flush_blocking();
            }
        }
    }

    /// `(color, text)` for the status bar. Hover the badge in the GUI to
    /// see [`last_error`](Self::last_error) details.
    pub fn status_summary(&self) -> (egui::Color32, String) {
        if self.last_error.is_some() {
            return (egui::Color32::from_rgb(220, 80, 80), "rerun: err".into());
        }
        match (&self.transport, self.live_streaming, self.rrd_recording) {
            (Transport::Off, _, _) => (egui::Color32::from_gray(160), "rerun: off".into()),
            (Transport::File { .. }, true, true) => {
                (egui::Color32::from_rgb(80, 220, 110), "rerun: rec".into())
            }
            (_, true, _) => (egui::Color32::from_rgb(80, 220, 110), "rerun: live".into()),
            _ => (
                egui::Color32::from_gray(180),
                format!("rerun: {} (paused)", self.transport.label()),
            ),
        }
    }
}

#[cfg(feature = "rerun")]
impl DeviceStream {
    fn update_heatmap(&mut self, amps: &[f32], width: usize) {
        if width != self.heatmap_width {
            self.heatmap_buf.clear();
            self.heatmap_width = width;
            self.rolling_max = 0.0;
        }
        let frame_max = amps.iter().copied().fold(0f32, f32::max);
        // Rolling max within the buffer window so a single hot frame doesn't
        // flatten everything to ~0 forever.
        self.rolling_max = self.rolling_max.max(frame_max);
        // Re-derive rolling max by scanning the buffer occasionally so it
        // decays as old hot frames fall out the back.
        if self.heatmap_buf.len() == ROLLING_HEATMAP_ROWS {
            self.heatmap_buf.pop_front();
            self.rolling_max = 1e-6;
            for row in &self.heatmap_buf {
                for &b in row {
                    let v = b as f32;
                    if v > self.rolling_max {
                        self.rolling_max = v;
                    }
                }
            }
            self.rolling_max = self.rolling_max.max(frame_max);
        }
        let scale = if self.rolling_max > 1e-6 {
            255.0 / self.rolling_max
        } else {
            0.0
        };
        let row: Vec<u8> = amps
            .iter()
            .map(|&v| (v * scale).round().clamp(0.0, 255.0) as u8)
            .collect();
        self.heatmap_buf.push_back(row);
    }

    fn build_heatmap_tensor(&self) -> Option<rerun::Tensor> {
        if self.heatmap_buf.is_empty() || self.heatmap_width == 0 {
            return None;
        }
        let height = self.heatmap_buf.len();
        let mut flat: Vec<u8> = Vec::with_capacity(height * self.heatmap_width);
        for row in &self.heatmap_buf {
            flat.extend_from_slice(row);
        }
        let shape: Vec<u64> = vec![height as u64, self.heatmap_width as u64];
        let data = rerun::TensorData::new(shape, rerun::TensorBuffer::U8(flat.into()));
        Some(rerun::Tensor::new(data))
    }
}

impl Drop for RerunSink {
    fn drop(&mut self) {
        self.flush_and_close();
    }
}

#[cfg(feature = "rerun")]
fn build_spawn() -> Result<rerun::RecordingStream, String> {
    rerun::RecordingStreamBuilder::new("csi-gui-rs")
        .spawn()
        .map_err(|e| e.to_string())
}

#[cfg(feature = "rerun")]
fn build_connect(host: &str, port: u16) -> Result<rerun::RecordingStream, String> {
    // rerun 0.20+ takes a `rerun+http://host:port/proxy` URL via gRPC.
    let url = format!("rerun+http://{host}:{port}/proxy");
    rerun::RecordingStreamBuilder::new("csi-gui-rs")
        .connect_grpc_opts(url)
        .map_err(|e| e.to_string())
}

#[cfg(feature = "rerun")]
fn build_file(path: &Path) -> Result<rerun::RecordingStream, String> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    rerun::RecordingStreamBuilder::new("csi-gui-rs")
        .save(path)
        .map_err(|e| e.to_string())
}

/// Replay an entire `History` into a fresh `.rrd` file.
///
/// Uses the same entity paths as the live sink (so live and exported
/// captures can be overlaid in one viewer instance). Skips the rolling
/// heatmap — reflogging it per tick would bloat the file without adding
/// information that the per-packet `bar_amplitude` doesn't already
/// reconstruct interactively.
///
/// Returns `Err("built without --features rerun".into())` when the crate
/// was compiled without the Rerun feature.
///
/// # Example
///
/// ```no_run
/// # use std::path::PathBuf;
/// # use csi_gui_rs::data::History;
/// # use csi_gui_rs::rerun_sink::export_history_to_rrd;
/// let history = History::new();
/// let path = PathBuf::from("exports/history.rrd");
/// export_history_to_rrd(&[("A0:85:E3:F5:48:90", &history)], &path).unwrap();
/// ```
pub fn export_history_to_rrd(histories: &[(&str, &History)], path: &Path) -> Result<(), String> {
    #[cfg(feature = "rerun")]
    {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let rec = rerun::RecordingStreamBuilder::new("csi-gui-rs-export")
            .save(path)
            .map_err(|e| e.to_string())?;

        for (device, history) in histories {
            let root = entity_root(device);
            let mut frame_idx: u64 = 0;
            for stats in history.entries.iter() {
                let Some(csi) = stats.csi.as_ref() else {
                    continue;
                };
                let n = csi.subcarriers();
                if n == 0 {
                    continue;
                }
                rec.set_time_sequence("frame_idx", frame_idx as i64);
                rec.set_time_sequence("tick_idx", stats.id as i64);

                let amps: Vec<f64> = (0..n).map(|k| csi.amplitude(k) as f64).collect();
                let _ = rec.log(format!("{root}/bar_amplitude"), &rerun::BarChart::new(amps));

                let positions: Vec<[f32; 3]> = (0..n)
                    .map(|k| {
                        let (i, q) = csi.iq(k);
                        [i as f32, q as f32, csi.amplitude(k)]
                    })
                    .collect();
                let colors: Vec<[u8; 4]> =
                    (0..n).map(|k| dsp::phase_to_rgba(csi.phase(k))).collect();
                let _ = rec.log(
                    format!("{root}/complex_scatter"),
                    &rerun::Points3D::new(positions).with_colors(colors),
                );

                let cir = dsp::compute_cir(csi);
                if !cir.is_empty() {
                    let cir_f64: Vec<f64> = cir.iter().map(|&v| v as f64).collect();
                    let _ = rec.log(format!("{root}/cir"), &rerun::BarChart::new(cir_f64));
                }

                let _ = rec.log(
                    format!("{root}/scalars/rssi"),
                    &rerun::Scalars::new([stats.rssi as f64]),
                );
                let _ = rec.log(
                    format!("{root}/scalars/snr"),
                    &rerun::Scalars::new([stats.snr as f64]),
                );
                let _ = rec.log(
                    format!("{root}/scalars/pps"),
                    &rerun::Scalars::new([stats.pps as f64]),
                );

                frame_idx += 1;
            }
        }
        let _ = rec.flush_blocking();
        Ok(())
    }
    #[cfg(not(feature = "rerun"))]
    {
        let _ = (histories, path);
        Err("built without --features rerun".into())
    }
}
