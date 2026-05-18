use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WifiMode {
    Station,
    Sniffer,
    EspNowCentral,
    EspNowPeripheral,
}

impl WifiMode {
    pub fn cli(self) -> &'static str {
        match self {
            Self::Station => "station",
            Self::Sniffer => "sniffer",
            Self::EspNowCentral => "esp-now-central",
            Self::EspNowPeripheral => "esp-now-peripheral",
        }
    }
    pub fn cycle(self) -> Self {
        match self {
            Self::Sniffer => Self::Station,
            Self::Station => Self::EspNowCentral,
            Self::EspNowCentral => Self::EspNowPeripheral,
            Self::EspNowPeripheral => Self::Sniffer,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CollectionMode {
    Collector,
    Listener,
}

impl CollectionMode {
    pub fn cli(self) -> &'static str {
        match self {
            Self::Collector => "collector",
            Self::Listener => "listener",
        }
    }
    pub fn cycle(self) -> Self {
        match self {
            Self::Collector => Self::Listener,
            Self::Listener => Self::Collector,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogMode {
    Serialized,
    Text,
    ArrayList,
    EspCsiTool,
}

impl LogMode {
    pub fn cli(self) -> &'static str {
        match self {
            Self::Serialized => "serialized",
            Self::Text => "text",
            Self::ArrayList => "array-list",
            Self::EspCsiTool => "esp-csi-tool",
        }
    }
    pub fn cycle(self) -> Self {
        match self {
            Self::Serialized => Self::Text,
            Self::Text => Self::ArrayList,
            Self::ArrayList => Self::EspCsiTool,
            Self::EspCsiTool => Self::Serialized,
        }
    }
    pub fn id(self) -> u8 {
        match self {
            Self::Serialized => 0,
            Self::Text => 1,
            Self::ArrayList => 2,
            Self::EspCsiTool => 3,
        }
    }
    pub fn host_parses(self) -> bool {
        matches!(
            self,
            Self::Serialized | Self::Text | Self::EspCsiTool | Self::ArrayList
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CsiDelivery {
    Off,
    Callback,
    Async,
}

impl CsiDelivery {
    pub fn cli(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Callback => "callback",
            Self::Async => "async",
        }
    }
    pub fn cycle(self) -> Self {
        match self {
            Self::Off => Self::Callback,
            Self::Callback => Self::Async,
            Self::Async => Self::Off,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PhyRate {
    R1m, R1mL, R2m, R5m5, R5m5L, R11m, R11mL,
    R6m, R9m, R12m, R18m, R24m, R36m, R48m, R54m,
    Mcs0Lgi, Mcs1Lgi, Mcs2Lgi, Mcs3Lgi, Mcs4Lgi, Mcs5Lgi, Mcs6Lgi, Mcs7Lgi,
    Mcs0Sgi,
}

impl PhyRate {
    pub const ALL: [PhyRate; 24] = [
        PhyRate::R1m, PhyRate::R1mL, PhyRate::R2m, PhyRate::R5m5, PhyRate::R5m5L,
        PhyRate::R11m, PhyRate::R11mL, PhyRate::R6m, PhyRate::R9m, PhyRate::R12m,
        PhyRate::R18m, PhyRate::R24m, PhyRate::R36m, PhyRate::R48m, PhyRate::R54m,
        PhyRate::Mcs0Lgi, PhyRate::Mcs1Lgi, PhyRate::Mcs2Lgi, PhyRate::Mcs3Lgi,
        PhyRate::Mcs4Lgi, PhyRate::Mcs5Lgi, PhyRate::Mcs6Lgi, PhyRate::Mcs7Lgi,
        PhyRate::Mcs0Sgi,
    ];

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
    pub fn cycle(self) -> Self {
        let i = Self::ALL.iter().position(|r| *r == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EspConfig {
    pub wifi_mode: WifiMode,
    pub channel: u8,
    pub sta_ssid: String,
    pub sta_password: String,
    pub traffic_hz: u64,
    pub collection_mode: CollectionMode,
    pub log_mode: LogMode,
    pub phy_rate: PhyRate,
    pub tx_task: bool,
    pub rx_task: bool,
    pub csi_delivery: CsiDelivery,
    pub csi_logging: bool,

    pub classic_disable_lltf: bool,
    pub classic_disable_htltf: bool,
    pub classic_disable_stbc_htltf: bool,
    pub classic_disable_ltf_merge: bool,

    pub he_disable_csi: bool,
    pub he_disable_csi_legacy: bool,
    pub he_disable_csi_ht20: bool,
    pub he_disable_csi_ht40: bool,
    pub he_disable_csi_su: bool,
    pub he_disable_csi_mu: bool,
    pub he_disable_csi_dcm: bool,
    pub he_disable_csi_beamformed: bool,
    pub he_stbc: u32,
    pub he_val_scale_cfg: u32,

    pub start_duration_sec: Option<u64>,
}

impl Default for EspConfig {
    fn default() -> Self {
        Self {
            wifi_mode: WifiMode::Sniffer,
            channel: 6,
            sta_ssid: String::new(),
            sta_password: String::new(),
            traffic_hz: 100,
            collection_mode: CollectionMode::Collector,
            log_mode: LogMode::Text,
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
        Ok(())
    }
}

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
