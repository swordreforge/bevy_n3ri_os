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

    println!(
        "loaded: {} drawables, canvas {}x{}px @ {}ppu",
        pet.drawable_count(),
        pet.canvas.size_in_pixels.X,
        pet.canvas.size_in_pixels.Y,
        pet.canvas.pixels_per_unit
    );

    // Sample one drawable's first vertex over time — it must move.
    let probe_idx = 0;
    let sample_vertex = |pet: &Live2dPet| {
        let d = pet.model.drawables();
        let ptr = d.vertex_positions()[probe_idx];
        unsafe { (*ptr.add(0).cast::<f32>(), *ptr.add(1).cast::<f32>()) }
    };

    let (x0, y0) = sample_vertex(&pet);
    let dt = 1.0 / 60.0;
    let mut user_time = 0.0f32;
    for frame in 0..120 {
        user_time += dt;
        pet.tick(dt, user_time);
        if frame % 30 == 0 {
            let (x, y) = sample_vertex(&pet);
            println!("frame {frame:3}: probe vertex ({x:+.5}, {y:+.5})");
        }
    }
    let (x1, y1) = sample_vertex(&pet);

    let moved = ((x1 - x0).abs() + (y1 - y0).abs()) > 1e-4;
    assert!(moved, "model did not animate — motion pipeline is dead");

    // Physics sanity: hair params should have been written by physics outputs.
    let params = pet.model.parameters();
    let ids = params.ids();
    if let Some(pos) = ids.iter().position(|id| id.to_string_lossy().starts_with("ParamHairFront")) {
        println!("hair param idx {pos} value {}", params.values()[pos]);
    }

    println!("SMOKE OK");
}
