//! On-disk persistence: layout templates (JSON) and CSI captures
//! (CSV/JSON/Parquet).
//!
//! Layout templates live in `templates/` (relative to the working
//! directory) and CSV/JSON/Parquet exports in `exports/`. The CSV
//! schema is uniform across every supported wire format — see
//! [`CSV_COLUMNS`].

use crate::data::PreprocessConfig;
use crate::layout::TilingManager;
use crate::views::ViewStates;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Subdirectory (relative to CWD) that holds the saved JSON templates.
pub const TEMPLATES_DIR: &str = "templates";

/// A saved workspace template — the tiling layout, the active
/// preprocessing config, and the per-pane view knobs (mode toggles,
/// window sizes, …). The wrapping struct is versioned so older
/// templates (which serialised a bare `TilingManager`) keep loading.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Template {
    /// Tiling layout (panes, splits, focused id, theme variant).
    pub tiling: TilingManager,
    /// Active preprocessing filters at save time.
    #[serde(default)]
    pub preprocess: PreprocessConfig,
    /// Per-pane view knobs (magnitude scale, phase mode, …).
    #[serde(default)]
    pub view_states: ViewStates,
}

impl From<TilingManager> for Template {
    fn from(tm: TilingManager) -> Self {
        Self {
            tiling: tm,
            preprocess: PreprocessConfig::default(),
            view_states: ViewStates::default(),
        }
    }
}

/// Listing entry for the template picker overlay.
pub struct TemplateEntry {
    /// Display name (file stem without `.json`).
    pub name: String,
    /// On-disk location.
    pub path: PathBuf,
    /// Whether this template is the auto-load-on-startup default.
    pub is_default: bool,
}

/// Directory where templates live (auto-created on first save).
pub fn templates_dir() -> PathBuf {
    PathBuf::from(TEMPLATES_DIR)
}

/// Persist a template under `templates/<name>.json`.
pub fn save_template(name: &str, tpl: &Template) -> std::io::Result<PathBuf> {
    let dir = templates_dir();
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{name}.json"));
    let s = serde_json::to_string_pretty(tpl)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    fs::write(&path, s)?;
    Ok(path)
}

/// Enumerate all templates in [`TEMPLATES_DIR`], sorted by name.
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
            .map(|tpl| tpl.tiling.is_default)
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

/// Parse a template file. Falls back to the legacy "bare
/// `TilingManager`" layout so templates saved before the `Template`
/// wrapper landed still load — they come back with default
/// preprocessing and empty view states.
pub fn read_template(path: &Path) -> std::io::Result<Template> {
    let s = fs::read_to_string(path)?;
    if let Ok(tpl) = serde_json::from_str::<Template>(&s) {
        return Ok(tpl);
    }
    let tm: TilingManager = serde_json::from_str(&s)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(tm.into())
}

/// Look up the template marked `is_default = true`, if any.
pub fn load_startup_template() -> Option<Template> {
    list_templates()
        .into_iter()
        .find(|e| e.is_default)
        .and_then(|e| read_template(&e.path).ok())
}

/// Toggle the `is_default` flag so exactly the named template loads on
/// next startup. Other templates have their `is_default` cleared in
/// the same pass.
pub fn set_default(name: &str) -> std::io::Result<()> {
    for ent in list_templates() {
        if let Ok(mut tpl) = read_template(&ent.path) {
            tpl.tiling.is_default = ent.name == name;
            let s = serde_json::to_string_pretty(&tpl)
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

/// Output format the export pipeline writes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExportFormat {
    /// Plain CSV (Hernandez-style schema, [`CSV_COLUMNS`]).
    #[default]
    Csv,
    /// Newline-delimited JSON (one [`crate::data::CsiData`] per line).
    Json,
    /// Apache Parquet with snappy compression. Same column layout as
    /// the CSV; `csi_raw_data` is stored as a space-separated UTF-8
    /// string for round-trip simplicity.
    Parquet,
}

impl ExportFormat {
    /// File extension (no leading dot) for the format.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Json => "jsonl",
            Self::Parquet => "parquet",
        }
    }

    /// Detect the format from a path's extension. Unknown / missing
    /// extensions fall back to [`ExportFormat::Csv`].
    pub fn from_path(path: &Path) -> Self {
        match path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase())
            .as_deref()
        {
            Some("json" | "jsonl" | "ndjson") => Self::Json,
            Some("parquet" | "pq") => Self::Parquet,
            _ => Self::Csv,
        }
    }
}

