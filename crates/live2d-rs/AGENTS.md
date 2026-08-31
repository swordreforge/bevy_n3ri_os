# live2d-rs — Agent Guide

## Workspace (6 crates)

| Crate | Description |
|---|---|
| `live2d-core-sys` | `bindgen` FFI for Cubism 5.x Core C API |
| `live2d-core` | Safe Rust wrapper: `Moc`, `Model`, `Parameters`, `Drawables` |
| `live2d-v2-core-sys` | `bindgen` FFI for Cubism 2.x C API (built from live2d-py C++ artifacts) |
| `live2d-v2-core` | Safe wrapper for V2 models |
| `live2d-viewer` | Binary: `winit` + `glutin` + `glow` (OpenGL 3.3 Core) + `egui` |

`live2d-py/` is a separate git submodule (not in workspace) — built C++ artifacts feed `live2d-v2-core-sys`.

## Build prerequisites

- **Rust 1.75+** (MSRV, set in each crate)
- **Cubism 5.x SDK**: place under workspace root as `CubismSdkForNative-5-r.5/`, or set `LIVE2D_SDK_ROOT` env var to point elsewhere
- **V2 (optional)**: build `live2d-py` first, then set `V2_PY_BUILD_DIR` pointing to its `build/` dir
- **OpenGL 3.3 Core** capable GPU (runtime, for viewer)

## Commands

```sh
# Build all (default features: static-link enabled)
cargo build --release

# Run viewer with a model
cargo run --release -p live2d-viewer -- /path/to/model-dir

# Overlay mode (small always-on-top window, bottom-right corner)
cargo run --release -p live2d-viewer -- --overlay /path/to/model-dir

# V3 model auto-scan: set LIVE2D_SDK_ROOT to scan Samples/Resources
LIVE2D_SDK_ROOT=/path/to/CubismSdkForNative-5-r.5 cargo run --release -p live2d-viewer

# Build without static linking (dynamic .so load at runtime)
cargo build --release --no-default-features

# Testing (requires SDK samples available at LIVE2D_SDK_ROOT/Samples/Resources)
LIVE2D_SDK_ROOT=/path/to/CubismSdkForNative-5-r.5 cargo test --release -p live2d-core-sys
```

## Key build quirks

- **`static-link` feature** is default-on across all crates. `live2d-core-sys/build.rs` links `libLive2DCubismCore.a` on Linux. Dynamic linking (`--no-default-features`) links `dylib` and adds `-rpath` to the binary via `live2d-viewer/build.rs`.
- **Release profile** (in root `Cargo.toml`): `lto=true`, `strip=true`, `panic="abort"`, `codegen-units=1`. Always build with `--release` for any actual use — debug is slow.
- **V2 core** depends on C++ build artifacts from `live2d-py`. The `live2d-v2-core-sys/build.rs` links `v2_c_api`, `V2`, and `glad` as static libs. On Linux it also links `GL`, `stdc++fs`, `stdc++`, `m`.
- **`live2d-core-sys/build.rs` fallback path** assumes Cubism SDK at `../CubismSdkForNative-5-r.5` relative to workspace root. Override with `LIVE2D_SDK_ROOT`.
- **bindgen** runs at build time for both `-sys` crates. Generated code lands in `OUT_DIR/bindings.rs`.

## Wayland caveat

`main.rs` detects `WAYLAND_DISPLAY` and forces `WINIT_UNIX_BACKEND=x11` + `GDK_BACKEND=x11`. The tray icon uses GTK (tray-icon crate with `gtk` feature), which also needs X11. On Wayland-only compositors, GTK init may fail → tray icon is disabled (graceful fallback). Minimize-to-floating-circle uses a different code path on X11 (hide) vs Wayland (resize to 50x50 float overlay).

## Architecture notes

- **`live2d-viewer` is the binary entrypoint**. `main.rs` sets up GL context, egui, event loop.
- **Motion system is self-implemented**, not Cubism Framework. Lives in `live2d-viewer/src/motion/`: custom JSON parser, curve evaluator, queue manager, eye blink, breath, look-at-cursor, expression, physics, pose.
- **V3 renderer** (`renderer/mod.rs`) uses cached uniform locations (queried once at init) for ~30 fewer driver calls/frame. Three shader programs: standard, mask (FBO), masked (composite).
- **V2 rendering** uses a separate GL 2.1-style path (requires a VAO wrapper for core profile). V2 code resets GL state (VAO, program, texture, blend) after drawing because V2 internals leave GL in an unknown state.
- **Async model switching**: V3 model I/O (file reads) happens on a background thread via `mpsc` channel. `AppState::complete_pending_switch()` drains the receiver on the main thread.
- **CJK font**: egui loads `SourceHanSansCN-Medium.otf` from `/usr/share/fonts/adobe-source-han-sans/` at startup. Without this, Chinese UI labels render as `□□□` (tofu).
- **egui_glow coordinate workaround**: floating play button is a raw GL triangle, not an egui shape — avoids an egui_glow coordinate bug at small window sizes.
- **Model format detection**: `detect_model_format()` checks for `*.model3.json` (V3), else `*model*.json` (V2). V2 detection checks file names case-insensitively via `is_v2_model_json()`.

## Testing

All tests are integration-style and require the Cubism SDK:

- `live2d-core-sys/src/lib.rs`: unit tests for `csmGetVersion()`, `csmGetLatestMocVersion()`, log function roundtrip.
- `live2d-viewer/src/model_loader.rs`: model dump tests that load real SDK sample models (Mao, Rice, Natori). These are debug/exploration tests — they print model structure, they don't assert much.
- Tests bypass the viewer's async loading: they call `Moc::revive()`, `Model::initialize()`, `model.update()` inline.
- **Critical**: tests require SDK samples at `LIVE2D_SDK_ROOT/Samples/Resources/`. Run with: `LIVE2D_SDK_ROOT=/path/to/sdk cargo test --release`.

## Lint & format

- `cargo fmt` — no custom `rustfmt.toml`, use defaults.
- `cargo clippy` — no custom `clippy.toml`. Some `#[allow(clippy::...)]` annotations exist on specific items.
- `cargo check --release` before committing.
- Order: `fmt → clippy → test`.

## ~~Missing features (from todolist.txt)~~(OLD Fasioned doc,dismissed it)

| ~~Status~~ | ~~Feature~~ |
|---|---|
| ~~❌~~ | ~~Physics (hair/clothing swing)~~ |
| ~~❌~~ | ~~Layout (model canvas position/scale)~~ |
| ~~❌~~ | ~~Save/Load Parameters~~ |
| ~~❌~~ | ~~UserData (clickable areas)~~ |

## What's not here

- No CI workflows, no pre-commit hooks, no Makefile
- No `opencode.json` config
- No `rustfmt.toml` or `clippy.toml`
- No snapshot tests or test fixtures in-repo
