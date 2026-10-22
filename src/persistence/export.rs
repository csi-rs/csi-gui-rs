//! CSI capture files: CSV, JSONL and Parquet.
//!
//! The three formats share one column table, [`COLUMNS`], so a capture
//! reads the same whichever format it was written in. CSV and Parquet
//! importers look columns up **by name**, which is what lets files
//! written by 1.x (no `device` / `session_id` / … columns) keep loading:
//! a missing column is left at its default.

use crate::data::CsiData;
use std::fs;
use std::path::{Path, PathBuf};

/// How a column is stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// UTF-8 text (Parquet `BYTE_ARRAY (UTF8)`).
    Str,
    /// Integer (Parquet `INT64`).
    Int,
}

/// One export column: its name, storage kind, and how to read it from /
/// write it into a [`CsiData`].
struct Column {
    name: &'static str,
    kind: Kind,
    get: fn(&CsiData) -> Value,
    set: fn(&mut CsiData, Value),
}

/// A cell value in transit between [`CsiData`] and a file.
enum Value {
    S(String),
    I(i64),
}

impl Value {
    fn int(self) -> i64 {
        match self {
            Value::I(v) => v,
            Value::S(s) => s.trim().parse().unwrap_or(0),
        }
    }

    fn string(self) -> String {
        match self {
            Value::S(s) => s,
            Value::I(v) => v.to_string(),
        }
    }
}

macro_rules! int_col {
    ($name:literal, $field:ident, $ty:ty) => {
        Column {
            name: $name,
            kind: Kind::Int,
            get: |p| Value::I(p.$field as i64),
            set: |p, v| p.$field = v.int() as $ty,
        }
    };
}

macro_rules! str_col {
    ($name:literal, $field:ident) => {
        Column {
            name: $name,
            kind: Kind::Str,
            get: |p| Value::S(p.$field.clone()),
            set: |p, v| p.$field = v.string(),
        }
    };
}

/// Canonical column order, emitted for **every** packet whatever wire
/// format the firmware used. `csi_raw_data` is space-separated decimal
/// integers wrapped in `[…]`, interleaved as the firmware sent them.
const COLUMNS: &[Column] = &[
    str_col!("device", device),
    int_col!("host_time_us", host_time_us, u64),
    str_col!("node_id", node_id),
    int_col!("session_id", session_id, u32),
    int_col!("stream_seq", stream_seq, u32),
    int_col!("sequence_number", sequence_number, u32),
    str_col!("mac", mac),
    int_col!("rssi", rssi, i32),
    int_col!("rate", rate, u32),
    int_col!("noise_floor", noise_floor, i32),
    int_col!("channel", channel, u32),
    int_col!("secondary_channel", secondary_channel, u32),
    int_col!("bandwidth_mhz", bandwidth_mhz, u32),
    str_col!("ppdu", ppdu),
    int_col!("timestamp", timestamp, u64),
    int_col!("sig_len", sig_len, u32),
    int_col!("rx_state", rx_state, u32),
    int_col!("sig_mode", sig_mode, u32),
    int_col!("mcs", mcs, u32),
    int_col!("cwb", cwb, u32),
    int_col!("smoothing", smoothing, u32),
    int_col!("not_sounding", not_sounding, u32),
    int_col!("aggregation", aggregation, u32),
    int_col!("stbc", stbc, u32),
    int_col!("fec_coding", fec_coding, u32),
    int_col!("sgi", sgi, u32),
    int_col!("ant", ant, u32),
    int_col!("ampdu_cnt", ampdu_cnt, u32),
    int_col!("data_length", data_length, u32),
    Column {
        name: "subcarriers",
        kind: Kind::Int,
        get: |p| Value::I(p.subcarriers() as i64),
        // Derived from `csi_raw_data`; nothing to restore.
        set: |_, _| {},
    },
    Column {
        name: "csi_raw_data",
        kind: Kind::Str,
        get: |p| Value::S(format!("[{}]", csi_join(&p.csi_raw_data))),
        set: |p, v| p.csi_raw_data = parse_csi_field(&v.string()),
    },
];

/// Column names in file order, as written in the CSV header.
pub fn csv_columns() -> impl Iterator<Item = &'static str> {
    COLUMNS.iter().map(|c| c.name)
}

/// Output format the export pipeline writes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExportFormat {
    /// Plain CSV, one row per packet (see [`csv_columns`]).
    #[default]
    Csv,
    /// Newline-delimited JSON (one [`CsiData`] per line).
    Json,
    /// Apache Parquet with snappy compression. Same columns as the CSV;
    /// `csi_raw_data` is a space-separated UTF-8 string.
    Parquet,
}

impl ExportFormat {
    /// Every format, in menu order.
    pub const ALL: [ExportFormat; 3] = [Self::Csv, Self::Json, Self::Parquet];

