// Flags probe: dump per-drawable constant flags + mask relations for ARGNori.
// Diagnostic for the inverted-mask rendering issue.

use live2d_core::moc::Moc;
use live2d_core::model::Model;
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
    let moc_path = find_asset("nori/ARGNori_web/ARGNori.moc3").expect("ARGNori.moc3 not found");
    let bytes = std::fs::read(&moc_path).expect("read moc3");
    let moc = Moc::revive(&bytes).expect("revive moc");
    let mut model = Model::initialize(&moc).expect("init model");
    model.update();

    let d = model.drawables();
    let count = d.len();
    let ids = d.ids();
    let const_flags = d.constant_flags();
    let mask_counts = d.mask_counts();
    let masks = d.masks();
    let _blend_modes = d.blend_modes();
    let opacities = d.opacities();
    let render_orders = model.render_orders();
    let tex_idx = d.texture_indices();
    let vcounts = d.vertex_counts();
    let positions = d.vertex_positions();

    // Which drawables are used as mask sources by others?
    let mut is_mask_source = vec![false; count];
    for i in 0..count {
        if mask_counts[i] > 0 {
            let slice =
                unsafe { std::slice::from_raw_parts(masks[i], mask_counts[i] as usize) };
            for &m in slice {
                if m >= 0 && (m as usize) < count {
                    is_mask_source[m as usize] = true;
                }
            }
        }
    }

    println!(
        "{:>4} {:<14} {:>6} {:>5} {:>4} {:>3} {:>5} {:>7}  {:>18}  flags",
        "idx", "id", "cflags", "bmask", "nmsk", "tex", "order", "opacity", "bbox-center(x,y)"
    );
    for i in 0..count {
        let cf = const_flags[i];
        let n = (vcounts[i].max(0) as usize).max(1);
        let (mut sx, mut sy) = (0.0f32, 0.0f32);
        let ptr = positions[i];
        for v in 0..vcounts[i].max(0) as usize {
            sx += unsafe { *ptr.add(v) }.X;
            sy += unsafe { *ptr.add(v) }.Y;
        }
        let (cx, cy) = (sx / n as f32, sy / n as f32);

        // Only print drawables that are interesting: masked, mask sources,
        // or have any constant flag bits set.
        let interesting =
            mask_counts[i] > 0 || is_mask_source[i] || cf != 0;
        if !interesting {
            continue;
        }

        let mut flag_str = String::new();
        if cf & 1 != 0 {
            flag_str.push_str("Additive ");
        }
        if cf & 2 != 0 {
            flag_str.push_str("Multiplicative ");
        }
        if cf & 4 != 0 {
            flag_str.push_str("DoubleSided ");
        }
        if cf & 8 != 0 {
            flag_str.push_str("INVERTED_MASK ");
        }

        let mask_ids: Vec<i32> = if mask_counts[i] > 0 {
            unsafe { std::slice::from_raw_parts(masks[i], mask_counts[i] as usize) }.to_vec()
        } else {
            vec![]
        };

        println!(
            "{:>4} {:<14} {:>6} {:>5} {:>4} {:>3} {:>5} {:>7.3}  {:>8.3},{:>8.3}  {} masks={:?}",
            i,
            ids[i].to_string_lossy(),
            format!("{cf:#04x}"),
            if is_mask_source[i] { "SRC" } else { "-" },
            mask_counts[i],
            tex_idx[i],
            render_orders[i],
            opacities[i],
            cx,
            cy,
            flag_str.trim_end(),
            mask_ids,
        );
    }
    println!("FLAGS PROBE OK");

    // UV bboxes for the inverted-mask drawables and their mask source, so the
    // texture regions can be cropped and inspected visually.
    let uvs = d.vertex_uvs();
    for i in [91usize, 204, 224] {
        let n = vcounts[i].max(0) as usize;
        let ptr = uvs[i];
        let (mut min_u, mut min_v) = (f32::MAX, f32::MAX);
        let (mut max_u, mut max_v) = (f32::MIN, f32::MIN);
        for v in 0..n {
            let p = unsafe { *ptr.add(v) };
            min_u = min_u.min(p.X);
            min_v = min_v.min(p.Y);
            max_u = max_u.max(p.X);
            max_v = max_v.max(p.Y);
        }
        println!(
            "uv bbox drawable[{i}] {}: u [{min_u:.4}, {max_u:.4}] v [{min_v:.4}, {max_v:.4}]",
            ids[i].to_string_lossy()
        );
    }
}
