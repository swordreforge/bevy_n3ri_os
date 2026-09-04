# AGENTS.md — n3ri_os

## What This Is

Bevy 0.19 pseudo-OS desktop environment replicating [os.inori.ai](https://os.inori.ai). Rust workspace. Chinese UI.

## 浏览器（2026-09-04 起为真实功能）

dock 的「浏览器」是**真实网页浏览器**：`apps/browser.rs`（BrowserPlugin）把 Servo 引擎嵌入
桌面窗口。技术路线 = wgpu-graft（`refer/wgpu-graft`）Linux **CPU readback**：
Servo 经 surfman/GL 离屏渲染 → `read_full_frame()` 读回 RGBA → render world
`queue.write_texture` 上传到 `ImageNode` 的稳定纹理（RENDER_WORLD + COPY_DST）。

- dock 只调用 `request_browser(&mut BrowserLaunch)` 发请求，真正建窗在
  `browser_launch_window`（需要 `Assets<Image>` 占位纹理 + desktop root + N3riFonts）。
- 输入路由：窗口焦点 = `FocusedTitle.title=="浏览器"` + `TextInputOwner::Browser`；
  页面/地址栏二选一（`BrowserFocus.page`）。键盘/鼠标/滚轮/IME 经
  `browser_keyutils.rs`（Bevy→keyboard_types）转发；IME 锚点写 `BrowserImeAnchor`
  由 `input_focus::sync_ime_window` 消费。
- 引擎懒构建（首次打开时一次性 `build_engine`，会卡一帧）；引擎核心
  （Servo 实例 + interop/GL 上下文）关窗不销毁，但**页面会话随窗丢弃**：
  `browser_session_track` 在窗口 despawn 时 `detach_session()`（drop WebView，
  释放页面 DOM/JS），下次打开 `browser_drive` 检测无会话 → `build_session` 轻量
  重建并回起始页——与终端「关窗即重置」语义一致；最小化只暂停 paint/readback，
  不算关闭。
- 依赖：`servo` git release/v0.5（default-features=false，baked-in-resources/
  bundled_freetype/js_jit）+ `servo-wgpu-interop-adapter`/`grafting`（path 直连
  `refer/wgpu-graft/`，MPL-2.0）。根 Cargo.toml 的 glslopt patch 是 Servo 编译
  必要条件，勿删。wgpu/winit 直依赖版本必须与 bevy 0.19 统一（纹理类型共享）。
- 壁纸模式：键盘/IME/滚轮经 `wallpaper_keyboard`/`wallpaper_ime`/`wallpaper_bridge`
  桥接注入 bevy 消息，浏览器同样可点链接/滚动/输入（需壁纸 surface 持有合成器
  键盘与 text-input 焦点）。页面输入坐标即 UI 渲染空间物理像素（`cursor.physical`），
  与 `ComputedNode`/`UiGlobalTransform` 同空间——勿再乘 scale（曾致页面点击/滚动全失效）。
- **不要改回** bevy_wry/bevy_cef 方案；也不要移除 CPU readback 改共享纹理（桌面
  单 GPU 是同一 Vulkan，但 render world 线程隔离使 handle 导入复杂化，收益低）。

## Non-Goals（明确不做）

## Build & Run

```bash
cargo run -p n3ri-minimal            # 窗口模式（默认）
cargo run -p n3ri-minimal -- --wallpaper   # 壁纸模式（layer-shell 桌面壁纸）
cargo run -p n3ri-minimal -- --satellite   # 全局指针卫星进程（壁纸模式自动拉起，也可手动调试）
cargo run -p n3ri-minimal --features embed-assets  # 嵌入资源模式（单二进制）
```

壁纸模式限制：键盘/IME 需壁纸 surface 持有合成器键盘与 text-input 焦点（经
`wallpaper_keyboard`/`wallpaper_ime` 桥接，非 winit 直通）；指针被其他窗口遮挡时卫星 delta 外推视差；
需 `input` 组权限（`sudo usermod -aG input $USER` 后重新登录）。
滚轮（触摸板/鼠标）经 vendored `bevy_live_wallpaper` 的 Wayland axis 捕获，指针位于壁纸 surface 上时生效。
设置 → 显示效果 → 壁纸模式 开关可互斥切换两种模式（自我重启）。

No other binary targets exist. The only runnable crate is `examples/minimal`（单二进制三模式：默认窗口 / `--wallpaper` / `--satellite`).

`.cargo/config.toml` sets `RUST_BACKTRACE=full` globally for post-mortem debugging.

## Workspace Structure

```
Cargo.toml          # workspace root — resolver = "2"
crates/n3ri-core/   # state machine, events, config — no rendering deps
crates/n3ri-ui/     # all UI: dock, topbar, window mgmt, apps, shader, cursor/wallpaper bridge
crates/n3ri-llm/    # OpenAI 兼容 LLM 客户端
crates/n3ri-live2d/ # Live2D 桌面宠物
examples/minimal/   # the actual binary — 窗口/壁纸/卫星三模式入口
vendor/             # vendored deps（本地补丁）— 当前含 bevy_live_wallpaper
assets/nori/        # extracted from os.inori.ai — fonts, icons, textures, audio
assets/shaders/     # WGSL shaders (desktop_background.wgsl)
```

### 光标与壁纸桥接（n3ri-ui）

- `cursor.rs`：`CursorPosition`（logical/physical/scale/active）与 `UiArea` 资源。**所有交互系统
  （window/resize/dock/snap/scroll/desktop/focus）只读这两个资源，禁止直接查询主窗**。
  窗口模式由 `sync_cursor_from_window`（First）同步；壁纸模式由 `wallpaper_bridge` 合并系统写入。
