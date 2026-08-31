//! `embed-model` feature 专属：把 ARGNori_web 模型目录（moc3/physics/motions/expressions）
//! 编译期嵌入静态表。贴图不在此列（renderer 走 AssetServer，由嵌入资源源覆盖）。

use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_FEATURE_EMBED_MODEL").is_err() {
        return;
    }
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let model_dir = manifest.join("../../assets/nori/ARGNori_web");
    let mut files = Vec::new();
    visit(&model_dir, &model_dir, &mut files);
    if files.is_empty() {
        panic!("embed-model: no model files under {}", model_dir.display());
    }
    let mut out = String::from("pub static EMBEDDED_MODEL_FILES: &[(&str, &[u8])] = &[\n");
    for (rel, abs) in &files {
        println!("cargo:rerun-if-changed={}", abs.display());
        out.push_str(&format!("    ({rel:?}, include_bytes!({abs:?})),\n"));
    }
    out.push_str("];\n");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    std::fs::write(out_dir.join("embedded_model.rs"), out).unwrap();
}

fn visit(root: &Path, dir: &Path, files: &mut Vec<(String, PathBuf)>) {
    for e in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("embed-model: {}: {e}", dir.display())).flatten() {
        let p = e.path();
        if p.is_dir() {
            visit(root, &p, files);
        } else if is_model_file(&p) {
            let rel = p
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            files.push((rel, p));
        }
    }
}

/// loader 只读 moc3 与各类 *3.json；贴图（ARGNori.4096/）走 AssetServer，不在此嵌
fn is_model_file(p: &Path) -> bool {
    let name = p.to_string_lossy();
    p.extension().is_some_and(|ext| ext == "moc3")
        || [
            ".model3.json", ".physics3.json", ".cdi3.json",
            ".motion3.json", ".exp3.json",
        ]
        .iter()
        .any(|suffix| name.ends_with(suffix))
}
