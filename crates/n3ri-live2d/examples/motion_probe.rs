// Motion probe: drawable/part opacity under default vs idle vs sleep motions.
use live2d_core::moc::Moc;
use live2d_core::model::Model;
use live2d_motion::json::parse_motion_json;
use live2d_motion::motion::CubismMotion;
use live2d_motion::queue::MotionQueueManager;
use std::collections::HashMap;
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

fn main() {
    let moc_path = find_asset("nori/ARGNori_web/ARGNori.moc3").expect("moc3");
    let bytes = std::fs::read(&moc_path).expect("read");
    let moc: &'static _ = Box::leak(Box::new(Moc::revive(&bytes).expect("revive")));
    let mut model = Model::initialize(moc).expect("init");

    let params = model.parameters();
    let param_lookup: HashMap<String, usize> = params
        .ids()
        .iter()
        .enumerate()
        .map(|(i, id)| (id.to_string_lossy().into_owned(), i))
        .collect();
    let defaults = params.default_values().to_vec();

    model.update();
    let d0 = model.drawables();
    let default_ops = d0.opacities().to_vec();
    let blend = d0.blend_modes().to_vec();
    let mask_counts = d0.mask_counts().to_vec();
    let ids: Vec<String> = d0
        .ids()
        .iter()
        .map(|i| i.to_string_lossy().into_owned())
        .collect();
    let interesting: Vec<usize> = (0..d0.len())
        .filter(|&i| blend[i] != 0 || mask_counts[i] > 0)
        .collect();

    let mut queue = MotionQueueManager::new();
    let idle_path = find_asset("nori/ARGNori_web/motions/01_Idle_Loop.motion3.json").expect("idle");
    let idle = CubismMotion::new(
        parse_motion_json(&std::fs::read(idle_path).expect("read")).expect("parse"),
        5.0,
        0.5,
    );
    queue.start_motion(idle, None);

    let mut saved_params = defaults.clone();
    let mut saved_parts = model.parts().opacities().to_vec();

    let mut tick = |model: &mut Model, queue: &mut MotionQueueManager, dt: f32| {
        queue.advance_time(dt);
        {
            let mut params = model.parameters();
            let mut parts = model.parts();
            let mut vals = params.values_mut();
            let pops = parts.opacities_mut();
            let v = vals.as_mut_slice();
            v.copy_from_slice(&saved_params);
            pops.copy_from_slice(&saved_parts);
            let empty: Vec<String> = vec![];
            queue.do_update_motion(&param_lookup, v, &empty, &empty, &HashMap::new(), pops);
        }
        model.reset_dynamic_flags();
        model.update();
        saved_params = model.parameters().values().to_vec();
        saved_parts = model.parts().opacities().to_vec();
    };

    for _ in 0..60 {
        tick(&mut model, &mut queue, 0.1);
    }
    println!("--- after 6s IDLE (changes vs default pose) ---");
    {
        let d = model.drawables();
        let ops = d.opacities();
        for &i in &interesting {
            if (ops[i] - default_ops[i]).abs() > 0.01 {
                println!(
                    "  [{i}] {} opacity {:.3} -> {:.3} (blend {})",
                    ids[i], default_ops[i], ops[i], blend[i]
                );
            }
        }
        let parts = model.parts();
        let dim: Vec<String> = parts
            .ids()
            .iter()
            .zip(parts.opacities())
            .enumerate()
            .filter(|(_, (_, &o))| o < 0.999)
            .map(|(i, (id, &o))| format!("[{i}]{}={:.2}", id.to_string_lossy(), o))
            .collect();
        println!(
            "  parts opacity<1: {}",
            if dim.is_empty() { "none".into() } else { dim.join(" ") }
        );
    }

    let sleep_path = find_asset("nori/ARGNori_web/motions/sleep_Loop.motion3.json").expect("sleep");
    let sleep = CubismMotion::new(
        parse_motion_json(&std::fs::read(sleep_path).expect("read")).expect("parse"),
        10.0,
        0.5,
    );
    queue.stop_all_motions();
    queue.start_motion(sleep, None);
    for _ in 0..100 {
        tick(&mut model, &mut queue, 0.1);
    }
    println!("--- after 10s SLEEP (changes vs default pose) ---");
    {
        let d = model.drawables();
        let ops = d.opacities();
        for &i in &interesting {
            if (ops[i] - default_ops[i]).abs() > 0.01 {
                println!(
                    "  [{i}] {} opacity {:.3} -> {:.3} (blend {})",
                    ids[i], default_ops[i], ops[i], blend[i]
                );
            }
        }
        let parts = model.parts();
        let dim: Vec<String> = parts
            .ids()
            .iter()
            .zip(parts.opacities())
            .enumerate()
            .filter(|(_, (_, &o))| o < 0.999)
            .map(|(i, (id, &o))| format!("[{i}]{}={:.2}", id.to_string_lossy(), o))
            .collect();
        println!(
            "  parts opacity<1: {}",
            if dim.is_empty() { "none".into() } else { dim.join(" ") }
        );
    }
    println!("MOTION PROBE OK");
}
