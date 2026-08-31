# n3ri\_os

[![Rust](https://img.shields.io/badge/lang-Rust-orange?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![Bevy](https://img.shields.io/badge/Bevy-0.19-blue)](https://bevyengine.org)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)

伪操作系统桌面环境，使用 Bevy 0.19 引擎从零搭建。复刻 [os.inori.ai](https://os.inori.ai) 的视觉风格与交互体验。

![total](total.png)

## 项目目的

本项目的核心动机是**学习 Bevy Shader 开发**以及**与底层 taffy 布局引擎的较量与妥协**。

Bevy 0.19 的 UI 系统构建在 taffy 之上，原生 Flexbox 布局在桌面级窗口管理场景下会遇到各种限制——窗口拖拽、吸附、缩放等交互需要在 taffy 的约束模型内寻找解法，同时 WGSL Shader 与 `UiMaterial` 的集成方式也与传统渲染管线有不少差异。这些正是本项目想要探索和记录的。

## 功能概览

### 系统层

- **启动流程** — Boot → Loading → Desktop 状态机，含进度条动画与加载环动画
- **顶栏** — 实时系统监控（CPU 使用率、电池状态、音量、网络）、时钟、电源菜单
- **Dock** — macOS 风格图标栏，距离感应放大效果，应用启动与运行指示器
- **窗口管理** — 拖拽移动、四向缩放、边缘吸附（Snap）、层级聚焦、滚动支持

### 桌面 Shader

深海青绿雾 + 近垂直宽光柱 + 蓝白萤火上浮 + 近摄 Bokeh + 底部 3D 透视网格地板。基于 `UiMaterialPlugin` 实现，鼠标位置实时响应。

### Live2D 桌面宠物

集成 `live2d-rs` 绑定（Cubism V3），支持：

- 桌面宠物显示与遮挡检测（窗口遮住宠物时自动切换为头部气泡显示）
- 表情切换（Happy、Sad、Angry、Surprised、Confused、Proud、Shy、Tired、Neutral）
- 与 Chat Capsule 情感输出联动

### Chat Capsule（对话胶囊）

内置 LLM 对话系统，支持 OpenAI 兼容 API：

- 输入框 IME 输入
- 句子逐字揭示动画
- 情感识别与 Live2D 表情联动

### 内置应用

| 应用 | 说明 |
| --- | --- |
| 终端 | 基于 `portable-pty` 的真实 PTY 终端 |
| 设置 | 主题、音量、亮度等系统设置 |
| 文件管理器 | 文件浏览 |
| 图片查看器 | 图片预览 |
| 国际象棋 | 完整棋盘与规则实现 |
| 你画我猜 | 绘画竞猜游戏 |
| 蛋糕对决 | 休闲对战 |
| 森林寻宝 | 探索寻宝 |
| 通讯 | 信号/消息应用 |
| 邮件 | 邮件客户端 |
| 算力 | 计算/性能展示 |
| 致谢 | 开源致谢页面 |
| 日志查看器 | 系统日志浏览 |
| 文本阅读器 | 纯文本阅读 |

![total](some_func.png)

![pasted-image](pasted-image.png)

## 架构

```
n3ri_os/
├── crates/
│   ├── n3ri-core/       # 状态机、事件、配置 — 无渲染依赖
│   ├── n3ri-ui/         # 所有 UI：Dock、Topbar、窗口管理、应用、Shader
│   ├── n3ri-llm/        # OpenAI 兼容 Chat Completion 客户端
│   ├── n3ri-live2d/     # Live2D 桌面宠物（基于 live2d-rs）
│   └── live2d-rs/       # Rust bindings（独立子项目）
├── examples/minimal/    # 可运行的二进制入口
└── assets/              # 字体、图标、纹理、Shader、音频
```

## 构建与运行

### 前置条件

- **Linux + Wayland**（推荐 niri 桌面环境，可获得最佳窗口位置对齐体验）
- **Rust 1.75+**
- **Live2D Cubism 5.x SDK** — 下载后放置于 `crates/CubismSdkForNative-5-r.5/`（如需 Live2D 功能）

### 编译运行

```bash
cargo run -p n3ri-minimal
```

嵌入资源模式（单二进制，无外部 assets 依赖）：

```bash
cargo run -p n3ri-minimal --features embed-assets
```

### 平台限制

> **仅支持 Linux Wayland。** 在 X11 下可能可以运行但未经测试。最佳体验在 [niri](https://github.com/niri-wm/niri) 桌面环境，其窗口位置对齐机制能与本项目的窗口吸附逻辑正确配合。

## 开源致谢

本项目使用了以下开源库，在此感谢各位作者：

| 库 | 作者/组织 | 用途 |
| --- | --- | --- |
| [Bevy](https://github.com/bevyengine/bevy) | bevyengine | 游戏引擎 / UI 框架 |
| [bevy\\_tweening](https://github.com/djeedai/bevy_tweening) | djeedai | 动画插值 |
| [bevy\\_woff](https://crates.io/crates/bevy_woff) | bevy 社区 | WOFF/WOFF2 字体加载 |
| [portable-pty](https://github.com/wrz/portable-pty) | wrz | 跨平台 PTY |
| [chrono](https://github.com/chronotope/chrono) | chronotope | 时间日期处理 |
| [reqwest](https://github.com/seanmonstar/reqwest) | seanmonstar | HTTP 客户端（LLM 通信） |
| [icu\\_provider](https://github.com/unicode-org/icu4x) | Unicode / ICU4X | 文本分段（中文支持） |
| [serde](https://github.com/serde-rs/serde) | dtolnay | 序列化框架 |
| [live2d-rs](https://github.com/swordreforge/live2d-rs) | swordreforge（本项目作者） | Live2D Cubism Rust 绑定 |

### 关于 Live2D 绑定

`live2d-rs` 是 Live2D Cubism SDK for Native 的 Rust 语言绑定，由本项目作者独立开发。它是对 Live2D 官方 C API 的 FFI 封装，**与 Live2D Inc. 官方无关**，不代表 Live2D Inc. 的立场或产品。Live2D、Cubism 是 Live2D Inc. 的注册商标。

## License

MIT License. 详见 [LICENSE](LICENSE)。
