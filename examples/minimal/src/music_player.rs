use bevy::audio::{AudioSink, AudioSource, GlobalVolume, PlaybackSettings, Volume};
use bevy::prelude::*;
use lofty::prelude::{Accessor, TaggedFileExt};
use n3ri_core::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const MAX_SCAN_DEPTH: u8 = 3;
const AUDIO_EXTS: [&str; 7] = ["mp3", "flac", "wav", "ogg", "m4a", "mp4", "aac"];
const FALLBACK_BGM: &str = "nori/audio/bgm1.ogg";

/// Marker for the single "now playing" audio entity (external track or fallback BGM).
#[derive(Component)]
pub struct NowPlaying;

/// Which library index is playing (None = internal fallback BGM).
#[derive(Component)]
pub struct PlayingTrack(pub Option<usize>);

/// Consecutive frames the sink reported empty before auto-advancing, so a freshly
/// started sink (queue not yet populated) is not mistaken for a finished track.
#[derive(Resource, Default)]
struct AutoplayGuard(u32);

pub struct MusicPlayerPlugin;

impl Plugin for MusicPlayerPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(AutoplayGuard::default());
        app.add_systems(OnEnter(OsState::Desktop), start_music)
            .add_systems(
                Update,
                (
                    music_scan,
                    music_control,
                    music_autoplay,
                    music_volume_sync,
                ),
            );
    }
}

/// Desktop entry: restore the persisted playback choice. Library preferred when a
/// dir is configured AND the user did not explicitly choose built-in BGM last
/// session (`music_source != Some(0)`); otherwise play the embedded BGM. The
/// boot-time builtin fallback is transient and never overwrites a stored choice.
fn start_music(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut audio_assets: ResMut<Assets<AudioSource>>,
    now_playing: Query<Entity, With<NowPlaying>>,
    mut library: ResMut<MusicLibrary>,
    mut status: ResMut<MusicStatus>,
    mut settings: ResMut<UserSettings>,
) {
    status.mode = PlayMode::from_u8(settings.music_mode);
    let tracks = settings
        .music_dir
        .as_ref()
        .map(PathBuf::from)
        .map(|d| scan_tracks(&d))
        .unwrap_or_default();
    if !tracks.is_empty() {
        library.0 = tracks;
        if settings.music_source == Some(0) {
            play_fallback(&mut commands, &asset_server, now_playing, &settings);
            status.current = None;
            status.playing = true;
        } else {
            let index = resolve_track_index(&library, settings.music_track_path.as_deref())
                .unwrap_or(0);
            play_track(&mut commands, &mut audio_assets, now_playing, &mut settings, &library, index);
            status.current = Some(index);
            status.playing = true;
        }
    } else {
        library.0.clear();
        play_fallback(&mut commands, &asset_server, now_playing, &settings);
        status.current = None;
        status.playing = true;
    }
}

/// Find the library index whose path matches the persisted playback path.
fn resolve_track_index(library: &MusicLibrary, stored: Option<&str>) -> Option<usize> {
    let stored = PathBuf::from(stored?);
    library.0.iter().position(|t| t.path == stored)
}

/// Persist which source (0 = builtin BGM, 1 = external library) and, for the
/// library, the exact track path so a later startup can resume the same song.
fn persist_source(settings: &mut UserSettings, source: u8, path: Option<&Path>) {
    if settings.music_source != Some(source)
        || settings.music_track_path.as_deref()
            != path.map(|p| p.to_str().unwrap_or_default())
    {
        settings.music_source = Some(source);
        settings.music_track_path = path.map(|p| p.to_string_lossy().into_owned());
        settings.save();
    }
}

/// Recursively scan `root` up to `MAX_SCAN_DEPTH` levels, collecting audio files and
/// reading tags via lofty (title/artist; filename fallback).
fn scan_tracks(root: &Path) -> Vec<MusicTrack> {
    let mut out = Vec::new();
    let mut stack: Vec<(PathBuf, u8)> = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > MAX_SCAN_DEPTH {
            continue;
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push((path, depth + 1));
                continue;
            }
            let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
                continue;
            };
            if !AUDIO_EXTS.contains(&ext.to_ascii_lowercase().as_str()) {
                continue;
            }
            out.push(read_track_tags(&path));
        }
    }
    out.sort_by(|a, b| a.title.cmp(&b.title));
    out
}

