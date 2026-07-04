//! Strongly-typed counterparts to the `set-*` family of CLI commands.
//!
//! The types here mirror the firmware's `UserConfig` surface and
//! cycle/serialize/render-as-CLI on demand. They are the GUI's
//! source-of-truth for what gets pushed to the device with `ApplyConfig`.
//!
//! Grammar matches esp-csi-cli-rs v0.7.x (protocol 2): `set-csi` flags are
//! positive `on|off` toggles (not the old `--disable-*` booleans), and the
//! surface includes AP mode, ESP-NOW fast modes, `set-protocol`, unsolicited
//! traffic, `raw` CSI delivery, CSI presets, and C5-only toggles.

use serde::{Deserialize, Serialize};

/// Wi-Fi operating mode — what the firmware's PHY/MAC layer does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WifiMode {
    /// Associate to an AP using the configured SSID/password.
    Station,
    /// Promiscuous capture without association.
    Sniffer,
    /// Self-contained softAP CSI collector (DHCP + ICMP flood).
    WifiAp,
    /// ESP-NOW initiator role.
    EspNowCentral,
    /// ESP-NOW peripheral role.
    EspNowPeripheral,
    /// Asymmetric ESP-NOW simplex collector (sparse beacon, then RX-only).
    EspNowFastCollector,
    /// Asymmetric ESP-NOW simplex source (unicast flood at forced PHY).
    EspNowFastSource,
}

impl WifiMode {
    pub const ALL: [WifiMode; 7] = [
        WifiMode::Station,
        WifiMode::Sniffer,
        WifiMode::WifiAp,
        WifiMode::EspNowCentral,
        WifiMode::EspNowPeripheral,
        WifiMode::EspNowFastCollector,
        WifiMode::EspNowFastSource,
    ];

    /// CLI token used by `set-wifi --mode=…`.
    pub fn cli(self) -> &'static str {
        match self {
            Self::Station => "station",
            Self::Sniffer => "sniffer",
            Self::WifiAp => "wifi-ap",
            Self::EspNowCentral => "esp-now-central",
            Self::EspNowPeripheral => "esp-now-peripheral",
            Self::EspNowFastCollector => "esp-now-fast-collector",
            Self::EspNowFastSource => "esp-now-fast-source",
        }
    }
    /// Round-robin to the next mode (used by the config panel's
    /// click-to-cycle button).
    pub fn cycle(self) -> Self {
        match self {
            Self::Sniffer => Self::Station,
            Self::Station => Self::WifiAp,
            Self::WifiAp => Self::EspNowCentral,
            Self::EspNowCentral => Self::EspNowPeripheral,
            Self::EspNowPeripheral => Self::EspNowFastCollector,
            Self::EspNowFastCollector => Self::EspNowFastSource,
            Self::EspNowFastSource => Self::Sniffer,
        }
    }
}

/// Wi-Fi PHY protocol (`set-protocol --protocol=…`). Firmware default is `lr`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[allow(missing_docs)] // each variant maps 1:1 to a CLI token
pub enum WifiProtocol {
    B,
    G,
    N,
    Lr,
    A,
    Ac,
    Ax,
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

/// Forced ESP-NOW HT40 transmit secondary channel (`set-wifi --ht40=…`).
/// `None` reverts to HT20/legacy per rate.
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

/// Collection role: `Collector` actively generates traffic; `Listener`
/// is passive receive-only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CollectionMode {
    /// Actively transmit traffic and capture CSI.
    Collector,
    /// Passive: capture CSI without generating any traffic.
    Listener,
}

impl CollectionMode {
    pub const ALL: [CollectionMode; 2] = [CollectionMode::Collector, CollectionMode::Listener];

    /// CLI token used by `set-collection-mode --mode=…`.
    pub fn cli(self) -> &'static str {
        match self {
            Self::Collector => "collector",
            Self::Listener => "listener",
        }
    }
    /// Round-robin to the other mode.
    pub fn cycle(self) -> Self {
        match self {
            Self::Collector => Self::Listener,
            Self::Listener => Self::Collector,
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

/// CSI acquisition preset (`set-csi --preset=…`). `He20` applies the
/// firmware's HE-LTF-only preset, overriding the individual HE flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CsiPreset {
    /// No preset — the individual `set-csi` flags apply.
    Default,
    /// HE-LTF-only preset (`he20`).
    He20,
}

impl CsiPreset {
    pub const ALL: [CsiPreset; 2] = [CsiPreset::Default, CsiPreset::He20];

    /// CLI token used by `set-csi --preset=…`.
    pub fn cli(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::He20 => "he20",
        }
    }
    /// Round-robin to the next preset.
    pub fn cycle(self) -> Self {
        match self {
            Self::Default => Self::He20,
            Self::He20 => Self::Default,
        }
    }
}

