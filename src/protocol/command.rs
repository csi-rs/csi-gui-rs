use super::config::{EspConfig, WifiMode};
use crate::protocol::info::ChipFamily;

/// Commands sent from the UI thread to the reader thread via mpsc.
#[derive(Clone, Debug)]
pub enum EspCommand {
    /// Pulse DTR/RTS, re-banner, re-info.
    Reset,
    /// Begin collection (`start [--duration=<secs>]`). Locks the CLI.
    Start {
        /// Auto-stop after `N` seconds. `None` runs until the user
        /// sends [`EspCommand::Stop`].
        duration_sec: Option<u64>,
    },
    /// Stop collection (`q`).
    Stop,
    /// Apply the entire `set-*` surface. Ignored if `is_collecting`.
    ApplyConfig {
        /// The new config — see [`crate::protocol::config::EspConfig`].
        cfg: EspConfig,
        /// Chip family (used to pick the right `set-csi` flag set).
        chip: ChipFamily,
    },
    /// Raw shell-out for advanced use (e.g. `show-config`).
    Raw(String),
    /// Open / re-open the named port.
    OpenPort(String),
    /// Quit the reader thread.
    Shutdown,
}

/// Render an `EspConfig` to the ordered list of CLI strings (§6.2).
///
/// `set-log-mode` is emitted **last** so `io_generation` bumps after every
/// stateful parser has been torn down.
pub fn render_apply_config(cfg: &EspConfig, chip: ChipFamily) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    out.push(format!("set-wifi --mode={}", cfg.wifi_mode.cli()));
    out.push(format!("set-wifi --set-channel={}", cfg.channel));
    if !cfg.sta_ssid.is_empty() {
        out.push(format!(
            "set-wifi --sta-ssid={}",
            quote_for_wire(&cfg.sta_ssid)
        ));
    }
    if !cfg.sta_password.is_empty() {
        out.push(format!(
            "set-wifi --sta-password={}",
            quote_for_wire(&cfg.sta_password)
        ));
    }
    // `peer-mac` / `ht40` are ESP-NOW-only knobs. Emit them only for the
    // ESP-NOW modes so other modes don't get spurious set-wifi lines. An
    // empty `peer-mac` is sent verbatim to clear any prior filter back to
    // automatic pairing.
    if matches!(
        cfg.wifi_mode,
        WifiMode::EspNowCentral | WifiMode::EspNowPeripheral
    ) {
        out.push(format!("set-wifi --peer-mac={}", cfg.peer_mac));
        out.push(format!("set-wifi --ht40={}", cfg.ht40.cli()));
    }
    out.push(format!("set-traffic --frequency-hz={}", cfg.traffic_hz));
    out.push(format!(
        "set-collection-mode --mode={}",
        cfg.collection_mode.cli()
    ));
    out.push(format!("set-rate --rate={}", cfg.phy_rate.cli()));
    out.push(format!(
        "set-io-tasks --tx={} --rx={}",
        on_off(cfg.tx_task),
        on_off(cfg.rx_task)
    ));
    out.push(format!(
        "set-csi-delivery --mode={} --logging={}",
        cfg.csi_delivery.cli(),
        on_off(cfg.csi_logging)
    ));

    if chip.is_classic() {
        let mut parts = vec!["set-csi".to_string()];
        if cfg.classic_disable_lltf {
            parts.push("--disable-lltf".into());
        }
        if cfg.classic_disable_htltf {
            parts.push("--disable-htltf".into());
        }
        if cfg.classic_disable_stbc_htltf {
            parts.push("--disable-stbc-htltf".into());
        }
        if cfg.classic_disable_ltf_merge {
            parts.push("--disable-ltf-merge".into());
        }
        if parts.len() > 1 {
            out.push(parts.join(" "));
        }
    } else if chip.is_he() {
        let mut parts = vec!["set-csi".to_string()];
        if cfg.he_disable_csi {
            parts.push("--disable-csi".into());
        }
        if cfg.he_disable_csi_legacy {
            parts.push("--disable-csi-legacy".into());
        }
        if cfg.he_disable_csi_ht20 {
            parts.push("--disable-csi-ht20".into());
        }
        if cfg.he_disable_csi_ht40 {
            parts.push("--disable-csi-ht40".into());
        }
        if cfg.he_disable_csi_su {
            parts.push("--disable-csi-su".into());
        }
        if cfg.he_disable_csi_mu {
            parts.push("--disable-csi-mu".into());
        }
        if cfg.he_disable_csi_dcm {
            parts.push("--disable-csi-dcm".into());
        }
        if cfg.he_disable_csi_beamformed {
            parts.push("--disable-csi-beamformed".into());
        }
        parts.push(format!("--csi-he-stbc={}", cfg.he_stbc));
        parts.push(format!("--val-scale-cfg={}", cfg.he_val_scale_cfg));
        out.push(parts.join(" "));
    }

    out.push(format!("set-log-mode --mode={}", cfg.log_mode.cli()));
    out
}

