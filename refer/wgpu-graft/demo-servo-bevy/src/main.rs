// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! Demo embedding Servo in a [Bevy] app, zero-copy.
//!
//! Bevy's render world runs on a separate thread, and Servo's surfman/GL context
//! is `!Send`, so they can't share the import in-process. Instead this uses the
//! shared-handle seam (the same reason the iced demo needs it):
//!
//! - Servo lives in the **main world** as a `NonSend` resource. A main-world
//!   system paints it and exports a cloneable D3D12 shared-resource token.
//! - An `ExtractSchedule` system carries the token into the **render world**.
//! - A render-world system (after `PrepareAssets`, before `Queue`) opens the
//!   resource on Bevy's `RenderDevice` and injects a `GpuImage` into
//!   `RenderAssets<GpuImage>` for a `Sprite`'s `Handle<Image>`. The sprite
//!   covers the window below a top URL bar (Bevy UI), which shows the current
//!   URL and navigates on Enter.
//!
//! surfman/ANGLE is LUID-anchored to a throwaway HighPerformance-DX12 device and
//! Bevy is forced to DX12 + HighPerformance, so both land on the same GPU.
//! Windows + DX12 only.

#![allow(clippy::type_complexity)]

use bevy::asset::RenderAssetUsages;
use bevy::clipboard::{Clipboard, ClipboardRead};
use bevy::ecs::message::MessageReader;
use bevy::image::Image;
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key as BevyKey, KeyboardInput};
use bevy::input::mouse::{MouseButtonInput, MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::render::settings::{Backends, PowerPreference, RenderCreation, WgpuSettings};
use bevy::render::texture::GpuImage;
use bevy::render::{Extract, ExtractSchedule, Render, RenderApp, RenderPlugin, RenderSystems};
use bevy::text::{FontSize, TextColor, TextFont};
use bevy::ui::widget::Text as UiText;
use bevy::ui::{AlignItems, BackgroundColor, Node, PositionType, UiRect, Val};
use bevy::window::{PrimaryWindow, WindowResolution};
#[cfg(not(target_os = "windows"))]
use grafting::SyncMechanism;
#[cfg(target_os = "windows")]
use grafting::{Dx12SharedTexture, HostWgpuContext, SyncMechanism, import_dx12_shared_texture};
use rustls::crypto::aws_lc_rs;
use servo::{
    CompositionEvent, CompositionState, CreateNewWebViewRequest, DevicePoint, EmbedderControl,
    EmbedderControlId, EventLoopWaker, ImeEvent, InputEvent, MouseButton as ServoMouseButton,
    MouseButtonAction, MouseButtonEvent, MouseLeftViewportEvent, MouseMoveEvent, Servo, ServoBuilder,
    WebView, WebViewBuilder, WebViewDelegate, WheelDelta, WheelEvent, WheelMode,
};
use servo_wgpu_interop_adapter::ServoWgpuInteropAdapter;
use std::cell::RefCell;
use std::rc::Rc;
use url::Url;
use winit::dpi::PhysicalSize;

mod keyutils;

const DEFAULT_WIDTH: u32 = 1280;
const DEFAULT_HEIGHT: u32 = 800;

/// User-Agent sent on HTTP requests and reported to `navigator.userAgent`.
/// Servo's default advertises `Servo/{version} Firefox/...`; spoof a desktop
/// Chrome string so sites serve their Chrome-compatible variants. Override at
/// runtime with `DEMO_USER_AGENT`.
const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

// ── Servo side (main world, !Send) ───────────────────────────────────────────

/// Non-`Send` Servo state, held as a `NonSend` resource on the main thread.
struct ServoState {
    servo: Servo,
    webview: WebView,
    interop: ServoWgpuInteropAdapter,
    size: PhysicalSize<u32>,
    delegate: Rc<DemoDelegate>,
    #[cfg(not(target_os = "windows"))]
    generation: u64,
}

/// The latest exported shared-handle frame (main world). `Send`.
#[derive(Resource, Default, Clone)]
struct ServoFrame(Option<FrameDesc>);

#[derive(Clone)]
struct FrameDesc {
    #[cfg(target_os = "windows")]
    resource: grafting::Dx12SharedResource,
    // Linux: CPU-readback pixels (top-left Rgba8, already Y-flipped by
    // `read_full_frame`). Carried main→render world, uploaded via
    // `queue.write_texture` into Bevy's stable GpuImage.
    #[cfg(not(target_os = "windows"))]
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    generation: u64,
}

/// The placeholder image the Servo frame is injected into (main world).
#[derive(Resource, Clone)]
struct ServoImage(Handle<Image>);

// ── Browser chrome: URL bar ─────────────────────────────────────────────────

/// Logical height of the top browser chrome. The Servo viewport occupies the
/// area below it, so all page coordinates are offset by this amount.
const URL_BAR_HEIGHT: f32 = 36.0;

// Toolbar geometry (logical px). Chrome nodes are positioned with these same
// constants so click hit-testing (`classify_bar_click`) matches rendering.
const TOOLBAR_PAD: f32 = 8.0;
const TOOLBAR_BUTTON_W: f32 = 34.0;
const TOOLBAR_BUTTON_H: f32 = 26.0;
const TOOLBAR_GAP: f32 = 4.0;
const FIELD_ROW_TOP: f32 = (URL_BAR_HEIGHT - TOOLBAR_BUTTON_H) / 2.0;
const GO_BUTTON_W: f32 = 46.0;
const EDGE_MARGIN: f32 = 8.0;

/// The four fixed navigation buttons on the left of the toolbar, in order.
const NAV_BUTTONS: [&str; 4] = ["<", ">", "R", "H"];

/// Actions the chrome buttons (and URL "Go") can queue for `finish_nav`.
enum NavCommand {
    Load(String),
    Back,
    Forward,
    Reload,
    Home,
}

/// Input routing: `true` sends keyboard/IME/mouse to the web page (Servo),
/// `false` keeps them on the URL bar (which is mid-edit).
#[derive(Resource)]
struct ChromeFocus {
    page: bool,
}

impl Default for ChromeFocus {
    fn default() -> Self {
        Self { page: true }
    }
}

/// The URL bar's edit state while `editing`.
///
/// Editing model (like a browser address bar): focusing the bar selects the
/// whole URL, so the next keystroke / paste / IME commit replaces it. `text`
/// holds committed content and `composing` the live IME preedit (rendered
/// inline after `text` until the composition commits or is dismissed).
#[derive(Resource, Default)]
struct UrlBarState {
    editing: bool,
    select_all: bool,
    text: String,
    composing: String,
}

impl UrlBarState {
    /// The visible bar content: committed text plus any in-progress IME
    /// composition.
    fn visible(&self) -> String {
        format!("{}{}", self.text, self.composing)
    }

    /// Replace the whole content (used when typing over the select-all state).
    fn replace_all(&mut self, content: String) {
        self.text = content;
        self.composing.clear();
        self.select_all = false;
    }

    /// Delete the selected content (backspace/cut with select-all), leaving an
    /// empty field positioned for new input.
    fn clear_selection(&mut self) {
        self.text.clear();
        self.composing.clear();
        self.select_all = false;
    }
}

/// The URL currently displayed in the bar, fed from the Servo delegate's
/// `notify_url_changed`. Send-safe mirror so input systems never touch the
/// `!Send` delegate.
#[derive(Resource, Default)]
struct CurrentUrl(String);

/// The URL the Home button navigates to (seeded from the startup URL).
#[derive(Resource)]
struct HomeUrl(String);

/// A navigation queued by the chrome (URL bar submit, Go button, or a
/// nav-button click), executed by `finish_nav`.
#[derive(Resource, Default)]
struct PendingNav(Option<NavCommand>);

/// Marker on the URL bar's text node.
#[derive(Component)]
struct UrlLabel;

/// Marker on the URL field container (width is laid out per window size).
#[derive(Component)]
struct UrlField;

/// Marker on the Go button (positioned per window size).
#[derive(Component)]
struct GoButton;

// ── Render world mirrors (filled by ExtractSchedule) ─────────────────────────

#[derive(Resource, Default, Clone)]
struct ExtractedFrame(Option<FrameDesc>);

#[derive(Resource, Default, Clone, Copy)]
struct ExtractedImageId(Option<AssetId<Image>>);

// ── Main ─────────────────────────────────────────────────────────────────────

fn main() {
    aws_lc_rs::default_provider()
        .install_default()
        .expect("failed to install rustls crypto provider");

    let initial_url = demo_support::resolve_initial_url(env!("CARGO_MANIFEST_DIR"))
        .expect("failed to resolve initial URL");

    // Anchor surfman/ANGLE to a HighPerformance-DX12 GPU and run Servo on it.
    // Bevy (forced to DX12 + HighPerformance below) lands on the same GPU, so the
    // shared handle opened on Bevy's RenderDevice stays single-GPU.
    let servo_state = build_servo(&initial_url);

    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "demo-servo-bevy".into(),
                    resolution: WindowResolution::new(DEFAULT_WIDTH, DEFAULT_HEIGHT),
                    // IME is enabled lazily: Servo reports when an editable field
                    // gains focus (via show_embedder_control), and apply_ime_window
                    // then turns `ime_enabled` on and anchors the candidate box at
                    // the field. Keeping it off otherwise stops an active Chinese
                    // IME from swallowing keys typed outside editable content.
                    ..default()
                }),
                ..default()
            })
            .set(RenderPlugin {
                // Windows: force DX12 + HighPerformance so Bevy's GPU matches
                // surfman's (shared NT handle stays single-GPU).
                // Linux: leave backend selection to wgpu (Vulkan/GL) — the
                // DX12 shared-handle path does not exist here; frames travel
                // via CPU readback instead.
                render_creation: RenderCreation::Automatic(Box::new({
                    #[cfg(target_os = "windows")]
                    {
                        WgpuSettings {
                            backends: Some(Backends::DX12),
                            power_preference: PowerPreference::HighPerformance,
                            ..default()
                        }
                    }
                    #[cfg(not(target_os = "windows"))]
                    {
                        WgpuSettings {
                            backends: Some(Backends::all()),
                            power_preference: PowerPreference::HighPerformance,
                            ..default()
                        }
                    }
                })),
                ..default()
            }),
    );

    app.insert_non_send(servo_state)
        .init_resource::<ServoFrame>()
        .init_resource::<ChromeFocus>()
        .init_resource::<UrlBarState>()
        .insert_resource(CurrentUrl(initial_url.to_string()))
        .insert_resource(HomeUrl(initial_url.to_string()))
        .init_resource::<PendingNav>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                // URL bar first: it owns clicks that grab/relinquish chrome
                // focus, so `forward_input` sees the final focus state.
                url_bar_input,
                forward_input.after(url_bar_input).before(drive_servo),
                finish_nav.after(forward_input).before(drive_servo),
                drive_servo,
                sync_url_and_label.after(drive_servo),
                apply_ime_window.after(drive_servo),
                resize_servo_image,
                layout_url_chrome,
                fit_sprite_to_window,
            ),
        );

    // Render world: extract the handle, then inject the imported texture.
    // `get_sub_app_mut` instead of `sub_app_mut`: if renderer creation
    // failed (no GPU backend), RenderApp never exists — log and run
    // without the render-world injection rather than panicking.
    if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
        render_app
            .init_resource::<ExtractedFrame>()
            .init_resource::<ExtractedImageId>()
            .add_systems(ExtractSchedule, extract_servo_frame)
            .add_systems(
                Render,
                inject_servo_image
                    .after(RenderSystems::PrepareAssets)
                    .before(RenderSystems::Queue),
            );
    } else {
        eprintln!(
            "[bevy] RenderApp missing (renderer creation failed) — running without Servo frame injection"
        );
    }

    app.run();
}

