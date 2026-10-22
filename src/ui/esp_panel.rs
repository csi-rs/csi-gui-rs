//! The device configuration form.
//!
//! Every editable `set-*` knob is a [`Field`]. Each field knows its label,
//! its section, and whether it applies to a given mode and chip, so the
//! form only shows settings the firmware will actually use. Whole sections
//! (station credentials, softAP, emitter, the C5/C6 acquisition flags)
//! disappear when they don't apply, instead of sitting greyed out.

use crate::protocol::command::{apply_csi_preset, csi_preset_names};
use crate::protocol::config::{
    CsiDelivery, EmitterIface, EspConfig, Ht40, LogMode, MinPhy, PhyRate, ReportTo, Reporting,
    WifiMode, WifiProtocol, valid_channel,
};
use crate::protocol::info::ChipFamily;

/// UI shell state for the right-hand device panel.
pub struct EspPanel {
    /// Whether the side panel is expanded.
    pub open: bool,
    /// Text of the raw-command box.
    pub raw_command: String,
    /// Index of the device picked as the ESP-NOW responder in the pairing row.
    pub pair_with: Option<usize>,
}

impl Default for EspPanel {
    fn default() -> Self {
        Self {
            open: true,
            raw_command: String::new(),
            pair_with: None,
        }
    }
}

/// Form sections, in display order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    /// Mode, channel, protocol and the per-mode radio knobs.
    Node,
    /// Station credentials.
    Station,
    /// SoftAP settings.
    AccessPoint,
    /// Emitter pacing.
    Emitter,
    /// Reporting policy and measurement traffic.
    Reporting,
    /// What the firmware delivers and how.
    Output,
    /// Which training fields the radio captures.
    Acquisition,
    /// Run length.
    Run,
}

impl Section {
    /// Every section, in order.
    pub const ALL: [Section; 8] = [
        Section::Node,
        Section::Station,
        Section::AccessPoint,
        Section::Emitter,
        Section::Reporting,
        Section::Output,
        Section::Acquisition,
        Section::Run,
    ];

    /// Heading text.
    pub fn title(self) -> &'static str {
        match self {
            Section::Node => "Node",
            Section::Station => "Station",
            Section::AccessPoint => "Access point",
            Section::Emitter => "Emitter",
            Section::Reporting => "Reporting & traffic",
            Section::Output => "Output",
            Section::Acquisition => "CSI acquisition",
            Section::Run => "Run",
        }
    }
}

/// One editable setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(missing_docs)] // each variant is the setting its label names
pub enum Field {
    Mode,
    Channel,
    Protocol,
    He20,
    PeerMac,
    Ht40,
    StaSsid,
    StaPassword,
    ApSsid,
    ApPassword,
    ApDhcp,
    ApLeases,
    ApBurst,
    InjectPeriod,
    EmitterIface,
    Reporting,
    ReportTo,
    ThresholdLevel,
    ThresholdHold,
    DecimateN,
    Periodicity,
    Unsolicited,
    LogMode,
    CsiOutput,
    FilterPeer,
    FilterMinPhy,
    Delivery,
    CsiLogging,
    TxTask,
    RxTask,
    PhyRate,
    Preset,
    ClassicLltf,
    ClassicHtltf,
    ClassicStbcHtltf,
    ClassicLtfMerge,
    C5c6Csi,
    C5c6Legacy,
    C5c6Ht20,
    C5c6Ht40,
    C5c6Su,
    C5c6Mu,
    C5c6Dcm,
    C5c6Beamformed,
    C5c6HeStbc,
    C5c6ValScale,
    DumpAck,
    C5ForceLltf,
    C5Vht,
    Duration,
}

