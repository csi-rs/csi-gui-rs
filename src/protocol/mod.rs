//! Wire protocol for the [`esp-csi-cli-rs`] firmware.
//!
//! - [`command`] — render configuration into the firmware's CLI string
//!   format and represent commands the reader thread should run.
//! - [`config`] — typed Wi-Fi / CSI configuration (`set-wifi`, `set-csi`,
//!   `set-rate`, …) including channel validation.
//! - [`info`] — parse the `info` response into [`DeviceInfo`] and check
//!   firmware/protocol compatibility.
//! - [`parse`] — incremental parsers for the text / array-list /
//!   esp-csi-tool CSV output formats.
//! - [`profile`] — pluggable seam for acquisition presets this crate does
//!   not name itself.
//! - [`serialized`] — COBS-framed `postcard` binary format.
//!
//! [`esp-csi-cli-rs`]: https://github.com/csi-rs/esp-csi-cli-rs

pub mod command;
pub mod config;
pub mod info;
pub mod parse;
pub mod profile;
pub mod serialized;

pub use command::EspCommand;
pub use config::{CsiDelivery, EspConfig, LogMode, PhyRate, Reporting, WifiMode, ESP_CONFIG_FIELDS};
pub use info::{ChipFamily, DeviceInfo};
pub use profile::{CsiProfile, StandardCsiProfile};
