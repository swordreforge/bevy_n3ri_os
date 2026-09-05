// Copyright 2026 Mark Alan Boykin (adapted from wgpu-graft demo-servo-bevy)
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0
#![allow(clippy::type_complexity)]
//
// n3ri_os 真实浏览器：Servo 引擎嵌入桌面窗口（Linux CPU readback 路径）。
// Servo 经 surfman/GL 离屏渲染 → read_full_frame() 读回 RGBA →
// render world 用 queue.write_texture 上传到 ImageNode 的稳定纹理。

#[path = "browser_keyutils.rs"]
mod browser_keyutils;

use bevy::asset::RenderAssetUsages;
use bevy::ecs::message::MessageReader;
use bevy::ecs::relationship::Relationship;
use bevy::image::Image;
use bevy::input::keyboard::{Key as BevyKey, KeyboardInput};
use bevy::input::mouse::{MouseButtonInput, MouseScrollUnit, MouseWheel};
use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::render::renderer::RenderQueue;
use bevy::render::texture::GpuImage;
use bevy::render::{Extract, ExtractSchedule, Render, RenderApp, RenderSystems};
use bevy::text::{FontSize, TextColor, TextFont};
use bevy::window::Ime;
use euclid::Scale;
use rustls::crypto::aws_lc_rs;
use servo::{
    CompositionEvent, CompositionState, CreateNewWebViewRequest, DevicePoint, EmbedderControl,
    EmbedderControlId, EventLoopWaker, ImeEvent, InputEvent, MouseButton as ServoMouseButton,
    MouseButtonAction, MouseButtonEvent, MouseLeftViewportEvent, MouseMoveEvent, Servo,
    ServoBuilder, WebView, WebViewBuilder, WebViewDelegate, WheelDelta, WheelEvent, WheelMode,
};
use servo_wgpu_interop_adapter::ServoWgpuInteropAdapter;
use std::cell::RefCell;
use std::rc::Rc;
use url::Url;
use winit::dpi::PhysicalSize;

use crate::apps::terminal::{copy_text, paste_text};
use crate::cursor::CursorPosition;
use crate::dock::{AppVisible, Dock};
use crate::font::N3riFonts;
use crate::input_focus::{TextInputFocus, TextInputOwner};
use crate::scroll::UiWheelConsumed;
use crate::topbar::FocusedTitle;
use crate::window::{spawn_window, AppWindow};

const BROWSER_TITLE: &str = "浏览器";
const WIN_W: f32 = 1100.0;
const WIN_H: f32 = 700.0;

const DEFAULT_HOME: &str = "https://www.bing.com";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";
const SEED_SIZE: (u32, u32) = (1024, 600);
/// 默认页面缩放（浏览器式 zoom，Servo 范围 0.1~10.0；等价 Ctrl+'+' × 2.5）
const PAGE_ZOOM: f32 = 2.0;

const TOOLBAR_H: f32 = 36.0;
const BAR_TEXT_SIZE: f32 = 13.0;

const TEXT_MAIN: Color = Color::srgb(0.86, 0.93, 0.93);
const TOOLBAR_BG: Color = Color::srgb(0.05, 0.08, 0.14);
const FIELD_BG: Color = Color::srgb(0.03, 0.05, 0.09);
const PAGE_BG: Color = Color::srgb(0.13, 0.15, 0.19);
const NAV_BG: Color = Color::srgb(0.16, 0.20, 0.27);
const NAV_BG_HOT: Color = Color::srgb(0.24, 0.30, 0.40);
const NAV_BG_DOWN: Color = Color::srgb(0.12, 0.15, 0.21);

#[derive(Component)]
pub struct BrowserPage;

#[derive(Component)]
struct BrowserNav(u8);

#[derive(Component)]
struct UrlField;

#[derive(Component)]
struct GoButton;

#[derive(Component)]
struct UrlLabel;

#[derive(Resource, Default)]
pub struct CurrentUrl(String);

#[derive(Resource)]
struct HomeUrl(String);

#[derive(Resource, Default)]
struct PendingNav(Option<NavCommand>);

enum NavCommand {
    Load(String),
    Back,
    Forward,
    Reload,
    Home,
}

#[derive(Resource)]
struct BrowserFocus {
    page: bool,
}

impl Default for BrowserFocus {
    fn default() -> Self {
        Self { page: true }
    }
}

#[derive(Resource, Default)]
struct UrlBarState {
    editing: bool,
    select_all: bool,
    text: String,
    composing: String,
}