/// Dispatch to the right `export_history_*` based on `format`.
/// Returns the path written.
pub fn export_history(
    name: &str,
    raw: &[crate::data::CsiData],
    format: ExportFormat,
) -> std::io::Result<PathBuf> {
    match format {
        ExportFormat::Csv => export_history_csv(name, raw),
        ExportFormat::Json => export_history_json(name, raw),
        ExportFormat::Parquet => export_history_parquet(name, raw),
    }
}

fn export_path(name: &str, ext: &str) -> std::io::Result<PathBuf> {
    let dir = PathBuf::from("exports");
    fs::create_dir_all(&dir)?;
    let unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Ok(dir.join(format!("{name}_{unix}.{ext}")))
}

/// Write a CSV with one row per packet to `exports/<name>_<unix>.csv`.
///
/// Uses the schema in [`CSV_COLUMNS`]. Returns the path written.
///
/// # Errors
///
/// Returns the underlying `std::io::Error` if the file cannot be created
/// or written.
pub fn export_history_csv(name: &str, raw: &[crate::data::CsiData]) -> std::io::Result<PathBuf> {
    let path = export_path(name, "csv")?;
    let mut wtr = csv::Writer::from_path(&path)?;
    wtr.write_record(CSV_COLUMNS)?;
    for p in raw {
        wtr.write_record(packet_to_string_row(p))?;
    }
    wtr.flush()?;
    Ok(path)
}

/// Write newline-delimited JSON (one [`crate::data::CsiData`] per
/// line) to `exports/<name>_<unix>.jsonl`. Uses the type's built-in
/// `Serialize` impl, so the field set is exactly the in-memory
/// representation — round-trips losslessly through
/// [`import_history_json`].
pub fn export_history_json(name: &str, raw: &[crate::data::CsiData]) -> std::io::Result<PathBuf> {
    use std::io::Write;
    let path = export_path(name, "jsonl")?;
    let mut f = std::io::BufWriter::new(fs::File::create(&path)?);
    for p in raw {
        let s = serde_json::to_string(p)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        f.write_all(s.as_bytes())?;
        f.write_all(b"\n")?;
    }
    f.flush()?;
    Ok(path)
}

