//! 主题资源覆盖层：默认内置 + 外部主题包文件。
//!
//! 机制 = Bevy 双源：`theme` 命名 AssetSource（File reader，根 = 主题包根）+
//! `theme-global`（根 = `~/.config/n3ri_os/`）。调用点零改动：需要被覆盖的资源
//! 改走 [`theme_asset_path`] 显式路由，`theme://` 命中则用外部文件，否则回内置。
//!
//! 为什么不用自定义 AssetReader 包默认源：默认源的 reader 在 `AssetPlugin::build`
//! 内同步构建，事后替换需要动 `AssetSources` 内部结构；双源 + 显式路由只用公开
//! API（`AssetSourceBuilders::insert` + `AssetPath::with_source`），无版本脆弱点。
//! 目录枚举（files/mail 打包内容）仍走 [`crate::content`] 的磁盘 overlay，不经 AssetServer。
//!
//! 注册顺序要求：在 `DefaultPlugins` 的 `AssetPlugin` 之后、`N3riUiPlugin` 之前调用
//! [`register_theme_sources`]（它只写 `AssetSourceBuilders`，`AssetPlugin` 在 build
//! 里才消费 builders，所以必须抢在它前面——`main.rs` 里 `DefaultPlugins` 整组先加，
//! 本函数紧随其后即可）。

use bevy::asset::io::{AssetSourceBuilder, AssetSourceBuilders, file::FileAssetReader};
use bevy::asset::AssetPath;
use bevy::prelude::*;
use std::path::PathBuf;

/// 主题包源名：根 = 当前主题包根（`themes/<name>/`）。
pub const THEME_SOURCE: &str = "theme";
/// 全局覆盖源名：根 = `~/.config/n3ri_os/`。
pub const THEME_GLOBAL_SOURCE: &str = "theme-global";

/// 在 `AssetSourceBuilders` 里注册两个主题源。无主题时源根不存在，
/// `FileAssetReader` 读到的是 NotFound，调用方自动回退内置。
pub fn register_theme_sources(app: &mut App) {
    let theme = n3ri_core::theme::resolve_theme();
    let theme_root: PathBuf = theme.overlay_roots.first().cloned().unwrap_or_else(|| {
        n3ri_core::theme::theme_dir(
            &theme
                .name
                .clone()
                .unwrap_or_else(|| "__none__".into()),
        )
    });
    let global_root = n3ri_core::theme::base_dir();
    let mut builders = app
        .world_mut()
        .get_resource_or_init::<AssetSourceBuilders>();
    // FileAssetReader::new 相对 exe 基址拼接，绝对根用 join 同样成立
    //（Path::join 遇绝对路径以后者为准）。
    builders.insert(
        THEME_SOURCE,
        AssetSourceBuilder::new(move || {
            Box::new(FileAssetReader::new(theme_root.clone()))
        }),
    );
    builders.insert(
        THEME_GLOBAL_SOURCE,
        AssetSourceBuilder::new(move || Box::new(FileAssetReader::new(global_root.clone()))),
    );
}

/// 主题可覆盖资源的显式路由：`theme://<rel>` → `theme-global://<rel>` → 内置 `<rel>`。
///
/// 用 [`std::fs`] 预检物理文件（主题包根/全局 basedir 下是否存在），命中即返回
/// 对应源路径，未命中一路回退。预检 = spawn 时几次 stat，不在热路径上。
/// 注意：不用 `AssetServer::load` 试错，失败 handle 会打 error 日志污染控制台。
pub fn theme_asset_path(asset_server: &AssetServer, rel: &str) -> AssetPath<'static> {
    let _ = asset_server;
    if rel.contains("..") {
        return AssetPath::from_path_buf(PathBuf::from(rel));
    }
    let theme = n3ri_core::theme::resolve_theme();
    let roots = theme.overlay_roots;
    // roots[0] = 主题包根（theme://），roots[1] = 全局 basedir（theme-global://）
    for (root, source) in roots.iter().zip([THEME_SOURCE, THEME_GLOBAL_SOURCE]) {
        if root.join(rel).is_file() {
            return AssetPath::from_path_buf(PathBuf::from(rel)).with_source(source);
        }
    }
    // 主题包内两种图标布局：icons/<app>/x（短式）→ 允许调用方传短式
    if let Some(rest) = rel.strip_prefix("nori/app-icons/") {
        let short = format!("icons/{rest}");
        for (root, source) in roots.iter().zip([THEME_SOURCE, THEME_GLOBAL_SOURCE]) {
            if root.join(&short).is_file() {
                return AssetPath::from_path_buf(PathBuf::from(short)).with_source(source);
            }
        }
    }
    AssetPath::from_path_buf(PathBuf::from(rel))
}

/// 主题包内文件直读（shader/wgsl、rules.rhai 等不需要进 AssetServer 的文本资源）。
/// 搜索序：主题包根 → 全局 basedir。`..` 拒绝，缺失返回 `None`。
pub fn read_theme_file(rel: &str) -> Option<Vec<u8>> {
    let theme = n3ri_core::theme::resolve_theme();
    let found = n3ri_core::theme::find_overlay_file(rel, &theme.overlay_roots)?;
    std::fs::read(found).ok()
}
