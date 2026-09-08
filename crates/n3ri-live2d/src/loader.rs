//! Model3.json parsing and mocari runtime loading for the desktop pet.
//!
//! Reads `assets/nori/ARGNori_web/` from disk (.moc3 / physics / motions);
//! under the `embed-model` feature it falls back to the compile-time
//! embedded model file table when disk files are absent (embed builds).
//! Textures always go through the AssetServer (see renderer).

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::pet::Live2dPet;
use mocari::{
    json::{Expression3, Motion3, Physics3, Pose3},
    moc3::{
        Moc3ArtMeshKeyforms, Moc3ArtMeshes, Moc3CanvasInfo, Moc3Deformers, Moc3DrawOrderGroups,
        Moc3Glues, Moc3Ids, Moc3KeyformBindings, Moc3OffscreenInfo, Moc3Parts,
    },
    motion::MotionPlayer,
    runtime::ModelRuntime,
};

/// Model directory relative to the assets root (same convention as
/// `asset_server.load("nori/...")` calls elsewhere in the app).
pub const MODEL_DIR: &str = "nori/ARGNori_web";

/// Motion groups played concurrently, each with its own player so they don't
/// fade each other out.
const CONCURRENT_GROUPS: [&str; 1] = ["Idle"];

// ── model3.json serde types (only what we consume) ──

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Model3Json {
    file_references: FileReferences,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct FileReferences {
    moc: String,
    #[serde(default)]
    textures: Vec<String>,
    #[serde(default)]
    physics: Option<String>,
    #[serde(default)]
    pose: Option<String>,
    #[serde(default)]
    motions: std::collections::HashMap<String, Vec<MotionRef>>,
    #[serde(default)]
    expressions: Vec<ExpressionRef>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct MotionRef {
    file: String,
    // model3-level fade overrides: mocari's Model3 manifest drops them, and
    // per-curve fades drive the actual blend — parsed but intentionally unused.
    #[allow(dead_code)]
    fade_in_time: Option<f32>,
    #[allow(dead_code)]
    fade_out_time: Option<f32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ExpressionRef {
    name: String,
    file: String,
}

// ── path resolution ──

/// Resolve the assets dir the same way the main app does:
/// `AssetPlugin { file_path: "../../assets" }` is relative to the process CWD.
fn resolve_assets_dir() -> Result<PathBuf, String> {
    for c in ["../../assets", "../../../assets", "../assets", "assets"] {
        let p = PathBuf::from(c);
        if p.is_dir() {
            return Ok(p);
        }
    }
    Err("assets dir not found (tried ../../assets, ../../../assets, ../assets, assets)".into())
}

/// 模型文件读取：磁盘优先，`embed-model` feature 下回退编译期内置表
fn read_model_file(dir: Option<&Path>, rel: &str) -> Result<Vec<u8>, String> {
    if let Some(dir) = dir {
        let p = dir.join(rel);
        if p.is_file() {
            return std::fs::read(&p).map_err(|e| format!("read {}: {e}", p.display()));
        }
    }
    #[cfg(feature = "embed-model")]
    if let Some(b) = crate::embed_model::lookup(rel) {
        return Ok(b.to_vec());
    }
    Err(format!("model file missing: {rel}"))
}

/// 定位 *.model3.json（磁盘扫描或内嵌表扫描），返回 (相对路径, 字节)
fn find_model3_json(dir: Option<&Path>) -> Result<(String, Vec<u8>), String> {
    if let Some(dir) = dir {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.is_file()
                    && p.extension().is_some_and(|ext| ext == "json")
                    && p.to_string_lossy().ends_with(".model3.json")
                {
                    let rel = p
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    let bytes =
                        std::fs::read(&p).map_err(|e| format!("read {}: {e}", p.display()))?;
                    return Ok((rel, bytes));
                }
            }
        }
    }
    #[cfg(feature = "embed-model")]
    if let Some((rel, b)) = crate::embed_model::find_model3() {
        return Ok((rel, b.to_vec()));
    }
    Err("no .model3.json found (disk or embedded)".into())
}

// ── loading ──

fn parse_component<T>(
    moc: &[u8],
    what: &str,
    parse: impl FnOnce(&[u8]) -> mocari::Result<T>,
) -> Result<T, String> {
    parse(moc).map_err(|e| format!("parse {what}: {e}"))
}

/// Load the pet model. Returns Err (not panic) on any failure so the desktop
/// stays usable without the pet.
pub fn load_pet() -> Result<Live2dPet, String> {
    let disk_dir = resolve_assets_dir()
        .ok()
        .map(|a| a.join(MODEL_DIR))
        .filter(|d| d.is_dir());
    let disk_dir: Option<&Path> = disk_dir.as_deref();

    let (json_rel, json_bytes) = find_model3_json(disk_dir)?;
    let json_text =
        std::str::from_utf8(&json_bytes).map_err(|e| format!("parse {json_rel}: {e}"))?;
    let model3: Model3Json =
        serde_json::from_str(json_text).map_err(|e| format!("parse {json_rel}: {e}"))?;

    // Moc3 — pure-Rust parse, no FFI, no leaking.
    let moc_bytes =
        read_model_file(disk_dir, &model3.file_references.moc).map_err(|e| format!("read moc3: {e}"))?;
    let art_meshes = parse_component(&moc_bytes, "art meshes", Moc3ArtMeshes::parse)?;
    let art_mesh_keyforms =
        parse_component(&moc_bytes, "keyforms", Moc3ArtMeshKeyforms::parse)?;
    let deformers = parse_component(&moc_bytes, "deformers", Moc3Deformers::parse)?;
    let bindings = parse_component(&moc_bytes, "bindings", Moc3KeyformBindings::parse)?;
    let ids = parse_component(&moc_bytes, "ids", Moc3Ids::parse)?;
    let offscreen = parse_component(&moc_bytes, "offscreen", Moc3OffscreenInfo::parse)?;
    let glues = parse_component(&moc_bytes, "glues", Moc3Glues::parse)?;
    let parts = parse_component(&moc_bytes, "parts", Moc3Parts::parse)?;
    let canvas = parse_component(&moc_bytes, "canvas", Moc3CanvasInfo::parse)?;
    let draw_order_groups = Moc3DrawOrderGroups::parse(&moc_bytes)
        .map_err(|e| format!("parse draw order groups: {e}"))?;

    // Pose (ARGNori's model3.json has no Pose entry — stays None).
    let pose: Option<Pose3> = match &model3.file_references.pose {
        Some(rel) => {
            let bytes =
                read_model_file(disk_dir, rel).map_err(|e| format!("read pose: {e}"))?;
            let text =
                std::str::from_utf8(&bytes).map_err(|e| format!("parse pose {rel}: {e}"))?;
            match Pose3::from_json_str(text) {
                Ok(p) => Some(p),
                Err(e) => {
                    bevy::log::warn!("live2d: pose disabled ({e})");
                    None
                }
            }
        }
        None => None,
    };

    let mut runtime = ModelRuntime::new(
        mocari_model_manifest(&model3, &json_rel)?,
        canvas,
        art_meshes,
        art_mesh_keyforms,
        deformers,
        bindings,
        ids,
        offscreen,
        glues,
        parts,
        draw_order_groups,
        pose,
    )
    .ok_or_else(|| "mocari: failed to build drawable meshes".to_string())?;

    // Physics (hair/clothes swing) — set_physics + one stabilization pass.
    if let Some(rel) = &model3.file_references.physics {
        match read_model_file(disk_dir, rel)
            .map_err(|e| format!("read physics: {e}"))
            .and_then(|b| {
                std::str::from_utf8(&b)
                    .map_err(|e| format!("parse physics {rel}: {e}"))
                    .and_then(|t| {
                        Physics3::from_json_str(t)
                            .map_err(|e| format!("parse physics {rel}: {e}"))
                    })
            }) {
            Ok(physics) => {
                runtime.set_physics(physics);
                runtime.stabilize_physics();
            }
            Err(e) => {
                bevy::log::warn!("live2d: physics disabled ({e})");
            }
        }
    }

    // Motion players — one per concurrent group. mocari's Model3 manifest
    // drops model3-level FadeIn/FadeOut overrides, so we stash the declared
    // fades per group (currently only informational).
    let mut players: Vec<MotionPlayer> = Vec::new();
    let mut idle_motion: Option<Motion3> = None;
    let mut sleep_motion: Option<Motion3> = None;

    for group in CONCURRENT_GROUPS {
        let Some(refs) = model3.file_references.motions.get(group) else {
            continue;
        };
        let Some(first) = refs.first() else {
            continue;
        };
        let bytes = match read_model_file(disk_dir, &first.file) {
            Ok(b) => b,
            Err(e) => {
                bevy::log::warn!("live2d: skip motion {}: {e}", first.file);
                continue;
            }
        };
        let text = match std::str::from_utf8(&bytes) {
            Ok(t) => t,
            Err(e) => {
                bevy::log::warn!("live2d: skip motion {}: {e}", first.file);
                continue;
            }
        };
        let motion = match Motion3::from_json_str(text) {
            Ok(m) => m,
            Err(e) => {
                bevy::log::warn!("live2d: skip motion {}: {e}", first.file);
                continue;
            }
        };
        players.push(MotionPlayer::new(motion.clone()));

        if group == "Idle" {
            idle_motion = Some(motion);
            if let Some(sleep_ref) = refs.iter().find(|r| r.file.contains("sleep")) {
                if let Ok(sleep_bytes) = read_model_file(disk_dir, &sleep_ref.file) {
                    if let Ok(sleep_text) = std::str::from_utf8(&sleep_bytes) {
                        if let Ok(sleep_parsed) = Motion3::from_json_str(sleep_text) {
                            sleep_motion = Some(sleep_parsed);
                        }
                    }
                }
            }
        }
    }
    if players.is_empty() {
        return Err("no playable motions found".into());
    }

    let texture_paths = model3.file_references.textures.clone();

    let mut expressions: std::collections::HashMap<String, Expression3> =
        std::collections::HashMap::new();
    for expr_ref in &model3.file_references.expressions {
        let bytes = match read_model_file(disk_dir, &expr_ref.file) {
            Ok(b) => b,
            Err(e) => {
                bevy::log::warn!("live2d: skip expression {}: {e}", expr_ref.file);
                continue;
            }
        };
        let text = match std::str::from_utf8(&bytes) {
            Ok(t) => t,
            Err(e) => {
                bevy::log::warn!("live2d: skip expression {}: {e}", expr_ref.file);
                continue;
            }
        };
        match Expression3::from_json_str(text) {
            Ok(parsed) => {
                expressions.insert(expr_ref.name.clone(), parsed);
            }
            Err(e) => {
                bevy::log::warn!("live2d: skip expression {}: {e}", expr_ref.file);
            }
        }
    }
    bevy::log::info!("live2d expressions loaded: {}", expressions.len());

    Ok(Live2dPet::new(
        runtime,
        texture_paths,
        players,
        idle_motion,
        sleep_motion,
        expressions,
    ))
}

/// Rebuild mocari's `Model3` manifest from our minimal serde view.
///
/// mocari 0.4.0's `ModelReference` only parses the `File` per motion curve
/// (model3-level FadeIn/FadeOut/Sound are dropped upstream), so a JSON
/// round-trip through our subset is lossless for what the runtime consumes:
/// moc path, textures, physics, pose, motions, expressions, hit areas
/// (ARGNori declares no groups/hit-areas of its own).
fn mocari_model_manifest(
    model3: &Model3Json,
    json_rel: &str,
) -> Result<mocari::json::Model3, String> {
    let refs = &model3.file_references;
    let mut motions_json = String::from("{");
    let mut first_group = true;
    for (group, list) in &refs.motions {
        if !first_group {
            motions_json.push(',');
        }
        first_group = false;
        motions_json.push_str(&format!("{group:?}:["));
        for (i, m) in list.iter().enumerate() {
            if i > 0 {
                motions_json.push(',');
            }
            motions_json.push_str(&format!("{{\"File\":{:?}}}", m.file));
        }
        motions_json.push(']');
    }
    motions_json.push('}');
    let mut exprs_json = String::from("[");
    for (i, e) in refs.expressions.iter().enumerate() {
        if i > 0 {
            exprs_json.push(',');
        }
        exprs_json.push_str(&format!("{{\"Name\":{:?},\"File\":{:?}}}", e.name, e.file));
    }
    exprs_json.push(']');

    let mut doc = format!(
        "{{\"Version\":3,\"FileReferences\":{{\"Moc\":{:?},\"Textures\":[",
        refs.moc
    );
    for (i, t) in refs.textures.iter().enumerate() {
        if i > 0 {
            doc.push(',');
        }
        doc.push_str(&format!("{t:?}"));
    }
    doc.push_str("],\"Motions\":");
    doc.push_str(&motions_json);
    doc.push_str(",\"Expressions\":");
    doc.push_str(&exprs_json);
    if let Some(p) = &refs.physics {
        doc.push_str(&format!(",\"Physics\":{p:?}"));
    }
    if let Some(p) = &refs.pose {
        doc.push_str(&format!(",\"Pose\":{p:?}"));
    }
    doc.push_str("}}");
    mocari::json::Model3::from_json_str(&doc)
        .map_err(|e| format!("parse {json_rel}: {e}"))
}
