//! Strongly-typed counterparts to the `set-*` family of CLI commands.
//!
//! The types here mirror the firmware's `UserConfig` surface and
//! cycle/serialize/render-as-CLI on demand. They are the GUI's
//! source-of-truth for what gets pushed to the device with `ApplyConfig`.
//!
//! Grammar matches esp-csi-cli-rs 0.10.x (CLI `protocol=4`, the IEEE
//! 802.11bf vocabulary): `set-wifi` with the nine modes and their extras
//! (HE20, AP leases / burst, emitter period and interface), `set-reporting`
//! with the threshold / decimate policies and `--report-to`,
//! `set-measurement`, `set-session`, `set-csi-filter`, `set-protocol`
//! (including `ax` on the ESP32-C5/C6), the classic and C5/C6 `set-csi`
//! acquisition flags with the built-in presets, `set-csi-delivery` and
//! `set-csi-output`.

use crate::protocol::info::ChipFamily;
use serde::{Deserialize, Serialize};

/// The node's **mode** — how it reaches the channel.
///
/// The other node attributes (session role, sensing role, reporting) are carried by the mode
/// rather than selected here, because which values a mode admits depends on the mode. The model
/// is documented once, in
/// [`esp-csi-rs/docs/network-model.md`](https://github.com/csi-rs/esp-csi-rs/blob/main/docs/network-model.md).
///
/// Where a mode admits a choice of reporting, [`EspConfig::reporting`] picks it; see
/// [`WifiMode::admits_reporting_choice`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WifiMode {
    /// Wi-Fi station: associate to an AP using the configured SSID/password. Reports or not,
    /// by choice.
    Station,
    /// Wi-Fi sniffer: promiscuous capture without association. Always reports.
    Sniffer,
    /// Wi-Fi access point: self-contained softAP (DHCP + ICMP flood). Session initiator;
    /// reports or not, by choice.
    WifiAp,
    /// Emitter: raw-inject HT20 sounding frames. A transmitter that measures and reports nothing.
    Ht20Emitter,
    /// Emitter: raw-inject HT40 sounding frames. A transmitter that measures and reports nothing.
    Ht40Emitter,
    /// ESP-NOW, initiator end: starts the measurement exchange. Reports or not, by choice.
    EspNowInitiator,
    /// ESP-NOW, responder end: answers it. Reports or not, by choice.
    EspNowResponder,
    /// ESP-NOW simplex, flooding end. Initiator and transmitter; it reports no CSI of its own.
    EspNowSimplexSource,
    /// ESP-NOW simplex, measuring end. Responder and receiver that reports; the highest CSI
    /// rate of any pairing.
    EspNowSimplexPeer,
}

impl WifiMode {
    pub const ALL: [WifiMode; 9] = [
        WifiMode::Station,
        WifiMode::Sniffer,
        WifiMode::WifiAp,
        WifiMode::Ht20Emitter,
        WifiMode::Ht40Emitter,
        WifiMode::EspNowInitiator,
        WifiMode::EspNowResponder,
        WifiMode::EspNowSimplexSource,
        WifiMode::EspNowSimplexPeer,
    ];