impl UrlBarState {
    fn visible(&self) -> String {
        format!("{}{}", self.text, self.composing)
    }
    fn replace_all(&mut self, content: String) {
        self.text = content;
        self.composing.clear();
        self.select_all = false;
    }
    fn clear_selection(&mut self) {
        self.text.clear();
        self.composing.clear();
        self.select_all = false;
    }
}

#[derive(Resource, Default)]
pub struct BrowserImeAnchor {
    pub enabled: bool,
    pub pos: Vec2,
}

#[derive(Resource, Default)]
pub struct BrowserLaunch(pub bool);

#[derive(Resource, Clone)]
struct BrowserImageHandle(Handle<Image>);

/// 常驻引擎核心：Servo 实例 + interop/GL 上下文（关窗不销毁）。
/// `webview/delegate` 是会话（一次打开的页面会话），关窗即 drop 释放页面 DOM/JS。
struct BrowserEngine {
    servo: Servo,
    interop: ServoWgpuInteropAdapter,
    size: PhysicalSize<u32>,
    webview: Option<WebView>,
    delegate: Option<Rc<BrowserDelegate>>,
}

impl BrowserEngine {
    fn has_session(&self) -> bool {
        self.webview.is_some()
    }
    fn detach_session(&mut self) {
        self.webview = None;
        self.delegate = None;
    }
    /// 用同一 Servo/上下文重建一个全新页面会话（关窗后下次打开的轻量恢复）。
    fn build_session(&mut self, home_url: &str) {
        let rendering_context = self.interop.rendering_context();
        let delegate = Rc::new(BrowserDelegate::new(
            rendering_context.clone(),
            home_url.to_string(),
        ));
        let webview = WebViewBuilder::new(&self.servo, rendering_context)
            .url(Url::parse(home_url).expect("invalid home url"))
            .hidpi_scale_factor(Scale::new(1.0))
            .delegate(delegate.clone())
            .build();
        webview.set_page_zoom(PAGE_ZOOM);
        self.webview = Some(webview);
        self.delegate = Some(delegate);
    }
}

#[derive(Default)]
struct BrowserHost(Option<BrowserEngine>);

#[derive(Resource, Default, Clone)]
pub struct BrowserFrame(Option<FrameDesc>);

#[derive(Clone)]
struct FrameDesc {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
}

#[derive(Resource, Default, Clone)]
struct ExtractedFrame(Option<FrameDesc>);

#[derive(Resource, Default, Clone, Copy)]
struct ExtractedImageId(Option<AssetId<Image>>);

struct ImeUi {
    control_id: Option<EmbedderControlId>,
    rect_min: Option<(f32, f32)>,
}

struct BrowserDelegate {
    rendering_context: Rc<dyn servo::RenderingContext>,
    pending: RefCell<Vec<WebView>>,
    ime: RefCell<ImeUi>,
    url: RefCell<Option<String>>,
}

impl BrowserDelegate {
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

impl WebViewDelegate for BrowserDelegate {
    fn notify_url_changed(&self, _webview: WebView, url: Url) {
        *self.url.borrow_mut() = Some(url.to_string());
    }
    fn notify_crashed(&self, _webview: WebView, reason: String, backtrace: Option<String>) {
        error!("[browser] Servo CRASH: {reason}");
        if let Some(bt) = backtrace {
            error!("{bt}");
        }
    }
    fn request_create_new(&self, parent_webview: WebView, request: CreateNewWebViewRequest) {
        let view = request
            .builder(self.rendering_context.clone())
            .hidpi_scale_factor(Scale::new(1.0))
            .delegate(parent_webview.delegate())
            .build();
        self.pending.borrow_mut().push(view);
    }
    fn show_embedder_control(&self, _webview: WebView, control: EmbedderControl) {
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

struct NoopWaker;

impl EventLoopWaker for NoopWaker {
    fn clone_box(&self) -> Box<dyn EventLoopWaker> {
        Box::new(NoopWaker)
    }
    fn wake(&self) {}
}

fn default_home() -> String {
    for dir in ["assets", "../assets", "../../assets"] {
        let file = std::path::Path::new(dir).join("nori/browser/home.html");
        if file.is_file() {
            if let Ok(url) = Url::from_file_path(file) {
                return url.to_string();
            }
        }
    }
    DEFAULT_HOME.to_string()
}

fn build_engine(home_url: &str) -> BrowserEngine {
    let size = PhysicalSize::new(SEED_SIZE.0, SEED_SIZE.1);

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        flags: wgpu::InstanceFlags::default(),
        memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
        backend_options: wgpu::BackendOptions::default(),
        display: None,
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
    }))
    .expect("no Vulkan/GL adapter for Servo interop (check GPU drivers)");
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("n3ri-browser-interop"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    }))
    .expect("failed to create Servo interop device");

    let interop = ServoWgpuInteropAdapter::new(device, queue, size)
        .expect("failed to create Servo interop adapter");

    let servo = ServoBuilder::default()
        .preferences(servo::Preferences {
            user_agent: USER_AGENT.to_string(),
            ..Default::default()
        })
        .event_loop_waker(Box::new(NoopWaker))
        .build();
    // 不调 servo.setup_logging()：它会 log::set_boxed_logger，而 Bevy LogPlugin
    // 已注册全局 logger（demo 在 App 创建前调用才不冲突）。Servo 组件的 log 记录
    // 经 log crate 自动落入 Bevy logger。

    let mut engine = BrowserEngine {
        servo,
        interop,
        size,
        webview: None,
        delegate: None,
    };
    engine.build_session(home_url);
    engine
}