impl Field {
    /// Every field, in display order (grouped by [`Self::section`]).
    pub const ALL: [Field; 50] = [
        Field::Mode,
        Field::Channel,
        Field::Protocol,
        Field::He20,
        Field::PeerMac,
        Field::Ht40,
        Field::StaSsid,
        Field::StaPassword,
        Field::ApSsid,
        Field::ApPassword,
        Field::ApDhcp,
        Field::ApLeases,
        Field::ApBurst,
        Field::InjectPeriod,
        Field::EmitterIface,
        Field::Reporting,
        Field::ReportTo,
        Field::ThresholdLevel,
        Field::ThresholdHold,
        Field::DecimateN,
        Field::Periodicity,
        Field::Unsolicited,
        Field::LogMode,
        Field::CsiOutput,
        Field::FilterPeer,
        Field::FilterMinPhy,
        Field::Delivery,
        Field::CsiLogging,
        Field::TxTask,
        Field::RxTask,
        Field::PhyRate,
        Field::Preset,
        Field::ClassicLltf,
        Field::ClassicHtltf,
        Field::ClassicStbcHtltf,
        Field::ClassicLtfMerge,
        Field::C5c6Csi,
        Field::C5c6Legacy,
        Field::C5c6Ht20,
        Field::C5c6Ht40,
        Field::C5c6Su,
        Field::C5c6Mu,
        Field::C5c6Dcm,
        Field::C5c6Beamformed,
        Field::C5c6HeStbc,
        Field::C5c6ValScale,
        Field::DumpAck,
        Field::C5ForceLltf,
        Field::C5Vht,
        Field::Duration,
    ];