fn read_track_tags(path: &Path) -> MusicTrack {
    let mut track = MusicTrack {
        path: path.to_path_buf(),
        title: path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
        artist: String::new(),
    };
    if let Ok(tagged) = lofty::read_from_path(path) {
        if let Some(tag) = tagged.primary_tag() {
            if let Some(t) = tag.title().map(|s| s.into_owned()).filter(|s| !s.is_empty()) {
                track.title = t;
            }
            if let Some(a) = tag.artist().map(|s| s.into_owned()).filter(|s| !s.is_empty()) {
                track.artist = a;
            }
        }
    }
    track
}

/// Read file bytes into an `AudioSource` asset, bypassing AssetServer's root limit.
fn load_external_source(
    audio_assets: &mut Assets<AudioSource>,
    path: &Path,
) -> Option<Handle<AudioSource>> {
    let bytes = fs::read(path).ok()?;
    Some(audio_assets.add(AudioSource {
        bytes: Arc::from(bytes),
    }))
}

fn play_track(
    commands: &mut Commands,
    audio_assets: &mut Assets<AudioSource>,
    now_playing: Query<Entity, With<NowPlaying>>,
    settings: &mut UserSettings,
    library: &MusicLibrary,
    index: usize,
) {
    let Some(track) = library.0.get(index) else {
        return;
    };
    let Some(handle) = load_external_source(audio_assets, &track.path) else {
        return;
    };
    let vol = music_volume(settings);
    despawn_now_playing(commands, now_playing);
    commands.spawn((
        NowPlaying,
        PlayingTrack(Some(index)),
        AudioPlayer::new(handle),
        PlaybackSettings::ONCE.with_volume(Volume::Linear(vol)),
    ));
    persist_source(settings, 1, Some(&track.path));
}

fn play_fallback(
    commands: &mut Commands,
    asset_server: &AssetServer,
    now_playing: Query<Entity, With<NowPlaying>>,
    settings: &UserSettings,
) {
    let vol = music_volume(settings);
    let handle = asset_server.load(FALLBACK_BGM);
    despawn_now_playing(commands, now_playing);
    commands.spawn((
        NowPlaying,
        PlayingTrack(None),
        AudioPlayer::new(handle),
        PlaybackSettings::LOOP.with_volume(Volume::Linear(vol)),
    ));
}

fn despawn_now_playing(commands: &mut Commands, now_playing: Query<Entity, With<NowPlaying>>) {
    let to_kill: Vec<Entity> = now_playing.iter().collect();
    for e in to_kill {
        commands.entity(e).despawn();
    }
}

fn music_volume(settings: &UserSettings) -> f32 {
    if settings.toggles[1] {
        settings.volumes[1] as f32 / 100.0
    } else {
        0.0
    }
}

/// Consume UI `Scan(dir)` commands. An empty scan falls back to builtin BGM
/// (transient — never overwrites a stored source choice) instead of going silent.
fn music_scan(
    mut commands: Commands,
    mut scan: MessageReader<MusicCommand>,
    mut audio_assets: ResMut<Assets<AudioSource>>,
    asset_server: Res<AssetServer>,
    now_playing: Query<Entity, With<NowPlaying>>,
    mut library: ResMut<MusicLibrary>,
    mut status: ResMut<MusicStatus>,
    mut settings: ResMut<UserSettings>,
) {
    for cmd in scan.read() {
        let MusicCommand::Scan(dir) = cmd else {
            continue;
        };
        let tracks = scan_tracks(dir);
        let dir_str = dir.to_string_lossy().into_owned();
        let changed_dir = settings.music_dir.as_deref() != Some(dir_str.as_str());
        settings.music_dir = Some(dir_str);
        settings.save();
        library.0 = tracks;
        if library.is_empty() {
            play_fallback(&mut commands, &asset_server, now_playing, &settings);
            status.current = None;
            status.playing = true;
            continue;
        }
        if changed_dir {
            play_track(&mut commands, &mut audio_assets, now_playing, &mut settings, &library, 0);
            status.current = Some(0);
            status.playing = true;
        }
    }
}

