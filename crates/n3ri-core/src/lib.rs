//! n3ri-core — Core engine for n3ri_os pseudo-operating system.
//!
//! This crate provides the foundation: state machine, event types,
//! configuration resources, and plugin setup.
//!
//! It has NO dependencies on rendering, audio, or UI — those are separate crates
//! that consume the Events and traits defined here.

pub mod config;
pub mod events;
pub mod music;
pub mod state;
pub mod theme;

use bevy::prelude::*;

use config::{OsConfig, ThemeConfig, UserSettings};
use music::{MusicLibrary, MusicStatus};
use state::{DesktopState, OsState};
use theme::{N3riConfig, ResolvedTheme};

#[derive(Default)]
pub struct N3riCorePlugin {
    pub config: OsConfig,
}

impl Plugin for N3riCorePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.config.clone());
        app.insert_resource(ThemeConfig::default());
        app.insert_resource(UserSettings::load());
        // 主题包包装器：默认内置 + 外部文件覆盖。无主题时 manifest 全空，只回退。
        let resolved = ResolvedTheme::from(());
        apply_theme_colors(&mut app.world_mut(), &resolved);
        app.insert_resource(N3riConfig::from(&resolved));
        app.insert_resource(resolved);
        app.init_resource::<state::BootState>();
        app.init_resource::<state::LoadState>();

        app.init_resource::<MusicLibrary>();
        app.init_resource::<MusicStatus>();

        app.init_state::<OsState>();
        app.add_sub_state::<DesktopState>();

        app.add_message::<events::BootCompleteEvent>();
        app.add_message::<events::LoadCompleteEvent>();
        app.add_message::<events::AppLaunchEvent>();
        app.add_message::<events::AppCloseEvent>();
        app.add_message::<events::AppFocusEvent>();
        app.add_message::<events::NotificationEvent>();
        app.add_message::<events::SystemMenuEvent>();
        app.add_message::<events::ShutdownEvent>();
        app.add_message::<music::MusicCommand>();

        app.add_systems(
            Update,
            (
                handle_boot_complete.run_if(in_state(OsState::Boot)),
                handle_load_complete.run_if(in_state(OsState::Loading)),
            ),
        );
    }
}

fn handle_boot_complete(
    mut ev_boot: MessageReader<events::BootCompleteEvent>,
    mut state: ResMut<NextState<OsState>>,
) {
    for _ in ev_boot.read() {
        state.set(OsState::Loading);
    }
}

fn handle_load_complete(
    mut ev_load: MessageReader<events::LoadCompleteEvent>,
    mut state: ResMut<NextState<OsState>>,
) {
    for _ in ev_load.read() {
        state.set(OsState::Desktop);
    }
}

pub mod prelude {
    pub use crate::config::{OsConfig, ThemeConfig, UserSettings};
    pub use crate::events::*;
    pub use crate::music::{MusicCommand, MusicLibrary, MusicStatus, MusicTrack, PlayMode};
    pub use crate::state::{AppId, BootState, DesktopState, LoadPhase, LoadState, OsState};
    pub use crate::theme::{N3riConfig, ResolvedTheme};
    pub use crate::N3riCorePlugin;
}

/// 主题 `[colors]` → [`ThemeConfig`]：只覆盖能解析的键，未知键忽略，失败回退默认。
fn apply_theme_colors(world: &mut World, resolved: &ResolvedTheme) {
    use config::ThemeConfig;
    let mut theme = ThemeConfig::default();
    let get = |key: &str| {
        resolved
            .overrides
            .colors
            .get(key)
            .or_else(|| resolved.manifest.colors.get(key))
            .and_then(|s| theme::parse_color(s))
            .map(|[r, g, b, a]| Color::srgba(r, g, b, a))
    };
    if let Some(c) = get("primary") {
        theme.primary = c;
    }
    if let Some(c) = get("secondary") {
        theme.secondary = c;
    }
    if let Some(c) = get("background") {
        theme.background = c;
    }
    if let Some(c) = get("surface") {
        theme.surface = c;
    }
    if let Some(c) = get("text") {
        theme.text = c;
    }
    if let Some(c) = get("text_muted") {
        theme.text_muted = c;
    }
    if let Some(c) = get("accent") {
        theme.accent = c;
    }
    if let Some(c) = get("error") {
        theme.error = c;
    }
    if let Some(c) = get("success") {
        theme.success = c;
    }
    world.insert_resource(theme);
}
