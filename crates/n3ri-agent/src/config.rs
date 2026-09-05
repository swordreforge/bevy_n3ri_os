//! Agent 配置（M3：load/save 落 `~/.config/n3ri_os/agent/config.json`，原子写）。

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

impl AgentConfig {
    fn path() -> std::path::PathBuf {
        crate::memory::agent_dir().join("config.json")
    }

    pub fn load() -> Self {
        match std::fs::read_to_string(Self::path()) {
            Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let tmp = path.with_extension("json.tmp");
            if std::fs::write(&tmp, &json).is_ok() {
                let _ = std::fs::rename(&tmp, &path);
            }
        }
    }
}