    /// CLI token used by `set-wifi --mode=…`.
    pub fn cli(self) -> &'static str {
        match self {
            Self::Station => "station",
            Self::Sniffer => "sniffer",
            Self::WifiAp => "wifi-ap",
            Self::Ht20Emitter => "ht20-emitter",
            Self::Ht40Emitter => "ht40-emitter",
            Self::EspNowInitiator => "esp-now-initiator",
            Self::EspNowResponder => "esp-now-responder",
            Self::EspNowSimplexSource => "esp-now-simplex-source",
            Self::EspNowSimplexPeer => "esp-now-simplex-peer",
        }
    }
    /// `true` for the emitter modes. They inject frames and capture nothing, so they take a
    /// destination MAC and ignore the capture-side knobs.
    pub fn is_emitter(self) -> bool {
        matches!(self, Self::Ht20Emitter | Self::Ht40Emitter)
    }

    /// `true` for the modes that report no CSI of their own — the emitters and the simplex source.
    /// All three have reporting fixed at `none`: the emitters measure nothing, and the simplex
    /// source measures but never reports, so there is no capture-side configuration to show.
    pub fn reports_no_csi(self) -> bool {
        self.is_emitter() || matches!(self, Self::EspNowSimplexSource)
    }

    /// `true` for the four ESP-NOW modes (the pair and both simplex ends).
    pub fn is_esp_now(self) -> bool {
        matches!(
            self,
            Self::EspNowInitiator
                | Self::EspNowResponder
                | Self::EspNowSimplexSource
                | Self::EspNowSimplexPeer
        )
    }

    /// `true` for the modes whose reporting is a choice rather than fixed by the mode:
    /// station, access point and the ESP-NOW pair. Only these send `set-wifi --reporting`;
    /// the sniffer always reports, the emitters never do, and each simplex end is fixed by
    /// which end it is.
    pub fn admits_reporting_choice(self) -> bool {
        matches!(
            self,
            Self::Station
                | Self::Sniffer
                | Self::WifiAp
                | Self::EspNowInitiator
                | Self::EspNowResponder
        )
    }

    /// `true` for the modes that send `--he20`: the emitters and the ESP-NOW
    /// pair (ESP32-C5/C6 only).
    pub fn uses_he20(self) -> bool {
        matches!(
            self,
            Self::Ht20Emitter | Self::Ht40Emitter | Self::EspNowInitiator | Self::EspNowResponder
        )
    }

    /// `true` for the modes that generate sounding traffic and so read
    /// `set-measurement --periodicity-hz`. The sniffer and the simplex peer
    /// sound nothing; the emitters pace by their inject period instead.
    pub fn uses_periodicity(self) -> bool {
        matches!(
            self,
            Self::Station
                | Self::WifiAp
                | Self::EspNowInitiator
                | Self::EspNowResponder
                | Self::EspNowSimplexSource
        )
    }

    /// Short human description, for tooltips.
    pub fn describe(self) -> &'static str {
        match self {
            Self::Station => "Joins an AP and captures CSI from its traffic (pings the gateway).",
            Self::Sniffer => "Passive monitor on one channel; captures CSI from every frame heard.",
            Self::WifiAp => "Runs a softAP and floods its stations; captures their replies.",
            Self::Ht20Emitter => "Injects HT20 frames; captures nothing. Pair with sniffers.",
            Self::Ht40Emitter => "Injects HT40 frames; captures nothing. Pair with sniffers.",
            Self::EspNowInitiator => "ESP-NOW pair, the end that sources the soundings.",
            Self::EspNowResponder => "ESP-NOW pair, the end that answers the initiator.",
            Self::EspNowSimplexSource => "One-way ESP-NOW flood; captures nothing.",
            Self::EspNowSimplexPeer => "Receives a simplex source's flood and measures it.",
        }
    }

    /// `true` for the modes that read `set-wifi --peer-mac`: the emitters (injection
    /// destination) and every ESP-NOW mode (explicit peer, set on both nodes).
    pub fn uses_peer_mac(self) -> bool {
        self.is_emitter() || self.is_esp_now()
    }

    /// `true` for the modes that read `set-wifi --ht40`: the access point (softAP secondary
    /// channel) and the ESP-NOW modes that transmit to a peer (forced per-peer TX PHY).
    pub fn uses_ht40(self) -> bool {
        matches!(
            self,
            Self::WifiAp
                | Self::EspNowInitiator
                | Self::EspNowResponder
                | Self::EspNowSimplexSource
        )
    }

    /// `true` for the modes that apply `set-rate`: the ESP-NOW pair. Everywhere else the rate is
    /// reporting only.
    pub fn applies_phy_rate(self) -> bool {
        matches!(self, Self::EspNowInitiator | Self::EspNowResponder)
    }
    /// Round-robin to the next mode (used by the config panel's
    /// click-to-cycle button). Every variant must appear exactly once or
    /// cycling silently skips or loops modes.
    pub fn cycle(self) -> Self {
        match self {
            Self::Sniffer => Self::Station,
            Self::Station => Self::WifiAp,
            Self::WifiAp => Self::Ht20Emitter,
            Self::Ht20Emitter => Self::Ht40Emitter,
            Self::Ht40Emitter => Self::EspNowInitiator,
            Self::EspNowInitiator => Self::EspNowResponder,
            Self::EspNowResponder => Self::EspNowSimplexSource,
            Self::EspNowSimplexSource => Self::EspNowSimplexPeer,
            Self::EspNowSimplexPeer => Self::Sniffer,
        }
    }
}

/// Wi-Fi PHY protocol (`set-protocol --protocol=…`). Firmware default is `lr`.
///
/// Deserialization is lenient: a name this build does not know (e.g. one a
/// saved config from an older release carries) falls back to the default,
/// [`WifiProtocol::Lr`], instead of failing the whole config.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Default)]
#[allow(missing_docs)] // each variant maps 1:1 to a CLI token
pub enum WifiProtocol {
    B,
    G,
    N,
    #[default]
    Lr,
    A,
    Ac,
    /// 802.11ax. ESP32-C5/C6 only; esp-csi-rs then runs the HE20 bring-up
    /// for `station`, `wifi-ap` and `sniffer`.
    Ax,
}

