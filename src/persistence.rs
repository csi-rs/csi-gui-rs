//! On-disk persistence: layout templates (JSON) and CSI captures (CSV).
//!
//! Layout templates live in `templates/` (relative to the working
//! directory) and CSV exports in `exports/`. The CSV schema is uniform
//! across every supported wire format — see [`CSV_COLUMNS`].

use crate::layout::TilingManager;
use std::fs;
use std::path::{Path, PathBuf};

pub const TEMPLATES_DIR: &str = "templates";

pub struct TemplateEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_default: bool,
}

pub fn templates_dir() -> PathBuf {
    PathBuf::from(TEMPLATES_DIR)
}

pub fn save_template(name: &str, tm: &TilingManager) -> std::io::Result<PathBuf> {
    let dir = templates_dir();
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{name}.json"));
    let s = serde_json::to_string_pretty(tm)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    fs::write(&path, s)?;
    Ok(path)
}

pub fn list_templates() -> Vec<TemplateEntry> {
    let dir = templates_dir();
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(&dir) else {
        return out;
    };
    for ent in entries.flatten() {
        let path = ent.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        let is_default = read_template(&path)
            .map(|tm| tm.is_default)
            .unwrap_or(false);
        out.push(TemplateEntry {
            name,
            path,
            is_default,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

pub fn read_template(path: &Path) -> std::io::Result<TilingManager> {
    let s = fs::read_to_string(path)?;
    serde_json::from_str(&s).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

pub fn load_startup_template() -> Option<TilingManager> {
    list_templates()
        .into_iter()
        .find(|e| e.is_default)
        .and_then(|e| read_template(&e.path).ok())
}

pub fn set_default(name: &str) -> std::io::Result<()> {
    for ent in list_templates() {
        if let Ok(mut tm) = read_template(&ent.path) {
            tm.is_default = ent.name == name;
            let s = serde_json::to_string_pretty(&tm)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            fs::write(&ent.path, s)?;
        }
    }
    Ok(())
}

/// Canonical CSV column order — emitted by [`export_history_csv`] for
/// **every** packet regardless of the wire format the firmware used.
///
/// `csi_raw_data` is space-separated decimal integers wrapped in `[…]`.
pub const CSV_COLUMNS: &[&str] = &[
    "mac",
    "rssi",
    "rate",
    "noise_floor",
    "channel",
    "secondary_channel",
    "timestamp",
    "sig_len",
    "rx_state",
    "sig_mode",
    "mcs",
    "cwb",
    "smoothing",
    "not_sounding",
    "aggregation",
    "stbc",
    "fec_coding",
    "sgi",
    "ant",
    "ampdu_cnt",
    "data_length",
    "subcarriers",
    "csi_raw_data",
];

/// Write a CSV with one row per packet to `exports/<name>_<unix>.csv`.
///
/// Uses the schema in [`CSV_COLUMNS`]. Returns the path written.
///
/// # Errors
///
/// Returns the underlying `std::io::Error` if the file cannot be created
/// or written.
pub fn export_history_csv(name: &str, raw: &[crate::data::CsiData]) -> std::io::Result<PathBuf> {
    let dir = PathBuf::from("exports");
    fs::create_dir_all(&dir)?;
    let unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = dir.join(format!("{name}_{unix}.csv"));
    let mut wtr = csv::Writer::from_path(&path)?;
    wtr.write_record(CSV_COLUMNS)?;
    for p in raw {
        let csi_str = p
            .csi_raw_data
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        wtr.write_record(&[
            p.mac.clone(),
            p.rssi.to_string(),
            p.rate.to_string(),
            p.noise_floor.to_string(),
            p.channel.to_string(),
            p.secondary_channel.to_string(),
            p.timestamp.to_string(),
            p.sig_len.to_string(),
            p.rx_state.to_string(),
            p.sig_mode.to_string(),
            p.mcs.to_string(),
            p.cwb.to_string(),
            p.smoothing.to_string(),
            p.not_sounding.to_string(),
            p.aggregation.to_string(),
            p.stbc.to_string(),
            p.fec_coding.to_string(),
            p.sgi.to_string(),
            p.ant.to_string(),
            p.ampdu_cnt.to_string(),
            p.data_length.to_string(),
            p.subcarriers().to_string(),
            format!("[{csi_str}]"),
        ])?;
    }
    wtr.flush()?;
    Ok(path)
}
