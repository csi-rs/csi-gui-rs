//! Strongly-typed counterparts to the `set-*` family of CLI commands.
//!
//! The types here mirror the firmware's `UserConfig` surface and
//! cycle/serialize/render-as-CLI on demand. They are the GUI's
//! source-of-truth for what gets pushed to the device with `ApplyConfig`.

use serde::{Deserialize, Serialize};

/// Wi-Fi operating mode — what the firmware's PHY/MAC layer does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WifiMode {
    /// Associate to an AP using the configured SSID/password.
    Station,
    /// Promiscuous capture without association.
    Sniffer,
    /// ESP-NOW initiator role.
    EspNowCentral,
    /// ESP-NOW peripheral role.
    EspNowPeripheral,
}

impl WifiMode {
    /// CLI token used by `set-wifi --mode=…`.
    pub fn cli(self) -> &'static str {
        match self {
            Self::Station => "station",
            Self::Sniffer => "sniffer",
            Self::EspNowCentral => "esp-now-central",
            Self::EspNowPeripheral => "esp-now-peripheral",
        }
    }
    /// Round-robin to the next mode (used by the config overlay's
    /// click-to-cycle button).
    pub fn cycle(self) -> Self {
        match self {
            Self::Sniffer => Self::Station,
            Self::Station => Self::EspNowCentral,
            Self::EspNowCentral => Self::EspNowPeripheral,
            Self::EspNowPeripheral => Self::Sniffer,
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
    /// status badge in the config overlay can stay generic.
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

/// Forced ESP-NOW TX HT40 secondary channel (`set-wifi --ht40=…`). Only
/// meaningful for the ESP-NOW modes; `None` keeps the default HT20/legacy
/// TX PHY.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Ht40 {
    /// HT20 / legacy TX PHY (firmware default).
    #[default]
    None,
    /// HT40 with the secondary channel above the primary.
    Above,
    /// HT40 with the secondary channel below the primary.
    Below,
}

impl Ht40 {
    /// CLI token used by `set-wifi --ht40=…`.
    pub fn cli(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Above => "above",
            Self::Below => "below",
        }
    }
    /// Round-robin to the next setting.
    pub fn cycle(self) -> Self {
        match self {
            Self::None => Self::Above,
            Self::Above => Self::Below,
            Self::Below => Self::None,
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
/// rendering. Field grouping mirrors the config-overlay UI.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EspConfig {
    /// Wi-Fi operating mode (`set-wifi --mode=…`).
    pub wifi_mode: WifiMode,
    /// Primary channel (`set-wifi --set-channel=…`). Must satisfy
    /// [`valid_channel`].
    pub channel: u8,
    /// Station-mode SSID (`set-wifi --sta-ssid=…`). Max 32 bytes.
    pub sta_ssid: String,
    /// Station-mode PSK (`set-wifi --sta-password=…`). Max 32 bytes.
    pub sta_password: String,
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

    /// Classic chips: disable Legacy-LTF capture (`set-csi --disable-lltf`).
    pub classic_disable_lltf: bool,
    /// Classic chips: disable HT-LTF capture (`--disable-htltf`).
    pub classic_disable_htltf: bool,
    /// Classic chips: disable STBC-HT-LTF capture (`--disable-stbc-htltf`).
    pub classic_disable_stbc_htltf: bool,
    /// Classic chips: disable LTF merging (`--disable-ltf-merge`).
    pub classic_disable_ltf_merge: bool,

    /// ESP32-C5/C6: disable CSI entirely (`set-csi --disable-csi`).
    pub he_disable_csi: bool,
    /// ESP32-C5/C6: disable legacy-format CSI (`--disable-csi-legacy`).
    pub he_disable_csi_legacy: bool,
    /// ESP32-C5/C6: disable HT20 CSI (`--disable-csi-ht20`).
    pub he_disable_csi_ht20: bool,
    /// ESP32-C5/C6: disable HT40 CSI (`--disable-csi-ht40`).
    pub he_disable_csi_ht40: bool,
    /// ESP32-C5/C6: disable HE-SU CSI (`--disable-csi-su`).
    pub he_disable_csi_su: bool,
    /// ESP32-C5/C6: disable HE-MU CSI (`--disable-csi-mu`).
    pub he_disable_csi_mu: bool,
    /// ESP32-C5/C6: disable HE-DCM CSI (`--disable-csi-dcm`).
    pub he_disable_csi_dcm: bool,
    /// ESP32-C5/C6: disable beamformed-CSI capture (`--disable-csi-beamformed`).
    pub he_disable_csi_beamformed: bool,
    /// ESP32-C5/C6: STBC mode 0..=2 (`--csi-he-stbc=…`).
    pub he_stbc: u32,
    /// ESP32-C5/C6: value-scale config 0..=3 (`--val-scale-cfg=…`).
    pub he_val_scale_cfg: u32,

    /// Auto-stop after this many seconds (`start --duration=N`). `None`
    /// means "run until user sends `q`".
    pub start_duration_sec: Option<u64>,
}

impl Default for EspConfig {
    fn default() -> Self {
        Self {
            wifi_mode: WifiMode::Sniffer,
            channel: 6,
            sta_ssid: String::new(),
            sta_password: String::new(),
            peer_mac: String::new(),
            ht40: Ht40::None,
            traffic_hz: 100,
            collection_mode: CollectionMode::Collector,
            log_mode: LogMode::Serialized,
            phy_rate: PhyRate::Mcs0Lgi,
            tx_task: true,
            rx_task: true,
            csi_delivery: CsiDelivery::Callback,
            csi_logging: true,

            classic_disable_lltf: false,
            classic_disable_htltf: false,
            classic_disable_stbc_htltf: false,
            classic_disable_ltf_merge: false,

            he_disable_csi: false,
            he_disable_csi_legacy: false,
            he_disable_csi_ht20: false,
            he_disable_csi_ht40: false,
            he_disable_csi_su: false,
            he_disable_csi_mu: false,
            he_disable_csi_dcm: false,
            he_disable_csi_beamformed: false,
            he_stbc: 0,
            he_val_scale_cfg: 0,

            start_duration_sec: None,
        }
    }
}

impl EspConfig {
    /// Validate string-field lengths before sending. SSIDs/passwords > 32 bytes
    /// would panic the device (`heapless::String::push_str().unwrap()`).
    pub fn validate(&self) -> Result<(), String> {
        if self.sta_ssid.len() > 32 {
            return Err("SSID exceeds 32 bytes".into());
        }
        if self.sta_password.len() > 32 {
            return Err("Password exceeds 32 bytes".into());
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
/// the config overlay and to fail `EspConfig::validate` early.
pub fn valid_channel(c: u8) -> bool {
    matches!(
        c,
        1..=14 | 36 | 40 | 44 | 48 | 149 | 153 | 157 | 161 | 165
    )
}

/// Display-ordered list of fields. Matches the TUI's `ESP_CONFIG_FIELDS`.
pub const ESP_CONFIG_FIELDS: &[&str] = &[
    "WiFi Mode",
    "Channel",
    "STA SSID",
    "STA Password",
    "Peer MAC (ESP-NOW)",
    "HT40 (ESP-NOW)",
    "Traffic Hz",
    "Collection Mode",
    "Log Mode",
    "PHY Rate (ESP-NOW)",
    "TX Task",
    "RX Task",
    "CSI Delivery",
    "CSI Logging",
    "Classic: disable LLTF",
    "Classic: disable HT-LTF",
    "Classic: disable STBC-HT-LTF",
    "Classic: disable LTF-merge",
    "HE: disable CSI",
    "HE: disable CSI legacy",
    "HE: disable CSI HT20",
    "HE: disable CSI HT40",
    "HE: disable CSI SU",
    "HE: disable CSI MU",
    "HE: disable CSI DCM",
    "HE: disable CSI beamformed",
    "HE: CSI STBC (0..2)",
    "HE: val_scale_cfg (0..3)",
    "Start duration (sec, blank = ∞)",
];
