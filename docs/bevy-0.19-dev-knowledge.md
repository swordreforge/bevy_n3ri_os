# Bevy 0.19 Dev Knowledge — bevy_n3ri_os Full Reference

> Source: deep-dive of this repo (2026-09-04). Versions pinned in `Cargo.lock`.
> Scope: everything a developer needs to work productively in **this** Bevy 0.19 codebase —
> setup, ECS, UI, render world, input, assets, state, windowing, gotchas.

---

## 1. Pinned versions (ground truth)

| Crate | Locked | Declared | Notes |
|---|---|---|---|
| `bevy` / `bevy_ui` / `bevy_ecs` / `bevy_app` / `bevy_render` / `bevy_winit` | `0.19.1` | `0.19` | crates.io |
| `winit` | `0.30.13` | `0.30` (`n3ri-ui` direct dep) | Must stay aligned with `bevy_winit` 0.19 |
| `wgpu` | **triple: `28.0.0` + `29.0.4` + `30.0.1`** | direct `29`, vendor `29.0.3` | 28 = Bevy 0.19 internal; 29 = direct + `bevy_live_wallpaper`; 30 = `refer/wgpu-graft` Servo adapter |
| `wayland-client` / `wayland-backend` | `0.31.15` / `0.3.17` | vendor min `0.31.11`/`0.3.11` | layer-shell |
| `bevy_live_wallpaper` | `0.5.0` vendored | `path=../../vendor/bevy_live_wallpaper` | Local `axis` scroll patch; do not switch back to crates.io |
| `bevy_tweening` | `0.16.0` | `0.16` | pet/head animation |
| `bevy_woff` | `0.2.0` | `0.2` | WOFF2 font loader |
| `bevy_embedded_assets` | `0.16.0` optional | `minimal:43` | `embed-assets` feature only |
| `servo` | `0.5.0` git `release/v0.5` | git branch, `default-features=false` + `baked-in-resources/bundled_freetype/js_jit` | Embedded browser |
| `portable-pty` | `0.8.1` | `0.8` | terminal app |
| `arboard` | `3.6.1` | `3`, `default-features=false` + `wayland-data-control` | clipboard without X11 |
| `x11rb` | `0.13.2` | `0.13` | satellite pointer poll |
| `reqwest` | `0.12.28` | `0.12`, `blocking,json,rustls-tls`, no native-tls | LLM client |
| `smol_str` | `0.2.2` | `0.2` | UI strings |

Rust workspace: `resolver = "2"`, members `n3ri-core, n3ri-llm, n3ri-ui, n3ri-live2d, examples/minimal`
(includes `mocari` from crates.io as pure-Rust Live2D runtime).
Commented out (not members): `n3ri-render, n3ri-audio, n3ri-apps`.

---

## 2. Feature topology — the single most important setup fact

### 2.1 `bevy_ui_render` is mandatory with `default-features = false`

When `default-features = false`, `bevy_ui` does **NOT** include the UI rendering pipeline.
UI nodes compile but render nothing unless `bevy_ui_render` is also enabled.

```toml
# Cargo.toml (workspace) — correct, do not remove
bevy = { version = "0.19", default-features = false, features = ["bevy_ui_render"] }
```

Per-crate layering:

| Crate | Bevy features | Role |
|---|---|---|
| `n3ri-core` | `workspace + bevy_asset, bevy_log, bevy_state` | Render-free: state machine + assets + logging only |
| `n3ri-ui` | `workspace + bevy_ui, bevy_ui_render, bevy_sprite, bevy_text, bevy_asset, default_font, png, jpeg` | Full 2D UI stack + image loaders |
| `n3ri-live2d` | `workspace + bevy_asset, bevy_log, bevy_render, bevy_core_pipeline, bevy_sprite, bevy_sprite_render, bevy_text, bevy_window, png` | Offscreen RTT pet pipeline |
| `examples/minimal` (only runnable binary) | `workspace + bevy_asset, bevy_audio, bevy_render, bevy_core_pipeline, bevy_sprite, bevy_text, bevy_ui, bevy_ui_render, bevy_winit, bevy_window, multi_threaded, default_font, png, vorbis, mp3, flac, wav, aac, mp4, x11, wayland` | Only full windowed/audio/windowing target |
| `vendor/bevy_live_wallpaper` | `bevy 0.19 default-features=false + bevy_render, bevy_camera, bevy_window` | Minimal wallpaper plugin |

`bevy_ui_render` guarantees the `UiMaterial`/`UiNode` render path even when the `bevy_ui`
widget crate is not enabled (used by headless core + `DesktopBackgroundMaterial`).

### 2.2 Binary entry — one binary, three modes (`examples/minimal/src/main.rs`)

```bash
cargo run -p n3ri-minimal                          # windowed (default)
cargo run -p n3ri-minimal -- --wallpaper           # layer-shell wallpaper
cargo run -p n3ri-minimal -- --satellite           # global pointer satellite (also auto-spawned by wallpaper mode)
cargo run -p n3ri-minimal --features embed-assets  # single-binary embedded resources
```