fn build_servo(initial_url: &Url) -> ServoState {
    let size = PhysicalSize::new(DEFAULT_WIDTH, DEFAULT_HEIGHT);

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        #[cfg(target_os = "windows")]
        backends: wgpu::Backends::DX12,
        // Linux: Vulkan when available, GL otherwise. `Backends::all()`
        // lets wgpu fall back instead of erroring with NotFound like a
        // hard-coded DX12 request does.
        #[cfg(not(target_os = "windows"))]
        backends: wgpu::Backends::all(),
        flags: wgpu::InstanceFlags::default(),
        memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
        backend_options: wgpu::BackendOptions::default(),
        display: None,
    });
    #[cfg(target_os = "windows")]
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
    }))
    .expect("no DX12 adapter for LUID anchoring");
    #[cfg(not(target_os = "windows"))]
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
    }))
    .expect("no Vulkan/GL adapter for Servo interop (check GPU drivers)");
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("servo-bevy-luid-anchor"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    }))
    .expect("failed to create LUID-anchor device");

    let interop = ServoWgpuInteropAdapter::new(device, queue, size)
        .expect("failed to create Servo interop adapter");

    let servo = ServoBuilder::default()
        .preferences(servo::Preferences {
            user_agent: std::env::var("DEMO_USER_AGENT")
                .unwrap_or_else(|_| DEFAULT_USER_AGENT.to_string()),
            ..Default::default()
        })
        .event_loop_waker(Box::new(NoopWaker))
        .build();
    servo.setup_logging();

    let rendering_context = interop.rendering_context();
    let delegate = Rc::new(DemoDelegate::new(rendering_context.clone(), initial_url.to_string()));
    let webview = WebViewBuilder::new(&servo, rendering_context)
        .url(initial_url.clone())
        .hidpi_scale_factor(euclid::Scale::new(1.0))
        .delegate(delegate.clone())
        .build();

    ServoState {
        servo,
        webview,
        interop,
        size,
        delegate,
        #[cfg(not(target_os = "windows"))]
        generation: 0,
    }
}

