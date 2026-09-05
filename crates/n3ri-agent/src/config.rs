//! Agent 配置（M0 占位：默认值 + 路径，load/save 在 M2 接 scheduler.json 时补全）。

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Resource, Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub enabled: bool,
    pub idle_threshold_secs: u64,
    pub idle_cooldown_secs: u64,
    pub daily_quota: f32,
    pub min_gap_secs: u64,
    pub memory_enabled: bool,
    pub hourly_chime: bool,
    pub break_reminder: bool,
    pub break_dwell_secs: u64,
    pub niri_tools: bool,
    pub niri_spawn_allow: Vec<String>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            idle_threshold_secs: 1800,
            idle_cooldown_secs: 21600,
            daily_quota: 1.0,
            min_gap_secs: 14400,
            memory_enabled: true,
            hourly_chime: false,
            break_reminder: true,
            break_dwell_secs: 2700,
            niri_tools: true,
            niri_spawn_allow: ["firefox", "kitty", "alacritty", "nautilus", "code"]
                .into_iter()
                .map(str::to_string)
                .collect(),
        }
    }
}
