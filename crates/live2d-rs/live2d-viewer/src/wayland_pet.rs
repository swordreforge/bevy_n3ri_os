use std::path::PathBuf;
use std::sync::mpsc;

use serde::Deserialize;
use smithay_client_toolkit::reexports::client::globals::{registry_queue_init, GlobalListContents};
use smithay_client_toolkit::reexports::client::protocol::wl_compositor;
use smithay_client_toolkit::reexports::client::protocol::wl_keyboard;
use smithay_client_toolkit::reexports::client::protocol::wl_output;
use smithay_client_toolkit::reexports::client::protocol::wl_pointer;
use smithay_client_toolkit::reexports::client::protocol::wl_region;
use smithay_client_toolkit::reexports::client::protocol::wl_seat;
use smithay_client_toolkit::reexports::client::protocol::{wl_registry, wl_surface};
use smithay_client_toolkit::reexports::client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
};
use smithay_client_toolkit::reexports::protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1, zwlr_layer_surface_v1,
};

use std::ffi::c_void;
use std::num::NonZeroU32;

use glow::HasContext;
use glutin::config::ConfigTemplateBuilder;
use glutin::context::ContextAttributesBuilder;
use glutin::display::{Display, DisplayApiPreference};
use glutin::prelude::*;
use glutin::surface::{SurfaceAttributesBuilder, WindowSurface};
use raw_window_handle::{
    RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle,
};

/// Main thread → Pet thread commands
pub enum PetCommand {
    Enter {
        model_dir: PathBuf,
        model_format: crate::app::ModelFormat,
        click_through: bool,
        zoom_scale: Option<f32>,
    },
    SetClickThrough(bool),
    SetParameters {
        values: Vec<f32>,
        part_opacities: Vec<f32>,
    },
    Exit,
    /// Model entries for the search panel (file_path, name).
    ModelList(Vec<(String, String)>),
}

/// Pet thread → Main thread events
pub enum PetEvent {
    Configured {
        width: u32,
        height: u32,
    },
    /// Raw tap (click without drag) with coordinates + viewport + camera.
    /// Main thread does the full hit test (V2/V3 dispatch, drawable hit, motion).
    Tap {
        x: f64,
        y: f64,
        w: f32,
        h: f32,
        cam_scale_x: f32,
        cam_scale_y: f32,
        cam_translate_x: f32,
        cam_translate_y: f32,
    },
    /// Cursor moved on the overlay surface (main thread uses for look-at tracking).
    CursorMoved {
        x: f64,
        y: f64,
        w: f32,
        h: f32,
    },
    /// Toolbar action that needs main-thread handling.
    ToolbarAction(crate::toolbar::ToolbarAction),
    Error(String),
    Exited,
}

/// The model variant loaded in the pet thread.
#[allow(clippy::large_enum_variant)]
enum PetModel {
    V3 {
        _moc: live2d_core::Moc,
        model: live2d_core::Model<'static>,
        renderer: crate::renderer::Live2dRenderer,
        camera: crate::camera::Camera,
        look: crate::motion::look::Look,
        param_lookup: std::collections::HashMap<String, usize>,
    },
    V2 {
        v2_model: live2d_v2_core::Model,
        v2_vao: glow::NativeVertexArray,
        v2_scale: f32,
        hit_areas: Vec<crate::model_loader::HitArea>,
        last_hovered_area: Option<String>,
        /// V2 motion sound lookup: group -> Vec<(mtn_filename, Option<absolute_sound_path>)>
        motion_sounds: std::collections::HashMap<String, Vec<(String, Option<std::path::PathBuf>)>>,
        /// Audio player
        audio_player: Option<crate::audio::AudioPlayer>,
    },
}

// ---------------------------------------------------------------------------
// Pointer / drag state
// ---------------------------------------------------------------------------

struct PointerState {
    pointer_x: f64,
    pointer_y: f64,
    dragging: bool,
    drag_start_x: f64,
    drag_start_y: f64,
    had_motion: bool,
    /// Current margin (persistent across frames, updated each frame end)
    margin_right: i32,
    margin_bottom: i32,
    /// Margin snapshot taken at the START of each frame, before dispatch.
    /// All Motion events within this frame compute new_mr relative to this,
    /// avoiding both intra-frame double-subtraction and inter-frame drift
    /// (surface_x resets after each frame's commits take effect).
    frame_margin_right: i32,
    frame_margin_bottom: i32,
    pending_click: Option<(f64, f64)>,
    /// Last cursor position sent to main thread (for throttling look-at updates)
    last_cursor_x: f64,
    last_cursor_y: f64,
    scroll_offset: f32,
}

impl PointerState {
    fn new() -> Self {
        Self {
            pointer_x: 0.0,
            pointer_y: 0.0,
            dragging: false,
            drag_start_x: 0.0,
            drag_start_y: 0.0,
            had_motion: false,
            margin_right: 20,
            margin_bottom: 20,
            frame_margin_right: 20,
            frame_margin_bottom: 20,
            pending_click: None,
            last_cursor_x: f64::NEG_INFINITY,
            last_cursor_y: f64::NEG_INFINITY,
            scroll_offset: 0.0,
        }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[allow(dead_code)]
struct PetState {
    configured_size: Option<(u32, u32)>,
    compositor: wl_compositor::WlCompositor,
    surface: wl_surface::WlSurface,
    layer_surface: zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
    seat: Option<wl_seat::WlSeat>,
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    shift_held: bool,
    search_open: bool,
    search_query: String,
    ptr: PointerState,
    event_tx: mpsc::Sender<PetEvent>,
    /// HiDPI output scale factor from wl_output (default 1, set by compositor's Scale event).
    output_scale: i32,
}

// ---------------------------------------------------------------------------
// Dispatch implementations for raw Wayland protocol types
// ---------------------------------------------------------------------------

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for PetState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_registry::WlRegistry,
        _event: wl_registry::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_compositor::WlCompositor, ()> for PetState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_compositor::WlCompositor,
        _event: wl_compositor::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        unreachable!("wl_compositor has no events")
    }
}

impl Dispatch<wl_surface::WlSurface, ()> for PetState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_surface::WlSurface,
        _event: wl_surface::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_output::WlOutput, ()> for PetState {
    fn event(
        state: &mut Self,
        _proxy: &wl_output::WlOutput,
        event: wl_output::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Scale { factor } = event {
            state.output_scale = factor;
            log::info!("[pet/wayland] output scale: {}", factor);
        }
    }
}

impl Dispatch<zwlr_layer_shell_v1::ZwlrLayerShellV1, ()> for PetState {
    fn event(
        _state: &mut Self,
        _proxy: &zwlr_layer_shell_v1::ZwlrLayerShellV1,
        _event: zwlr_layer_shell_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        unreachable!("zwlr_layer_shell_v1 has no events")
    }
}

impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, ()> for PetState {
    fn event(
        state: &mut Self,
        _proxy: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_layer_surface_v1::Event::Configure {
                serial,
                width,
                height,
            } => {
                match state.configured_size {
                    None => {
                        // First Configure: accept the compositor-given size as our
                        // one-and-only surface size.  We keep this forever.
                        if width > 0 && height > 0 {
                            state.configured_size = Some((width, height));
                        }
                    }
                    Some((ow, oh)) => {
                        // Compositor is trying to resize us (e.g. niri shrinks an
                        // overlay dragged to a screen edge).  Bounce back to the
                        // original size so the surface geometry never changes and
                        // neither V2 nor V3 model rendering is distorted.
                        if width != ow || height != oh {
                            state.layer_surface.set_size(ow, oh);
                            state.surface.commit();
                            log::debug!(
                                "[pet/wayland] compositor tried to resize to \
                                 ({width},{height}), bouncing to ({ow},{oh})"
                            );
                        }
                    }
                }
                state.layer_surface.ack_configure(serial);
            }
            zwlr_layer_surface_v1::Event::Closed => {
                log::info!("[pet/wayland] layer surface closed by compositor");
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_region::WlRegion, ()> for PetState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_region::WlRegion,
        _event: wl_region::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for PetState {
    fn event(
        state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_seat::Event::Capabilities { capabilities } => {
                let cap = match capabilities {
                    smithay_client_toolkit::reexports::client::WEnum::Value(c) => c,
                    _ => return,
                };
                if cap.contains(wl_seat::Capability::Pointer) {
                    let pointer = seat.get_pointer(qh, ());
                    state.pointer = Some(pointer);
                    log::info!("[pet/wayland] got wl_pointer");
                }
                if cap.contains(wl_seat::Capability::Keyboard) {
                    let keyboard = seat.get_keyboard(qh, ());
                    state.keyboard = Some(keyboard);
                    log::info!("[pet/wayland] got wl_keyboard");
                }
            }
            wl_seat::Event::Name { name } => {
                log::debug!("[pet/wayland] seat name: {name}");
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for PetState {
    fn event(
        state: &mut Self,
        _pointer: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_pointer::Event::Enter {
                surface_x,
                surface_y,
                ..
            } => {
                state.ptr.pointer_x = surface_x;
                state.ptr.pointer_y = surface_y;
            }
            wl_pointer::Event::Leave { .. } => {
                state.ptr.dragging = false;
            }
            wl_pointer::Event::Motion {
                surface_x,
                surface_y,
                ..
            } => {
                if state.ptr.dragging {
                    let offset_x = surface_x - state.ptr.drag_start_x;
                    let offset_y = surface_y - state.ptr.drag_start_y;

                    if offset_x.abs() + offset_y.abs() > 3.0 {
                        state.ptr.had_motion = true;
                    }
                    // Compute from frame-start margin snapshot.
                    // frame_margin_right is updated at the start of each frame BEFORE
                    // dispatch, so all Motion events in this batch share the same base.
                    // This avoids:
                    //   - intra-frame: multiple Motion events corrupting margin updates
                    //   - inter-frame: surface_x reset after commit → offset_x under-reports
                    //     total drag movement (surface_x is surface-local, not absolute)
                    let new_mr = (state.ptr.frame_margin_right as f64 - offset_x).round() as i32;
                    let new_mb = (state.ptr.frame_margin_bottom as f64 - offset_y).round() as i32;
                    state.ptr.margin_right = new_mr.max(0);
                    state.ptr.margin_bottom = new_mb.max(0);
                    state.layer_surface.set_margin(
                        0,
                        state.ptr.margin_right,
                        state.ptr.margin_bottom,
                        0,
                    );
                    state.surface.commit();
                } else {
                    state.ptr.pointer_x = surface_x;
                    state.ptr.pointer_y = surface_y;
                    // Forward cursor to main thread for look-at tracking (throttled).
                    // The main thread updates its look target, advances the look
                    // system, and the modified params flow back via SetParameters.
                    let dist = (surface_x - state.ptr.last_cursor_x).abs()
                        + (surface_y - state.ptr.last_cursor_y).abs();
                    if dist > 2.0 {
                        state.ptr.last_cursor_x = surface_x;
                        state.ptr.last_cursor_y = surface_y;
                        if let Some(size) = state.configured_size {
                            let _ = state.event_tx.send(PetEvent::CursorMoved {
                                x: surface_x,
                                y: surface_y,
                                w: size.0 as f32,
                                h: size.1 as f32,
                            });
                        }
                    }
                }
            }
            wl_pointer::Event::Button {
                button,
                state: btn_state,
                ..
            } => {
                const BTN_LEFT: u32 = 0x110;
                if button == BTN_LEFT {
                    let btn = match btn_state {
                        smithay_client_toolkit::reexports::client::WEnum::Value(b) => b,
                        _ => return,
                    };
                    match btn {
                        wl_pointer::ButtonState::Pressed => {
                            state.ptr.dragging = true;
                            state.ptr.drag_start_x = state.ptr.pointer_x;
                            state.ptr.drag_start_y = state.ptr.pointer_y;
                            state.ptr.had_motion = false;
                        }
                        wl_pointer::ButtonState::Released => {
                            let was_click = state.ptr.dragging && !state.ptr.had_motion;
                            state.ptr.dragging = false;
                            if was_click {
                                state.ptr.pending_click =
                                    Some((state.ptr.pointer_x, state.ptr.pointer_y));
                            }
                        }
                        _ => {}
                    }
                }
            }
            wl_pointer::Event::Frame => {}
            wl_pointer::Event::Axis { axis, value, .. } => {
                // vertical scroll only
                if matches!(
                    axis,
                    smithay_client_toolkit::reexports::client::WEnum::Value(
                        wl_pointer::Axis::VerticalScroll
                    )
                ) {
                    state.ptr.scroll_offset += value as f32;
                    // clamp to >= 0 (can't scroll above top)
                    if state.ptr.scroll_offset < 0.0 {
                        state.ptr.scroll_offset = 0.0;
                    }
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for PetState {
    fn event(
        state: &mut Self,
        _keyboard: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use wl_keyboard::Event::*;
        use wl_keyboard::KeyState;
        match event {
            Key {
                key,
                state: key_state,
                ..
            } => {
                let pressed = matches!(
                    key_state,
                    smithay_client_toolkit::reexports::client::WEnum::Value(KeyState::Pressed)
                );
                if !pressed {
                    return;
                }
                if !state.search_open {
                    return;
                }
                let sh = state.shift_held;
                let ch = evdev_key_to_char(key, sh);
                match (key, ch, sh) {
                    // Escape — close search + restore keyboard interactivity
                    (1, _, _) => {
                        state.search_open = false;
                        state.search_query.clear();
                        state.layer_surface.set_keyboard_interactivity(
                            zwlr_layer_surface_v1::KeyboardInteractivity::None,
                        );
                        state.surface.commit();
                    }
                    // Backspace
                    (14, _, _) => {
                        if !state.search_query.is_empty() {
                            state.search_query.pop();
                        }
                    }
                    // Space
                    (57, _, _) => {
                        state.search_query.push(' ');
                    }
                    // Enter — handled by consumer
                    (28, _, _) => {}
                    // Printable character
                    (_, Some(c), _) => {
                        state.search_query.push(c);
                    }
                    // Shift keys (track for future press)
                    (42, _, _) | (54, _, _) => {
                        state.shift_held = true;
                    }
                    _ => {}
                }
            }
            Modifiers { .. } => {}
            _ => {}
        }
    }
}

fn evdev_key_to_char(key: u32, shift: bool) -> Option<char> {
    // evdev keycodes for US-QWERTY letters (NOT sequentially ordered)
    let letter = match key {
        16 => Some(b'q'),
        17 => Some(b'w'),
        18 => Some(b'e'),
        19 => Some(b'r'),
        20 => Some(b't'),
        21 => Some(b'y'),
        22 => Some(b'u'),
        23 => Some(b'i'),
        24 => Some(b'o'),
        25 => Some(b'p'),
        30 => Some(b'a'),
        31 => Some(b's'),
        32 => Some(b'd'),
        33 => Some(b'f'),
        34 => Some(b'g'),
        35 => Some(b'h'),
        36 => Some(b'j'),
        37 => Some(b'k'),
        38 => Some(b'l'),
        44 => Some(b'z'),
        45 => Some(b'x'),
        46 => Some(b'c'),
        47 => Some(b'v'),
        48 => Some(b'b'),
        49 => Some(b'n'),
        50 => Some(b'm'),
        _ => None,
    };
    if let Some(base) = letter {
        return Some(if shift {
            (base - 32) as char
        } else {
            base as char
        });
    }

    // Digits 2-11
    if (2..=11).contains(&key) {
        let digit = if key == 11 { 0 } else { key - 1 };
        let chars = if shift {
            [')', '!', '@', '#', '$', '%', '^', '&', '*', '(']
        } else {
            ['0', '1', '2', '3', '4', '5', '6', '7', '8', '9']
        };
        return chars.get(digit as usize).copied();
    }

    match key {
        12 => Some(if shift { '_' } else { '-' }),  // KEY_MINUS
        13 => Some(if shift { '+' } else { '=' }),  // KEY_EQUAL
        26 => Some(if shift { '{' } else { '[' }),  // KEY_LEFTBRACE
        27 => Some(if shift { '}' } else { ']' }),  // KEY_RIGHTBRACE
        39 => Some(if shift { ':' } else { ';' }),  // KEY_SEMICOLON
        40 => Some(if shift { '"' } else { '\'' }), // KEY_APOSTROPHE
        51 => Some(if shift { '<' } else { ',' }),  // KEY_COMMA
        52 => Some(if shift { '>' } else { '.' }),  // KEY_DOT
        53 => Some(if shift { '?' } else { '/' }),  // KEY_SLASH
        43 => Some(if shift { '|' } else { '\\' }), // KEY_BACKSLASH
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Surface setup
// ---------------------------------------------------------------------------

fn setup_pet_surface(
    model_dir: &PathBuf,
    model_format: crate::app::ModelFormat,
    click_through: bool,
    zoom_scale: Option<f32>,
    cmd_rx: &mpsc::Receiver<PetCommand>,
    event_tx: &mpsc::Sender<PetEvent>,
) -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::connect_to_env()?;
    let mut event_queue = conn.new_event_queue();
    let qh = event_queue.handle();
    let (globals, _) = registry_queue_init::<PetState>(&conn)?;

    let compositor: wl_compositor::WlCompositor = globals.bind(&qh, 1..=4, ())?;
    let layer_shell: zwlr_layer_shell_v1::ZwlrLayerShellV1 = globals.bind(&qh, 1..=4, ())?;
    let seat: wl_seat::WlSeat = globals.bind(&qh, 1..=9, ())?;
    // Bind to wl_output to receive HiDPI scale factor; ignore error (scale=1 fallback).
    let _output: Result<wl_output::WlOutput, _> = globals.bind(&qh, 1..=4, ());

    let surface = compositor.create_surface(&qh, ());
    let layer_surface = layer_shell.get_layer_surface(
        &surface,
        None,
        zwlr_layer_shell_v1::Layer::Overlay,
        "live2d-pet".to_string(),
        &qh,
        (),
    );
    layer_surface.set_size(400, 500);
    layer_surface
        .set_anchor(zwlr_layer_surface_v1::Anchor::Bottom | zwlr_layer_surface_v1::Anchor::Right);
    layer_surface.set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::None);
    layer_surface.set_margin(0, 20, 20, 0);

    // Apply initial click-through state (empty input region → passthrough)
    if click_through {
        let empty_region = compositor.create_region(&qh, ());
        surface.set_input_region(Some(&empty_region));
    }
    surface.commit();

    // Clone surface for GL context (PetState takes ownership)
    let surface_for_gl = surface.clone();
    let mut state = PetState {
        configured_size: None,
        compositor,
        surface,
        layer_surface,
        seat: Some(seat),
        pointer: None,
        keyboard: None,
        shift_held: false,
        search_open: false,
        search_query: String::new(),
        ptr: PointerState::new(),
        event_tx: event_tx.clone(),
        output_scale: 1,
    };

    event_queue.roundtrip(&mut state)?;

    // Apply HiDPI buffer scale from wl_output::Event::Scale (default 1).
    let scale = state.output_scale.max(1) as u32;
    if scale > 1 {
        state.surface.set_buffer_scale(scale as i32);
        log::info!("[pet/wayland] buffer_scale={}", scale);
    }

    if let Some((w, h)) = state.configured_size {
        let _ = event_tx.send(PetEvent::Configured {
            width: w,
            height: h,
        });
    }

    // === GL context creation ===
    let display_ptr = conn.backend().display_ptr() as *mut c_void;
    let mut raw_display_handle = WaylandDisplayHandle::empty();
    raw_display_handle.display = display_ptr;
    let raw_display = RawDisplayHandle::Wayland(raw_display_handle);

    let gl_display = unsafe { Display::new(raw_display, DisplayApiPreference::Egl)? };

    let template = ConfigTemplateBuilder::new().with_alpha_size(8).build();
    let gl_config = unsafe { gl_display.find_configs(template)? }
        .next()
        .ok_or_else(|| anyhow::anyhow!("no suitable GL config"))?;

    let surface_ptr = surface_for_gl.id().as_ptr() as *mut c_void;
    let mut raw_window_handle = WaylandWindowHandle::empty();
    raw_window_handle.surface = surface_ptr;
    let raw_window = RawWindowHandle::Wayland(raw_window_handle);

    let context_attrs = ContextAttributesBuilder::new().build(Some(raw_window));
    let not_current = unsafe { gl_display.create_context(&gl_config, &context_attrs)? };

    let (init_w, init_h) = state.configured_size.unwrap_or((400, 500));
    let egl_w = NonZeroU32::new((init_w * scale).max(1))
        .ok_or_else(|| std::io::Error::other("zero width"))?;
    let egl_h = NonZeroU32::new((init_h * scale).max(1))
        .ok_or_else(|| std::io::Error::other("zero height"))?;
    let surf_attrs =
        SurfaceAttributesBuilder::<WindowSurface>::new().build(raw_window, egl_w, egl_h);
    let egl_surface = unsafe { gl_display.create_window_surface(&gl_config, &surf_attrs)? };

    let gl_context = not_current.make_current(&egl_surface)?;

    let gl = unsafe {
        glow::Context::from_loader_function(|s| {
            let c_str = std::ffi::CString::new(s).expect("gl proc name");
            gl_display.get_proc_address(&c_str) as *const _
        })
    };

    // Test clear + swap
    unsafe {
        gl.clear_color(0.0, 0.0, 0.0, 0.0);
        gl.clear(glow::COLOR_BUFFER_BIT);
    }
    egl_surface.swap_buffers(&gl_context)?;

    // Initialize V2 glad loader (harmless if no V2 features used)
    if live2d_v2_core::gl_init() == 0 {
        log::warn!("[pet/wayland] V2 glInit returned 0 — V2 rendering may not work");
    }

    // Create a VAO for V2 rendering (V2 uses core-profile-incompatible no-VAO GL 2.1 pattern)
    let v2_vao = unsafe { gl.create_vertex_array().expect("create V2 VAO") };

    // === Model loading (branch by format) ===
    let init_size = state.configured_size.unwrap_or((400, 500));

    let pet_model = match model_format {
        crate::app::ModelFormat::V3 => {
            // Find model3.json by scanning directory (supports any filename)
            let model3_path = crate::model_loader::find_model3_json(model_dir)
                .map_err(|e| anyhow::anyhow!("find model3.json: {e}"))?;
            // Base dir for resolving relative paths (moc, textures)
            let base_dir = model3_path.parent().unwrap_or(model_dir).to_path_buf();
            let model3_json = crate::model_loader::Model3Json::from_file(&model3_path)
                .map_err(|e| anyhow::anyhow!("parse model3.json: {e}"))?;

            let moc_path = base_dir.join(&model3_json.file_references.moc);
            let moc_data =
                std::fs::read(&moc_path).map_err(|e| anyhow::anyhow!("read moc3: {e}"))?;

            let moc = live2d_core::Moc::revive(&moc_data)
                .map_err(|e| anyhow::anyhow!("Moc::revive: {e}"))?;
            let moc_ptr: *const live2d_core::Moc = &moc as *const live2d_core::Moc;
            let mut model: live2d_core::Model<'static> = unsafe {
                std::mem::transmute(
                    live2d_core::Model::initialize(&*moc_ptr)
                        .map_err(|e| anyhow::anyhow!("Model::initialize: {e}"))?,
                )
            };

            // Load textures (paths relative to base_dir)
            let mut textures = Vec::new();
            for tex_path in &model3_json.file_references.textures {
                let full_path = base_dir.join(tex_path);
                if let Ok(data) = std::fs::read(&full_path) {
                    if let Ok(tex) = unsafe { crate::texture::load_texture(&gl, &data) } {
                        textures.push(tex);
                    } else {
                        log::warn!("[pet/wayland] failed to load texture: {full_path:?}");
                    }
                } else {
                    log::warn!("[pet/wayland] texture file not found: {full_path:?}");
                }
            }

            // Initialize renderer
            let mut renderer = unsafe {
                crate::renderer::Live2dRenderer::new(&gl)
                    .map_err(|e| anyhow::anyhow!("renderer: {e}"))?
            };
            renderer.textures = textures;
            model.update();

            // Camera sized to the pet window
            let mut camera = crate::camera::Camera::new();
            let canvas_info = model.canvas_info();
            camera.fit_to_canvas(
                canvas_info.size_in_pixels.X,
                canvas_info.size_in_pixels.Y,
                canvas_info.pixels_per_unit,
                init_size.0 as f32,
                init_size.1 as f32,
            );
            if let Some(z) = zoom_scale {
                // Apply saved zoom as a multiplicative factor relative to the
                // fit_to_canvas baseline.  This preserves the Y-flip sign and
                // aspect ratio — unlike the old absolute override which set
                // both axes to the same positive value (losing the Y-flip,
                // rendering the model upside down).
                let avg = (camera.scale_x.abs() + camera.scale_y.abs()) / 2.0;
                if avg > 0.0 {
                    let factor = z / avg;
                    camera.scale_x *= factor;
                    camera.scale_y *= factor;
                }
                camera.translate_x = 0.0;
                camera.translate_y = 0.0;
            }

            log::info!(
                "[pet/wayland] V3 model loaded, canvas=({:.0},{:.0}), configured size: {:?}",
                canvas_info.size_in_pixels.X,
                canvas_info.size_in_pixels.Y,
                state.configured_size
            );

            let param_lookup: std::collections::HashMap<String, usize> = model
                .parameters()
                .ids()
                .iter()
                .enumerate()
                .map(|(i, id)| (id.to_string_lossy().into_owned(), i))
                .collect();

            PetModel::V3 {
                _moc: moc,
                model,
                renderer,
                camera,
                look: crate::motion::look::Look::new(),
                param_lookup,
            }
        }
        crate::app::ModelFormat::V2 => {
            let model_json_path = crate::app::find_v2_model_json(model_dir)
                .ok_or_else(|| anyhow::anyhow!("no V2 model JSON found in {:?}", model_dir))?;

            let mut v2 = live2d_v2_core::Model::new()
                .map_err(|e| anyhow::anyhow!("create V2 model: {e}"))?;
            v2.load_json(&model_json_path.to_string_lossy())
                .map_err(|e| anyhow::anyhow!("V2 load_json: {e}"))?;
            v2.resize(init_size.0 as i32, init_size.1 as i32);

            // Parse V2 hit_areas for per-part tap handling
            let hit_areas = {
                #[derive(Deserialize)]
                struct V2HitArea {
                    name: String,
                    id: String,
                }
                #[derive(Deserialize)]
                struct V2ModelJson {
                    hit_areas: Option<Vec<V2HitArea>>,
                }
                let json_text = std::fs::read_to_string(&model_json_path).unwrap_or_default();
                serde_json::from_str::<V2ModelJson>(&json_text)
                    .ok()
                    .and_then(|p| p.hit_areas)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|a| crate::model_loader::HitArea {
                        name: a.name,
                        id: a.id,
                    })
                    .collect::<Vec<_>>()
            };

            let cw = v2.canvas_width();
            let ch = v2.canvas_height();

            log::info!(
                "[pet/wayland] V2 model loaded, canvas=({:.0},{:.0}), hit_areas={}, configured size: {:?}",
                cw, ch, hit_areas.len(), state.configured_size
            );

            // Parse V2 model JSON motion sound paths
            let motion_sounds = {
                let json_text = std::fs::read_to_string(&model_json_path).unwrap_or_default();
                crate::v2_motion_sound::parse_v2_motions(
                    &json_text,
                    model_json_path.parent().unwrap_or(&model_json_path),
                )
            };
            let audio_player = crate::audio::AudioPlayer::new().ok();

            // Opening animation: play a random motion on model load
            v2.start_random_motion("", 3);
            if let Some(ref player) = audio_player {
                let group = v2.current_group();
                let no = v2.current_no() as usize;
                if let Some(entries) = motion_sounds.get(&group) {
                    if let Some((_, Some(path))) = entries.get(no) {
                        player.play(path);
                    }
                }
            }

            let default_zoom = 1.0f32;
            let zoom = zoom_scale.unwrap_or(default_zoom);
            if (zoom - default_zoom).abs() > f32::EPSILON {
                v2.set_scale(zoom);
            }

            PetModel::V2 {
                v2_model: v2,
                v2_vao,
                v2_scale: zoom,
                hit_areas,
                last_hovered_area: None,
                motion_sounds,
                audio_player,
            }
        }
    };

    // Enter event loop
    run_event_loop(
        &mut state,
        &mut event_queue,
        &qh,
        gl,
        egl_surface,
        gl_context,
        pet_model,
        cmd_rx,
        event_tx,
        model_dir,
        click_through,
    )?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Real event loop (Task 5)
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn run_event_loop(
    state: &mut PetState,
    event_queue: &mut EventQueue<PetState>,
    qh: &QueueHandle<PetState>,
    gl: glow::Context,
    egl_surface: glutin::surface::Surface<glutin::surface::WindowSurface>,
    gl_context: glutin::context::PossiblyCurrentContext,
    mut pet_model: PetModel,
    cmd_rx: &mpsc::Receiver<PetCommand>,
    event_tx: &mpsc::Sender<PetEvent>,
    model_dir: &std::path::Path,
    mut click_through: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let frame_duration = std::time::Duration::from_secs_f64(1.0 / 60.0);
    log::info!("[pet/wayland] event loop started (60 fps)");

    // Create overlay toolbar (semi-transparent button column on the right edge).
    let mut toolbar = unsafe {
        crate::toolbar::ToolbarOverlay::new(&gl)
            .map_err(|e| anyhow::anyhow!("toolbar init: {e}"))?
    };

    // Create text renderer for the search panel overlay
    let mut text_renderer: Option<crate::text_renderer::TextRenderer> = unsafe {
        match crate::text_renderer::TextRenderer::new(&gl, 28.0) {
            Ok(tr) => Some(tr),
            Err(e) => {
                log::warn!("[pet/wayland] text renderer init failed: {e}");
                log::warn!("[pet/wayland] search panel text disabled — install DejaVu/Noto fonts");
                None
            }
        }
    };

    // Search panel state
    let mut search_entries: Vec<(String, String)> = Vec::new();

    let mut prev_look_time = std::time::Instant::now();
    // Surface size is pinned: the Configure handler bounces any compositor-
    // initiated resize back to the original configured size.  No EGL buffer /
    // camera / model resize is needed after initial setup.
    'frame: loop {
        let frame_start = std::time::Instant::now();

        // Snapshot current margin for this frame.  All Motion events dispatched
        // below compute offset from frame_margin_right, which stays constant
        // throughout the dispatch batch.  After this frame's commits are flushed,
        // margin_right is used as the next frame's starting point.
        state.ptr.frame_margin_right = state.ptr.margin_right;
        state.ptr.frame_margin_bottom = state.ptr.margin_bottom;

        // 1. Dispatch pending Wayland events (configure, closed, etc.)
        event_queue.dispatch_pending(state)?;

        // 2. Check for commands from main thread (non-blocking)
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                PetCommand::Exit => {
                    log::info!("[pet/wayland] received exit command");
                    let _ = event_tx.send(PetEvent::Exited);
                    break 'frame;
                }
                PetCommand::Enter { .. } => {
                    // Already in pet mode, ignore
                }
                PetCommand::SetClickThrough(enabled) => {
                    click_through = enabled;
                    if enabled {
                        let empty_region = state.compositor.create_region(qh, ());
                        state.surface.set_input_region(Some(&empty_region));
                    } else {
                        state.surface.set_input_region(None);
                    }
                    state.surface.commit();
                    log::info!(
                        "[pet/wayland] click-through: {}",
                        if enabled { "on" } else { "off" }
                    );
                }
                PetCommand::SetParameters {
                    values,
                    part_opacities,
                } => {
                    if let PetModel::V3 { ref mut model, .. } = &mut pet_model {
                        let mut params = model.parameters();
                        let mut param_slice = params.values_mut();
                        let dst = param_slice.as_mut_slice();
                        let copy_len = dst.len().min(values.len());
                        dst[..copy_len].copy_from_slice(&values[..copy_len]);

                        if !part_opacities.is_empty() {
                            let mut parts = model.parts();
                            let opacities = parts.opacities_mut();
                            let copy_len = opacities.len().min(part_opacities.len());
                            opacities[..copy_len].copy_from_slice(&part_opacities[..copy_len]);
                        }
                    }
                }
                PetCommand::ModelList(entries) => {
                    search_entries = entries;
                }
            }
        }

        // 3. Update & handle overlay toolbar
        let size = state.configured_size.unwrap_or((400, 500));
        let sf = state.output_scale.max(1) as f32;
        let phys_size = ((size.0 as f32 * sf) as u32, (size.1 as f32 * sf) as u32);
        // Convert pointer from surface-local (logical) to physical pixels.
        let px_phys = state.ptr.pointer_x * sf as f64;
        let py_phys = state.ptr.pointer_y * sf as f64;

        // Update fade / hover state based on current pointer position.
        toolbar.update(px_phys, py_phys, phys_size.0, phys_size.1);

        // Check if toolbar consumes a pending click before model interaction.
        let pending = state.ptr.pending_click.take();
        let toolbar_action = pending.and_then(|(cx, cy)| {
            toolbar.handle_click(cx * sf as f64, cy * sf as f64, phys_size.0, phys_size.1)
        });
        if let Some(action) = toolbar_action {
            if matches!(action, crate::toolbar::ToolbarAction::Search) {
                state.search_open = !state.search_open;
                if state.search_open {
                    state.search_query.clear();
                    search_entries.clear();
                    state.layer_surface.set_keyboard_interactivity(
                        zwlr_layer_surface_v1::KeyboardInteractivity::Exclusive,
                    );
                    let _ = event_tx.send(PetEvent::ToolbarAction(action));
                } else {
                    state.layer_surface.set_keyboard_interactivity(
                        zwlr_layer_surface_v1::KeyboardInteractivity::None,
                    );
                }
                state.surface.commit();
            } else {
                handle_toolbar_action(action, &mut pet_model, model_dir, click_through);
            }
        } else if let Some(click) = pending {
            state.ptr.pending_click = Some(click); // restore for model handling
        }

        unsafe {
            gl.viewport(0, 0, phys_size.0 as i32, phys_size.1 as i32);
            gl.clear_color(0.0, 0.0, 0.0, 0.0);
            gl.clear(glow::COLOR_BUFFER_BIT);
        }

        // ── Search panel (overrides model rendering when open) ──
        if state.search_open {
            let vpw = phys_size.0 as f32;
            let vph = phys_size.1 as f32;
            let pad = 0.1;
            let px = vpw * pad;
            let py = vph * pad;
            let pw = (vpw - px - 56.0).max(120.0); // leave room for toolbar + 12px gap
            let ph = vph * (1.0 - 2.0 * pad);

            unsafe {
                // ── Set up 2D overlay state ──
                gl.disable(glow::DEPTH_TEST);
                gl.disable(glow::CULL_FACE);
                // ── Background panel ──
                let mut overlay_verts: Vec<f32> = Vec::new();
                crate::toolbar::ToolbarOverlay::push_rect(
                    &mut overlay_verts,
                    px,
                    py,
                    pw,
                    ph,
                    0.08,
                    0.08,
                    0.12,
                    0.92,
                );
                let mut ndc_buf: Vec<f32> = Vec::with_capacity(overlay_verts.len());
                for chunk in overlay_verts.chunks_exact(6) {
                    let [nx, ny] = crate::toolbar::ndc_pos(chunk[0], chunk[1], vpw, vph);
                    ndc_buf.push(nx);
                    ndc_buf.push(ny);
                    ndc_buf.extend_from_slice(&chunk[2..6]);
                }
                gl.use_program(Some(toolbar.program_id()));
                gl.bind_vertex_array(Some(toolbar.vao_id()));
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(toolbar.vbo_id()));
                gl.buffer_data_u8_slice(
                    glow::ARRAY_BUFFER,
                    std::slice::from_raw_parts(ndc_buf.as_ptr() as *const u8, ndc_buf.len() * 4),
                    glow::STREAM_DRAW,
                );
                gl.enable(glow::BLEND);
                gl.blend_func_separate(
                    glow::SRC_ALPHA,
                    glow::ONE_MINUS_SRC_ALPHA,
                    glow::ONE,
                    glow::ONE_MINUS_SRC_ALPHA,
                );
                gl.draw_arrays(glow::TRIANGLES, 0, ndc_buf.len() as i32 / 6);
                gl.bind_vertex_array(None);
                gl.use_program(None);

                // Search query label (baseline-aligned)
                if let Some(ref mut tr) = text_renderer {
                    tr.draw_text(
                        &gl,
                        "Search:",
                        px + 8.0,
                        py + 30.0,
                        [0.5, 0.5, 0.6, 1.0],
                        phys_size.0,
                        phys_size.1,
                        1.0,
                    );
                    let qx = px + 8.0 + 120.0;
                    let display_q = format!("{}{}", "_", state.search_query);
                    tr.draw_text(
                        &gl,
                        &display_q,
                        qx,
                        py + 30.0,
                        [1.0, 1.0, 0.8, 1.0],
                        phys_size.0,
                        phys_size.1,
                        1.0,
                    );
                }

                // Separator line — rebind toolbar program/vao (text_renderer unbound them)
                let sep_y = py + 40.0;
                let mut sep_v: Vec<f32> = Vec::new();
                crate::toolbar::ToolbarOverlay::push_rect(
                    &mut sep_v,
                    px + 6.0,
                    sep_y,
                    pw - 12.0,
                    1.0,
                    0.3,
                    0.3,
                    0.4,
                    0.5,
                );
                let mut ndc_s: Vec<f32> = Vec::with_capacity(sep_v.len());
                for chunk in sep_v.chunks_exact(6) {
                    let [nx, ny] = crate::toolbar::ndc_pos(chunk[0], chunk[1], vpw, vph);
                    ndc_s.push(nx);
                    ndc_s.push(ny);
                    ndc_s.extend_from_slice(&chunk[2..6]);
                }
                gl.use_program(Some(toolbar.program_id()));
                gl.bind_vertex_array(Some(toolbar.vao_id()));
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(toolbar.vbo_id()));
                gl.buffer_data_u8_slice(
                    glow::ARRAY_BUFFER,
                    std::slice::from_raw_parts(ndc_s.as_ptr() as *const u8, ndc_s.len() * 4),
                    glow::STREAM_DRAW,
                );
                gl.enable(glow::BLEND);
                gl.blend_func_separate(
                    glow::SRC_ALPHA,
                    glow::ONE_MINUS_SRC_ALPHA,
                    glow::ONE,
                    glow::ONE_MINUS_SRC_ALPHA,
                );
                gl.draw_arrays(glow::TRIANGLES, 0, ndc_s.len() as i32 / 6);
                gl.bind_vertex_array(None);
                gl.use_program(None);

                // Result entries
                let entry_h = text_renderer
                    .as_ref()
                    .map_or(32.0, |tr| tr.line_height() + 4.0);
                let list_y = py + 50.0;
                let q_lower = state.search_query.to_lowercase();
                let filtered: Vec<&(String, String)> = search_entries
                    .iter()
                    .filter(|(_, name)| {
                        q_lower.is_empty() || name.to_lowercase().contains(&q_lower)
                    })
                    .collect();
                if let Some(ref mut tr) = text_renderer {
                    if search_entries.is_empty() {
                        tr.draw_text(
                            &gl,
                            "(loading...)",
                            px + 8.0,
                            list_y,
                            [0.5, 0.5, 0.5, 1.0],
                            phys_size.0,
                            phys_size.1,
                            1.0,
                        );
                    } else if filtered.is_empty() && !q_lower.is_empty() {
                        tr.draw_text(
                            &gl,
                            "(no matches)",
                            px + 8.0,
                            list_y,
                            [0.6, 0.4, 0.4, 1.0],
                            phys_size.0,
                            phys_size.1,
                            1.0,
                        );
                    } else {
                        let total = filtered.len();
                        let max_visible = ((ph - 60.0) / entry_h) as usize;
                        let visible = max_visible.min(total);
                        // Clamp scroll to valid range
                        let max_scroll = ((total as f32) - visible as f32).max(0.0) * entry_h;
                        if state.ptr.scroll_offset > max_scroll {
                            state.ptr.scroll_offset = max_scroll;
                        }
                        let first_idx = (state.ptr.scroll_offset / entry_h) as usize;
                        let remainder = state.ptr.scroll_offset % entry_h;
                        let end_idx = (first_idx + visible + 1).min(total);
                        for (i, item) in filtered
                            .iter()
                            .enumerate()
                            .skip(first_idx)
                            .take(end_idx - first_idx)
                        {
                            let ey = list_y + (i - first_idx) as f32 * entry_h - remainder;
                            let baseline = ey + tr.line_height() * 0.75;
                            tr.draw_text(
                                &gl,
                                &item.1,
                                px + 8.0,
                                baseline,
                                [1.0, 1.0, 1.0, 1.0],
                                phys_size.0,
                                phys_size.1,
                                1.0,
                            );
                        }
                    }

                    // Close button hint
                    tr.draw_text(
                        &gl,
                        "[Close]",
                        px + 8.0,
                        py + ph - 20.0,
                        [0.6, 0.3, 0.3, 1.0],
                        phys_size.0,
                        phys_size.1,
                        1.0,
                    );
                }

                gl.disable(glow::BLEND);
                gl.bind_texture(glow::TEXTURE_2D, None);
                gl.bind_vertex_array(None);
                gl.use_program(None);
            }

            // Handle click on search results
            if let Some((cx, cy)) = state.ptr.pending_click.take() {
                let entry_h = text_renderer
                    .as_ref()
                    .map_or(32.0, |tr| tr.line_height() + 4.0);
                let list_y = py + 50.0;
                let max_visible = ((ph - 60.0) / entry_h) as usize;
                let filtered_clone: Vec<(String, String)> = search_entries
                    .iter()
                    .filter(|(_, name)| {
                        state.search_query.is_empty()
                            || name
                                .to_lowercase()
                                .contains(&state.search_query.to_lowercase())
                    })
                    .cloned()
                    .collect();
                let total = filtered_clone.len();
                let visible = max_visible.min(total);
                let first_idx = (state.ptr.scroll_offset / entry_h) as usize;
                let remainder = state.ptr.scroll_offset % entry_h;
                let end_idx = (first_idx + visible + 1).min(total);
                for (i, item) in filtered_clone
                    .iter()
                    .enumerate()
                    .skip(first_idx)
                    .take(end_idx - first_idx)
                {
                    let ey = list_y + (i - first_idx) as f32 * entry_h - remainder;
                    if ey > py + 24.0
                        && ey + entry_h < py + ph - 4.0
                        && (cx - px as f64 - 8.0).abs() < pw as f64 - 16.0
                        && cy >= ey as f64
                        && cy <= (ey + entry_h) as f64
                    {
                        let (path, _) = item;
                        respawn_process(std::path::Path::new(path), click_through, true);
                    }
                }
                // Close button area (text rendered at baseline py+ph-20, top ≈ py+ph-48)
                let close_y = py + ph - 48.0;
                if cx >= (px + 6.0) as f64
                    && cx <= (px + pw - 6.0) as f64
                    && cy >= close_y as f64
                    && cy <= (close_y + 34.0) as f64
                {
                    state.search_open = false;
                    state.search_query.clear();
                    state.layer_surface.set_keyboard_interactivity(
                        zwlr_layer_surface_v1::KeyboardInteractivity::None,
                    );
                    state.surface.commit();
                }
            }

            // Toolbar should still be rendered when search is open
            unsafe {
                toolbar.render(&gl, phys_size.0, phys_size.1);
            }

            // Skip model rendering
            egl_surface.swap_buffers(&gl_context)?;
            continue;
        }

        match &mut pet_model {
            PetModel::V3 {
                model,
                renderer,
                camera,
                look,
                param_lookup,
                ..
            } => {
                // Surface size is pinned (Configure handler bounces resizes), so
                // the camera only needs its initial fit_to_canvas at model load.
                // V3 local look: self-contained like V2's drag() — no main-thread dependency
                let (px, py) = (state.ptr.pointer_x as f32, state.ptr.pointer_y as f32);
                let (cw, ch) = (size.0 as f32, size.1 as f32);
                let ndc_x = 2.0 * px / cw - 1.0;
                let ndc_y = 1.0 - 2.0 * py / ch;
                look.set_target(ndc_x, ndc_y);

                let now = std::time::Instant::now();
                let dt = (now - prev_look_time).as_secs_f32().min(0.1);
                prev_look_time = now;

                {
                    let mut params = model.parameters();
                    let mut vals = params.values_mut();
                    let slice = vals.as_mut_slice();
                    for p in &look.params {
                        if let Some(&idx) = param_lookup.get(&p.id) {
                            if idx < slice.len() {
                                slice[idx] -= p.current_offset;
                            }
                        }
                    }
                }
                look.compute_raw(dt);
                {
                    let mut params = model.parameters();
                    let mut vals = params.values_mut();
                    let slice = vals.as_mut_slice();
                    for p in &look.params {
                        if let Some(&idx) = param_lookup.get(&p.id) {
                            if idx < slice.len() {
                                slice[idx] += p.current_offset;
                            }
                        }
                    }
                }

                model.update();

                // Forward tap to main thread (it has model, motion, V2/V2 dispatch)
                if let Some((cx, cy)) = state.ptr.pending_click.take() {
                    let _ = event_tx.send(PetEvent::Tap {
                        x: cx,
                        y: cy,
                        w: size.0 as f32,
                        h: size.1 as f32,
                        cam_scale_x: camera.scale_x,
                        cam_scale_y: camera.scale_y,
                        cam_translate_x: camera.translate_x,
                        cam_translate_y: camera.translate_y,
                    });
                }

                unsafe {
                    renderer.render(&gl, model, camera);
                }
            }
            PetModel::V2 {
                v2_model,
                v2_vao,
                v2_scale: _,
                hit_areas,
                last_hovered_area,
                ref motion_sounds,
                ref audio_player,
            } => {
                // Helper: play sound for the motion that just started
                let play_sound = |v2: &live2d_v2_core::Model| {
                    if let Some(ref player) = audio_player {
                        let group = v2.current_group();
                        let no = v2.current_no() as usize;
                        if let Some(entries) = motion_sounds.get(&group) {
                            if let Some((_, Some(path))) = entries.get(no) {
                                player.play(path);
                            }
                        }
                    }
                };

                // V2 head tracking (drag manager converts screen → scene internally)
                v2_model.drag(state.ptr.pointer_x as f32, state.ptr.pointer_y as f32);

                // V2 per-area hover: detect area transition
                if !hit_areas.is_empty() {
                    let cx = state.ptr.pointer_x as f32;
                    let cy = state.ptr.pointer_y as f32;
                    let mut found: Option<String> = None;
                    for area in hit_areas.iter() {
                        if v2_model.hit_test(&area.id, cx, cy) {
                            found = Some(area.name.clone());
                            break;
                        }
                    }
                    if found != *last_hovered_area {
                        if let Some(ref name) = found {
                            v2_model.start_random_motion(&format!("tap_{}", name), 3);
                            play_sound(v2_model);
                            if name == "head" {
                                v2_model.start_random_motion("flick_head", 3);
                                play_sound(v2_model);
                            }
                        }
                        *last_hovered_area = found;
                    }
                }

                // V2 tap: Python convention — random motion on click
                if state.ptr.pending_click.take().is_some() {
                    v2_model.start_random_motion("", 3);
                    play_sound(v2_model);
                }

                // Surface size is pinned (Configure handler bounces resizes), so
                // the V2 model only needs its initial resize() call at model load.
                unsafe {
                    gl.bind_vertex_array(Some(*v2_vao));
                }
                v2_model.update();
                v2_model.draw();
                unsafe {
                    // Reset GL state V2 left dirty
                    gl.bind_vertex_array(None);
                    for _ in 0..8 {
                        if gl.get_error() == glow::NO_ERROR {
                            break;
                        }
                    }
                    gl.use_program(None);
                    gl.active_texture(glow::TEXTURE0);
                    gl.front_face(glow::CCW);
                    gl.blend_equation_separate(glow::FUNC_ADD, glow::FUNC_ADD);
                    gl.blend_func_separate(
                        glow::ONE,
                        glow::ONE_MINUS_SRC_ALPHA,
                        glow::ONE,
                        glow::ONE_MINUS_SRC_ALPHA,
                    );
                }
            }
        }

        // 4. Render overlay toolbar on top of the model
        unsafe {
            toolbar.render(&gl, phys_size.0, phys_size.1);
        }

        // 5. Swap buffers
        egl_surface.swap_buffers(&gl_context)?;

        // 5. Frame rate control
        let elapsed = frame_start.elapsed();
        if elapsed < frame_duration {
            std::thread::sleep(frame_duration - elapsed);
        }
    }

    // Drop GL resources in safe order: toolbar → model → context
    unsafe {
        toolbar.destroy(&gl);
    }
    match pet_model {
        PetModel::V3 {
            _moc,
            model,
            renderer,
            camera,
            ..
        } => {
            drop((renderer, model, _moc, camera));
        }
        PetModel::V2 {
            v2_model, v2_vao, ..
        } => {
            drop((v2_model, v2_vao));
        }
    }
    drop((gl, egl_surface, gl_context));
    Ok(())
}

fn query_prev_next(model_dir: &std::path::Path) -> Option<(String, String)> {
    let db = crate::db::AppDb::open(&crate::data_dir::db_path()).ok()?;
    db.prev_next_paths(&model_dir.to_string_lossy()).ok()?
}

// ---------------------------------------------------------------------------
// Toolbar action dispatch
// ---------------------------------------------------------------------------

/// Spawn a new process and exit the current one.
///
/// When `alwaysontop` is true the new process starts with
/// `--pet-mode=alwaysontop` (model cycling).  When false it starts
/// without any `--pet-mode` flag, which defaults to Off (exit pet).
fn respawn_process(model_dir: &std::path::Path, click_through: bool, alwaysontop: bool) {
    if let Ok(exe) = std::env::current_exe() {
        let mut args: Vec<String> = std::env::args()
            .skip(1)
            .filter(|a| {
                !a.starts_with("--click-through")
                    && !a.starts_with("--pet-mode=")
                    && !a.starts_with("--overlay")
            })
            .collect();
        // Drop all positional (non-flag) args from the original invocation,
        // then push the single model dir we want — avoids accumulating stale
        // positional args across successive respawns (matching tray thread).
        args.retain(|a| a.starts_with("--"));
        // Push the model dir as the sole positional arg (the binary to load).
        args.push(model_dir.to_string_lossy().into_owned());
        if click_through {
            args.push("--click-through".into());
        }
        if alwaysontop {
            args.push("--pet-mode=alwaysontop".into());
        }
        let _ = std::process::Command::new(&exe).args(&args).spawn();
    }
    std::process::exit(0);
}

/// Execute a toolbar action.
///
/// Local actions (zoom, reset) are handled immediately.
/// Prev/Next model and ExitPet respawn the process (same pattern as the
/// tray thread) because on Wayland the main window is minimized during
/// AlwaysOnTop mode — the winit event loop may not fire
/// `RedrawRequested`, so the main thread would never drain our events.
fn handle_toolbar_action(
    action: crate::toolbar::ToolbarAction,
    pet_model: &mut PetModel,
    model_dir: &std::path::Path,
    click_through: bool,
) {
    match action {
        crate::toolbar::ToolbarAction::PrevModel => {
            if let Some((prev, _)) = query_prev_next(model_dir) {
                respawn_process(std::path::Path::new(&prev), click_through, true);
            }
        }
        crate::toolbar::ToolbarAction::NextModel => {
            if let Some((_, next)) = query_prev_next(model_dir) {
                respawn_process(std::path::Path::new(&next), click_through, true);
            }
        }
        crate::toolbar::ToolbarAction::ExitPet => {
            respawn_process(model_dir, click_through, false);
        }
        crate::toolbar::ToolbarAction::ResetCamera => match pet_model {
            PetModel::V3 { camera, .. } => {
                camera.reset_pan();
            }
            PetModel::V2 {
                v2_model, v2_scale, ..
            } => {
                *v2_scale = 1.0;
                v2_model.set_scale(1.0);
                v2_model.set_offset(0.0, 0.0);
            }
        },
        crate::toolbar::ToolbarAction::ZoomIn => match pet_model {
            PetModel::V3 { camera, .. } => {
                camera.zoom_in();
            }
            PetModel::V2 {
                v2_model, v2_scale, ..
            } => {
                *v2_scale = (*v2_scale * 1.1).min(10.0);
                v2_model.set_scale(*v2_scale);
            }
        },
        crate::toolbar::ToolbarAction::ZoomOut => match pet_model {
            PetModel::V3 { camera, .. } => {
                camera.zoom_out();
            }
            PetModel::V2 {
                v2_model, v2_scale, ..
            } => {
                *v2_scale = (*v2_scale * 0.9).max(0.1);
                v2_model.set_scale(*v2_scale);
            }
        },
        crate::toolbar::ToolbarAction::Search => {
            // handled before handle_toolbar_action is called
        }
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Spawn a separate thread that creates an sctk layer-shell surface + GL context.
///
/// Returns a `JoinHandle` for the pet thread.
/// The caller sends commands via `cmd_tx` to control the thread lifecycle.
pub fn spawn_pet_surface(
    cmd_rx: mpsc::Receiver<PetCommand>,
    event_tx: mpsc::Sender<PetEvent>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        log::info!("[pet/wayland] thread started");
        // Wait for Enter command
        match cmd_rx.recv() {
            Ok(PetCommand::Enter {
                model_dir,
                model_format,
                click_through,
                zoom_scale,
            }) => {
                log::info!("[pet/wayland] enter: {:?}", model_dir);
                if let Err(e) = setup_pet_surface(
                    &model_dir,
                    model_format,
                    click_through,
                    zoom_scale,
                    &cmd_rx,
                    &event_tx,
                ) {
                    log::error!("[pet/wayland] setup error: {:?}", e);
                    let _ = event_tx.send(PetEvent::Error(format!("{:#}", e)));
                }
            }
            Ok(PetCommand::SetClickThrough(_)) => {
                // Received before Enter (toggled before pet mode started) — ignore
                log::info!("[pet/wayland] click-through ignored (no surface yet)");
            }
            Ok(PetCommand::Exit) | Err(_) => {
                log::info!("[pet/wayland] exited before enter");
                return;
            }
            Ok(_) => {
                log::warn!("[pet/wayland] ignored command before enter");
            }
        }
        log::info!("[pet/wayland] thread ended");
    })
}