// ── Startup: camera + fullscreen sprite on a placeholder image ───────────────

fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    commands.spawn(Camera2d);

    // Bevy owns this texture; the render world COPIES the imported Servo frame
    // into it each frame (so the sprite's bind group aliases a stable Bevy
    // texture, not the short-lived shared-handle import). RENDER_WORLD only (no
    // CPU data); needs COPY_DST for the copy and TEXTURE_BINDING to be sampled.
    let mut placeholder = Image::new_uninit(
        Extent3d {
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    placeholder.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST;
    let handle = images.add(placeholder);
    commands.insert_resource(ServoImage(handle.clone()));

    let size = windows
        .single()
        .map(|w| Vec2::new(w.width(), w.height()))
        .unwrap_or(Vec2::new(DEFAULT_WIDTH as f32, DEFAULT_HEIGHT as f32));

    // The Servo viewport is the window minus the top URL bar. The sprite is
    // inset to that area and nudged down so its top edge sits below the bar.
    let content_size = Vec2::new(size.x, (size.y - URL_BAR_HEIGHT).max(1.0));

    // Windows DX12 shared texture is bottom-left origin → flip so the page
    // displays upright. Linux CPU readback (`read_full_frame`) is already
    // Y-flipped to top-left, so no flip there.
    #[allow(unused_mut)]
    let mut sprite = Sprite {
        image: handle,
        custom_size: Some(content_size),
        ..default()
    };
    #[cfg(target_os = "windows")]
    {
        sprite.flip_y = true;
    }
    commands.spawn((
        sprite,
        Transform::from_xyz(0.0, -URL_BAR_HEIGHT / 2.0, 0.0),
    ));

    // ── Browser chrome: navigation buttons, URL field, Go button ─────────
    // One full-width root container holds the toolbar. Children are absolute
    // and positioned with the same `toolbar_geometry` constants used by
    // `classify_bar_click`, so hit tests always match what is rendered. Field
    // width and the Go button's right edge depend on the window width and are
    // refreshed each frame by `layout_url_chrome`.
    let geom = toolbar_geometry(size.x);

    commands
        .spawn((
            Node {
                position_type: PositionType::Relative,
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Px(URL_BAR_HEIGHT),
                ..default()
            },
            BackgroundColor(Color::srgb(0.13, 0.14, 0.16)),
        ))
        .with_children(|bar| {
            // Fixed navigation buttons on the left: back, forward, reload, home.
            for (i, label) in NAV_BUTTONS.iter().enumerate() {
                let x = TOOLBAR_PAD + i as f32 * (TOOLBAR_BUTTON_W + TOOLBAR_GAP);
                bar.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        top: Val::Px(FIELD_ROW_TOP),
                        left: Val::Px(x),
                        width: Val::Px(TOOLBAR_BUTTON_W),
                        height: Val::Px(TOOLBAR_BUTTON_H),
                        display: bevy::ui::Display::Flex,
                        align_items: AlignItems::Center,
                        justify_content: bevy::ui::JustifyContent::Center,
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.22, 0.24, 0.28)),
                ))
                .with_children(|button| {
                    button.spawn((
                        UiText::new(*label),
                        TextFont {
                            font_size: FontSize::Px(13.0),
                            ..default()
                        },
                        TextColor(Color::srgb(0.85, 0.88, 0.92)),
                    ));
                });
            }

            // URL field: dark strip with the label text inside.
            bar.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(FIELD_ROW_TOP),
                    left: Val::Px(geom.field_x),
                    width: Val::Px((geom.field_right - geom.field_x).max(1.0)),
                    height: Val::Px(TOOLBAR_BUTTON_H),
                    padding: UiRect::horizontal(Val::Px(10.0)),
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(Color::srgb(0.07, 0.08, 0.10)),
                UrlField,
            ))
            .with_children(|field| {
                field.spawn((
                    UiText::new(""),
                    TextFont {
                        font_size: FontSize::Px(14.0),
                        ..default()
                    },
                    TextColor(Color::srgb(0.92, 0.92, 0.92)),
                    UrlLabel,
                ));
            });

            // 转到 (Go) button at the right edge of the URL field.
            bar.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(FIELD_ROW_TOP),
                    left: Val::Px(geom.go_x),
                    width: Val::Px(GO_BUTTON_W),
                    height: Val::Px(TOOLBAR_BUTTON_H),
                    display: bevy::ui::Display::Flex,
                    align_items: AlignItems::Center,
                    justify_content: bevy::ui::JustifyContent::Center,
                    ..default()
                },
                BackgroundColor(Color::srgb(0.22, 0.24, 0.28)),
                GoButton,
            ))
            .with_children(|button| {
                button.spawn((
                    UiText::new("Go"),
                    TextFont {
                        font_size: FontSize::Px(13.0),
                        ..default()
                    },
                    TextColor(Color::srgb(0.85, 0.88, 0.92)),
                ));
            });
        });
}