    /// Row label.
    pub fn label(self) -> &'static str {
        match self {
            Field::Mode => "Mode",
            Field::Channel => "Channel",
            Field::Protocol => "Wi-Fi protocol",
            Field::He20 => "HE20 (802.11ax)",
            Field::PeerMac => "Peer MAC",
            Field::Ht40 => "HT40",
            Field::StaSsid => "SSID",
            Field::StaPassword => "Password",
            Field::ApSsid => "SSID",
            Field::ApPassword => "Password",
            Field::ApDhcp => "DHCP server",
            Field::ApLeases => "DHCP leases",
            Field::ApBurst => "Burst flood",
            Field::InjectPeriod => "Inject period (µs)",
            Field::EmitterIface => "Interface",
            Field::Reporting => "Reporting",
            Field::ReportTo => "Report to",
            Field::ThresholdLevel => "Threshold level",
            Field::ThresholdHold => "Hold (ms)",
            Field::DecimateN => "Every n-th",
            Field::Periodicity => "Periodicity (Hz)",
            Field::Unsolicited => "Unsolicited replies",
            Field::LogMode => "Log mode",
            Field::CsiOutput => "CSI output",
            Field::FilterPeer => "Only from MAC",
            Field::FilterMinPhy => "Minimum PHY",
            Field::Delivery => "Delivery",
            Field::CsiLogging => "Per-packet logging",
            Field::TxTask => "TX task",
            Field::RxTask => "RX task",
            Field::PhyRate => "PHY rate",
            Field::Preset => "Preset",
            Field::ClassicLltf => "L-LTF",
            Field::ClassicHtltf => "HT-LTF",
            Field::ClassicStbcHtltf => "STBC HT-LTF",
            Field::ClassicLtfMerge => "LTF merge",
            Field::C5c6Csi => "CSI (master)",
            Field::C5c6Legacy => "Legacy (11g)",
            Field::C5c6Ht20 => "HT20",
            Field::C5c6Ht40 => "HT40",
            Field::C5c6Su => "HE SU",
            Field::C5c6Mu => "HE MU",
            Field::C5c6Dcm => "HE DCM",
            Field::C5c6Beamformed => "HE beamformed",
            Field::C5c6HeStbc => "HE STBC",
            Field::C5c6ValScale => "Value scale (0–3)",
            Field::DumpAck => "Dump ACK frames",
            Field::C5ForceLltf => "Force L-LTF (C5)",
            Field::C5Vht => "VHT20 (C5)",
            Field::Duration => "Duration",
        }
    }

    /// Hover text explaining the setting.
    pub fn help(self) -> &'static str {
        match self {
            Field::Mode => {
                "How the node reaches the channel. Session and sensing roles follow from it."
            }
            Field::Channel => {
                "Wi-Fi channel. Ignored in station mode, where the AP picks it. 5 GHz needs an ESP32-C5."
            }
            Field::Protocol => "set-protocol. `ax` is ESP32-C5/C6 only.",
            Field::He20 => {
                "Force HE20 (802.11ax SU) on the emitters and the ESP-NOW pair. Pair the receivers with the he20 preset."
            }
            Field::PeerMac => {
                "Emitters: injection destination (blank = broadcast). ESP-NOW: explicit peer (blank = auto-pair; set it on both ends)."
            }
            Field::Ht40 => {
                "wifi-ap: softAP secondary channel. ESP-NOW: force the per-peer TX PHY to HT40."
            }
            Field::ApLeases => {
                "DHCP pool size. More than one lease round-robins the flood across every station."
            }
            Field::ApBurst => {
                "Send one frame to every station back-to-back each tick: time-aligned multi-receiver capture."
            }
            Field::InjectPeriod => "Delay between injected frames. 20000 µs ≈ 50 frames/s.",
            Field::Reporting => {
                "Whether measurements leave the node: every one, none, only while the channel moves (threshold), or every n-th."
            }
            Field::ReportTo => {
                "host = the serial output this GUI reads. local = on-device consumers only (plots stay empty)."
            }
            Field::ThresholdLevel => {
                "Score at or above which a measurement is reported. Uncalibrated: a still room is not zero, so calibrate on your link."
            }
            Field::ThresholdHold => "Keep reporting this long after crossing the threshold.",
            Field::DecimateN => "Report every n-th measurement.",
            Field::Periodicity => {
                "Rate of the sounding traffic this node generates. 0 disables it."
            }
            Field::Unsolicited => {
                "Flood echo replies instead of requests: one-directional traffic, no CSI back to this node."
            }
            Field::LogMode => {
                "Output format. serialized is the most compact and carries the most metadata."
            }
            Field::CsiOutput => "Runtime delivery gate: off keeps capturing but delivers nothing.",
            Field::FilterPeer => "Deliver CSI only for frames from this transmitter. Blank = any.",
            Field::FilterMinPhy => "ht drops legacy 11b/g frames.",
            Field::Delivery => "raw is a zero-copy benchmark path: nothing is delivered or logged.",
            Field::PhyRate => "Reporting only, except on the ESP-NOW pair, which applies it.",
            Field::Preset => {
                "he20: HE-LTF only (legacy, HT and ACK off), for the receivers of an HE20 link."
            }
            Field::Duration => "Stop the run by itself after this long. Off = until stopped.",
            _ => "",
        }
    }

    /// The section the field lives in.
    pub fn section(self) -> Section {
        use Field::*;
        match self {
            Mode | Channel | Protocol | He20 | PeerMac | Ht40 => Section::Node,
            StaSsid | StaPassword => Section::Station,
            ApSsid | ApPassword | ApDhcp | ApLeases | ApBurst => Section::AccessPoint,
            InjectPeriod | EmitterIface => Section::Emitter,
            Reporting | ReportTo | ThresholdLevel | ThresholdHold | DecimateN | Periodicity
            | Unsolicited => Section::Reporting,
            LogMode | CsiOutput | FilterPeer | FilterMinPhy | Delivery | CsiLogging | TxTask
            | RxTask | PhyRate => Section::Output,
            Preset | ClassicLltf | ClassicHtltf | ClassicStbcHtltf | ClassicLtfMerge | C5c6Csi
            | C5c6Legacy | C5c6Ht20 | C5c6Ht40 | C5c6Su | C5c6Mu | C5c6Dcm | C5c6Beamformed
            | C5c6HeStbc | C5c6ValScale | DumpAck | C5ForceLltf | C5Vht => Section::Acquisition,
            Duration => Section::Run,
        }
    }

    /// Whether the field applies to `cfg` on `chip`. Fields that don't are
    /// not shown. An unidentified chip shows the fields of every chip.
    pub fn visible(self, cfg: &EspConfig, chip: ChipFamily) -> bool {
        use Field::*;
        let mode = cfg.wifi_mode;
        let unknown = chip == ChipFamily::Unknown;
        let classic = chip.is_classic() || unknown;
        let c5c6 = chip.is_c5c6() || unknown;
        let c5 = chip == ChipFamily::Esp32C5 || unknown;
        let preset = cfg.csi_preset.is_some();
        match self {
            Channel => mode != WifiMode::Station,
            He20 => mode.uses_he20() && c5c6,
            PeerMac => mode.uses_peer_mac(),
            Ht40 => mode.uses_ht40(),
            StaSsid | StaPassword => mode == WifiMode::Station,
            ApSsid | ApPassword | ApDhcp | ApLeases | ApBurst => mode == WifiMode::WifiAp,
            InjectPeriod | EmitterIface => mode.is_emitter(),
            Reporting | ReportTo => mode.admits_reporting_choice(),
            ThresholdLevel | ThresholdHold => {
                mode.admits_reporting_choice()
                    && cfg.reporting == crate::protocol::config::Reporting::Threshold
            }
            DecimateN => {
                mode.admits_reporting_choice()
                    && cfg.reporting == crate::protocol::config::Reporting::Decimate
            }
            Periodicity | Unsolicited => mode.uses_periodicity(),
            // CSI capture settings mean nothing on a node that captures none.
            CsiOutput | FilterPeer | FilterMinPhy | Delivery | CsiLogging => !mode.reports_no_csi(),
            Preset => c5c6 && !mode.reports_no_csi(),
            ClassicLltf | ClassicHtltf | ClassicStbcHtltf | ClassicLtfMerge => {
                classic && !mode.reports_no_csi()
            }
            C5c6Csi | C5c6Legacy | C5c6Ht20 | C5c6Ht40 | C5c6Su | C5c6Mu | C5c6Dcm
            | C5c6Beamformed | C5c6HeStbc | C5c6ValScale | DumpAck => {
                c5c6 && !mode.reports_no_csi() && !preset
            }
            C5ForceLltf | C5Vht => c5 && !mode.reports_no_csi() && !preset,
            Mode | Protocol | LogMode | TxTask | RxTask | PhyRate | Duration => true,
        }
    }
}

