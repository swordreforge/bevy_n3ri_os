//! OS state machine.
//!
//! Uses Bevy States + SubStates for desktop environment nesting.

use bevy::prelude::*;

/// Top-level OS state.
#[derive(States, Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum OsState {
    /// Boot animation phase — system startup visual.
    #[default]
    Boot,
    /// Loading assets phase — load fonts, textures, audio.
    Loading,
    /// Main desktop environment — all app interaction happens here.
    Desktop,
    /// Running a specific application — carries app identifier.
    App(AppId),
    /// Shutdown animation phase — system shutdown visual.
    Shutdown,
}

/// Application identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AppId(pub String);

impl AppId {
    pub fn new(name: &str) -> Self {
        Self(name.to_string())
    }
}

/// Desktop sub-states (active when OsState::Desktop).
#[derive(SubStates, Debug, Clone, PartialEq, Eq, Hash, Default)]
#[source(OsState = OsState::Desktop)]
pub enum DesktopState {
    /// Normal desktop — no overlays open.
    #[default]
    Normal,
    /// System menu is open (top-left corner).
    MenuOpen,
    /// Notification panel is open.
    Notification,
    /// Settings overlay is open.
    Settings,
}

/// Boot phase — controls what's shown during boot animation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BootPhase {
    #[default]
    Logo,
    Loading,
    Transition,
}

/// Loading phase — tracks what's being loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LoadPhase {
    #[default]
    Fonts,
    Textures,
    Audio,
    Complete,
}

/// Runtime state for boot phase tracking.
#[derive(Resource, Debug, Clone, Default)]
pub struct BootState {
    pub phase: BootPhase,
    pub elapsed: f32,
    pub progress: f32,
}

/// Runtime state for loading phase tracking.
#[derive(Resource, Debug, Clone, Default)]
pub struct LoadState {
    pub phase: LoadPhase,
    pub elapsed: f32,
    pub progress: f32,
}