impl<'de> Deserialize<'de> for WifiProtocol {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let name = String::deserialize(d)?;
        Ok(Self::ALL
            .into_iter()
            .find(|p| name.eq_ignore_ascii_case(p.cli()))
            .unwrap_or_default())
    }
}

impl WifiProtocol {
    pub const ALL: [WifiProtocol; 7] = [
        WifiProtocol::B,
        WifiProtocol::G,
        WifiProtocol::N,
        WifiProtocol::Lr,
        WifiProtocol::A,
        WifiProtocol::Ac,
        WifiProtocol::Ax,
    ];

    /// `true` when the firmware accepts this protocol on `chip`. `ax` needs
    /// an ESP32-C5/C6; an unidentified chip is given the benefit of the doubt.
    pub fn supported_on(self, chip: ChipFamily) -> bool {
        self != Self::Ax || chip.is_c5c6() || chip == ChipFamily::Unknown
    }

    /// CLI token used by `set-protocol --protocol=…`.
    pub fn cli(self) -> &'static str {
        match self {
            Self::B => "b",
            Self::G => "g",
            Self::N => "n",
            Self::Lr => "lr",
            Self::A => "a",
            Self::Ac => "ac",
            Self::Ax => "ax",
        }
    }
    /// Round-robin to the next protocol.
    pub fn cycle(self) -> Self {
        match self {
            Self::B => Self::G,
            Self::G => Self::N,
            Self::N => Self::Lr,
            Self::Lr => Self::A,
            Self::A => Self::Ac,
            Self::Ac => Self::Ax,
            Self::Ax => Self::B,
        }
    }
}

/// HT40 secondary channel (`set-wifi --ht40=…`). In `wifi-ap` it runs the
/// softAP as HT40; in the ESP-NOW modes it forces the per-peer TX PHY to
/// HT40. `None` keeps HT20. See [`WifiMode::uses_ht40`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Ht40 {
    /// HT20 / legacy (the `none` token).
    #[default]
    None,
    /// Secondary channel above the primary.
    Above,
    /// Secondary channel below the primary.
    Below,
}

impl Ht40 {
    pub const ALL: [Ht40; 3] = [Ht40::None, Ht40::Above, Ht40::Below];

    /// CLI token used by `set-wifi --ht40=…`.
    pub fn cli(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Above => "above",
            Self::Below => "below",
        }
    }
    /// Round-robin to the next option.
    pub fn cycle(self) -> Self {
        match self {
            Self::None => Self::Above,
            Self::Above => Self::Below,
            Self::Below => Self::None,
        }
    }
}

/// The node's **reporting** (`set-wifi --reporting=…` /
/// `set-reporting --policy=…`): whether, and how often, its measurements
/// leave it.
///
/// Only read by the modes that admit a choice
/// ([`WifiMode::admits_reporting_choice`]); emitters always report none and
/// each ESP-NOW simplex end is fixed by which end it is. A sniffer takes
/// every value except [`Reporting::None`] ([`Reporting::allowed_for`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Reporting {
    /// Report every measurement (the firmware default).
    #[default]
    Report,
    /// Measure, but report nothing.
    None,
    /// IEEE 802.11bf threshold-based reporting: report only while the
    /// channel moves, i.e. while the measurement score is at or above
    /// [`EspConfig::threshold_level`], and for
    /// [`EspConfig::threshold_hold_ms`] after.
    Threshold,
    /// Report every [`EspConfig::decimate_n`]-th measurement.
    Decimate,
}

impl Reporting {
    /// Every value, in menu order.
    pub const ALL: [Reporting; 4] = [
        Reporting::Report,
        Reporting::None,
        Reporting::Threshold,
        Reporting::Decimate,
    ];

    /// CLI token used by `--reporting=…` and `--policy=…`.
    pub fn cli(self) -> &'static str {
        match self {
            Self::Report => "report",
            Self::None => "none",
            Self::Threshold => "threshold",
            Self::Decimate => "decimate",
        }
    }

    /// `false` for the one combination the firmware refuses: a sniffer that
    /// reports nothing observes nothing.
    pub fn allowed_for(self, mode: WifiMode) -> bool {
        !(mode == WifiMode::Sniffer && self == Self::None)
    }
}

/// Where `report` sends measurements (`--report-to=…`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ReportTo {
    /// The serial output, where this GUI reads them (the default).
    #[default]
    Host,
    /// Only on-device consumers (callback / queue). Nothing reaches the
    /// GUI, so plots stay empty.
    Local,
}

impl ReportTo {
    /// Every value, in menu order.
    pub const ALL: [ReportTo; 2] = [ReportTo::Host, ReportTo::Local];

    /// CLI token.
    pub fn cli(self) -> &'static str {
        match self {
            Self::Host => "host",
            Self::Local => "local",
        }
    }
}