/// Channels offered for `chip`: 2.4 GHz everywhere, 5 GHz on the C5 (and
/// on a chip not identified yet).
pub fn channels_for(chip: ChipFamily) -> Vec<u8> {
    let mut v: Vec<u8> = (1..=14).collect();
    if chip == ChipFamily::Esp32C5 || chip == ChipFamily::Unknown {
        v.extend([36, 40, 44, 48, 149, 153, 157, 161, 165]);
    }
    v.retain(|c| valid_channel(*c));
    v
}

/// Render the form for `cfg` on `chip`. Returns `true` when anything
/// changed. `id` keeps widget ids unique when several forms exist.
pub fn config_form(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash,
    cfg: &mut EspConfig,
    chip: ChipFamily,
) -> bool {
    let mut changed = false;
    ui.push_id(id, |ui| {
        for section in Section::ALL {
            let fields: Vec<Field> = Field::ALL
                .into_iter()
                .filter(|f| f.section() == section && f.visible(cfg, chip))
                .collect();
            if fields.is_empty() {
                continue;
            }
            ui.add_space(4.0);
            ui.label(egui::RichText::new(section.title()).strong().small());
            egui::Grid::new(("cfg-grid", section.title()))
                .num_columns(2)
                .spacing([10.0, 4.0])
                .min_col_width(110.0)
                .striped(true)
                .show(ui, |ui| {
                    for f in fields {
                        let label = ui.label(f.label());
                        if !f.help().is_empty() {
                            label.on_hover_text(f.help());
                        }
                        changed |= field_editor(ui, f, cfg, chip);
                        ui.end_row();
                    }
                });
        }
    });
    changed
}