/// Write a Snappy-compressed Apache Parquet file with the same column
/// layout as the CSV exporter to `exports/<name>_<unix>.parquet`.
/// `csi_raw_data` is stored as a space-separated UTF-8 string in a
/// single `BYTE_ARRAY` column to keep the schema flat — readers can
/// split on whitespace to recover the `Vec<i32>`.
pub fn export_history_parquet(
    name: &str,
    raw: &[crate::data::CsiData],
) -> std::io::Result<PathBuf> {
    use parquet::basic::Compression;
    use parquet::data_type::{ByteArray, ByteArrayType, Int32Type, Int64Type};
    use parquet::file::properties::WriterProperties;
    use parquet::file::writer::SerializedFileWriter;
    use parquet::schema::parser::parse_message_type;
    use std::sync::Arc;

    let path = export_path(name, "parquet")?;
    let schema = parse_message_type(PARQUET_SCHEMA)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    let props = Arc::new(
        WriterProperties::builder()
            .set_compression(Compression::SNAPPY)
            .build(),
    );
    let file = fs::File::create(&path)?;
    let mut writer = SerializedFileWriter::new(file, Arc::new(schema), props)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    let mut rg = writer
        .next_row_group()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    // Pre-materialise each column's vector once — the writer wants one
    // call per column in schema order.
    let macs: Vec<ByteArray> = raw
        .iter()
        .map(|p| ByteArray::from(p.mac.as_str()))
        .collect();
    let csi_strs: Vec<ByteArray> = raw
        .iter()
        .map(|p| ByteArray::from(format!("[{}]", csi_join(&p.csi_raw_data)).as_str()))
        .collect();
    let rssi: Vec<i32> = raw.iter().map(|p| p.rssi).collect();
    let nf: Vec<i32> = raw.iter().map(|p| p.noise_floor).collect();
    let rate: Vec<i64> = raw.iter().map(|p| p.rate as i64).collect();
    let chan: Vec<i64> = raw.iter().map(|p| p.channel as i64).collect();
    let sec_chan: Vec<i64> = raw.iter().map(|p| p.secondary_channel as i64).collect();
    let ts: Vec<i64> = raw.iter().map(|p| p.timestamp as i64).collect();
    let sig_len: Vec<i64> = raw.iter().map(|p| p.sig_len as i64).collect();
    let rx_state: Vec<i64> = raw.iter().map(|p| p.rx_state as i64).collect();
    let sig_mode: Vec<i64> = raw.iter().map(|p| p.sig_mode as i64).collect();
    let mcs: Vec<i64> = raw.iter().map(|p| p.mcs as i64).collect();
    let cwb: Vec<i64> = raw.iter().map(|p| p.cwb as i64).collect();
    let smoothing: Vec<i64> = raw.iter().map(|p| p.smoothing as i64).collect();
    let not_sounding: Vec<i64> = raw.iter().map(|p| p.not_sounding as i64).collect();
    let aggregation: Vec<i64> = raw.iter().map(|p| p.aggregation as i64).collect();
    let stbc: Vec<i64> = raw.iter().map(|p| p.stbc as i64).collect();
    let fec_coding: Vec<i64> = raw.iter().map(|p| p.fec_coding as i64).collect();
    let sgi: Vec<i64> = raw.iter().map(|p| p.sgi as i64).collect();
    let ant: Vec<i64> = raw.iter().map(|p| p.ant as i64).collect();
    let ampdu_cnt: Vec<i64> = raw.iter().map(|p| p.ampdu_cnt as i64).collect();
    let data_length: Vec<i64> = raw.iter().map(|p| p.data_length as i64).collect();
    let subcarriers: Vec<i64> = raw.iter().map(|p| p.subcarriers() as i64).collect();

    // Write columns in the order declared by `PARQUET_SCHEMA`.
    let cols_str: [&[ByteArray]; 1] = [&macs];
    let cols_i32: [&[i32]; 2] = [&rssi, &nf];
    let cols_i64: [&[i64]; 19] = [
        &rate,
        &chan,
        &sec_chan,
        &ts,
        &sig_len,
        &rx_state,
        &sig_mode,
        &mcs,
        &cwb,
        &smoothing,
        &not_sounding,
        &aggregation,
        &stbc,
        &fec_coding,
        &sgi,
        &ant,
        &ampdu_cnt,
        &data_length,
        &subcarriers,
    ];
    let cols_byte_tail: [&[ByteArray]; 1] = [&csi_strs];

    // mac
    write_column::<ByteArrayType>(&mut rg, cols_str[0])?;
    // rssi, noise_floor
    for col in cols_i32 {
        write_column::<Int32Type>(&mut rg, col)?;
    }
    // remaining numeric columns
    for col in cols_i64 {
        write_column::<Int64Type>(&mut rg, col)?;
    }
    // csi_raw_data
    write_column::<ByteArrayType>(&mut rg, cols_byte_tail[0])?;

    rg.close()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    writer
        .close()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    Ok(path)
}

fn write_column<T>(
    rg: &mut parquet::file::writer::SerializedRowGroupWriter<'_, std::fs::File>,
    values: &[T::T],
) -> std::io::Result<()>
where
    T: parquet::data_type::DataType,
{
    let Some(mut col) = rg
        .next_column()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?
    else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "missing next column in parquet writer",
        ));
    };
    col.typed::<T>()
        .write_batch(values, None, None)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    col.close()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    Ok(())
}

fn packet_to_string_row(p: &crate::data::CsiData) -> [String; 23] {
    [
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
        format!("[{}]", csi_join(&p.csi_raw_data)),
    ]
}

fn csi_join(samples: &[i32]) -> String {
    let mut out = String::with_capacity(samples.len() * 4);
    for (i, v) in samples.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(&v.to_string());
    }
    out
}

