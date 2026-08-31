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
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            volumes: [80, 71, 80, 100],
            toggles: [true, true, true, true],
            quality_idx: 0,
        }
    }
}

impl UserSettings {
    fn settings_path() -> PathBuf {
        let mut path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        path.push("user_settings.json");
        path
    }

    pub fn load() -> Self {
        let path = Self::settings_path();
        match fs::read_to_string(&path) {
            Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) {
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
