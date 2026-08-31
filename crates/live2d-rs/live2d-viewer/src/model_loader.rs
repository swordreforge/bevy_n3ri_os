use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Parsed `userdata3.json` — metadata attached to Parts/Drawables.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
#[serde(rename_all = "PascalCase")]
pub struct UserData3Json {
    pub version: u32,
    pub user_data_count: u32,
    pub total_user_data_size: u32,
    pub user_data: Vec<UserDataEntry>,
}

/// A single UserData entry mapping a target (Part/Drawable) to a description.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct UserDataEntry {
    pub target: String,
    pub id: String,
    pub value: String,
}

impl UserData3Json {
    /// Load and parse a userdata3.json file.
    pub fn from_file<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path.as_ref())?;
        Ok(serde_json::from_str(&content)?)
    }

    /// Build a lookup map: drawable/part ID → description text.
    pub fn to_map(&self) -> HashMap<String, String> {
        self.user_data
            .iter()
            .map(|e| (e.id.clone(), e.value.clone()))
            .collect()
    }
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
#[serde(rename_all = "PascalCase")]
pub struct Model3Json {
    pub version: u32,
    pub file_references: FileReferences,
    pub groups: Option<Vec<Group>>,
    pub hit_areas: Option<Vec<HitArea>>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
#[serde(rename_all = "PascalCase")]
pub struct FileReferences {
    pub moc: String,
    pub textures: Vec<String>,
    pub physics: Option<String>,
    pub pose: Option<String>,
    pub display_info: Option<String>,
    pub expressions: Option<Vec<ExpressionRef>>,
    pub motions: Option<HashMap<String, Vec<MotionRef>>>,
    pub user_data: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
#[serde(rename_all = "PascalCase")]
pub struct ExpressionRef {
    pub name: String,
    pub file: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
#[serde(rename_all = "PascalCase")]
pub struct MotionRef {
    pub file: String,
    pub fade_in_time: Option<f64>,
    pub fade_out_time: Option<f64>,
    pub sound: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[allow(dead_code)]
#[serde(rename_all = "PascalCase")]
pub struct Group {
    pub target: String,
    pub name: String,
    pub ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
#[serde(rename_all = "PascalCase")]
pub struct HitArea {
    pub id: String,
    pub name: String,
}

impl Model3Json {
    pub fn from_file<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path.as_ref())?;
        Ok(serde_json::from_str(&content)?)
    }

    pub fn texture_paths(&self) -> &[String] {
        &self.file_references.textures
    }

    pub fn moc3_path(&self) -> &str {
        &self.file_references.moc
    }
}

pub struct LoadedModel {
    pub model3_json: Model3Json,
    pub moc3_data: Vec<u8>,
    pub base_dir: PathBuf,
}

impl LoadedModel {
    pub fn load<P: AsRef<Path>>(model_dir: P) -> anyhow::Result<Self> {
        let model_dir = model_dir.as_ref();
        let model3_path = find_model3_json(model_dir)?;
        let base_dir = model3_path.parent().unwrap_or(model_dir).to_path_buf();

        let json = Model3Json::from_file(&model3_path)?;
        let moc3_path = base_dir.join(json.moc3_path());
        let moc3_data = std::fs::read(&moc3_path)?;

        Ok(Self {
            model3_json: json,
            moc3_data,
            base_dir,
        })
    }

    pub fn texture_paths(&self) -> Vec<PathBuf> {
        self.model3_json
            .texture_paths()
            .iter()
            .map(|p| self.base_dir.join(p))
            .collect()
    }
}

pub fn find_model3_json(dir: &Path) -> anyhow::Result<PathBuf> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            if let Some(name) = path.file_stem() {
                if name.to_string_lossy().ends_with(".model3") {
                    return Ok(path);
                }
            }
        }
    }
    anyhow::bail!("No *.model3.json found in {:?}", dir)
}

/// Find a specific model3.json file by filename within a directory.
pub fn find_model3_file(dir: &Path, filename: &str) -> anyhow::Result<PathBuf> {
    let path = dir.join(filename);
    if path.exists() {
        Ok(path)
    } else {
        anyhow::bail!("{} not found in {:?}", filename, dir)
    }
}

