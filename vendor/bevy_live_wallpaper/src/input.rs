use bevy::prelude::*;
use std::collections::HashSet;

/// Pointer state snapshot, updated every Wayland dispatch tick.
#[derive(Resource, Clone, Debug, Default)]
pub struct WallpaperPointerState {
    /// Last observed pointer sample across all outputs.
    pub last: Option<PointerSample>,
    /// Accumulated scroll delta since last read (pixels, y-up positive = scroll up).
    /// Consumed (zeroed) by the host application each frame.
    pub scroll: Vec2,
}

#[derive(Clone, Debug, Default)]
pub struct PointerSample {
    /// Backend-specific output/monitor identifier (per backend, best-effort).
    /// `None` when the pointer is not over any known output.
    pub output: Option<u32>,
    /// Global logical position (surface local + output offset).
    pub position: Vec2,
    /// Delta from the previous sample in global logical coordinates.
    pub delta: Vec2,
    pub last_button: Option<PointerButton>,
    /// Buttons currently held down.
    pub pressed: HashSet<MouseButton>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PointerButton {
    pub button: Option<MouseButton>,
    pub pressed: bool,
}

/// A raw keyboard event captured from the compositor, awaiting consumption by
/// the host application.
#[derive(Clone, Copy, Debug)]
pub struct KeyboardEvent {
    /// XKB keycode (Linux evdev scan code).
    pub keycode: u32,
    /// True for key press, false for release.
    pub pressed: bool,
}

/// Keyboard event queue, updated every Wayland dispatch tick.
/// Consumed (drained) by the host application each frame.
#[derive(Resource, Clone, Debug, Default)]
pub struct WallpaperKeyboardState {
    /// Raw key events received since the last read.
    pub events: Vec<KeyboardEvent>,
}

/// A text-input (IME) event forwarded from the compositor via
/// `zwp_text_input_v3`, awaiting consumption by the host application.
#[derive(Clone, Debug)]
pub enum TextInputEvent {
    /// Seat text-input focus entered our surface.
    Enter,
    /// Seat text-input focus left our surface.
    Leave,
    /// Composing (pre-edit) text update; buffered until `Done`.
    Preedit {
        text: String,
        cursor_begin: i32,
        cursor_end: i32,
    },
    /// Committed text; buffered until `Done`.
    Commit { text: String },
    /// Apply buffered preedit/commit state.
    Done { serial: u32 },
}

/// Text-input event queue, updated every Wayland dispatch tick.
/// Consumed (drained) by the host application each frame.
#[derive(Resource, Clone, Debug, Default)]
pub struct WallpaperTextInputState {
    /// Text-input events received since the last read.
    pub events: Vec<TextInputEvent>,
}

/// Host-app → backend control for the text-input protocol. Written by the host
/// app each frame; applied by `wayland_event_system` when the state changes.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct WallpaperTextInputControl {
    /// Whether the host app wants IME enabled on the focused surface.
    pub enabled: bool,
    /// Surrounding text around the cursor: (text, cursor byte offset, anchor
    /// byte offset). Optional; omitted means "no support" for the compositor.
    pub surrounding_text: Option<(String, usize, usize)>,
    /// Cursor rectangle in surface-local logical pixels: (x, y, w, h).
    pub cursor_rect: Option<(i32, i32, i32, i32)>,
}