// ── Update (main world): forward Bevy input, drive Servo, fit the sprite ─────

fn forward_input(
    servo: NonSendMut<ServoState>,
    windows: Query<&Window, With<PrimaryWindow>>,
    primary_window: Query<Entity, With<PrimaryWindow>>,
    chrome: Res<ChromeFocus>,
    mut keyboard: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    mut ime: MessageReader<Ime>,
    mut buttons: MessageReader<MouseButtonInput>,
    mut wheels: MessageReader<MouseWheel>,
    mut last_content: Local<Option<Vec2>>,
    mut prev_page: Local<bool>,
) {
    if !chrome.page {
        *prev_page = false;
        // The URL bar is being edited; it owns keyboard/IME/mouse this frame.
        return;
    }

    if !*prev_page {
        // Focus just returned to the page. This system's message cursors did
        // not advance while the bar was editing, so drop the buffered
        // keystrokes/IME from that period before forwarding resumes.
        for _ in keyboard.read() {}
        for _ in ime.read() {}
    }
    *prev_page = true;

    let Ok(window) = windows.single() else {
        return;
    };
    let Ok(primary) = primary_window.single() else {
        return;
    };

    // Physical cursor mapped into the Servo viewport, which starts below the
    // URL bar. Positions on the bar itself are treated as "outside" the page.
    let bar_phys = URL_BAR_HEIGHT * window.scale_factor();
    let content = window
        .physical_cursor_position()
        .filter(|p| p.y >= bar_phys)
        .map(|p| Vec2::new(p.x, p.y - bar_phys));

    // ── Keyboard: letters, digits, space, backspace, … ───────────────────
    // Converted via keyutils (Bevy 0.19 → Servo keyboard_types mapping).
    // Reaches Servo while the page owns focus regardless of pointer position
    // (the URL-bar edit frames already returned above).
    for ev in keyboard.read() {
        if ev.window != primary {
            continue;
        }
        let kbd = keyutils::keyboard_event_from_bevy(ev, &keys);
        servo.webview.notify_input_event(InputEvent::Keyboard(kbd));
    }

    // ── IME ───────────────────────────────────────────────────────────────
    // Mirrors servoshell's winit→Servo mapping: Enabled → Composition::Start,
    // Preedit → Composition::Update, Commit → Composition::End. Disabled only
    // echoes Dismissed when the *user* dismissed the IME (an InputMethod
    // control was visible); when Servo itself moved focus it already sent
    // hide_embedder_control, and echoing Dismissed would blur the newly
    // focused element. Composition text is inserted by Servo via this path,
    // not via Keyboard events.
    for ev in ime.read() {
        let (window, event) = match ev {
            Ime::Enabled { window } => (
                *window,
                Some(ImeEvent::Composition(CompositionEvent {
                    state: CompositionState::Start,
                    data: String::new(),
                })),
            ),
            Ime::Preedit { window, value, .. } => (
                *window,
                Some(ImeEvent::Composition(CompositionEvent {
                    state: CompositionState::Update,
                    data: value.clone(),
                })),
            ),
            Ime::Commit { window, value } => (
                *window,
                Some(ImeEvent::Composition(CompositionEvent {
                    state: CompositionState::End,
                    data: value.clone(),
                })),
            ),
            Ime::Disabled { window } => {
                let user_dismissed = servo.delegate.ime.borrow_mut().control_id.take().is_some();
                (*window, user_dismissed.then_some(ImeEvent::Dismissed))
            }
        };
        let Some(event) = event else {
            continue;
        };
        if window != primary {
            continue;
        }
        servo.webview.notify_input_event(InputEvent::Ime(event));
    }

    match (content, *last_content) {
        (Some(p), prev) if Some(p) != prev => {
            //eprintln!("[bevy-input] move {}x{}", p.x as u32, p.y as u32);
            servo
                .webview
                .notify_input_event(InputEvent::MouseMove(MouseMoveEvent::new(
                    servo::WebViewPoint::Device(DevicePoint::new(p.x, p.y)),
                )));
        }
        (None, Some(_)) => {
            //eprintln!("[bevy-input] leave");
            servo
                .webview
                .notify_input_event(InputEvent::MouseLeftViewport(
                    MouseLeftViewportEvent::default(),
                ));
        }
        _ => {}
    }
    *last_content = content;

    // Button and wheel events only reach Servo over the page area; clicks on
    // the URL bar were consumed by `url_bar_input` (which runs first and owns
    // the focus switch), so they never get here.
    let Some(pos) = content else {
        return;
    };
    let point = DevicePoint::new(pos.x, pos.y);

    for ev in buttons.read() {
        let servo_button = match ev.button {
            bevy::prelude::MouseButton::Left => ServoMouseButton::Left,
            bevy::prelude::MouseButton::Right => ServoMouseButton::Right,
            bevy::prelude::MouseButton::Middle => ServoMouseButton::Middle,
            _ => continue,
        };
        let action = match ev.state {
            ButtonState::Pressed => MouseButtonAction::Down,
            ButtonState::Released => MouseButtonAction::Up,
        };
        servo
            .webview
            .notify_input_event(InputEvent::MouseButton(MouseButtonEvent::new(
                action,
                servo_button,
                servo::WebViewPoint::Device(point),
            )));
    }

    for ev in wheels.read() {
        let (dx, dy, mode) = match ev.unit {
            MouseScrollUnit::Line => (
                f64::from(ev.x) * 38.0,
                f64::from(ev.y) * 38.0,
                WheelMode::DeltaLine,
            ),
            MouseScrollUnit::Pixel => (f64::from(ev.x), f64::from(ev.y), WheelMode::DeltaPixel),
        };
        servo
            .webview
            .notify_input_event(InputEvent::Wheel(WheelEvent::new(
                WheelDelta {
                    x: dx,
                    y: dy,
                    z: 0.0,
                    mode,
                },
                servo::WebViewPoint::Device(point),
            )));
    }
}

// ── URL bar chrome ──────────────────────────────────────────────────────────

