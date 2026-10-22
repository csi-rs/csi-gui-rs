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
        /// `set-session` to send before `start`: the session id and the
        /// host's wall-clock time in µs. "Start all" gives every device the
        /// same id so their frames can be lined up.
        session: Option<(u32, u64)>,
    },
    /// Stop collection (`q`).
    Stop,
    /// Apply the entire `set-*` surface. Ignored if `is_collecting`.
    ApplyConfig {
        /// The new config — see [`crate::protocol::config::EspConfig`].
        /// Boxed: it is by far the largest variant.
        cfg: Box<EspConfig>,
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

/// Built-in `set-csi --preset=` names the firmware itself accepts (on the
/// ESP32-C5/C6). Profiles ([`crate::protocol::profile`]) can add more.
pub const BUILTIN_CSI_PRESETS: &[&str] = &["he20"];

/// Every preset the config panel offers: the built-ins, then the installed
/// profile's.
pub fn csi_preset_names() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = BUILTIN_CSI_PRESETS.to_vec();
    v.extend(active_profile().preset_names().iter().copied());
    v
}

/// Fold a preset into `cfg`'s individual acquisition toggles so the panel
/// shows what the device will run.
pub fn apply_csi_preset(name: &str, cfg: &mut EspConfig) {
    if name == "he20" {
        // Mirrors the firmware: HE-LTF only; legacy, HT and ACK off.
        cfg.c5c6_csi_en = true;
        cfg.c5c6_csi_legacy_en = false;
        cfg.c5c6_csi_ht20_en = false;
        cfg.c5c6_csi_ht40_en = false;
        cfg.csi_dump_ack = false;
        cfg.c5c6_csi_su = true;
    } else {
        active_profile().apply_preset(name, cfg);
    }
}

fn preset_cli(name: &str) -> Option<String> {
    if BUILTIN_CSI_PRESETS.contains(&name) {
        Some(format!("set-csi --preset={name}"))
    } else {
        active_profile().preset_cli(name)
    }
}

