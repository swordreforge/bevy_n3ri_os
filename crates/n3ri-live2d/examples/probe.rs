// Probe: dump ARGNori drawable stats before designing the renderer.
// Run from repo root or examples dir; resolves assets like the main app.

use live2d_core::moc::Moc;
use live2d_core::model::Model;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn find_asset(rel: &str) -> Option<PathBuf> {
    let candidates = ["../../assets", "../../../assets", "assets"];
    for c in candidates {
        let p = PathBuf::from(c).join(rel);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

fn main() {
    let moc_path = find_asset("nori/ARGNori_web/ARGNori.moc3")
        .expect("ARGNori.moc3 not found — run from repo root");
    println!("moc3: {}", moc_path.display());

    let bytes = std::fs::read(&moc_path).expect("read moc3");
    println!("moc3 bytes: {}", bytes.len());

    let moc = Moc::revive(&bytes).expect("revive moc");
    let mut model = Model::initialize(&moc).expect("init model");

    // Initial deforming update so vertex data is valid.
    model.update();

    let canvas = model.canvas_info();
    println!(
        "canvas: {}x{} px, origin ({}, {}), pixels_per_unit {}",
        canvas.size_in_pixels.X,
        canvas.size_in_pixels.Y,
        canvas.origin_in_pixels.X,
        canvas.origin_in_pixels.Y,
        canvas.pixels_per_unit
    );

    let d = model.drawables();
    let count = d.len();
    println!("drawable count: {count}");

    let tex_idx = d.texture_indices();
    let mut tex_hist: BTreeMap<i32, usize> = BTreeMap::new();
    for t in tex_idx {
        *tex_hist.entry(*t).or_default() += 1;
    }
    println!("texture usage: {tex_hist:?}");

    let mask_counts = d.mask_counts();
    let masks = d.masks();
    let masked: Vec<usize> = (0..count).filter(|&i| mask_counts[i] > 0).collect();
    println!("masked drawables: {}/{}", masked.len(), count);
    for &i in masked.iter().take(20) {
        let mask_ids: &[i32] = unsafe { std::slice::from_raw_parts(masks[i], mask_counts[i] as usize) };
        let ids: Vec<String> = mask_ids.iter().map(|&m| m.to_string()).collect();
        println!("  drawable[{i}] id={} masks={:?}", i, ids);
    }

    let blend_modes = d.blend_modes();
    let mut bm_hist: BTreeMap<i32, usize> = BTreeMap::new();
    for b in blend_modes {
        *bm_hist.entry(*b).or_default() += 1;
    }
    println!("blend modes histogram: {bm_hist:?}");

    let vcounts = d.vertex_counts();
    let total_verts: i32 = vcounts.iter().sum();
    println!("total vertices: {total_verts}");
    println!(
        "vertex count range: min {} max {}",
        vcounts.iter().min().unwrap_or(&0),
        vcounts.iter().max().unwrap_or(&0)
    );

    // Bounding box of all vertex positions (normalized coords).
    let positions = d.vertex_positions();
    let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
    let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
    for i in 0..count {
        let ptr = positions[i];
        for v in 0..vcounts[i] as usize {
            let p = unsafe { *ptr.add(v) };
            min_x = min_x.min(p.X);
            min_y = min_y.min(p.Y);
            max_x = max_x.max(p.X);
            max_y = max_y.max(p.Y);
        }
    }
    println!("bbox x [{min_x:.4}, {max_x:.4}] y [{min_y:.4}, {max_y:.4}]");

    // Parameters.
    let params = model.parameters();
    println!("parameter count: {}", params.len());
    let ids = params.ids();
    let interesting = [
        "ParamAngleX",
        "ParamAngleY",
        "ParamAngleZ",
        "ParamBodyAngleX",
        "ParamBreath",
        "ParamEyeLOpen",
        "ParamEyeROpen",
        "ParamMouthOpenY",
    ];
    for want in interesting {
        match ids.iter().position(|id| id.to_string_lossy() == want) {
            Some(i) => println!("  {want} -> idx {i}"),
            None => println!("  {want} -> MISSING"),
        }
    }

    // Parts.
    println!("part count: {}", model.parts().len());

    // Render orders sanity.
    let orders = model.render_orders();
    println!("render_orders len: {}", orders.len());

    println!("PROBE OK");
}