    /// File extension (no leading dot) for the format.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Json => "jsonl",
            Self::Parquet => "parquet",
        }
    }

    /// Short label for menus.
    pub fn label(self) -> &'static str {
        match self {
            Self::Csv => "CSV",
            Self::Json => "JSONL",
            Self::Parquet => "Parquet",
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

/// Write `packets` to `<dir>/<stem>_<unix>.<ext>` in `format`, creating
/// `dir` if needed. Returns the path written.
///
/// # Errors
///
/// The underlying I/O error if the directory or file cannot be written.
pub fn export_packets(
    dir: &Path,
    stem: &str,
    packets: &[CsiData],
    format: ExportFormat,
) -> std::io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let path = dir.join(format!(
        "{}_{}.{}",
        crate::paths::sanitize_file_stem(stem),
        crate::paths::unix_now(),
        format.extension()
    ));
    write_packets(&path, packets, format)?;
    Ok(path)
}

/// Write `packets` to exactly `path` in `format`.
///
/// # Errors
///
/// The underlying I/O error if the file cannot be written.
pub fn write_packets(path: &Path, packets: &[CsiData], format: ExportFormat) -> std::io::Result<()> {
    match format {
        ExportFormat::Csv => write_csv(path, packets),
        ExportFormat::Json => write_jsonl(path, packets),
        ExportFormat::Parquet => write_parquet(path, packets),
    }
}

fn write_csv(path: &Path, packets: &[CsiData]) -> std::io::Result<()> {
    let mut wtr = csv::Writer::from_path(path)?;
    wtr.write_record(csv_columns())?;
    for p in packets {
        wtr.write_record(COLUMNS.iter().map(|c| (c.get)(p).string()))?;
    }
    wtr.flush()?;
    Ok(())
}

fn write_jsonl(path: &Path, packets: &[CsiData]) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::io::BufWriter::new(fs::File::create(path)?);
    for p in packets {
        serde_json::to_writer(&mut f, p).map_err(std::io::Error::other)?;
        f.write_all(b"\n")?;
    }
    f.flush()
}

fn parquet_schema() -> String {
    let mut s = String::from("message csi_packet {\n");
    for c in COLUMNS {
        match c.kind {
            Kind::Str => s.push_str(&format!("    required binary {} (UTF8);\n", c.name)),
            Kind::Int => s.push_str(&format!("    required int64 {};\n", c.name)),
        }
    }
    s.push('}');
    s
}

fn write_parquet(path: &Path, packets: &[CsiData]) -> std::io::Result<()> {
    use parquet::basic::Compression;
    use parquet::data_type::{ByteArray, ByteArrayType, Int64Type};
    use parquet::file::properties::WriterProperties;
    use parquet::file::writer::SerializedFileWriter;
    use parquet::schema::parser::parse_message_type;
    use std::sync::Arc;

    let schema = parse_message_type(&parquet_schema()).map_err(std::io::Error::other)?;
    let props = Arc::new(
        WriterProperties::builder()
            .set_compression(Compression::SNAPPY)
            .build(),
    );
    let file = fs::File::create(path)?;
    let mut writer =
        SerializedFileWriter::new(file, Arc::new(schema), props).map_err(std::io::Error::other)?;
    let mut rg = writer.next_row_group().map_err(std::io::Error::other)?;
    for c in COLUMNS {
        let Some(mut col) = rg.next_column().map_err(std::io::Error::other)? else {
            return Err(std::io::Error::other("parquet writer ran out of columns"));
        };
        match c.kind {
            Kind::Str => {
                let vals: Vec<ByteArray> = packets
                    .iter()
                    .map(|p| ByteArray::from((c.get)(p).string().as_str()))
                    .collect();
                col.typed::<ByteArrayType>()
                    .write_batch(&vals, None, None)
                    .map_err(std::io::Error::other)?;
            }
            Kind::Int => {
                let vals: Vec<i64> = packets.iter().map(|p| (c.get)(p).int()).collect();
                col.typed::<Int64Type>()
                    .write_batch(&vals, None, None)
                    .map_err(std::io::Error::other)?;
            }
        }
        col.close().map_err(std::io::Error::other)?;
    }
    rg.close().map_err(std::io::Error::other)?;
    writer.close().map_err(std::io::Error::other)?;
    Ok(())
}

/// Read packets back out of a file produced by [`export_packets`] (or by
/// csi-gui-rs 1.x). The format is detected from the extension.
///
/// # Errors
///
/// The underlying I/O error, or `InvalidData` for a malformed Parquet file.
pub fn import_packets(path: &Path) -> std::io::Result<Vec<CsiData>> {
    let mut out = match ExportFormat::from_path(path) {
        ExportFormat::Csv => read_csv(path)?,
        ExportFormat::Json => read_jsonl(path)?,
        ExportFormat::Parquet => read_parquet(path)?,
    };
    for p in &mut out {
        p.data_length = p.data_length.max(p.csi_raw_data.len() as u32);
    }
    Ok(out)
}

fn column(name: &str) -> Option<&'static Column> {
    COLUMNS.iter().find(|c| c.name == name)
}

