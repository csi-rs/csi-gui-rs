//! Strongly-typed counterparts to the `set-*` family of CLI commands.
//!
//! The types here mirror the firmware's `UserConfig` surface and
//! cycle/serialize/render-as-CLI on demand. They are the GUI's
//! source-of-truth for what gets pushed to the device with `ApplyConfig`.
//!
//! Grammar matches esp-csi-cli-rs v0.10.0 or later: `set-csi` flags are
//! positive `on|off` toggles (not the old `--disable-*` booleans), and the
//! surface includes AP mode, the emitter and ESP-NOW modes, the reporting
//! setting (`set-wifi --reporting`), `set-protocol`, the measurement
//! periodicity (`set-measurement`), `raw` CSI delivery, the CSI-output
//! delivery gate, and C5-only toggles. Mode tokens and flag names follow the
//! IEEE 802.11bf vocabulary of esp-csi-cli-rs 0.10.0 (CLI `protocol=4`).

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
            Self::Station | Self::WifiAp | Self::EspNowInitiator | Self::EspNowResponder
        )
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
            Self::WifiAp | Self::EspNowInitiator | Self::EspNowResponder | Self::EspNowSimplexSource
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
    pub const ALL: [WifiProtocol; 6] = [
        WifiProtocol::B,
        WifiProtocol::G,
        WifiProtocol::N,
        WifiProtocol::Lr,
        WifiProtocol::A,
        WifiProtocol::Ac,
    ];

    /// CLI token used by `set-protocol --protocol=…`.
    pub fn cli(self) -> &'static str {
        match self {
            Self::B => "b",
            Self::G => "g",
            Self::N => "n",
            Self::Lr => "lr",
            Self::A => "a",
            Self::Ac => "ac",
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
            Self::Ac => Self::B,
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

/// The node's **reporting** setting (`set-wifi --reporting=…`): whether the
/// CSI it measures is reported to the host. `report` sends it; `none`
/// measures but reports nothing.
///
/// Only read by the modes that admit a choice
/// ([`WifiMode::admits_reporting_choice`]); every other mode fixes it.
/// Needs esp-csi-cli-rs 0.10.0 or later.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Reporting {
    /// Report measured CSI to the host (the firmware default).
    #[default]
    Report,
    /// Measure, but report nothing.
    None,
}

impl Reporting {
    pub const ALL: [Reporting; 2] = [Reporting::Report, Reporting::None];

    /// CLI token used by `set-wifi --reporting=…`.
    pub fn cli(self) -> &'static str {
        match self {
            Self::Report => "report",
            Self::None => "none",
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
    R1m, R1mL, R2m, R5m5, R5m5L, R11m, R11mL,
    R6m, R9m, R12m, R18m, R24m, R36m, R48m, R54m,
    Mcs0Lgi, Mcs1Lgi, Mcs2Lgi, Mcs3Lgi, Mcs4Lgi, Mcs5Lgi, Mcs6Lgi, Mcs7Lgi,
    Mcs0Sgi,
}

impl PhyRate {
    /// Every variant in declaration order — used for round-robin cycling.
    pub const ALL: [PhyRate; 24] = [
        PhyRate::R1m, PhyRate::R1mL, PhyRate::R2m, PhyRate::R5m5, PhyRate::R5m5L,
        PhyRate::R11m, PhyRate::R11mL, PhyRate::R6m, PhyRate::R9m, PhyRate::R12m,
        PhyRate::R18m, PhyRate::R24m, PhyRate::R36m, PhyRate::R48m, PhyRate::R54m,
        PhyRate::Mcs0Lgi, PhyRate::Mcs1Lgi, PhyRate::Mcs2Lgi, PhyRate::Mcs3Lgi,
        PhyRate::Mcs4Lgi, PhyRate::Mcs5Lgi, PhyRate::Mcs6Lgi, PhyRate::Mcs7Lgi,
        PhyRate::Mcs0Sgi,
    ];

