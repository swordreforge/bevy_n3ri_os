# live2d-rs

[![Rust](https://img.shields.io/badge/lang-Rust-orange?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![English](https://img.shields.io/badge/lang-EN-blue.svg)](README.md)

[Live2D Cubism SDK for Native](https://www.live2d.com/download/cubism-sdk/download-native/) 的 Rust 绑定与桌面查看器。

在一个应用中同时支持 **Cubism 5.x (V3)** 和 **Cubism 2.x (V2)** 模型。

## 工作空间结构

| Crate | 描述 |
|-------|------|
| `live2d-core-sys` | 由 `bindgen` 生成的 Cubism 5.x Core C API 原始 FFI 声明（`csmMoc`、`csmModel` 等） |
| `live2d-core` | V3 的安全 Rust 封装 — `Moc`、`Model`、`Parameters`、`Parts`、`Drawables`、`CanvasInfo`、`OffscreenInfos` |
| `live2d-v2-core-sys` | Cubism 2.x C API 的 `bindgen` FFI（从 `live2d-py` C++ 编译产物生成） |
| `live2d-v2-core` | V2 的安全 Rust 封装 — 模型加载、参数控制、渲染 |
| `live2d-viewer` | 桌面查看器二进制：`winit` + `glutin` + `glow`（OpenGL 3.3 Core）+ `egui` 界面 + 自实现动作系统 |

`live2d-py/` 是一个独立的 git 子模块（不在工作空间中）——其编译的 C++ 产物供 `live2d-v2-core-sys` 使用。

## 快速开始

### 前置条件

- **Rust 1.75+**（MSRV，在每个 crate 中设置）
- **Cubism 5.x SDK for Native** — 放在工作空间根目录下，名为 `CubismSdkForNative-5-r.5/`，或设置 `LIVE2D_SDK_ROOT` 环境变量指向其他位置
- **支持 OpenGL 3.3 Core** 的 GPU（运行时，查看器需要）

### 设置

```bash
# 1. 从 Live2D 下载 Cubism SDK for Native：
#    https://www.live2d.com/download/cubism-sdk/download-native/
#    解压到工作空间根目录下，名为 CubismSdkForNative-5-r.5/

# 2. 用模型目录构建和运行：
cargo build --release
cargo run --release -p live2d-viewer -- /path/to/model-directory
```

构建系统默认从 SDK **静态链接** `libLive2DCubismCore.a`。参见[链接方式](#链接方式)了解其他选项。

### 运行示例模型

示例模型位于 Cubism SDK 的 `Samples/Resources/` 目录下：

```bash
cargo run --release -p live2d-viewer -- \
    /path/to/CubismSdkForNative-5-r.5/Samples/Resources/Haru
```

### 使用 LIVE2D_SDK_ROOT 自动扫描

设置了 `LIVE2D_SDK_ROOT` 且未在命令行中指定模型路径时，查看器会自动扫描 `$LIVE2D_SDK_ROOT/Samples/Resources/` 下的所有 V3 和 V2 模型：

```bash
LIVE2D_SDK_ROOT=/path/to/CubismSdkForNative-5-r.5 cargo run --release -p live2d-viewer
```

### 叠加层模式

一个位于屏幕右下角的小型置顶窗口：

```bash
cargo run --release -p live2d-viewer -- --overlay /path/to/model-directory
```

## 命令

```sh
# 默认构建（静态链接）
cargo build --release

# 动态链接（运行时加载 libLive2DCubismCore.so）
cargo build --release --no-default-features

# 运行查看器
cargo run --release -p live2d-viewer -- /path/to/model-dir

# 叠加层模式（小型窗口，右下角）
cargo run --release -p live2d-viewer -- --overlay /path/to/model-dir

# 测试（需要 LIVE2D_SDK_ROOT/Samples/Resources 中的 SDK 示例）
LIVE2D_SDK_ROOT=/path/to/CubismSdkForNative-5-r.5 cargo test --release -p live2d-core-sys
```

### V2 模型支持（可选）

V2 模型需要先从 `live2d-py` 子模块构建 C++ 包装层：

```bash
# 编译 live2d-py（参见其自身的构建说明）
cd live2d-py && mkdir build && cd build && cmake .. && make

# 设置编译输出目录并运行
V2_PY_BUILD_DIR=/path/to/live2d-py/build cargo run --release -p live2d-viewer -- /path/to/v2-model-dir
```

查看器通过检查目录中是否存在 `*.model3.json`（V3）或 `*model*.json`（V2）**自动检测模型格式**。

## 查看器功能

### 模型管理

- **模型列表** — 选择、重命名（双击或点击 ✏ 按钮）、删除（点击 ✖ 按钮）、或通过文件选择器添加新的模型目录
- **模型历史** — 之前加载过的模型会持久化保存在 SQLite 数据库中（`~/.local/share/live2d-viewer/`），启动时自动恢复
- **扫描目录** — 配置要递归扫描的文件夹；通用系统名称（如工程代码名）会自动跳过
- **模型搜索** — 基于字符 n-gram 嵌入的语义相似度搜索，在后台线程执行，输入不会阻塞渲染
- **模型验证** — V3 模型文件在添加前会验证（MOC3 + 贴图是否存在），无效条目会被跳过
- **异步模型切换** — V3 模型文件通过 `mpsc` 通道在后台线程加载；主线程在不阻塞渲染的情况下接收结果

### 渲染（V3）

- **OpenGL 3.3 Core** 渲染器，使用 `glow` 类型安全封装
- **3 个着色器程序**：标准（无遮罩绘制项）、遮罩（FBO）、遮罩合成（逐像素遮罩合成）
- **缓存 uniform 位置** — 在着色器初始化时一次性查询，每帧减少约 30 次驱动调用
- **乘算/屏幕混合** 支持，通过 `uMultiplyColor` / `uScreenColor` uniform 实现
- **遮罩合成** — 遮罩形状渲染到离屏 FBO，然后在遮罩片段着色器中通过 `gl_FragCoord` / `uMaskSize` 采样

### 渲染（V2）

- 通过 C++ 包装层使用独立的 GL 2.1 风格渲染路径
- 需要 VAO 包装器以实现 OpenGL Core Profile 兼容
- V2 代码在绘制后会重置 GL 状态（VAO、程序、贴图、混合），因为 V2 内部会使 GL 处于未知状态

### 动作系统

动作系统是**独立实现的**（不依赖 Cubism Framework），位于 `live2d-viewer/src/motion/`：

- **自定义 JSON 解析器** — 无需官方框架即可解析 Cubism motion3.json 文件
- **曲线求值器** — 支持所有插值类型（线性、贝塞尔、阶梯）
- **队列管理器** — 按组管理队列，支持同时播放独立的动作（如待机 + 表情）
- **眨眼** — 自动眼睑闭合，可配置间隔和持续时间
- **呼吸** — 胸部/身体参数的细微呼吸动作
- **注视光标** — 眼睛平滑跟踪鼠标位置
- **表情** — 从 `expressions/*.exp3.json` 加载，支持淡入淡出过渡
- **物理** — physics3.json 物理模拟，弹簧动力学（头发、衣服、悬挂物等）
- **姿态** — 通过 pose3.json 组在点击动作之间自动淡入淡出
- **点击检测** — 点击模型身体触发随机"TapBody"动作（V3：从已加载动作中选取；V2：循环使用内部 C++ 动作）
- **音效** — V2 动作可通过 `rodio` 触发声音播放（OGG/WAV/MP3）

### 相机与布局

- **交互式平移与缩放** — 拖拽平移，滚轮或按钮缩放
- **重置** — 一键恢复默认视角
- **布局模式** — 进入调节模式，通过滑块精确控制 X/Y 偏移和缩放
- **布局持久化** — 每个模型的偏移和缩放独立存入数据库，切换时自动恢复（V2 保存 `v2_scale`；V3 通过相机正确计算 Y 翻转和宽高比）

### 宠物/叠加层模式

三种桌面宠物模式，可从 GUI 或系统托盘切换：

| 模式 | 描述 |
|---|---|
| **关闭** | 普通窗口查看器 |
| **窗口宠物** | 无边框置顶窗口，尺寸适配模型画布；类似桌面宠物 |
| **置顶宠物** | Linux/Wayland 上：创建独立的 `smithay-client-toolkit` layer-shell 表面（绕过窗口管理器）。X11 上：与窗口宠物类似，但支持圆形悬浮图标最小化 |

其他宠物功能：
- **圆形悬浮图标最小化** — 最小化为一个可拖拽的小圆形悬浮图标（Wayland 上保存为 50×50）
- **宠物工具栏** — 原始 GL 叠加层，包含按钮（上一个/下一个模型、放大、缩小、重置相机、搜索、退出宠物）
- **穿透模式** — 输入穿透，点击可以直接穿透到下面的窗口
- **托盘图标** — 系统托盘菜单，可切换宠物模式、切换穿透模式、显示/隐藏窗口或退出

### GUI（egui）

查看器使用 `egui` 实现所有界面叠加层：

- **模型列表**面板 — 选择、重命名、删除、添加模型目录
- **参数**面板 — 所有模型参数的滑块，实时更新
- **搜索**面板 — 全文模型搜索，带有相似度评分
- **设置**面板 — 配置扫描目录、查看扫描结果
- **动作状态** — 显示当前动作队列条目、表情状态、淡入淡出权重
- **操作按钮** — 重放待机动作、全部停止、点击身体
- **缩放控制** — 放大、缩小、重置按钮
- **宠物模式按钮** — 切换窗口宠物 / 置顶宠物
- **加载指示器** — 异步模型切换时显示
- **CJK 字体支持** — 启动时加载 `SourceHanSansCN-Medium.otf`；没有该字体时中文/日文标签会显示为 `□□□`

### 其他功能

- **系统托盘** — 跨平台托盘图标（Linux 上使用 GTK 通过 `ksni`，其他平台使用 `tray-icon`），带有完整菜单
- **Wayland 处理** — 自动检测 Wayland，强制使用 X11 后端；仅 Wayland 合成器上托盘会优雅降级；宠物模式会使用适当标志重新启动进程
- **浮动播放按钮** — 原始 GL 三角形覆盖层，避免了小窗口尺寸下 `egui_glow` 的坐标错误
- **TextRenderer** — 基于图集的等宽字体文本渲染器，用于宠物叠加层（原始 GL，非 egui）
- **数据库** — 通过 `libsql` 使用 SQLite 保存模型历史和设置

## 架构

```
┌─────────────────────────────────────────────┐
│              live2d-viewer                   │  桌面查看器二进制
│  ┌──────────┐ ┌──────────┐ ┌─────────────┐  │
│  │ 渲染器   │ │ GUI      │ │ 动作系统    │  │
│  │ (glow)   │ │ (egui)   │ │ 眨眼/呼吸   │  │
│  │ V3/V2    │ │ 宠物工具 │ │ 注视/物理   │  │
│  │ 着色器   │ │ 栏       │ │             │  │
│  └──────────┘ └──────────┘ └─────────────┘  │
│  ┌──────────┐ ┌──────────┐ ┌─────────────┐  │
│  │ 相机     │ │ 数据库   │ │ 宠物/叠加   │  │
│  │ (缩放)   │ │ (SQLite) │ │ (Wayland)   │  │
│  └──────────┘ └──────────┘ └─────────────┘  │
├─────────────────────────────────────────────┤
│   live2d-core  （安全的 V3 封装）            │
│   Moc / Model / Parameters / Drawables       │
├─────────────────────────────────────────────┤
│   live2d-v2-core  （安全的 V2 封装）         │
├──────────────────┬──────────────────────────┤
│ live2d-core-sys  │ live2d-v2-core-sys       │
│ (bindgen V3 FFI) │ (bindgen V2 FFI)         │
├──────────────────┴──────────────────────────┤
│   Cubism Core 5.x（C 静态库 / .so）          │
│   live2d-py（V2 C++ 包装层，子模块）          │
└─────────────────────────────────────────────┘
```

## Crate 详情

### `live2d-core-sys` (`live2d_core_sys`)

通过 `bindgen` 自动生成的 FFI 声明。构建时从 `CubismCore/include/Live2DCubismCore.h` 生成。

关键类型：`csmMoc`、`csmModel`、`csmVector2` 等。

### `live2d-core` (`live2d_core`)

V3 模型的安全零成本封装，带有生命周期管理：

```rust
let moc = Moc::from_bytes(&moc3_data)?;       // 加载 MOC3 数据
let mut model = Model::initialize(&moc)?;     // 创建模型实例

let canvas = model.canvas_info();              // 像素尺寸、原点
let params = model.parameters();              // 参数 ID、值、范围
let parts = model.parts();                    // 部件不透明度
let drawables = model.drawables();            // 顶点/索引数据、贴图、遮罩
let orders = model.render_orders();           // 绘制顺序

model.update();                                // 运行物理/动作
```

### `live2d-v2-core` (`live2d_v2_core`)

V2 模型的安全 Rust 封装，包装了来自 `live2d-py` 的 C++ API：
- 通过 `Model::from_file()` / `Model::from_bytes()` 加载模型
- 参数读取/设置，缩放和偏移控制
- 动作触发和音效路径查找

### `live2d-viewer` (`live2d_viewer`)

完整的桌面应用程序。参见上方的[查看器功能](#查看器功能)。

## 着色器（V3）

查看器使用 3 个 GLSL 330 Core 着色器：

| 程序 | Uniform | 用途 |
|------|---------|------|
| `program` | uTexture, uMultiplyColor, uScreenColor, uOpacity | 标准无遮罩绘制项 |
| `mask_program` | uScale, uTranslate, uOpacity | 将遮罩形状渲染到 FBO |
| `masked_program` | +uMaskTexture, uMaskSize | 通过 `gl_FragCoord` 进行逐像素遮罩合成 |

遮罩渲染流程：
1. 绑定离屏 FBO，绘制遮罩几何体 → 遮罩 alpha 存入 FBO 贴图
2. 切换到遮罩合成程序，将 FBO 贴图绑定为 `uMaskTexture`
3. 在片段着色器中通过 `gl_FragCoord` / `uMaskSize` 计算 UV 采样遮罩
4. 将片段 alpha 乘以遮罩值

## 链接方式

默认构建使用 **`static-link`** 特性（通过所有 crate 的 `Cargo.toml` 中的 `default = ["static-link"]` 启用）：

- **静态链接**（默认）：构建时链接 SDK 中的 `libLive2DCubismCore.a`
- **动态链接**（`--no-default-features`）：链接 `libLive2DCubismCore.so` 并通过 `live2d-viewer/build.rs` 向二进制添加 `-rpath`

SDK 的位置：
1. 工作空间根目录下的 `../CubismSdkForNative-5-r.5`
2. 通过环境变量 `LIVE2D_SDK_ROOT` 覆盖路径

## 构建注意事项

- **Release profile**（根目录 `Cargo.toml` 中）：`lto=true`、`strip=true`、`panic="abort"`、`codegen-units=1`、`overflow-checks=false`。实际使用时务必使用 `--release` 构建 —— debug 模式非常慢。
- **V2 core** 依赖于 `live2d-py` 的 C++ 编译产物。`live2d-v2-core-sys/build.rs` 会链接 `v2_c_api`、`V2` 和 `glad` 静态库。在 Linux 上还会链接 `GL`、`stdc++fs`、`stdc++`、`m`。
- **`bindgen`** 在两个 `-sys` crate 构建时运行。生成的代码位于 `OUT_DIR/bindings.rs`。

## Wayland 注意事项

`main.rs` 会检测 `WAYLAND_DISPLAY` 并强制设置 `WINIT_UNIX_BACKEND=x11` + `GDK_BACKEND=x11`。托盘图标使用 GTK（`ksni` crate），这也需要 X11。在仅 Wayland 的合成器上，GTK 初始化可能失败 → 托盘图标会被禁用（优雅降级）。

最小化到圆形悬浮图标在 X11（隐藏窗口）和 Wayland（调整窗口大小为 50×50 浮动叠加层）上使用不同的代码路径。Wayland 上的置顶宠物模式会创建一个独立的 `smithay-client-toolkit` layer-shell 线程，而不是依赖 winit。

## 测试

所有测试都是集成风格，需要 Cubism SDK：

```sh
LIVE2D_SDK_ROOT=/path/to/CubismSdkForNative-5-r.5 cargo test --release -p live2d-core-sys
```

- `live2d-core-sys/src/lib.rs`：`csmGetVersion()`、`csmGetLatestMocVersion()`、日志函数往返测试
- `live2d-viewer/src/model_loader.rs`：加载实际 SDK 示例模型（Mao、Rice、Natori）的模型转储测试 —— 这些是调试/探索性测试，会打印模型结构但不做过多断言

## 代码格式与 lint

```sh
cargo fmt
cargo clippy
cargo check --release
```

顺序：`fmt → clippy → test`。没有自定义 `rustfmt.toml` 或 `clippy.toml`。某些特定项上存在 `#[allow(clippy::...)]` 注解。

## 缺失功能 / TODO

| 状态 | 功能 |
|------|------|
| ❌ | **AI 聊天助手**（已规划 —— 参见 `docs/superpowers/`） |
| (✅) | HitAreas + 点击触发动作 — 从 `model3.json` HitAreas（V3）和 C++ hitTest（V2）实现碰撞检测；点击触发身体动作。**已实现。** |
| ✅ | UserData（`userdata3.json`）— 解析为 ID→描述查找表；点击时在 GUI 中显示 |
| ✅ | 参数预设保存/加载 — 命名快照存入数据库，按模型隔离；GUI 一键恢复 |
| ✅ | 布局设置 — 每个模型的偏移和缩放独立保存至数据库；布局模式滑块精确调节 |

## 许可协议

本项目与 Live2D Inc. 无关。

- `live2d-core-sys` 包含 Cubism Core SDK 的 FFI 声明，使用 Live2D 的许可条款。
- 所有其他代码以 [MIT 许可证](LICENSE) 提供。