pub struct BrowserPlugin;

impl Plugin for BrowserPlugin {
    fn build(&self, app: &mut App) {
        let _ = aws_lc_rs::default_provider().install_default();

        app.world_mut().insert_non_send(BrowserHost::default());
        app.init_resource::<BrowserFrame>()
            .init_resource::<CurrentUrl>()
            .init_resource::<BrowserFocus>()
            .init_resource::<UrlBarState>()
            .init_resource::<PendingNav>()
            .init_resource::<BrowserImeAnchor>()
            .init_resource::<BrowserLaunch>()
            .insert_resource(HomeUrl(default_home()))
            .add_systems(Startup, create_placeholder_image)
            .add_systems(
                Update,
                (
                    browser_launch_window.after(crate::window::WindowFocusSet),
                    browser_chrome
                        .after(browser_launch_window)
                        .after(crate::window::WindowFocusSet),
                    browser_bar_input
                        .after(browser_chrome)
                        .after(crate::window::WindowFocusSet),
                    browser_page_input
                        .after(browser_bar_input)
                        .after(crate::window::WindowFocusSet)
                        .after(crate::scroll::wheel_dispatch),
                    browser_drive
                        .after(browser_page_input)
                        .after(crate::window::WindowFocusSet),
                    browser_resize_texture.after(browser_drive),
                    browser_ui_sync.after(browser_resize_texture),
                    browser_session_track.after(browser_ui_sync),
                ),
            );

        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app
                .init_resource::<ExtractedFrame>()
                .init_resource::<ExtractedImageId>()
                .add_systems(ExtractSchedule, extract_browser_frame)
                .add_systems(
                    Render,
                    inject_browser_frame
                        .after(RenderSystems::PrepareAssets)
                        .before(RenderSystems::Queue),
                );
        } else {
            warn!("[browser] RenderApp missing — Servo 帧注入不可用");
        }
    }
}

