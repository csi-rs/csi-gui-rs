//! # csi-gui-rs
//!
//! A desktop GUI library and binary for capturing, visualizing, and exporting
//! Wi-Fi Channel-State-Information (CSI) packets emitted by ESP32 devices
//! running the [`esp-csi-cli-rs`] firmware.
//!
//! [`esp-csi-cli-rs`]: https://github.com/connectedmotion-io/esp-csi-cli-rs
//!
//! Most users only need the [`csi-gui-rs` binary](../csi-gui-rs/index.html);
//! this library crate exists so the same building blocks (serial reader,
//! parsers, history buffer, visualizations, Rerun sink, persistence) can be
//! reused in other tools or wrapped in custom front-ends.
//!
//! ## At a glance
//!
//! - **Auto-connect** to the first available serial port. The [`reader`]
//!   thread resets the chip via DTR/RTS, identifies the firmware via the
//!   `info` command, and decodes CSI in any of four wire formats (text,
//!   serialized/COBS, array-list, esp-csi-tool CSV). All formats normalize
//!   into the same [`CsiData`] struct.
//! - **In-memory ring buffer** ([`History`]) holds the last *N* per-tick
//!   aggregates; configurable via [`data::DEFAULT_HISTORY_CAP`] or the
//!   `--history-size` CLI flag.
//! - **Visualizations** in [`views`] use [`egui_plot`](https://docs.rs/egui_plot)
//!   for high-resolution 2D rendering. Every plot is flat 2D — no fake 3D
//!   rotations — and exposes proper axes, zoom, pan, and tooltips.
//! - **Global replay** ([`views::ReplayState`]) anchors all panes to a
//!   historic packet id; setting it back to `None` snaps every pane to LIVE.
//! - **CSV export** ([`persistence::export_history_csv`]) writes a single
//!   uniform 22-column schema regardless of the wire format that originally
//!   produced the data.
//! - **Optional Rerun integration** ([`rerun_sink`], gated behind the `rerun`
//!   Cargo feature) logs every decoded packet to a [Rerun](https://rerun.io)
//!   viewer or an `.rrd` file.
//!
//! [`CsiData`]: crate::data::CsiData
//! [`History`]: crate::data::History
//!
//! ## Quick example — running the GUI
//!
//! The binary entry point parses CLI flags and starts the eframe app:
//!
//! ```no_run
//! use csi_gui_rs::app::App;
//! use csi_gui_rs::data::DEFAULT_HISTORY_CAP;
//! use csi_gui_rs::rerun_sink::Transport;
//! use csi_gui_rs::shared::DEFAULT_BAUD;
//!
//! eframe::run_native(
//!     "csi-gui-rs",
//!     eframe::NativeOptions::default(),
//!     Box::new(|cc| {
//!         Ok(Box::new(App::new(
//!             cc,
//!             DEFAULT_BAUD,
//!             DEFAULT_HISTORY_CAP,
//!             Transport::Off,
//!         )))
//!     }),
//! ).unwrap();
//! ```
//!
//! ## Quick example — using the reader without the GUI
//!
//! If you only want decoded CSI packets — say, to feed your own model or
//! pipeline — spawn the reader directly and pump the channel:
//!
//! ```no_run
//! use std::sync::Arc;
//! use csi_gui_rs::console::RawConsole;
//! use csi_gui_rs::protocol::command::EspCommand;
//! use csi_gui_rs::reader::spawn_reader;
//! use csi_gui_rs::shared::EspSharedState;
//!
//! let shared = EspSharedState::handle();
//! let console = Arc::new(RawConsole::new(2_000));
//! let handles = spawn_reader(shared.clone(), console);
//!
//! handles.commands
//!     .send(EspCommand::OpenPort("/dev/ttyUSB0".into()))
//!     .unwrap();
//!
//! while let Ok(packet) = handles.packets.recv() {
//!     if packet.is_decoded() {
//!         println!(
//!             "MAC={} RSSI={} subcarriers={}",
//!             packet.mac, packet.rssi, packet.subcarriers()
//!         );
//!     }
//! }
//! ```
//!
//! ## Cargo features
//!
//! | Feature | Default | What it adds |
//! | ------- | ------- | ------------ |
//! | `rerun` | off     | [`rerun_sink::RerunSink`] backed by the Rerun 0.19 SDK. With the feature off, the sink type still exists and exposes the same public API but all methods are no-ops, so call sites don't need conditional compilation. |
//!
//! ## Module map
//!
//! - [`app`] — top-level eframe application: tiling layout, menu bar, status
//!   bar, overlays, key handling.
//! - [`reader`] — background thread that owns the serial port and decodes
//!   the wire stream.
//! - [`protocol`] — wire-level parsers, firmware identification, command
//!   rendering.
//! - [`data`] — in-memory CSI representation, ring-buffer history,
//!   pre-computed per-tick stats, shared DSP helpers, the live
//!   preprocessing pipeline.
//! - [`views`] — every visualization pane (Dashboard, Magnitude, Phase,
//!   Polar, CIR, Spectrogram, Evolution, I/Q Scatter, Console, Replay).
//! - [`layout`] — tiling pane manager and persistable layout tree.
//! - [`persistence`] — full-workspace JSON templates (layout + theme +
//!   preprocessing + view-state knobs) and capture export/import in
//!   CSV / JSONL / Parquet.
//! - [`rerun_sink`] — Rerun integration (Cargo feature `rerun`).
//! - [`console`] — rolling raw-UART text buffer used by the Console view.
//! - [`shared`] — lock-free state shared between the reader thread and the
//!   UI thread.
//! - [`ui`] — modal overlays (help, view/layout pickers, theme picker,
//!   preprocessing, …) and the docked ESP device panel.
//! - [`theme`] — color schemes.

pub mod app;
pub mod console;
pub mod data;
pub mod layout;
pub mod persistence;
pub mod protocol;
pub mod reader;
pub mod rerun_sink;
pub mod shared;
pub mod theme;
pub mod ui;
pub mod views;