/// What a click on the chrome hit. Mirrors the layout produced in `setup` from
/// the same geometry constants.
enum BarRegion {
    Field,
    Go,
    Nav(usize), // index into NAV_BUTTONS (0 back, 1 forward, 2 reload, 3 home)
}

/// Toolbar geometry for the current window width.
struct ToolbarGeom {
    nav_button_x: [f32; 4],
    field_x: f32,
    field_right: f32,
    go_x: f32,
}

fn toolbar_geometry(win_width: f32) -> ToolbarGeom {
    let nav_button_x = core::array::from_fn(|i| TOOLBAR_PAD + i as f32 * (TOOLBAR_BUTTON_W + TOOLBAR_GAP));
    let field_x = TOOLBAR_PAD + 4.0 * (TOOLBAR_BUTTON_W + TOOLBAR_GAP);
    let go_x = win_width - EDGE_MARGIN - GO_BUTTON_W;
    ToolbarGeom {
        nav_button_x,
        field_x,
        field_right: go_x - TOOLBAR_GAP * 2.0,
        go_x,
    }
}

/// Classify a chrome click (logical window coords) into its target region.
fn classify_bar_click(pos: Vec2, geom: &ToolbarGeom) -> Option<BarRegion> {
    let row_hit = pos.y >= FIELD_ROW_TOP && pos.y <= FIELD_ROW_TOP + TOOLBAR_BUTTON_H;
    if !row_hit {
        return None;
    }
    for (i, x) in geom.nav_button_x.iter().enumerate() {
        if pos.x >= *x && pos.x <= *x + TOOLBAR_BUTTON_W {
            return Some(BarRegion::Nav(i));
        }
    }
    if pos.x >= geom.field_x && pos.x <= geom.field_right {
        return Some(BarRegion::Field);
    }
    if pos.x >= geom.go_x && pos.x <= geom.go_x + GO_BUTTON_W {
        return Some(BarRegion::Go);
    }
    None
}

/// Approximate horizontal extent of the visible bar text in logical px. Used
/// to anchor the IME candidate window near the end of the content.
fn visible_text_width(urlbar: &UrlBarState) -> f32 {
    const BAR_PADDING: f32 = 12.0;
    const MONO_ADVANCE: f32 = 9.0; // FiraMono at 15px: ~0.6em advance per glyph.
    let glyph_advance: f32 = urlbar
        .visible()
        .chars()
        .map(|c| if c.is_ascii() { MONO_ADVANCE } else { MONO_ADVANCE * 2.0 })
        .sum();
    BAR_PADDING + glyph_advance
}

/// Insert clipboard text, replacing the select-all content when present.
fn paste_into_bar(urlbar: &mut UrlBarState, text: &str) {
    if urlbar.select_all {
        urlbar.replace_all(text.to_string());
    } else {
        urlbar.text.push_str(text);
    }
}

/// The single ASCII character a `Character` key represents (lowercased), if
/// any — used to recognize Ctrl/Cmd shortcuts.
fn shortcut_char(key: &BevyKey) -> Option<char> {
    let BevyKey::Character(text) = key else {
        return None;
    };
    let mut chars = text.chars();
    let c = chars.next()?;
    chars.next().is_none().then(|| c.to_ascii_lowercase())
}