Windowed (`run_windowed`, main.rs:94):

```rust
app.add_plugins(
    DefaultPlugins
        .set(LogPlugin { filter: log_filter(), ..default() })
        .set(WindowPlugin {
            primary_window: Some(Window {
                title: "n3ri_os".into(),
                resolution: (1920u32, 1080u32).into(),
                ..default()
            }),
            ..default()
        })
        .set(AssetPlugin { file_path: "../../assets".into(), ..default() }),
)
```

Wallpaper (`run_wallpaper`, main.rs:146) differences:

```rust
.set(WindowPlugin {
    primary_window: None,  // no winit main window!
    exit_condition: ExitCondition::DontExit,
    ..default()
})
// + LiveWallpaperPlugin + WallpaperInputBridgePlugin + WallpaperImePlugin + WallpaperKeyboardPlugin
// + WinitSettings Reactive{wait:15ms} both focused/unfocused — see §9.4 why
```

`EmbeddedAssetPlugin{ mode: ReplaceDefault }` **must** be added before `AssetPlugin`
(adding after panics). Asset path convention: `AssetPlugin.file_path = "../../assets"`,
so loads are `asset_server.load("nori/icon.png")`, never `assets/...`.

Cameras: windowed `commands.spawn(Camera2d)` (main.rs:228); wallpaper
`commands.spawn((Camera2d, LiveWallpaperCamera, IsDefaultUiCamera))` (main.rs:250).

---

## 3. ECS fundamentals as used here (Bevy 0.19)

### 3.1 Everything is `Message`, not `Event`

Bevy 0.19 renamed events to messages. **There is zero `add_event` / `EventReader` /
`EventWriter` in this codebase** — all custom + builtin comms use:

```rust
app.add_message::<MyMsg>();
fn sys(mut r: MessageReader<MyMsg>, mut w: MessageWriter<MyMsg>) { for e in r.read() { … } }
```

Custom messages: `n3ri-core/src/events.rs` (`BootComplete/LoadComplete/LoadProgress/
AppLaunch/AppClose/AppFocus/SystemMenu/Notification/Shutdown/WindowCreated/Moved/
Resized/Focused`), `chat_capsule.rs:39 ChatEmotionEvent`, `music::MusicCommand`.
Builtin messages: `KeyboardInput, MouseButtonInput, MouseWheel, Ime, AppExit`.

Pattern: bridges `MessageWriter`-inject → `bevy_input` rebuilds `ButtonInput` →
`Update` systems consume via `MessageReader` **or** `Res<ButtonInput<…>>`.
On focus loss, systems **drain** stale input: `reader.clear()` / `for _ in read(){}` —
see `terminal_input:546-548`, `browser_bar:754-756`, `browser_page:881-887`.

### 3.2 States / SubStates

```rust
// n3ri-core/src/state.rs
#[derive(States, Default)] enum OsState { #[default] Boot, Loading, Desktop, App(AppId), Shutdown }
#[derive(SubStates, Default)] #[source(OsState = OsState::Desktop)]
enum DesktopState { #[default] Normal, MenuOpen, Notification, Settings }
```

Wiring (`n3ri-core/src/lib.rs`): `init_state::<OsState>() + add_sub_state::<DesktopState>()`,
`handle_boot_complete.run_if(in_state(Boot))`, `handle_load_complete.run_if(in_state(Loading))`.
`minimal` additionally drives boot/loading screens via `OnEnter(Boot/Loading/Desktop)`.
Rule in AGENTS.md: transitions via `MessageReader<Event>` patterns, not arbitrary `.set()`.

### 3.3 Plugin organization (`n3ri-ui/src/lib.rs`)

```rust
app.add_plugins(WoffPlugin);
app.add_plugins(cursor::CursorPlugin);
app.init_resource::<TextInputOwner>();
app.add_systems(PostUpdate, sync_ime_window);
app.add_systems(Startup, load_fonts);
app.add_plugins((Topbar, DesktopFx, Dock, Window, Snap, Resize, Scroll, ChatCapsule, credits, clicker, settings, terminal, files, txt_reader, log_viewer));
app.add_plugins((image_viewer, chess, mail, pictionary, seek_treasure, signal, cakeduel, browser));
```

One `Plugin` per app (`TerminalPlugin`, `BrowserPlugin`, …). New app checklist (§12).

### 3.4 Schedules and ordering

