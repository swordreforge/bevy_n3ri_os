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
pub mod resize;
pub mod scroll;
pub mod snap;
pub mod topbar;
pub mod wallpaper_bridge;
pub mod wallpaper_ime;
pub mod wallpaper_keyboard;
pub mod window;

pub use chat_capsule::ChatCapsuleState;

pub struct N3riUiPlugin;

impl Plugin for N3riUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(WoffPlugin);
        app.add_plugins(cursor::CursorPlugin);
        app.init_resource::<input_focus::TextInputOwner>();
        app.add_systems(PostUpdate, input_focus::sync_ime_window);
        
        app.add_systems(Startup, font::load_fonts);

        app.add_plugins((
            topbar::TopbarPlugin,
            desktop::DesktopFxPlugin,
            dock::DockPlugin,
            window::WindowPlugin,
            agent_bridge::AgentBridgePlugin,
            snap::SnapPlugin,
            resize::ResizePlugin,
            scroll::ScrollPlugin,
            chat_capsule::ChatCapsulePlugin,
            apps::credits::CreditsPlugin,
            apps::clicker::ClickerPlugin,
            apps::settings::SettingsPlugin,
            apps::terminal::TerminalPlugin,
            apps::files::FilesPlugin,
        ));
        app.add_plugins(apps::txt_reader::TxtReaderPlugin);
        app.add_plugins(apps::log_viewer::LogViewerPlugin);
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