/// Which interface an emitter injects from (`--emitter-iface=…`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum EmitterIface {
    /// The station interface (the default).
    #[default]
    Sta,
    /// The softAP interface.
    Ap,
}

impl EmitterIface {
    /// Every value, in menu order.
    pub const ALL: [EmitterIface; 2] = [EmitterIface::Sta, EmitterIface::Ap];

    /// CLI token.
    pub fn cli(self) -> &'static str {
        match self {
            Self::Sta => "sta",
            Self::Ap => "ap",
        }
    }
}

/// Minimum PHY class a frame needs for its CSI to be delivered
/// (`set-csi-filter --min-phy=…`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MinPhy {
    /// Every PHY (the default).
    #[default]
    Any,
    /// 802.11n and better; drops legacy 11b/g frames.
    Ht,
}

impl MinPhy {
    /// Every value, in menu order.
    pub const ALL: [MinPhy; 2] = [MinPhy::Any, MinPhy::Ht];

    /// CLI token.
    pub fn cli(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::Ht => "ht",
        }
    }
}

/// Wire format the firmware emits CSI packets in. All four are
/// decoded by the host; see [`crate::protocol::parse`] and
/// [`crate::protocol::serialized`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogMode {
    /// COBS-framed `postcard` binary — preferred default; smallest on
    /// the wire and structured.
    Serialized,
    /// Human-readable `key: value` lines per packet.
    Text,
    /// One-line nested-array per packet (CSV-ish).
    ArrayList,
    /// Hernandez ESP32-CSI-Tool's 26-column CSV.
    EspCsiTool,
}

impl LogMode {
    pub const ALL: [LogMode; 4] = [
        LogMode::Serialized,
        LogMode::Text,
        LogMode::ArrayList,
        LogMode::EspCsiTool,
    ];

    /// CLI token used by `set-log-mode --mode=…`.
    pub fn cli(self) -> &'static str {
        match self {
            Self::Serialized => "serialized",
            Self::Text => "text",
            Self::ArrayList => "array-list",
            Self::EspCsiTool => "esp-csi-tool",
        }
    }
    /// Round-robin to the next mode.
    pub fn cycle(self) -> Self {
        match self {
            Self::Serialized => Self::Text,
            Self::Text => Self::ArrayList,
            Self::ArrayList => Self::EspCsiTool,
            Self::EspCsiTool => Self::Serialized,
        }
    }
    /// Internal `AtomicU8` tag the reader thread uses to pick a parser.
    /// Not on the wire — used only between the UI and reader.
    pub fn from_id(id: u8) -> Self {
        match id {
            0 => Self::Serialized,
            2 => Self::ArrayList,
            3 => Self::EspCsiTool,
            _ => Self::Text,
        }
    }

    /// Internal tag the reader keys its parser on; inverse of [`Self::from_id`].
    pub fn id(self) -> u8 {
        match self {
            Self::Serialized => 0,
            Self::Text => 1,
            Self::ArrayList => 2,
            Self::EspCsiTool => 3,
        }
    }
    /// `true` when the host has a working decoder for this mode.
    /// All four are supported today; the method is kept so the
    /// status badge in the config panel can stay generic.
    pub fn host_parses(self) -> bool {
        matches!(
            self,
            Self::Serialized | Self::Text | Self::EspCsiTool | Self::ArrayList
        )
    }
}

/// Whether (and how) the firmware delivers CSI from the Wi-Fi callback
/// to its log path. `Off` suppresses CSI emission entirely.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CsiDelivery {
    /// CSI emission disabled.
    Off,
    /// Inline in the Wi-Fi callback (low latency, low DRAM).
    Callback,
    /// Async over a bounded channel + dedicated drain task.
    Async,
    /// Zero-copy fast-path. Stored as a device flag and applied on the
    /// **next `start`**; while active no CSI data is delivered or logged
    /// and the `q`-key stop peek is skipped (the run is duration-bound or
    /// reset-driven). Host views receive nothing in this mode.
    Raw,
}

impl CsiDelivery {
    pub const ALL: [CsiDelivery; 4] = [
        CsiDelivery::Off,
        CsiDelivery::Callback,
        CsiDelivery::Async,
        CsiDelivery::Raw,
    ];

    /// CLI token used by `set-csi-delivery --mode=…`.
    pub fn cli(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Callback => "callback",
            Self::Async => "async",
            Self::Raw => "raw",
        }
    }
    /// Round-robin to the next delivery mode.
    pub fn cycle(self) -> Self {
        match self {
            Self::Off => Self::Callback,
            Self::Callback => Self::Async,
            Self::Async => Self::Raw,
            Self::Raw => Self::Off,
        }
    }
}

