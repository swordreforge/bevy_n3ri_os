# n3ri_os — Bevy 0.19 伪操作系统开发计划

## 项目概述

基于 Bevy 0.19 构建名为 **n3ri_os** 的伪操作系统，复刻 [os.inori.ai](https://os.inori.ai) 的视觉效果和交互体验。

### 目标平台
- 桌面端 (Windows/Linux/macOS)
- Web 端 (WASM) — 可选，通过 bevy_cef 或原生 WASM

### 参考生态
- Bevy 0.19
- bevy_cef 0.12.0 (CEF 149.3.0) — 如需嵌入网页内容

---

## 一、资产清单 (从 dump 中提取)

### 1.1 视觉资产
| 类别 | 文件 | 用途 |
|------|------|------|
| 纹理 | `ocean/dust.jpg`, `ocean/gradient-noise.jpg`, `ocean/water-normal.png` | 海洋着色器纹理 |
| 3D 模型 | `datasea/cosmicweb.min.glb` | 宇宙网 3D 模型 |
| 纹理 | `datasea/disp_height.png`, `datasea/nebula_color_live.png` | 星云/高度图 |
| CG | `datasea/cg-touch-hand.webp`, `datasea/cg-touch-her.webp` | 过场图片 |
| 图标 | `app-icons/*/icon-a.png`, `icon-b.png` | 应用图标 (12 组) |
| 图标 | `icon.png` | 系统图标 |

### 1.2 Live2D 模型
| 模型 | 路径 | 表情数 | 动作数 |
|------|------|--------|--------|
| Nori | `Nori_web/` | 23 | 14 |
| ARGNori | `ARGNori_web/` | 15 | 13 |

### 1.3 音频资产
| 类别 | 数量 | 示例 |
|------|------|------|
| BGM | 4 | `bgm1.m4a`, `bgm_void.m4a`, `bgm_memory.mp3`, `nori_daily_manifold.mp3` |
| SFX | ~90 | 按钮点击、菜单、通知、游戏音效等 |
| 冷启动音效 | 6 | `cold-open/*.m4a` |
| 数据海音效 | 8 | `datasea/*.m4a` |
| 卡牌游戏 | 7 | `cakeduel/*.wav` |
| 棋类游戏 | 10 | `chess/*.mp3` |

### 1.4 字体资产
| 字体 | 文件 | 用途 |
|------|------|------|
| Fusion Pixel 12px Mono SC | `fusion-pixel-12px-monospaced-sc.woff2` | 等宽像素字体 |
| Fusion Pixel 12px Proportional SC | `fusion-pixel-12px-proportional-sc.woff2` | 比例像素字体 |
| Press Start 2P | `press-start-2p-latin.woff2` | 复古游戏字体 |
| Sarasa Fixed SC | `sarasa-fixed-sc.woff2`, `sarasa-fixed-sc-bold.woff2` | 更纱黑体等宽 |
| Silkscreen | `silkscreen-latin.woff2`, `silkscreen-bold-latin.woff2` | 像素字体 |
| VT323 | `vt323-latin.woff2` | 终端字体 |
| LXGW WenKai | `lxgwwenkai-regular.woff2`, `lxgwwenkai-light.woff2` | 霞鹜文楷 |

---

## 二、系统架构

### 2.1 Workspace 结构

```
bevy_n3ri_os/
├── Cargo.toml                    # Workspace root
├── crates/
│   ├── n3ri-core/                # 核心: 状态机、事件、配置
│   ├── n3ri-render/              # 渲染: 着色器、粒子、网格、光束
│   ├── n3ri-ui/                  # UI: 桌面、Dock、顶栏、窗口
│   ├── n3ri-audio/               # 音频: BGM、SFX
│   ├── n3ri-live2d/              # Live2D 集成 (可选)
│   └── n3ri-apps/                # 应用: 文件管理器、消息、终端等
├── assets/                       # 游戏资产
│   └── nori/                     # 从 dump 复制的资产
└── examples/
    └── minimal/                  # 最小示例
```

### 2.2 模块依赖关系

```
n3ri-core (无依赖)
    ↓
n3ri-render (依赖 core)
n3ri-audio (依赖 core)
n3ri-live2d (依赖 core)
    ↓
n3ri-ui (依赖 core, render)
    ↓
n3ri-apps (依赖 core, ui, audio)
```

### 2.3 状态机设计

```rust
#[derive(States, Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum OsState {
    #[default]
    Boot,           // 启动动画
    Loading,        // 加载资源
    Desktop,        // 桌面环境
    App(String),    // 运行应用 (应用名)
    Shutdown,       // 关机动画
}

#[derive(SubStates, Clone, PartialEq, Eq, Hash, Default)]
#[source(OsState = OsState::Desktop)]
pub enum DesktopState {
    #[default]
    Normal,
    MenuOpen,       // 系统菜单打开
    Notification,   // 通知面板
}
```

---

## 三、按 Create 拆分的开发计划

### Phase 1: 项目脚手架 (n3ri-core)

**目标**: 建立基础框架、状态机、事件系统
**状态**: ✅ 已完成 (2026-08-24)

| 任务 | 描述 | 产出 |
|------|------|------|
| 1.1 | 初始化 workspace Cargo.toml | 可编译的空项目 |
| 1.2 | 创建 n3ri-core crate | OsState, DesktopState, 事件类型 |
| 1.3 | 定义系统配置资源 | WindowConfig, ThemeConfig |
| 1.4 | 实现基础 Runner | 窗口创建、图标设置 |

**关键文件**:
- `Cargo.toml` (workspace)
- `crates/n3ri-core/src/lib.rs`
- `crates/n3ri-core/src/state.rs`
- `crates/n3ri-core/src/events.rs`
- `crates/n3ri-core/src/config.rs`

#### ⚠️ 已知坑: Bevy 0.19 `bevy_ui_render`

当 `default-features = false` 时，`bevy_ui` 不会自动包含 UI 渲染管线。
必须在 workspace 级别显式添加 `bevy_ui_render` feature，否则 UI 节点能编译但屏幕上什么都不显示。

```toml
# workspace Cargo.toml — 必须这样写
bevy = { version = "0.19", default-features = false, features = ["bevy_ui_render"] }
```

**已完成产出**:
- `examples/minimal/` — Boot → Loading → Desktop 状态机，UI 文字可见
- n3ri-core 状态机 (OsState, BootState, LoadState) 运行正常

---

### Phase 1.5: Desktop Shell (桌面外壳)

**目标**: 构建完整的桌面 UI 骨架，包括顶栏、底部 Dock、窗口系统、右键菜单
**状态**: 📋 规划中 (2026-08-24)

#### 1.5a — 顶栏图标替换

| 任务 | 描述 | 产出 |
|------|------|------|
| 1.5a.1 | 加载 `assets/nori/icon.png` (591×591) 作为系统图标 | 图标资源 |
| 1.5a.2 | 替换顶栏 "n3ri_os" 文字为图标 + 小标题 | 顶栏左侧图标化 |
| 1.5a.3 | 顶栏右侧添加时钟、系统状态指示 | 顶栏功能完善 |

#### 1.5b — 底部 Dock (macOS 风格)

**Dock 规格**:
- 位置: 屏幕底部居中
- 图标来源: `assets/nori/app-icons/` (13 个应用)
- 图标尺寸: 基础 64px，放大时最大 96px
- 两态切换: `icon-a.png` (默认) → `icon-b.png` (悬停/激活)
- 运行指示器: 底部小蓝点 (直径 6px, 颜色 `#4FC3F7`)

**应用清单**:
| 应用 | 文件夹 | 有 icon-b |
|------|--------|-----------|
| browser | browser/ | ✓ |
| cakeduel | cakeduel/ | ✓ |
| chess | chess/ | ✓ |
| codenames | codenames/ | ✓ |
| credits | credits/ | ✗ (仅 icon-a) |
| files | files/ | ✓ |
| idle | idle/ | ✓ |
| mail | mail/ | ✓ |
| pictionary | pictionary/ | ✓ |
| preview | preview/ | ✓ |
| signal | signal/ | ✓ |
| terminal | terminal/ | ✓ |

**macOS 放大效果算法**:
```
鼠标距离图标中心: distance
放大范围: max_distance = 120px
基础缩放: base_scale = 1.0
最大缩放: max_scale = 1.5

scale = 1.0 + (max_scale - 1.0) * (1.0 - distance / max_distance)
如果 distance < max_distance，则应用缩放
否则 scale = 1.0
```

| 任务 | 描述 | 产出 |
|------|------|------|
| 1.5b.1 | 创建 `n3ri-ui` crate | UI 框架 |
| 1.5b.2 | 实现 Dock 容器 (水平布局, 居中) | Dock 骨架 |
| 1.5b.3 | 加载 13 个应用图标 | 图标显示 |
| 1.5b.4 | 实现鼠标悬停放大效果 | macOS 风格缩放 |
| 1.5b.5 | 实现 icon-a ↔ icon-b 两态切换 | 图标状态 |
| 1.5b.6 | 实现运行中蓝点指示器 | 应用状态 |

#### 1.5c — 窗口系统

**窗口规格**:
- 标题栏高度: 32px
- 标题栏左侧: 应用图标 + 标题文字
- 标题栏右侧: 最小化 (−)、最大化 (□)、关闭 (×) 按钮
- 窗口背景: 半透明深色 (`rgba(12, 18, 30, 0.95)`)
- 圆角: 8px

**窗口状态**:
| 状态 | 描述 | 行为 |
|------|------|------|
| Normal | 默认大小 | 可拖拽、可缩放 |
| Maximized | 铺满顶栏和 Dock 之间 | 标题栏保留，不可拖拽 |
| Minimized | 缩小到 Dock 图标 | 窗口隐藏，Dock 蓝点保留 |
| Closed | 关闭 | 销毁窗口实体 |

| 任务 | 描述 | 产出 |
|------|------|------|
| 1.5c.1 | 实现窗口实体 (Node + 标题栏 + 内容区) | 窗口组件 |
| 1.5c.2 | 标题栏按钮 (最小化/最大化/关闭) | 窗口控制 |
| 1.5c.3 | 窗口拖拽 (鼠标按住标题栏移动) | 交互 |
| 1.5c.4 | 窗口缩放 (鼠标拖拽边缘) | 交互 |
| 1.5c.5 | 最小化动画 (缩小到 Dock 位置) | 动画 |
| 1.5c.6 | 最大化逻辑 (铺满顶栏+Dock 之间) | 布局 |

#### 1.5d — 右键菜单

**菜单规格**:
- 触发: 桌面右键
- 背景: 半透明深色 (`rgba(20, 28, 42, 0.95)`)
- 圆角: 8px
- 阴影: 大模糊半径投影

**菜单项**:
| 项 | 动作 |
|----|------|
| 打开终端 | 启动 terminal 应用 |
| 打开文件 | 启动 files 应用 |
| 设置 | 启动 settings (预留) |
| ───── | 分隔线 |
| 关机 | 触发 Shutdown 状态 |

| 任务 | 描述 | 产出 |
|------|------|------|
| 1.5d.1 | 右键事件检测 | 输入处理 |
| 1.5d.2 | 菜单 UI 渲染 | 菜单显示 |
| 1.5d.3 | 菜单项点击处理 | 事件分发 |
| 1.5d.4 | 点击外部关闭菜单 | 交互 |

#### 1.5e — 窗口管理器

| 任务 | 描述 | 产出 |
|------|------|------|
| 1.5e.1 | 窗口 Z-order 管理 (点击置顶) | 层级控制 |
| 1.5e.2 | Dock 点击切换窗口显示/隐藏 | 快捷切换 |
| 1.5e.3 | 窗口焦点管理 | 输入路由 |

---

### Phase 2: 渲染引擎 (n3ri-render)

**目标**: 实现所有视觉特效

#### 2A: 蓝色伪 3D 底部网格

**着色器逻辑** (从 JS 提取):
```glsl
// 顶点着色器: 波浪变形
uniform float time;
varying vec2 vUv;

void main() {
    vUv = uv;
    vec3 pos = position;
    // 正弦波叠加
    pos.y += sin(pos.x * 2.0 + time) * 0.1;
    pos.y += sin(pos.z * 1.5 + time * 0.8) * 0.08;
    gl_Position = projectionMatrix * modelViewMatrix * vec4(pos, 1.0);
}

// 片段着色器: 网格线 + 距离衰减
uniform vec3 color;
varying vec2 vUv;

void main() {
    vec2 grid = fract(vUv * 20.0);
    float line = step(0.95, grid.x) + step(0.95, grid.y);
    float fade = 1.0 - smoothstep(0.3, 1.0, length(vUv - 0.5));
    float alpha = line * fade * 0.6;
    gl_FragColor = vec4(color, alpha);
}
```

| 任务 | 描述 |
|------|------|
| 2A.1 | 创建平面网格几何体 (可配置分段数) |
| 2A.2 | 编写波浪顶点着色器 |
| 2A.3 | 编写网格线片段着色器 |
| 2A.4 | 实时 uniform 更新 (time) |

#### 2B: 蓝色粒子系统

**着色器逻辑** (从 JS 提取):
```glsl
// 顶点着色器
attribute float aBornAt;
attribute float aLifespan;
attribute vec3 aVelocity;
attribute float aSize;
attribute float aSeed;
uniform float time;
uniform float baseSize;
varying float vAlpha;

void main() {
    float age = time - aBornAt;
    float t = age / aLifespan;
    if (aBornAt < 0.0 || t < 0.0 || t > 1.0) {
        vAlpha = 0.0;
        gl_Position = vec4(0.0, 0.0, -10000.0, 1.0);
        return;
    }
    vec3 pos = position + aVelocity * age;
    pos.x += sin(time * 1.3 + aSeed * 6.28) * 0.05;
    pos.y += sin(time * 0.9 + aSeed * 11.0) * 0.04;
    float fadeIn = smoothstep(0.0, 0.1, t);
    float fadeOut = 1.0 - smoothstep(0.55, 1.0, t);
    vAlpha = fadeIn * fadeOut;
    vec4 mv = modelViewMatrix * vec4(pos, 1.0);
    gl_PointSize = aSize * baseSize * 320.0 / -mv.z;
    gl_Position = projectionMatrix * mv;
}

// 片段着色器: 光晕粒子
uniform vec3 color;
varying float vAlpha;

void main() {
    vec2 d = gl_PointCoord - 0.5;
    float r = length(d);
    if (r > 0.5) discard;
    float core = exp(-r * r * 32.0);
    float halo = exp(-r * r * 8.0) * 0.55;
    float twinkle = 0.85 + 0.15 * sin(vSeed * 33.0);
    vec3 c = color * (core * 1.3 + halo * 0.7) * twinkle;
    gl_FragColor = vec4(c, (core + halo) * vAlpha);
}
```

| 任务 | 描述 |
|------|------|
| 2B.1 | 创建粒子管理器 (最大粒子数、生成率) |
| 2B.2 | 实现 CPU 端粒子更新 (位置、生命周期) |
| 2B.3 | 编写粒子着色器 (顶点 + 片段) |
| 2B.4 | 支持多种生成模式 (ambient, burst, anchor) |

#### 2C: 顶部光束 (Light Beams)

**着色器逻辑** (从 JS 提取):
```glsl
// 光束: 环形几何体 + 渐变透明
uniform vec3 color;
uniform float opacity;
uniform float intensity;
varying vec2 vUv;
varying vec3 vWorldNormal;

void main() {
    float y = vUv.y - 0.5;
    float core = exp(-y * y * 80.0);
    float halo = exp(-y * y * 10.0) * 0.3;
    float profile = core + halo;
    float rim = pow(1.0 - abs(vWorldNormal.z), 1.5) * 0.3;
    float a = clamp((profile + rim) * opacity * intensity, 0.0, 1.0);
    gl_FragColor = vec4(color, a);
}
```

| 任务 | 描述 |
|------|------|
| 2C.1 | 创建环形几何体 (TorusGeometry) |
| 2C.2 | 编写着色器 (核心 + 光晕 + 边缘) |
| 2C.3 | 实现旋转动画 |
| 2C.4 | 支持生命周期 (淡入/淡出) |

#### 2D: 故障方块 (Glitch Tiles)

| 任务 | 描述 |
|------|------|
| 2D.1 | 创建实例化方块几何体 |
| 2D.2 | 编写着色器 (空心/填充、闪烁) |
| 2D.3 | 实现集群生成模式 |

#### 2E: 冷启动动画 (Cold Open)

| 任务 | 描述 |
|------|------|
| 2E.1 | 创建嵌套椭圆环动画 |
| 2E.2 | 创建轨道光点动画 |
| 2E.3 | 创建流光条动画 |
| 2E.4 | 实现时序控制 (序列播放) |

**关键文件**:
- `crates/n3ri-render/src/lib.rs`
- `crates/n3ri-render/src/grid.rs` — 底部网格
- `crates/n3ri-render/src/particles.rs` — 粒子系统
- `crates/n3ri-render/src/light_beam.rs` — 光束
- `crates/n3ri-render/src/glitch_tiles.rs` — 故障方块
- `crates/n3ri-render/src/cold_open.rs` — 冷启动动画
- `assets/shaders/` — WGSL/GLSL 着色器文件

---

### Phase 3: UI 系统 (n3ri-ui)

**目标**: 实现桌面环境 UI

#### 3A: 启动/加载画面

| 任务 | 描述 |
|------|------|
| 3A.1 | 创建启动画面布局 (居中 logo + 进度条) |
| 3A.2 | 实现呼吸光晕动画 |
| 3A.3 | 实现旋转加载指示器 |
| 3A.4 | 模拟加载进度 (伪进度条) |

#### 3B: 顶栏 (TopBar)

| 任务 | 描述 |
|------|------|
| 3B.1 | 创建顶栏布局 (系统图标 | 应用名 | 指示器 | 时钟) |
| 3B.2 | 实现系统菜单下拉 |
| 3B.3 | 实现应用名显示 |
| 3B.4 | 实现状态指示器 (网络、音量、电池) |
| 3B.5 | 实现时钟显示 |

#### 3C: Dock 栏

| 任务 | 描述 |
|------|------|
| 3C.1 | 创建 Dock 布局 (底部居中) |
| 3C.2 | 实现图标悬停放大动画 |
| 3C.3 | 实现图标点击启动应用 |
| 3C.4 | 支持 12 个应用图标 |

#### 3D: 窗口管理

| 任务 | 描述 |
|------|------|
| 3D.1 | 创建窗口容器 (标题栏 + 内容区) |
| 3D.2 | 实现窗口拖拽 |
| 3D.3 | 实现窗口缩放 |
| 3D.4 | 实现窗口最小化/最大化/关闭 |
| 3D.5 | 实现窗口层级管理 (Z-order) |

#### 3E: 桌面背景

| 任务 | 描述 |
|------|------|
| 3E.1 | 集成 n3ri-render 的网格/粒子/光束作为背景 |
| 3E.2 | 实现背景与窗口的分层渲染 |

**关键文件**:
- `crates/n3ri-ui/src/lib.rs`
- `crates/n3ri-ui/src/boot.rs` — 启动画面
- `crates/n3ri-ui/src/topbar.rs` — 顶栏
- `crates/n3ri-ui/src/dock.rs` — Dock 栏
- `crates/n3ri-ui/src/window.rs` — 窗口管理
- `crates/n3ri-ui/src/desktop.rs` — 桌面背景

---

### Phase 4: 音频系统 (n3ri-audio)

**目标**: 实现背景音乐和音效

| 任务 | 描述 |
|------|------|
| 4.1 | 创建音频管理器 (BGM 通道、SFX 通道) |
| 4.2 | 实现 BGM 播放/停止/淡入淡出 |
| 4.3 | 实现 SFX 播放 (支持并发) |
| 4.4 | 实现音量控制 |
| 4.5 | 集成冷启动音效序列 |

**关键文件**:
- `crates/n3ri-audio/src/lib.rs`
- `crates/n3ri-audio/src/bgm.rs`
- `crates/n3ri-audio/src/sfx.rs`
- `crates/n3ri-audio/src/volume.rs`

---

### Phase 5: 应用系统 (n3ri-apps)

**目标**: 实现各应用功能

#### 5A: 文件管理器 (Files)

| 任务 | 描述 |
|------|------|
| 5A.1 | 创建虚拟文件系统 (VFS) |
| 5A.2 | 实现真实目录读取 |
| 5A.3 | 实现伪文件目录 (特殊文件) |
| 5A.4 | 创建文件浏览器 UI (树形/列表) |
| 5A.5 | 实现文件预览 |

#### 5B: 消息应用 (Messages)

| 任务 | 描述 |
|------|------|
| 5B.1 | 创建消息数据结构 |
| 5B.2 | 实现消息列表 UI |
| 5B.3 | 实现消息详情 UI |
| 5B.4 | 预设伪造消息内容 |

#### 5C: 终端 (Terminal)

| 任务 | 描述 |
|------|------|
| 5C.1 | 创建终端模拟器 |
| 5C.2 | 实现命令解析 |
| 5C.3 | 实现伪命令 (help, ls, cat, echo 等) |
| 5C.4 | 实现终端 UI (输入/输出滚动) |

#### 5D: 其他应用 (可选)

| 应用 | 描述 |
|------|------|
| Browser | 简单网页浏览器 (可嵌入 WebView) |
| Mail | 邮件客户端 (伪造) |
| Chess | 棋类游戏 |
| CakeDuel | 卡牌游戏 |
| Pictionary | 画画游戏 |
| Credits | 制作人员名单 |

**关键文件**:
- `crates/n3ri-apps/src/lib.rs`
- `crates/n3ri-apps/src/files/` — 文件管理器
- `crates/n3ri-apps/src/messages/` — 消息应用
- `crates/n3ri-apps/src/terminal/` — 终端
- `crates/n3ri-apps/src/vfs.rs` — 虚拟文件系统

---

### Phase 6: Live2D 集成 (n3ri-live2d)

**目标**: 集成 Live2D 角色 (可选，复杂度高)

| 任务 | 描述 |
|------|------|
| 6.1 | 调研 Bevy Live2D 方案 (bevy_live2d crate?) |
| 6.2 | 实现模型加载 (.moc3, .model3.json) |
| 6.3 | 实现表情系统 (23 + 15 个表情) |
| 6.4 | 实现动作系统 (14 + 13 个动作) |
| 6.5 | 集成到桌面右下角 |

**注意**: Live2D 在 Bevy 中没有成熟方案，可能需要:
- 使用 bevy_cef 嵌入网页版本
- 或使用 FFI 绑定 Live2D Cubism SDK
- 或用 2D 骨骼动画替代

**关键文件**:
- `crates/n3ri-live2d/src/lib.rs`
- `crates/n3ri-live2d/src/model.rs`
- `crates/n3ri-live2d/src/expressions.rs`
- `crates/n3ri-live2d/src/motions.rs`

---

## 四、着色器参考 (从 JS 提取)

### 4.1 粒子顶点着色器
```glsl
attribute float aBornAt;
attribute float aLifespan;
attribute vec3 aVelocity;
attribute float aSize;
attribute float aSeed;
uniform float time;
uniform float baseSize;
varying float vAlpha;
varying float vSeed;

void main() {
    float age = time - aBornAt;
    float t = age / aLifespan;
    if (aBornAt < 0.0 || t < 0.0 || t > 1.0) {
        vAlpha = 0.0;
        gl_Position = vec4(0.0, 0.0, -10000.0, 1.0);
        gl_PointSize = 0.0;
        return;
    }
    vec3 pos = position + aVelocity * age;
    pos.x += sin(time * 1.3 + aSeed * 6.28) * 0.05;
    pos.y += sin(time * 0.9 + aSeed * 11.0) * 0.04;
    float fadeIn = smoothstep(0.0, 0.1, t);
    float fadeOut = 1.0 - smoothstep(0.55, 1.0, t);
    vAlpha = fadeIn * fadeOut;
    vSeed = aSeed;
    vec4 mv = modelViewMatrix * vec4(pos, 1.0);
    gl_PointSize = aSize * baseSize * 320.0 / -mv.z;
    gl_Position = projectionMatrix * mv;
}
```

### 4.2 粒子片段着色器
```glsl
uniform vec3 color;
varying float vAlpha;
varying float vSeed;

void main() {
    if (vAlpha <= 0.001) discard;
    vec2 d = gl_PointCoord - 0.5;
    float r = length(d);
    if (r > 0.5) discard;
    float core = exp(-r * r * 32.0);
    float halo = exp(-r * r * 8.0) * 0.55;
    float twinkle = 0.85 + 0.15 * sin(vSeed * 33.0);
    vec3 c = color * (core * 1.3 + halo * 0.7) * twinkle;
    gl_FragColor = vec4(c, (core + halo) * vAlpha);
}
```

### 4.3 光束片段着色器
```glsl
uniform vec3 color;
uniform float opacity;
uniform float intensity;
varying vec2 vUv;
varying vec3 vWorldNormal;

void main() {
    float y = vUv.y - 0.5;
    float core = exp(-y * y * 80.0);
    float halo = exp(-y * y * 10.0) * 0.3;
    float profile = core + halo;
    float rim = pow(1.0 - abs(vWorldNormal.z), 1.5) * 0.3;
    float a = clamp((profile + rim) * opacity * intensity, 0.0, 1.0);
    gl_FragColor = vec4(color, a);
}
```

### 4.4 故障方块顶点着色器
```glsl
attribute vec3 aOffset;
attribute float aSize;
attribute float aHollow;
attribute float aBornAt;
attribute float aLifespan;
attribute float aSeed;
uniform float time;
varying vec2 vUv;
varying float vAlpha;
varying float vHollow;
varying float vSeed;

void main() {
    float age = time - aBornAt;
    float t = age / aLifespan;
    if (aBornAt < 0.0 || t < 0.0 || t > 1.0) {
        vAlpha = 0.0;
        gl_Position = vec4(0.0, 0.0, -10000.0, 1.0);
        return;
    }
    vec3 pos = position * aSize + aOffset;
    vUv = uv;
    float glitchRand = fract(sin(aSeed * 12.9898 + floor(time * 12.0)) * 43758.5453);
    float jitter = (glitchRand - 0.5) * 0.06 * step(0.92, glitchRand);
    pos.x += jitter;
    float fadeIn = smoothstep(0.0, 0.08, t);
    float fadeOut = 1.0 - smoothstep(0.6, 1.0, t);
    vAlpha = fadeIn * fadeOut;
    vHollow = aHollow;
    vSeed = aSeed;
    gl_Position = projectionMatrix * modelViewMatrix * vec4(pos, 1.0);
}
```

### 4.5 故障方块片段着色器
```glsl
uniform vec3 color;
uniform float glitchAmount;
uniform float time;
varying vec2 vUv;
varying float vAlpha;
varying float vHollow;
varying float vSeed;

void main() {
    if (vAlpha <= 0.001) discard;
    float border = 0.18;
    float onEdge = step(vUv.x, border) + step(1.0 - vUv.x, border)
                 + step(vUv.y, border) + step(1.0 - vUv.y, border);
    onEdge = clamp(onEdge, 0.0, 1.0);
    float fillMask = mix(1.0, onEdge, vHollow);
    float flickerSeed = floor(time * 14.0) + vSeed * 7.0;
    float flicker = fract(sin(flickerSeed) * 43758.5453);
    float flickerMute = step(1.0 - glitchAmount * 0.4, flicker);
    fillMask *= 1.0 - flickerMute;
    if (fillMask <= 0.001) discard;
    vec3 col = color * mix(0.55, 1.0, onEdge);
    gl_FragColor = vec4(col, vAlpha * fillMask);
}
```

---

## 五、应用图标映射

从 `app-icons/` 目录提取:

| 应用名 | icon-a (默认) | icon-b (激活) |
|--------|---------------|---------------|
| browser | ✅ | ✅ |
| cakeduel | ✅ | ✅ |
| chess | ✅ | ✅ |
| codenames | ✅ | ✅ |
| credits | ✅ | ❌ |
| files | ✅ | ✅ |
| idle | ✅ | ✅ |
| mail | ✅ | ✅ |
| pictionary | ✅ | ✅ |
| preview | ✅ | ✅ |
| signal | ✅ | ✅ |
| terminal | ✅ | ✅ |

---

## 六、开发优先级

### P0 — 核心体验 (必须)
1. 项目脚手架 + 状态机
2. 底部蓝色网格 (视觉核心)
3. 蓝色粒子系统 (视觉核心)
4. 顶部光束 (视觉核心)
5. 启动动画
6. 桌面环境 + 顶栏 + Dock
7. 窗口管理系统

### P1 — 应用功能
8. 文件管理器 (真实目录 + 伪文件)
9. 消息应用 (伪造消息)
10. 终端 (伪命令)

### P2 — 增强体验
11. 音频系统 (BGM + SFX)
12. 故障方块效果
13. 冷启动完整动画序列

### P3 — 可选扩展
14. Live2D 角色集成
15. 卡牌游戏 (CakeDuel)
16. 棋类游戏 (Chess)
17. 其他小游戏

---

## 七、技术决策待定

| 问题 | 选项 | 建议 |
|------|------|------|
| 着色器语言 | WGSL vs GLSL | WGSL (Bevy 原生) |
| UI 框架 | bevy_ui vs egui | bevy_ui (原生集成) |
| Live2D | FFI/嵌入/替代 | 先跳过，后期评估 |
| 文件系统 | 真实 + 伪 | VFS 抽象层 |
| 音频格式 | mp3/m4a/wav | 转换为 ogg (Bevy 原生) |

---

## 八、下一步行动

1. ~~**确认技术决策**: 着色器语言、UI 框架选择~~ ✅ 已决定: WGSL + bevy_ui
2. ~~**初始化项目**: 创建 workspace 和第一个 crate~~ ✅ 已完成
3. ~~**实现 Phase 1**: 核心状态机~~ ✅ 已完成 (Boot→Loading→Desktop 可见)
4. **实现 Phase 1.5**: Desktop Shell (顶栏图标 + Dock + 窗口系统 + 右键菜单)
5. **实现 Phase 2**: 渲染引擎 (网格/粒子/光束)
6. **逐步推进**: 按 Phase 顺序开发

---

*计划生成时间: 2026-08-24*
*最后更新: 2026-08-24 — Phase 1 已完成，bevy_ui_render 问题已解决*
*基于资产分析: /home/swordreforge/project/业余项目/bevy_n3ri_os/assets/nori/*
*参考项目: /home/swordreforge/project/业余项目/bevy_n3ri_os/example/bevy-vn-engine/*