| Schedule | Who | Purpose |
|---|---|---|
| `First` | `sync_cursor_from_window` (windowed) / `(drain_satellite → sync_cursor_from_wallpaper).chain() + inject_mouse_buttons + inject_mouse_wheel.after(drain)` (wallpaper) | Cursor/buttons/wheel ready **before** `PreUpdate` Focus/Input |
| `PreUpdate` | `wallpaper_keyboard.before(InputSystems)`, `wallpaper_ime_inject`, `wallpaper_ui_focus_system.after(UiSystems::Focus)` | Input injection + focus override |
| `Update` | Everything app-level | `WindowFocusSet{promote.before(focus), focus, validate.after}` → interact/launch `.after(WindowFocusSet)` → `input/ime.after(selection)` → `dispatch.after(input)`; browser chain `launch→chrome→bar→page→drive→resize_texture→ui_sync→session_track` all `.after`; `resize_apply/end.after(start)`, `snap preview/apply.after(detect)` |
| `PostUpdate` | `sync_ime_window` | IME anchor after layout |
| `Startup` / `OnEnter` | fonts, camera, desktop, boot/loading screens | one-shot setup |
| `Render` / `ExtractSchedule` | `extract_browser_frame` (extract) → `inject_browser_frame.after(PrepareAssets).before(Queue)` | GPU upload (§6) |

### 3.5 B0001 conflict avoidance (`docs/bevy-ecs-patterns.md`)

`error[B0001]` = two systems (or two params in one system) access the same component
read+write. Fixes used here, in order of preference:

1. **Merge systems** into one.
2. **`Commands` deferred writes** instead of direct `&mut`.
3. **Marker-component split** (the codebase's standard): business state in markers
   (`AppVisible(bool)`, `IsDragging`), render state (`Visibility`, `Node`) written via
   `Commands`. Architecture: `business markers → Commands → render state`.
4. **`Without` filters** to prove disjointness.
5. **`ParamSet`** for time-shared access.

Common conflict components: `Visibility`, `Node`, `Transform`, `BackgroundColor`.
Extra rules: explicit `.after()` ordering; `ResMut<IsDragging>` both serializes
*and* acts as gesture mutex (§8).

---

## 4. UI toolkit reference (Bevy 0.19 `bevy_ui` as used here)

### 4.1 Spawning idiom

```rust
parent.spawn((
    AppWindow { title, app_id, z: 1 }, AppVisible(true), GlobalZIndex(1),
    Node { position_type: PositionType::Absolute, width: Val::Px(w), height: Val::Px(h + 32.0),
        flex_direction: FlexDirection::Column, top: Val::Px(72.0),
        left: Val::Px(((1920.0 - w) / 2.0).max(0.0)),
        border_radius: BorderRadius::all(Val::Px(10.0)), overflow: Overflow::hidden(), ..default() },
    BackgroundColor(WINDOW_BG),
)).with_children(|window| {
    window.spawn((TitleBar, Button, Node { width: Val::Percent(100.0), height: Val::Px(32.0),
        align_items: AlignItems::Center, justify_content: JustifyContent::SpaceBetween,
        display: Display::Flex, ..default() }));
    // traffic lights 12×12 circles, BorderRadius 6px; title Text::new + TextFont + TextColor
});
```

Key components: `Node`, `Text::new(..)` + `TextFont{font: Handle<Font>, font_size}` +
`TextColor`, `ImageNode{image: Handle<Image>}`, `MaterialNode<M>(handle)`,
`BackgroundColor`, `BorderColor::all(..)`, `Button`, `Interaction`, `Visibility`
(`Inherited`/`Hidden`), `GlobalZIndex` (cross-tree) vs `ZIndex` (intra-stack),
`ChildOf` (0.19 parent link — `query.get::<ChildOf>().get()`), `FocusPolicy::Block/Pass`.

### 4.2 Layout values

- Sizes: `Val::Px(..)` fixed, `Val::Percent(100.0)` bleed, `min_width: Val::Px(150)` popups,
  `max_width: Val::Px(460)` chat bubbles.
- Chrome is `PositionType::Absolute`; content is `FlexDirection::{Column, Row}` with
  `Center / SpaceBetween / FlexEnd`, `gap`, `padding: UiRect::px(..)/all(..)`.
- Rounded corners require `overflow: Overflow::hidden()` or children bleed out.
- Stacking: windows `GlobalZIndex 1..32`, dock `50`, topbar/chat `100`,
  scrollbar `ZIndex(1)`, snap preview `GlobalZIndex(-1)`.
- Change-guard every per-frame write (`if **text != target`, dock `last_widths` map,
  `set_width_if_changed`) — otherwise taffy re-layouts every frame.

### 4.3 Interaction gate (universal)

```rust
if *interaction == Interaction::Pressed && mouse.just_pressed(MouseButton::Left) { … }
```

`Hovered` for tooltips. `Button` + `Interaction` is the only click source in windowed mode;
wallpaper mode re-derives `Interaction` from the cursor resource (§9).

### 4.4 Fonts (`font.rs`, `bevy_woff`)

```rust
#[derive(Resource)] pub struct N3riFonts { pub default, terminal, ui, dock: Handle<Font> }
pub enum FontContext { Terminal, Ui, Dock }
pub fn load_fonts(commands, asset_server) // Startup; all four = "nori/fonts/sarasa-fixed-sc.woff2" today
```

Usage: `TextFont { font: fonts.default.clone(), .. }`, `fonts.get(FontContext::Dock)`.
`WoffPlugin` registered in `N3riUiPlugin`. All UI labels are Chinese.

---

## 5. Coordinate spaces — the most expensive lesson (`docs/terminal-hit-testing-lessons.md`)

Bevy 0.19 has **two** pixel spaces. Mixing them compiles fine and only breaks on
scale ≠ 100% (error grows linearly with distance from top-left; sf=1.0 hides it):

| API | Space |
|---|---|
| `Window::cursor_position()` | **logical** (physical ÷ scale) |
| `Window::physical_cursor_position()` | **physical** |
| `ComputedNode` / `UiGlobalTransform` / `TextLayoutInfo` | **physical** |
| `Node` style `Val::Px` (left/top/width/height) | **logical** |

Rules:

```rust
// Hit-test against layout — PHYSICAL (window.rs:303, scroll.rs:145, browser.rs:563):
let local = transform.try_inverse().map(|t| t.transform_point2(cursor.physical));
let half = node.size() * 0.5;
local.x.abs() <= half.x && local.y.abs() <= half.y
// or: node.contains_point(*transform, point) + clip_check_recursive + normalize_point

// Drag/resize/snap against Node style — LOGICAL:
WindowDrag { offset: cursor.logical - Vec2(win_left, win_top) }

// IME anchor is the reverse projection:
window.ime_position = transform.transform_point2(local);
```

This repo centralizes the duality in `CursorPosition{ logical, physical, scale, active }`
+ `UiArea(Vec2)` (`cursor.rs`). **All interact systems read those two resources, never
`Window` directly.** Windowed sync (`First`): `area = window.size()`,
`logical = cursor_position()`, `scale = scale_factor()`,
`physical = physical_cursor_position() || pos * scale`.
Wallpaper sync takes over the same resources when there is no window (§9).

Checklist for any new hit-test: source/target spaces match; tested at scale ≠ 100%;
aligned with `ui_focus_system`; float→index goes through signed types
(`(neg) as usize` saturates to 0!); viewport math uses measured `ComputedNode.size()`,
constants only as pre-layout fallback.

---

## 6. Render-world + GPU bridges (browser readback, Live2D RTT)

### 6.1 Canonical pattern: Servo browser CPU readback (`apps/browser.rs`)

Linux path: `Servo / surfman / GL (main world) → read_full_frame() → Vec<u8> →
Extract → Render: queue.write_texture(GpuImage)`. No zero-copy.

Texture creation (`create_placeholder_image:399`) — memorize these flags:

```rust
let mut placeholder = Image::new_uninit(
    Extent3d { width: 1024, height: 600, depth_or_array_layers: 1 },
    TextureDimension::D2,
    TextureFormat::Rgba8Unorm,          // linear — Servo sRGB bytes shown as-is
    RenderAssetUsages::RENDER_WORLD,    // GPU-only, no MAIN, no CPU data
);
placeholder.texture_descriptor.usage =
    TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST;
commands.insert_resource(BrowserImageHandle(images.add(placeholder)));
```

`new_uninit` = descriptor + `data: None`, never read on CPU. The `Handle<Image>` /
`AssetId` is **stable forever**; only `texture_descriptor.size` is mutated on resize —
that mutation fires `AssetEvent::Modified` so render-world recreates `GpuImage` and
refreshes the `ImageNode` bind group. Without it the inject step size-mismatches and
skips every frame.

Main-world drive (`browser_drive`, `Update`, gated on `AppVisible`, lazy `build_engine`
that hitches one frame on first open):

```rust
engine.servo.spin_event_loop(); webview.paint();
if let Some(image) = engine.interop.rendering_context_handle().read_full_frame() {
    frame.0 = Some(FrameDesc { pixels: image.into_raw(), width: w, height: h });
}
```

Extract → render inject (`ExtractSchedule` → `Render.after(PrepareAssets).before(Queue)`):

```rust
// extract: only Send data crosses (BrowserFrame: Vec<u8> + u32s, cloned)
out_frame.0 = frame.0.clone();
out_id.0 = page.iter().next().map(|img| img.image.id());
// render:
render_queue.write_texture(
    TexelCopyTextureInfo { texture: &gpu_image.texture, .. },
    &desc.pixels,
    TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * w), rows_per_image: Some(h) },
    Extent3d { width: w, height: h, depth_or_array_layers: 1 });
```

`bytes_per_row = 4*w` is legal for `write_texture` (the 256-alignment rule only
applies to `copy_texture_to_buffer`). The inject must early-return (not panic) on the
1-frame size race between main `Assets<Image>` and render `GpuImage`.

`page_device_size` comes from `ComputedNode.size().round()` with `hidpi = 1.0` —
device px 1:1, **do not multiply by scale_factor** (a past bug multiplied and killed
all page clicks/scrolls). Always resize via `webview.resize(size)`, never the raw
GL context. `take_pending()` (same-tab `target=_blank`) drains *after* paint/export
to avoid reentrant `RefCell` borrows.

### 6.2 Threading constraints

- `BrowserHost(Option<BrowserEngine>)` is `NonSend` (`Rc<RefCell>`, surfman
  `Device/Context` are `!Send/!Sync`): `insert_non_send`, access only via
  `NonSend` / `NonSendMut`. Never put it in `Extract`.
- Never touch `RenderDevice / RenderQueue / RenderAssets<GpuImage>` from main;
  never touch `Servo / WebView / interop` from render.
- `n3ri-ui wgpu = 29` must match the adapter's `wgpu-29` feature (imported textures
  are only valid on the same major version).