/// Render the `start` command line. `Some(N)` adds `--duration=N`;
/// `None` returns just `"start"`.
pub fn render_start(duration_sec: Option<u64>) -> String {
    match duration_sec {
        Some(d) => format!("start --duration={d}"),
        None => "start".to_string(),
    }
}

fn on_off(b: bool) -> &'static str {
    if b {
        "on"
    } else {
        "off"
    }
}

/// Per §2.3 — quote with `'…'` and encode embedded spaces as `0x1F` (US).
fn quote_for_wire(s: &str) -> String {
    if !s.contains(' ') {
        return s.to_string();
    }
    let encoded: String = s
        .chars()
        .map(|c| if c == ' ' { '\u{1F}' } else { c })
        .collect();
    format!("'{}'", encoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::config::{CsiDelivery, Ht40};

    #[test]
    fn esp_now_emits_peer_mac_and_ht40() {
        let cfg = EspConfig {
            wifi_mode: WifiMode::EspNowCentral,
            peer_mac: "aa:bb:cc:dd:ee:ff".into(),
            ht40: Ht40::Above,
            ..Default::default()
        };
        let lines = render_apply_config(&cfg, ChipFamily::Esp32);
        assert!(lines.contains(&"set-wifi --peer-mac=aa:bb:cc:dd:ee:ff".to_string()));
        assert!(lines.contains(&"set-wifi --ht40=above".to_string()));
    }

    #[test]
    fn esp_now_empty_peer_mac_clears_to_auto() {
        let cfg = EspConfig {
            wifi_mode: WifiMode::EspNowPeripheral,
            ..Default::default()
        };
        // empty peer_mac is still emitted (verbatim) to clear back to auto.
        let lines = render_apply_config(&cfg, ChipFamily::Esp32);
        assert!(lines.contains(&"set-wifi --peer-mac=".to_string()));
        assert!(lines.contains(&"set-wifi --ht40=none".to_string()));
    }

    #[test]
    fn non_esp_now_omits_peer_mac_and_ht40() {
        let cfg = EspConfig {
            wifi_mode: WifiMode::Sniffer,
            peer_mac: "aa:bb:cc:dd:ee:ff".into(),
            ..Default::default()
        };
        let lines = render_apply_config(&cfg, ChipFamily::Esp32);
        assert!(!lines.iter().any(|l| l.contains("--peer-mac")));
        assert!(!lines.iter().any(|l| l.contains("--ht40")));
    }

    #[test]
    fn renders_raw_csi_delivery() {
        let cfg = EspConfig {
            csi_delivery: CsiDelivery::Raw,
            ..Default::default()
        };
        let lines = render_apply_config(&cfg, ChipFamily::Esp32);
        assert!(lines.iter().any(|l| l.starts_with("set-csi-delivery --mode=raw")));
    }
}
