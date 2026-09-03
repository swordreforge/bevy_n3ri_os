//! External music playlist data types.
//!
//! Pure data — no audio/rendering deps. The playback side (examples/minimal)
//! consumes [`MusicCommand`] and writes back [`MusicLibrary`] / [`MusicStatus`].

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Playback mode for the external music library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PlayMode {
    #[default]
    Sequential,
    Shuffle,
    SingleLoop,
}

impl PlayMode {
    pub fn from_u8(value: u8) -> Self {
        match value {
            1 => PlayMode::Shuffle,
            2 => PlayMode::SingleLoop,
            _ => PlayMode::Sequential,
        }
    }

    pub fn as_u8(self) -> u8 {
        match self {
            PlayMode::Sequential => 0,
            PlayMode::Shuffle => 1,
            PlayMode::SingleLoop => 2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            PlayMode::Sequential => "顺序播放",
            PlayMode::Shuffle => "随机播放",
            PlayMode::SingleLoop => "单曲循环",
        }
    }
}

/// A single scanned track with parsed tags (fallback: filename as title).
#[derive(Debug, Clone)]
pub struct MusicTrack {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
}

/// Result of a directory scan, consumed by the settings UI to list tracks.
#[derive(Resource, Debug, Clone, Default)]
pub struct MusicLibrary(pub Vec<MusicTrack>);

impl MusicLibrary {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Live playback state, written by the minimal playback systems.
#[derive(Resource, Debug, Clone)]
pub struct MusicStatus {
    pub mode: PlayMode,
    pub current: Option<usize>,
    pub playing: bool,
}

impl Default for MusicStatus {
    fn default() -> Self {
        Self {
            mode: PlayMode::Sequential,
            current: None,
            playing: false,
        }
    }
}

/// UI → playback commands.
#[derive(Message, Clone)]
pub enum MusicCommand {
    Scan(PathBuf),
    Play(usize),
    Next,
    Prev,
    SetMode(PlayMode),
}