/// Parquet schema string — `csi_raw_data` is stored as a UTF-8
/// space-separated list to keep the file structure flat.
const PARQUET_SCHEMA: &str = "
message csi_packet {
    required binary mac (UTF8);
    required int32 rssi;
    required int32 noise_floor;
    required int64 rate;
    required int64 channel;
    required int64 secondary_channel;
    required int64 timestamp;
    required int64 sig_len;
    required int64 rx_state;
    required int64 sig_mode;
    required int64 mcs;
    required int64 cwb;
    required int64 smoothing;
    required int64 not_sounding;
    required int64 aggregation;
    required int64 stbc;
    required int64 fec_coding;
    required int64 sgi;
    required int64 ant;
    required int64 ampdu_cnt;
    required int64 data_length;
    required int64 subcarriers;
    required binary csi_raw_data (UTF8);
}
";

/// Read packets back out of a file produced by [`export_history`].
/// The format is auto-detected from the path's extension.
pub fn import_history(path: &Path) -> std::io::Result<Vec<crate::data::CsiData>> {
    match ExportFormat::from_path(path) {
        ExportFormat::Csv => import_history_csv(path),
        ExportFormat::Json => import_history_json(path),
        ExportFormat::Parquet => import_history_parquet(path),
    }
}

fn parse_csi_field(s: &str) -> Vec<i32> {
    s.trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|t| !t.is_empty())
        .filter_map(|t| t.parse::<i32>().ok())
        .collect()
}

/// Parse a CSV produced by [`export_history_csv`].
pub fn import_history_csv(path: &Path) -> std::io::Result<Vec<crate::data::CsiData>> {
    let mut rdr = csv::Reader::from_path(path)?;
    let mut out = Vec::new();
    for rec in rdr.records() {
        let rec = rec?;
        if rec.len() < CSV_COLUMNS.len() {
            continue;
        }
        let mut p = crate::data::CsiData {
            mac: rec[0].to_string(),
            rssi: rec[1].parse().unwrap_or(0),
            rate: rec[2].parse().unwrap_or(0),
            noise_floor: rec[3].parse().unwrap_or(0),
            channel: rec[4].parse().unwrap_or(0),
            secondary_channel: rec[5].parse().unwrap_or(0),
            timestamp: rec[6].parse().unwrap_or(0),
            sig_len: rec[7].parse().unwrap_or(0),
            rx_state: rec[8].parse().unwrap_or(0),
            sig_mode: rec[9].parse().unwrap_or(0),
            mcs: rec[10].parse().unwrap_or(0),
            cwb: rec[11].parse().unwrap_or(0),
            smoothing: rec[12].parse().unwrap_or(0),
            not_sounding: rec[13].parse().unwrap_or(0),
            aggregation: rec[14].parse().unwrap_or(0),
            stbc: rec[15].parse().unwrap_or(0),
            fec_coding: rec[16].parse().unwrap_or(0),
            sgi: rec[17].parse().unwrap_or(0),
            ant: rec[18].parse().unwrap_or(0),
            ampdu_cnt: rec[19].parse().unwrap_or(0),
            data_length: rec[20].parse().unwrap_or(0),
            sig_len_extra: 0,
            csi_raw_data: parse_csi_field(&rec[22]),
        };
        p.data_length = p.data_length.max(p.csi_raw_data.len() as u32);
        out.push(p);
    }
    Ok(out)
}

/// Parse a JSONL file produced by [`export_history_json`].
pub fn import_history_json(path: &Path) -> std::io::Result<Vec<crate::data::CsiData>> {
    let s = fs::read_to_string(path)?;
    let mut out = Vec::new();
    for line in s.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<crate::data::CsiData>(line) {
            Ok(p) => out.push(p),
            Err(_) => continue,
        }
    }
    Ok(out)
}

#[cfg(test)]
mod export_roundtrip_tests {
    use super::*;
    use crate::data::CsiData;
    use std::sync::Mutex;

    // export_history* writes into the CWD-relative `exports/` directory,
    // so tests that exercise it have to serialise their CWD swap.
    static CWD_LOCK: Mutex<()> = Mutex::new(());

    fn sample_packet(seed: i32) -> CsiData {
        let mut p = CsiData::default();
        p.mac = format!("AA:BB:CC:DD:EE:{:02X}", seed.unsigned_abs() & 0xFF);
        p.rssi = -50 - seed;
        p.rate = 11;
        p.noise_floor = -90;
        p.channel = 6;
        p.timestamp = seed as u64 * 1000;
        p.sig_len = 128;
        p.csi_raw_data = vec![seed, -seed, seed + 1, -(seed + 1)];
        p.data_length = p.csi_raw_data.len() as u32;
        p
    }