    /// CLI token used by `set-rate --rate=…`.
    pub fn cli(self) -> &'static str {
        match self {
            Self::R1m => "1m", Self::R1mL => "1m-l", Self::R2m => "2m",
            Self::R5m5 => "5m5", Self::R5m5L => "5m5-l", Self::R11m => "11m",
            Self::R11mL => "11m-l", Self::R6m => "6m", Self::R9m => "9m",
            Self::R12m => "12m", Self::R18m => "18m", Self::R24m => "24m",
            Self::R36m => "36m", Self::R48m => "48m", Self::R54m => "54m",
            Self::Mcs0Lgi => "mcs0-lgi", Self::Mcs1Lgi => "mcs1-lgi",
            Self::Mcs2Lgi => "mcs2-lgi", Self::Mcs3Lgi => "mcs3-lgi",
            Self::Mcs4Lgi => "mcs4-lgi", Self::Mcs5Lgi => "mcs5-lgi",
            Self::Mcs6Lgi => "mcs6-lgi", Self::Mcs7Lgi => "mcs7-lgi",
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
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EspConfig {
    /// Mode (`set-wifi --mode=…`).
    pub wifi_mode: WifiMode,
    /// Reporting (`set-wifi --reporting=…`). Sent only for the modes
    /// that admit a choice; see [`WifiMode::admits_reporting_choice`].
    #[serde(default)]
    pub reporting: Reporting,
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

    /// Auto-stop after this many seconds (`start --duration=N`). `None`
    /// means "run until user sends `q`".
    pub start_duration_sec: Option<u64>,
}

impl Default for EspConfig {
    fn default() -> Self {
        Self {
            wifi_mode: WifiMode::Sniffer,
            reporting: Reporting::Report,
            protocol: WifiProtocol::Lr,
            channel: 6,
            sta_ssid: String::new(),
            sta_password: String::new(),
            ap_ssid: "esp-csi-ap".to_string(),
            ap_password: String::new(),
            ap_dhcp: true,
            peer_mac: String::new(),
            ht40: Ht40::None,
            periodicity_hz: 100,
            unsolicited: false,
            csi_output: true,
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

            start_duration_sec: None,
        }
    }
}

impl EspConfig {
    /// Validate string-field lengths / formats before sending. SSIDs/passwords
    /// > 32 bytes would panic the device (`heapless::String::push_str().unwrap()`).
    pub fn validate(&self) -> Result<(), String> {
        if self.sta_ssid.len() > 32 {
            return Err("STA SSID exceeds 32 bytes".into());
        }
        if self.sta_password.len() > 32 {
            return Err("STA password exceeds 32 bytes".into());
        }
        if self.ap_ssid.len() > 32 {
            return Err("AP SSID exceeds 32 bytes".into());
        }
        if self.ap_password.len() > 32 {
            return Err("AP password exceeds 32 bytes".into());
        }
        if !self.peer_mac.trim().is_empty() && parse_mac(&self.peer_mac).is_none() {
            return Err("Destination MAC must be aa:bb:cc:dd:ee:ff".into());
        }
        if self.c5c6_val_scale_cfg > 3 {
            return Err("val_scale_cfg must be 0..=3".into());
        }
        if !valid_channel(self.channel) {
            return Err(format!("invalid channel {}", self.channel));
        }
        if !valid_peer_mac(&self.peer_mac) {
            return Err(format!(
                "invalid destination MAC '{}' (use aa:bb:cc:dd:ee:ff)",
                self.peer_mac
            ));
        }
        Ok(())
    }
}

/// `true` when `mac` is empty (meaning broadcast) or six hex octets
/// separated by `:` or `-`, mirroring the firmware's `--peer-mac` parser.
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
    matches!(
        c,
        1..=14 | 36 | 40 | 44 | 48 | 149 | 153 | 157 | 161 | 165
    )
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

/// Display-ordered list of fields. The index of each label here MUST match
/// the arm order in [`crate::ui::esp_panel`]'s `render_value`.
pub const ESP_CONFIG_FIELDS: &[&str] = &[
    "WiFi Mode",
    "WiFi Protocol",
    "Channel",
    "STA SSID",
    "STA Password",
    "AP SSID",
    "AP Password",
    "AP DHCP",
    "Peer MAC (emitter / ESP-NOW)",
    "HT40 (AP / ESP-NOW)",
    "Reporting",
    "Periodicity (Hz)",
    "Unsolicited",
    "CSI Output",
    "Log Mode",
    "PHY Rate",
    "TX Task",
    "RX Task",
    "CSI Delivery",
    "CSI Logging",
    "Classic: LLTF",
    "Classic: HT-LTF",
    "Classic: STBC-HT-LTF",
    "Classic: LTF-merge",
    "C5/C6: CSI",
    "C5/C6: CSI legacy",
    "C5/C6: CSI HT20",
    "C5/C6: CSI HT40",
    "C5/C6: val_scale_cfg (0..3)",
    "CSI preset",
    "C5/C6: dump ACK",
    "C5: force LLTF",
    "C5: VHT",
    "Start duration (sec, blank = ∞)",
];

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