- Engine core (Servo instance + GL context) survives window close; the **page session
  is dropped** (`detach_session` on window despawn) and rebuilt lightly on next open —
  same "close resets" semantics as the terminal. Minimize only pauses paint/readback.
- Do not revert to `bevy_wry` / `bevy_cef`, and do not replace CPU readback with
  shared-texture import (same-Vulkan render-world thread isolation makes handle
  import complex for little gain).

### 6.3 `UiMaterial` vs `Material2d`

`UiMaterial` (desktop background, `desktop.rs`):

```rust
#[derive(AsBindGroup, Asset, TypePath, Debug, Clone)]
pub struct DesktopBackgroundMaterial {
    #[uniform(0)] pub mouse_pos: Vec2, #[uniform(0)] pub time: f32,
    #[uniform(0)] pub zoom: f32, #[uniform(0)] pub offset: Vec2, // all uniform(0) → one Config block
    #[texture(1)] #[sampler(2)] pub water_normal: Handle<Image>,
    #[texture(3)] #[sampler(4)] pub noise: Handle<Image>,
}
impl UiMaterial for DesktopBackgroundMaterial {
    fn fragment_shader() -> ShaderRef { "shaders/desktop_background.wgsl".into() }
}
// Plugin: UiMaterialPlugin::<M>::default() + Update animate (mouse = logical/area*2-1, time += delta)
// Node: MaterialNode<M>(handle) + fullscreen absolute Node
```

Shader (`assets/shaders/desktop_background.wgsl`) must use the **UI** vertex import,
not the sprite one:

```wgsl
#import bevy_ui::ui_vertex_output::UiVertexOutput
struct Config { mouse_pos: vec2<f32>, time: f32, zoom: f32, offset: vec2<f32> };
@group(1) @binding(0) var<uniform> config: Config; // group1 = material (group0 = UI view)
```

`Material2d` contrast (`n3ri-live2d/src/renderer.rs:201 Live2dDrawableMaterial`):
`#[bind_group_data(Key)]`, `Material2dPlugin + MeshMaterial2d + Mesh2d +
RenderLayers(1)`, fragment `live2d_drawable.wgsl` imports
`bevy_sprite::mesh2d_vertex_output::VertexOutput` + `#{MATERIAL_BIND_GROUP}`.

### 6.4 `Image` creation cheat-sheet

| Site | Constructor | Format / usage | CPU data |
|---|---|---|---|
| browser page | `Image::new_uninit(extent, D2, Rgba8Unorm, RENDER_WORLD)` + `TEXTURE_BINDING\|COPY_DST` | linear, GPU-only | `None` |
| pictionary canvas 512² | `Image::new(extent, D2, data(paper), Rgba8UnormSrgb, default())` | sRGB, `MAIN\|RENDER`, mutated on CPU | retained (`stamp_circle/draw_line` → `Modified` re-upload) |
| Live2D pet/head/mask RTT | `Image::new_target_texture(w, h, Bgra8UnormSrgb, None)` + drop CPU data | sRGB target, `Camera{RenderTarget::Image} + Msaa::Sample4` | **must clear** — constructor forces zeroed data + `copy_on_resize=true`, which stalls per-frame-cleared RTTs |
| 1px fallback | `Image::new_fill(1×1, D2, white, Bgra8UnormSrgb, RENDER_WORLD)` | static | static |
| image_viewer / ocean | `asset_server.load(..)` | loader-decoded, sampled in WGSL | n/a |

---

## 7. Window management (`window.rs`, `resize.rs`, `snap.rs`, `scroll.rs`)

- Components: `AppWindow{ title, app_id, z }`, `TitleBar`, `Close/Minimize/MaximizeButton`,
  `WindowDrag{ offset }`, `WindowOriginalLayout`, `Maximized`, `CinematicLocked`
  (no drag/resize/min/max/snap, close only). `MAX_WINDOW_Z = 32` with compaction.
- Spawn: `spawn_window(parent, title, app_id, w, h, fonts)` (+32px title bar);
  `spawn_window_with_options(…, show_minimize)` for txt/log viewers.
