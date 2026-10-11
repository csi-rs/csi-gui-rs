//! Firmware identification: parse the `info` response into a
//! [`DeviceInfo`], including the [`ChipFamily`] used to gate
//! chip-specific config options (classic vs ESP32-C5/C6).

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Which Espressif SoC the firmware reports running on.
///
/// `is_classic` covers ESP32 / C3 / S3 (legacy 802.11n); `is_c5c6`
/// covers C5 / C6. The two groups have different `set-csi` options and
/// different on-wire CSI struct layouts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[allow(missing_docs)] // each variant maps 1:1 to the SoC name
pub enum ChipFamily {
    Esp32,
    Esp32C3,
    Esp32C5,
    Esp32C6,
    Esp32S3,
    /// Firmware reported a `chip=` value the host doesn't recognise.
    Unknown,
}

impl ChipFamily {
    /// Match a firmware `chip=…` token. Case-insensitive; unknown
    /// values map to [`ChipFamily::Unknown`].
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "esp32" => Self::Esp32,
            "esp32c3" => Self::Esp32C3,
            "esp32c5" => Self::Esp32C5,
            "esp32c6" => Self::Esp32C6,
            "esp32s3" => Self::Esp32S3,
            _ => Self::Unknown,
        }
    }

    /// `true` for ESP32-C5 and ESP32-C6.
    pub fn is_c5c6(self) -> bool {
        matches!(self, Self::Esp32C5 | Self::Esp32C6)
    }

    /// `true` for classic 802.11n chips (ESP32 / C3 / S3).
    pub fn is_classic(self) -> bool {
        matches!(self, Self::Esp32 | Self::Esp32C3 | Self::Esp32S3)
    }

    /// Lowercase canonical name (e.g. `"esp32c6"`). Used in the status
    /// bar and CSV export.
    pub fn label(self) -> &'static str {
        match self {
            Self::Esp32 => "esp32",
            Self::Esp32C3 => "esp32c3",
            Self::Esp32C5 => "esp32c5",
            Self::Esp32C6 => "esp32c6",
            Self::Esp32S3 => "esp32s3",
            Self::Unknown => "unknown",
        }
    }
}

/// Result of the firmware's `info` command — parsed from the
/// `key=value` lines emitted between the welcome banner and
/// `END-INFO`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// Firmware name token (typically `"esp-csi-cli-rs"`).
    pub name: String,
    /// Firmware version string (`"x.y.z"`).
    pub version: String,
    /// Reported SoC family.
    pub chip: ChipFamily,
    /// Wire-protocol version. Compared against [`SUPPORTED_PROTOCOL`]
    /// to decide whether the host understands this firmware build.
    pub protocol: u32,
    /// Device MAC (uppercase, colon-separated). Emitted as the `mac=`
    /// line when the firmware advertises protocol ≥ 2; `None` on older
    /// builds. Used as a stable device identity.
    pub mac: Option<String>,
    /// Feature tags advertised by the firmware (e.g. `statistics`,
    /// `async-print`). Treated as opaque — the host stores them so
    /// views can opt into firmware-side capabilities.
    pub features: HashSet<String>,
}

impl Default for DeviceInfo {
    fn default() -> Self {
        Self {
            name: String::new(),
            version: String::new(),
            chip: ChipFamily::Unknown,
            protocol: 0,
            mac: None,
            features: HashSet::new(),
        }
    }
}

/// Wire-protocol version this build of the GUI understands. Compared
/// against [`DeviceInfo::protocol`] in [`DeviceInfo::protocol_supported`].
/// Bumped 1 → 2 for esp-csi-cli-rs v0.6.0+ (adds the `mac=` info line and
/// the v0.7.x command surface); 2 → 4 for esp-csi-cli-rs v0.10.0+ (the
/// IEEE 802.11bf vocabulary: `esp-now-initiator`/`-responder` and
/// `esp-now-simplex-*` mode tokens, `set-wifi --reporting`,
/// `set-measurement --periodicity-hz`).
pub const SUPPORTED_PROTOCOL: u32 = 4;

impl DeviceInfo {
    /// Parse `key=value` lines emitted between the banner and `END-INFO`.
    pub fn parse(text: &str) -> Self {
        let mut info = DeviceInfo::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line == "END-INFO" {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                let v = v.trim();
                match k.trim() {
                    "name" => info.name = v.to_string(),
                    "version" => info.version = v.to_string(),
                    "chip" => info.chip = ChipFamily::parse(v),
                    "protocol" => info.protocol = v.parse().unwrap_or(0),
                    "mac" => {
                        if !v.is_empty() {
                            info.mac = Some(v.to_ascii_uppercase());
                        }
                    }
                    "features" => {
                        info.features = v
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    }
                    _ => {}
                }
            }
        }
        info
    }

    /// `true` when [`Self::protocol`] matches [`SUPPORTED_PROTOCOL`].
    /// Used by the reader to decide whether to enter the streaming
    /// state or surface a "firmware not recognized" dialog.
    pub fn protocol_supported(&self) -> bool {
        self.protocol == SUPPORTED_PROTOCOL
    }
}
