//! 主题模型自洽校验冒烟：ok / 缺 motion / 缺纹理 三种目录。
//! 自包含：从仓库 `assets/nori/ARGNori_web` 取真 moc3/motion，现场组装临时主题目录。

use std::path::{Path, PathBuf};

fn repo_assets() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("assets")
        .join("nori")
        .join("ARGNori_web")
}

/// 组装一个临时主题模型目录：真 moc3 + 真 Idle motion + 自定义 model3。
/// `textures`: 纹理相对路径表；`write_tex`: 是否把列出的纹理写成假文件；
/// `motion_file`: motion 相对路径（写真文件当且仅当 `write_motion=true`）。
fn make_theme_dir(
    tag: &str,
    textures: &[&str],
    write_tex: bool,
    motion_file: &str,
    write_motion: bool,
) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "n3ri-theme-validate-{}-{}",
        tag,
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let assets = repo_assets();
    std::fs::create_dir_all(&root).unwrap();
    std::fs::copy(assets.join("ARGNori.moc3"), root.join("ARGNori.moc3")).unwrap();
    for t in textures {
        let p = root.join(t);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        if write_tex {
            std::fs::write(&p, b"PNGFAKE").unwrap();
        }
    }
    if write_motion {
        let p = root.join(motion_file);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        let src = assets.join(motion_file);
        std::fs::copy(&src, &p).unwrap();
    }
    let tex_json = textures
        .iter()
        .map(|t| format!("{t:?}"))
        .collect::<Vec<_>>()
        .join(",");
    let model3 = format!(
        r#"{{"Version":3,"FileReferences":{{"Moc":"ARGNori.moc3","Textures":[{tex_json}],"Motions":{{"Idle":[{{"File":{motion_file:?}}}]}}}}}}"#
    );
    std::fs::write(root.join("t.model3.json"), model3).unwrap();
    root
}

fn cleanup(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn complete_png_model_passes() {
    let dir = make_theme_dir(
        "ok",
        &["tex/a.png", "tex/b.png"],
        true,
        "motions/01_Idle_Loop.motion3.json",
        true,
    );
    assert!(n3ri_live2d::loader::validate_theme_model_dir_pub(&dir).is_ok());
    let pet = n3ri_live2d::loader::load_pet_from_dir_for_test(Some(dir.as_path()));
    cleanup(&dir);
    assert!(pet.is_ok(), "complete theme model should load: {:?}", pet.err());
    assert_eq!(pet.unwrap().texture_paths, vec!["tex/a.png", "tex/b.png"]);
}

#[test]
fn missing_motion_fails_validation() {
    let dir = make_theme_dir(
        "no-motion",
        &["tex/a.png", "tex/b.png"],
        true,
        "motions/NOPE.motion3.json",
        false,
    );
    let r = n3ri_live2d::loader::validate_theme_model_dir_pub(&dir);
    cleanup(&dir);
    let err = r.unwrap_err();
    assert!(err.contains("motion"), "unexpected: {err}");
}

#[test]
fn missing_texture_fails_validation() {
    let dir = make_theme_dir(
        "no-tex",
        &["tex/a.png", "tex/b.png"],
        false,
        "motions/01_Idle_Loop.motion3.json",
        true,
    );
    let r = n3ri_live2d::loader::validate_theme_model_dir_pub(&dir);
    cleanup(&dir);
    let err = r.unwrap_err();
    assert!(err.contains("tex/"), "unexpected: {err}");
}

#[test]
fn toml_trailing_comment_is_stripped() {
    // 回归：theme.toml 行尾 `# 中文注释` 不能吞进值里。
    // fixtures 一律走 make_theme_dir；此处直测 theme_model_dir 的行解析：
    // 用带注释的 config + manifest 组装最小 basedir。
    let base = std::env::temp_dir().join(format!("n3ri-theme-cfg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let theme_root = base.join("themes").join("t");
    std::fs::create_dir_all(&theme_root).unwrap();
    std::fs::write(base.join("config.toml"), "theme = \"t\"  # 当前主题\n").unwrap();
    std::fs::write(
        theme_root.join("theme.toml"),
        "model_dir = \"mdir\"   # 主题包内模型目录\n",
    )
    .unwrap();
    std::fs::create_dir_all(theme_root.join("mdir")).unwrap();
    // mdir 为空目录 → validate 失败 → theme_model_dir 应返回 None 而非带注释的路径
    unsafe { std::env::set_var("N3RI_CONFIG", &base) };
    let r = n3ri_live2d::loader::theme_model_dir();
    unsafe { std::env::remove_var("N3RI_CONFIG") };
    let _ = std::fs::remove_dir_all(&base);
    assert!(r.is_none(), "comment must not leak into path: {r:?}");
}