/// PHY rate the firmware uses when transmitting (`set-rate --rate=…`).
/// Reporting only (stored and echoed by `show-config`), except on the ESP-NOW
/// pair (`esp-now-initiator` / `esp-now-responder`), which applies it. See
/// [`WifiMode::applies_phy_rate`].
///
/// `RxxL` variants are 802.11b long-preamble; `Mcs*Lgi` / `Mcs*Sgi` are
/// 802.11n MCS indices with long/short guard interval.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[allow(missing_docs)] // each variant maps 1:1 to a labelled CLI token
pub enum PhyRate {
    // The firmware treats `1m-l` / `5m5-l` / `11m-l` as the same rates as
    // `1m` / `5m5` / `11m`; configs saved with those variants load as these.
    #[serde(alias = "R1mL")]
    R1m,
    R2m,
    #[serde(alias = "R5m5L")]
    R5m5,
    #[serde(alias = "R11mL")]
    R11m,
    R6m,
    R9m,
    R12m,
    R18m,
    R24m,
    R36m,
    R48m,
    R54m,
    Mcs0Lgi,
    Mcs1Lgi,
    Mcs2Lgi,
    Mcs3Lgi,
    Mcs4Lgi,
    Mcs5Lgi,
    Mcs6Lgi,
    Mcs7Lgi,
    Mcs0Sgi,
}

impl PhyRate {
    /// Every variant in declaration order — used for round-robin cycling.
    pub const ALL: [PhyRate; 21] = [
        PhyRate::R1m,
        PhyRate::R2m,
        PhyRate::R5m5,
        PhyRate::R11m,
        PhyRate::R6m,
        PhyRate::R9m,
        PhyRate::R12m,
        PhyRate::R18m,
        PhyRate::R24m,
        PhyRate::R36m,
        PhyRate::R48m,
        PhyRate::R54m,
        PhyRate::Mcs0Lgi,
        PhyRate::Mcs1Lgi,
        PhyRate::Mcs2Lgi,
        PhyRate::Mcs3Lgi,
        PhyRate::Mcs4Lgi,
        PhyRate::Mcs5Lgi,
        PhyRate::Mcs6Lgi,
        PhyRate::Mcs7Lgi,
        PhyRate::Mcs0Sgi,
    ];