/// Render an `EspConfig` to the ordered list of CLI strings pushed by
/// `ApplyConfig`.
///
/// Settings a mode ignores are not sent, so the firmware never "believes"
/// a value it would not apply (and the console stays readable).
/// `set-log-mode` is emitted **last** so `io_generation` bumps after every
/// stateful parser has been torn down.
pub fn render_apply_config(cfg: &EspConfig, chip: ChipFamily) -> Vec<String> {
    let mode = cfg.wifi_mode;
    let mut out: Vec<String> = Vec::new();
    out.push(format!("set-wifi --mode={}", mode.cli()));
    // In Station mode the channel follows the associated AP, so the firmware
    // ignores an explicit channel — omit it (mirrors csi-webserver-rs).
    if mode != WifiMode::Station {
        out.push(format!("set-wifi --set-channel={}", cfg.channel));
    }
    if mode == WifiMode::Station {
        if !cfg.sta_ssid.is_empty() {
            out.push(format!("set-wifi --sta-ssid={}", quote_for_wire(&cfg.sta_ssid)));
        }
        if !cfg.sta_password.is_empty() {
            out.push(format!(
                "set-wifi --sta-password={}",
                quote_for_wire(&cfg.sta_password)
            ));
        }
    }
    if mode == WifiMode::WifiAp {
        if !cfg.ap_ssid.is_empty() {
            out.push(format!("set-wifi --ap-ssid={}", quote_for_wire(&cfg.ap_ssid)));
        }
        if !cfg.ap_password.is_empty() {
            out.push(format!(
                "set-wifi --ap-password={}",
                quote_for_wire(&cfg.ap_password)
            ));
        }
        out.push(format!(
            "set-wifi --ap-dhcp={} --ap-leases={} --ap-burst={}",
            on_off(cfg.ap_dhcp),
            cfg.ap_leases,
            on_off(cfg.ap_burst)
        ));
    }
    // `peer-mac` is an emitter's injection destination and an ESP-NOW node's
    // explicit peer. An empty value is sent verbatim to clear it (back to
    // broadcast, or back to automatic pairing).
    if mode.uses_peer_mac() {
        out.push(format!("set-wifi --peer-mac={}", cfg.peer_mac.trim()));
    }
    // `ht40` is the softAP's secondary channel in `wifi-ap` and the forced
    // per-peer TX PHY in the ESP-NOW modes; other modes would get a spurious
    // set-wifi line.
    if mode.uses_ht40() {
        out.push(format!("set-wifi --ht40={}", cfg.ht40.cli()));
    }
    if mode.uses_he20() && chip.is_c5c6() {
        out.push(format!("set-wifi --he20={}", on_off(cfg.he20)));
    }
    if mode.is_emitter() {
        out.push(format!(
            "set-wifi --inject-period-us={} --emitter-iface={}",
            cfg.inject_period_us,
            cfg.emitter_iface.cli()
        ));
    }
    // Reporting is only read by the modes that admit a choice; the rest fix
    // it, so it is not sent there. `set-reporting` carries the policy and
    // its parameters in one line.
    if mode.admits_reporting_choice() {
        out.push(format!(
            "set-reporting --policy={} --report-to={} --level={} --hold-ms={} --n={}",
            cfg.reporting.cli(),
            cfg.report_to.cli(),
            cfg.threshold_level,
            cfg.threshold_hold_ms,
            cfg.decimate_n
        ));
    }

    out.push(format!("set-protocol --protocol={}", cfg.protocol.cli()));
    out.push(format!(
        "set-measurement --periodicity-hz={} --unsolicited={}",
        cfg.periodicity_hz,
        on_off(cfg.unsolicited)
    ));
    out.push(format!("set-csi-output --enabled={}", cfg.csi_output));
    let filter_peer = cfg.csi_filter_peer.trim();
    out.push(format!(
        "set-csi-filter --peer-mac={} --min-phy={}",
        if filter_peer.is_empty() { "any" } else { filter_peer },
        cfg.csi_filter_min_phy.cli()
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

    // `set-csi` flags are positive on|off toggles. Emit all of them so a
    // feature turned off earlier gets re-enabled without a reset-config.
    if chip.is_classic() {
        out.push(format!(
            "set-csi --lltf={} --htltf={} --stbc-htltf={} --ltf-merge={}",
            on_off(cfg.classic_lltf_en),
            on_off(cfg.classic_htltf_en),
            on_off(cfg.classic_stbc_htltf_en),
            on_off(cfg.classic_ltf_merge_en),
        ));
    } else if chip.is_c5c6() {
        // A preset overrides the individual acquisition flags on the firmware
        // side, so it is sent alone. An unresolved name falls back to the
        // explicit flags rather than pushing a token the firmware would reject.
        if let Some(line) = cfg.csi_preset.as_deref().and_then(preset_cli) {
            out.push(line);
        } else {
            let mut parts = vec![
                "set-csi".to_string(),
                format!("--csi={}", on_off(cfg.c5c6_csi_en)),
                format!("--csi-legacy={}", on_off(cfg.c5c6_csi_legacy_en)),
                format!("--csi-ht20={}", on_off(cfg.c5c6_csi_ht20_en)),
                format!("--csi-ht40={}", on_off(cfg.c5c6_csi_ht40_en)),
                format!("--val-scale-cfg={}", cfg.c5c6_val_scale_cfg),
                format!("--dump-ack={}", on_off(cfg.csi_dump_ack)),
                format!("--csi-su={}", on_off(cfg.c5c6_csi_su)),
                format!("--csi-mu={}", on_off(cfg.c5c6_csi_mu)),
                format!("--csi-dcm={}", on_off(cfg.c5c6_csi_dcm)),
                format!("--csi-beamformed={}", on_off(cfg.c5c6_csi_beamformed)),
                format!("--csi-he-stbc={}", on_off(cfg.c5c6_csi_he_stbc)),
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

/// Render `set-session`: stamp the next run's frames with `id` and, when
/// given, announce `epoch_unix_us` as its wall-clock anchor.
pub fn render_set_session(id: u32, epoch_unix_us: Option<u64>) -> String {
    match epoch_unix_us {
        Some(e) => format!("set-session --id=0x{id:08x} --epoch={e}"),
        None => format!("set-session --id=0x{id:08x}"),
    }
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
    use crate::protocol::config::{CsiDelivery, Ht40, Reporting};
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
    fn receive_only_modes_omit_destination_mac() {
        let cfg = EspConfig {
            wifi_mode: WifiMode::Sniffer,
            peer_mac: "aa:bb:cc:dd:ee:ff".into(),
            ..Default::default()
        };
        let lines = render_apply_config(&cfg, ChipFamily::Esp32);
        assert!(!lines.iter().any(|l| l.starts_with("set-wifi --peer-mac")));
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
        assert!(!lines.iter().any(|l| l.starts_with("set-wifi --peer-mac")));
    }

    #[test]
    fn reporting_sent_only_where_admitted() {
        for mode in [
            WifiMode::Station,
            WifiMode::Sniffer,
            WifiMode::WifiAp,
            WifiMode::EspNowInitiator,
            WifiMode::EspNowResponder,
        ] {
            let cfg = EspConfig {
                wifi_mode: mode,
                reporting: Reporting::Decimate,
                decimate_n: 5,
                ..Default::default()
            };
            let lines = render_apply_config(&cfg, ChipFamily::Esp32);
            assert!(
                lines.contains(
                    &"set-reporting --policy=decimate --report-to=host --level=6000 --hold-ms=1000 --n=5"
                        .to_string()
                ),
                "{mode:?} should send set-reporting"
            );
        }
        for mode in [
            WifiMode::Ht20Emitter,
            WifiMode::Ht40Emitter,
            WifiMode::EspNowSimplexSource,
            WifiMode::EspNowSimplexPeer,
        ] {
            let cfg = EspConfig {
                wifi_mode: mode,
                reporting: Reporting::None,
                ..Default::default()
            };
            let lines = render_apply_config(&cfg, ChipFamily::Esp32);
            assert!(
                !lines.iter().any(|l| l.contains("set-reporting")),
                "{mode:?} fixes its reporting"
            );
        }
    }

    #[test]
    fn esp_now_pair_emits_peer_mac_and_ht40() {
        let cfg = EspConfig {
            wifi_mode: WifiMode::EspNowResponder,
            peer_mac: "aa:bb:cc:dd:ee:ff".into(),
            ht40: Ht40::Below,
            ..Default::default()
        };
        let lines = render_apply_config(&cfg, ChipFamily::Esp32);
        assert!(lines.contains(&"set-wifi --peer-mac=aa:bb:cc:dd:ee:ff".to_string()));
        assert!(lines.contains(&"set-wifi --ht40=below".to_string()));
        assert!(lines.iter().any(|l| l.starts_with("set-reporting --policy=report")));
    }

    #[test]
    fn simplex_peer_takes_peer_mac_but_not_ht40() {
        let cfg = EspConfig {
            wifi_mode: WifiMode::EspNowSimplexPeer,
            peer_mac: "aa:bb:cc:dd:ee:ff".into(),
            ht40: Ht40::Above,
            ..Default::default()
        };
        let lines = render_apply_config(&cfg, ChipFamily::Esp32);
        assert!(lines.contains(&"set-wifi --peer-mac=aa:bb:cc:dd:ee:ff".to_string()));
        assert!(!lines.iter().any(|l| l.contains("--ht40")));
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
    fn measurement_uses_periodicity_hz() {
        let cfg = EspConfig {
            periodicity_hz: 250,
            unsolicited: true,
            ..Default::default()
        };
        let lines = render_apply_config(&cfg, ChipFamily::Esp32);
        assert!(lines.contains(&"set-measurement --periodicity-hz=250 --unsolicited=on".to_string()));
        assert!(!lines.iter().any(|l| l.starts_with("set-traffic")));
    }

    #[test]
    fn esp_now_modes_use_bf_tokens() {
        for (mode, token) in [
            (WifiMode::EspNowInitiator, "esp-now-initiator"),
            (WifiMode::EspNowResponder, "esp-now-responder"),
            (WifiMode::EspNowSimplexSource, "esp-now-simplex-source"),
            (WifiMode::EspNowSimplexPeer, "esp-now-simplex-peer"),
        ] {
            let cfg = EspConfig {
                wifi_mode: mode,
                ..Default::default()
            };
            let lines = render_apply_config(&cfg, ChipFamily::Esp32);
            assert!(lines.contains(&format!("set-wifi --mode={token}")));
        }
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
    fn builtin_he20_preset_and_he20_mode_flag() {
        let cfg = EspConfig {
            wifi_mode: WifiMode::EspNowInitiator,
            he20: true,
            csi_preset: Some("he20".into()),
            ..Default::default()
        };
        let lines = render_apply_config(&cfg, ChipFamily::Esp32C5);
        assert!(lines.contains(&"set-csi --preset=he20".to_string()));
        assert!(lines.contains(&"set-wifi --he20=on".to_string()));
        // Classic chips have no HE: neither line goes out.
        let lines = render_apply_config(&cfg, ChipFamily::Esp32S3);
        assert!(!lines.iter().any(|l| l.contains("he20")));
    }

    #[test]
    fn c5c6_set_csi_carries_he_flags() {
        let lines = render_apply_config(&EspConfig::default(), ChipFamily::Esp32C6);
        let csi = lines.iter().find(|l| l.starts_with("set-csi --csi=")).unwrap();
        for flag in ["--csi-su=on", "--csi-mu=on", "--csi-dcm=on", "--csi-beamformed=on", "--csi-he-stbc=off"] {
            assert!(csi.contains(flag), "{csi} lacks {flag}");
        }
        assert!(!csi.contains("--csi-vht"), "VHT is C5-only");
    }

    #[test]
    fn ap_extras_and_emitter_period_only_in_their_modes() {
        let ap = EspConfig { wifi_mode: WifiMode::WifiAp, ap_leases: 6, ap_burst: true, ..Default::default() };
        let lines = render_apply_config(&ap, ChipFamily::Esp32);
        assert!(lines.contains(&"set-wifi --ap-dhcp=on --ap-leases=6 --ap-burst=on".to_string()));
        assert!(!lines.iter().any(|l| l.contains("--inject-period")));

        let em = EspConfig { wifi_mode: WifiMode::Ht20Emitter, inject_period_us: 5000, ..Default::default() };
        let lines = render_apply_config(&em, ChipFamily::Esp32);
        assert!(lines.contains(&"set-wifi --inject-period-us=5000 --emitter-iface=sta".to_string()));
        assert!(!lines.iter().any(|l| l.contains("--ap-dhcp")));
    }

    #[test]
    fn csi_filter_blank_means_any() {
        let lines = render_apply_config(&EspConfig::default(), ChipFamily::Esp32);
        assert!(lines.contains(&"set-csi-filter --peer-mac=any --min-phy=any".to_string()));
        let cfg = EspConfig { csi_filter_peer: "aa:bb:cc:dd:ee:ff".into(), ..Default::default() };
        let lines = render_apply_config(&cfg, ChipFamily::Esp32);
        assert!(lines.contains(&"set-csi-filter --peer-mac=aa:bb:cc:dd:ee:ff --min-phy=any".to_string()));
    }

    #[test]
    fn set_session_renders_hex_id() {
        assert_eq!(render_set_session(42, None), "set-session --id=0x0000002a");
        assert_eq!(
            render_set_session(0xdead_beef, Some(1_791_234_567_000_000)),
            "set-session --id=0xdeadbeef --epoch=1791234567000000"
        );
    }

    #[test]
    fn log_mode_is_last() {
        let lines = render_apply_config(&EspConfig::default(), ChipFamily::Esp32C5);
        assert!(lines.last().unwrap().starts_with("set-log-mode"));
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
