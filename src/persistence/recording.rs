//! Live session recorder — streams post-preprocess packets to JSONL on disk
//! so the in-memory history can stay a bounded tick window.

use crate::data::CsiData;
use crossbeam_channel::{bounded, Receiver, Sender, TrySendError};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

const CHANNEL_CAP: usize = 64;
const FLUSH_EVERY: usize = 256;

enum RecorderMsg {
    Batch(Vec<CsiData>),
    Stop,
}

/// Result of a completed recording session.
#[derive(Clone, Debug)]
pub struct RecordingSummary {
    pub path: PathBuf,
    pub packets_written: u64,
}

/// Background JSONL writer tied to Start/Stop collection.
#[derive(Default)]
pub struct SessionRecorder {
    sender: Option<Sender<RecorderMsg>>,
    handle: Option<JoinHandle<std::io::Result<RecordingSummary>>>,
    path: Option<PathBuf>,
    counter: Arc<AtomicU64>,
    last_recording: Option<RecordingSummary>,
    last_error: Option<String>,
}

impl SessionRecorder {
    pub fn is_recording(&self) -> bool {
        self.sender.is_some()
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn packets_written(&self) -> u64 {
        self.counter.load(Ordering::Relaxed)
    }

    pub fn last_recording(&self) -> Option<&RecordingSummary> {
        self.last_recording.as_ref()
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// Open `exports/<label>_<unix>.jsonl` and spawn the writer thread.
    pub fn start(&mut self, label: &str) -> std::io::Result<PathBuf> {
        self.stop();
        self.last_error = None;
        let path = recording_path(label)?;
        let (tx, rx) = bounded(CHANNEL_CAP);
        let counter = Arc::new(AtomicU64::new(0));
        let counter_worker = counter.clone();
        let path_worker = path.clone();
        let handle = thread::Builder::new()
            .name("csi-jsonl-recorder".into())
            .spawn(move || writer_loop(rx, path_worker, counter_worker))?;
        self.sender = Some(tx);
        self.handle = Some(handle);
        self.path = Some(path.clone());
        self.counter = counter;
        Ok(path)
    }

    /// Queue a batch for the background writer (non-blocking; drops on back-pressure).
    pub fn append_batch(&mut self, packets: &[CsiData]) {
        if packets.is_empty() {
            return;
        }
        let Some(tx) = &self.sender else {
            return;
        };
        match tx.try_send(RecorderMsg::Batch(packets.to_vec())) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                log::warn!("session recorder channel full — dropping batch of {} packets", packets.len());
            }
            Err(TrySendError::Disconnected(_)) => {
                self.sender = None;
            }
        }
    }

    /// Flush and close the active recording. Returns the final summary.
    pub fn stop(&mut self) -> Option<RecordingSummary> {
        if let Some(tx) = self.sender.take() {
            let _ = tx.send(RecorderMsg::Stop);
        }
        let handle = self.handle.take()?;
        self.path = None;
        match handle.join() {
            Ok(Ok(summary)) => {
                self.last_recording = Some(summary.clone());
                self.last_error = None;
                Some(summary)
            }
            Ok(Err(e)) => {
                let msg = e.to_string();
                log::error!("session recorder failed: {msg}");
                self.last_error = Some(msg);
                None
            }
            Err(_) => {
                self.last_error = Some("recorder thread panicked".into());
                None
            }
        }
    }
}

fn sanitize_label(label: &str) -> String {
    let mut out = String::new();
    for c in label.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "session".to_string()
    } else {
        out
    }
}

fn recording_path(label: &str) -> std::io::Result<PathBuf> {
    let dir = PathBuf::from("exports");
    fs::create_dir_all(&dir)?;
    let unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Ok(dir.join(format!("{}_{unix}.jsonl", sanitize_label(label))))
}

fn writer_loop(
    rx: Receiver<RecorderMsg>,
    path: PathBuf,
    counter: Arc<AtomicU64>,
) -> std::io::Result<RecordingSummary> {
    let file = File::create(&path)?;
    let mut w = BufWriter::new(file);
    let mut packets_written = 0u64;
    let mut since_flush = 0usize;

    loop {
        match rx.recv() {
            Ok(RecorderMsg::Batch(batch)) => {
                for p in &batch {
                    let line = serde_json::to_string(p).map_err(std::io::Error::other)?;
                    w.write_all(line.as_bytes())?;
                    w.write_all(b"\n")?;
                    packets_written += 1;
                    since_flush += 1;
                    if since_flush >= FLUSH_EVERY {
                        w.flush()?;
                        since_flush = 0;
                    }
                }
                counter.store(packets_written, Ordering::Relaxed);
            }
            Ok(RecorderMsg::Stop) => break,
            Err(_) => break,
        }
    }
    w.flush()?;
    counter.store(packets_written, Ordering::Relaxed);
    Ok(RecordingSummary {
        path,
        packets_written,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::import_history;

    fn sample_packet(seed: i32) -> CsiData {
        let mut p = CsiData::default();
        p.mac = format!("AA:BB:CC:DD:EE:{:02X}", seed.unsigned_abs() & 0xFF);
        p.rssi = -50 - seed;
        p.csi_raw_data = vec![seed, -seed, seed + 1, -(seed + 1)];
        p.data_length = p.csi_raw_data.len() as u32;
        p
    }

    #[test]
    fn recorder_roundtrip_via_import() {
        let _guard = crate::persistence::TEST_CWD_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join("csi-gui-rs-recorder-test");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(&tmp).unwrap();

        let mut rec = SessionRecorder::default();
        let rel = rec.start("test_port").unwrap();
        assert_eq!(rel.extension().and_then(|s| s.to_str()), Some("jsonl"));
        let abs = tmp.join(&rel);
        rec.append_batch(&[sample_packet(1), sample_packet(2)]);
        let summary = rec.stop().unwrap();
        assert_eq!(summary.packets_written, 2);

        let imported = import_history(&abs).unwrap();
        assert_eq!(imported.len(), 2);
        assert_eq!(imported[0].csi_raw_data, vec![1, -1, 2, -2]);

        std::env::set_current_dir(&prev).unwrap();
    }
}