    /// CLI token used by `set-rate --rate=…`.
    pub fn cli(self) -> &'static str {
        match self {
            Self::R1m => "1m",
            Self::R2m => "2m",
            Self::R5m5 => "5m5",
            Self::R11m => "11m",
            Self::R6m => "6m",
            Self::R9m => "9m",
            Self::R12m => "12m",
            Self::R18m => "18m",
            Self::R24m => "24m",
            Self::R36m => "36m",
            Self::R48m => "48m",
            Self::R54m => "54m",
            Self::Mcs0Lgi => "mcs0-lgi",
            Self::Mcs1Lgi => "mcs1-lgi",
            Self::Mcs2Lgi => "mcs2-lgi",
            Self::Mcs3Lgi => "mcs3-lgi",
            Self::Mcs4Lgi => "mcs4-lgi",
            Self::Mcs5Lgi => "mcs5-lgi",
            Self::Mcs6Lgi => "mcs6-lgi",
            Self::Mcs7Lgi => "mcs7-lgi",
            Self::Mcs0Sgi => "mcs0-sgi",
        }
    }
    /// Round-robin to the next rate.
    pub fn cycle(self) -> Self {
        let i = Self::ALL.iter().position(|r| *r == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
}

/// Full configuration pushed to the device by `EspCommand::ApplyConfig`.
///
/// Each field maps 1:1 to a `set-*` CLI command — see
/// [`crate::protocol::command::render_apply_config`] for the exact
/// rendering. Field grouping mirrors the ESP config panel.
///
/// The CSI feature flags are **enable-sense** (`true` = feature on),
/// matching the firmware's positive `--lltf=on|off` grammar. All default
/// to `true` (the firmware default is "all CSI features on").
// `serde(default)`: configs saved by an older build (devices.json) load with
// the defaults for any field they predate.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EspConfig {
    /// Mode (`set-wifi --mode=…`).
    pub wifi_mode: WifiMode,
    /// Reporting (`set-wifi --reporting=…`). Sent only for the modes
    /// that admit a choice; see [`WifiMode::admits_reporting_choice`].
    #[serde(default)]
    pub reporting: Reporting,
    /// Where `report` sends measurements (`--report-to`).
    pub report_to: ReportTo,
    /// Threshold score for [`Reporting::Threshold`] (`set-reporting --level`,
    /// 0–65535, firmware default 6000, uncalibrated).
    pub threshold_level: u16,
    /// How long to keep reporting after crossing the threshold, in ms
    /// (`set-reporting --hold-ms`).
    pub threshold_hold_ms: u32,
    /// Decimation factor for [`Reporting::Decimate`] (`set-reporting --n`).
    pub decimate_n: u32,
    /// Force HE20 / 802.11ax SU on the emitters and the ESP-NOW pair
    /// (`set-wifi --he20`, ESP32-C5/C6 only).
    pub he20: bool,
    /// Wi-Fi PHY protocol (`set-protocol --protocol=…`).
    pub protocol: WifiProtocol,
    /// Primary channel (`set-wifi --set-channel=…`). Must satisfy
    /// [`valid_channel`].
    pub channel: u8,
    /// Station-mode SSID (`set-wifi --sta-ssid=…`). Max 32 bytes.
    pub sta_ssid: String,
    /// Station-mode PSK (`set-wifi --sta-password=…`). Max 32 bytes.
    pub sta_password: String,
    /// SoftAP SSID (`set-wifi --ap-ssid=…`). Max 32 bytes.
    pub ap_ssid: String,
    /// SoftAP password (`set-wifi --ap-password=…`, empty = open). Max 32 bytes.
    pub ap_password: String,
    /// Serve DHCP in AP mode (`set-wifi --ap-dhcp=on|off`).
    pub ap_dhcp: bool,
    /// DHCP lease pool size in `wifi-ap` (`--ap-leases`, 1–8).
    pub ap_leases: u8,
    /// Synchronized burst flood to every station in `wifi-ap` (`--ap-burst`).
    pub ap_burst: bool,
    /// Emitter inject period in µs (`--inject-period-us`; 20000 ≈ 50 frames/s).
    pub inject_period_us: u32,
    /// Which interface an emitter injects from (`--emitter-iface`).
    pub emitter_iface: EmitterIface,
    /// Peer MAC (`set-wifi --peer-mac=…`). In the emitter modes it is the
    /// destination for injected frames (empty = broadcast); in the ESP-NOW
    /// modes it is the explicit peer (empty = automatic pairing), and must be
    /// set on both nodes. See [`WifiMode::uses_peer_mac`].
    #[serde(default)]
    pub peer_mac: String,
    /// HT40 secondary channel (`set-wifi --ht40=…`): the softAP's in
    /// `wifi-ap`, the forced per-peer TX PHY in the ESP-NOW modes. See
    /// [`WifiMode::uses_ht40`].
    #[serde(default)]
    pub ht40: Ht40,
    /// Measurement periodicity in Hz (`set-measurement --periodicity-hz=…`).
    pub periodicity_hz: u64,
    /// One-directional unsolicited ICMP-reply flood (`set-measurement --unsolicited=…`).
    pub unsolicited: bool,
    /// Runtime CSI delivery gate (`set-csi-output --enabled=…`). With it off
    /// the radio still captures — nothing is decoded or logged — so the RX
    /// path keeps its timing. This is not the reporting setting; see
    /// [`EspConfig::reporting`]. No effect on an emitter, which captures nothing.
    pub csi_output: bool,
    /// Deliver CSI only for frames from this transmitter
    /// (`set-csi-filter --peer-mac`). Empty = any.
    pub csi_filter_peer: String,
    /// Minimum PHY class for delivered CSI (`set-csi-filter --min-phy`).
    pub csi_filter_min_phy: MinPhy,
    /// Wire format for emitted CSI packets (`set-log-mode`).
    pub log_mode: LogMode,
    /// PHY rate (`set-rate`). Applied only by the ESP-NOW pair; reporting only
    /// elsewhere.
    pub phy_rate: PhyRate,
    /// Spawn the TX task on `start` (`set-io-tasks --tx=…`).
    pub tx_task: bool,
    /// Spawn the RX task on `start` (`set-io-tasks --rx=…`).
    pub rx_task: bool,
    /// How CSI is delivered to the log path (`set-csi-delivery --mode=…`).
    pub csi_delivery: CsiDelivery,
    /// Master switch for CSI logging (`set-csi-delivery --logging=…`).
    pub csi_logging: bool,

    /// Classic chips: capture Legacy-LTF (`set-csi --lltf=on|off`).
    pub classic_lltf_en: bool,
    /// Classic chips: capture HT-LTF (`--htltf=on|off`).
    pub classic_htltf_en: bool,
    /// Classic chips: capture STBC-HT-LTF (`--stbc-htltf=on|off`).
    pub classic_stbc_htltf_en: bool,
    /// Classic chips: LTF merging (`--ltf-merge=on|off`).
    pub classic_ltf_merge_en: bool,

    /// ESP32-C5/C6: master CSI acquisition switch (`set-csi --csi=on|off`).
    pub c5c6_csi_en: bool,
    /// ESP32-C5/C6: L-LTF on 11g PPDU (`--csi-legacy=on|off`).
    pub c5c6_csi_legacy_en: bool,
    /// ESP32-C5/C6: HT-LTF on HT20 PPDU (`--csi-ht20=on|off`).
    pub c5c6_csi_ht20_en: bool,
    /// ESP32-C5/C6: HT-LTF on HT40 PPDU (`--csi-ht40=on|off`).
    pub c5c6_csi_ht40_en: bool,
    /// ESP32-C5/C6: value-scale config 0..=3 (`--val-scale-cfg=…`).
    pub c5c6_val_scale_cfg: u32,
    /// Named acquisition preset (`--preset=…`). `None` emits the individual
    /// flags instead. Names are resolved by the installed
    /// [`CsiProfile`](crate::protocol::profile::CsiProfile) — this crate names
    /// none of them itself.
    #[serde(default)]
    pub csi_preset: Option<String>,
    /// ESP32-C5/C6: dump 802.11 ACK frames (`--dump-ack=on|off`).
    pub csi_dump_ack: bool,
    /// ESP32-C5 only: force L-LTF acquisition (`--csi-force-lltf=on|off`).
    pub c5_csi_force_lltf: bool,
    /// ESP32-C5 only: VHT-LTF on VHT20 PPDU (`--csi-vht=on|off`).
    pub c5_csi_vht: bool,
    /// C5/C6: HE-LTF on HE SU PPDUs (`--csi-su`).
    pub c5c6_csi_su: bool,
    /// C5/C6: HE-LTF on HE MU PPDUs (`--csi-mu`).
    pub c5c6_csi_mu: bool,
    /// C5/C6: HE-LTF on DCM PPDUs (`--csi-dcm`).
    pub c5c6_csi_dcm: bool,
    /// C5/C6: HE-LTF on beamformed PPDUs (`--csi-beamformed`).
    pub c5c6_csi_beamformed: bool,
    /// C5/C6: HE-LTF on HE STBC PPDUs (`--csi-he-stbc`).
    pub c5c6_csi_he_stbc: bool,

    /// Auto-stop after this many seconds (`start --duration=N`). `None`
    /// means "run until user sends `q`".
    pub start_duration_sec: Option<u64>,
}

