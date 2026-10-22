//! Rolling raw-UART text buffer shared between the reader and the
//! Console view.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Instant;

/// Rolling capture of raw text lines the reader has seen on the serial
/// port. Used by the Console view so users can inspect command responses
/// (`show-config`, `show-stats`, `info`, …) and boot/banner output that
/// the CSI parser would otherwise discard.
///
/// `RawConsole` is internally `Mutex`-protected and `Send + Sync`; wrap it
/// in an `Arc` and share between threads.
///
/// # Example
///
/// ```
/// use std::sync::Arc;
/// use csi_gui_rs::console::RawConsole;
///
/// let console = Arc::new(RawConsole::new(1024));
/// console.push("hello");
/// console.push("world");
/// let lines = console.snapshot(None);
/// assert_eq!(lines.len(), 2);
/// assert_eq!(lines[1].1, "world");
/// ```
pub struct RawConsole {
    inner: Mutex<Inner>,
}

struct Inner {
    lines: VecDeque<Line>,
    max: usize,
    started: Instant,
}

pub struct Line {
    pub ms: u128,
    pub text: String,
}

impl RawConsole {
    /// Construct a buffer that retains at most `max` lines. Older lines are
    /// dropped from the front when the cap is reached.
    pub fn new(max: usize) -> Self {
        Self {
            inner: Mutex::new(Inner {
                lines: VecDeque::with_capacity(max),
                max,
                started: Instant::now(),
            }),
        }
    }

    /// Append one line. Trailing CR/LF are stripped, empty lines ignored.
    /// The line is timestamped with the elapsed time since this buffer was
    /// created.
    pub fn push(&self, text: impl Into<String>) {
        let mut g = self.inner.lock().expect("RawConsole mutex poisoned");
        let ms = g.started.elapsed().as_millis();
        if g.lines.len() == g.max {
            g.lines.pop_front();
        }
        let mut text = text.into();
        // Strip trailing CR/LF for display.
        while matches!(text.chars().last(), Some('\n') | Some('\r')) {
            text.pop();
        }
        if text.is_empty() {
            return;
        }
        g.lines.push_back(Line { ms, text });
    }

    /// Drop all retained lines.
    pub fn clear(&self) {
        self.inner
            .lock()
            .expect("RawConsole mutex poisoned")
            .lines
            .clear();
    }

    /// Copy the most recent `n` lines (or all if `n` is `None`). Returned
    /// in chronological order (oldest first). Each tuple is
    /// `(elapsed_ms_since_buffer_start, text)`.
    pub fn snapshot(&self, n: Option<usize>) -> Vec<(u128, String)> {
        let g = self.inner.lock().expect("RawConsole mutex poisoned");
        let count = n.map_or(g.lines.len(), |k| k.min(g.lines.len()));
        let start = g.lines.len() - count;
        g.lines
            .iter()
            .skip(start)
            .map(|l| (l.ms, l.text.clone()))
            .collect()
    }

    /// Current number of retained lines.
    pub fn len(&self) -> usize {
        self.inner
            .lock()
            .expect("RawConsole mutex poisoned")
            .lines
            .len()
    }

    /// `true` if no lines have been retained.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
