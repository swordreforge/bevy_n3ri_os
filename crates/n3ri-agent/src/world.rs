//! 上下文快照：AgentWorldView（ui 侧 bridge 写入）→ ContextSnapshot（1s 聚合）。
//!
//! 判定数字出处见 `docs/nori-agent-dev.md` §6 对照表；纯函数可单测，不碰 ECS。

use bevy::prelude::*;
use std::collections::VecDeque;
use std::sync::OnceLock;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct WindowInfo {
    pub app_id: String,
    pub title: String,
    pub z: i32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct OutsideWindow {
    pub id: u64,
    pub title: String,
    pub app_id: String,
    pub focused: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct OutsideView {
    pub workspace: String,
    pub windows: Vec<OutsideWindow>,
    pub available: bool,
}

/// 运行模式标记（minimal 启动参数透入；bridge 只读）。
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WallpaperMode(pub bool);

/// ui → agent 的唯一输入（`n3ri-ui` 侧 bridge 写入；`cursor_moved`/`input_event`
/// 为粘性位：bridge 只置 true，`context_tick` 消费后清零）。
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct AgentWorldView {
    pub focused_title: String,
    pub focused_app_id: Option<String>,
    pub visible_windows: Vec<WindowInfo>,
    pub typing: bool,
    pub cursor_moved: bool,
    pub input_event: bool,
    pub music_playing: bool,
    pub music_title: Option<String>,
    pub immersive: bool,
    pub outside: Option<OutsideView>,
    pub wallpaper_mode: bool,
    pub now_local: String,
    pub cpu_pct: Option<f32>,
    pub net_online: Option<bool>,
    pub battery_pct: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Presence {
    Active,
    #[default]
    Idle,
    Away,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Activity {
    #[default]
    Free,
    FocusedWork,
    Immersive,
}

/// 1s tick 聚合结果（prompt 的直接原料）。
#[derive(Resource, Debug, Clone, Default)]
pub struct ContextSnapshot {
    pub updated_at: f64,
    pub last_active_at: f64,
    pub idle_secs: f32,
    pub presence: Presence,
    pub activity: Activity,
    pub focus_dwell: (String, f32),
    pub focus_trail: VecDeque<(String, f64)>,
}

pub const TICK_INTERVAL: f32 = 1.0;
pub const DEBUG_INTERVAL: f32 = 10.0;
pub const IDLE_SECS: f32 = 300.0;
pub const AWAY_IDLE_SECS: f32 = 900.0;
pub const FOCUSED_WORK_DWELL_SECS: f32 = 90.0;
pub const BROWSER_IMMERSIVE_DWELL_SECS: f32 = 120.0;
pub const TRANSITION_WINDOW_SECS: f64 = 300.0;
pub const TRANSITION_DISTINCT: usize = 5;
pub const FOCUS_TRAIL_MAX: usize = 8;

pub fn classify_presence(idle_secs: f32) -> Presence {
    if idle_secs >= AWAY_IDLE_SECS {
        Presence::Away
    } else if idle_secs >= IDLE_SECS {
        Presence::Idle
    } else {
        Presence::Active
    }
}

pub fn classify_activity(
    immersive: bool,
    focused_app_id: &str,
    dwell_secs: f32,
    engaged: bool,
) -> Activity {
    if immersive {
        Activity::Immersive
    } else if focused_app_id == "browser" && dwell_secs >= BROWSER_IMMERSIVE_DWELL_SECS {
        Activity::Immersive
    } else if dwell_secs >= FOCUSED_WORK_DWELL_SECS && engaged {
        Activity::FocusedWork
    } else {
        Activity::Free
    }
}

pub fn update_dwell(prev_id: &str, prev_dwell: f32, next_id: &str, dt: f32) -> (String, f32) {
    if next_id == prev_id && !next_id.is_empty() {
        (next_id.to_string(), prev_dwell + dt)
    } else {
        (next_id.to_string(), 0.0)
    }
}

pub fn push_trail(trail: &mut VecDeque<(String, f64)>, id: &str, now: f64) {
    if trail.back().map(|(last, _)| last.as_str() != id).unwrap_or(true) {
        trail.push_back((id.to_string(), now));
        while trail.len() > FOCUS_TRAIL_MAX {
            trail.pop_front();
        }
    }
}

/// 5min 窗口内切过 ≥5 个不同窗口 → transitioning（此时按 Free 处理，idle 钩子延迟）。
pub fn is_transitioning(trail: &VecDeque<(String, f64)>, now: f64) -> bool {
    let mut distinct: Vec<&str> = Vec::new();
    for (id, ts) in trail.iter() {
        if now - ts > TRANSITION_WINDOW_SECS {
            continue;
        }
        if !distinct.contains(&id.as_str()) {
            distinct.push(id.as_str());
        }
    }
    distinct.len() >= TRANSITION_DISTINCT
}

static AGENT_DEBUG: OnceLock<bool> = OnceLock::new();

pub fn context_tick(
    time: Res<Time>,
    mut view: ResMut<AgentWorldView>,
    mut snap: ResMut<ContextSnapshot>,
    mut acc: Local<f32>,
    mut dbg_acc: Local<f32>,
) {
    let dt = time.delta_secs();
    *acc += dt;
    if *acc < TICK_INTERVAL {
        return;
    }
    let step = *acc;
    *acc = 0.0;
    let now = time.elapsed_secs_f64();

    if snap.last_active_at == 0.0 {
        snap.last_active_at = now;
    }
    if view.input_event || view.cursor_moved {
        snap.last_active_at = now;
    }
    view.input_event = false;
    view.cursor_moved = false;

    snap.idle_secs = (now - snap.last_active_at).max(0.0) as f32;
    snap.presence = classify_presence(snap.idle_secs);

    let cur_id = view.focused_app_id.clone().unwrap_or_default();
    let (id, dwell) = update_dwell(&snap.focus_dwell.0, snap.focus_dwell.1, &cur_id, step);
    snap.focus_dwell = (id.clone(), dwell);
    push_trail(&mut snap.focus_trail, &id, now);

    let engaged = view.typing;
    snap.activity = classify_activity(view.immersive, &cur_id, dwell, engaged);
    snap.updated_at = now;

    let debug = *AGENT_DEBUG.get_or_init(|| std::env::var("N3RI_AGENT_DEBUG").is_ok());
    if debug {
        *dbg_acc += step;
        if *dbg_acc >= DEBUG_INTERVAL {
            *dbg_acc = 0.0;
            info!(
                "[agent] presence={:?} activity={:?} idle={:.0}s focus={}({:.0}s) typing={} music={}",
                snap.presence,
                snap.activity,
                snap.idle_secs,
                if id.is_empty() { "桌面" } else { &id },
                dwell,
                view.typing,
                view.music_playing,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presence_boundaries() {
        assert_eq!(classify_presence(0.0), Presence::Active);
        assert_eq!(classify_presence(299.9), Presence::Active);
        assert_eq!(classify_presence(300.0), Presence::Idle);
        assert_eq!(classify_presence(899.9), Presence::Idle);
        assert_eq!(classify_presence(900.0), Presence::Away);
    }

    #[test]
    fn activity_rules() {
        assert_eq!(
            classify_activity(true, "terminal", 0.0, false),
            Activity::Immersive
        );
        assert_eq!(
            classify_activity(false, "browser", 119.9, false),
            Activity::Free
        );
        assert_eq!(
            classify_activity(false, "browser", 120.0, false),
            Activity::Immersive
        );
        assert_eq!(
            classify_activity(false, "terminal", 90.0, true),
            Activity::FocusedWork
        );
        assert_eq!(
            classify_activity(false, "terminal", 90.0, false),
            Activity::Free
        );
        assert_eq!(
            classify_activity(false, "terminal", 89.9, true),
            Activity::Free
        );
    }

    #[test]
    fn dwell_accumulates_and_resets() {
        let (id, d) = update_dwell("terminal", 10.0, "terminal", 1.0);
        assert_eq!((id.as_str(), d), ("terminal", 11.0));
        let (id, d) = update_dwell("terminal", 10.0, "browser", 1.0);
        assert_eq!((id.as_str(), d), ("browser", 0.0));
        let (id, d) = update_dwell("", 0.0, "", 1.0);
        assert_eq!((id.as_str(), d), ("", 0.0));
    }

    #[test]
    fn trail_caps_and_detects_transition() {
        let mut trail = VecDeque::new();
        for i in 0..10 {
            push_trail(&mut trail, &format!("app{i}"), i as f64);
        }
        assert_eq!(trail.len(), FOCUS_TRAIL_MAX);
        push_trail(&mut trail, "app9", 10.0);
        assert_eq!(trail.len(), FOCUS_TRAIL_MAX);

        let mut trail = VecDeque::new();
        for (i, id) in ["a", "b", "c", "d"].iter().enumerate() {
            push_trail(&mut trail, id, i as f64);
        }
        assert!(!is_transitioning(&trail, 100.0));
        push_trail(&mut trail, "e", 100.0);
        assert!(is_transitioning(&trail, 100.0));

        let mut old = VecDeque::new();
        for (i, id) in ["a", "b", "c", "d", "e"].iter().enumerate() {
            push_trail(&mut old, id, i as f64);
        }
        assert!(!is_transitioning(&old, 1000.0));
    }
}
