# AGENTS.md — n3ri_os

## What This Is

Bevy 0.19 pseudo-OS desktop environment replicating [os.inori.ai](https://os.inori.ai). Rust workspace. Chinese UI.

## Non-Goals（明确不做）

- **真实浏览器窗口**：dock 的「浏览器」只开一个占位提示窗（`apps/browser.rs`，纯静态文本），
  不要给它加真实网页能力。复刻官方 web 端内嵌浏览器需要 bevy_cef 或 bevy_wry(0.16)——复杂度陡增、
  需分析 web 端静态资源（Nori_web/NoriOS_files 等抓取残留）、拖慢编译，收益不成比例。**不要实现它**，
  也不要为它搭 HTML 渲染方案；若未来要做，从 bevy_wry 起步并单独评估。

## Build & Run

```bash
cargo run -p n3ri-minimal
```

No other binary targets exist. The only runnable crate is `examples/minimal`.

## Workspace Structure

```
Cargo.toml          # workspace root — resolver = "2"
crates/n3ri-core/   # state machine, events, config — no rendering deps
crates/n3ri-ui/     # all UI: dock, topbar, window mgmt, apps, shader
examples/minimal/   # the actual binary — wires core + ui together
assets/nori/        # extracted from os.inori.ai — fonts, icons, textures, audio
assets/shaders/     # WGSL shaders (desktop_background.wgsl)
```

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
