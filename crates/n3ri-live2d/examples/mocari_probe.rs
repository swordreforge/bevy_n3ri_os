// mocari probe: load ARGNori via pure-rust mocari 0.4.0 and dump the same
// signals the FFI path exposes (drawable ids/blend/mask/opacity, vertex motion,
// physics params), plus the Y/UV-orientation question.
// Run: cargo run -p n3ri-live2d --example mocari_probe
//
// NOTE: mocari is an optional dev-only dep, no wgpu feature (avoids bevy wgpu clash).

use mocari::{
    assets::load_model_runtime,
    motion::{MotionPlayer, load_motion},
    render::common::{DrawableInfo, draw_order_indices},
};
use std::path::PathBuf;

fn find_asset(rel: &str) -> Option<PathBuf> {
    for c in ["../../assets", "../../../assets", "assets"] {
        let p = PathBuf::from(c).join(rel);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

fn dump_drawables(tag: &str, model: &mocari::ModelRuntime) {
    let infos: Vec<DrawableInfo> = model.meshes().iter().map(DrawableInfo::from_mesh).collect();
    let order = draw_order_indices(&infos);
    let masked = infos.iter().filter(|d| !d.masks().is_empty()).count();
    let inverted = infos.iter().filter(|d| d.inverted_mask()).count();
    let blend_n: usize = infos
        .iter()
        .filter(|d| matches!(d.blend_mode(), mocari::moc3::Moc3DrawableBlendMode::Normal))
        .count();
    let blend_a: usize = infos
        .iter()
        .filter(|d| matches!(d.blend_mode(), mocari::moc3::Moc3DrawableBlendMode::Additive))
        .count();
    let blend_m: usize = infos
        .iter()
        .filter(|d| matches!(d.blend_mode(), mocari::moc3::Moc3DrawableBlendMode::Multiplicative))
        .count();
    println!(
        "[{tag}] drawables={} normal/add/mul={blend_n}/{blend_a}/{blend_m} masked={masked} inverted={inverted}",
        infos.len()
    );
    println!("[{tag}] first 5 in draw order:");
    for &i in order.iter().take(5) {
        let m = &model.meshes()[i];
        let v0 = m.vertices().first().map(|v| v.position()).unwrap_or([f32::NAN; 2]);
        let uv0 = m.vertices().first().map(|v| v.uv()).unwrap_or([f32::NAN; 2]);
        println!(
            "  [{i}] order={:.1}/{:+} tex={} op={:.3} nverts={} nidx={} masks={:?} v0=({:+.4},{:+.4}) uv0=({:.4},{:.4})",
            m.draw_order(),
            m.render_order(),
            m.texture_index(),
            m.opacity(),
            m.vertices().len(),
            m.indices().len(),
            m.masks(),
            v0[0],
            v0[1],
            uv0[0],
            uv0[1],
        );
    }
    // bbox of all vertices (model space)
    let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
    let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
    for m in model.meshes() {
        for v in m.vertices() {
            let [x, y] = v.position();
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }
    println!("[{tag}] bbox x=[{min_x:+.3},{max_x:+.3}] y=[{min_y:+.3},{max_y:+.3}]");
}

fn main() {
    let model3 = find_asset("nori/ARGNori_web/ARGNori.model3.json").expect("model3.json");
    let mut model = load_model_runtime(&model3).expect("load_model_runtime");
    let rt = model.runtime_mut();

    let canvas = rt.canvas();
    println!(
        "canvas w={:.3} h={:.3} origin=({:.3},{:.3}) ppu={:.3} reverse_y={}",
        canvas.width(),
        canvas.height(),
        canvas.origin_x(),
        canvas.origin_y(),
        canvas.pixels_per_unit(),
        canvas.reverse_y_coordinate(),
    );
    for (i, t) in model.textures().iter().enumerate() {
        println!("  tex[{i}] {}x{}", t.width(), t.height());
    }
    let param_count = model.runtime().parameter_ids().len();
    let part_count = model.runtime().part_ids().len();
    let drawable_count = model.runtime().meshes().len();
    let tex_count = model.textures().len();
    println!("params={param_count} parts={part_count} drawables={drawable_count} textures={tex_count}");
    // motion refs from model3.json
    for (group, refs) in model.runtime().model().motions() {
        for r in refs {
            println!("  motion group [{group}] {}", r.file());
        }
    }

    let rt = model.runtime_mut();
    dump_drawables("default", rt);

    // Physics: check a physics-OUTPUT param (not motion-driven) moves.
    // ARGNori physics outputs use ParamHair1X* / ParamMouthPhysic* ids.
    let phys_ids = ["ParamHair1X1", "ParamMouthPhysicY", "Param121"];
    let phys_before: Vec<(String, Option<f32>)> = phys_ids
        .iter()
        .map(|id| (id.to_string(), rt.parameter_value(id)))
        .collect();
    println!("physics outputs before ticks: {phys_before:?}");
    // Probe vertex of drawable 0 for motion
    let probe = |rt: &mocari::ModelRuntime| {
        rt.meshes()[0]
            .vertices()
            .first()
            .map(|v| v.position())
            .unwrap_or([f32::NAN; 2])
    };
    let p0 = probe(rt);
    println!("probe d0 v0 = ({:+.5}, {:+.5})", p0[0], p0[1]);

    // Idle motion — model3 declares FadeIn 5.0 / FadeOut 0.5 for 01_Idle_Loop
    let dir = model3.parent().unwrap().to_path_buf();
    let idle_path = dir.join("motions/01_Idle_Loop.motion3.json");
    let motion = load_motion(&idle_path).expect("load idle");
    println!(
        "idle meta: duration={} fps={} loop={} curves={}",
        motion.meta().duration(),
        motion.meta().fps(),
        motion.meta().is_looping(),
        motion.curves().len(),
    );
    let mut player = MotionPlayer::new(motion);

    let dt = 1.0 / 60.0;
    for frame in 0..120 {
        player.tick(dt);
        player.apply(rt);
        rt.apply_physics(dt);
        rt.update_meshes();
        if frame % 30 == 0 {
            let p = probe(rt);
            println!("frame {frame:3}: probe d0 v0 ({p0:+.5}, {p1:+.5})", p0 = p[0], p1 = p[1]);
        }
    }
    let p1 = probe(rt);
    let moved = (p1[0] - p0[0]).abs() + (p1[1] - p0[1]).abs();
    println!("moved={moved:.6} (must be > 1e-4)");
    assert!(moved > 1e-4, "model did not animate under mocari");

    let phys_after: Vec<(String, Option<f32>)> = phys_ids
        .iter()
        .map(|id| (id.to_string(), rt.parameter_value(id)))
        .collect();
    println!("physics outputs after 2s idle: {phys_after:?}");

    dump_drawables("after-idle-2s", rt);

    // Physics: hair param should be driven
    if let Some(idx) = rt.parameter_ids().iter().position(|id| id.starts_with("ParamHairFront")) {
        println!(
            "hair param idx {idx} id={} value={}",
            rt.parameter_ids()[idx],
            rt.parameter_value_by_index(idx).unwrap_or(f32::NAN)
        );
    } else {
        println!("WARN: no ParamHairFront param found");
    }

    // Expression: 07_Smile exp3
    let exp_src =
        std::fs::read_to_string(dir.join("expressions/07_Smile.exp3.json")).expect("read smile");
    let exp = mocari::json::Expression3::from_json_str(&exp_src).expect("parse smile");
    let mut mgr = mocari::ExpressionManager::new();
    mgr.play(exp);
    for _ in 0..30 {
        mgr.tick(dt);
        mgr.apply(rt);
        rt.update_meshes();
    }
    println!(
        "after smile expr: eye-smile ParamEyeSmile={:?}",
        rt.parameter_value("ParamEyeSmile")
    );

    // Hit areas come from model3.json HitAreas — ARGNori declares none
    println!("hit_areas={:?}", rt.model().hit_areas());
    println!("MOCARI PROBE OK");
}
