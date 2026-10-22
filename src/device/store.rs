//! Per-device settings that outlive a session: the alias and the config,
//! keyed by the board's MAC, saved to [`crate::paths::devices_file`].
//!
//! Replug a board — into any socket — and it comes back with the name and
//! configuration it had.

use crate::protocol::config::EspConfig;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// What is remembered about one board.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StoredDevice {
    /// User-given name; empty for none.
    pub alias: String,
    /// The last configuration applied to it.
    pub config: EspConfig,
}

/// Every remembered board, by MAC.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeviceStore {
    /// Format version, for future migrations.
    pub version: u32,
    /// MAC (`AA:BB:CC:DD:EE:FF`) → settings.
    pub devices: BTreeMap<String, StoredDevice>,
}

impl DeviceStore {
    /// Load from `path`. A missing or unreadable file is an empty store: a
    /// corrupt settings file must never stop the app from starting.
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Write to `path` atomically (temp file + rename).
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(&Self { version: 1, ..self.clone() })
            .map_err(std::io::Error::other)?;
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, path)
    }

    /// Settings for `mac`, if remembered.
    pub fn get(&self, mac: &str) -> Option<&StoredDevice> {
        self.devices.get(mac)
    }

    /// Remember `alias` and `config` for `mac`.
    pub fn remember(&mut self, mac: &str, alias: &str, config: &EspConfig) {
        self.devices.insert(
            mac.to_string(),
            StoredDevice {
                alias: alias.to_string(),
                config: config.clone(),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::config::WifiMode;

    #[test]
    fn roundtrips_and_tolerates_garbage() {
        let dir = std::env::temp_dir().join(format!("csi-gui-rs-store-{}", std::process::id()));
        let path = dir.join("devices.json");
        let mut store = DeviceStore::default();
        let cfg = EspConfig {
            wifi_mode: WifiMode::EspNowResponder,
            channel: 11,
            ..EspConfig::default()
        };
        store.remember("A0:85:E3:F5:48:90", "bench-left", &cfg);
        store.save(&path).unwrap();

        let back = DeviceStore::load(&path);
        let d = back.get("A0:85:E3:F5:48:90").unwrap();
        assert_eq!(d.alias, "bench-left");
        assert_eq!(d.config, cfg);

        std::fs::write(&path, "{not json").unwrap();
        assert_eq!(DeviceStore::load(&path).devices.len(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
