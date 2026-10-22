//! Where the app keeps its files.
//!
//! Everything lives in the platform's standard directories rather than
//! the working directory, so launching the binary from anywhere (a
//! desktop launcher, `cargo run`, a USB stick) finds the same state:
//!
//! | What | Linux | macOS | Windows |
//! |---|---|---|---|
//! | config: templates, `devices.json` | `~/.config/csi-gui-rs` | `~/Library/Application Support/io.csi-rs.csi-gui-rs` | `%APPDATA%\csi-rs\csi-gui-rs\config` |
//! | data: exports, recordings, `.rrd` | `~/.local/share/csi-gui-rs` | same as config | `%APPDATA%\csi-rs\csi-gui-rs\data` |
//!
//! `--data-dir` (see [`set_data_dir_override`]) moves the data side, for
//! example onto a bigger disk. If the platform reports no home directory
//! at all, both fall back to the working directory.

use std::path::PathBuf;
use std::sync::OnceLock;

static DATA_OVERRIDE: OnceLock<PathBuf> = OnceLock::new();
static CONFIG_OVERRIDE: OnceLock<PathBuf> = OnceLock::new();

fn project_dirs() -> Option<directories::ProjectDirs> {
    directories::ProjectDirs::from("io", "csi-rs", "csi-gui-rs")
}

/// Redirect [`data_dir`] for the rest of the process. Only the first
/// call wins; call it once, from `main`, before anything writes.
pub fn set_data_dir_override(dir: PathBuf) {
    let _ = DATA_OVERRIDE.set(dir);
}

/// Redirect [`config_dir`] for the rest of the process. Meant for tests
/// and portable installs; only the first call wins.
pub fn set_config_dir_override(dir: PathBuf) {
    let _ = CONFIG_OVERRIDE.set(dir);
}

/// Directory for user configuration: templates and saved device configs.
pub fn config_dir() -> PathBuf {
    if let Some(d) = CONFIG_OVERRIDE.get() {
        return d.clone();
    }
    project_dirs()
        .map(|p| p.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Directory for captured data: exports, recordings and Rerun files.
pub fn data_dir() -> PathBuf {
    if let Some(d) = DATA_OVERRIDE.get() {
        return d.clone();
    }
    project_dirs()
        .map(|p| p.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Saved layout templates.
pub fn templates_dir() -> PathBuf {
    config_dir().join("templates")
}

/// CSV / JSONL / Parquet exports and live JSONL recordings.
pub fn exports_dir() -> PathBuf {
    data_dir().join("exports")
}

/// Rerun `.rrd` recordings.
pub fn rerun_dir() -> PathBuf {
    data_dir().join("rrd")
}

/// Per-device saved configuration (see [`crate::device::store`]).
pub fn devices_file() -> PathBuf {
    config_dir().join("devices.json")
}

/// Seconds since the Unix epoch, for file names. `0` if the clock is
/// before 1970.
pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Microseconds since the Unix epoch. `0` if the clock is before 1970.
pub fn unix_now_us() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0)
}

/// Replace characters that are awkward in file names (path separators,
/// `:` from MAC addresses and `/dev/` paths, spaces) with `-`.
pub fn sanitize_file_stem(s: &str) -> String {
    let out: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "capture".to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_ports_and_macs() {
        assert_eq!(sanitize_file_stem("/dev/ttyACM0"), "dev-ttyACM0");
        assert_eq!(sanitize_file_stem("A0:85:E3:F5:48:90"), "A0-85-E3-F5-48-90");
        assert_eq!(sanitize_file_stem("COM3"), "COM3");
        assert_eq!(sanitize_file_stem("///"), "capture");
    }
}