fn field_editor(ui: &mut egui::Ui, f: Field, cfg: &mut EspConfig, chip: ChipFamily) -> bool {
    match f {
        Field::Mode => {
            let before = cfg.wifi_mode;
            let changed = combo(ui, "mode", &mut cfg.wifi_mode, &WifiMode::ALL, |m| m.cli());
            if changed && before != cfg.wifi_mode {
                // A sniffer can't report none: move it to the default.
                if !cfg.reporting.allowed_for(cfg.wifi_mode) {
                    cfg.reporting = Reporting::Report;
                }
            }
            ui.end_row();
            ui.label("");
            ui.label(egui::RichText::new(cfg.wifi_mode.describe()).weak().small());
            changed
        }
        Field::Channel => {
            let opts = channels_for(chip);
            let mut c = cfg.channel;
            let r = egui::ComboBox::from_id_salt("channel")
                .selected_text(channel_label(c))
                .width(140.0)
                .show_ui(ui, |ui| {
                    for ch in opts {
                        ui.selectable_value(&mut c, ch, channel_label(ch));
                    }
                });
            let _ = r;
            let changed = c != cfg.channel;
            cfg.channel = c;
            changed
        }
        Field::Protocol => {
            let opts: Vec<WifiProtocol> = WifiProtocol::ALL
                .into_iter()
                .filter(|p| p.supported_on(chip))
                .collect();
            combo(ui, "protocol", &mut cfg.protocol, &opts, |p| p.cli())
        }
        Field::He20 => toggle(ui, &mut cfg.he20),
        Field::PeerMac => mac_edit(ui, &mut cfg.peer_mac, "blank = broadcast / auto-pair"),
        Field::Ht40 => combo(ui, "ht40", &mut cfg.ht40, &Ht40::ALL, |h| h.cli()),
        Field::StaSsid => text(ui, &mut cfg.sta_ssid, false),
        Field::StaPassword => text(ui, &mut cfg.sta_password, true),
        Field::ApSsid => text(ui, &mut cfg.ap_ssid, false),
        Field::ApPassword => text(ui, &mut cfg.ap_password, true),
        Field::ApDhcp => toggle(ui, &mut cfg.ap_dhcp),
        Field::ApLeases => drag(ui, &mut cfg.ap_leases, 1..=8, ""),
        Field::ApBurst => toggle(ui, &mut cfg.ap_burst),
        Field::InjectPeriod => drag(ui, &mut cfg.inject_period_us, 100..=10_000_000, " µs"),
        Field::EmitterIface => combo(
            ui,
            "iface",
            &mut cfg.emitter_iface,
            &EmitterIface::ALL,
            |i| i.cli(),
        ),
        Field::Reporting => {
            let opts: Vec<Reporting> = Reporting::ALL
                .into_iter()
                .filter(|r| r.allowed_for(cfg.wifi_mode))
                .collect();
            combo(ui, "reporting", &mut cfg.reporting, &opts, |r| r.cli())
        }
        Field::ReportTo => {
            let c = combo(ui, "report_to", &mut cfg.report_to, &ReportTo::ALL, |r| {
                r.cli()
            });
            if cfg.report_to == ReportTo::Local {
                ui.end_row();
                ui.label("");
                ui.colored_label(
                    egui::Color32::from_rgb(220, 180, 60),
                    egui::RichText::new("local: nothing reaches this GUI").small(),
                );
            }
            c
        }
        Field::ThresholdLevel => drag(ui, &mut cfg.threshold_level, 0..=u16::MAX, ""),
        Field::ThresholdHold => drag(ui, &mut cfg.threshold_hold_ms, 0..=600_000, " ms"),
        Field::DecimateN => drag(ui, &mut cfg.decimate_n, 1..=10_000, ""),
        Field::Periodicity => drag(ui, &mut cfg.periodicity_hz, 0..=10_000, " Hz"),
        Field::Unsolicited => toggle(ui, &mut cfg.unsolicited),
        Field::LogMode => combo(ui, "log_mode", &mut cfg.log_mode, &LogMode::ALL, |m| {
            m.cli()
        }),
        Field::CsiOutput => toggle(ui, &mut cfg.csi_output),
        Field::FilterPeer => mac_edit(ui, &mut cfg.csi_filter_peer, "blank = any"),
        Field::FilterMinPhy => combo(
            ui,
            "min_phy",
            &mut cfg.csi_filter_min_phy,
            &MinPhy::ALL,
            |m| m.cli(),
        ),
        Field::Delivery => combo(
            ui,
            "delivery",
            &mut cfg.csi_delivery,
            &CsiDelivery::ALL,
            |d| d.cli(),
        ),
        Field::CsiLogging => toggle(ui, &mut cfg.csi_logging),
        Field::TxTask => toggle(ui, &mut cfg.tx_task),
        Field::RxTask => toggle(ui, &mut cfg.rx_task),
        Field::PhyRate => combo(ui, "phy_rate", &mut cfg.phy_rate, &PhyRate::ALL, |r| {
            r.cli()
        }),
        Field::Preset => {
            let mut changed = false;
            let current = cfg.csi_preset.clone().unwrap_or_else(|| "custom".into());
            egui::ComboBox::from_id_salt("preset")
                .selected_text(&current)
                .width(140.0)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(cfg.csi_preset.is_none(), "custom")
                        .clicked()
                    {
                        cfg.csi_preset = None;
                        changed = true;
                    }
                    for name in csi_preset_names() {
                        if ui.selectable_label(current == name, name).clicked() {
                            cfg.csi_preset = Some(name.to_string());
                            apply_csi_preset(name, cfg);
                            changed = true;
                        }
                    }
                });
            changed
        }
        Field::ClassicLltf => toggle(ui, &mut cfg.classic_lltf_en),
        Field::ClassicHtltf => toggle(ui, &mut cfg.classic_htltf_en),
        Field::ClassicStbcHtltf => toggle(ui, &mut cfg.classic_stbc_htltf_en),
        Field::ClassicLtfMerge => toggle(ui, &mut cfg.classic_ltf_merge_en),
        Field::C5c6Csi => toggle(ui, &mut cfg.c5c6_csi_en),
        Field::C5c6Legacy => toggle(ui, &mut cfg.c5c6_csi_legacy_en),
        Field::C5c6Ht20 => toggle(ui, &mut cfg.c5c6_csi_ht20_en),
        Field::C5c6Ht40 => toggle(ui, &mut cfg.c5c6_csi_ht40_en),
        Field::C5c6Su => toggle(ui, &mut cfg.c5c6_csi_su),
        Field::C5c6Mu => toggle(ui, &mut cfg.c5c6_csi_mu),
        Field::C5c6Dcm => toggle(ui, &mut cfg.c5c6_csi_dcm),
        Field::C5c6Beamformed => toggle(ui, &mut cfg.c5c6_csi_beamformed),
        Field::C5c6HeStbc => toggle(ui, &mut cfg.c5c6_csi_he_stbc),
        Field::C5c6ValScale => drag(ui, &mut cfg.c5c6_val_scale_cfg, 0..=3, ""),
        Field::DumpAck => toggle(ui, &mut cfg.csi_dump_ack),
        Field::C5ForceLltf => toggle(ui, &mut cfg.c5_csi_force_lltf),
        Field::C5Vht => toggle(ui, &mut cfg.c5_csi_vht),
        Field::Duration => {
            let mut limited = cfg.start_duration_sec.is_some();
            let mut changed = false;
            ui.horizontal(|ui| {
                if ui.checkbox(&mut limited, "limit").changed() {
                    cfg.start_duration_sec = limited.then_some(60);
                    changed = true;
                }
                if let Some(d) = cfg.start_duration_sec.as_mut() {
                    changed |= ui
                        .add(egui::DragValue::new(d).range(1..=86_400).suffix(" s"))
                        .changed();
                }
            });
            changed
        }
    }
}

