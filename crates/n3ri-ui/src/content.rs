//! 虚拟内容层：files/mail/signal 的模拟内容读取。
//!
//! 路径一律是 assets 相对路径（如 `nori/app-icons/files/本机/文稿/读书.txt`）。
//! 磁盘优先（开发期可热改内容），`embed-content` feature 下回退编译期内置表；
//! 绝对路径（如 std::env::temp_dir 下的临时文件）直接透传给磁盘。

use std::path::{Path, PathBuf};

#[cfg(feature = "embed-content")]
const CONTENT_DIRS: [&str; 3] = [
    "nori/app-icons/files",
    "nori/app-icons/mail",
    "nori/app-icons/signal",
];

fn assets_root() -> Option<PathBuf> {
    for c in ["assets", "../assets", "../../assets", "../../../assets"] {
        let p = PathBuf::from(c);
        if p.is_dir() {
            return Some(p);
        }
    }
    None
}

#[cfg(feature = "embed-content")]
mod embedded {
    include!(concat!(env!("OUT_DIR"), "/embedded_content.rs"));

    pub fn lookup(rel: &str) -> Option<&'static [u8]> {
        EMBEDDED_CONTENT_FILES.iter().find(|(k, _)| *k == rel).map(|(_, v)| *v)
    }

    pub fn keys() -> impl Iterator<Item = &'static str> {
        EMBEDDED_CONTENT_FILES.iter().map(|(k, _)| *k)
    }
}

/// 读取内容字节。`path` 为 assets 相对路径或绝对路径（临时文件）。
pub fn read_bytes(path: &str) -> Option<Vec<u8>> {
    let p = Path::new(path);
    if p.is_absolute() {
        return std::fs::read(p).ok();
    }
    if let Some(root) = assets_root() {
        let full = root.join(p);
        if full.is_file() {
            return std::fs::read(&full).ok();
        }
    }
    #[cfg(feature = "embed-content")]
    if CONTENT_DIRS.iter().any(|d| path.starts_with(d)) {
        if let Some(b) = embedded::lookup(path) {
            return Some(b.to_vec());
        }
    }
    None
}

pub fn read_to_string(path: &str) -> Option<String> {
    read_bytes(path).and_then(|b| String::from_utf8(b).ok())
}

pub fn exists(path: &str) -> bool {
    read_bytes(path).is_some()
}

/// 目录枚举，返回 (名称, 是否目录)，目录优先、名称次序。
/// `rel` 为 assets 相对路径（如 `nori/app-icons/files/本机`）。
pub fn list_dir(rel: &str) -> Vec<(String, bool)> {
    if let Some(root) = assets_root() {
        let full = root.join(rel);
        if full.is_dir() {
            if let Ok(rd) = std::fs::read_dir(&full) {
                let mut out: Vec<(String, bool)> = rd
                    .flatten()
                    .map(|e| {
                        let name = e.file_name().to_string_lossy().into_owned();
                        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                        (name, is_dir)
                    })
                    .collect();
                out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
                return out;
            }
        }
    }
    #[cfg(feature = "embed-content")]
    if CONTENT_DIRS.iter().any(|d| rel.starts_with(d)) {
        let prefix = format!("{}/", rel.trim_end_matches('/'));
        let mut dirs: Vec<String> = Vec::new();
        let mut files: Vec<String> = Vec::new();
        for k in embedded::keys() {
            let Some(rest) = k.strip_prefix(&prefix) else {
                continue;
            };
            if rest.is_empty() {
                continue;
            }
            match rest.find('/') {
                Some(i) => {
                    let d = rest[..i].to_string();
                    if !dirs.contains(&d) {
                        dirs.push(d);
                    }
                }
                None => files.push(rest.to_string()),
            }
        }
        dirs.sort();
        files.sort();
        return dirs
            .into_iter()
            .map(|d| (d, true))
            .chain(files.into_iter().map(|f| (f, false)))
            .collect();
    }
    Vec::new()
}