- `WindowPlugin` order: `promote_added_windows.before(focus)` →
  `window_focus_system.in_set(WindowFocusSet)` → `window_focus_validate.after(focus)`,
  plus drag/close/min/max systems. New windows auto-promote z + `FocusedTitle`.
- Focus: `TitleBar Interaction::Pressed` via `ChildOf` walk, else topmost hit
  (`cursor.physical` vs `ComputedNode`) by max `z`; overflow compacts to `1..n`.
  `find_window_entity` walks `ChildOf` to the `AppWindow` root.
- Drag (`window_drag_*`): `just_pressed(Left) + !IsDragging + TitleBar Pressed`,
  `offset = logical − (left, top)`; clamp top to `[32, H − 66]` (66 = dock total).
- Close: `despawn` (+ `terminal.reset()` for the terminal);
  minimize: `AppVisible(false) + Visibility::Hidden`;
  maximize: stash `WindowOriginalLayout`, fill `[0, 32] → [W, H−66−32]`, second press restores.
- Resize (`resize.rs`): 8px handles, min 200×100; `ResizeEdge` + `ResizeState`;
  hover sets `CursorIcon::System(Ew/Ns/Nwse/Nesw)` on the primary window;
  apply clamps per edge; `resize_end` **removes** the icon (see §8.3).
- Snap (`snap.rs`): corner 80px / edge 20px zones, top = maximize; preview entity
  (`SnapPreview`, `GlobalZIndex(-1)`, write `Node` only on change); apply on release.
- Scroll (`scroll.rs`): `ScrollableArea{ scroll_offset }`, `WHEEL = 40`,
  `MessageReader<MouseWheel>` summed; topmost-window + `window_ancestor == target`
  gating; thumb drag ratio-maps; `sync` writes content `Node.top = Px(−offset)` and
  thumb geometry.

---

## 8. Gesture isolation — three-layer bug history (`docs/window-gesture-isolation.md`)

Any new drag/resize/snap interaction must satisfy all three (one symptom had three
independent faults: `d1c7fb6`, `011b197`):

1. **Mutual exclusion on shared input.** ECS scheduling serializes on access
   conflicts but does *not* stop two systems reacting to the same `just_pressed`.
   Gate every gesture start on shared `IsDragging`:
   `if is_dragging.0 { return; }` — check-then-set serializes via `ResMut`.
2. **Hit-test filtering is the isolation boundary.** Hidden/minimized windows keep
   their `Node` data and still match queries. Always filter `&Visibility` (and
   `CinematicLocked`, `AppVisible`) in hover/start queries or "ghost edges" trigger.
3. **Every `insert` needs a paired `remove` on all exits.** `ResizeState` reset does
   not roll back `CursorIcon` inserted on the OS window. Clean up on: normal release,
   cursor-leaves-window, window-closed.

Plus: keep min-size constraints through the whole gesture; re-read the reporter's
original hypothesis if symptoms persist after one layer is fixed (each fix can unmask
the next layer).

---

## 9. Input, cursor, and wallpaper bridges

### 9.1 The `CursorPosition` / `UiArea` contract (`cursor.rs`)

All interact systems read only these two resources. Windowed mode fills them in
`First`; wallpaper mode's bridge takes over the *same* resources (never both).

### 9.2 Wallpaper pointer + satellite (`wallpaper_bridge.rs`, `main.rs:257-458`)

- `sync_cursor_from_wallpaper`: `area = surface.size`, `ui_scale = surface.scale.max(1)`;
  layer-shell `pointer.last` (trusted buttons) wins, else satellite `XQueryPointer`
  absolute pos (valid even when occluded).
- Buttons (`inject_mouse_buttons`, `First`): diff pressed set → 
  `MessageWriter<MouseButtonInput>{ button, state, window: PLACEHOLDER }`. Never write
  `ButtonInput` directly — let `mouse_button_input_system` rebuild it (avoids the
  per-frame-clear race). Consumers use either `ButtonInput::just_pressed` or raw
  `MessageReader<MouseButtonInput>` + `cursor.physical` geometry (browser).
- Wheel (`inject_mouse_wheel.after(drain_satellite)`, `First`): merges vendored
  `wl_pointer.axis` (`WallpaperPointerState.scroll`, real touchpad/mouse) + satellite
  `w dx dy` (synthetic 4/5/6/7 presses); `natural_scroll` only flips the touchpad leg;
  emits `MouseWheel{ unit: Line, … }`, zeroes the accumulator. Consumed by
  `scroll_wheel_system` (Σ `e.y`), terminal (`y*3`), browser page.
- Satellite protocol (stdout lines, only on change): `x y` absolute pos @~120Hz
  (8ms `XQueryPointer` poll), `w dx dy` wheel (buttons 4/5/6/7 presses only),
  `s w h` screen size once. Parent death → stdin EOF → satellite suicide.
  `drain_satellite` (First, chained before cursor sync) drains the mpsc channel into
  `SatelliteFrame{ pos, scroll }` + `SatelliteScreen`.

