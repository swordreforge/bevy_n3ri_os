// Headless smoke test: load ARGNori, tick the animation pipeline for ~2s of
// frames, and verify the model actually animates (vertices move).
// Run: cargo run -p n3ri-live2d --example smoke

use n3ri_live2d::pet::Live2dPet;

fn main() {
    let mut pet = match n3ri_live2d::loader::load_pet() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("load failed: {e}");
            std::process::exit(1);
        }
    };

    let canvas = pet.runtime.canvas();
    println!(
        "loaded: {} drawables, canvas {}x{} @ {}ppu",
        pet.drawable_count(),
        canvas.width(),
        canvas.height(),
        canvas.pixels_per_unit()
    );

    // Sample one drawable's first vertex over time — it must move.
    let sample_vertex = |pet: &Live2dPet| {
        pet.runtime.meshes()[0]
            .vertices()
            .first()
            .map(|v| v.position())
            .unwrap_or([f32::NAN; 2])
    };

    let [x0, y0] = sample_vertex(&pet);
    let dt = 1.0 / 60.0;
    for frame in 0..120 {
        pet.tick(dt);
        if frame % 30 == 0 {
            let [x, y] = sample_vertex(&pet);
            println!("frame {frame:3}: probe vertex ({x:+.5}, {y:+.5})");
        }
    }
    let [x1, y1] = sample_vertex(&pet);

    let moved = ((x1 - x0).abs() + (y1 - y0).abs()) > 1e-4;
    assert!(moved, "model did not animate — motion pipeline is dead");

    // Physics sanity: a physics-OUTPUT param should have been written.
    let ids = pet.runtime.parameter_ids();
    for want in ["ParamHair1X1", "ParamMouthPhysicY"] {
        if let Some(pos) = ids.iter().position(|id| id == want) {
            println!(
                "physics param {want} idx {pos} value {}",
                pet.runtime.parameter_value_by_index(pos).unwrap_or(f32::NAN)
            );
        }
    }

    println!("SMOKE OK");
}
