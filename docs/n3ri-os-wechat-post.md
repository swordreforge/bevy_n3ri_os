# 我用 Bevy 从零搓了个假的「操作系统」，然后把它开源了

> 项目仓库：**https://github.com/swordreforge/bevy_n3ri_os**
> Rust · Bevy 0.19 · Servo · wgpu · 纯 Rust Live2D
> 34,783 行代码，81 次提交，11 天

先放地址，免得你划到最后还得往回翻：

**https://github.com/swordreforge/bevy_n3ri_os**

---

## 起因：我为什么非要干这件事

事情是这样的。

我在网上看到一个网页版的虚拟操作系统 [os.inori.ai](https://os.inori.ai)，界面做得特别
舒服——深海配色的桌面、会呼吸的光柱、桌宠、终端、各种小应用。作为一个写了多年 Rust
的人，我脑子里冒出的第一个念头不是"好漂亮"，而是：

**"这玩意能不能用 Bevy 原生跑一遍？"**

网页能做的，Bevy 凭什么不能做？而且我想练的东西正好都在里面：WGSL shader、和 taffy
布局引擎的"搏斗"、把浏览器引擎塞进游戏引擎里——这些东西单拎出来每一个都够写一篇
博客，凑一起能搭成一个完整的桌面环境。

于是 n3ri_os 就开工了。

先说清楚定位：**这是个伪操作系统**。它不是要替代 GNOME/KDE，就是把一个网页版的虚拟
桌面，用 Rust + Bevy 0.19 从零复刻出来的一个"大玩具"。但我是个较真的人，玩具也要
按生产标准做。

---

## 一、先给你看看它现在长什么样

别听我吹，看数字：

- **34,783 行 Rust**，65 个源文件
- **5 个 crate** + 1 个二进制入口
- **81 次提交**，从 8 月 31 号干到 9 月 10 号
- `Cargo.lock` 里 **1193 个依赖包**

能跑的东西也不少：

- **真·终端**：基于 `portable-pty` 起一个真实的 `sh`，不是你画个框假装能打字
- **真·浏览器**：嵌了 Servo 引擎，能真上网那种
- **Live2D 桌宠**：纯 Rust 运行时，不是 FFI 绑定
- **顶栏**：读 `/proc/stat`、`/sys/class/power_supply`，是真系统监控
- **15 个应用**：文件管理器、设置、邮件、棋类、你画我猜……
- **两种模式**：普通窗口 + Wayland 桌面壁纸

够了吧？下面开始讲怎么做的，以及——我翻了多少车。

---

## 二、第一个硬骨头：把 Servo 塞进 Bevy

这是整个项目最刺激的部分。

dock 上那个「浏览器」图标，点开是个**真网页浏览器**。技术路线我选了 **Servo + CPU
readback**，简单说就是：

```text
Servo 在离屏 GL 里渲染网页
  → read_full_frame() 把画面读回 CPU（RGBA 字节数组）
  → 跨线程丢到 render world
  → queue.write_texture 上传成 Bevy 的纹理
  → 贴到 UI 上
```

为什么不用现成的 `bevy_wry` / `bevy_cef`？因为它俩要拖一整个 Chromium，依赖体积和
构建时间都爆炸。Servo 是 Rust 写的，原生契合，编译出来也干净。

但这条路全是坑，我挑几个印象最深的：

**坑一：纹理句柄必须稳定。** 我用 `Image::new_uninit()` 建了个占位纹理，这个
`Handle` 建好之后**一辈子不能变**。窗口缩放的时候怎么办？只改
`texture_descriptor.size`，让 Bevy 自己侦测到 `AssetEvent::Modified` 去重建 GPU
纹理和刷新 bind group。要是不这么做，注入那一步会尺寸对不上，然后**每帧静默跳过**——
画面就是黑的，还不报错。

**坑二：`BrowserHost` 是 `NonSend` 的。** surfman 的 Device/Context 都是
`!Send/!Sync`，所以这个资源只能 `insert_non_send` 访问，**绝对不能塞进
`Extract`**。主世界和 render world 的边界，在这里是硬的。

**坑三：页面坐标千万别乘 scale。** 这个坑当时让我页面点击和滚动**全部失效**，查了
半天。最后发现是我手贱乘了个 `scale_factor`。页面输入坐标本来就和 UI 渲染空间同一个
物理像素空间，乘了反而错。

我在文档里立了个规矩：**不要改回 bevy_wry/bevy_cef，也别想换成共享纹理导入。**
后者理论可行，但 render world 的线程隔离会让 handle 导入特别复杂，收益又低——**这
笔账我算过，不划算。**

---

## 三、第二个决定：把 Live2D 从 FFI 换成纯 Rust

这个决策我到现在都觉得是整项目最值的一步。

一开始 Live2D 桌宠是走 FFI 绑定官方 Cubism Native SDK 的。能跑，但心里一直不舒服：
一堆 native 构建配置、一堆 `unsafe`、跨平台还麻烦。

后来我发现了 [`mocari`](https://github.com/Eatgrapes/Mocari)——一个纯 Rust 的 Live2D
运行时，而且人家 `#![forbid(unsafe_code)]`。

我直接切过去了。相关提交：

```text
fb1470b  feat(live2d): switch runtime from live2d-rs FFI to mocari 0.4.0 pure-Rust
2d0aaad  fix(live2d): multiplicative alpha keep dst (shadow no longer punches holes)
17e12ed  chore(live2d): remove FFI runtime dirs, docs follow mocari
```

切完之后整个仓库清爽了一大截。**在 Rust 生态里，能干掉一个 native 依赖、砍掉一段
FFI，通常比多写 200 行代码值一百倍。** 这句话我送给所有正在纠结"要不要绑个 C 库"的
朋友。

---

## 四、高光时刻：一次 perf 抠出 30% 帧率

这部分是我最想拿出来讲的，因为方法比结果重要。

Live2D 桌宠一开始卡得要命，render 线程单核跑满。我没瞎改代码，先上 `perf`：

```bash
perf record -F ... -g --call-graph dwarf,32768 -p $(pidof n3ri-minimal)
perf report --stdio
```

然后**一轮只验证一个假设**，四轮干完：

**第一轮：写放大。** 我在 `sync_live2d` 里每帧对每个 drawable 都调
`materials.get_mut()`。要命的是，Bevy 的 `AssetMut::DerefMut` **一碰就触发
`Modified` 事件**——哪怕你什么都没改，render world 也会老老实实每帧重建全部
BindGroup。改成"先 `get` 读出来比对，变了才 `get_mut`"，一下子从 17.6% 掉到 ~10%。

**第二轮：定凶 libc。** 有个热点是 `libc.so.6` 里一个裸地址 `0x176ed3`，看着没法
下手。我用 `eu-addr2line` 一查——原来是 `memset`。再顺着调用栈看，是驱动在
**每创建一个 command encoder、每开一个 render pass** 时清零内存。结论很关键：
**这东西随 pass 数线性涨，开关在我手上，不在驱动手上。**

**第三轮：砍 pass，25 → 7。** 看启动日志才发现：
`live2d mask groups: 25 groups, mask sources: 27`。几乎每个被遮罩的部件都独占了一整套
全屏 RTT + 相机 + pass！我把它改成 4 个 mask 组打包进一张 RTT 的 RGBA 四个通道，
`write_mask` 分管线——**25 个全屏 pass 直接砍到 7 个**。

这里插一句血泪教训：我一度想让 shader 按通道输出 `(c.a,1,1,1)`，这是**错的**。因为
同一个单元的四组是**串行画进同一张 RTT** 的，全量输出会把别组的通道冲掉。正确的做法是
shader 原样输出，靠管线 `write_mask` 做通道路由——**`write_mask` 是"限输出"，不是
"搬值"**。想清楚这个语义，我省了一次 shader 返工。

**第四轮：降频。** 主线程 `csmiUpdateModel` 占 4.2%，这是闭源的 Core，单次成本改不
动。我就加了个 `PetTickState`，让它 **30Hz 推进一次**，累积量一次性推进——**总量等于
逐帧之和，所以动作速度完全不变**。交互系统还是逐帧跑，快速切换表情最多延迟半帧，肉眼
根本看不出来。

**最终：帧率 +30%。**

顺带说个方法论上的坑：`perf` 有时候会把调用图画错。我遇到过 `FreeListAllocator` 被
画成 `libvulkan_intel` 的"调用者"——**驱动怎么可能调用 Rust 分配器**？那是 DWARF 穿过
stripped 驱动时的展开伪影。所以：**self 占比是线索，不是判决**，一定要看调用子树 +
旁证。

---

## 五、翻车现场：我踩过的那些坑

这部分我写得特别细，因为我觉得**踩坑记录才是给后来人最值钱的东西**。

### 翻车一：Bevy 有两套像素坐标

这是全项目最贵的一课。

Bevy 0.19 里，`cursor_position()` 是**逻辑**像素，`physical_cursor_position()` 是
**物理**像素，而 `ComputedNode` 的几何是**物理**像素。你混着用，**编译期不报错**，
只在缩放 ≠ 100% 的屏幕上出错，而且**误差随距离线性放大**。

我开发机 `scale_factor = 1.0`，所以死活复现不出来。这个 bug 是在终端选择功能里炸的。

更值得说的是我**误诊过一次**。我一开始咬定是"系统执行顺序竞态"，那个假设也自洽、
改法也合理，但**没治好病**。真正让我锁定根因的，是用户补了一句关键症状：
"**短输出的时候双击根本选不中**"。

- 如果是竞态：短输出交错少，应该基本正常 → 和事实矛盾
- 如果是坐标错配：短输出集中在容器顶部，点击换算后落到容器外 → 全部无命中，完美解释

所以我总结出一条铁律，也送给你们：

> **提出一个假设后，用它去推演每一个已知症状，尤其是最反直觉的那个。
> 只能解释部分症状的假设，无论多自洽，都是错的或者是次要的。**

另外：**别凭记忆猜引擎行为，直接去 `~/.cargo/registry` 读对应版本的源码。** 这次我
读了两分钟 `focus.rs` 和 `ui_node.rs`，直接从"循环猜测"跳到"锤实结论"。

### 翻车二：一个症状，三层根因

有次用户报"移动窗口 A，窗口 B 的宽高被拉伸了"。我修了一层，症状变了，再修一层，
又变了——最后发现**一个症状背后有三个独立缺陷**：

1. **触发层**：拖拽和缩放两个系统监听同一个鼠标事件，互不知晓。Bevy 的 ECS 调度器只
   保证"访问冲突时串行"，**不阻止多个系统响应同一帧同一个按键**。
2. **隔离层**：最小化的窗口只是 `Visibility::Hidden`，`Node` 坐标还在，边缘检测一
   遍历就命中了"幽灵边缘"。
3. **清理层**：手势里 `insert` 了 `CursorIcon`，但 Resource 重置不会回滚组件写入，
   光标就卡死了。

三条原则刻进 DNA：

- **共享输入事件的系统必须显式互斥**
- **有 insert 就必有配对的 remove**
- **查询即命中检测，过滤条件就是隔离边界**

### 翻车三：负浮点 `as usize` 会饱和成 0

```rust
let lines_to_scroll = (wheel.y * 3.0).round() as usize;  // wheel.y < 0 时 = 0！
```

Rust 里负浮点转 `usize` **饱和为 0**，不回绕也不 panic。于是"向下滚"变成了
`saturating_sub(0)`，恒等于没动。凡是可能为负的量，先落 `isize` 再按符号分流。

---

## 六、我做对的一件事：写文档

项目里有一堆 `docs/*.md`，很多人可能觉得这是浪费时间。但我觉得这是这个项目最值钱的
资产——**代码会过时，踩坑的教训不会**。

比如 `docs/bevy-0.19-dev-knowledge.md`，630 行，我把它写成了一本 Bevy 0.19 实战
手册：版本锁定、feature 拓扑、ECS、UI、render world、输入、状态机、所有 gotcha。
新接手的人读一遍就能干活。

`AGENTS.md` 里我专门维护了一个"**哪些能改、哪些千万别动**"清单。比如：

- 根 `Cargo.toml` 里那个 `glslopt` patch **千万别删**——glibc 2.34+ 上编译 Servo
  全靠它，不然 Fedora 40+ / Ubuntu 24.04+ 直接编译失败
- `vendor/bevy_live_wallpaper` **绝对不能切回 crates.io**——上游从不发
  `wl_pointer.axis`，本地 patch 是真实滚轮的**唯一来源**
- 浏览器**必须留在 Servo + CPU readback**，别换共享纹理

这些不是"最佳实践"，是**血的教训**。写下来，下一个掉坑的人（包括三个月后的我自己）
就能绕开。

---

## 七、还有哪些坑没填

我不想只报喜，直说短板：

- **音频系统还没做**。`assets` 里躺着 4 首 BGM 和 ~90 个音效，`n3ri-audio` 这个
  crate 还是注释状态。
- **只支持 Linux Wayland**，最佳体验还绑 niri 桌面。X11 我没测过。
- **终端还很朴素**：逐行文本模型，直接剥掉所有 ANSI。想要 zsh/p10k 那种花花绿绿的
  效果，得先实现真正的 VT 模拟（256 色、光标寻址、差分重绘）。我特意没迁就——因为
  **输入侧协议的能力，不能超过输出侧解析器的水平**，硬塞只会换来越位和乱码。
- **国际象棋 ELO 还没接**，几个游戏也偏"体验型"。

---

## 八、最后

做 n3ri_os 这 11 天，我最大的感受是：

**Rust 生态到底成不成熟，不是靠嘴吵出来的。**

有人天天在网上争论"Bevy 能不能做 UI"、"Rust 能不能写桌面应用"。我没参与吵架，
我直接把 Bevy 0.19 + Servo + 纯 Rust Live2D + Wayland layer-shell 全拼到一起跑起来，
然后把每一次翻车都写进文档。

它是不是生产级桌面环境？不是。它是个大玩具。**但这个玩具，每个零件都是真的。**

代码全在下面，MIT 协议，随便看、随便玩、欢迎提 issue：

## 仓库地址

> **https://github.com/swordreforge/bevy_n3ri_os**

```bash
git clone https://github.com/swordreforge/bevy_n3ri_os.git
cd bevy_n3ri_os
cargo run -p n3ri-minimal                  # 窗口模式
cargo run -p n3ri-minimal -- --wallpaper   # 桌面壁纸模式
```

如果这篇文章帮你绕开了哪怕一个坑，或者你就是单纯觉得这玩具挺酷——**去仓库点个 star
吧，那是我继续填坑的动力。**

也欢迎在评论区聊聊：**你觉得下一个该填的坑是音频，还是终端 VT？**

---

*本文由项目开发者本人撰写，基于仓库真实文档与提交历史。*

*致谢所有开源作者：Bevy、wgpu-graft、bevy_tweening、bevy_woff、portable-pty、
chrono、reqwest、icu_provider、serde、mocari。*
