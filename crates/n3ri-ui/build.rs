//! `embed-content` feature 专属：把 files/mail/signal 的模拟内容目录
//! （本机家目录 / 邮件 / 聊天记录）编译期嵌入静态表，键为 assets 相对路径。
//! 图标（icon-a/b.png）走 AssetServer，由嵌入资源源覆盖，此处不排除（体积可忽略）。

use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_FEATURE_EMBED_CONTENT").is_err() {
        return;
    }
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let assets_dir = manifest.join("../../assets");
    let content_dirs = [
        "nori/app-icons/files",
        "nori/app-icons/mail",
        "nori/app-icons/signal",
    ];
    let mut files = Vec::new();
    for dir in content_dirs {
        visit(&assets_dir, &assets_dir.join(dir), &mut files);
    }
    if files.is_empty() {
        panic!("embed-content: no content files under {}", assets_dir.display());
    }
    files.sort();
    let mut out = String::from("pub static EMBEDDED_CONTENT_FILES: &[(&str, &[u8])] = &[\n");
    for (rel, abs) in &files {
        println!("cargo:rerun-if-changed={}", abs.display());
        out.push_str(&format!("    ({rel:?}, include_bytes!({abs:?})),\n"));
    }
    out.push_str("];\n");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    std::fs::write(out_dir.join("embedded_content.rs"), out).unwrap();
}

fn visit(root: &Path, dir: &Path, files: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            visit(root, &p, files);
        } else {
            let rel = p
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            files.push((rel, p));
        }
    }
}