/// A single entry in a pose group: maps to a part, with optional linked parts
#[derive(Debug, Clone)]
pub struct PoseEntry {
    pub id: String,
    pub links: Vec<String>,
}

pub type PoseGroup = Vec<PoseEntry>;

#[derive(Debug, Clone)]
pub struct PoseData {
    pub fade_in_time: f32,
    pub groups: Vec<PoseGroup>,
}

pub fn parse_pose_json(data: &[u8]) -> anyhow::Result<PoseData> {
    let root: serde_json::Value = serde_json::from_slice(data)?;
    let fade_in_time = root
        .get("FadeInTime")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.5) as f32;

    let groups = root["Groups"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|group| {
                    group
                        .as_array()
                        .map(|entries| {
                            entries
                                .iter()
                                .map(|entry| PoseEntry {
                                    id: entry["Id"].as_str().unwrap_or_default().to_string(),
                                    links: entry["Link"]
                                        .as_array()
                                        .map(|links| {
                                            links
                                                .iter()
                                                .filter_map(|l| l.as_str().map(String::from))
                                                .collect()
                                        })
                                        .unwrap_or_default(),
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(PoseData {
        fade_in_time,
        groups,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use live2d_core::{Moc, Model};

    fn sdk_root_dir() -> PathBuf {
        if let Ok(val) = std::env::var("LIVE2D_SDK_ROOT") {
            return PathBuf::from(val);
        }
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("CubismSdkForNative-5-r.5")
    }

    #[test]
    fn dump_natori_offscreen() {
        let base = sdk_root_dir().join("Samples/Resources/Natori");
        let loaded = LoadedModel::load(&base).unwrap();
        let moc = Moc::revive(&loaded.moc3_data).unwrap();
        let moc_ptr: *const Moc = &moc as *const Moc;
        let mut model = unsafe { Model::initialize(&*moc_ptr) }.unwrap();
        model.update();

        // Check offscreen count
        let offscreens = model.offscreens();
        println!("=== Natori offscreen count: {} ===", offscreens.len());
        if !offscreens.is_empty() {
            let blend_modes = offscreens.blend_modes();
            let opacities = offscreens.opacities();
            let owner_indices = offscreens.owner_indices();
            let multiply_colors = offscreens.multiply_colors();
            let screen_colors = offscreens.screen_colors();
            let mask_counts = offscreens.mask_counts();
            let constant_flags = offscreens.constant_flags();
            for i in 0..offscreens.len() {
                println!("  Offscreen[{}]: blend={} op={:.3} owner={} mc=({:.3},{:.3},{:.3},{:.3}) sc=({:.3},{:.3},{:.3},{:.3}) masks={} flags={}",
                        i, blend_modes[i], opacities[i], owner_indices[i],
                        multiply_colors[i].X, multiply_colors[i].Y, multiply_colors[i].Z, multiply_colors[i].W,
                        screen_colors[i].X, screen_colors[i].Y, screen_colors[i].Z, screen_colors[i].W,
                        mask_counts[i], constant_flags[i]);
                if mask_counts[i] > 0 {
                    let masks_ptr = offscreens.masks();
                    let mask_slice = unsafe {
                        std::slice::from_raw_parts(masks_ptr[i], mask_counts[i] as usize)
                    };
                    println!("         mask indices: {:?}", mask_slice.to_vec());
                }
            }
        }

        // Check part offscreen indices
        let parts = model.parts();
        let pids = parts.ids();
        let pi = parts.offscreen_indices();
        println!("\n=== Parts with offscreen targets ===");
        for i in 0..pids.len() {
            if pi[i] >= 0 {
                let name = pids[i].to_string_lossy();
                println!("  Part[{}] '{}' -> offscreen index {}", i, name, pi[i]);
            }
        }
        println!(
            "  (total {} parts, {} with offscreen)",
            pids.len(),
            pi.iter().filter(|&&v| v >= 0).count()
        );

        // Check drawable parent parts for offscreen-rendered parts
        let drawables = model.drawables();
        let parent_parts = drawables.parent_part_indices();
        let ids = drawables.ids();
        let render_order = drawables.render_order_indices();

        println!("\n=== Drawables in offscreen-rendered parts ===");
        for (pos, &di) in render_order.iter().enumerate() {
            let pi_idx = parent_parts[di] as usize;
            let part_name = if pi_idx < pids.len() {
                pids[pi_idx].to_string_lossy()
            } else {
                "???".into()
            };
            let part_offscreen = if pi_idx < pi.len() { pi[pi_idx] } else { -1 };
            if part_offscreen >= 0 {
                println!(
                    "  [ro={pos}] #[{di}] {} part='{part_name}' offscreen={part_offscreen}",
                    ids[di].to_string_lossy()
                );
            }
        }
    }

    fn test_model_dir(name: &str) -> PathBuf {
        sdk_root_dir().join("Samples/Resources").join(name)
    }

    #[test]
    fn dump_mao_drawables() {
        let loaded = LoadedModel::load(test_model_dir("Mao")).expect("load Mao model");
        let moc = Moc::revive(&loaded.moc3_data).expect("revive moc");
        let moc_ptr: *const Moc = &moc as *const Moc;
        let mut model = unsafe { Model::initialize(&*moc_ptr) }.expect("init model");
        {
            let params = model.parameters();
            let default_vals = params.default_values().to_vec();
            let mut params = model.parameters();
            let mut vals = params.values_mut();
            for (i, &val) in default_vals.iter().enumerate() {
                vals.set(i, val);
            }
        }
        model.update();

        {
            let parts = model.parts();
            let pids = parts.ids();
            let popacs = parts.opacities();
            println!("\n=== Mao parts (n={}) ===", pids.len());
            for i in 0..pids.len() {
                let name = pids[i].to_string_lossy();
                if popacs[i] < 0.999
                    || name.contains("Arm")
                    || name.contains("Hand")
                    || name.contains("Wand")
                {
                    println!("  [{i:>2}] op={:.3} {}", popacs[i], name);
                }
            }
        }

        let drawables = model.drawables();
        let n = drawables.len();
        println!("=== Mao drawables (n={n}) ===");
        let ids = drawables.ids();
        let orders = drawables.draw_orders();
        let opacities = drawables.opacities();
        let dflags = drawables.dynamic_flags();
        let cflags = drawables.constant_flags();
        let mask_counts = drawables.mask_counts();
        let vcounts = drawables.vertex_counts();
        let mut sorted: Vec<_> = (0..n).collect();
        sorted.sort_by_key(|&i| orders[i]);

        println!("Render order / draw order / visibility / opacity / verts / masks / id");
        for (pos, &i) in sorted.iter().enumerate() {
            let vis = dflags[i] & live2d_core_sys::csmIsVisible as u8 != 0;
            let vis_s = if vis { 'V' } else { '_' };
            let is_inv = cflags[i] & live2d_core_sys::csmIsInvertedMask as u8 != 0;
            let inv_s = if is_inv { " INV" } else { "" };
            let order = orders[i];
            println!(
                "  [{pos:>3}] ro={order:>4} {vis_s} op={:.3} v={} m={}{inv_s} {}",
                opacities[i],
                vcounts[i],
                mask_counts[i],
                ids[i].to_string_lossy()
            );
        }
    }

    #[test]
    fn dump_rice_drawables() {
        let loaded = LoadedModel::load(test_model_dir("Rice")).expect("load Rice model");
        let moc = Moc::revive(&loaded.moc3_data).expect("revive moc");
        let moc_ptr: *const Moc = &moc as *const Moc;
        let mut model = unsafe { Model::initialize(&*moc_ptr) }.expect("init model");
        {
            let params = model.parameters();
            let default_vals = params.default_values().to_vec();
            let mut params = model.parameters();
            let mut vals = params.values_mut();
            for (i, &val) in default_vals.iter().enumerate() {
                vals.set(i, val);
            }
        }
        model.update();

        // Dump parameter default values
        {
            let params = model.parameters();
            let pids = params.ids();
            let default_vals = params.default_values();
            let vals = params.values();
            println!("\n=== Rice parameters (n={}) ===", pids.len());
            for i in 0..pids.len() {
                let name = pids[i].to_string_lossy();
                println!(
                    "  {}: default={:.3} current={:.3}",
                    name, default_vals[i], vals[i]
                );
            }
        }

        let drawables = model.drawables();
        let n = drawables.len();
        println!("=== Rice drawables (n={n}) ===");
        let ids = drawables.ids();
        let orders = drawables.draw_orders();
        let opacities = drawables.opacities();
        let dflags = drawables.dynamic_flags();
        let cflags = drawables.constant_flags();
        let mask_counts = drawables.mask_counts();
        let _vcounts = drawables.vertex_counts();
        let tex_indices = drawables.texture_indices();
        let mul_colors = drawables.multiply_colors();
        let scr_colors = drawables.screen_colors();
        let parent_parts = drawables.parent_part_indices();

        let parts = model.parts();
        let part_ids: Vec<String> = parts
            .ids()
            .iter()
            .map(|id| id.to_string_lossy().into_owned())
            .collect();

        let render_order = drawables.render_order_indices();

        println!("ro / #[id] / draw_order / vis / op / tex / masks / mul/screen / parent / name");
        for (pos, &i) in render_order.iter().enumerate() {
            let vis = dflags[i] & live2d_core_sys::csmIsVisible as u8 != 0;
            let vis_s = if vis { 'V' } else { '_' };
            let is_inv = cflags[i] & live2d_core_sys::csmIsInvertedMask as u8 != 0;
            let inv_s = if is_inv { " INV" } else { "" };
            let pi = parent_parts[i] as usize;
            let pname = if pi < part_ids.len() {
                &part_ids[pi]
            } else {
                "???"
            };
            let mc = mul_colors[i];
            let sc = scr_colors[i];
            println!("  [{pos:>3}] #[{i:>3}] ro={:>4} {vis_s} op={:.3} t={} m={}{inv_s} mc=({:.3},{:.3},{:.3}) sc=({:.3},{:.3},{:.3}) {pname} {}",
                orders[i], opacities[i], tex_indices[i], mask_counts[i],
                mc.X, mc.Y, mc.Z, sc.X, sc.Y, sc.Z,
                ids[i].to_string_lossy());
            if mask_counts[i] > 0 {
                let masks_ptr = drawables.masks();
                let mask_slice =
                    unsafe { std::slice::from_raw_parts(masks_ptr[i], mask_counts[i] as usize) };
                println!(
                    "         masks: {:?}",
                    mask_slice.iter().map(|&m| m as usize).collect::<Vec<_>>()
                );
            }
        }
    }

    #[test]
    fn dump_natori_drawables() {
        let _ = env_logger::builder()
            .is_test(true)
            .filter_level(log::LevelFilter::Info)
            .try_init();

        let base = sdk_root_dir().join("Samples/Resources/Natori");
        let loaded = LoadedModel::load(&base).unwrap();

        let moc = Moc::revive(&loaded.moc3_data).unwrap();
        let moc_ptr: *const Moc = &moc as *const Moc;
        let mut model = unsafe { Model::initialize(&*moc_ptr) }.unwrap();
        {
            let params = model.parameters();
            let default_vals = params.default_values().to_vec();
            let mut params = model.parameters();
            let mut vals = params.values_mut();
            for (i, &val) in default_vals.iter().enumerate() {
                vals.set(i, val);
            }
        }

        if let Some(ref pose_path) = loaded.model3_json.file_references.pose {
            let pose_data = std::fs::read(base.join(pose_path)).unwrap();
            let pose = parse_pose_json(&pose_data).unwrap();
            println!("\n=== Natori pose groups ===");
            for (gi, group) in pose.groups.iter().enumerate() {
                println!("  Group {gi}:");
                for entry in group {
                    println!("    id={}  links={:?}", entry.id, entry.links);
                }
            }

            let mut parts = model.parts();
            let pids: Vec<String> = parts
                .ids()
                .iter()
                .map(|id| id.to_string_lossy().into_owned())
                .collect();
            let popac = parts.opacities_mut();
            for group in &pose.groups {
                let mut first_found = false;
                for entry in group {
                    if let Some(part_idx) = pids.iter().position(|id| id == &entry.id) {
                        if !first_found {
                            popac[part_idx] = 1.0;
                            first_found = true;
                        } else {
                            popac[part_idx] = 0.0;
                        }
                    }
                }
            }
            model.update();
        }

        println!("\n=== Natori parameter default values ===");
        {
            let params = model.parameters();
            let pids = params.ids();
            let default_vals = params.default_values();
            for i in 0..pids.len() {
                let name = pids[i].to_string_lossy();
                println!("  {} = {:.3}", name, default_vals[i]);
            }
        }

        {
            let parts = model.parts();
            let pids = parts.ids();
            let popacs = parts.opacities();
            println!("\n=== Natori parts (n={}) ===", pids.len());
            for i in 0..pids.len() {
                let name = pids[i].to_string_lossy();
                if popacs[i] < 0.999 {
                    println!("  [{i:>2}] op={:.3} {}", popacs[i], name);
                }
            }
            for i in 0..pids.len() {
                let name = pids[i].to_string_lossy();
                if name.contains("Face")
                    || name.contains("Head")
                    || name.contains("Eye")
                    || name.contains("Mouth")
                    || name.contains("Nose")
                    || name.contains("Ear")
                    || name.contains("Hood")
                {
                    println!("  [{i:>2}] op={:.3} {}", popacs[i], name);
                }
            }
        }

        let drawables = model.drawables();
        let n = drawables.len();
        println!("\n=== Natori drawables (n={n}) ===");
        let ids = drawables.ids();
        let orders = drawables.draw_orders();
        let opacities = drawables.opacities();
        let dflags = drawables.dynamic_flags();
        let cflags = drawables.constant_flags();
        let mask_counts = drawables.mask_counts();
        let vcounts = drawables.vertex_counts();
        let parent_parts = drawables.parent_part_indices();
        let tex_indices = drawables.texture_indices();

        let parts = model.parts();
        let part_ids: Vec<String> = parts
            .ids()
            .iter()
            .map(|id| id.to_string_lossy().into_owned())
            .collect();

        let render_order = drawables.render_order_indices();

        println!("Render order (render_order_indices) / draw_order / vis / opacity / verts / masks / tex / parent / id");
        for (pos, &i) in render_order.iter().enumerate() {
            let vis = dflags[i] & live2d_core_sys::csmIsVisible as u8 != 0;
            let vis_s = if vis { 'V' } else { '_' };
            let is_inv = cflags[i] & live2d_core_sys::csmIsInvertedMask as u8 != 0;
            let inv_s = if is_inv { " INV" } else { "" };
            let order = orders[i];
            let pi = parent_parts[i] as usize;
            let pname = if pi < part_ids.len() {
                &part_ids[pi]
            } else {
                "???"
            };
            let tex = tex_indices[i];
            println!("  [{pos:>3}] #[{i:>3}] ro={order:>4} {vis_s} op={:.3} v={} m={}{inv_s} t={tex} {pname} {}",
                opacities[i], vcounts[i], mask_counts[i], ids[i].to_string_lossy());
            if mask_counts[i] > 0 {
                let masks_ptr = drawables.masks();
                let mask_slice =
                    unsafe { std::slice::from_raw_parts(masks_ptr[i], mask_counts[i] as usize) };
                println!(
                    "         masks: {:?}",
                    mask_slice.iter().map(|&m| m as usize).collect::<Vec<_>>()
                );
            }
        }

        // Dump vertex bounding boxes for PartHead drawables
        let vert_positions = drawables.vertex_positions();
        println!("\n=== PartHead drawable bounding boxes (model coordinates) ===");
        for &i in render_order.iter() {
            let pi = parent_parts[i] as usize;
            let pname = if pi < part_ids.len() {
                &part_ids[pi]
            } else {
                "???"
            };
            if pname != "PartHead" {
                continue;
            }
            let vc = vcounts[i] as usize;
            let pos_slice = unsafe { std::slice::from_raw_parts(vert_positions[i], vc) };
            let mut min_x = f32::MAX;
            let mut min_y = f32::MAX;
            let mut max_x = f32::MIN;
            let mut max_y = f32::MIN;
            for pos in pos_slice {
                let x = pos.X;
                let y = pos.Y;
                if x < min_x {
                    min_x = x;
                }
                if y < min_y {
                    min_y = y;
                }
                if x > max_x {
                    max_x = x;
                }
                if y > max_y {
                    max_y = y;
                }
            }
            let w = max_x - min_x;
            let h = max_y - min_y;
            println!(
                "  #[{i:>3}] {} v={}: x=[{:.1},{:.1}] y=[{:.1},{:.1}] w={:.1} h={:.1} op={:.3}",
                ids[i].to_string_lossy(),
                vc,
                min_x,
                max_x,
                min_y,
                max_y,
                w,
                h,
                opacities[i]
            );
        }

        // Dump ALL drawables with their bounding boxes sorted by y-center
        // This helps identify what covers the face area
        println!("\n=== All drawable bounding boxes (model coords, sorted by Y center) ===");
        #[allow(clippy::type_complexity)]
        let mut bboxes: Vec<(usize, f32, f32, f32, f32, f32, f32, f32)> = Vec::new();
        for &i in render_order.iter() {
            let vc = vcounts[i] as usize;
            let pos_slice = unsafe { std::slice::from_raw_parts(vert_positions[i], vc) };
            let mut min_x = f32::MAX;
            let mut min_y = f32::MAX;
            let mut max_x = f32::MIN;
            let mut max_y = f32::MIN;
            for pos in pos_slice {
                let x = pos.X;
                let y = pos.Y;
                if x < min_x {
                    min_x = x;
                }
                if y < min_y {
                    min_y = y;
                }
                if x > max_x {
                    max_x = x;
                }
                if y > max_y {
                    max_y = y;
                }
            }
            let w = max_x - min_x;
            let h = max_y - min_y;
            let cy = (min_y + max_y) / 2.0;
            bboxes.push((i, min_x, max_x, min_y, max_y, w, h, cy));
        }
        bboxes.sort_by(|a, b| a.7.partial_cmp(&b.7).unwrap());
        for (i, min_x, max_x, min_y, max_y, w, h, _cy) in &bboxes {
            let pi = parent_parts[*i] as usize;
            let pname = if pi < part_ids.len() {
                &part_ids[pi]
            } else {
                "???"
            };
            let vis = if dflags[*i] & live2d_core_sys::csmIsVisible as u8 != 0 {
                "V"
            } else {
                "_"
            };
            if *h > 0.05 || *w > 0.05 {
                // Skip tiny details
                println!("  #[{i:>3}] {vis} op={:.3} {} v={}: x=[{:.2},{:.2}] y=[{:.2},{:.2}] w={:.2} h={:.2}",
                    opacities[*i], pname, vcounts[*i], min_x, max_x, min_y, max_y, w, h);
            }
        }

        // Compare csmGetRenderOrders() vs sorted-by-draw-order
        {
            let render_orders_from_core = model.render_orders();
            let our_sorted = model.drawables().render_order_indices();
            let mut mismatches = 0;
            for (pos, (&core_idx, &our_idx)) in render_orders_from_core
                .iter()
                .zip(our_sorted.iter())
                .enumerate()
            {
                if core_idx as usize != our_idx {
                    mismatches += 1;
                    if mismatches <= 20 {
                        let core_id = drawables.ids()[core_idx as usize].to_string_lossy();
                        let our_id = drawables.ids()[our_idx].to_string_lossy();
                        println!("  MISMATCH [{pos}]: core={core_idx} ({core_id}) vs our={our_idx} ({our_id})");
                    }
                }
            }
            if mismatches == 0 {
                println!("  Render order MATCHES csmGetRenderOrders()");
            } else {
                println!("  Render order MISMATCHES in {mismatches}/{n} positions");
            }
        }
    }
}