fn create_placeholder_image(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let mut placeholder = Image::new_uninit(
        Extent3d {
            width: SEED_SIZE.0,
            height: SEED_SIZE.1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    placeholder.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST;
    commands.insert_resource(BrowserImageHandle(images.add(placeholder)));
}

fn browser_launch_window(
    mut launch: ResMut<BrowserLaunch>,
    pages: Query<Entity, With<BrowserPage>>,
    dock: Query<&ChildOf, With<Dock>>,
    image: Res<BrowserImageHandle>,
    fonts: Res<N3riFonts>,
    mut commands: Commands,
) {
    if !launch.0 {
        return;
    }
    launch.0 = false;
    if !pages.is_empty() {
        return;
    }
    let Ok(dock_parent) = dock.single() else {
        return;
    };
    let root = dock_parent.get();
    commands.entity(root).with_children(|parent| {
        spawn_browser_window(parent, &image.0, &fonts);
    });
}

fn spawn_browser_window(
    parent: &mut ChildSpawnerCommands,
    image: &Handle<Image>,
    fonts: &N3riFonts,
) {
    let window_e = spawn_window(parent, BROWSER_TITLE, "browser", WIN_W, WIN_H, fonts);
    let font = fonts.default.clone();

    parent.commands().entity(window_e).with_children(|win| {
        win.spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(TOOLBAR_H),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(Val::Px(8.0)),
                column_gap: Val::Px(4.0),
                ..default()
            },
            BackgroundColor(TOOLBAR_BG),
        ))
        .with_children(|toolbar| {
            for (i, glyph) in ["←", "→", "↻", "⌂"].iter().enumerate() {
                toolbar
                    .spawn((
                        BrowserNav(i as u8),
                        Button,
                        Node {
                            width: Val::Px(30.0),
                            height: Val::Px(24.0),
                            flex_shrink: 0.0,
                            border_radius: BorderRadius::all(Val::Px(5.0)),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            ..default()
                        },
                        BackgroundColor(NAV_BG),
                    ))
                    .with_children(|btn| {
                        btn.spawn((
                            Text::new(*glyph),
                            TextFont {
                                font: FontSource::Handle(font.clone()),
                                font_size: FontSize::Px(BAR_TEXT_SIZE),
                                ..default()
                            },
                            TextColor(TEXT_MAIN),
                        ));
                    });
            }
            toolbar
                .spawn((
                    UrlField,
                    Button,
                    Node {
                        flex_grow: 1.0,
                        height: Val::Px(24.0),
                        border_radius: BorderRadius::all(Val::Px(5.0)),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(Val::Px(10.0)),
                        ..default()
                    },
                    BackgroundColor(FIELD_BG),
                ))
                .with_children(|field| {
                    field.spawn((
                        UrlLabel,
                        Text::new(""),
                        TextFont {
                            font: FontSource::Handle(font.clone()),
                            font_size: FontSize::Px(BAR_TEXT_SIZE),
                            ..default()
                        },
                        TextColor(TEXT_MAIN),
                    ));
                });
            toolbar
                .spawn((
                    GoButton,
                    Button,
                    Node {
                        width: Val::Px(48.0),
                        height: Val::Px(24.0),
                        flex_shrink: 0.0,
                        border_radius: BorderRadius::all(Val::Px(5.0)),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    BackgroundColor(NAV_BG),
                ))
                .with_children(|btn| {
                    btn.spawn((
                        Text::new("转到"),
                        TextFont {
                            font: FontSource::Handle(font.clone()),
                            font_size: FontSize::Px(BAR_TEXT_SIZE),
                            ..default()
                        },
                        TextColor(TEXT_MAIN),
                    ));
                });
        });

        win.spawn((
            BrowserPage,
            ImageNode {
                image: image.clone(),
                ..default()
            },
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                ..default()
            },
            BackgroundColor(PAGE_BG),
        ));
    });
}

/// dock 点击调用的启动入口：发请求，由 BrowserPlugin 消费并真正建窗。
pub fn request_browser(launch: &mut BrowserLaunch) {
    launch.0 = true;
}

fn node_hit(node: &ComputedNode, tf: &UiGlobalTransform, cursor: Vec2) -> bool {
    let Some(inv) = tf.try_inverse() else {
        return false;
    };
    let local = inv.transform_point2(cursor);
    let half = node.size() * 0.5;
    local.x.abs() <= half.x && local.y.abs() <= half.y
}

/// 页面节点局部（居中原点）→ 页面左上原点像素。
/// 坐标系即 `cursor.physical` 所在空间（UI 渲染空间物理像素，与 ComputedNode/
/// UiGlobalTransform 一致），Servo hidpi=1 下 device px 与此 1:1，勿再乘 scale。
fn page_device_point(
    node: &ComputedNode,
    tf: &UiGlobalTransform,
    cursor: &CursorPosition,
) -> Option<Vec2> {
    if !cursor.active {
        return None;
    }
    let inv = tf.try_inverse()?;
    let local = inv.transform_point2(cursor.physical);
    let half = node.size() * 0.5;
    if local.x.abs() > half.x || local.y.abs() > half.y {
        return None;
    }
    Some(local + half)
}

fn page_device_size(node: &ComputedNode) -> (u32, u32) {
    (
        node.size().x.round().max(1.0) as u32,
        node.size().y.round().max(1.0) as u32,
    )
}

fn leave_editing(focus: &mut BrowserFocus, urlbar: &mut UrlBarState) {
    focus.page = true;
    urlbar.editing = false;
    urlbar.select_all = false;
    urlbar.text.clear();
    urlbar.composing.clear();
}