/// Consume UI playback commands (Play/PlayBuiltin/Next/Prev/SetMode).
fn music_control(
    mut commands: Commands,
    mut ctrl: MessageReader<MusicCommand>,
    mut audio_assets: ResMut<Assets<AudioSource>>,
    asset_server: Res<AssetServer>,
    now_playing: Query<Entity, With<NowPlaying>>,
    library: Res<MusicLibrary>,
    mut status: ResMut<MusicStatus>,
    mut settings: ResMut<UserSettings>,
) {
    let mut play_index: Option<usize> = None;
    let mut play_builtin = false;
    for cmd in ctrl.read() {
        match cmd {
            MusicCommand::Scan(_) => {}
            MusicCommand::Play(i) => play_index = Some(*i),
            MusicCommand::PlayBuiltin => play_builtin = true,
            MusicCommand::Next => {
                if !library.is_empty() {
                    let len = library.len();
                    let cur = status.current.unwrap_or(0).min(len - 1);
                    play_index = Some((cur + 1) % len);
                }
            }
            MusicCommand::Prev => {
                if !library.is_empty() {
                    let len = library.len();
                    let cur = status.current.unwrap_or(0).min(len - 1);
                    play_index = Some((cur + len - 1) % len);
                }
            }
            MusicCommand::SetMode(m) => {
                status.mode = *m;
                settings.music_mode = m.as_u8();
                settings.save();
            }
        }
    }
    if play_builtin {
        play_fallback(&mut commands, &asset_server, now_playing, &settings);
        status.current = None;
        status.playing = true;
        persist_source(&mut settings, 0, None);
    } else if let Some(i) = play_index {
        if i < library.len() {
            play_track(&mut commands, &mut audio_assets, now_playing, &mut settings, &library, i);
            status.current = Some(i);
            status.playing = true;
        }
    }
}

/// Detect track end via `AudioSink.empty()` and auto-advance per play mode.
fn music_autoplay(
    mut commands: Commands,
    mut audio_assets: ResMut<Assets<AudioSource>>,
    now_playing: Query<Entity, With<NowPlaying>>,
    sinks: Query<(&AudioSink, &PlayingTrack), With<NowPlaying>>,
    library: Res<MusicLibrary>,
    mut status: ResMut<MusicStatus>,
    mut settings: ResMut<UserSettings>,
    mut guard: ResMut<AutoplayGuard>,
) {
    if !status.playing || library.is_empty() {
        guard.0 = 0;
        return;
    }
    let Ok((sink, playing)) = sinks.single() else {
        guard.0 = 0;
        return; // sink not created yet this frame
    };
    let Some(current) = playing.0 else {
        return; // fallback BGM loops forever
    };
    if !sink.empty() {
        guard.0 = 0;
        return;
    }
    guard.0 += 1;
    if guard.0 < 3 {
        return; // debounce: fresh sinks may briefly report empty
    }
    guard.0 = 0;

    let len = library.len();
    let next = match status.mode {
        PlayMode::Sequential => {
            if current + 1 < len {
                Some(current + 1)
            } else {
                None
            }
        }
        PlayMode::Shuffle => Some(shuffle_next(current, len)),
        PlayMode::SingleLoop => Some(current),
    };
    match next {
        Some(i) => {
            play_track(&mut commands, &mut audio_assets, now_playing, &mut settings, &library, i);
            status.current = Some(i);
        }
        None => {
            despawn_now_playing(&mut commands, now_playing);
            status.current = None;
            status.playing = false;
        }
    }
}

/// Deterministic next-index offset that never immediately repeats.
fn shuffle_next(current: usize, len: usize) -> usize {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0x9E37_79B9);
    let mut x = seed ^ (current as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    let offset = (x % (len as u64 - 1)) as usize + 1;
    (current + offset) % len
}

/// Keep NowPlaying sink volume in sync with music volume + master mute settings.
fn music_volume_sync(
    settings: Res<UserSettings>,
    mut sinks: Query<&mut AudioSink, With<NowPlaying>>,
    mut global_volume: ResMut<GlobalVolume>,
) {
    if !settings.is_changed() {
        return;
    }
    let master = if settings.toggles[0] {
        settings.volumes[0] as f32 / 100.0
    } else {
        0.0
    };
    global_volume.volume = Volume::Linear(master);

    let music = music_volume(&settings);
    for mut sink in sinks.iter_mut() {
        sink.set_volume(Volume::Linear(music));
    }
}
