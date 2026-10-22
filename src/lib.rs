//! # csi-gui-rs
//!
//! A desktop GUI library and binary for capturing, visualizing, and exporting
//! Wi-Fi Channel State Information (CSI) from **one or many** ESP32 boards
//! running the [`esp-csi-cli-rs`] firmware.
//!
//! [`esp-csi-cli-rs`]: https://github.com/csi-rs/esp-csi-cli-rs
//!
//! Most users only need the `csi-gui-rs` binary (`cargo install csi-gui-rs`);
//! this library crate exists so the same building blocks (device manager,
//! serial reader, decoders, history buffer, visualizations, Rerun sink,
//! persistence) can be reused in other tools or wrapped in custom front-ends.
//!
//! ## Firmware compatibility
//!
//! | csi-gui-rs | esp-csi-cli-rs | CLI protocol | Serialized format |
//! |---|---|---|---|
//! | 2.0 | 0.10.x (recommended), 0.9 | 4, 3 | wire v1 (esp-csi-rs 0.12+), pre-0.12 legacy decoded too |
//!
//! ## At a glance
//!
//! - **Many devices at once.** [`device::DeviceManager`] runs one
//!   [`device::DeviceSession`] per board, each with its own reader thread,
//!   history, preprocessing context and recorder. Boards are discovered by
//!   USB vendor, named by MAC, followed across re-enumeration, and remember
//!   their alias and config ([`device::store`]). "Start all" runs every
//!   board under one shared measurement session (`set-session`), so their
//!   frames line up.
//! - **Every wire format.** The [`reader`] identifies the firmware via
//!   `info` and decodes serialized (the esp-csi-rs [`protocol::wire`]
//!   contract, vendored), array-list, text and esp-csi-tool output into one
//!   [`CsiData`] struct, stamped with the device it came from.
//! - **Per-pane device binding.** Every pane in the tiling layout shows the
//!   active device or a specific one; the Devices view and the Evolution
//!   overlay compare them.
//! - **Capture files.** Per-device JSONL recording while collecting, and
//!   export / import as CSV, JSONL or Parquet ([`persistence::export`]).
//! - **Optional Rerun integration** ([`rerun_sink`], Cargo feature
//!   `rerun`), one entity tree per device.
//! - **Headless probe** ([`probe`]) and a **simulated device** ([`sim`]) for
//!   testing without the GUI or without hardware.
//!
//! [`CsiData`]: crate::data::CsiData
//!
//! ## Example — the device pipeline without the GUI
//!
//! ```no_run
//! use csi_gui_rs::data::PreprocessConfig;
//! use csi_gui_rs::device::DeviceManager;
//! use csi_gui_rs::rerun_sink::{RerunSink, Transport};
//!
//! let mut devices = DeviceManager::new(115_200, 1_000);
//! devices.rescan();
//! devices.attach_all(); // every ESP32-looking serial port
//! let mut rerun = RerunSink::new(Transport::Off);
//! loop {
//!     for notice in devices.pump() {
//!         println!("{notice:?}");
//!     }
//!     devices.tick(&PreprocessConfig::default(), &mut rerun);
//!     for s in &devices.sessions {
//!         if let Some(csi) = s.history.last().and_then(|t| t.csi.as_ref()) {
//!             println!("{}: {} subcarriers, {} dBm", s.label(), csi.subcarriers(), csi.rssi);
//!         }
//!     }
//!     std::thread::sleep(std::time::Duration::from_millis(100));
//! }
//! ```
//!
//! ## Example — one reader, raw packets
//!
//! ```no_run
//! use std::sync::Arc;
//! use csi_gui_rs::console::RawConsole;
//! use csi_gui_rs::protocol::command::EspCommand;
//! use csi_gui_rs::reader::spawn_reader;
//! use csi_gui_rs::shared::EspSharedState;
//!
//! let handles = spawn_reader(EspSharedState::handle(), Arc::new(RawConsole::new(2_000)));
//! handles.commands.send(EspCommand::OpenPort("/dev/ttyUSB0".into())).unwrap();
//! while let Ok(packet) = handles.packets.recv() {
//!     println!("{} from {}: {} subcarriers", packet.device, packet.mac, packet.subcarriers());
//! }
//! ```
//!
//! ## Cargo features
//!
//! | Feature | Default | What it adds |
//! | ------- | ------- | ------------ |
//! | `rerun` | off     | [`rerun_sink::RerunSink`] backed by the Rerun SDK. With the feature off the sink still exists with the same API, as no-ops, so call sites need no conditional compilation. |
//!
//! ## Module map
//!
//! - [`app`] — the eframe application: menus, status bar, device panel,
//!   panes, overlays, command palette.
//! - [`device`] — device sessions, the manager, discovery and saved settings.
//! - [`reader`] — the per-device thread that owns a port and decodes it.
//! - [`protocol`] — firmware identification, command rendering, decoders,
//!   and the vendored [`protocol::wire`] contract.
//! - [`data`] — [`CsiData`], the tick history, stats, DSP, preprocessing.
//! - [`views`] — every visualization pane.
//! - [`layout`] — tiling pane manager and persistable layout tree.
//! - [`persistence`] — workspace templates and capture files.
//! - [`paths`] — platform config / data directories.
//! - [`probe`] — the headless `csi-gui-rs probe` command.
//! - [`sim`] — a simulated esp-csi-cli-rs device.
//! - [`rerun_sink`] — Rerun integration (Cargo feature `rerun`).
//! - [`console`], [`shared`], [`ui`], [`theme`] — support.

pub mod app;
pub mod console;
pub mod data;
pub mod device;
pub mod layout;
pub mod paths;
pub mod persistence;
pub mod probe;
pub mod protocol;
pub mod reader;
pub mod rerun_sink;
pub mod shared;
pub mod sim;
pub mod theme;
pub mod ui;
pub mod views;
