//! Global OS configuration.
//!
//! Inserted as a Resource by N3riCorePlugin.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Resource, Debug, Clone)]
pub struct OsConfig {
    pub resolution: (f32, f32),
    pub window_title: String,
    pub assets_dir: String,
    pub default_font: String,
    pub boot_duration: f32,
    pub loading_duration: f32,
    pub show_boot: bool,
}

impl Default for OsConfig {
    fn default() -> Self {
        Self {
            resolution: (1920.0, 1080.0),
            window_title: "n3ri_os".into(),
            assets_dir: "assets".into(),
            default_font: "fonts/fusion-pixel-12px-proportional-sc.woff2".into(),
            boot_duration: 3.0,
            loading_duration: 2.0,
            show_boot: true,
        }
    }
}

#[derive(Resource, Debug, Clone, Serialize, Deserialize)]
pub struct UserSettings {
    pub volumes: [u8; 4],
    pub toggles: [bool; 4],
    pub quality_idx: usize,
    /// 帧率上限档位（设置 → 显示效果 → 帧率上限）；索引见 [`FPS_TIER_HZ`]。
    /// 0 = 无限制（默认：窗口模式不限帧；壁纸模式仍受 15ms Reactive wait ≈66fps 上限约束）。
    #[serde(default)]
    pub fps_idx: usize,
    /// 垂直同步开关（设置 → 显示效果 → 垂直同步）：开 = `PresentMode::AutoVsync`，
    /// 关 = `AutoNoVsync`（默认，帧就绪即呈现，帧率跟实际帧时间走）。
    #[serde(default)]
    pub vsync: bool,
    /// 抗锯齿档位（设置 → 显示效果 → 抗锯齿）；索引见 [`MSAA_SAMPLES`]。默认 4x。
    #[serde(default = "default_msaa_idx")]
    pub msaa_idx: usize,
    /// 壁纸模式开关（设置 → 显示效果）；切换时主程序自我重启进入另一模式
    #[serde(default)]
    pub wallpaper_enabled: bool,
    /// 自然滚动开关（设置 → 触控）；开启时壁纸模式触摸板滚动方向反转（内容跟随手指）
    #[serde(default = "default_true")]
    pub natural_scroll: bool,
    /// 外部音乐歌单目录（设置 → 声音 → 音乐歌单）；None = 未配置（回退内嵌 BGM）
    #[serde(default)]
    pub music_dir: Option<String>,
    /// 播放模式 0=顺序 1=随机 2=单曲循环
    #[serde(default)]
    pub music_mode: u8,
    /// 播放源偏好：0=内置 BGM，1=外部歌单。None = 未显式选择（默认：有歌单播歌单）
    #[serde(default)]
    pub music_source: Option<u8>,
    /// 歌单中最后播放的曲目路径（仅 music_source=1 时有意义；重启按路径反查 index）
    #[serde(default)]
    pub music_track_path: Option<String>,
}

fn default_true() -> bool {
    true
}

/// 帧率上限档位（设置 → 显示效果 → 帧率上限），索引即 [`UserSettings::fps_idx`]。
/// `None` = 无限制。
pub const FPS_TIER_HZ: [Option<u32>; 7] = [
    None,
    Some(24),
    Some(30),
    Some(45),
    Some(60),
    Some(90),
    Some(120),
];

/// fps_idx → 帧率上限（Hz）；`None` = 无限制。越界索引回落为 0（无限制）。
pub fn fps_limit_hz(idx: usize) -> Option<f64> {
    FPS_TIER_HZ.get(idx).copied().flatten().map(f64::from)
}

/// 抗锯齿档位样本数（设置 → 显示效果 → 抗锯齿），索引即 [`UserSettings::msaa_idx`]。
/// 1 sample = 关闭（bevy `Msaa::Off` 即 1 sample）。
pub const MSAA_SAMPLES: [u32; 4] = [1, 2, 4, 8];

/// msaa_idx → 样本数。越界回落为 4（默认 4x）。
pub fn msaa_samples(idx: usize) -> u32 {
    MSAA_SAMPLES.get(idx).copied().unwrap_or(4)
}

fn default_msaa_idx() -> usize {
    2
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            volumes: [80, 71, 80, 100],
            toggles: [true, true, true, true],
            quality_idx: 0,
            fps_idx: 0,
            vsync: false,
            msaa_idx: 2,
            wallpaper_enabled: false,
            natural_scroll: true,
            music_dir: None,
            music_mode: 0,
            music_source: None,
            music_track_path: None,
        }
    }
}

impl UserSettings {
    /// 统一配置目录 ~/.config/n3ri_os（与 n3ri-llm 的 llm-config.json 同目录）
    fn config_dir() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("n3ri_os")
    }

    fn settings_path() -> PathBuf {
        Self::config_dir().join("user_settings.json")
    }

    /// 迁移源：统一到 config dir 之前遗留的 cwd/user_settings.json
    fn legacy_settings_path() -> PathBuf {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("user_settings.json")
    }

    pub fn load() -> Self {
        let path = Self::settings_path();
        match fs::read_to_string(&path) {
            Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
            Err(_) => {
                let legacy = Self::legacy_settings_path();
                match fs::read_to_string(&legacy) {
                    Ok(json) => {
                        let settings: Self = serde_json::from_str(&json).unwrap_or_default();
                        settings.save();
                        settings
                    }
                    Err(_) => Self::default(),
                }
            }
        }
    }

    pub fn save(&self) {
        let dir = Self::config_dir();
        let _ = fs::create_dir_all(&dir);
        let path = Self::settings_path();
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = fs::write(path, json);
        }
    }

    pub fn music_volume(&self) -> f32 {
        if self.toggles[0] {
            self.volumes[0] as f32 / 100.0
        } else {
            0.0
        }
    }
}

#[derive(Resource, Debug, Clone)]
pub struct ThemeConfig {
    pub primary: Color,
    pub secondary: Color,
    pub background: Color,
    pub surface: Color,
    pub text: Color,
    pub text_muted: Color,
    pub accent: Color,
    pub error: Color,
    pub success: Color,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            primary: Color::srgb(0.4, 0.95, 1.0),
            secondary: Color::srgb(0.15, 0.45, 1.0),
            background: Color::srgb(0.02, 0.05, 0.1),
            surface: Color::srgba(0.05, 0.1, 0.18, 0.9),
            text: Color::srgb(0.86, 0.93, 0.93),
            text_muted: Color::srgba(0.6, 0.75, 0.85, 0.6),
            accent: Color::srgb(0.49, 0.89, 1.0),
            error: Color::srgb(1.0, 0.3, 0.3),
            success: Color::srgb(0.3, 1.0, 0.5),
        }
    }
}

impl ThemeConfig {
    pub fn with_alpha(color: Color, alpha: f32) -> Color {
        match color {
            Color::Srgba(c) => Color::Srgba(Srgba {
                alpha: c.alpha * alpha,
                ..c
            }),
            _ => color,
        }
    }
}
