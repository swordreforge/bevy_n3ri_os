//! ui → agent 世界视图桥（M1：只写 `AgentWorldView`，不读回、不做决策）。
//!
//! 在 `WindowFocusSet` 之后跑（保证 `FocusedTitle` 已结算），1s 快照一次，
//! 变化才写。`cursor_moved`/`input_event` 为粘性位：只置 true，由 agent 侧
//! `context_tick` 消费后清零。

use bevy::input::keyboard::KeyboardInput;
use bevy::input::mouse::MouseButtonInput;
use bevy::prelude::*;
use chrono::{Datelike, Local as ChronoLocal, Timelike};
use n3ri_agent::{AgentWorldView, OutsideView, WallpaperMode, WindowInfo};

use crate::cursor::{CursorPosition, UiArea};
use crate::dock::AppVisible;
use crate::input_focus::{TextInputFocus, TextInputOwner};
use crate::topbar::FocusedTitle;
use crate::window::{AppWindow, CinematicLocked};

pub struct AgentBridgePlugin;

impl Plugin for AgentBridgePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BridgeScratch>().add_systems(
            Update,
            world_view_bridge.after(crate::window::WindowFocusSet),
        );
    }
}

const SNAPSHOT_INTERVAL: f32 = 1.0;
const OUTSIDE_POLL_SECS: f32 = 5.0;

fn now_local_string() -> String {
    let now = ChronoLocal::now();
    let weekday = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"]
        [now.weekday().num_days_from_sunday() as usize];
    format!(
        "{}年{}月{}日 {} {} {:02}:{:02}",
        now.year(),
        now.month(),
        now.day(),
        weekday,
        n3ri_agent::daypart(now.hour()),
        now.hour(),
        now.minute()
    )
}

#[derive(Resource, Default)]
struct BridgeScratch {
    acc: f32,
    last_cursor: Vec2,
    last_now: String,
    last_visible: Vec<WindowInfo>,
    last_outside: Option<OutsideView>,
    outside_acc: f32,
}

#[allow(clippy::too_many_arguments)]
fn world_view_bridge(
    time: Res<Time>,
    mut scratch: ResMut<BridgeScratch>,
    focused: Res<FocusedTitle>,
    owner: Res<TextInputOwner>,
    cursor: Res<CursorPosition>,
    area: Res<UiArea>,
    mut keyboard: MessageReader<KeyboardInput>,
    mut mouse_btn: MessageReader<MouseButtonInput>,
    windows: Query<(&AppWindow, &AppVisible, &Visibility)>,
    windows_by_entity: Query<&AppWindow>,
    locked: Query<(), With<CinematicLocked>>,
    mode: Option<Res<WallpaperMode>>,
    music: Option<Res<n3ri_core::music::MusicStatus>>,
    library: Option<Res<n3ri_core::music::MusicLibrary>>,
    mut view: ResMut<AgentWorldView>,
) {
    for _ in keyboard.read() {
        view.input_event = true;
    }
    for _ in mouse_btn.read() {
        view.input_event = true;
    }
    if cursor.logical != scratch.last_cursor {
        scratch.last_cursor = cursor.logical;
        view.cursor_moved = true;
    }

    scratch.acc += time.delta_secs();
    if scratch.acc < SNAPSHOT_INTERVAL {
        return;
    }
    scratch.acc = 0.0;
    let _ = &area;

    let now_str = now_local_string();
    if scratch.last_now != now_str {
        scratch.last_now = now_str.clone();
        view.now_local = now_str;
    }

    let mut infos: Vec<WindowInfo> = windows
        .iter()
        .filter(|(_, vis, _)| vis.0)
        .map(|(w, _, _)| WindowInfo {
            app_id: w.app_id.clone(),
            title: w.title.clone(),
            z: w.z,
        })
        .collect();
    infos.sort_by_key(|w| w.z);
    if scratch.last_visible != infos {
        scratch.last_visible = infos.clone();
        view.visible_windows = infos;
    }

    view.focused_title = focused.title.clone();
    view.focused_app_id = focused
        .entity
        .and_then(|e| windows_by_entity.get(e).ok())
        .map(|w| w.app_id.clone())
        .or_else(|| {
            view.visible_windows
                .iter()
                .rev()
                .find(|w| w.title == focused.title)
                .map(|w| w.app_id.clone())
        });

    view.typing = !matches!(owner.0, TextInputFocus::None);
    view.immersive = !locked.is_empty();
    view.wallpaper_mode = mode.map(|m| m.0).unwrap_or(false);

    if let Some(status) = music.as_ref() {
        view.music_playing = status.playing;
        view.music_title = status.current.and_then(|i| {
            library.as_ref().and_then(|lib| {
                lib.0.get(i).map(|t| {
                    if t.artist.is_empty() {
                        t.title.clone()
                    } else {
                        format!("{} - {}", t.artist, t.title)
                    }
                })
            })
        });
    }

    scratch.outside_acc += SNAPSHOT_INTERVAL;
    if scratch.outside_acc >= OUTSIDE_POLL_SECS {
        scratch.outside_acc = 0.0;
        let next = poll_outside();
        if scratch.last_outside != next {
            scratch.last_outside = next.clone();
            view.outside = next;
        }
    }
}

/// M1 占位：niri 投影在 M4 `tools/niri.rs` 落地后由此调用；此前恒为 None
/// （`build_context_block` 遇到 None 即跳过该行）。
fn poll_outside() -> Option<OutsideView> {
    None
}
