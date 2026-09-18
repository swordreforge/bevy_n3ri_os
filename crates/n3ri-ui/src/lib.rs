// wgpu 深嵌套类型的 auto-trait（Send/Sync）求值会触发 rustc 递归上限：
// 提高 crate 递归深度，否则 1.100 起 `recursion_depth_exceeding_limit`
// 会从 warning 变成 hard error。
#![recursion_limit = "512"]

use bevy::prelude::*;
use bevy_woff::WoffPlugin;

pub mod agent_bridge;
pub mod apps;
pub mod chat_capsule;
pub mod content;
pub mod cursor;
pub mod desktop;
pub mod dock;
pub mod font;
pub mod input_focus;
pub mod pure_mode;
pub mod resize;
pub mod scroll;
pub mod snap;
pub mod theme_source;
pub mod topbar;
pub mod wallpaper_bridge;
pub mod wallpaper_ime;
pub mod wallpaper_keyboard;
pub mod window;
pub mod window_anim;

pub use chat_capsule::ChatCapsuleState;
pub use theme_source::{register_theme_sources, theme_asset_path, ThemeSourcePlugin};

pub struct N3riUiPlugin;

impl Plugin for N3riUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(WoffPlugin);
        app.add_plugins(cursor::CursorPlugin);
        app.init_resource::<input_focus::TextInputOwner>();
        app.add_systems(PostUpdate, input_focus::sync_ime_window);
        
        app.add_systems(Startup, font::load_fonts);

        // n3ri-live2d 也会注册同一插件，去重避免重复添加。
        if !app.is_plugin_added::<bevy_tweening::TweeningPlugin>() {
            app.add_plugins(bevy_tweening::TweeningPlugin);
        }

        app.add_plugins(topbar::TopbarPlugin);
        app.add_plugins(desktop::DesktopFxPlugin);
        app.add_plugins(dock::DockPlugin);
        app.add_plugins(pure_mode::PureModePlugin);
        app.add_plugins(window::WindowPlugin);
        app.add_plugins(window_anim::WindowAnimPlugin);
        app.add_plugins(agent_bridge::AgentBridgePlugin);
        app.add_plugins(snap::SnapPlugin);
        app.add_plugins(resize::ResizePlugin);
        app.add_plugins(scroll::ScrollPlugin);
        app.add_plugins(chat_capsule::ChatCapsulePlugin);
        app.add_plugins(apps::credits::CreditsPlugin);
        app.add_plugins(apps::clicker::ClickerPlugin);
        app.add_plugins(apps::settings::SettingsPlugin);
        app.add_plugins(apps::terminal::TerminalPlugin);
        app.add_plugins(apps::files::FilesPlugin);
        app.add_plugins(apps::txt_reader::TxtReaderPlugin);
        app.add_plugins(apps::log_viewer::LogViewerPlugin);
        app.add_plugins(apps::pdf_viewer::PdfViewerPlugin);
        app.add_plugins((
            apps::image_viewer::ImageViewerPlugin,
            apps::international_chess::InternationalChessPlugin,
            apps::mail::MailPlugin,
            apps::pictionary::PictionaryPlugin,
            apps::seek_treasure::SeekTreasurePlugin,
            apps::signal::SignalPlugin,
            apps::cakeduel::CakeduelPlugin,
            apps::browser::BrowserPlugin,
        ));
    }
}
