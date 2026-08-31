// Probe: dump blend-class run structure along render order.
// Determines how many ping-pong passes the renderer needs for ARGNori.

use live2d_core::moc::Moc;
use live2d_core::model::Model;
use std::path::PathBuf;

fn main() {
    let moc_path = ["../../assets", "../../../assets", "assets"]
        .iter()
        .map(|c| PathBuf::from(c).join("nori/ARGNori_web/ARGNori.moc3"))
        .find(|p| p.is_file())
        .expect("ARGNori.moc3 not found");

    let moc = Moc::revive(&std::fs::read(&moc_path).unwrap()).unwrap();
    let mut model = Model::initialize(&moc).unwrap();
    model.update();

    let d = model.drawables();
    let n = d.len();
    let blend_modes = d.blend_modes().to_vec();
    let orders = model.render_orders();

    // Iterate source indices in draw order; label N (normal incl. masked-normal)
    // vs S (special: additive | multiplicative).
    let mut seq: Vec<(usize, char)> = Vec::with_capacity(n);
    let mut sorted: Vec<usize> = (0..n).collect();
    sorted.sort_by_key(|&i| orders[i]);
    for i in sorted {
        let cls = match blend_modes[i] {
            1 | 2 => 'S',
            _ => 'N',
        };
        seq.push((i, cls));
    }

    let mut runs: Vec<(char, usize, usize)> = Vec::new(); // (class, start_idx_in_sorted, len)
    for &(i, cls) in &seq {
        match runs.last_mut() {
            Some((c, _, len)) if *c == cls => *len += 1,
            _ => {
                let pos = runs.last().map(|(_, s, l)| s + l).unwrap_or(0);
                runs.push((cls, pos, 1));
                let _ = i;
            }
        }
    }

    println!("total drawables: {n}");
    println!("run count: {}", runs.len());
    for (k, (cls, start, len)) in runs.iter().enumerate() {
        let first_drawable = seq[*start].0;
        let last_drawable = seq[start + len - 1].0;
        println!(
            "  run[{k}] class={cls} len={len} drawables [{first_drawable}..={last_drawable}]"
        );
    }
    let special_runs = runs.iter().filter(|(c, _, _)| *c == 'S').count();
    println!("special (ping-pong) runs: {special_runs}");
}