impl Default for EspConfig {
    fn default() -> Self {
        Self {
            wifi_mode: WifiMode::Sniffer,
            reporting: Reporting::Report,
            report_to: ReportTo::Host,
            threshold_level: 6000,
            threshold_hold_ms: 1000,
            decimate_n: 2,
            he20: false,
            protocol: WifiProtocol::Lr,
            channel: 6,
            sta_ssid: String::new(),
            sta_password: String::new(),
            ap_ssid: "esp-csi-ap".to_string(),
            ap_password: String::new(),
            ap_dhcp: true,
            ap_leases: 4,
            ap_burst: false,
            inject_period_us: 20_000,
            emitter_iface: EmitterIface::Sta,
            peer_mac: String::new(),
            ht40: Ht40::None,
            periodicity_hz: 100,
            unsolicited: false,
            csi_output: true,
            csi_filter_peer: String::new(),
            csi_filter_min_phy: MinPhy::Any,
            log_mode: LogMode::Serialized,
            phy_rate: PhyRate::Mcs0Lgi,
            tx_task: true,
            rx_task: true,
            csi_delivery: CsiDelivery::Callback,
            csi_logging: true,

            classic_lltf_en: true,
            classic_htltf_en: true,
            classic_stbc_htltf_en: true,
            classic_ltf_merge_en: true,

            c5c6_csi_en: true,
            c5c6_csi_legacy_en: true,
            c5c6_csi_ht20_en: true,
            c5c6_csi_ht40_en: true,
            c5c6_val_scale_cfg: 2,
            csi_preset: None,
            csi_dump_ack: true,
            c5_csi_force_lltf: true,
            c5_csi_vht: true,
            c5c6_csi_su: true,
            c5c6_csi_mu: true,
            c5c6_csi_dcm: true,
            c5c6_csi_beamformed: true,
            c5c6_csi_he_stbc: false,

            start_duration_sec: None,
        }
    }
}

impl EspConfig {
    /// Validate string-field lengths / formats before sending. SSIDs/passwords
    /// > 32 bytes would panic the device (`heapless::String::push_str().unwrap()`).
    pub fn validate(&self) -> Result<(), String> {
        self.validate_for(ChipFamily::Unknown)
    }

