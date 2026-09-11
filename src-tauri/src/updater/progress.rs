//! io::Write wrapper that counts bytes and emits throttled
//! `updater-progress` events (at most ~10 Hz) to the frontend.

use std::io::Write;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};

#[derive(serde::Serialize, Clone)]
struct ProgressPayload {
    downloaded: u64,
}

pub(super) struct ProgressWriter<W: Write> {
    inner: W,
    app: AppHandle,
    downloaded: u64,
    last_emit: Instant,
}

impl<W: Write> ProgressWriter<W> {
    pub(super) fn new(inner: W, app: AppHandle) -> Self {
        Self {
            inner,
            app,
            downloaded: 0,
            // Force the first write to emit immediately.
            last_emit: Instant::now() - Duration::from_secs(1),
        }
    }

    fn emit(&mut self, force: bool) {
        if !force && self.last_emit.elapsed() < Duration::from_millis(100) {
            return;
        }
        self.last_emit = Instant::now();
        let _ = self.app.emit(
            "updater-progress",
            ProgressPayload {
                downloaded: self.downloaded,
            },
        );
    }
}

impl<W: Write> Write for ProgressWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.downloaded = self.downloaded.saturating_add(n as u64);
        self.emit(false);
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()?;
        self.emit(true);
        Ok(())
    }
}