fn channel_label(c: u8) -> String {
    if c > 14 {
        format!("{c} (5 GHz)")
    } else {
        format!("{c} (2.4 GHz)")
    }
}

fn combo<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    id: &str,
    value: &mut T,
    options: &[T],
    label: impl Fn(T) -> &'static str,
) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(label(*value))
        .width(140.0)
        .show_ui(ui, |ui| {
            for &v in options {
                if ui.selectable_label(*value == v, label(v)).clicked() && *value != v {
                    *value = v;
                    changed = true;
                }
            }
        });
    changed
}

fn toggle(ui: &mut egui::Ui, b: &mut bool) -> bool {
    ui.checkbox(b, if *b { "on" } else { "off" }).changed()
}

fn drag<T: egui::emath::Numeric>(
    ui: &mut egui::Ui,
    v: &mut T,
    range: std::ops::RangeInclusive<T>,
    suffix: &str,
) -> bool {
    ui.add(egui::DragValue::new(v).range(range).suffix(suffix))
        .changed()
}

fn text(ui: &mut egui::Ui, s: &mut String, password: bool) -> bool {
    let mut buf = s.clone();
    let r = ui.add(
        egui::TextEdit::singleline(&mut buf)
            .password(password)
            .char_limit(32)
            .desired_width(160.0),
    );
    if r.changed() {
        *s = buf;
        true
    } else {
        false
    }
}

