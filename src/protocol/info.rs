use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChipFamily {
    Esp32,
    Esp32C3,
    Esp32C5,
    Esp32C6,
    Esp32S3,
    Unknown,
}

impl ChipFamily {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "esp32" => Self::Esp32,
            "esp32c3" => Self::Esp32C3,
            "esp32c5" => Self::Esp32C5,
            "esp32c6" => Self::Esp32C6,
            "esp32s3" => Self::Esp32S3,
            _ => Self::Unknown,
        }
    }

    pub fn is_he(self) -> bool {
        matches!(self, Self::Esp32C5 | Self::Esp32C6)
    }

    pub fn is_classic(self) -> bool {
        matches!(self, Self::Esp32 | Self::Esp32C3 | Self::Esp32S3)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Esp32 => "esp32",
            Self::Esp32C3 => "esp32c3",
            Self::Esp32C5 => "esp32c5",
            Self::Esp32C6 => "esp32c6",
            Self::Esp32S3 => "esp32s3",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub name: String,
    pub version: String,
    pub chip: ChipFamily,
    pub protocol: u32,
    pub features: HashSet<String>,
}

impl Default for DeviceInfo {
    fn default() -> Self {
        Self {
            name: String::new(),
            version: String::new(),
            chip: ChipFamily::Unknown,
            protocol: 0,
            features: HashSet::new(),
        }
    }
}

pub const SUPPORTED_PROTOCOL: u32 = 1;

impl DeviceInfo {
    /// Parse `key=value` lines emitted between the banner and `END-INFO`.
    pub fn parse(text: &str) -> Self {
        let mut info = DeviceInfo::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line == "END-INFO" {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                let v = v.trim();
                match k.trim() {
                    "name" => info.name = v.to_string(),
                    "version" => info.version = v.to_string(),
                    "chip" => info.chip = ChipFamily::parse(v),
                    "protocol" => info.protocol = v.parse().unwrap_or(0),
                    "features" => {
                        info.features = v
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    }
                    _ => {}
                }
            }
        }
        info
    }

    pub fn protocol_supported(&self) -> bool {
        self.protocol == SUPPORTED_PROTOCOL
    }
}
