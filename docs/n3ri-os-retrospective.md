# 用 Bevy 0.19 从零手搓一个伪操作系统：n3ri_os 复盘

> **仓库源地址**：<https://github.com/swordreforge/bevy_n3ri_os>
> 协议：MIT · 平台：Linux + Wayland（最佳体验 niri）
> 技术栈：Rust · Bevy 0.19 · Servo · wgpu · mocari(Live2D)

作为一个写了多年 Rust、又长期围观 Bevy 生态的博主，我见过太多"三天热情"式的
side project：一个旋转的三角形、一两个 shader demo，然后就没有然后了。所以当我
第一次翻到 **n3ri_os** 这个仓库时，是有那么一点意外的——它是一个**从零复刻网页版
虚拟 OS（[os.inori.ai](https://os.inori.ai)）的 Bevy 0.19 桌面环境**，而且真的把
终端、浏览器、文件系统、LLM 对话、Live2D 桌宠这些硬骨头都啃了下来。

这篇文章不是软文，是一份尽量诚实的工程复盘：它做对了什么、踩了哪些坑、哪些坑值得
你提前绕开。代码都在上面的仓库里，欢迎对照阅读。

---

## 一、先上数据：这个项目到底有多大

| 指标 | 数值 |
|---|---|
| 语言 / 引擎 | Rust（edition 2021 harness）/ Bevy 0.19.1 |
| Rust 源码 | 65 个 `.rs` 文件，**34,783 行** |
| Workspace 成员 | 5 个 crate + 1 个二进制入口 |
| 提交数 | 81（2026-08-31 ~ 2026-09-10） |
| 依赖树 | `Cargo.lock` 锁定 **1193** 个包 |
| 授权 | MIT |

Workspace 结构（`Cargo.toml`）：

```toml
members = [
    "crates/n3ri-core",    # 状态机、事件、配置 —— 无渲染依赖
    "crates/n3ri-llm",     # OpenAI 兼容 LLM 客户端（无 Bevy）
    "crates/n3ri-agent",   # 主动式 agent：调度 / 记忆 / tool-loop
    "crates/n3ri-ui",      # 全部 UI：dock、topbar、窗口、15 个应用、shader
    "crates/n3ri-live2d",  # Live2D 桌宠（纯 Rust 运行时）
    "examples/minimal",    # 唯一可运行二进制（窗口 / 壁纸 / 卫星三模式）
]
```

> 值得注意：`n3ri-render`、`n3ri-audio`、`n3ri-apps` 三个早期规划的 crate 被**注释掉**
> 了。作者没有为了"架构好看"硬塞三个空 crate，而是把渲染并进 `n3ri-ui`、把应用放成
> `n3ri-ui::apps::*` 模块。**知道什么时候删掉自己画的架构图**，是很多 Rust 项目缺的
> 克制。

81 个提交、34k 行、11 天，平均每天 7+ 次提交。看提交前缀分布（`feat` 23 / `fix` 22 /
`perf` 13 / `chore` 8），你会发现 `fix` 几乎和 `feat` 一样多——这不是坏事，反而说明
作者在**高频地跑起来、看到问题、立刻修**，而不是闭门造车攒一波大的。

---

## 二、技术选型：三个"不妥协"

这个项目最值得聊的，是它在几个关键路口没有走"看起来更省事"的那条路。

### 1. 浏览器：为什么是 Servo + CPU readback，而不是 bevy_wry/bevy_cef

dock 里的「浏览器」是一个**真·网页浏览器**。实现路径（`crates/n3ri-ui/src/apps/browser.rs`）：

```text
Servo / surfman / GL（主世界，!Send）
  → read_full_frame() 读回 RGBA Vec<u8>
  → ExtractSchedule 跨到 render world
  → Render::after(PrepareAssets).before(Queue)
  → queue.write_texture(GpuImage)
```

里面有几个细节，做过 GPU 互操作的人会会心一笑：

- **纹理句柄必须稳定**：用 `Image::new_uninit(...)` + `RenderAssetUsages::RENDER_WORLD`
  创建占位纹理，`Handle<Image>` 永不重建；缩放时**只改 `texture_descriptor.size`**，
  靠 `AssetEvent::Modified` 触发 render world 重建 `GpuImage` 和刷新 bind group。
- **`bytes_per_row = 4 * w` 在这里合法**：256 字节对齐的规则只约束
  `copy_texture_to_buffer`，不约束 `write_texture`。这个坑我在别处见人写过补丁。
- **`BrowserHost` 是 `NonSend`**（`Rc<RefCell>`，surfman 的 Device/Context 都是
  `!Send/!Sync`），只能 `insert_non_send` 访问，**绝不能塞进 `Extract`**。
- **页面坐标不乘 scale**：`page_device_size` 取 `ComputedNode.size()`，`hidpi = 1.0`。
  AGENTS.md 里明确记着"曾致页面点击/滚动全失效"——又是一个 logical/physical 坐标的
  血案（见第五节）。

作者在文档里明确写了"**不要改回 bevy_wry / bevy_cef**，也不要换共享纹理导入"。理由很
工程化：同一 Vulkan 上共享纹理理论可行，但 render world 的线程隔离让 handle 导入变得
复杂，收益却很低。这是一个**算过账的妥协**，不是偷懒。

### 2. Live2D：从 FFI 换成纯 Rust（mocari）

这是一个我特别欣赏的决策。项目早期用 FFI 绑定官方 Cubism Native SDK，后来整体切到了
[`mocari`](https://github.com/Eatgrapes/Mocari)——一个 `#![forbid(unsafe_code)]` 的纯
Rust Live2D 运行时。相关提交：

```text
fb1470b  feat(live2d): switch runtime from live2d-rs FFI to mocari 0.4.0 pure-Rust
2d0aaad  fix(live2d): multiplicative alpha keep dst (shadow no longer punches holes)
0392202  merge: mocari live2d runtime + multiplicative alpha fix
17e12ed  chore(live2d): remove FFI runtime dirs, docs follow mocari
```

切过去之后，项目里少了一大坨 `unsafe` 和 native 构建链。文档里有一句很实在的话：
"支持纯 Rust 运行时（`#![forbid(unsafe_code)]`），无需官方 Native SDK、无 FFI。"

**在 Rust 生态里，能去掉一个 native 依赖 / 一段 FFI，通常比多写 200 行代码更值。**
这个选择直接降低了整个仓库的可构建性和可审计性。

### 3. 双模式：窗口 + 真·壁纸（layer-shell）

`examples/minimal` 是**单二进制三模式**：

```bash
cargo run -p n3ri-minimal                  # 窗口模式（winit）
cargo run -p n3ri-minimal -- --wallpaper   # 壁纸模式（layer-shell 桌面层）
cargo run -p n3ri-minimal -- --satellite   # 全局指针卫星进程
```

壁纸模式不是"把窗口置底"那么简单，它有几个硬碰硬的子问题：

- **滚轮**：壁纸 surface 是 bevy 进程自己的 layer-shell surface，触摸板的
  `wl_pointer.axis` 由合成器直接发给 bevy 自己的 Wayland 连接，
  `xwayland-satellite` **永远看不到**。所以必须 vendor `bevy_live_wallpaper`，
  手动处理 `Dispatch<wl_pointer>` 的 `Axis` 事件——**不能改回 crates.io 版本**，
  否则真实滚动丢失。
- **焦点**：Bevy 内置 `ui_focus_system` 对 `Image` 相机的 target 直接跳过 Interaction。
  于是作者复刻了一份 `wallpaper_ui_focus_system`，`.after(UiSystem)` 覆盖其重置结果。
- **键盘/IME**：走 `text-input-v3` 双缓冲桥接，不是 winit 直通。
- **帧率**：**不能注册 `FramepacePlugin`**。reactive wait 和 render cleanup 的
  `spin_sleep` 是两个叠加节流器，混用会让 24fps 档实际跑到 ~18fps。

这些都不是看文档能查到的，是**读完 Bevy 和 Wayland 源码、动手验证**才写得出来的结论。

---

## 三、架构：一个 workspace 的"家规"

### 3.1 依赖方向是硬约束

```
n3ri-llm ──► 无 Bevy（只放 wire 类型 + 纯函数解析，可 cargo test）
n3ri-core ─► 无渲染依赖（状态机 / 事件 / 配置）
n3ri-agent ► 依赖 { core, llm }，不依赖 ui / live2d
n3ri-ui ───► 依赖 { core, llm, agent }（bridge + 表现层）
minimal ───► 组装一切
```

`docs/nori-agent-dev.md` 里把这条规矩写得非常死：**Agent 内的所有类型不得引用
`n3ri-ui` 的 `FocusedTitle/AppWindow/CursorPosition`**，需要的世界信息必须由 UI 侧的
薄 bridge 翻译成纯数据 `AgentWorldView` 再喂进去。这是**防止 crate 循环依赖**最有效的
做法——用数据边界而不是模块可见性来切。

### 3.2 Message 不是 Event

Bevy 0.19 把事件重命名为消息。这个项目里**零 `add_event` / `EventReader` / `EventWriter`**，
全部是：

```rust
app.add_message::<MyMsg>();
fn sys(mut r: MessageReader<MyMsg>) { for e in r.read() { … } }
```

这是跟着引擎走的正确姿势。很多项目升级 Bevy 时会卡在这类 API 重命名上，这里的文档
直接把差异整理成了表格（`Parent → ChildOf`、`ZIndex::Global(-1) → GlobalZIndex(-1)`、
`BorderColor(Color) → BorderColor::all()`……），省下大量试错。

### 3.3 CursorPosition / UiArea 契约

这是我见过对"多输入源"处理得比较干净的做法：**所有交互系统（window/resize/dock/
snap/scroll/desktop/focus）只读 `CursorPosition{ logical, physical, scale, active }` 和
`UiArea` 两个资源，禁止直接查询主窗**。

- 窗口模式：`sync_cursor_from_window`（`First` 调度）填这两个资源。
- 壁纸模式：`wallpaper_bridge` 接管**同一组资源**（两种模式绝不并存）。

好处是显而易见的：输入源（winit / layer-shell / 卫星进程）对上层完全透明。代价是
下面这个必须说清楚的坑。

---

## 四、性能工程：把 Live2D 帧率抠出 +30%

`docs/live2d-perf-recap.md` 是整个仓库里含金量最高的文档之一。它记录了一轮**教科书级
的性能排查**，方法比结论更值得学。

### 4.1 方法论：先数据，后动手，每轮一个假设

| 步骤 | 命令 | 用途 |
|---|---|---|
| 粗扫 | `perf top` / 无栈 report | 看 self 占比定方向 |
| 定凶 | `perf record -F … -g --call-graph dwarf,32768 -p $(pidof n3ri-minimal)` | 区分主世界写放大还是 render 线程流量 |
| DSO 确认 | `--sort dso,symbol` + `perf script \| grep` | 裸地址先认 DSO，stripped 驱动到 DSO 即止 |
| 符号解析 | `eu-addr2line -e /usr/lib/libc.so.6 <addr>` | libc 内部地址定性 |

**最重要的一条判定原则**：`self` 占比是线索，不是判决。必须看调用子树 + 旁证，否则
会被 DWARF 穿过 stripped 驱动的**展开伪影**误导——文档里那张"`FreeListAllocator` 被画成
`libvulkan_intel` 的调用者"的假调用边，就是很好的反例（驱动不可能调用 Rust 分配器）。

### 4.2 四轮优化

1. **写放大治理（17.6% → ~10%）**：`sync_live2d` 每帧对每个 drawable 无条件
   `materials.get_mut(mat_h)`。Bevy 的 `AssetMut::DerefMut` **一触即发 `Modified` 事件**，
   即使值没变，render world 也会每帧重建全部 BindGroup。改法：先 `get` 读比对，变化才
   `get_mut`；顶点直写 mesh 缓冲；`With<PetDrawable>` 收窄查询；`gate_pet_cameras` 按
   可见性翻 `Camera::is_active`（只在目标状态变化时写）。
2. **定凶 libc 与驱动 memset**：`0x176ed3` 经 `eu-addr2line` 定性为 `memset`；
   调用栈显示 `submit → maintain → drop EncoderInFlight → memset` 和
   `camera_driver → begin_encoding → memset`。结论：**驱动在创建/销毁 encoder、开启 pass
   时清零，随 pass 数线性涨——开关在应用侧，不在驱动侧**。
3. **mask 通道打包（25 pass → 7 pass）**：启动日志
   `live2d mask groups: 25 groups, mask sources: 27`——几乎每个被遮罩 drawable 独占
   一套全屏 RTT + 相机 + pass。改法：4 个 mask 组打包进一张 RTT 的 RGBA 四通道，
   `BlendKind::MaskFbo → MaskLane(u8)`，按 `lane % 4` 分管线，管线设 `write_mask`。
   文档里有一句特别重要的教训：**`write_mask` 是"限输出"，不是"搬值"**——曾想
   shader 按通道输出 `(c.a,1,1,1)`，这是错的，因为同单元四组串行画进同一张 RTT，全量
   输出会冲掉别组通道。
4. **tick 降频（帧率 +30%）**：主线程 `tick_pet → csmiUpdateModel` 4.2%（闭源 Core，
   单次 cost 改不动）。新增 `PetTickState` + `PET_TICK_INTERVAL = 1/30`，30Hz 推进一次，
   **累积量一次性推进，总量 == 逐帧之和，动作速度不变**；`sync_live2d` 用
   `pet_tick_done` 同拍门控，只在 tick 帧上传。交互系统保持逐帧，快速表情切换最多半帧
   延迟，肉眼无感。

### 4.3 最终地板（明确不追）

作者很诚实地列出了"接受、不再追"的项：

- `FreeListAllocator` ~10%：每帧 CPU 蒙皮顶点上传刚需；
- render 线程单核架构：submit / maintain / allocator 全串行，拆不开；
- `renderer_extract` 4.1%：drawable 数量决定的 draw call 税。

**知道性能优化的"地板"在哪，比无脑压榨更重要。** 有些成本是架构决定的，继续抠只会
换来收益递减和代码复杂度上升。

---

## 五、那些"最贵的几课"

### 课 1：Bevy 0.19 有两套像素坐标（本次最贵）

`docs/terminal-hit-testing-lessons.md` 完整记录了终端选择功能的排查。根因是：

| API | 空间 |
|---|---|
| `Window::cursor_position()` | **逻辑**像素 |
| `Window::physical_cursor_position()` | **物理**像素 |
| `ComputedNode` / `UiGlobalTransform` / `TextLayoutInfo` | **物理** |
| `Node` 样式 `Val::Px` | **逻辑** |

混用**编译期不报错**，只在缩放 ≠ 100% 的屏幕上出错，且**误差随离窗口左上角的距离线性
放大**。开发机 `scale_factor = 1.0` 时完全无法复现——这是最阴险的一类 bug。

规则的最终形态：**与 `ComputedNode`/变换做几何运算，一律用 `physical_cursor_position()`；
与 `Node` 样式 `Val` 做运算，用 `cursor_position()`。** 项目把它集中进
`CursorPosition` 资源，从架构上堵死复发。

还有一个"误诊复盘"值得单拎出来：作者最初把症状归因为"系统执行顺序竞态"，那个假设其实
也自洽、修复也合理，但**没治好病**。真正锁死根因的是用户补充的关键鉴别症状——
"短输出时双击完全无法选中"：

- 竞态假设预测：短输出交错少，应基本正常 → **与事实矛盾** ✗
- 坐标错配假设预测：短输出集中在容器顶部，点击换算后落到容器外 → 全部无命中 ✓

**结论：提出假设后，用它推演每一个已知症状，尤其是最反直觉的那个。只能覆盖部分症状
的假设，无论多么自洽，都是错的或次要的。** 另外——验证 Bevy 行为不要凭记忆猜，直接
读 `~/.cargo/registry` 里对应版本的源码。这一条我在自己项目里也反复吃亏。

### 课 2：一个症状，三个断层（手势隔离）

`docs/window-gesture-isolation.md` 里，"移动窗口 A、窗口 B 被拉伸"这个症状背后是三层
独立缺陷：

1. **触发层**：`resize_start` 和 `window_drag_start` 监听同一 `just_pressed`，互不知晓。
   ECS 调度器只保证访问冲突时的串行，**不阻止多个系统响应同一帧同一按键**。修复：共享
   `IsDragging` 资源 check-then-set。
2. **隔离层**：最小化窗口只是 `Visibility::Hidden`，`Node` 坐标照常存在，边缘检测遍历
   所有 `AppWindow` 就命中了"幽灵边缘"。修复：查询加 `&Visibility` 过滤。
3. **清理层**：手势期间 insert 了 `CursorIcon`，`ResizeState` 重置不会回滚组件写入，
   光标卡死。修复：`resize_end` 显式 `remove::<CursorIcon>()`。

提炼出的三条原则值得刻进 DNA：**共享输入事件的系统必须显式互斥**；
**有 insert 必有配对的 remove 路径**；**查询即命中检测，过滤条件就是隔离边界**。

### 课 3：浮点转无符号的饱和陷阱

```rust
let lines_to_scroll = (wheel.y * 3.0).round() as usize;  // wheel.y < 0 时 = 0！
```

Rust 中负浮点 `as usize` **饱和为 0**（不回绕也不 panic），于是"向下滚"变成
`saturating_sub(0)` 恒零操作。凡可能为负的量，先落 `isize`/`i64` 再按符号分流。作者还
顺手点名了 clippy 的 `cast_possible_truncation` 应该在本项目启用——这是很地道的 Rust
工程习惯。

### 课 4：B0001 与"标记组件"架构

`docs/bevy-ecs-patterns.md` 把 Bevy 的组件访问冲突讲得很清楚。这个项目最终形成的模式是：

```
业务状态层 (AppVisible, IsDragging)   ← 标记组件，安全查询
        ↓  Commands
渲染状态层 (Visibility, Node)          ← Bevy 内部使用
```

即**业务标记 → `Commands` 延迟写 → 渲染组件**。这是 Bevy UI 项目里非常实用的一套
范式，避免了大量 B0001。

---

## 六、帧率上限 / 垂直同步 / MSAA：显示设置那点事

AGENTS.md 里对显示设置的解释，是我见过对 Bevy 帧治理讲得最细的实践记录之一：

- **帧率档位真源** = `config::FPS_TIER_HZ`，UI 文本由同一数组衍生，**禁止另建标签清单**
  （单一数据源）。
- **窗口模式**用 `bevy_framepace`，改 `FramepaceSettings.limiter` 即下一帧生效；
  `sync_fps_limiter` 用"值不同才写"而非 `is_changed` 门控，避免和插件自带系统同步竞态。
- **壁纸模式禁注册 `FramepacePlugin`**（前面已说，两套节流器叠加）。
- **VSync**：写主窗 `Window.present_mode`（`AutoVsync` / `AutoNoVsync`）；
  `bevy_render` 侦测 `present_mode_changed` 即重建 surface。
- **MSAA**：写带 `UiMainCamera` 标记相机的 `Msaa` 组件，per-camera、每帧重建附件、
  即时生效；启动期仍尊重 `N3RI_MSAA=0|1|2|4|8` 环境变量做 A/B。

这套东西没有一个是"看文档就会"的，全是**踩过坑后写下来的护栏**。

---

## 七、依赖管理里的三个"别删"

一个 1193 包的依赖树，能长期可构建，靠的是对补丁的**明确记录**：

1. **`[patch.crates-io] glslopt`**（根 `Cargo.toml`）：Servo/webrender 拉入的
   `glslopt 0.1.12` 自带 C11 threads 兼容头，其 `typedef pthread_once_t once_flag;` 与
   glibc 2.34+ 的 `<stdlib.h>` 冲突，在 Fedora 40+ / Ubuntu 24.04+ 上**编译 Servo 直接
   失败**。pinned 的 git rev 加了 `#ifndef` 保护。**勿删。**
2. **Vendored `vendor/bevy_live_wallpaper` 0.5.0**：上游从不发 `wl_pointer.axis`，本地
   patch 是真实滚轮的**唯一来源**。**勿切回 crates.io。**
3. **`wgpu` / `winit` 直依赖版本必须与 Bevy 0.19 统一**（`wgpu 29` / `winit 0.30`），
   因为纹理/设备类型跨 Bevy↔adapter 边界共享。文档里甚至记着 `Cargo.lock` 里
   **wgpu 出现 28/29/30 三个大版本共存**（28 = Bevy 内部，29 = 直依赖 + 壁纸，30 = Servo
   adapter）——这在生产项目里是个危险信号，但在这里是**跨生态嵌合不可避免的代价**，
   记录在案比藏着好。

---

## 八、Agent：一个"安静等待型"的主动式设计

`docs/nori-agent-dev.md` 是我在仓库里第二喜欢的文档（第一是性能复盘）。它实现了一个
**主动式 LLM 陪伴 agent**，但有个很有意思的产品判断：

> N.E.K.O 是"主动陪伴"产品，默认积极搭话；Nori 默认是**安静等待型**人格
> （世界观："等待用户是身份的一部分"），主动开口是例外不是常态。所以门控数字只抄
> 上限语义，默认阈值全部调保守。

具体到工程：

- **触发器只有 4 种**（Idle / Hourly / Break / Startup），默认 idle 阈值 30 分钟、
  日配额 1.0、整点报时默认关——全部进 `AgentConfig`，设置页可调。
- **门控是纯函数**：`gate(fire, snap, st, cfg) -> GateResult`，不碰 ECS，方便 `cargo test`。
  这是 N.E.K.O 决策逻辑的 Rust 翻版。
- **输入侧受限于渲染侧能力**（终端那课的延伸）：`niri_spawn` 有白名单，
  `spawn-sh` 整段禁用（shell 注入面太大），`close-window` v1 直接**不暴露**——
  用"不给"代替"确认框"。这是很懂安全的取舍。
- **失败降级矩阵**写得很细：LLM 不可达（主动轮）静默丢弃不弹错；记忆文件损坏当层视为
  空、不删文件；`scheduler.json` 损坏 → 配额重置为 0（**宁可多说一条，不可静默失忆**）。

---

## 九、诚实的不足

复盘不能只夸。这个项目也有明显的短板：

1. **音频系统缺席**：`n3ri-audio` 仍是注释状态。`assets/nori/audio` 里躺着 4 首 BGM 和
   ~90 个 SFX，大部分还没接。README 的功能清单里也没把它当卖点。
2. **平台单一**：只支持 Linux Wayland，最佳体验绑定 niri。X11 未测试。这是 layer-shell
   深度绑定的必然代价。
3. **终端 VT 是"逐行文本模型 + 全量剥离 ANSI"**：无颜色、无光标寻址、无区域重绘。
   作者自己写得很清楚：想让 zsh/p10k/fish 好看，得先实现真 VT 模拟（256 色 / 光标寻址 /
   差分重绘）。**输入侧协议能力不能超过输出侧解析能力**——这是那篇终端复盘里最漂亮的
   一条原则。
4. **国际象棋 ELO 待接**、部分应用是"伪造数据"的体验型应用。项目定位是复刻视觉与交互，
   不是真做生产力工具。
5. **`docs/virtual-fs-plan.md` 和 `nori-agent-plan.md` 仍带"规划中"标记**：规划和落地之间
   有 gap，这很正常，但也说明某些设计文档跑在了实现前面。

---

## 十、结语：一个 Rust 项目该有的样子

如果只用一句话总结 n3ri_os，我会说：**它不是"用 Rust 写了个 UI demo"，而是一个把
Bevy 当生产引擎用、并且认真记录了每一次翻车的工程样本。**

它值得你 clone 下来读的理由，不是那些花哨的 shader 或桌宠，而是这些：

- `docs/live2d-perf-recap.md`——一次完整的 perf 排查，含伪影识别和不追项的判断；
- `docs/terminal-hit-testing-lessons.md`——"假设必须解释全部症状"的活教材；
- `docs/window-gesture-isolation.md`——一个症状三层根因的分层排查；
- `docs/bevy-0.19-dev-knowledge.md`——630 行、几乎可以直接当 Bevy 0.19 中文实战手册；
- `AGENTS.md`——把"哪些能改、哪些千万别动"写得清清楚楚，这对任何接手者都是福音。

在 Rust 圈子里，我们常常争论"生态够不够成熟"。n3ri_os 的态度是：**不抱怨，直接下场
把 Bevy 0.19 + Servo + 纯 Rust Live2D + Wayland layer-shell 拼起来，然后把所有血泪写进
文档。** 这种工程诚实，比任何 benchmark 都更有说服力。

---

**仓库源地址（欢迎 star / 提 issue）**：

> **<https://github.com/swordreforge/bevy_n3ri_os>**
>
> 克隆：`git clone https://github.com/swordreforge/bevy_n3ri_os.git`
>
> 运行：`cargo run -p n3ri-minimal`（窗口）· `cargo run -p n3ri-minimal -- --wallpaper`（壁纸）

致谢仓库 README 中列出的所有开源作者：Bevy、wgpu-graft、bevy_tweening、bevy_woff、
portable-pty、chrono、reqwest、icu_provider、serde、mocari。

*本文基于仓库文档（README / AGENTS.md / PLAN.md / docs/*）与 git 历史撰写。若与代码
实现有出入，以仓库源码为准。*