fn mac_edit(ui: &mut egui::Ui, s: &mut String, hint: &str) -> bool {
    let ok = crate::protocol::config::valid_peer_mac(s.trim());
    let r = ui.add(
        egui::TextEdit::singleline(s)
            .hint_text(hint)
            .char_limit(17)
            .desired_width(160.0)
            .text_color_opt((!ok).then_some(egui::Color32::from_rgb(230, 90, 90))),
    );
    r.changed()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::config::Reporting as Rep;

    fn shown(cfg: &EspConfig, chip: ChipFamily) -> Vec<Field> {
        Field::ALL
            .into_iter()
            .filter(|f| f.visible(cfg, chip))
            .collect()
    }

    #[test]
    fn chip_decides_acquisition_rows() {
        let cfg = EspConfig::default();
        let s3 = shown(&cfg, ChipFamily::Esp32S3);
        assert!(s3.contains(&Field::ClassicLltf));
        assert!(!s3.contains(&Field::C5c6Su));
        assert!(!s3.contains(&Field::Preset));
        let c6 = shown(&cfg, ChipFamily::Esp32C6);
        assert!(c6.contains(&Field::C5c6Su));
        assert!(!c6.contains(&Field::ClassicLltf));
        assert!(!c6.contains(&Field::C5Vht), "VHT is C5-only");
        assert!(shown(&cfg, ChipFamily::Esp32C5).contains(&Field::C5Vht));
    }

    #[test]
    fn mode_decides_node_rows() {
        let mut cfg = EspConfig {
            wifi_mode: WifiMode::Station,
            ..EspConfig::default()
        };
        let st = shown(&cfg, ChipFamily::Esp32);
        assert!(st.contains(&Field::StaSsid) && !st.contains(&Field::Channel));
        assert!(!st.contains(&Field::ApLeases));

        cfg.wifi_mode = WifiMode::Ht20Emitter;
        let em = shown(&cfg, ChipFamily::Esp32C5);
        assert!(em.contains(&Field::InjectPeriod) && em.contains(&Field::He20));
        assert!(!em.contains(&Field::Reporting), "emitters report none");
        assert!(!em.contains(&Field::C5c6Su), "emitters capture nothing");
        assert!(!shown(&cfg, ChipFamily::Esp32).contains(&Field::He20));
    }

    #[test]
    fn reporting_parameters_follow_the_policy() {
        let mut cfg = EspConfig::default();
        assert!(!shown(&cfg, ChipFamily::Esp32).contains(&Field::ThresholdLevel));
        cfg.reporting = Rep::Threshold;
        let f = shown(&cfg, ChipFamily::Esp32);
        assert!(f.contains(&Field::ThresholdLevel) && f.contains(&Field::ThresholdHold));
        cfg.reporting = Rep::Decimate;
        assert!(shown(&cfg, ChipFamily::Esp32).contains(&Field::DecimateN));
    }

    #[test]
    fn preset_hides_individual_c5c6_flags() {
        let cfg = EspConfig {
            csi_preset: Some("he20".into()),
            ..EspConfig::default()
        };
        let f = shown(&cfg, ChipFamily::Esp32C5);
        assert!(f.contains(&Field::Preset));
        assert!(!f.contains(&Field::C5c6Legacy));
    }

    #[test]
    fn five_ghz_channels_only_on_c5() {
        assert!(channels_for(ChipFamily::Esp32C5).contains(&149));
        assert!(!channels_for(ChipFamily::Esp32S3).contains(&149));
    }

    #[test]
    fn every_field_has_a_section_and_label() {
        for f in Field::ALL {
            assert!(!f.label().is_empty());
            let _ = f.section();
        }
    }
}
