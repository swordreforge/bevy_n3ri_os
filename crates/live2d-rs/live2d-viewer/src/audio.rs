use std::path::Path;
use std::sync::Mutex;

pub struct AudioPlayer {
    sink: Mutex<rodio::Sink>,
    _stream: rodio::OutputStream,
}

impl AudioPlayer {
    pub fn new() -> anyhow::Result<Self> {
        let (stream, stream_handle) = rodio::OutputStream::try_default()?;
        let sink = rodio::Sink::try_new(&stream_handle)?;
        Ok(Self {
            sink: Mutex::new(sink),
            _stream: stream,
        })
    }

    /// Play a sound file (OGG/WAV/MP3). If a sound is already playing, it
    /// queues without interrupting the current one (rodio default behavior).
    pub fn play(&self, path: &Path) {
        if !path.exists() {
            log::warn!("[audio] sound file not found: {:?}", path);
            return;
        }
        match std::fs::File::open(path) {
            Ok(file) => match rodio::Decoder::new(std::io::BufReader::new(file)) {
                Ok(source) => {
                    if let Ok(sink) = self.sink.lock() {
                        sink.append(source);
                        log::info!("[audio] playing: {:?}", path);
                    }
                }
                Err(e) => log::warn!("[audio] decoder error for {:?}: {}", path, e),
            },
            Err(e) => log::warn!("[audio] open error for {:?}: {}", path, e),
        }
    }
}