#[allow(clippy::too_many_arguments)]
fn browser_chrome(
    focused: Res<FocusedTitle>,
    mut owner: ResMut<TextInputOwner>,
    mut focus: ResMut<BrowserFocus>,
    mut urlbar: ResMut<UrlBarState>,
    mut pending: ResMut<PendingNav>,
    current: Res<CurrentUrl>,
    cursor: Res<CursorPosition>,
    pages: Query<Entity, With<BrowserPage>>,
    page_area: Query<(&ComputedNode, &UiGlobalTransform), (With<BrowserPage>, Without<UrlField>)>,
    mut buttons: MessageReader<MouseButtonInput>,
    url_field: Query<(&ComputedNode, &UiGlobalTransform), (With<UrlField>, Without<BrowserPage>)>,
    go_button: Query<(&ComputedNode, &UiGlobalTransform), (With<GoButton>, Without<BrowserPage>)>,
    nav_nodes: Query<
        (&BrowserNav, &ComputedNode, &UiGlobalTransform, &Interaction),
        (Without<UrlField>, Without<GoButton>, Without<BrowserPage>),
    >,
    mut nav_bg: Query<(&mut BackgroundColor, &BrowserNav)>,
) {
    let active = focused.title == BROWSER_TITLE && !pages.is_empty();
    if !active {
        buttons.clear();
        return;
    }

    for ev in buttons.read() {
        if ev.state != ButtonState::Pressed || ev.button != MouseButton::Left {
            continue;
        }
        owner.0 = TextInputFocus::Browser;

        let hit_field = url_field
            .iter()
            .any(|(n, t)| node_hit(n, t, cursor.physical));
        let hit_go = go_button
            .iter()
            .any(|(n, t)| node_hit(n, t, cursor.physical));
        let mut nav_click: Option<u8> = None;
        for (nav, n, t, _) in nav_nodes.iter() {
            if node_hit(n, t, cursor.physical) {
                nav_click = Some(nav.0);
                break;
            }
        }

        if hit_field {
            if !focus.page {
                urlbar.select_all = true;
            } else {
                focus.page = false;
                urlbar.editing = true;
                urlbar.select_all = true;
                urlbar.text = current.0.clone();
                urlbar.composing.clear();
            }
            continue;
        }

        if let Some(kind) = nav_click {
            leave_editing(&mut focus, &mut urlbar);
            pending.0 = Some(match kind {
                0 => NavCommand::Back,
                1 => NavCommand::Forward,
                2 => NavCommand::Reload,
                _ => NavCommand::Home,
            });
            continue;
        }

        if hit_go {
            let raw = if focus.page {
                current.0.clone()
            } else {
                let raw = urlbar.visible();
                leave_editing(&mut focus, &mut urlbar);
                raw
            };
            let raw = raw.trim().to_string();
            if !raw.is_empty() {
                pending.0 = Some(NavCommand::Load(raw));
            }
            continue;
        }

        if !focus.page {
            let hit_page = page_area
                .iter()
                .any(|(n, t)| node_hit(n, t, cursor.physical));
            if hit_page {
                leave_editing(&mut focus, &mut urlbar);
            }
        }
    }

    for (mut bg, nav) in nav_bg.iter_mut() {
        let state = nav_nodes
            .iter()
            .find(|(n, _, _, _)| n.0 == nav.0)
            .map(|(_, _, _, i)| *i)
            .unwrap_or(Interaction::None);
        let target = match state {
            Interaction::Pressed => NAV_BG_DOWN,
            Interaction::Hovered => NAV_BG_HOT,
            Interaction::None => NAV_BG,
        };
        if bg.0 != target {
            bg.0 = target;
        }
    }
}

fn shortcut_char(key: &BevyKey) -> Option<char> {
    let BevyKey::Character(text) = key else {
        return None;
    };
    let mut chars = text.chars();
    let c = chars.next()?;
    chars.next().is_none().then(|| c.to_ascii_lowercase())
}

fn paste_into_bar(urlbar: &mut UrlBarState, text: &str) {
    if urlbar.select_all {
        urlbar.replace_all(text.to_string());
    } else {
        urlbar.text.push_str(text);
    }
}