### 9.3 Focus override (wallpaper only)

Bevy 0.19's `ui_focus_system` skips `Image`-target cameras
("Interactions are only supported for cameras rendering to a window") and resets
`Interaction → None`. `wallpaper_ui_focus_system`
(`PreUpdate.after(UiSystems::Focus)`) re-implements the same algorithm against
`CursorPosition.physical` (`UiStack.partition` rev, `contains_point` +
`clip_check_recursive`, `Block/Pass` truncation) and overwrites the reset result.
`N3RI_FOCUS_DEBUG=1` enables a click-time probe (`focus_debug_probe`).

### 9.4 Keyboard / IME / browser key mapping

- `wallpaper_keyboard` (`PreUpdate.before(InputSystems)`): drains evdev/XKB events,
  static US `keycode → KeyCode` + shift-sensitive `logical_key()` (mirrors
  `bevy_winit::convert_logical_key`), cross-frame modifier state, emits
  `KeyboardInput{ …, repeat: false, window: PLACEHOLDER }`. Chinese goes through IME, not here.
- `wallpaper_ime`: `text-input-v3` double-buffer (`Preedit/Commit` → `ImeBatch`,
  flush on `Done/Enter/Leave` as `Ime::{Commit, Preedit, Enabled, Disabled}`);
  reverse: `TextInputOwner != None → control.enabled = true`.
- Windowed IME (`PostUpdate sync_ime_window`): `TextInputOwner{ None, Chat,
  Terminal, Settings(usize), Pictionary, SeekTreasure, Browser }` + `FocusedTitle` /
  `BrowserImeAnchor` / `ComputedNode` → `Window{ ime_enabled, ime_position }`.
- `browser_keyutils.rs`: Bevy `KeyboardInput` + live `ButtonInput<KeyCode>` modifier
  snapshot → Servo W3C `KeyboardEvent` (`key_state Down/Up`, `location`, 1:1 named keys,
  `Super → MetaLeft/Right`). `KeyboardInput` carries no modifier snapshot, so callers
  must pass `keys` in.
- Wallpaper `WinitSettings`: no main window ⇒ winit reports "unfocused" ⇒
  `reactive_low_power` drops to ~8fps and key releases lag. `Continuous` freezes with
  no window to redraw. Fix: `Reactive{ wait: 15ms }` for **both** modes ≈ 66fps tick.

### 9.5 Focus-loss hygiene

Every text/IME consumer clears its readers when unfocused
(`terminal:546-548,650-653`, `browser:754-756,881-887,628-631`) so stale keys don't
leak into the newly focused page. Copy this pattern for any new input consumer.

---

## 10. Assets, content, audio

- `AssetPlugin.file_path = "../../assets"` (both modes). Disk paths in code are
  assets-relative: `"nori/icon.png"`, `"nori/ocean/water-normal.png"`,
  `"nori/fonts/sarasa-fixed-sc.woff2"`, `"shaders/desktop_background.wgsl"`.
- Virtual content layer (`content.rs`): `read_bytes / read_to_string / exists /
  list_dir` take assets-relative (or absolute temp) paths, **disk-first** for hot-edit;
  `embed-content` feature falls back to a `build.rs`-generated `EMBEDDED_CONTENT_FILES`
  table (walks `app-icons/{files,mail,signal}`). `embed-assets =
  ["dep:bevy_embedded_assets", "n3ri-live2d/embed-model", "n3ri-ui/embed-content"]`.
- Icons: `assets/nori/app-icons/{name}/icon-a.png` (running) / `icon-b.png` (idle);
  content bundles (`files/本机/…`, `mail/*.mail.json`, `signal/*.chat.json`,
  `codenames/words.json`, `drawings.json`, `browser/home.html`, `bgm1.ogg`, …).
- Topbar probes real Linux: `/proc/stat` (CPU delta), `/proc/net/route` (default route),
  `/sys/class/power_supply/BAT*`, one-shot `pactl get-sink-volume` thread →
  `Arc<Mutex<Option<u8>>>`. Won't work off-Linux.
- Terminal: `portable-pty` (`sh`, `TERM=xterm-256color`, 40×120, 8KB reader thread →
  `Arc<Mutex<String>>`), minimal VT (`CSI K/D/C/G`, OSC passthrough), `arboard`
  clipboard (`Ctrl+Shift+C/V`), `PositionedGlyph` hit-test selection.
- LLM (`n3ri-llm`): OpenAI-compatible blocking + SSE client over rustls-only reqwest.
- Music: `MusicPlayerPlugin` (`lofty` tags) + `MusicCommand` message.

---

## 11. Patches, vendoring, and do-not-touch list

