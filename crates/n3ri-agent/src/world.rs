//! 上下文快照（M0 占位：类型先行，聚合逻辑 M1）。

use bevy::prelude::*;
use std::collections::VecDeque;

#[derive(Debug, Clone, Default)]
pub struct WindowInfo {
    pub app_id: String,
    pub title: String,
    pub z: i32,
}

#[derive(Debug, Clone, Default)]
pub struct OutsideWindow {
    pub id: u64,
    pub title: String,
    pub app_id: String,
    pub focused: bool,
}

#[derive(Debug, Clone, Default)]
pub struct OutsideView {
    pub workspace: String,
    pub windows: Vec<OutsideWindow>,
    pub available: bool,
}

/// ui → agent 的唯一输入（`n3ri-ui` 侧 bridge 写入，M1 接线）。
#[derive(Resource, Debug, Clone, Default)]
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
    Free,
    #[default]
    FocusedWork,
    Immersive,
}

/// 1s tick 聚合结果（M1 填充，prompt 的直接原料）。
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
