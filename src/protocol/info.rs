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
    /// Log encoding the build was compiled with (`log=`): `text`, or
    /// `defmt`, which this GUI can't read. Empty on firmware before 0.9.
    #[serde(default)]
    pub log: String,
    /// Console the build writes to (`transport=`): `auto`, `jtag` or `uart`.
    #[serde(default)]
    pub transport: String,
    /// UART rate the build was compiled with (`baud=`), when reported.
    #[serde(default)]
    pub baud: Option<u32>,
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
            log: String::new(),
            transport: String::new(),
            baud: None,
        }
    }
}

/// Newest CLI protocol this build of the GUI speaks: esp-csi-cli-rs 0.10.x,
/// the IEEE 802.11bf vocabulary (`esp-now-initiator`/`-responder`,
/// `set-reporting`, `set-measurement --periodicity-hz`).
pub const SUPPORTED_PROTOCOL: u32 = 4;

/// Oldest CLI protocol the GUI still accepts. Protocol 3 (esp-csi-cli-rs
/// 0.9) has the same wire format and nearly the same commands; the
/// renamed ones (the ESP-NOW mode tokens, `set-measurement`,
/// `--reporting`) are rejected by that firmware with an error on the
/// console, everything else applies.
pub const MIN_SUPPORTED_PROTOCOL: u32 = 3;

/// Why a device can't be used, as decided from its `info`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Incompatibility {
    /// `protocol=` is outside [`MIN_SUPPORTED_PROTOCOL`]`..=`[`SUPPORTED_PROTOCOL`].
    Protocol(u32),
    /// A `defmt` build: its log frames are defmt-encoded, which this GUI
    /// does not decode.
    Defmt,
}

impl std::fmt::Display for Incompatibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Protocol(p) if *p > SUPPORTED_PROTOCOL => write!(
                f,
                "firmware speaks protocol {p}, newer than this GUI ({SUPPORTED_PROTOCOL}); update csi-gui-rs"
            ),
            Self::Protocol(p) => write!(
                f,
                "firmware speaks protocol {p}; this GUI needs {MIN_SUPPORTED_PROTOCOL}–{SUPPORTED_PROTOCOL} \
                 (esp-csi-cli-rs 0.9 or later). Reflash the board"
            ),
            Self::Defmt => write!(
                f,
                "this is a defmt build (log=defmt); flash the plain-text esp-csi-cli-rs build"
            ),
        }
    }
}

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
                    "log" => info.log = v.to_ascii_lowercase(),
                    "transport" => info.transport = v.to_string(),
                    "baud" => info.baud = v.parse().ok(),
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

    /// `true` when [`Self::protocol`] is within
    /// [`MIN_SUPPORTED_PROTOCOL`]`..=`[`SUPPORTED_PROTOCOL`].
    pub fn protocol_supported(&self) -> bool {
        (MIN_SUPPORTED_PROTOCOL..=SUPPORTED_PROTOCOL).contains(&self.protocol)
    }

    /// `true` when the device speaks an older protocol than the newest
    /// this GUI knows; it works, but some settings won't apply.
    pub fn protocol_outdated(&self) -> bool {
        self.protocol_supported() && self.protocol < SUPPORTED_PROTOCOL
    }

    /// `Ok` when the GUI can drive this device, otherwise why not.
    pub fn check(&self) -> Result<(), Incompatibility> {
        if !self.protocol_supported() {
            return Err(Incompatibility::Protocol(self.protocol));
        }
        if self.log == "defmt" {
            return Err(Incompatibility::Defmt);
        }
        Ok(())
    }
}

impl DeviceInfo {
    /// One-line summary, e.g. `esp-csi-cli-rs 0.10.1 · esp32c5 · proto 4`.
    pub fn summary(&self) -> String {
        format!(
            "{} {} · {} · proto {}",
            if self.name.is_empty() {
                "firmware"
            } else {
                &self.name
            },
            self.version,
            self.chip.label(),
            self.protocol
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INFO_0_10_1: &str = "ESP-CSI-CLI/0.10.1\r\nname=esp-csi-cli-rs\r\nversion=0.10.1\r\n\
        chip=esp32c5\r\nprotocol=4\r\nmac=10:bd:a3:ca:34:98\r\nlog=text\r\n\
        transport=jtag\r\nbaud=115200\r\nfeatures=statistics,async-print\r\nEND-INFO\r\n";

    #[test]
    fn parses_current_info_block() {
        let info = DeviceInfo::parse(INFO_0_10_1);
        assert_eq!(info.name, "esp-csi-cli-rs");
        assert_eq!(info.version, "0.10.1");
        assert_eq!(info.chip, ChipFamily::Esp32C5);
        assert_eq!(info.protocol, 4);
        assert_eq!(info.mac.as_deref(), Some("10:BD:A3:CA:34:98"));
        assert_eq!(info.log, "text");
        assert_eq!(info.transport, "jtag");
        assert_eq!(info.baud, Some(115_200));
        assert!(info.features.contains("statistics"));
        assert_eq!(info.check(), Ok(()));
        assert!(!info.protocol_outdated());
    }

    #[test]
    fn protocol_range_and_defmt() {
        let mut info = DeviceInfo::parse(INFO_0_10_1);
        info.protocol = 3;
        assert_eq!(info.check(), Ok(()));
        assert!(info.protocol_outdated());
        info.protocol = 2;
        assert_eq!(info.check(), Err(Incompatibility::Protocol(2)));
        info.protocol = 5;
        assert!(
            info.check()
                .unwrap_err()
                .to_string()
                .contains("update csi-gui-rs")
        );
        info.protocol = 4;
        info.log = "defmt".into();
        assert_eq!(info.check(), Err(Incompatibility::Defmt));
    }
}