fn read_csv(path: &Path) -> std::io::Result<Vec<CsiData>> {
    let mut rdr = csv::Reader::from_path(path)?;
    let cols: Vec<Option<&Column>> = rdr.headers()?.iter().map(column).collect();
    let mut out = Vec::new();
    for rec in rdr.records() {
        let rec = rec?;
        let mut p = CsiData::default();
        for (cell, col) in rec.iter().zip(&cols) {
            if let Some(col) = col {
                (col.set)(&mut p, Value::S(cell.to_string()));
            }
        }
        out.push(p);
    }
    Ok(out)
}

fn read_jsonl(path: &Path) -> std::io::Result<Vec<CsiData>> {
    let s = fs::read_to_string(path)?;
    Ok(s.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect())
}

fn read_parquet(path: &Path) -> std::io::Result<Vec<CsiData>> {
    use parquet::file::reader::{FileReader, SerializedFileReader};
    use parquet::record::Field;

    let invalid = |e| std::io::Error::new(std::io::ErrorKind::InvalidData, e);
    let reader = SerializedFileReader::new(fs::File::open(path)?).map_err(invalid)?;
    let mut out = Vec::new();
    for row in reader.get_row_iter(None).map_err(invalid)? {
        let Ok(row) = row else { continue };
        let mut p = CsiData::default();
        for (name, field) in row.get_column_iter() {
            let Some(col) = column(name) else { continue };
            // 1.x wrote rssi / noise_floor as INT32.
            let v = match field {
                Field::Str(s) => Value::S(s.clone()),
                Field::Long(v) => Value::I(*v),
                Field::Int(v) => Value::I(*v as i64),
                _ => continue,
            };
            (col.set)(&mut p, v);
        }
        out.push(p);
    }
    Ok(out)
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

fn parse_csi_field(s: &str) -> Vec<i32> {
    s.trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|t| !t.is_empty())
        .filter_map(|t| t.parse::<i32>().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_packet(seed: i32) -> CsiData {
        CsiData {
            device: "A0:85:E3:F5:48:90".into(),
            session_id: 0xC0FFEE,
            stream_seq: seed as u32,
            ppdu: "Ht".into(),
            bandwidth_mhz: 20,
            mac: format!("AA:BB:CC:DD:EE:{:02X}", seed.unsigned_abs() & 0xFF),
            rssi: -50 - seed,
            rate: 11,
            noise_floor: -90,
            channel: 6,
            timestamp: seed as u64 * 1000,
            sig_len: 128,
            csi_raw_data: vec![seed, -seed, seed + 1, -(seed + 1)],
            data_length: 4,
            ..CsiData::default()
        }
    }

    fn roundtrip(format: ExportFormat) -> Vec<CsiData> {
        let dir = std::env::temp_dir().join(format!(
            "csi-gui-rs-export-{format:?}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        let pkts: Vec<CsiData> = (1..=3).map(sample_packet).collect();
        let written = export_packets(&dir, "/dev/ttyACM0", &pkts, format).unwrap();
        assert!(written.file_name().unwrap().to_str().unwrap().starts_with("dev-ttyACM0_"));
        let out = import_packets(&written).unwrap();
        let _ = fs::remove_dir_all(&dir);
        out
    }

    fn check(out: &[CsiData]) {
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].csi_raw_data, vec![1, -1, 2, -2]);
        assert_eq!(out[2].rssi, -53);
        assert_eq!(out[1].mac, sample_packet(2).mac);
        assert_eq!(out[0].timestamp, 1000);
        assert_eq!(out[0].device, "A0:85:E3:F5:48:90");
        assert_eq!(out[2].session_id, 0xC0FFEE);
        assert_eq!(out[2].stream_seq, 3);
        assert_eq!(out[0].ppdu, "Ht");
    }

    #[test]
    fn csv_roundtrip() {
        check(&roundtrip(ExportFormat::Csv));
    }

    #[test]
    fn json_roundtrip() {
        check(&roundtrip(ExportFormat::Json));
    }

    #[test]
    fn parquet_roundtrip() {
        check(&roundtrip(ExportFormat::Parquet));
    }

    /// A CSV with the 1.x header (no device / session columns) still
    /// imports; the new fields come back empty.
    #[test]
    fn imports_v1_csv() {
        let dir = std::env::temp_dir().join(format!("csi-gui-rs-v1csv-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("old.csv");
        fs::write(
            &path,
            "mac,rssi,rate,noise_floor,channel,secondary_channel,timestamp,sig_len,rx_state,\
             sig_mode,mcs,cwb,smoothing,not_sounding,aggregation,stbc,fec_coding,sgi,ant,\
             ampdu_cnt,data_length,subcarriers,csi_raw_data\n\
             AA:BB:CC:DD:EE:01,-51,11,-90,6,0,1000,128,0,0,0,0,0,0,0,0,0,0,0,0,4,2,[1 -1 2 -2]\n",
        )
        .unwrap();
        let out = import_packets(&path).unwrap();
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].rssi, -51);
        assert_eq!(out[0].csi_raw_data, vec![1, -1, 2, -2]);
        assert!(out[0].device.is_empty());
    }
}
