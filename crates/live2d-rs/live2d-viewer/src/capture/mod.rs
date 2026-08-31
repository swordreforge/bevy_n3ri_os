//! Wayland screen capture via wlr-screencopy-unstable-v1 protocol.
//!
//! Feature-gated behind `capture` (off by default).

mod frame;
mod wlr_screencopy;

pub use frame::{CapturedFrame, FrameSender};

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use anyhow::Result;

/// Manages the lifetime of the capture thread.
pub struct CaptureSession {
    stop_flag: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl CaptureSession {
    /// Spawn the capture thread.
    pub fn start(tx: FrameSender) -> Result<Self> {
        let stop_flag = Arc::new(AtomicBool::new(false));
        let flag = stop_flag.clone();

        let handle = thread::Builder::new()
            .name("capture".into())
            .spawn(move || {
                if let Err(e) = wlr_screencopy::run(tx, flag) {
                    log::error!("Capture thread failed: {e:#}");
                }
            })
            .expect("spawn capture thread");

        Ok(Self {
            stop_flag,
            handle: Some(handle),
        })
    }

    /// Signal the capture thread to stop and wait for it.
    pub fn stop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for CaptureSession {
    fn drop(&mut self) {
        self.stop();
    }
}