/// Owns clicks on the URL bar / page content (focus arbitration), edits the
/// bar's text (select-all address-bar model), runs IME composition, and
/// serves copy/paste shortcuts. Navigation is only *queued* here — loading
/// happens in `finish_nav`, after `forward_input`, so the Enter/click that
/// released focus is never forwarded to the page.
#[allow(clippy::too_many_arguments)]
fn url_bar_input(
    mut chrome: ResMut<ChromeFocus>,
    mut urlbar: ResMut<UrlBarState>,
    mut pending: ResMut<PendingNav>,
    current: Res<CurrentUrl>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    primary_window: Query<Entity, With<PrimaryWindow>>,
    mut buttons: MessageReader<MouseButtonInput>,
    mut keyboard: MessageReader<KeyboardInput>,
    mut ime: MessageReader<Ime>,
    mut clipboard: ResMut<Clipboard>,
    mut pending_paste: Local<Option<ClipboardRead>>,
    mut prev_editing: Local<bool>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let Ok(primary) = primary_window.single() else {
        return;
    };

    let win_width = window.width();
    let geom = toolbar_geometry(win_width);

    // A helper that ends URL editing (without submitting) and hands input back
    // to the page — used by chrome buttons, which act on the page regardless.
    let leave_editing = |chrome: &mut ChromeFocus, urlbar: &mut UrlBarState| {
        if !chrome.page {
            chrome.page = true;
            urlbar.editing = false;
            urlbar.select_all = false;
            urlbar.text.clear();
            urlbar.composing.clear();
        }
    };

    for ev in buttons.read() {
        if ev.window != primary
            || ev.state != ButtonState::Pressed
            || ev.button != bevy::prelude::MouseButton::Left
        {
            continue;
        }
        let Some(pos) = window.cursor_position() else {
            continue;
        };
        match classify_bar_click(pos, &geom) {
            Some(BarRegion::Field) => {
                if chrome.page {
                    // Begin editing: prefill the URL and select it all, so the
                    // next keystroke/paste replaces the old address.
                    chrome.page = false;
                    urlbar.editing = true;
                    urlbar.select_all = true;
                    urlbar.text = current.0.clone();
                    urlbar.composing.clear();
                }
            }
            Some(BarRegion::Go) => {
                // 转到: submit whatever the address field holds. While editing
                // this is the typed URL; otherwise it is the current page URL.
                if !chrome.page {
                    let raw = urlbar.visible();
                    leave_editing(&mut chrome, &mut urlbar);
                    let raw = raw.trim();
                    if !raw.is_empty() {
                        pending.0 = Some(NavCommand::Load(raw.to_string()));
                    }
                } else if !current.0.trim().is_empty() {
                    pending.0 = Some(NavCommand::Load(current.0.trim().to_string()));
                }
            }
            Some(BarRegion::Nav(index)) => {
                leave_editing(&mut chrome, &mut urlbar);
                let command = match index {
                    0 => NavCommand::Back,
                    1 => NavCommand::Forward,
                    2 => NavCommand::Reload,
                    _ => NavCommand::Home,
                };
                pending.0 = Some(command);
            }
            None if !chrome.page && pos.y >= URL_BAR_HEIGHT => {
                // Click on page content: hand input back to the web view.
                chrome.page = true;
                urlbar.editing = false;
                urlbar.composing.clear();
            }
            None => {}
        }
    }

    let started_editing = urlbar.editing && !*prev_editing;
    *prev_editing = urlbar.editing;
    if started_editing {
        urlbar.select_all = true;
        // Drop keystrokes/IME from the page-owned period; each system keeps its
        // own message cursor, so this reader's queue still holds them.
        for _ in keyboard.read() {}
        for _ in ime.read() {}
    }

    if !chrome.page {
        // Resolve any pending (async) clipboard paste from a previous frame.
        if let Some(read) = pending_paste.as_mut()
            && let Some(result) = read.poll_result()
        {
            match result {
                Ok(text) => paste_into_bar(&mut urlbar, &text),
                Err(e) => eprintln!("[bevy] clipboard read failed: {e:?}"),
            }
            *pending_paste = None;
        }

        // IME composition for the bar. Keys pressed while a composition is
        // active are handled by the OS IME (Preedit updates), so the raw
        // keyboard handling below skips edits while `composing` is non-empty.
        for ev in ime.read() {
            let window = match ev {
                Ime::Enabled { window } => *window,
                Ime::Preedit { window, .. } => *window,
                Ime::Commit { window, .. } => *window,
                Ime::Disabled { window } => *window,
            };
            if window != primary {
                continue;
            }
            match ev {
                Ime::Enabled { .. } => {}
                // First preedit with content replaces the select-all URL.
                Ime::Preedit { value, .. } if !value.is_empty() && urlbar.select_all => {
                    urlbar.clear_selection();
                    urlbar.composing = value.clone();
                }
                Ime::Preedit { value, .. } => urlbar.composing = value.clone(),
                Ime::Commit { value, .. } => {
                    if urlbar.select_all {
                        urlbar.replace_all(value.clone());
                    } else {
                        urlbar.composing.clear();
                        urlbar.text.push_str(value);
                    }
                }
                Ime::Disabled { .. } => urlbar.composing.clear(),
            }
        }

        let composing = !urlbar.composing.is_empty();
        let ctrl = keys.any_pressed([
            KeyCode::ControlLeft,
            KeyCode::ControlRight,
            KeyCode::SuperLeft,
            KeyCode::SuperRight,
        ]);

        for ev in keyboard.read() {
            if ev.window != primary || ev.state != ButtonState::Pressed {
                continue;
            }

            // Copy / cut / paste / select-all shortcuts.
            if ctrl && !ev.repeat {
                match shortcut_char(&ev.logical_key) {
                    Some('a') => urlbar.select_all = true,
                    Some('c') if urlbar.select_all => {
                        if let Err(e) = clipboard.set_text(urlbar.visible()) {
                            eprintln!("[bevy] clipboard write failed: {e:?}");
                        }
                    }
                    Some('x') if urlbar.select_all => {
                        if let Err(e) = clipboard.set_text(urlbar.visible()) {
                            eprintln!("[bevy] clipboard write failed: {e:?}");
                        }
                        urlbar.clear_selection();
                    }
                    Some('v') => {
                        let mut read = clipboard.fetch_text();
                        if let Some(result) = read.poll_result() {
                            match result {
                                Ok(text) => paste_into_bar(&mut urlbar, &text),
                                Err(e) => eprintln!("[bevy] clipboard read failed: {e:?}"),
                            }
                        } else {
                            *pending_paste = Some(read);
                        }
                    }
                    _ => {}
                }
                continue;
            }

            if composing {
                // While composing, only Enter/Escape terminate the composition;
                // letter/backspace edits arrive as Preedit updates instead.
                match &ev.logical_key {
                    BevyKey::Enter | BevyKey::Escape => {}
                    _ => continue,
                }
            }

            match &ev.logical_key {
                BevyKey::Character(c) => {
                    if urlbar.select_all {
                        urlbar.replace_all(c.to_string());
                    } else {
                        urlbar.text.push_str(c);
                    }
                }
                BevyKey::Backspace if !ev.repeat => {
                    if urlbar.select_all {
                        urlbar.clear_selection();
                    } else {
                        urlbar.text.pop();
                    }
                }
                BevyKey::Enter if !ev.repeat => {
                    let raw = urlbar.visible();
                    urlbar.editing = false;
                    urlbar.select_all = false;
                    urlbar.text.clear();
                    urlbar.composing.clear();
                    chrome.page = true;
                    if !raw.trim().is_empty() {
                        pending.0 = Some(NavCommand::Load(raw.trim().to_string()));
                    }
                }
                BevyKey::Escape if !ev.repeat => {
                    urlbar.composing.clear();
                    urlbar.editing = false;
                    urlbar.select_all = false;
                    urlbar.text.clear();
                    chrome.page = true;
                }
                _ => {}
            }
        }
    }
}

/// Executes navigation queued by the chrome (URL bar submit, Go button, or a
/// nav-button click), after input forwarding has run so the release keystroke
/// (Enter) is not also delivered to the page.
fn finish_nav(
    servo: NonSendMut<ServoState>,
    home: Res<HomeUrl>,
    mut pending: ResMut<PendingNav>,
) {
    let Some(command) = pending.0.take() else {
        return;
    };
    let webview = &servo.webview;
    match command {
        NavCommand::Load(raw) => {
            let url = Url::parse(&raw).or_else(|_| Url::parse(&format!("https://{raw}")));
            match url {
                Ok(url) => {
                    println!("[bevy] navigate → {url}");
                    webview.load(url);
                }
                Err(_) => eprintln!("[bevy] invalid URL: {raw}"),
            }
        }
        NavCommand::Back => {
            let _ = webview.go_back(1);
        }
        NavCommand::Forward => {
            let _ = webview.go_forward(1);
        }
        NavCommand::Reload => webview.reload(),
        NavCommand::Home => {
            if let Ok(url) = Url::parse(&home.0) {
                println!("[bevy] home → {url}");
                webview.load(url);
            }
        }
    }
}