#[allow(clippy::too_many_arguments)]
fn browser_bar_input(
    focused: Res<FocusedTitle>,
    owner: Res<TextInputOwner>,
    mut focus: ResMut<BrowserFocus>,
    mut urlbar: ResMut<UrlBarState>,
    mut pending: ResMut<PendingNav>,
    keys: Res<ButtonInput<KeyCode>>,
    pages: Query<(), With<BrowserPage>>,
    mut keyboard: MessageReader<KeyboardInput>,
    mut ime: MessageReader<Ime>,
    mut prev_editing: Local<bool>,
) {
    let editing = focused.title == BROWSER_TITLE
        && owner.is(TextInputFocus::Browser)
        && !focus.page
        && urlbar.editing
        && !pages.is_empty();
    if !editing {
        keyboard.clear();
        ime.clear();
        *prev_editing = false;
        return;
    }

    if !*prev_editing {
        for _ in keyboard.read() {}
        for _ in ime.read() {}
    }
    *prev_editing = true;

    let ctrl = keys.any_pressed([
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ]);

    for ev in ime.read() {
        match ev {
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
            Ime::Enabled { .. } => {}
            Ime::Disabled { .. } => urlbar.composing.clear(),
        }
    }

    let composing = !urlbar.composing.is_empty();
    for ev in keyboard.read() {
        if ev.state != ButtonState::Pressed {
            continue;
        }

        if ctrl && !ev.repeat {
            match shortcut_char(&ev.logical_key) {
                Some('a') => urlbar.select_all = true,
                Some('c') if urlbar.select_all => {
                    copy_text(&urlbar.visible());
                }
                Some('x') if urlbar.select_all => {
                    copy_text(&urlbar.visible());
                    urlbar.clear_selection();
                }
                Some('v') => {
                    if let Some(text) = paste_text() {
                        paste_into_bar(&mut urlbar, &text);
                    }
                }
                _ => {}
            }
            continue;
        }

        if composing {
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
                leave_editing(&mut focus, &mut urlbar);
                let raw = raw.trim();
                if !raw.is_empty() {
                    pending.0 = Some(NavCommand::Load(raw.to_string()));
                }
            }
            BevyKey::Escape if !ev.repeat => {
                urlbar.composing.clear();
                urlbar.editing = false;
                urlbar.select_all = false;
                urlbar.text.clear();
                focus.page = true;
            }
            _ => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn browser_page_input(
    focused: Res<FocusedTitle>,
    owner: Res<TextInputOwner>,
    focus: Res<BrowserFocus>,
    mut host: NonSendMut<BrowserHost>,
    keys: Res<ButtonInput<KeyCode>>,
    cursor: Res<CursorPosition>,
    page: Query<(&ComputedNode, &UiGlobalTransform), With<BrowserPage>>,
    primary_window: Query<Entity, With<bevy::window::PrimaryWindow>>,
    mut keyboard: MessageReader<KeyboardInput>,
    mut ime: MessageReader<Ime>,
    mut buttons: MessageReader<MouseButtonInput>,
    mut wheels: MessageReader<MouseWheel>,
    ui_consumed: Res<UiWheelConsumed>,
    mut last_content: Local<Option<Vec2>>,
    mut prev_page: Local<bool>,
) {
    let engine_active =
        focused.title == BROWSER_TITLE && owner.is(TextInputFocus::Browser) && focus.page;
    if !engine_active {
        *prev_page = false;
        *last_content = None;
        keyboard.clear();
        ime.clear();
        buttons.clear();
        wheels.clear();
        return;
    }
    // 页面从失活转激活的当帧：只丢弃积压的键盘/IME（避免把旧按键灌进刚聚焦的
    // 页面），鼠标点击/滚轮必须放行——首击用于把焦点给到 Servo 的 DOM 元素。
    if !*prev_page {
        for _ in keyboard.read() {}
        for _ in ime.read() {}
    }
    *prev_page = true;

    let Some(engine) = host.0.as_mut() else {
        return;
    };
    let Some(webview) = engine.webview.as_mut() else {
        return;
    };
    // 窗口模式：事件带真实主窗 entity，按 window 过滤；壁纸模式无主窗，
    // 事件 window 一律 PLACEHOLDER，全量接收（与 terminal/chat 消费方一致）。
    let primary = primary_window.single().ok();
    let Some((node, tf)) = page.iter().next() else {
        return;
    };
    let content = page_device_point(node, tf, &cursor);

    for ev in keyboard.read() {
        if let Some(p) = primary {
            if ev.window != p {
                continue;
            }
        }
        let kbd = browser_keyutils::keyboard_event_from_bevy(ev, &keys);
        webview.notify_input_event(InputEvent::Keyboard(kbd));
    }

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
                let user_dismissed = engine
                    .delegate
                    .as_ref()
                    .is_some_and(|d| d.ime.borrow_mut().control_id.take().is_some());
                (*window, user_dismissed.then_some(ImeEvent::Dismissed))
            }
        };
        let Some(event) = event else {
            continue;
        };
        if let Some(p) = primary {
            if window != p {
                continue;
            }
        }
        webview.notify_input_event(InputEvent::Ime(event));
    }

    match (content, *last_content) {
        (Some(p), prev) if Some(p) != prev => {
            webview.notify_input_event(InputEvent::MouseMove(MouseMoveEvent::new(
                servo::WebViewPoint::Device(DevicePoint::new(p.x, p.y)),
            )));
        }
        (None, Some(_)) => {
            webview.notify_input_event(InputEvent::MouseLeftViewport(
                MouseLeftViewportEvent::default(),
            ));
        }
        _ => {}
    }
    *last_content = content;

    let Some(pos) = content else {
        return;
    };
    let point = DevicePoint::new(pos.x, pos.y);

    for ev in buttons.read() {
        let servo_button = match ev.button {
            MouseButton::Left => ServoMouseButton::Left,
            MouseButton::Right => ServoMouseButton::Right,
            MouseButton::Middle => ServoMouseButton::Middle,
            _ => continue,
        };
        let action = match ev.state {
            ButtonState::Pressed => MouseButtonAction::Down,
            ButtonState::Released => MouseButtonAction::Up,
        };
        webview.notify_input_event(InputEvent::MouseButton(MouseButtonEvent::new(
            action,
            servo_button,
            servo::WebViewPoint::Device(point),
        )));
    }

    for ev in wheels.read() {
        if ui_consumed.0 {
            // UI 滚动链已接管本帧滚轮（如浏览器窗口内的通用滚动区），
            // 不再转发给 Servo，避免一滚两处生效。
            continue;
        }
        let (dx, dy, mode) = match ev.unit {
            MouseScrollUnit::Line => (
                f64::from(ev.x) * 38.0,
                f64::from(ev.y) * 38.0,
                WheelMode::DeltaLine,
            ),
            MouseScrollUnit::Pixel => (f64::from(ev.x), f64::from(ev.y), WheelMode::DeltaPixel),
        };
        webview.notify_input_event(InputEvent::Wheel(WheelEvent::new(
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

fn browser_drive(
    mut host: NonSendMut<BrowserHost>,
    mut frame: ResMut<BrowserFrame>,
    mut pending: ResMut<PendingNav>,
    home: Res<HomeUrl>,
    page: Query<(Entity, &ChildOf, &ComputedNode), With<BrowserPage>>,
    windows: Query<(&AppWindow, &AppVisible)>,
) {
    let Ok((_, child_of, node)) = page.single() else {
        return;
    };
    let Ok((_, app_visible)) = windows.get(child_of.get()) else {
        return;
    };
    if !app_visible.0 {
        return;
    }

    if host.0.is_none() {
        info!("[browser] 首次启动 Servo 引擎…");
        *host = BrowserHost(Some(build_engine(&home.0)));
        info!("[browser] Servo 引擎就绪");
    }
    let Some(engine) = host.0.as_mut() else {
        return;
    };

    if !engine.has_session() {
        // 关窗后重开：轻量重建页面会话（引擎/GL 常驻），直接回起始页
        info!("[browser] 重建页面会话 → {}", home.0);
        engine.build_session(&home.0);
    }
    let Some(webview) = engine.webview.as_mut() else {
        return;
    };

    let (w, h) = page_device_size(node);
    let new_size = PhysicalSize::new(w.max(1), h.max(1));
    if new_size != engine.size {
        webview.resize(new_size);
        engine.size = new_size;
    }

    if let Some(command) = pending.0.take() {
        match command {
            NavCommand::Load(raw) => {
                if let Ok(url) = Url::parse(&raw).or_else(|_| Url::parse(&format!("https://{raw}")))
                {
                    info!("[browser] navigate → {url}");
                    webview.load(url);
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
                    webview.load(url);
                }
            }
        }
    }

    engine.servo.spin_event_loop();
    webview.paint();

    if let Some(image) = engine.interop.rendering_context_handle().read_full_frame() {
        let (w, h) = image.dimensions();
        frame.0 = Some(FrameDesc {
            pixels: image.into_raw(),
            width: w,
            height: h,
        });
    }

    if let Some(new_view) = engine
        .delegate
        .as_ref()
        .and_then(|d| d.take_pending())
    {
        new_view.resize(engine.size);
        engine.webview = Some(new_view);
    }
}

/// 帧尺寸与占位纹理不一致时更新 asset descriptor：触发 AssetEvent::Modified，
/// 让 render world 以新尺寸重建 GpuImage（否则注入端永远因尺寸不匹配而跳过）。
fn browser_resize_texture(
    frame: Res<BrowserFrame>,
    image: Res<BrowserImageHandle>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(desc) = frame.0.as_ref() else {
        return;
    };
    if let Some(mut img) = images.get_mut(&image.0) {
        if img.texture_descriptor.size.width != desc.width
            || img.texture_descriptor.size.height != desc.height
        {
            img.texture_descriptor.size = Extent3d {
                width: desc.width,
                height: desc.height,
                depth_or_array_layers: 1,
            };
        }
    }
}

/// 窗口真关闭（实体 despawn，区别于最小化）→ 丢弃页面会话（释放 DOM/JS），
/// 引擎核心（Servo/GL 上下文）保留供下次打开轻量重建；与终端「关窗即重置」一致。
fn browser_session_track(
    mut host: NonSendMut<BrowserHost>,
    home: Res<HomeUrl>,
    pages: Query<(), With<BrowserPage>>,
    mut urlbar: ResMut<UrlBarState>,
    mut focus: ResMut<BrowserFocus>,
    mut pending: ResMut<PendingNav>,
    mut current: ResMut<CurrentUrl>,
    mut was_open: Local<bool>,
) {
    let open = !pages.is_empty();
    if !open && *was_open {
        *urlbar = UrlBarState::default();
        *focus = BrowserFocus::default();
        pending.0 = None;
        current.0 = home.0.clone();
        if let Some(engine) = host.0.as_mut() {
            engine.detach_session();
        }
    }
    *was_open = open;
}

#[allow(clippy::too_many_arguments)]
fn browser_ui_sync(
    host: NonSend<BrowserHost>,
    focused: Res<FocusedTitle>,
    owner: Res<TextInputOwner>,
    focus: Res<BrowserFocus>,
    urlbar: Res<UrlBarState>,
    mut current: ResMut<CurrentUrl>,
    mut anchor: ResMut<BrowserImeAnchor>,
    mut labels: Query<&mut Text, With<UrlLabel>>,
    url_field: Query<(&ComputedNode, &UiGlobalTransform), With<UrlField>>,
    page: Query<(&ComputedNode, &UiGlobalTransform), With<BrowserPage>>,
) {
    if let Some(engine) = host.0.as_ref() {
        if let Some(delegate) = engine.delegate.as_ref() {
            if let Some(url) = delegate.url.borrow().as_ref() {
                if current.0 != *url {
                    current.0 = url.clone();
                }
            }
        }
    }

    let browser_active = focused.title == BROWSER_TITLE && owner.is(TextInputFocus::Browser);
    let editing = browser_active && !focus.page && urlbar.editing;
    let text = if editing {
        urlbar.visible()
    } else {
        current.0.clone()
    };
    if let Ok(mut label) = labels.single_mut() {
        if **label != text {
            **label = text;
        }
    }

    let mut next = BrowserImeAnchor {
        enabled: false,
        pos: Vec2::ZERO,
    };
    if editing {
        if let Ok((field, tf)) = url_field.single() {
            let field_size = field.size();
            let text_w = visible_text_width(&urlbar).min(field_size.x - 8.0);
            let x = -field_size.x * 0.5 + text_w;
            next.enabled = true;
            next.pos = tf.transform_point2(Vec2::new(x, field_size.y * 0.5 + 2.0));
        }
    } else if browser_active && focus.page {
        if let Some(engine) = host.0.as_ref() {
            if let Some(delegate) = engine.delegate.as_ref() {
                let ime = delegate.ime.borrow();
                if let (Some(_), Some((rx, ry))) = (ime.control_id, ime.rect_min) {
                    if let Ok((node, tf)) = page.single() {
                        let half = node.size() * 0.5;
                        let top_left = tf.transform_point2(-half);
                        next.enabled = true;
                        next.pos = top_left + Vec2::new(rx, ry);
                    }
                }
            }
        }
    }
    if anchor.enabled != next.enabled || anchor.pos != next.pos {
        *anchor = next;
    }
}

fn visible_text_width(urlbar: &UrlBarState) -> f32 {
    const BAR_PAD: f32 = 10.0;
    const ADVANCE: f32 = BAR_TEXT_SIZE * 0.6;
    let w: f32 = urlbar
        .visible()
        .chars()
        .map(|c| if c.is_ascii() { ADVANCE } else { ADVANCE * 2.0 })
        .sum();
    BAR_PAD + w
}

fn extract_browser_frame(
    frame: Extract<Res<BrowserFrame>>,
    page: Extract<Query<&ImageNode, With<BrowserPage>>>,
    mut out_frame: ResMut<ExtractedFrame>,
    mut out_id: ResMut<ExtractedImageId>,
) {
    out_frame.0 = frame.0.clone();
    out_id.0 = page.iter().next().map(|img| img.image.id());
}

fn inject_browser_frame(
    frame: Res<ExtractedFrame>,
    image_id: Res<ExtractedImageId>,
    render_queue: Res<RenderQueue>,
    gpu_images: Res<RenderAssets<GpuImage>>,
) {
    let (Some(desc), Some(id)) = (frame.0.clone(), image_id.0) else {
        return;
    };
    let Some(gpu_image) = gpu_images.get(id) else {
        return;
    };
    if gpu_image.texture_descriptor.size.width != desc.width
        || gpu_image.texture_descriptor.size.height != desc.height
    {
        return;
    }
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
