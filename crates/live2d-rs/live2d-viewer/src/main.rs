#[cfg(feature = "ai")]
mod ai;
mod app;
mod audio;
mod camera;
mod data_dir;
mod db;
mod gui;
mod model_loader;
pub mod motion;
mod renderer;
mod text_renderer;
mod texture;
mod theme;
mod toolbar;
mod tray;
mod v2_motion_sound;
#[cfg(target_os = "linux")]
pub mod wayland_pet;

#[cfg(feature = "capture")]
mod capture;

use app::PetMode;
use glow::HasContext;
use glutin::config::ConfigTemplateBuilder;

/// Clear X11 override_redirect flag, restoring window manager
/// management for the window.  Safe to call even when the flag is
/// not set (sets override_redirect=0 which is the default).
#[cfg(target_os = "linux")]
fn clear_x11_override_redirect(window: &winit::window::Window) {
    use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

    let (display, x11_window) = match (window.raw_display_handle(), window.raw_window_handle()) {
        (RawDisplayHandle::Xlib(dh), RawWindowHandle::Xlib(wh)) => (
            dh.display as *mut x11::xlib::Display,
            wh.window as x11::xlib::Window,
        ),
        (RawDisplayHandle::Xlib(dh), RawWindowHandle::Xcb(wh)) => (
            dh.display as *mut x11::xlib::Display,
            wh.window as x11::xlib::Window,
        ),
        (_, _) => return,
    };

    let mut attrs = x11::xlib::XSetWindowAttributes {
        override_redirect: 0,
        ..unsafe { std::mem::zeroed() }
    };
    unsafe {
        x11::xlib::XChangeWindowAttributes(
            display,
            x11_window,
            x11::xlib::CWOverrideRedirect,
            &mut attrs,
        );
        x11::xlib::XFlush(display);
    }
    log::info!("[x11] override_redirect cleared on window {x11_window:#x}");
}

use glutin::context::{ContextAttributesBuilder, NotCurrentGlContext};
use glutin::display::{Display, DisplayApiPreference};
use glutin::prelude::*;
use glutin::surface::{GlSurface, SurfaceAttributesBuilder, SwapInterval, WindowSurface};
use raw_window_handle::{HasRawDisplayHandle, HasRawWindowHandle};
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;
use winit::event::{ElementState, Event, WindowEvent};
use winit::platform::x11::WindowBuilderExtX11;
use winit::window::WindowBuilder;
use winit::window::WindowLevel;

#[cfg(target_os = "linux")]
use std::sync::mpsc;

#[cfg(feature = "capture")]
fn rgba_vec_to_color32(mut rgba: Vec<u8>) -> Vec<egui::Color32> {
    let len = rgba.len() / 4;
    let cap = rgba.capacity() / 4;
    let ptr = rgba.as_mut_ptr() as *mut egui::Color32;
    std::mem::forget(rgba);
    unsafe { Vec::from_raw_parts(ptr, len, cap) }
}

/// Discover a CJK-capable font from the system using fontdb.
///
/// Tries a priority ordered list of known Chinese/Japanese font families,
/// falling back to the first available one. Returns the raw font bytes
/// for use with egui's FontDefinitions.
fn load_cjk_font() -> Result<Vec<u8>, String> {
    use fontdb::{Database, Family, Query, Source};

    let mut db = Database::new();
    db.load_system_fonts();

    const CJK_FAMILIES: &[&str] = &[
        "Source Han Sans CN",
        "Source Han Sans SC",
        "Noto Sans CJK SC",
        "Noto Sans CJK",
        "Noto Sans SC",
        "WenQuanYi Micro Hei",
        "WenQuanYi Zen Hei",
        "Droid Sans Fallback",
    ];

    for &family_name in CJK_FAMILIES {
        let query = Query {
            families: &[Family::Name(family_name)],
            ..Default::default()
        };
        let Some(id) = db.query(&query) else { continue };

        let face = db.face(id).unwrap();
        match &face.source {
            Source::File(path) | Source::SharedFile(path, _) => {
                let data = std::fs::read(path)
                    .map_err(|e| format!("failed to read font {}: {e}", path.display()))?;
                log::info!("CJK font loaded: {} ({})", family_name, path.display());
                return Ok(data);
            }
            _ => continue,
        }
    }

    log::warn!(
        "No CJK font found — Chinese/Japanese text will render as tofu (□□□). \
         Install adobe-source-han-sans-cn-fonts or noto-fonts-cjk."
    );
    Err("no CJK font found on this system".into())
}