fn drive_servo(
    mut servo: NonSendMut<ServoState>,
    mut frame: ResMut<ServoFrame>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    servo.servo.spin_event_loop();

    if let Ok(window) = windows.single() {
        // Servo's viewport is the window minus the top URL bar.
        let phys = window.resolution.physical_size();
        let bar_phys = (URL_BAR_HEIGHT * window.scale_factor()).round() as u32;
        let new_size = PhysicalSize::new(phys.x.max(1), phys.y.saturating_sub(bar_phys).max(1));
        if new_size != servo.size {
            servo.webview.resize(new_size);
            servo.size = new_size;
        }
    }

    servo.webview.paint();

    #[cfg(target_os = "windows")]
    match servo
        .interop
        .rendering_context_handle()
        .current_dx12_shared_texture()
    {
        Ok(shared) => {
            let metadata = shared.metadata();
            frame.0 = Some(FrameDesc {
                resource: shared.resource(),
                width: metadata.size.width,
                height: metadata.size.height,
                generation: metadata.generation,
            });
        }
        Err(e) => eprintln!("[bevy] shared-texture export failed: {e:?}"),
    }
    // Linux: no cross-device shared handle — CPU readback (`read_full_frame`
    // → bytes) carried into the render world and uploaded with
    // `queue.write_texture`. Top-left already, no flip needed.
    #[cfg(not(target_os = "windows"))]
    {
        if let Some(image) = servo.interop.rendering_context_handle().read_full_frame() {
            let (w, h) = image.dimensions();
            servo.generation = servo.generation.wrapping_add(1);
            frame.0 = Some(FrameDesc {
                pixels: image.into_raw(),
                width: w,
                height: h,
                generation: servo.generation,
            });
        }
    }

    // Same-tab policy for `target="_blank"` / `window.open` links: the
    // delegate queued the requested view during this frame's spin/paint, so
    // activate it now. Dropping the old handle destroys that view.
    // Draining after paint/export avoids borrowing the queue reentrantly.
    if let Some(new_view) = servo.delegate.take_pending() {
        new_view.resize(servo.size);
        servo.webview = new_view;
    }
}

/// Mirror input-method state onto the Bevy window so the OS IME is enabled and
/// anchored at the right place: at the focused Servo editable element when the
/// page owns focus, or under the URL bar's text while the bar is being edited
/// (which routes `Ime` events to `url_bar_input`, not to Servo).
fn apply_ime_window(
    servo: NonSend<ServoState>,
    chrome: Res<ChromeFocus>,
    urlbar: Res<UrlBarState>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    let (enabled, pos) = if !chrome.page && urlbar.editing {
        // Editing the URL bar: enable the OS IME and put the candidate window
        // just below the end of the bar's text (offset by the field's position
        // in the toolbar).
        let geom = toolbar_geometry(window.width());
        let x = (geom.field_x + visible_text_width(&urlbar))
            .min(geom.field_right - 8.0)
            .max(4.0);
        (true, Vec2::new(x, URL_BAR_HEIGHT))
    } else {
        let ime = servo.delegate.ime.borrow();
        match (ime.control_id, ime.rect_min) {
            (Some(_), Some((x, y))) => {
                let scale = window.scale_factor();
                // Coordinate frames differ: the rect is relative to the Servo
                // surface (top-left below the bar), `ime_position` is relative
                // to the window's top-left corner.
                (true, Vec2::new(x / scale, URL_BAR_HEIGHT + y / scale))
            }
            _ => (false, Vec2::ZERO),
        }
    };
    if window.ime_enabled != enabled {
        window.ime_enabled = enabled;
    }
    if window.ime_position != pos {
        window.ime_position = pos;
    }
}

/// Keep the bar's text node in sync: the delegate's current URL while the page
/// owns focus, or the URL bar's live content (committed text plus any IME
/// preedit) while editing.
fn sync_url_and_label(
    servo: NonSend<ServoState>,
    chrome: Res<ChromeFocus>,
    urlbar: Res<UrlBarState>,
    mut current: ResMut<CurrentUrl>,
    mut label: Query<&mut UiText, With<UrlLabel>>,
) {
    let delegate_url = servo.delegate.url.borrow();
    if let Some(url) = delegate_url.as_ref() {
        if current.0 != *url {
            current.0 = url.clone();
        }
    }
    let text = if chrome.page || !urlbar.editing {
        current.0.clone()
    } else {
        urlbar.visible()
    };
    if let Ok(mut label) = label.single_mut() {
        if label.0 != text {
            label.0 = text;
        }
    }
}

/// Keep the Bevy-owned placeholder image sized to the Servo frame. Resizing the
/// asset fires an `AssetEvent::Modified`, which makes Bevy re-create the GpuImage
/// texture at the new size and refresh the sprite's image bind group (otherwise
/// the cached bind group would keep pointing at the old-size texture).
fn resize_servo_image(
    frame: Res<ServoFrame>,
    servo_image: Res<ServoImage>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(desc) = frame.0.as_ref() else {
        return;
    };
    if let Some(mut image) = images.get_mut(&servo_image.0) {
        let size = image.texture_descriptor.size;
        if size.width != desc.width || size.height != desc.height {
            image.texture_descriptor.size = Extent3d {
                width: desc.width,
                height: desc.height,
                depth_or_array_layers: 1,
            };
        }
    }
}

fn fit_sprite_to_window(
    windows: Query<&Window, With<PrimaryWindow>>,
    mut sprites: Query<(&mut Sprite, &mut Transform)>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let size = Vec2::new(window.width(), (window.height() - URL_BAR_HEIGHT).max(1.0));
    let y = -URL_BAR_HEIGHT / 2.0;
    for (mut sprite, mut transform) in &mut sprites {
        if sprite.custom_size != Some(size) {
            sprite.custom_size = Some(size);
        }
        if transform.translation.y != y {
            transform.translation.y = y;
        }
    }
}

