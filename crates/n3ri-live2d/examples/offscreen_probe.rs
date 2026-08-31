// Probe offscreen surfaces of ARGNori (Cubism 5.3+/6 feature).
use live2d_core::moc::Moc;
use live2d_core::model::Model;
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
    let moc_path = find_asset("nori/ARGNori_web/ARGNori.moc3").expect("moc3 not found");
    let bytes = std::fs::read(&moc_path).expect("read");
    let moc = Moc::revive(&bytes).expect("revive");
    let mut model = Model::initialize(&moc).expect("init");
    model.update();

    let off = model.offscreens();
    println!("offscreen count: {}", off.len());
    let blend = off.blend_modes();
    let opac = off.opacities();
    let owners = off.owner_indices();
    let mask_counts = off.mask_counts();
    let cflags = off.constant_flags();
    for i in 0..off.len() {
        println!(
            "offscreen[{i}] owner={} blend={} opacity={:.3} mask_count={} cflags={:#04x}",
            owners[i], blend[i], opac[i], mask_counts[i], cflags[i]
        );
    }

    let all_orders = model.render_orders();
    let n_draw = model.drawables().len();
    println!(
        "render_orders len={} drawables={} (offscreen entries: {})",
        all_orders.len(),
        n_draw,
        all_orders.len() - n_draw
    );

    let d = model.drawables();
    let parent = d.parent_part_indices();
    let owners_set: Vec<i32> = owners.to_vec();
    let mut in_offscreen_parts: Vec<usize> = vec![];
    for (d, &p) in parent.iter().enumerate() {
        if owners_set.contains(&p) {
            in_offscreen_parts.push(d);
        }
    }
    println!(
        "drawables whose parent part OWNS an offscreen: {:?}",
        in_offscreen_parts
    );
    println!("OFFSCREEN PROBE OK");
}