fn main() -> anyhow::Result<()> {
    // Prefer X11 backend on Wayland — winit's X11 backend supports
    // window control features (name, transparency) used below.
    let on_wayland = std::env::var("WAYLAND_DISPLAY").is_ok();
    if on_wayland && std::env::var("WINIT_UNIX_BACKEND").is_err() {
        std::env::set_var("WINIT_UNIX_BACKEND", "x11");
        log::info!("Wayland detected: using X11 backend for winit window control");
    }

    env_logger::init();

    // Initialize user data directory and database
    let _data_dir = data_dir::ensure_data_dir()?;
    let db = db::AppDb::open(&data_dir::db_path())?;

    // Parse CLI args: --overlay flag optional, then model path
    let mut args: Vec<String> = std::env::args().collect();
    let mut overlay_mode = false;
    let mut arg_click_through = false;
    let mut arg_pet_mode: Option<String> = None;
    #[allow(unused_mut)]
    let mut arg_capture = false;
    args.retain(|a| {
        if a == "--overlay" {
            overlay_mode = true;
            false
        } else if a == "--click-through" {
            arg_click_through = true;
            false
        } else if let Some(val) = a.strip_prefix("--pet-mode=") {
            arg_pet_mode = Some(val.to_string());
            false
        } else if a == "--capture" {
            if !cfg!(feature = "capture") {
                eprintln!("error: --capture requires the 'capture' feature");
                std::process::exit(1);
            }
            arg_capture = true;
            false
        } else {
            true
        }
    });

    let event_loop =
        winit::event_loop::EventLoopBuilder::<tray::AppEvent>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();
    use std::sync::atomic::AtomicU8;

    let pet_state: tray::PetModeState = Arc::new(AtomicU8::new(tray::PET_OFF));
    let (_tray, tray_rx) = tray::create_tray(pet_state.clone());

    // Shared state for Wayland direct-respawn: on Wayland, set_visible(false)
    // kills event-loop delivery so the tray thread must act directly instead
    // of sending events via EventLoopProxy.
    let click_through_state: Arc<std::sync::atomic::AtomicBool> =
        Arc::new(std::sync::atomic::AtomicBool::new(false));
    let pet_mode_state: Arc<AtomicU8> = Arc::new(AtomicU8::new(0)); // 0=Off,1=Windowed,2=AlwaysOnTop
    let current_model_dir: Arc<std::sync::Mutex<Option<String>>> =
        Arc::new(std::sync::Mutex::new(None));

    let tray_proxy = proxy.clone();
    let tray_ct_state = click_through_state.clone();
    let tray_pm_state = pet_mode_state.clone();
    let tray_model_dir = current_model_dir.clone();
    let _tray_thread = std::thread::spawn(move || {
        while let Ok(id) = tray_rx.recv() {
            #[cfg(target_os = "linux")]
            if on_wayland {
                // On Wayland, AlwaysOnTop mode minimizes the main window —
                // EventLoopProxy can't deliver to a minimized X11 toplevel.
                // For those, spawn a new process directly.  Windowed/Off
                // modes have the window visible, so EventLoopProxy works.
                let pm = tray_pm_state.load(std::sync::atomic::Ordering::Relaxed);
                let needs_respawn = id == "show" || (pm == 2 && id != "quit");
                if needs_respawn {
                    let ct = tray_ct_state.load(std::sync::atomic::Ordering::Relaxed);
                    let new_ct = if id == "clickthrough" { !ct } else { ct };
                    let new_pm = match id.as_str() {
                        "windowed_pet" => {
                            if pm == 1 {
                                0u8
                            } else {
                                1u8
                            }
                        }
                        "alwaysontop_pet" => {
                            if pm == 2 {
                                0u8
                            } else {
                                2u8
                            }
                        }
                        _ => pm,
                    };
                    if let Ok(exe) = std::env::current_exe() {
                        let mut args: Vec<String> = std::env::args()
                            .skip(1)
                            .filter(|a| {
                                !a.starts_with("--click-through")
                                    && !a.starts_with("--pet-mode=")
                                    && !a.starts_with("--overlay")
                            })
                            .collect();
                        // Use the currently displayed model dir, not the original CLI arg
                        if let Ok(guard) = tray_model_dir.lock() {
                            if let Some(ref dir) = *guard {
                                args.retain(|a| a.starts_with("--"));
                                args.push(dir.clone());
                            }
                        }
                        if new_ct {
                            args.push("--click-through".into());
                        }
                        match new_pm {
                            1 => args.push("--pet-mode=windowed".into()),
                            2 => args.push("--pet-mode=alwaysontop".into()),
                            _ => {}
                        }
                        let _ = std::process::Command::new(&exe).args(&args).spawn();
                    }
                    std::process::exit(0);
                }
                if id == "quit" {
                    std::process::exit(0);
                }
            }
            // Non-Wayland path: send via EventLoopProxy as before
            let event = match id.as_str() {
                "show" => tray::AppEvent::ShowWindow,
                "clickthrough" => tray::AppEvent::ToggleClickThrough,
                "windowed_pet" => tray::AppEvent::ToggleWindowedPet,
                "alwaysontop_pet" => tray::AppEvent::ToggleAlwaysOnTopPet,
                "quit" => tray::AppEvent::Quit,
                _ => continue,
            };
            let _ = tray_proxy.send_event(event);
        }
    });

    let window = Arc::new(
        WindowBuilder::new()
            .with_title("Live2D Viewer")
            .with_name("live2d-viewer", "live2d-viewer")
            .with_transparent(true)
            .build(&event_loop)?,
    );
    // Enable IME (fcitx5 / ibus) for CJK text input in egui text fields
    window.set_ime_allowed(true);

    // Overlay mode: small window always-on-top at bottom-right corner
    if overlay_mode {
        window.set_decorations(false);
        window.set_window_level(WindowLevel::AlwaysOnTop);
        let sf = window.scale_factor();
        let monitor = window.current_monitor();
        if let Some(mon) = monitor {
            let phys = mon.size();
            let log_w = phys.width as f64 / sf;
            let log_h = phys.height as f64 / sf;
            let w = 300.0f64;
            let h = 400.0f64;
            let _ = window.request_inner_size(winit::dpi::LogicalSize::new(w, h));
            window.set_outer_position(winit::dpi::LogicalPosition::new(
                log_w - w - 10.0,
                log_h - h - 10.0,
            ));
        }
    }

    let display_handle = window.raw_display_handle();
    let window_handle = window.raw_window_handle();

    let gl_display = unsafe { Display::new(display_handle, DisplayApiPreference::Egl)? };

    let template = ConfigTemplateBuilder::new().with_alpha_size(8).build();
    let gl_config = unsafe {
        gl_display
            .find_configs(template)?
            .next()
            .ok_or_else(|| anyhow::anyhow!("no suitable GL config"))?
    };

    let context_attrs = ContextAttributesBuilder::new().build(Some(window_handle));
    let not_current = unsafe { gl_display.create_context(&gl_config, &context_attrs)? };

    let (init_w, init_h) = {
        let size = window.inner_size();
        (
            NonZeroU32::new(size.width).unwrap_or(NonZeroU32::new(1).unwrap()),
            NonZeroU32::new(size.height).unwrap_or(NonZeroU32::new(1).unwrap()),
        )
    };
    let surf_attrs =
        SurfaceAttributesBuilder::<WindowSurface>::new().build(window_handle, init_w, init_h);
    let surface = unsafe { gl_display.create_window_surface(&gl_config, &surf_attrs)? };

    let gl_context = not_current.make_current(&surface)?;
    let _ = surface.set_swap_interval(&gl_context, SwapInterval::Wait(NonZeroU32::new(1).unwrap()));

    // Initialize V2's glad OpenGL loader (must happen after GL context is current)
    if live2d_v2_core::gl_init() == 0 {
        log::warn!("V2 glInit returned 0 — V2 rendering may not work");
    }

    #[allow(clippy::arc_with_non_send_sync)]
    let gl = Arc::new(unsafe {
        glow::Context::from_loader_function(|s| {
            let c_str = std::ffi::CString::new(s).expect("gl proc name");
            gl_display.get_proc_address(&c_str) as *const _
        })
    });

    // Create a VAO for V2 rendering (V2 uses core-profile-incompatible no-VAO GL 2.1 pattern)
    let v2_vao = unsafe { gl.create_vertex_array().expect("create V2 VAO") };

    let mut app = app::AppState::new(Some(db));
    let mut renderer = unsafe {
        renderer::Live2dRenderer::new(&gl).map_err(|e| anyhow::anyhow!("renderer: {e}"))?
    };

    // Floating button overlay (raw GL, bypasses egui_glow coordinate bug)
    let mut float_overlay = renderer::FloatOverlay::new();

    // egui setup
    let egui_ctx = egui::Context::default();
    theme::apply_aira_dark(&egui_ctx);

    // Load CJK font to fix □□□ (tofu) for Chinese/Japanese text
    if let Ok(cjk_data) = load_cjk_font() {
        let mut fonts = egui::FontDefinitions::default();
        fonts
            .font_data
            .insert("CJK".into(), egui::FontData::from_owned(cjk_data));
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            if let Some(list) = fonts.families.get_mut(&family) {
                list.push("CJK".into());
            }
        }
        egui_ctx.set_fonts(fonts);
    }
    let mut painter = egui_glow::Painter::new(gl.clone(), "", None)
        .map_err(|e| anyhow::anyhow!("painter: {:?}", e))?;
    let mut egui_state = egui_winit::State::new(
        egui_ctx.clone(),
        egui::ViewportId::ROOT,
        &*window,
        None,
        Some(painter.max_texture_side()),
    );

    let mut prev_idx: Option<usize> = None;

    let model_path_arg = args.get(1).cloned();
    let model_loaded = if let Some(arg) = model_path_arg {
        let model_dir = PathBuf::from(&arg);
        if model_dir.exists() {
            let name = model_dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("model");
            let name_owned = name.to_string();
            let cli_format = app::detect_model_format(&model_dir);
            app.model_list.push(app::ModelEntry {
                name: name.into(),
                dir: model_dir,
                loaded: false,
                format: cli_format,
                model3_file: None,
            });
            // Record CLI model in DB
            if let Some(ref db) = app.db {
                let model_version = match cli_format {
                    Some(app::ModelFormat::V3) => "V3",
                    Some(app::ModelFormat::V2) => "V2",
                    None => "Unknown",
                };
                let _ = db.add_or_update_model(&arg, &name_owned, model_version, None);
            }
            true
        } else {
            eprintln!("model directory not found: {arg}");
            false
        }
    } else {
        false
    };

    if !model_loaded {
        // 运行时模型扫描：仅当 LIVE2D_SDK_ROOT 设了才扫描 Samples
        let samples_resources = std::env::var("LIVE2D_SDK_ROOT")
            .map(|root| PathBuf::from(root).join("Samples").join("Resources"))
            .unwrap_or_else(|_| PathBuf::new());

        if samples_resources.exists() {
            if let Ok(entries) = std::fs::read_dir(&samples_resources) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() && app::detect_model_format(&path).is_some() {
                        app.add_model_dir(path);
                    }
                }
            }
        }
    }

    // Restore model history from DB (merge into model_list, prefer existing entries)
    if let Ok(records) = app
        .db
        .as_ref()
        .map(|db| db.model_history())
        .unwrap_or(Ok(Vec::new()))
    {
        for rec in records {
            let p = std::path::PathBuf::from(&rec.file_path);
            if p.exists() && !app.model_list.iter().any(|e| e.dir == p) {
                let fmt = app::detect_model_format(&p);
                app.model_list.push(app::ModelEntry {
                    name: rec.name.clone(),
                    dir: p,
                    loaded: false,
                    format: fmt,
                    model3_file: None,
                });
            }
        }
    }

    app.load_scan_dirs();

    if !app.model_list.is_empty() {
        let _ = app.switch_to(0);

        // Restore state from previous process (Wayland respawn preserves via CLI flags)
        if arg_click_through {
            app.click_through = true;
            let _ = window.set_cursor_hittest(false);
        }
        if let Some(ref mode) = arg_pet_mode {
            match mode.as_str() {
                "windowed" => app.pet_mode = PetMode::Windowed,
                "alwaysontop" => app.pet_mode = PetMode::AlwaysOnTop,
                _ => {}
            }
            if app.pet_mode != PetMode::Off {
                app.pet_mode_changed = true;
            }
        }
        for path in &app.texture_paths {
            match std::fs::read(path) {
                Ok(img_data) => match unsafe { texture::load_texture(&gl, &img_data) } {
                    Ok(tex) => renderer.textures.push(tex),
                    Err(e) => eprintln!("texture load {:?}: {e}", path),
                },
                Err(e) => eprintln!("texture read {:?}: {e}", path),
            }
        }
    }

    // Frame timing
    let mut last_frame_time = Instant::now();

    #[cfg(target_os = "linux")]
    let mut gnome_x11_display: Option<*mut x11::xlib::Display> = None;
    #[cfg(target_os = "linux")]
    let mut gnome_x11_window: Option<x11::xlib::Window> = None;
    #[cfg(target_os = "linux")]
    let mut last_x11_raise = Instant::now();

    // Helper: detach always-on-top pet thread (non-blocking).
    // Sends Exit and drops the JoinHandle — the old thread runs its
    // cleanup asynchronously. Old and new threads share no resources.
    #[cfg(target_os = "linux")]
    let detach_always_on_top = |app: &mut app::AppState| {
        if let Some(tx) = app.pet_wayland_cmd_tx.take() {
            let _ = tx.send(crate::wayland_pet::PetCommand::Exit);
        }
        let _ = app.pet_wayland_thread.take();
        app.pet_wayland_event_rx = None;
    };

    // ── Capture mode initialization ──────────────────────────────────────
    #[cfg(feature = "capture")]
    if arg_capture {
        app.start_capture();
    }

    event_loop.run(move |event, target| {
        match event {
            Event::WindowEvent { event, .. } => {
                match event {
                    WindowEvent::RedrawRequested => {
                        // --- Complete any pending async model switch ---
                        app.complete_pending_switch();

                        // --- Drain background model-search results / dispatch pending ---
                        app.poll_search(&egui_ctx);

                        // --- Frame timing ---
                        let now = Instant::now();
                        let delta = now.duration_since(last_frame_time).as_secs_f32().min(0.1);
                        last_frame_time = now;

                        // Sync shared state to tray thread atomics (used for Wayland respawn)
                        click_through_state.store(
                            app.click_through,
                            std::sync::atomic::Ordering::Relaxed,
                        );
                        pet_mode_state.store(
                            match app.pet_mode {
                                PetMode::Off => 0,
                                PetMode::Windowed => 1,
                                PetMode::AlwaysOnTop => 2,
                            },
                            std::sync::atomic::Ordering::Relaxed,
                        );
                        if let Ok(mut guard) = current_model_dir.lock() {
                            let dir = app.current_model_dir()
                                .map(|p| p.to_string_lossy().into_owned());
                            if *guard != dir {
                                *guard = dir;
                            }
                        }

                        // --- Helper: request window sized to model display, clamped to monitor ---
                        fn request_model_window(window: &winit::window::Window, cw: f32, ch: f32) {
                            let sf = window.scale_factor();
                            let max_lh = window
                                .current_monitor()
                                .map(|m| m.size().height as f64 / sf - 40.0)
                                .unwrap_or(800.0)
                                .max(200.0);
                            let target_h = max_lh * 0.9;
                            let model_display_w = target_h as f32 * cw / ch;
                            let target_w = (model_display_w * 1.1 + 50.0) as f64; // 10% padding + toolbar
                            let _ = window.request_inner_size(winit::dpi::LogicalSize::new(
                                target_w, target_h,
                            ));
                        }

                        // --- Apply pending pet mode window changes ---
                        if app.pet_mode_changed {
                            match app.pet_mode {
                                PetMode::Windowed => {
                                    // ── Windowed pet: main window → transparent, always-on-top ──
                                    pet_state.store(tray::PET_WINDOWED, Ordering::Release);
                                    #[cfg(target_os = "linux")]
                                    detach_always_on_top(&mut app);

                                    window.set_decorations(false);
                                    window.set_window_level(WindowLevel::AlwaysOnTop);
                                    if let Some(ref model) = app.current_model {
                                        let canvas = model.canvas_info();
                                        request_model_window(
                                            &window,
                                            canvas.size_in_pixels.X,
                                            canvas.size_in_pixels.Y,
                                        );
                                    }
                                    log::info!(
                                        "[pet/windowed] enter: canvas=({:.0},{:.0})",
                                        app.canvas_pixel_size.0,
                                        app.canvas_pixel_size.1
                                    );
                                    app.camera_needs_fit = true;
                                    app.pet_mode_delay = 2;
                                }
                                PetMode::AlwaysOnTop => {
                                    // ── Always on Top pet: spawn sctk layer-shell thread ──
                                    pet_state.store(tray::PET_ALWAYS_ON_TOP, Ordering::Release);
                                    #[cfg(target_os = "linux")]
                                    if on_wayland && !app::is_gnome() {
                                        window.set_decorations(false);
                                        window.set_window_level(WindowLevel::AlwaysOnTop);

                                        // Resize to model size first — same sequence as Windowed pet,
                                        // "warms up" XWayland so subsequent 50×50 resize is accepted.
                                        let cw = app.canvas_pixel_size.0;
                                        let ch = app.canvas_pixel_size.1;
                                        if cw > 0.0 && ch > 0.0 {
                                            request_model_window(&window, cw, ch);
                                        }

                                        // Detach any existing thread before spawning a new one
                                        detach_always_on_top(&mut app);

                                        let (cmd_tx, cmd_rx) = mpsc::channel();
                                        let (event_tx, event_rx) = mpsc::channel();
                                        let handle =
                                            crate::wayland_pet::spawn_pet_surface(cmd_rx, event_tx);

                                        if let (Some(dir), Some(format)) =
                                            (app.current_model_dir(), app.current_model_format())
                                        {
                                            let zoom = match format {
                                                crate::app::ModelFormat::V2 => Some(app.v2_scale),
                                                // V3: no zoom_scale — fit_to_canvas in the pet thread
                                                // correctly computes camera with Y-flip and aspect ratio.
                                                // Passing zoom_scale here is fundamentally broken:
                                                //   (1) scale_y is negative (Y-flip), so (scale_x+scale_y)/2
                                                //       gives a near-zero value, and
                                                //   (2) overwriting both axes with the same positive value
                                                //       loses the Y-flip, rendering the model upside down.
                                                // The user can adjust zoom via toolbar buttons in the pet.
                                                crate::app::ModelFormat::V3 => None,
                                            };
                                            let _ = cmd_tx.send(
                                                crate::wayland_pet::PetCommand::Enter {
                                                    model_dir: dir,
                                                    model_format: format,
                                                    click_through: app.click_through,
                                                    zoom_scale: zoom,
                                                },
                                            );
                                        }

                                        app.pet_wayland_cmd_tx = Some(cmd_tx);
                                        app.pet_wayland_event_rx = Some(event_rx);
                                        app.pet_wayland_thread = Some(handle);

                                        app.request_minimize = true;
                                        log::info!("[pet] AlwaysOnTop set request_minimize=true");
                                    } else if app::is_gnome() || !on_wayland {
                                        // ── GNOME (XWayland) or Xorg: AlwaysOnTop window ──
                                        detach_always_on_top(&mut app);

                                        window.set_decorations(false);
                                        window.set_window_level(WindowLevel::AlwaysOnTop);

                                        // GNOME XWayland: override_redirect + periodic XRaiseWindow
                                        // to keep the window above others.  _NET_WM_STATE_ABOVE alone
                                        // is ignored by Mutter on XWayland when another window is
                                        // focused.  With override_redirect, XRaiseWindow directly
                                        // manipulates X11 stacking; Mutter's stack-tracker handles
                                        // override-redirect XWayland windows correctly.
                                        if on_wayland && app::is_gnome() {
                                            use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
                                            let (dsp, xw) = match (window.raw_display_handle(), window.raw_window_handle()) {
                                                (RawDisplayHandle::Xlib(dh), RawWindowHandle::Xlib(wh)) => {
                                                    (dh.display as *mut x11::xlib::Display, wh.window as x11::xlib::Window)
                                                }
                                                (RawDisplayHandle::Xlib(dh), RawWindowHandle::Xcb(wh)) => {
                                                    (dh.display as *mut x11::xlib::Display, wh.window as x11::xlib::Window)
                                                }
                                                _ => (std::ptr::null_mut(), 0),
                                            };
                                            if !dsp.is_null() && xw != 0 {
                                                let mut attrs = x11::xlib::XSetWindowAttributes {
                                                    override_redirect: 1,
                                                    ..unsafe { std::mem::zeroed() }
                                                };
                                                unsafe {
                                                    x11::xlib::XChangeWindowAttributes(dsp, xw, x11::xlib::CWOverrideRedirect, &mut attrs);
                                                    x11::xlib::XFlush(dsp);
                                                }
                                                gnome_x11_display = Some(dsp);
                                                gnome_x11_window = Some(xw);
                                                last_x11_raise = Instant::now();
                                                log::info!("[x11/gnome] override_redirect + periodic raise for window {xw:#x}");
                                            }
                                        }

                                        let cw = app.canvas_pixel_size.0;
                                        let ch = app.canvas_pixel_size.1;
                                        if cw > 0.0 && ch > 0.0 {
                                            request_model_window(&window, cw, ch);

                                            // Immediately set up GL surface for the DESIRED
                                            // pet size, before the async X11 resize event.
                                            let sf = window.scale_factor();
                                            let max_lh = window
                                                .current_monitor()
                                                .map(|m| m.size().height as f64 / sf - 40.0)
                                                .unwrap_or(800.0)
                                                .max(200.0);
                                            let target_h = max_lh * 0.9;
                                            let model_display_w = target_h as f32 * cw / ch;
                                            let target_w =
                                                (model_display_w * 1.1 + 50.0) as f64;
                                            let phys_w = (target_w * sf) as u32;
                                            let phys_h = (target_h * sf) as u32;

                                            // Override: camera, viewport, egui, toolbar all
                                            // use this size until the Resized event confirms.
                                            app.window_size_override =
                                                Some((phys_w as f32, phys_h as f32));

                                            if let (Some(pw), Some(ph)) = (
                                                NonZeroU32::new(phys_w.max(1)),
                                                NonZeroU32::new(phys_h.max(1)),
                                            ) {
                                                surface.resize(&gl_context, pw, ph);
                                            }
                                        }
                                        if app.click_through {
                                            let _ = window.set_cursor_hittest(false);
                                        }
                                        app.camera_needs_fit = true;
                                        app.pet_mode_delay = 2;
                                        log::info!(
                                            "[pet/always-on-top] GNOME/Xorg: override_redirect + periodic raise"
                                        );
                                    } else {
                                        // Wayland without layer-shell support: should not reach here.
                                        log::warn!("[pet/always-on-top] not supported on this platform");
                                        app.pet_mode = PetMode::Off;
                                    }
                                }
                                PetMode::Off => {
                                    // ── Exit any active pet mode ──
                                    pet_state.store(tray::PET_OFF, Ordering::Release);
                                    #[cfg(target_os = "linux")]
                                    {
                                        detach_always_on_top(&mut app);
                                        // Defensive: clear X11 override_redirect if it was set
                                        clear_x11_override_redirect(&window);
                                        // Stop GNOME periodic XRaiseWindow
                                        gnome_x11_display = None;
                                        gnome_x11_window = None;
                                    }

                                    window.set_decorations(true);
                                    window.set_window_level(WindowLevel::Normal);
                                    let _ = window.set_cursor_hittest(true);
                                    log::info!("[pet] exit");
                                }
                            }
                            app.pet_mode_changed = false;
                        }

                        // --- Resize window when model switches in pet mode ---
                        if app.pet_resize_pending {
                            app.pet_resize_pending = false;
                            app.pet_mode_delay = 2;
                            if app.pet_mode == PetMode::Windowed {
                                if let Some(ref model) = app.current_model {
                                    let canvas = model.canvas_info();
                                    request_model_window(
                                        &window,
                                        canvas.size_in_pixels.X,
                                        canvas.size_in_pixels.Y,
                                    );
                                }
                            }
                        }

                        // --- Camera recalculation (model switch OR pending pet mode resize) ---
                        if app.current_idx != prev_idx || app.camera_needs_fit {
                            let (cam_w, cam_h) = app
                                .window_size_override
                                .unwrap_or_else(|| {
                                    let s = window.inner_size();
                                    (s.width as f32, s.height as f32)
                                });
                            if let Some(ref model) = app.current_model {
                                let canvas = model.canvas_info();
                                app.camera.fit_to_canvas(
                                    canvas.size_in_pixels.X,
                                    canvas.size_in_pixels.Y,
                                    canvas.pixels_per_unit,
                                    cam_w,
                                    cam_h,
                                );
                            }
                            app.camera_needs_fit = false;
                        }

                        // --- Advance motion system (V3 only) ---
                        // Runs even when minimized to float — needed for look-at
                        // tracking on the AlwaysOnTop overlay (parameter sync below).
                        if !app.is_v2 {
                            app.advance_motion(delta);
                            app.update_parameters();
                            app.update_pose(delta);
                        }

                        // Sync parameters to AlwaysOnTop overlay thread
                        // Guard: V3 only (V2 models never need pet overlay sync).
                        // When minimized, throttle to ~15 Hz (every 4th frame)
                        // to reduce Vec allocation & copy churn on idle frames.
                        #[cfg(target_os = "linux")]
                        if !app.is_v2 {
                            if let Some(ref tx) = app.pet_wayland_cmd_tx {
                                let need_sync = if app.minimized_to_float {
                                    app.pet_sync_counter = (app.pet_sync_counter + 1) & 3;
                                    app.pet_sync_counter == 0
                                } else {
                                    true
                                };
                                if need_sync {
                                    let values = app.parameter_values.clone();
                                    let part_opacities = app
                                        .current_model
                                        .as_ref()
                                        .map(|m| m.parts().opacities().to_vec())
                                        .unwrap_or_default();
                                    let _ = tx.send(
                                        crate::wayland_pet::PetCommand::SetParameters {
                                            values,
                                            part_opacities,
                                        },
                                    );
                                }
                            }
                        }

                        let size = if let Some((ow, oh)) = app.window_size_override {
                            app.window_size = (ow, oh);
                            winit::dpi::PhysicalSize::new(ow as u32, oh as u32)
                        } else {
                            let s = window.inner_size();
                            app.window_size = (s.width as f32, s.height as f32);
                            s
                        };
                        let clear_color = if app.minimized_to_float {
                            if app.pet_mode == PetMode::AlwaysOnTop {
                                // AlwaysOnTop: transparent window (pet on Wayland surface)
                                egui::Color32::from_rgba_premultiplied(0, 0, 0, 0)
                            } else {
                                egui::Color32::from_rgb(0x33, 0x99, 0xff)
                            }
                        } else if app.pet_mode == PetMode::Windowed
                            || (app.pet_mode == PetMode::AlwaysOnTop && app::is_gnome())
                        {
                            // Windowed pet, or GNOME AlwaysOnTop (main window IS the pet)
                            egui::Color32::from_rgba_premultiplied(0, 0, 0, 0)
                        } else {
                            egui::Color32::from_rgb(0x1a, 0x1a, 0x2e)
                        };

                        // Texture reload on model switch only
                        if app.current_idx != prev_idx {
                            unsafe {
                                for tex in renderer.textures.drain(..) {
                                    gl.delete_texture(tex);
                                }
                            }
                            for path in &app.texture_paths {
                                match std::fs::read(path) {
                                    Ok(img_data) => {
                                        match unsafe { texture::load_texture(&gl, &img_data) } {
                                            Ok(tex) => renderer.textures.push(tex),
                                            Err(e) => eprintln!("texture load {:?}: {e}", path),
                                        }
                                    }
                                    Err(e) => eprintln!("texture read {:?}: {e}", path),
                                }
                            }
                            prev_idx = app.current_idx;
                        }

                        unsafe {
                            gl.clear_color(
                                clear_color.r() as f32 / 255.0,
                                clear_color.g() as f32 / 255.0,
                                clear_color.b() as f32 / 255.0,
                                clear_color.a() as f32 / 255.0,
                            );
                            gl.clear(glow::COLOR_BUFFER_BIT);
                        }

                        if app.is_v2 {
                            if let Some(ref mut v2) = app.v2_model {
                                if !app.minimized_to_float {
                                    unsafe {
                                        gl.viewport(0, 0, size.width as i32, size.height as i32);
                                        // Bind VAO so V2's glVertexAttribPointer works (V2 uses GL 2.1 style without VAO)
                                        gl.bind_vertex_array(Some(v2_vao));
                                    }
                                    let (vw, vh) = (size.width as i32, size.height as i32);
                                    if (vw, vh) != app.last_v2_size {
                                        v2.resize(vw, vh);
                                        app.last_v2_size = (vw, vh);
                                    }
                                    v2.update();
                                    v2.draw();
                                    unsafe {
                                        // Reset GL state V2 left dirty, drain stale errors
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
                        } else if let Some(ref mut model) = app.current_model {
                            if !app.minimized_to_float {
                                unsafe {
                                    gl.viewport(0, 0, size.width as i32, size.height as i32);
                                    gl.disable(glow::DEPTH_TEST);
                                    gl.disable(glow::CULL_FACE);
                                    renderer.render(&gl, model, &app.camera);
                                }
                            }
                        }

                        // Upload latest capture frame as egui texture (before egui frame)
                        #[cfg(feature = "capture")]
                        if let Some(ref frame) = app.capture_latest_frame {
                            if frame.width > 0 && frame.height > 0 {
                                let pixels = rgba_vec_to_color32(frame.data.clone());
                                let color_image = egui::ColorImage {
                                    size: [frame.width as usize, frame.height as usize],
                                    pixels,
                                };
                                if let Some(ref mut tex) = app.capture_texture {
                                    tex.set(color_image, egui::TextureOptions::LINEAR);
                                } else {
                                    app.capture_texture = Some(egui_ctx.load_texture(
                                        "capture_preview",
                                        color_image,
                                        egui::TextureOptions::LINEAR,
                                    ));
                                }
                            }
                        }

                        // --- Egui frame (skipped when minimized to 1×1 floating circle) ---
                        if app.pet_mode == PetMode::AlwaysOnTop && app.minimized_to_float {
                            // Wayland: 1×1 surface crashes font.rs, skip egui entirely.
                        } else {
                            unsafe {
                                gl.bind_framebuffer(glow::FRAMEBUFFER, None);
                                gl.viewport(0, 0, size.width as i32, size.height as i32);
                            }
                            let mut raw_input = egui_state.take_egui_input(&window);
                            // GNOME AlwaysOnTop: override screen_rect with the desired
                            // pet size until the Resized event confirms the new window size.
                            if let Some((pw, ph)) = app.window_size_override {
                                let sf = window.scale_factor() as f32;
                                raw_input.screen_rect = Some(egui::Rect::from_min_size(
                                    egui::pos2(0.0, 0.0),
                                    egui::vec2(pw / sf, ph / sf),
                                ));
                            }
                            egui_ctx.begin_frame(raw_input);
                            gui::draw_ui(&egui_ctx, &mut app);
                            let output = egui_ctx.end_frame();

                            let copied = &output.platform_output.copied_text;
                            if !copied.is_empty() {
                                if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                    let _ = clipboard.set_text(copied);
                                }
                            }

                            // Always process textures
                            for (id, delta) in &output.textures_delta.set {
                                painter.set_texture(*id, delta);
                            }
                            for id in &output.textures_delta.free {
                                painter.free_texture(*id);
                            }

                            // Render shapes: egui_glow when normal, raw GL triangle when floating
                            if app.minimized_to_float {
                                if app.pet_mode != PetMode::AlwaysOnTop {
                                    unsafe {
                                        gl.bind_framebuffer(glow::FRAMEBUFFER, None);
                                        gl.viewport(0, 0, size.width as i32, size.height as i32);
                                        float_overlay.draw_play_button(
                                            &gl,
                                            size.width as f32,
                                            size.height as f32,
                                        );
                                    }
                                }
                            } else {
                                let clipped_primitives =
                                    egui_ctx.tessellate(output.shapes, output.pixels_per_point);
                                painter.paint_primitives(
                                    [size.width, size.height],
                                    output.pixels_per_point,
                                    &clipped_primitives,
                                );
                            }
                        }

                        // Intercept WM minimize: on X11 hide the window so we can
                        // later restore via ShowWindow.  On Wayland, set_visible(false)
                        // kills event-loop delivery (tray operations stop working), so
                        // we use the 50×50 float circle instead — keeps the X11 toplevel
                        // mapped so EventLoopProxy can still deliver tray events.
                        if app.restore_cooldown > 0 {
                            app.restore_cooldown -= 1;
                        } else if !app.minimized_to_float && !app.request_restore
                            && window.is_minimized().unwrap_or(false) {
                                app.minimized_to_float = true;
                                app.camera_needs_fit = false;
                                if on_wayland {
                                    let sf = window.scale_factor();
                                    app.saved_window_pet_size = (
                                        app.window_size.0 as f64 / sf,
                                        app.window_size.1 as f64 / sf,
                                    );
                                    window.set_max_inner_size(Some(
                                        winit::dpi::LogicalSize::new(50.0, 50.0),
                                    ));
                                    let _ = window
                                        .request_inner_size(winit::dpi::LogicalSize::new(50.0, 50.0));
                                    let phys_w = (50.0_f64 * sf) as u32;
                                    let phys_h = (50.0_f64 * sf) as u32;
                                    if let (Some(rw), Some(rh)) =
                                        (NonZeroU32::new(phys_w), NonZeroU32::new(phys_h))
                                    {
                                        surface.resize(&gl_context, rw, rh);
                                    }
                                } else {
                                    window.set_visible(false);
                                }
                            }

                        // Minimize (↓ button)
                        if app.request_minimize {
                            log::info!("[pet] request_minimize block entered");
                            app.request_minimize = false;
                            let on_x11 = matches!(
                                window.raw_window_handle(),
                                raw_window_handle::RawWindowHandle::Xlib(_)
                                    | raw_window_handle::RawWindowHandle::Xcb(_)
                            );
                            let is_aot = app.pet_mode == PetMode::AlwaysOnTop;
                            let is_capable_de = app::is_gnome() || app::is_kde() || app::is_mango();
                            // Always save restore state regardless of strategy
                            let sf = window.scale_factor();
                            app.saved_window_pet_size =
                                (app.window_size.0 as f64 / sf, app.window_size.1 as f64 / sf);
                            app.minimized_to_float = true;
                            app.camera_needs_fit = false;

                            if on_x11 && !on_wayland {
                                window.set_visible(false);
                            } else if is_aot && is_capable_de {
                                // GNOME/KDE/MangoWM: proper window minimize — 1×1 crashes egui / protocol error
                                log::info!("[pet] request_minimize: set_minimized(true)");
                                window.set_minimized(true);
                            } else {
                                // niri and others: 1×1 or 50×50 resize to become invisible
                                let float_size = if is_aot { 1.0 } else { 50.0 };
                                log::info!(
                                    "[pet] request_minimize: doing {float_size}x{float_size} resize"
                                );
                                window.set_max_inner_size(Some(winit::dpi::LogicalSize::new(
                                    float_size, float_size,
                                )));
                                let _ = window.request_inner_size(
                                    winit::dpi::LogicalSize::new(float_size, float_size),
                                );
                                // Force EGL surface resize immediately (Wayland workaround)
                                let phys_w = (float_size * sf) as u32;
                                let phys_h = (float_size * sf) as u32;
                                if let (Some(rw), Some(rh)) = (
                                    NonZeroU32::new(phys_w.max(1)),
                                    NonZeroU32::new(phys_h.max(1)),
                                ) {
                                    surface.resize(&gl_context, rw, rh);
                                }
                                // XWayland buffers resize requests and only flushes
                                // when user X11 events arrive. current_monitor()
                                // triggers an X11 roundtrip that forces the flush.
                                let _ = window.current_monitor();
                            }
                        }
                        if app.request_restore {
                            app.request_restore = false;
                            if app.minimized_to_float {
                                app.minimized_to_float = false;
                                app.restore_cooldown = 10;
                                // GNOME/KDE minimised via set_minimized — undo it first
                                if app::is_gnome() || app::is_kde() || app::is_mango() {
                                    window.set_minimized(false);
                                }
                                let (w, h) = app.saved_window_pet_size;
                                let rw = w.clamp(200.0, 4000.0);
                                let rh = h.clamp(200.0, 4000.0);
                                window.set_max_inner_size(None::<winit::dpi::LogicalSize<f64>>);
                                let _ =
                                    window.request_inner_size(winit::dpi::LogicalSize::new(rw, rh));
                                let sf = window.scale_factor();
                                let phys_w = (rw * sf) as u32;
                                let phys_h = (rh * sf) as u32;
                                if let (Some(pw), Some(ph)) =
                                    (NonZeroU32::new(phys_w), NonZeroU32::new(phys_h))
                                {
                                    surface.resize(&gl_context, pw, ph);
                                }
                                window.set_visible(true);
                                app.camera_needs_fit = true;
                            } else {
                                window.set_visible(true);
                            }
                            // Re-apply window-level state lost during minimize
                            let _ = window.set_cursor_hittest(!app.click_through);
                            match app.pet_mode {
                                PetMode::Windowed => {
                                    window.set_decorations(false);
                                    window.set_window_level(WindowLevel::AlwaysOnTop);
                                }
                                PetMode::AlwaysOnTop => {
                                    window.set_decorations(true);
                                    window.set_window_level(WindowLevel::Normal);
                                }
                                PetMode::Off => {
                                    window.set_decorations(true);
                                    window.set_window_level(WindowLevel::Normal);
                                }
                            }
                            // Snap look to neutral (escapes stale pre-minimize target)
                            app.look.target.reset();
                        }

                        let _ = surface.swap_buffers(&gl_context);
                    }
                    _ => {
                        // Always track mouse for look (before egui may consume the event)
                        if let WindowEvent::CursorMoved { position, .. } = &event {
                            let (mx, my) = (position.x, position.y);
                            app.last_mouse_x = mx;
                            app.last_mouse_y = my;
                            let size = window.inner_size();
                            app.update_mouse_for_look(
                                mx,
                                my,
                                size.width as f32,
                                size.height as f32,
                            );
                            // V2 head tracking: feed mouse to drag manager (screen → scene internally)
                            if app.is_v2 {
                                if let Some(ref mut v2) = app.v2_model {
                                    v2.drag(mx as f32, my as f32);
                                }
                                app.handle_v2_hover(mx, my);
                            }
                        }

                        let egui_consumed = egui_state.on_window_event(&window, &event).consumed;

                        if !egui_consumed {
                            match event {
                                WindowEvent::CloseRequested => {
                                    if app.pet_mode == PetMode::Windowed {
                                        if app.minimized_to_float {
                                            app.request_restore = true;
                                        } else {
                                            window.set_visible(false);
                                        }
                                    } else {
                                        target.exit();
                                    }
                                }
                                WindowEvent::Resized(size) => {
                                    // GNOME/KDE AlwaysOnTop uses set_minimized, not 1×1 — skip the guard
                                    let skip_guard = app.pet_mode == PetMode::AlwaysOnTop
                                        && (app::is_gnome() || app::is_kde() || app::is_mango());
                                    if app.minimized_to_float && !skip_guard {
                                        let float_logical = if app.pet_mode == PetMode::AlwaysOnTop {
                                            1.0
                                        } else {
                                            50.0
                                        };
                                        let max_phys =
                                            (float_logical * window.scale_factor()).ceil() as u32;
                                        if size.width > max_phys || size.height > max_phys {
                                            let _ = window.request_inner_size(
                                                winit::dpi::LogicalSize::new(
                                                    float_logical,
                                                    float_logical,
                                                ),
                                            );
                                            if let (Some(rw), Some(rh)) = (
                                                NonZeroU32::new(max_phys.max(1)),
                                                NonZeroU32::new(max_phys.max(1)),
                                            ) {
                                                surface.resize(&gl_context, rw, rh);
                                            }
                                            return;
                                        }
                                    }
                                    if let (Some(w), Some(h)) =
                                        (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
                                    {
                                        surface.resize(&gl_context, w, h);
                                    }
                                    unsafe {
                                        gl.viewport(0, 0, size.width as i32, size.height as i32);
                                    }
                                    // V2: update matrix manager projection on resize
                                    if app.is_v2 {
                                        if let Some(ref mut v2) = app.v2_model {
                                            let (vw, vh) = (size.width as i32, size.height as i32);
                                            v2.resize(vw, vh);
                                            app.last_v2_size = (vw, vh);
                                        }
                                    }
                                    // Recalculate camera when window size changes (skip when floating)
                                    if !app.minimized_to_float {
                                        app.camera_needs_fit = true;
                                    }
                                    // Resize confirmed — window_size_override no longer needed
                                    app.window_size_override = None;
                                }
                                WindowEvent::KeyboardInput { event: ref ke, .. } => {
                                    if ke.state == ElementState::Pressed {
                                        use winit::keyboard::KeyCode;
                                        if let winit::keyboard::PhysicalKey::Code(code) =
                                            ke.physical_key
                                        {
                                            if code == KeyCode::ArrowLeft {
                                                let idx = app.current_idx.unwrap_or(0);
                                                if idx > 0 {
                                                    let _ = app.begin_switch(idx - 1);
                                                }
                                            } else if code == KeyCode::ArrowRight {
                                                let idx = app.current_idx.unwrap_or(0);
                                                if idx + 1 < app.model_list.len() {
                                                    let _ = app.begin_switch(idx + 1);
                                                }
                                            }
                                            #[cfg(feature = "capture")]
                                            if code == KeyCode::F9 {
                                                if app.is_capturing() {
                                                    app.stop_capture();
                                                } else {
                                                    app.start_capture();
                                                }
                                            }
                                            #[cfg(feature = "capture")]
                                            if code == KeyCode::F10 {
                                                app.trigger_vision_snapshot();
                                            }
                                        }
                                    }
                                }
                                WindowEvent::MouseWheel { delta, .. } => {
                                    if app.pet_mode != PetMode::Windowed {
                                        let d = match delta {
                                            winit::event::MouseScrollDelta::LineDelta(_, y) => y,
                                            winit::event::MouseScrollDelta::PixelDelta(p) => {
                                                p.y as f32
                                            }
                                        };
                                        if app.is_v2 {
                                            // V2 zoom via MatrixManager.setScale (tracked in app.v2_scale)
                                            app.v2_scale =
                                                (app.v2_scale + d * 0.15).clamp(0.1, 10.0);
                                            if let Some(ref mut v2) = app.v2_model {
                                                v2.set_scale(app.v2_scale);
                                            }
                                            app.save_zoom();
                                        } else {
                                            app.camera.zoom(d, 0.5, 0.5);
                                            app.save_zoom();
                                        }
                                    }
                                }
                                WindowEvent::CursorMoved { position, .. } => {
                                    // Camera pan (mouse_down in normal mode) — V2 uses drag() above instead
                                    if app.mouse_down && app.pet_mode != PetMode::Windowed && !app.is_v2 {
                                        let dx = position.x - app.last_mouse_x;
                                        let dy = position.y - app.last_mouse_y;
                                        app.camera.pan(dx as f32, dy as f32);
                                    }
                                }
                                WindowEvent::MouseInput { state, .. } => {
                                    let was_down = app.mouse_down;
                                    app.mouse_down = state == ElementState::Pressed;
                                    if state == ElementState::Pressed && !was_down {
                                        let size = window.inner_size();
                                        let mx = app.last_mouse_x;
                                        let my = app.last_mouse_y;
                                        let cam_scale_x = app.camera.scale_x;
                                        let cam_scale_y = app.camera.scale_y;
                                        let cam_trans_x = app.camera.translate_x;
                                        let cam_trans_y = app.camera.translate_y;
                                        app.handle_tap_with_cam(
                                            mx,
                                            my,
                                            size.width as f32,
                                            size.height as f32,
                                            cam_scale_x,
                                            cam_scale_y,
                                            cam_trans_x,
                                            cam_trans_y,
                                        );
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            Event::UserEvent(event) => match event {
                tray::AppEvent::ShowWindow => {
                    // On Wayland, the tray thread handles ShowWindow directly
                    // via process respawn — this code only runs on non-Wayland.
                    app.request_restore = true;
                }
                tray::AppEvent::ToggleClickThrough => {
                    app.click_through = !app.click_through;
                    let _ = window.set_cursor_hittest(!app.click_through);
                    // Sync to pet thread if active
                    #[cfg(target_os = "linux")]
                    if let Some(ref tx) = app.pet_wayland_cmd_tx {
                        let _ = tx.send(
                            crate::wayland_pet::PetCommand::SetClickThrough(app.click_through),
                        );
                    }
                    log::info!(
                        "[click-through] {}",
                        if app.click_through {
                            "enabled"
                        } else {
                            "disabled"
                        }
                    );
                }
                tray::AppEvent::ToggleWindowedPet => {
                    if app.pet_mode == PetMode::Windowed {
                        app.pet_mode = PetMode::Off;
                    } else {
                        app.pet_mode = PetMode::Windowed;
                    }
                    app.pet_mode_changed = true;
                }
                tray::AppEvent::ToggleAlwaysOnTopPet => {
                    if app.pet_mode == PetMode::AlwaysOnTop {
                        app.pet_mode = PetMode::Off;
                    } else {
                        app.pet_mode = PetMode::AlwaysOnTop;
                    }
                    app.pet_mode_changed = true;
                }
                tray::AppEvent::Quit => {
                    #[cfg(target_os = "linux")]
                    {
                        detach_always_on_top(&mut app);
                        gnome_x11_display = None;
                        gnome_x11_window = None;
                    }
                    // On Wayland, target.exit() may not take effect when the window is
                    // minimized because the event loop is blocked on wl_dispatch and
                    // EventLoopProxy cannot reliably wake it up in winit 0.29.
                    // Force immediate exit to work around this.
                    std::process::exit(0);
                }
            },
            Event::LoopExiting => {
                #[cfg(target_os = "linux")]
                {
                    gnome_x11_display = None;
                    gnome_x11_window = None;
                }
                // Save last active model path
                if let Some(idx) = app.current_idx {
                    if let Some(entry) = app.model_list.get(idx) {
                        if let Some(ref db) = app.db {
                            let _ = db.set_setting(
                                "last_active_model_path",
                                &entry.dir.to_string_lossy(),
                            );
                        }
                    }
                }
                // Save AI config (to DB + JSON file)
                crate::ai::config::save_config(&app.ai_config, app.db.as_deref());
                painter.destroy();
            }
            Event::AboutToWait => {
                // Note: tray events are forwarded by a background thread directly
                // through the EventLoopProxy.  No polling needed here.

                // Check for pet thread events (take scratch buffer to release app borrow)
                #[cfg(target_os = "linux")]
                {
                    let mut pet_events = std::mem::take(&mut app.pet_events_scratch);
                    if let Some(ref rx) = app.pet_wayland_event_rx {
                        while let Ok(event) = rx.try_recv() {
                            pet_events.push(event);
                        }
                    }
                    for event in pet_events.drain(..) {
                        match event {
                            crate::wayland_pet::PetEvent::Configured { width, height } => {
                                log::info!(
                                    "[pet/wayland] surface configured: {width}x{height}"
                                );
                            }
                            crate::wayland_pet::PetEvent::Tap {
                                x, y, w, h,
                                cam_scale_x, cam_scale_y,
                                cam_translate_x, cam_translate_y,
                            } => {
                                app.handle_tap_with_cam(
                                    x, y, w, h,
                                    cam_scale_x, cam_scale_y,
                                    cam_translate_x, cam_translate_y,
                                );
                            }
                            crate::wayland_pet::PetEvent::Error(e) => {
                                log::error!("[pet/wayland] error: {e}");
                                window.set_visible(true);
                            }
                            crate::wayland_pet::PetEvent::CursorMoved { x, y, w, h } => {
                                app.update_mouse_for_look(x, y, w, h);
                            }
                            crate::wayland_pet::PetEvent::ToolbarAction(action) => {
                                match action {
                                    crate::toolbar::ToolbarAction::PrevModel => {
                                        if let Some(idx) = app.current_idx {
                                            if idx > 0 {
                                                let _ = app.begin_switch(idx - 1);
                                            }
                                        }
                                    }
                                    crate::toolbar::ToolbarAction::NextModel => {
                                        if let Some(idx) = app.current_idx {
                                            if idx + 1 < app.model_list.len() {
                                                let _ = app.begin_switch(idx + 1);
                                            }
                                        }
                                    }
                                    crate::toolbar::ToolbarAction::ExitPet => {
                                        app.pet_mode = PetMode::Off;
                                        app.pet_mode_changed = true;
                                    }
                                    crate::toolbar::ToolbarAction::Search => {
                                        // Send model list to pet thread for its search panel
                                        if let (Some(db), Some(tx)) =
                                            (app.db.as_ref(), app.pet_wayland_cmd_tx.as_ref())
                                        {
                                            if let Ok(records) = db.model_history() {
                                                let entries: Vec<_> = records
                                                    .into_iter()
                                                    .map(|r| (r.file_path.clone(), r.name.clone()))
                                                    .collect();
                                                let _ = tx.send(
                                                    crate::wayland_pet::PetCommand::ModelList(entries),
                                                );
                                            }
                                        }
                                    }
                                    _ => {} // local actions handled in pet thread
                                }
                            }
                            crate::wayland_pet::PetEvent::Exited => {
                                log::info!("[pet/wayland] thread exited");
                            }
                        }
                    }
                    // Return the (now-empty) buffer to app for reuse next frame
                    app.pet_events_scratch = pet_events;
                }

                // Drain captured frames from the capture thread, keep only the latest
                #[cfg(feature = "capture")]
                {
                    app.drain_capture_frames();
                    app.tick_vision_timer();
                }

                // GNOME XWayland periodic XRaiseWindow — keeps the AlwaysOnTop window
                // above others when Mutter is running on Wayland.  With override_redirect
                // enabled, XRaiseWindow directly manipulates X11 stacking, and Mutter's
                // stack-tracker handles override-redirect XWayland windows correctly.
                #[cfg(target_os = "linux")]
                if let Some(dsp) = gnome_x11_display {
                    if let Some(xw) = gnome_x11_window {
                        let now = Instant::now();
                        if now - last_x11_raise >= std::time::Duration::from_millis(100) {
                            unsafe {
                                x11::xlib::XRaiseWindow(dsp, xw);
                                x11::xlib::XFlush(dsp);
                            }
                            last_x11_raise = now;
                        }
                    }
                }

                window.request_redraw();
            }
            _ => {}
        }
    })?;
    Ok(())
}