/// Keeps the URL field's width and the Go button's right-edge position in sync
/// with the current window width (both are width-dependent chrome geometry).
fn layout_url_chrome(
    windows: Query<&Window, With<PrimaryWindow>>,
    mut fields: Query<&mut Node, (With<UrlField>, Without<GoButton>)>,
    mut go_buttons: Query<&mut Node, (With<GoButton>, Without<UrlField>)>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let geom = toolbar_geometry(window.width());
    if let Ok(mut field) = fields.single_mut() {
        let width = (geom.field_right - geom.field_x).max(1.0);
        if field.width != Val::Px(width) {
            field.width = Val::Px(width);
        }
    }
    if let Ok(mut go) = go_buttons.single_mut() {
        if go.left != Val::Px(geom.go_x) {
            go.left = Val::Px(geom.go_x);
        }
    }
}

// ── Render world: extract + inject ───────────────────────────────────────────

fn extract_servo_frame(
    frame: Extract<Res<ServoFrame>>,
    image: Extract<Option<Res<ServoImage>>>,
    mut out_frame: ResMut<ExtractedFrame>,
    mut out_id: ResMut<ExtractedImageId>,
) {
    out_frame.0 = frame.0.clone();
    out_id.0 = image.as_ref().map(|i| i.0.id());
}

fn inject_servo_image(
    frame: Res<ExtractedFrame>,
    image_id: Res<ExtractedImageId>,
    device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    gpu_images: Res<RenderAssets<GpuImage>>,
) {
    let (Some(desc), Some(id)) = (frame.0.clone(), image_id.0) else {
        return;
    };
    let metadata = grafting::FrameMetadata {
        size: PhysicalSize::new(desc.width, desc.height),
        format: TextureFormat::Rgba8Unorm,
        generation: desc.generation,
        producer_sync: SyncMechanism::None,
    };
    // Bevy's own GpuImage for the sprite. It lags a frame during resize, so only
    // copy when its size matches the freshly imported frame.
    let Some(gpu_image) = gpu_images.get(id) else {
        return;
    };
    if gpu_image.texture_descriptor.size.width != metadata.size.width
        || gpu_image.texture_descriptor.size.height != metadata.size.height
    {
        return;
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = &device;
        render_queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &gpu_image.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &desc.pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * desc.width),
                rows_per_image: Some(desc.height),
            },
            Extent3d {
                width: desc.width,
                height: desc.height,
                depth_or_array_layers: 1,
            },
        );
    }

    #[cfg(target_os = "windows")]
    let imported = {
        let host = HostWgpuContext::new(device.wgpu_device().clone(), (**render_queue.0).clone());
        match import_dx12_shared_texture(Dx12SharedTexture::new(metadata, desc.resource, 0), &host)
        {
            Ok(t) => t,
            Err(e) => {
                eprintln!("[bevy] import_dx12_shared_texture failed: {e:?}");
                return;
            }
        }
    };

    // Copy the imported (short-lived) alias into Bevy's stable owned texture, so
    // the sprite's cached bind group keeps sampling a texture that stays valid
    // across frames and resizes.
    #[cfg(target_os = "windows")]
    {
        let mut encoder =
            device
                .wgpu_device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("servo-bevy-copy"),
                });
        encoder.copy_texture_to_texture(
            imported.as_image_copy(),
            gpu_image.texture.as_image_copy(),
            Extent3d {
                width: metadata.size.width,
                height: metadata.size.height,
                depth_or_array_layers: 1,
            },
        );
        render_queue.submit([encoder.finish()]);
    }
}

// ── Servo support ────────────────────────────────────────────────────────────

#[derive(Clone)]
struct NoopWaker;

impl EventLoopWaker for NoopWaker {
    fn clone_box(&self) -> Box<dyn EventLoopWaker> {
        Box::new(self.clone())
    }
    fn wake(&self) {}
}

/// The embedder-side view of an active input method: set when Servo focuses an
/// editable element (show_embedder_control) and cleared on blur. `rect_min` is
/// the focused field's top-left corner in device px on the Servo surface (the
/// area below the URL bar).
struct ImeUi {
    control_id: Option<EmbedderControlId>,
    rect_min: Option<(f32, f32)>,
}

struct DemoDelegate {
    rendering_context: Rc<dyn servo::RenderingContext>,
    pending: RefCell<Vec<WebView>>,
    ime: RefCell<ImeUi>,
    url: RefCell<Option<String>>,
}

impl DemoDelegate {
    fn new(rendering_context: Rc<dyn servo::RenderingContext>, initial_url: String) -> Self {
        Self {
            rendering_context,
            pending: RefCell::new(Vec::new()),
            ime: RefCell::new(ImeUi {
                control_id: None,
                rect_min: None,
            }),
            url: RefCell::new(Some(initial_url)),
        }
    }

    fn take_pending(&self) -> Option<WebView> {
        self.pending.borrow_mut().pop()
    }
}

impl WebViewDelegate for DemoDelegate {
    fn notify_url_changed(&self, _webview: WebView, url: Url) {
        *self.url.borrow_mut() = Some(url.to_string());
        println!("[servo] URL changed: {url}");
    }
    fn notify_crashed(&self, _webview: WebView, reason: String, backtrace: Option<String>) {
        eprintln!("[servo] CRASH: {reason}");
        if let Some(bt) = backtrace {
            eprintln!("{bt}");
        }
    }
    fn request_create_new(&self, parent_webview: WebView, request: CreateNewWebViewRequest) {
        let view = request
            .builder(self.rendering_context.clone())
            .hidpi_scale_factor(euclid::Scale::new(1.0))
            .delegate(parent_webview.delegate())
            .build();
        eprintln!("[servo] new-view link: opening in same tab");
        self.pending.borrow_mut().push(view);
    }
    fn show_embedder_control(&self, _webview: WebView, control: EmbedderControl) {
        // An editable element gained focus; Servo gives us the element rect so
        // the OS IME can anchor its candidate window there. Non-IME controls
        // (select/color/file pickers) are out of scope for this demo.
        if let EmbedderControl::InputMethod(input) = control {
            let min = input.position().min;
            *self.ime.borrow_mut() = ImeUi {
                control_id: Some(input.id()),
                rect_min: Some((min.x as f32, min.y as f32)),
            };
        }
    }
    fn hide_embedder_control(&self, _webview: WebView, control_id: EmbedderControlId) {
        let mut ime = self.ime.borrow_mut();
        if ime.control_id == Some(control_id) {
            ime.control_id = None;
            ime.rect_min = None;
        }
    }
}
