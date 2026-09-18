use bevy::prelude::*;
use bevy::text::Font;

/// 字体上下文 - 不同场景使用不同字体
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontContext {
    /// 终端/代码 - 等宽字体，支持中文
    Terminal,
    /// UI元素/标题 - 像素风格
    Ui,
    /// Dock图标标签
    Dock,
}

/// 字体资源 - 存储所有字体句柄
#[derive(Resource)]
pub struct N3riFonts {
    /// 兜底字体 - 支持中文的等宽字体，用于所有未指定字体的地方
    pub default: Handle<Font>,
    pub terminal: Handle<Font>,
    pub ui: Handle<Font>,
    pub dock: Handle<Font>,
}

impl N3riFonts {
    pub fn get(&self, context: FontContext) -> Handle<Font> {
        match context {
            FontContext::Terminal => self.terminal.clone(),
            FontContext::Ui => self.ui.clone(),
            FontContext::Dock => self.dock.clone(),
        }
    }
}

/// 加载字体资源：主题包字体映射优先（`theme.toml [fonts]`），缺失回退内置。
pub fn load_fonts(mut commands: Commands, asset_server: Res<AssetServer>) {
    let theme = n3ri_core::theme::resolve_theme();
    let pick = |key: Option<&String>| -> String {
        key.and_then(|rel| {
            // 主题包/全局 basedir 命中 → 走 theme 源；否则当作普通 assets 相对路径
            let roots = &theme.overlay_roots;
            if n3ri_core::theme::find_overlay_file(rel, roots).is_some() {
                Some(rel.clone())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "nori/fonts/sarasa-fixed-sc.woff2".into())
    };
    let map = |rel: String| crate::theme_source::theme_asset_path(&asset_server, &rel);
    let default = asset_server.load(map(pick(theme.manifest.fonts.default.as_ref())));
    let terminal = asset_server.load(map(pick(theme.manifest.fonts.terminal.as_ref())));
    let ui = asset_server.load(map(pick(theme.manifest.fonts.ui.as_ref())));
    let dock = asset_server.load(map(pick(theme.manifest.fonts.dock.as_ref())));

    commands.insert_resource(N3riFonts {
        default,
        terminal,
        ui,
        dock,
    });
}
