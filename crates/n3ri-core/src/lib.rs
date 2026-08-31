//! n3ri-core — Core engine for n3ri_os pseudo-operating system.
//!
//! This crate provides the foundation: state machine, event types,
//! configuration resources, and plugin setup.
//!
//! It has NO dependencies on rendering, audio, or UI — those are separate crates
//! that consume the Events and traits defined here.

pub mod config;
pub mod events;
pub mod state;

use bevy::prelude::*;

use config::{OsConfig, ThemeConfig, UserSettings};
use state::{DesktopState, OsState};

pub struct N3riCorePlugin {
    pub config: OsConfig,
}

impl Default for N3riCorePlugin {
    fn default() -> Self {
        Self {
            config: OsConfig::default(),
        }
    }
}

impl Plugin for N3riCorePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.config.clone());
        app.insert_resource(ThemeConfig::default());
        app.insert_resource(UserSettings::load());
        app.init_resource::<state::BootState>();
        app.init_resource::<state::LoadState>();

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
    pub use crate::state::{AppId, BootState, DesktopState, LoadPhase, LoadState, OsState};
    pub use crate::N3riCorePlugin;
}
