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

/// 加载字体资源
pub fn load_fonts(mut commands: Commands, asset_server: Res<AssetServer>) {
    let default = asset_server.load("nori/fonts/sarasa-fixed-sc.woff2");
    let terminal = asset_server.load("nori/fonts/sarasa-fixed-sc.woff2");
    let ui = asset_server.load("nori/fonts/sarasa-fixed-sc.woff2");
    let dock = asset_server.load("nori/fonts/sarasa-fixed-sc.woff2");

    commands.insert_resource(N3riFonts {
        default,
        terminal,
        ui,
        dock,
    });
}