1. **`[patch.crates-io] glslopt`** (workspace root) — Servo/webrender pulls
   `glslopt 0.1.12` whose C11-threads shim `typedef pthread_once_t once_flag`
   conflicts with glibc 2.34+; the pinned git rev adds the `#ifndef` guard.
   Required to compile Servo on Fedora 40+ / Ubuntu 24.04+. Do not delete.
2. **Vendored `vendor/bevy_live_wallpaper` 0.5.0** — upstream never emits
   `wl_pointer.axis`; the vendored `Dispatch<wl_pointer>` → `PendingPointerEventKind::Scroll`
   → `WallpaperPointerState.scroll` path is the only real touchpad/mouse wheel source.
   Do not switch back to crates.io. Only the `wayland` feature is enabled (X11 goes
   through the `x11rb` satellite instead).
3. **`wgpu`/`winit` direct deps must match Bevy 0.19** (`wgpu 29`, `winit 0.30`) —
   texture/device types are shared across the Bevy ↔ adapter boundary.
4. **Browser stays on Servo + CPU readback.** No `bevy_wry`/`bevy_cef`, no shared-texture import.
5. **`.cargo/config.toml` forces `RUST_BACKTRACE=full`** for post-mortem boot-panic debugging.

---

## 12. How to add things (checklists)

### 12.1 New app

1. Create `crates/n3ri-ui/src/apps/yourapp.rs` with `spawn_yourapp(parent, fonts)` (+ `YourappPlugin` if it needs systems).
2. Add a match arm in `dock.rs` `dock_update` for the app name.
3. Register the plugin in `lib.rs` + `apps/mod.rs`.
4. Add `assets/nori/app-icons/yourapp/` (`icon-a/b.png` + content).
5. Follow: `AppVisible` marker + `Commands`-applied `Visibility`; `cursor.physical`
   hit-tests; `reader.clear()` on focus loss; `GlobalZIndex` in the 1..32 window band;
   change-guarded writes.

### 12.2 New gesture (from §8)

- [ ] Shared-state mutex with existing gestures (`IsDragging` check-then-set).
- [ ] Hit-test query filters `Visibility` + `AppVisible` + `CinematicLocked`.
- [ ] Every `insert` has a `remove` on release / cursor-leave / window-close.
- [ ] Cursor/preview feedback recycled on both end paths.
- [ ] Min-size constraints hold for the whole gesture.

### 12.3 New hit-test (from §5)

- [ ] Cursor source and comparison target are the same space (logical vs physical).
- [ ] Tested on a display with scale ≠ 100%.
- [ ] Matches `ui_focus_system`'s approach (`try_inverse` / `contains_point` + clip).
- [ ] Signed intermediates before integer indices; measured `ComputedNode` geometry,
      constants only as fallback.

---

## 13. Debugging

- `RUST_BACKTRACE=full` is already set via `.cargo/config.toml`.
- `N3RI_FOCUS_DEBUG=1` — wallpaper click probe (cursor / injected state / hover).
- `N3RI_PROF=1` (+ `profiling` feature) — per-system CPU diagnostics
  (`SystemInformationDiagnosticsPlugin`).
- Don't guess Bevy internals from memory — read the pinned sources in
  `~/.cargo/registry/src/…/bevy_ui-0.19.1/src/{focus,ui_node}.rs`; that's what
  settled the physical-vs-logical dispute (§5).
- Prior hypothesis discipline (`docs/terminal-hit-testing-lessons.md`): a hypothesis
  must explain **all** known symptoms, especially the most counter-intuitive one,
  or it is wrong or secondary.

---

## 14. Module map (one line each)

- `n3ri-core/{lib,state,events,config,music}` — stateless OS foundation (no rendering).
- `n3ri-ui/lib` — `N3riUiPlugin` aggregator; `Startup load_fonts`; `PostUpdate sync_ime_window`.
- `n3ri-ui/{window,resize,snap,scroll}` — window lifecycle, gestures, scrolling.
- `n3ri-ui/{dock,topbar,desktop,chat_capsule}` — shell chrome + LLM capsule.
- `n3ri-ui/{cursor,input_focus}` — cursor contract + IME ownership.
- `n3ri-ui/{wallpaper_bridge,wallpaper_keyboard,wallpaper_ime}` — wallpaper-only bridges.
- `n3ri-ui/{font,content}` — font handles + virtual FS layer.
- `n3ri-ui/apps/{terminal,files,browser,browser_keyutils,settings,mail,signal,txt_reader,log_viewer,image_viewer,clicker,cakeduel,international_chess,pictionary,seek_treasure,credits}` — 15 apps.
- `n3ri-llm` — blocking + SSE chat client + `llm-config.json`.
- `n3ri-live2d/{lib,loader,pet,renderer,head_anim}` — Cubism pet, RTT materials, head view.
- `examples/minimal/{main,focus,music_player}` — the only binary (3 modes).
- `vendor/bevy_live_wallpaper` — patched layer-shell backend (axis scroll).
- `assets/{shaders,nori,prompt}` + `assets_dev/` hot-edit overlay.