    /// [`Self::validate`], plus the checks that depend on the chip: `ax`
    /// and HE20 need an ESP32-C5/C6, and 5 GHz channels need a chip with
    /// a 5 GHz radio (the C5).
    pub fn validate_for(&self, chip: ChipFamily) -> Result<(), String> {
        for (what, v) in [
            ("STA SSID", &self.sta_ssid),
            ("STA password", &self.sta_password),
            ("AP SSID", &self.ap_ssid),
            ("AP password", &self.ap_password),
        ] {
            if v.len() > 32 {
                return Err(format!("{what} exceeds 32 bytes"));
            }
        }
        if self.wifi_mode != WifiMode::Station && !valid_channel(self.channel) {
            return Err(format!("invalid channel {}", self.channel));
        }
        if self.channel > 14 && chip != ChipFamily::Unknown && chip != ChipFamily::Esp32C5 {
            return Err(format!(
                "channel {} is 5 GHz; only the ESP32-C5 has a 5 GHz radio",
                self.channel
            ));
        }
        if !valid_peer_mac(self.peer_mac.trim()) {
            return Err(format!(
                "invalid peer MAC '{}' (use aa:bb:cc:dd:ee:ff, or blank)",
                self.peer_mac
            ));
        }
        if !valid_peer_mac(self.csi_filter_peer.trim()) {
            return Err(format!(
                "invalid CSI filter MAC '{}' (use aa:bb:cc:dd:ee:ff, or blank for any)",
                self.csi_filter_peer
            ));
        }
        if self.c5c6_val_scale_cfg > 3 {
            return Err("val_scale_cfg must be 0..=3".into());
        }
        if self.wifi_mode.admits_reporting_choice() && !self.reporting.allowed_for(self.wifi_mode) {
            return Err("a sniffer can't use reporting `none`: it would observe nothing".into());
        }
        if !(1..=8).contains(&self.ap_leases) {
            return Err("AP leases must be 1..=8".into());
        }
        if self.decimate_n == 0 {
            return Err("decimation factor must be at least 1".into());
        }
        if self.inject_period_us == 0 {
            return Err("inject period must be at least 1 µs".into());
        }
        if !self.protocol.supported_on(chip) {
            return Err(format!(
                "protocol `{}` needs an ESP32-C5/C6 (this is an {})",
                self.protocol.cli(),
                chip.label()
            ));
        }
        if self.he20 && !(chip.is_c5c6() || chip == ChipFamily::Unknown) {
            return Err(format!(
                "HE20 needs an ESP32-C5/C6 (this is an {})",
                chip.label()
            ));
        }
        Ok(())
    }
}

/// `true` for a blank MAC or `aa:bb:cc:dd:ee:ff` (`:` or `-` separated).
pub fn valid_peer_mac(mac: &str) -> bool {
    if mac.is_empty() {
        return true;
    }
    let sep = if mac.contains(':') {
        ':'
    } else if mac.contains('-') {
        '-'
    } else {
        return false;
    };
    let octets: Vec<&str> = mac.split(sep).collect();
    octets.len() == 6
        && octets
            .iter()
            .all(|o| o.len() == 2 && o.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// `true` when `c` is a 2.4 GHz (1..=14) or 5 GHz (UNII-1/2A/3) channel
/// number the firmware will accept. Used to gate the channel-picker in
/// the config panel and to fail `EspConfig::validate` early.
pub fn valid_channel(c: u8) -> bool {
    matches!(c, 1..=14 | 36 | 40 | 44 | 48 | 149 | 153 | 157 | 161 | 165)
}

/// Parse an `aa:bb:cc:dd:ee:ff` MAC into six bytes. Case-insensitive;
/// returns `None` on any malformed input. Used to validate `--peer-mac`.
pub fn parse_mac(s: &str) -> Option<[u8; 6]> {
    let parts: Vec<&str> = s.trim().split(':').collect();
    if parts.len() != 6 {
        return None;
    }
    let mut out = [0u8; 6];
    for (i, p) in parts.iter().enumerate() {
        out[i] = u8::from_str_radix(p.trim(), 16).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A config written by another build may name a PHY protocol this build
    /// does not offer, and may predate the reporting field. It must still
    /// load, falling back to the defaults for both.
    #[test]
    fn unknown_protocol_and_missing_reporting_fall_back() {
        let mut v = serde_json::to_value(EspConfig::default()).unwrap();
        let obj = v.as_object_mut().unwrap();
        obj.remove("reporting").unwrap();
        obj.insert("protocol".into(), "not-a-protocol".into());

        let cfg: EspConfig = serde_json::from_value(v).unwrap();
        assert_eq!(cfg.protocol, WifiProtocol::Lr);
        assert_eq!(cfg.reporting, Reporting::Report);

        let json = serde_json::to_string(&cfg).unwrap();
        let back: EspConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.protocol, WifiProtocol::Lr);
    }

    #[test]
    fn every_protocol_round_trips() {
        for p in WifiProtocol::ALL {
            let json = serde_json::to_string(&p).unwrap();
            assert_eq!(serde_json::from_str::<WifiProtocol>(&json).unwrap(), p);
        }
    }
}
