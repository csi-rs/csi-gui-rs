use super::config::{EspConfig, LogMode, WifiMode};
use crate::protocol::info::ChipFamily;
use crate::protocol::profile::active_profile;

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
    // In Station mode the channel follows the associated AP, so the firmware
    // ignores an explicit channel — omit it (mirrors csi-webserver-rs).
    if cfg.wifi_mode != WifiMode::Station {
        out.push(format!("set-wifi --set-channel={}", cfg.channel));
    }
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
    // An emitter takes its bandwidth from the mode token itself and a
    // destination for the injected frames. An empty `peer-mac` is sent verbatim
    // to clear any prior destination back to broadcast.
    if cfg.wifi_mode.is_emitter() {
        out.push(format!("set-wifi --peer-mac={}", cfg.peer_mac.trim()));
    }
    // `ht40` is the softAP's secondary channel, so only `wifi-ap` uses it;
    // other modes would get a spurious set-wifi line.
    if cfg.wifi_mode == WifiMode::WifiAp {
        out.push(format!("set-wifi --ht40={}", cfg.ht40.cli()));
    }

    out.push(format!("set-protocol --protocol={}", cfg.protocol.cli()));
    out.push(format!(
        "set-traffic --frequency-hz={} --unsolicited={}",
        cfg.traffic_hz,
        on_off(cfg.traffic_unsolicited)
    ));
    out.push(format!("set-csi-output --enabled={}", cfg.csi_output));
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
        // A preset overrides the individual acquisition flags on the firmware
        // side, so it is sent alone. Preset names are resolved by the installed
        // profile — an unresolved name falls back to the explicit flags rather
        // than pushing a token the firmware would reject.
        let preset = cfg
            .csi_preset
            .as_deref()
            .and_then(|name| active_profile().preset_cli(name));
        if let Some(line) = preset {
            out.push(line);
        } else {
            let mut parts = vec![
                "set-csi".to_string(),
                format!("--csi={}", on_off(cfg.he_csi_en)),
                format!("--csi-legacy={}", on_off(cfg.he_csi_legacy_en)),
                format!("--csi-ht20={}", on_off(cfg.he_csi_ht20_en)),
                format!("--csi-ht40={}", on_off(cfg.he_csi_ht40_en)),
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
    use crate::protocol::profile::{install_profile, CsiProfile};

    #[test]
    fn emitter_emits_destination_mac() {
        let cfg = EspConfig {
            wifi_mode: WifiMode::Ht40Emitter,
            peer_mac: "aa:bb:cc:dd:ee:ff".into(),
            ..Default::default()
        };
        let lines = render_apply_config(&cfg, ChipFamily::Esp32);
        assert!(lines.contains(&"set-wifi --mode=ht40-emitter".to_string()));
        assert!(lines.contains(&"set-wifi --peer-mac=aa:bb:cc:dd:ee:ff".to_string()));
        // Bandwidth rides on the mode token, so no `--ht40` for an emitter.
        assert!(!lines.iter().any(|l| l.contains("--ht40=")));
    }

    #[test]
    fn emitter_empty_destination_mac_clears_to_broadcast() {
        let cfg = EspConfig {
            wifi_mode: WifiMode::Ht20Emitter,
            ..Default::default()
        };
        // empty peer_mac is still emitted (verbatim) to clear back to broadcast.
        let lines = render_apply_config(&cfg, ChipFamily::Esp32);
        assert!(lines.contains(&"set-wifi --mode=ht20-emitter".to_string()));
        assert!(lines.contains(&"set-wifi --peer-mac=".to_string()));
    }

    #[test]
    fn collector_modes_omit_destination_mac() {
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
    fn wifi_ap_emits_ht40_secondary() {
        let cfg = EspConfig {
            wifi_mode: WifiMode::WifiAp,
            ht40: Ht40::Above,
            ..Default::default()
        };
        let lines = render_apply_config(&cfg, ChipFamily::Esp32);
        assert!(lines.contains(&"set-wifi --ht40=above".to_string()));
        assert!(!lines.iter().any(|l| l.contains("--peer-mac")));
    }

    #[test]
    fn csi_output_disabled_emits_false() {
        let cfg = EspConfig {
            csi_output: false,
            ..Default::default()
        };
        let lines = render_apply_config(&cfg, ChipFamily::Esp32);
        assert!(lines.contains(&"set-csi-output --enabled=false".to_string()));
        assert!(!lines.iter().any(|l| l.starts_with("set-collection-mode")));
    }

    #[test]
    fn station_mode_omits_set_channel() {
        let cfg = EspConfig {
            wifi_mode: WifiMode::Station,
            channel: 6,
            ..Default::default()
        };
        let lines = render_apply_config(&cfg, ChipFamily::Esp32);
        assert!(!lines.iter().any(|l| l.contains("--set-channel")));
    }

    #[test]
    fn non_station_mode_emits_set_channel() {
        let cfg = EspConfig {
            wifi_mode: WifiMode::Sniffer,
            channel: 6,
            ..Default::default()
        };
        let lines = render_apply_config(&cfg, ChipFamily::Esp32);
        assert!(lines.contains(&"set-wifi --set-channel=6".to_string()));
    }

    /// The only test that installs a profile: `active_profile()` is a
    /// process-wide `OnceLock`, and every other test leaves `csi_preset` at
    /// `None`, where the profile is never consulted.
    #[test]
    fn profile_preset_collapses_to_single_flag() {
        struct TestProfile;
        impl CsiProfile for TestProfile {
            fn preset_names(&self) -> &'static [&'static str] {
                &["probe"]
            }
            fn preset_cli(&self, name: &str) -> Option<String> {
                (name == "probe").then(|| "set-csi --preset=probe".to_string())
            }
        }
        install_profile(&TestProfile);

        let cfg = EspConfig {
            csi_preset: Some("probe".into()),
            ..Default::default()
        };
        let lines = render_apply_config(&cfg, ChipFamily::Esp32C6);
        assert!(lines.contains(&"set-csi --preset=probe".to_string()));
        // The verbose per-flag `set-csi --csi=…` line must not also appear.
        assert!(!lines.iter().any(|l| l.starts_with("set-csi --csi=")));

        // A name the profile does not resolve falls back to the flags.
        let cfg = EspConfig {
            csi_preset: Some("nope".into()),
            ..Default::default()
        };
        let lines = render_apply_config(&cfg, ChipFamily::Esp32C6);
        assert!(!lines.iter().any(|l| l.contains("--preset=")));
        assert!(lines.iter().any(|l| l.starts_with("set-csi --csi=")));
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
