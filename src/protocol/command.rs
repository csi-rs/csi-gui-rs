use super::config::{EspConfig, LogMode, WifiMode};
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
        /// Wire format to sync via `set-log-mode` immediately before `start`.
        log_mode: LogMode,
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
    /// Soft firmware reboot via the `restart` command (device re-emits its
    /// banner, so the reader re-identifies afterwards).
    Restart,
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
    if !cfg.ap_ssid.is_empty() {
        out.push(format!("set-wifi --ap-ssid={}", quote_for_wire(&cfg.ap_ssid)));
    }
    if !cfg.ap_password.is_empty() {
        out.push(format!(
            "set-wifi --ap-password={}",
            quote_for_wire(&cfg.ap_password)
        ));
    }
    out.push(format!("set-wifi --ap-dhcp={}", on_off(cfg.ap_dhcp)));
    if matches!(
        cfg.wifi_mode,
        WifiMode::EspNowCentral
            | WifiMode::EspNowPeripheral
            | WifiMode::EspNowFastCollector
            | WifiMode::EspNowFastSource
    ) {
        out.push(format!("set-wifi --peer-mac={}", cfg.peer_mac.trim()));
        out.push(format!("set-wifi --ht40={}", cfg.ht40.cli()));
    }

    out.push(format!("set-protocol --protocol={}", cfg.protocol.cli()));
    out.push(format!(
        "set-traffic --frequency-hz={} --unsolicited={}",
        cfg.traffic_hz,
        on_off(cfg.traffic_unsolicited)
    ));
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

    // `set-csi` flags are positive on|off toggles (fw v0.7.x). Emit all of
    // them so a feature turned off earlier gets re-enabled without a
    // reset-config.
    if chip.is_classic() {
        out.push(format!(
            "set-csi --lltf={} --htltf={} --stbc-htltf={} --ltf-merge={}",
            on_off(cfg.classic_lltf_en),
            on_off(cfg.classic_htltf_en),
            on_off(cfg.classic_stbc_htltf_en),
            on_off(cfg.classic_ltf_merge_en),
        ));
    } else if chip.is_he() {
        // The `he20` preset overrides the individual acquisition flags on the
        // firmware side, so send it alone; otherwise send the explicit flags.
        if cfg.csi_preset == crate::protocol::config::CsiPreset::He20 {
            out.push("set-csi --preset=he20".to_string());
        } else {
            let mut parts = vec![
                "set-csi".to_string(),
                format!("--csi={}", on_off(cfg.he_csi_en)),
                format!("--csi-legacy={}", on_off(cfg.he_csi_legacy_en)),
                format!("--csi-ht20={}", on_off(cfg.he_csi_ht20_en)),
                format!("--csi-ht40={}", on_off(cfg.he_csi_ht40_en)),
                format!("--csi-su={}", on_off(cfg.he_csi_su_en)),
                format!("--csi-mu={}", on_off(cfg.he_csi_mu_en)),
                format!("--csi-dcm={}", on_off(cfg.he_csi_dcm_en)),
                format!("--csi-beamformed={}", on_off(cfg.he_csi_beamformed_en)),
                format!("--csi-he-stbc={}", cfg.he_stbc),
                format!("--val-scale-cfg={}", cfg.he_val_scale_cfg),
                format!("--dump-ack={}", on_off(cfg.csi_dump_ack)),
            ];
            // C5-only acquisition toggles.
            if chip == ChipFamily::Esp32C5 {
                parts.push(format!("--csi-force-lltf={}", on_off(cfg.c5_csi_force_lltf)));
                parts.push(format!("--csi-vht={}", on_off(cfg.c5_csi_vht)));
            }
            out.push(parts.join(" "));
        }
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

/// Render the `restart` command line (soft firmware reboot).
pub fn render_restart() -> String {
    "restart".to_string()
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