/// PHY rate the firmware uses when transmitting (only meaningful for
/// `Collector` mode; ignored for `Listener`).
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
    /// Wi-Fi operating mode (`set-wifi --mode=…`).
    pub wifi_mode: WifiMode,
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
    /// ESP-NOW peer source MAC filter (`set-wifi --peer-mac=…`). Empty means
    /// automatic magic-prefix pairing (`auto`). ESP-NOW modes only.
    #[serde(default)]
    pub peer_mac: String,
    /// Forced ESP-NOW TX HT40 secondary channel (`set-wifi --ht40=…`).
    /// ESP-NOW modes only.
    #[serde(default)]
    pub ht40: Ht40,
    /// Traffic-generator frequency in Hz (`set-traffic --frequency-hz=…`).
    pub traffic_hz: u64,
    /// One-directional unsolicited ICMP-reply flood (`set-traffic --unsolicited=…`).
    pub traffic_unsolicited: bool,
    /// Collector vs Listener role (`set-collection-mode`).
    pub collection_mode: CollectionMode,
    /// Wire format for emitted CSI packets (`set-log-mode`).
    pub log_mode: LogMode,
    /// PHY rate to transmit at (`set-rate`).
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
    pub he_csi_en: bool,
    /// ESP32-C5/C6: L-LTF on 11g PPDU (`--csi-legacy=on|off`).
    pub he_csi_legacy_en: bool,
    /// ESP32-C5/C6: HT-LTF on HT20 PPDU (`--csi-ht20=on|off`).
    pub he_csi_ht20_en: bool,
    /// ESP32-C5/C6: HT-LTF on HT40 PPDU (`--csi-ht40=on|off`).
    pub he_csi_ht40_en: bool,
    /// ESP32-C5/C6: HE-LTF on HE20 SU PPDU (`--csi-su=on|off`).
    pub he_csi_su_en: bool,
    /// ESP32-C5/C6: HE-LTF on HE20 MU PPDU (`--csi-mu=on|off`).
    pub he_csi_mu_en: bool,
    /// ESP32-C5/C6: HE-LTF on HE20 DCM PPDU (`--csi-dcm=on|off`).
    pub he_csi_dcm_en: bool,
    /// ESP32-C5/C6: HE-LTF on HE20 beamformed PPDU (`--csi-beamformed=on|off`).
    pub he_csi_beamformed_en: bool,
    /// ESP32-C5/C6: STBC mode 0..=2 (`--csi-he-stbc=…`).
    pub he_stbc: u32,
    /// ESP32-C5/C6: value-scale config 0..=3 (`--val-scale-cfg=…`).
    pub he_val_scale_cfg: u32,
    /// ESP32-C5/C6: CSI acquisition preset (`--preset=…`).
    pub csi_preset: CsiPreset,
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
            protocol: WifiProtocol::Lr,
            channel: 6,
            sta_ssid: String::new(),
            sta_password: String::new(),
            ap_ssid: "esp-csi-ap".to_string(),
            ap_password: String::new(),
            ap_dhcp: true,
            peer_mac: String::new(),
            ht40: Ht40::None,
            traffic_hz: 100,
            traffic_unsolicited: false,
            collection_mode: CollectionMode::Collector,
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

            he_csi_en: true,
            he_csi_legacy_en: true,
            he_csi_ht20_en: true,
            he_csi_ht40_en: true,
            he_csi_su_en: true,
            he_csi_mu_en: true,
            he_csi_dcm_en: true,
            he_csi_beamformed_en: true,
            he_stbc: 2,
            he_val_scale_cfg: 2,
            csi_preset: CsiPreset::Default,
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
            return Err("Peer MAC must be aa:bb:cc:dd:ee:ff".into());
        }
        if self.he_stbc > 2 {
            return Err("CSI STBC must be 0..=2".into());
        }
        if self.he_val_scale_cfg > 3 {
            return Err("val_scale_cfg must be 0..=3".into());
        }
        if !valid_channel(self.channel) {
            return Err(format!("invalid channel {}", self.channel));
        }
        if !valid_peer_mac(&self.peer_mac) {
            return Err(format!(
                "invalid peer MAC '{}' (use aa:bb:cc:dd:ee:ff)",
                self.peer_mac
            ));
        }
        Ok(())
    }
}

/// `true` when `mac` is empty (meaning automatic pairing) or six hex octets
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
    "Peer MAC (ESP-NOW)",
    "HT40 (ESP-NOW)",
    "Traffic Hz",
    "Traffic Unsolicited",
    "Collection Mode",
    "Log Mode",
    "PHY Rate (ESP-NOW)",
    "TX Task",
    "RX Task",
    "CSI Delivery",
    "CSI Logging",
    "Classic: LLTF",
    "Classic: HT-LTF",
    "Classic: STBC-HT-LTF",
    "Classic: LTF-merge",
    "HE: CSI",
    "HE: CSI legacy",
    "HE: CSI HT20",
    "HE: CSI HT40",
    "HE: CSI SU",
    "HE: CSI MU",
    "HE: CSI DCM",
    "HE: CSI beamformed",
    "HE: CSI STBC (0..2)",
    "HE: val_scale_cfg (0..3)",
    "HE: CSI preset",
    "HE: dump ACK",
    "C5: force LLTF",
    "C5: VHT",
    "Start duration (sec, blank = ∞)",
];