    fn roundtrip(format: ExportFormat) -> Vec<CsiData> {
        let _guard = CWD_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join(format!("csi-gui-rs-test-{:?}", format));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(&tmp).unwrap();
        let pkts: Vec<CsiData> = (1..=3).map(sample_packet).collect();
        let written = export_history("roundtrip", &pkts, format).unwrap();
        let out = import_history(&written).unwrap();
        std::env::set_current_dir(&prev).unwrap();
        out
    }

    #[test]
    fn csv_roundtrip_preserves_packets() {
        let out = roundtrip(ExportFormat::Csv);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].csi_raw_data, vec![1, -1, 2, -2]);
        assert_eq!(out[2].rssi, -53);
    }

    #[test]
    fn json_roundtrip_preserves_packets() {
        let out = roundtrip(ExportFormat::Json);
        assert_eq!(out.len(), 3);
        assert_eq!(out[1].csi_raw_data, vec![2, -2, 3, -3]);
        assert_eq!(out[1].mac, sample_packet(2).mac);
    }

    #[test]
    fn parquet_roundtrip_preserves_packets() {
        let out = roundtrip(ExportFormat::Parquet);
        assert_eq!(out.len(), 3);
        assert_eq!(out[2].csi_raw_data, vec![3, -3, 4, -4]);
        assert_eq!(out[0].timestamp, 1000);
    }
}

/// Parse a Parquet file produced by [`export_history_parquet`].
pub fn import_history_parquet(path: &Path) -> std::io::Result<Vec<crate::data::CsiData>> {
    use parquet::file::reader::{FileReader, SerializedFileReader};
    use parquet::record::Field;

    let file = fs::File::open(path)?;
    let reader = SerializedFileReader::new(file)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let iter = reader
        .get_row_iter(None)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let mut out = Vec::new();
    for row in iter {
        let row = match row {
            Ok(r) => r,
            Err(_) => continue,
        };
        let mut p = crate::data::CsiData::default();
        for (name, field) in row.get_column_iter() {
            match (name.as_str(), field) {
                ("mac", Field::Str(s)) => p.mac = s.clone(),
                ("rssi", Field::Int(v)) => p.rssi = *v,
                ("noise_floor", Field::Int(v)) => p.noise_floor = *v,
                ("rate", Field::Long(v)) => p.rate = *v as u32,
                ("channel", Field::Long(v)) => p.channel = *v as u32,
                ("secondary_channel", Field::Long(v)) => p.secondary_channel = *v as u32,
                ("timestamp", Field::Long(v)) => p.timestamp = *v as u64,
                ("sig_len", Field::Long(v)) => p.sig_len = *v as u32,
                ("rx_state", Field::Long(v)) => p.rx_state = *v as u32,
                ("sig_mode", Field::Long(v)) => p.sig_mode = *v as u32,
                ("mcs", Field::Long(v)) => p.mcs = *v as u32,
                ("cwb", Field::Long(v)) => p.cwb = *v as u32,
                ("smoothing", Field::Long(v)) => p.smoothing = *v as u32,
                ("not_sounding", Field::Long(v)) => p.not_sounding = *v as u32,
                ("aggregation", Field::Long(v)) => p.aggregation = *v as u32,
                ("stbc", Field::Long(v)) => p.stbc = *v as u32,
                ("fec_coding", Field::Long(v)) => p.fec_coding = *v as u32,
                ("sgi", Field::Long(v)) => p.sgi = *v as u32,
                ("ant", Field::Long(v)) => p.ant = *v as u32,
                ("ampdu_cnt", Field::Long(v)) => p.ampdu_cnt = *v as u32,
                ("data_length", Field::Long(v)) => p.data_length = *v as u32,
                ("csi_raw_data", Field::Str(s)) => p.csi_raw_data = parse_csi_field(s),
                _ => {}
            }
        }
        p.data_length = p.data_length.max(p.csi_raw_data.len() as u32);
        out.push(p);
    }
    Ok(out)
}