- `wallpaper_bridge.rs`（仅壁纸模式注册）：layer-shell 指针 + 卫星 delta 合并光标；
  按钮 diff → `MouseButtonInput` 消息（避免与 `ButtonInput` 每帧 clear 竞态）；
  滚轮 = `inject_mouse_wheel` 合并两条路径——vendored `WallpaperPointerState.scroll`
  （Wayland `wl_pointer.axis`，真实触摸板/鼠标滚轮）与卫星 `w dx dy`（仅 XTEST 合成事件可达），
  写成 `MouseWheel` 消息由 `scroll_wheel_system` 消费；`WallpaperPointerState.scroll` 消费后清零；
  `wallpaper_ui_focus_system` 复刻 bevy `ui_focus_system`（原版对 Image 相机直接跳过 Interaction）
  并 `.after(ui_focus_system)` 覆盖其重置结果。
- 卫星协议：stdout 行流 `x y`（XQueryPointer 轮询的桌面全局绝对坐标，位置变化才发行，~120Hz）；
  `w dx dy`（核心协议按钮 4/5/6/7 滚轮增量，仅 press 计一次，变化才发行）；
  父进程退出 → stdin EOF → 卫星自杀。触摸板/鼠标通吃；被遮挡时指针坐标依然有效。壁纸模式 v1 无键盘/IME。

### 壁纸模式滚轮（vendored bevy_live_wallpaper）

壁纸 surface 是 bevy 进程自己的 layer-shell surface——指针在其上时，合成器把 `wl_pointer.axis`
发给 **bevy 自己的 Wayland 连接**，xwayland-satellite 永远看不到真实触摸板滚动（XInput `present=false`）。
因此依赖必须 vendored（`vendor/bevy_live_wallpaper`，0.5.0 为最终版本）：`Dispatch<wl_pointer>`
处理 `Axis` 事件 → `PendingPointerEventKind::Scroll` → `apply_pointer_events` 累积进
`WallpaperPointerState.scroll`。**不要改回 crates.io 版本**，否则真实滚动丢失。

Commented-out crates (not in workspace): `n3ri-render`, `n3ri-audio`, `n3ri-live2d`, `n3ri-apps`.
Live2D FFI crates exist in `crates/` but are not workspace members.

## Critical Gotchas

### Bevy 0.19 `bevy_ui_render` Feature

When using `default-features = false`, `bevy_ui` does NOT include the UI rendering pipeline. You **must** explicitly add `bevy_ui_render` or UI nodes compile but render nothing:

```toml
# Workspace Cargo.toml — this is correct
bevy = { version = "0.19", default-features = false, features = ["bevy_ui_render"] }
```

Individual crates also need `bevy_ui_render` in their own feature list if they use UI rendering.

### Asset Path Convention

The `AssetPlugin` in `examples/minimal` sets `file_path: "../../assets"`. All asset loads use paths relative to `assets/`, e.g. `asset_server.load("nori/icon.png")` not `assets/nori/icon.png`.

### Font Loading

Fonts are loaded via `bevy_woff` (`WoffPlugin`). The `N3riFonts` resource in `crates/n3ri-ui/src/font.rs` holds handles for `default`, `terminal`, `ui`, `dock` contexts — all currently point to `sarasa-fixed-sc.woff2`. Use `fonts.get(FontContext::Terminal)` etc. to access.

### Desktop Background Shader

`crates/n3ri-ui/src/desktop.rs` uses `UiMaterialPlugin` with a custom `DesktopBackgroundMaterial`. The shader is at `assets/shaders/desktop_background.wgsl`. It imports `bevy_ui::ui_vertex_output::UiVertexOutput` — not the standard vertex output.

## Architecture Patterns

### State Machine

`OsState` (Boot → Loading → Desktop → Shutdown) with `DesktopState` sub-state. State transitions use `MessageReader<Event>` patterns, not direct `.set()` calls from arbitrary systems.

### Window Management

Windows are spawned via `crate::window::spawn_window(parent, title, app_id, w, h, fonts)`. The `AppWindow` component carries `title`, `app_id`, and `z` (z-order). Window drag/resize/snap/focus are handled by separate plugin systems in `window.rs`, `resize.rs`, `snap.rs`.

`IsDragging` resource (from `dock.rs`) is shared between dock magnification, window drag, and resize to prevent conflicts.

### Dock

Icons loaded from `assets/nori/app-icons/{name}/icon-a.png` and `icon-b.png`. Magnification uses distance-based scaling with `MAGNETIC_RANGE = 120px`. App launch is matched by string name in `dock_update` — new apps need a match arm added there.

### Topbar System Polling

`topbar.rs` reads real Linux system info: `/proc/stat` for CPU, `/proc/net/route` for network, `/sys/class/power_supply/` for battery, `pactl` for volume. The volume fetch spawns a background thread. These will not work on non-Linux or without those interfaces.

### Terminal

Uses `portable-pty` to spawn a real `sh` shell. VT100 escape sequence parsing is minimal (CSI K/D/C/G, OSC passthrough). Selection uses hit-testing against `PositionedGlyph` positions. Clipboard via `arboard` (Wayland data control feature).

## Adding a New App

1. Create `crates/n3ri-ui/src/apps/yourapp.rs` with a `spawn_yourapp(parent, fonts)` function
2. Add a `match` arm in `dock.rs` `dock_update` for the app name
3. Register the plugin in `crates/n3ri-ui/src/lib.rs` and `apps/mod.rs`
4. Add icon assets to `assets/nori/app-icons/yourapp/`

## Style Conventions

- Colors are hardcoded as constants at module top (e.g. `const WINDOW_BG`, `const TEXT_MAIN`)
- No shared color palette in code — `ThemeConfig` exists but is not used by UI modules
- Window dimensions and spacing are `const` values, not config-driven
- Chinese text for all UI labels (app names, status text)
