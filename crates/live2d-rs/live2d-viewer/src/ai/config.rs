use std::path::PathBuf;

use crate::ai::types::AiConfig;

/// Directory: $CONFIG_DIR/live2d-viewer/
fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("live2d-viewer")
}

/// File path: $CONFIG_DIR/live2d-viewer/ai-config.json
fn config_path() -> PathBuf {
    config_dir().join("ai-config.json")
}

/// Load AI config from DB (preferred) or JSON file (fallback).
///
/// When a database connection is available, reads from the `global_settings`
/// table (key `"ai_config"`).  Falls back to the JSON file on disk for
/// backward compatibility.
pub fn load_config(db: Option<&crate::db::AppDb>) -> AiConfig {
    // Try DB first
    if let Some(db) = db {
        if let Some(json) = db.get_setting("ai_config") {
            if let Ok(config) = serde_json::from_str(&json) {
                return config;
            }
            log::warn!("Failed to parse AI config from DB, falling back to file");
        }
    }
    // Fallback to JSON file on disk
    let path = config_path();
    match std::fs::read_to_string(&path) {
        Ok(json) => serde_json::from_str(&json).unwrap_or_else(|e| {
            log::warn!("Failed to parse AI config (using defaults): {e}");
            AiConfig::default()
        }),
        Err(_) => AiConfig::default(),
    }
}

/// Save AI config to DB (if available) and JSON file (backup).
///
/// Writes to both the `global_settings` SQLite table and the JSON file on
/// disk so the config survives a missing/corrupt database.
pub fn save_config(config: &AiConfig, db: Option<&crate::db::AppDb>) {
    // Save to DB if available
    if let Some(db) = db {
        if let Ok(json) = serde_json::to_string(config) {
            let _ = db.set_setting("ai_config", &json);
        }
    }
    // Also keep JSON file as backup (atomic write via temp + rename)
    let dir = config_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = config_path();
    let tmp = dir.join("ai-config.json.tmp");
    match serde_json::to_string_pretty(config) {
        Ok(json) => {
            if std::fs::write(&tmp, &json).is_ok() {
                let _ = std::fs::rename(&tmp, &path);
            }
        }
        Err(e) => log::error!("Failed to serialize AI config: {e}"),
    }
}
