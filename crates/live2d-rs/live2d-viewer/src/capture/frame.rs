use std::sync::mpsc;

/// A single captured video frame in RGBA format.
pub struct CapturedFrame {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Channel sender for delivering captured frames from the capture thread.
pub type FrameSender = mpsc::Sender<CapturedFrame>;
