//! Model3.json parsing and Cubism asset loading for the desktop pet.
//!
//! Reads `assets/nori/ARGNori_web/` from disk (moc3 / physics / motions);
//! under the `embed-model` feature it falls back to the compile-time
//! embedded model file table when disk files are absent (embed builds).
//! Textures always go through the AssetServer (see renderer).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::pet::Live2dPet;
use live2d_core::moc::Moc;
use live2d_core::model::Model;
use live2d_motion::breath::Breath;
use live2d_motion::{ExpressionMotion};
use live2d_motion::json::{parse_expression_json, parse_motion_json};
use live2d_motion::motion::CubismMotion;
use live2d_motion::physics::{PhysicsEngine, PhysicsParams};
use live2d_motion::queue::MotionQueueManager;

/// Model directory relative to the assets root (same convention as
/// `asset_server.load("nori/...")` calls elsewhere in the app).
pub const MODEL_DIR: &str = "nori/ARGNori_web";

/// Motion groups played concurrently, each with its own queue so they don't
/// fade each other out (`start_motion` fades all entries of the SAME queue).
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
    motions: HashMap<String, Vec<MotionRef>>,
    #[serde(default)]
    expressions: Vec<ExpressionRef>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct MotionRef {
    file: String,
    fade_in_time: Option<f32>,
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

/// Load the pet model. Returns Err (not panic) on any failure so the desktop
/// stays usable without the pet.
pub fn load_pet() -> Result<Live2dPet, String> {
    let disk_dir = resolve_assets_dir()
        .ok()
        .map(|a| a.join(MODEL_DIR))
        .filter(|d| d.is_dir());
    let disk_dir: Option<&Path> = disk_dir.as_deref();

    let (json_rel, json_bytes) = find_model3_json(disk_dir)?;
    let model3: Model3Json = serde_json::from_slice(&json_bytes)
        .map_err(|e| format!("parse {json_rel}: {e}"))?;

    // Moc — leaked to 'static so Model<'static> is self-contained.
    let moc_bytes = read_model_file(disk_dir, &model3.file_references.moc)
        .map_err(|e| format!("read moc3: {e}"))?;
    let moc = Moc::revive(&moc_bytes).map_err(|e| format!("revive moc: {e:?}"))?;
    let moc: &'static Moc = Box::leak(Box::new(moc));
    let mut model = Model::initialize(moc).map_err(|e| format!("init model: {e:?}"))?;
    model.update();

    // Parameter bookkeeping (ids + ranges) — needed by motions & physics.
    let params = model.parameters();
    let param_ids: Vec<String> =
        params.ids().iter().map(|id| id.to_string_lossy().into_owned()).collect();
    let param_lookup: HashMap<String, usize> =
        param_ids.iter().enumerate().map(|(i, id)| (id.clone(), i)).collect();
    let mins = params.minimum_values().to_vec();
    let maxs = params.maximum_values().to_vec();
    let defaults = params.default_values().to_vec();

    // Parts lookup for PartOpacity motion curves.
    let parts = model.parts();
    let part_ids: Vec<String> =
        parts.ids().iter().map(|id| id.to_string_lossy().into_owned()).collect();
    let part_lookup: HashMap<String, usize> =
        part_ids.iter().enumerate().map(|(i, id)| (id.clone(), i)).collect();
    let saved_parts = parts.opacities().to_vec();

    let saved_params = defaults.clone();

    let mut queues: Vec<MotionQueueManager> = Vec::new();
    let mut idle_motion: Option<CubismMotion> = None;
    let mut sleep_motion: Option<CubismMotion> = None;

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
        let parsed =
            parse_motion_json(&bytes)
            .map_err(|e| format!("parse motion {}: {e}", first.file))?;
        let motion = CubismMotion::new(
            parsed,
            first.fade_in_time.unwrap_or(1.0),
            first.fade_out_time.unwrap_or(1.0),
        );
        let mut queue = MotionQueueManager::new();
        queue.start_motion(motion.clone(), None);
        queues.push(queue);

        if *group == *"Idle" {
            idle_motion = Some(motion);
            if let Some(sleep_ref) = refs.iter().find(|r| r.file.contains("sleep")) {
                if let Ok(sleep_bytes) = read_model_file(disk_dir, &sleep_ref.file) {
                    if let Ok(sleep_parsed) = parse_motion_json(&sleep_bytes) {
                        sleep_motion = Some(CubismMotion::new(
                            sleep_parsed,
                            sleep_ref.fade_in_time.unwrap_or(1.0),
                            sleep_ref.fade_out_time.unwrap_or(1.0),
                        ));
                    }
                }
            }
        }
    }
    if queues.is_empty() {
        return Err("no playable motions found".into());
    }

    // Physics (hair/clothes swing).
    let physics = match &model3.file_references.physics {
        Some(rel) => {
            match read_model_file(disk_dir, rel)
                .map_err(|e| format!("read physics: {e}"))
                .and_then(|b| PhysicsEngine::from_json(&b))
            {
                Ok(mut engine) => {
                    // One stabilization pass at rest pose before first evaluate.
                    let mut values = saved_params.clone();
                    engine.stabilization(&mut PhysicsParams {
                        values: &mut values,
                        minimums: &mins,
                        maximums: &maxs,
                        defaults: &defaults,
                        names: &param_ids,
                    });
                    Some(engine)
                }
                Err(e) => {
                    bevy::log::warn!("live2d: physics disabled ({e})");
                    None
                }
            }
        }
        None => None,
    };

    let canvas = model.canvas_info();

    let mut expressions: HashMap<String, ExpressionMotion> = HashMap::new();
    for expr_ref in &model3.file_references.expressions {
        let bytes = match read_model_file(disk_dir, &expr_ref.file) {
            Ok(b) => b,
            Err(e) => {
                bevy::log::warn!("live2d: skip expression {}: {e}", expr_ref.file);
                continue;
            }
        };
        match parse_expression_json(&bytes) {
            Ok(parsed) => {
                expressions.insert(expr_ref.name.clone(), ExpressionMotion::new(parsed));
            }
            Err(e) => {
                bevy::log::warn!("live2d: skip expression {}: {e}", expr_ref.file);
            }
        }
    }
    bevy::log::info!("live2d expressions loaded: {}", expressions.len());

    Ok(Live2dPet {
        model,
        canvas,
        texture_paths: model3.file_references.textures.clone(),
        param_ids,
        param_lookup,
        part_lookup,
        mins,
        maxs,
        defaults,
        saved_params,
        saved_parts,
        queues,
        physics,
        breath: Breath::new(),
        idle_motion,
        sleep_motion,
        expressions,
        expression_manager: live2d_motion::ExpressionManager::new(),
    })
}
