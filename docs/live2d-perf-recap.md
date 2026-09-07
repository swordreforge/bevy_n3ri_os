# Live2D 渲染性能优化复盘

> 日期：2026-09-06 ~ 2026-09-07
> 范围：`crates/n3ri-live2d`（`renderer.rs` / `lib.rs` / `pet.rs`）+ `assets/shaders/live2d_drawable.wgsl`
> 提交：`bf5b1b0`（写放大治理）→ `f34dff9`（mask 通道打包 25→7）→ `fb94711`（tick 降频）
> 结果：帧率 +30%，GPU 上传瓶颈解除，交互系统不受影响

---

## 1. 背景与目标

- 项目 `bevy_n3ri_os`，Bevy 0.19 伪 OS 桌面。Live2D 宠物经离屏 RTT
  （pet / head / mask）渲染后贴到 UI，位于应用窗口下一层。
- 现象：GPU 上传瓶颈，帧率上不去，render 线程单核跑满。
- 工具链：`perf record -F … -g --call-graph dwarf,32768 -p $(pidof n3ri-minimal)`
  + `perf report --stdio` 系列文本报告（`perf_one.txt` / `perf_orig.txt` /
  `perf_flame.txt` / `full.txt` / `pref_full_new.txt` / `perf_new_first.txt` /
  `pref_symbol_new.txt` 等）。

## 2. 测量方法（固定下来，后续沿用）

| 步骤 | 命令 | 用途 |
|---|---|---|
| 粗扫 | `perf top` / 无栈 report | 看 self 占比定方向 |
| 定凶 | `perf record -g` + 调用子树 | 区分主世界写放大还是 render 线程流量 |
| DSO 确认 | `--sort dso,symbol` + `perf script \| grep` | 裸地址先认 DSO；stripped 驱动到 DSO 即止，不再深挖符号 |
| 符号解析 | `eu-addr2line -e /usr/lib/libc.so.6 <addr>` | libc 内部地址定性（如 `0x176ed3` → `memset`） |

判定原则：**self 占比是线索不是判决**。必须看调用子树 + 旁证
（slab / page fault / write_buffer 是否同侧），否则会被伪影误导
（见 §5 的 FreeList 驱动调用边案例）。

## 3. 瓶颈总览（优化前 → 优化后）

| 热点 | 优化前 | 优化后 | 备注 |
|---|---|---|---|
| `FreeListAllocator::allocate` | 17.6% | ~10% | 性质变化：主世界 BindGroup 重建 → render 线程顶点上传刚需 |
| `MeshSlabAllocator::allocate` | 4.3% | 3~6% 波动 | 伴随 hashbrown find/insert、write_buffer、page fault，同属 render 上传流量 |
| `libc 0x176ed3` | 5.3% | ~3% | 实为 `memset`（`memset-vec-unaligned-erms.S:321`） |
| `device_create_bind_group` | 0.7% | 0.47% | 主世界写放大已压住 |
| `PreparedMaterial2d::prepare_asset` | — | 0.12% | 同上 |
| `sync_live2d` 自身 | — | 0.42~0.63% | 主世界侧干净 |
| `tick_pet → csmiUpdateModel` | 被掩盖 | 4.2% → ~2% | 优化后期浮现，闭源 Core，单次 cost 改不动 |
| `Queue::submit` 4.7% + 驱动 memset 两链 | — | 随 pass 数同比掉 | `submit → maintain → drop EncoderInFlight → memset`；`camera_driver → begin_encoding → memset` |
| futex 系 | 6~8% | 同左 | 线程 park 空等（`futex_wait`），不是锁竞争，不追 |

最终：帧率 +30%，交互系统（petting / 点击 / idle）逐帧不受影响。

## 4. 第一轮：写放大治理（17.6% → ~10%）

### 4.1 根因

`sync_live2d`（`renderer.rs`）每帧对每个 drawable 无条件调用
`materials.get_mut(mat_h)`。Bevy 的 `AssetMut::DerefMut` **一触即发
`Modified` 事件**——即使值完全没变，render world 也会每帧重建全部
BindGroup（`device_create_bind_group` + `FreeListAllocator::allocate`）。

连带问题：

- 顶点经 `Local<Vec>` scratch `push` + `copy_from_slice` 双拷贝；
- 查询 `With<Mesh2d>` 扫全场 UI 网格；
- pet/head/mask 相机 `is_active` 恒 true，隐藏了还在空转
  （pet 全尺寸 + head 256 全套重画）；
- `pet.rs tick` 每帧堆分配两个 `Vec<String>` 空参数。

### 4.2 改法（提交 `bf5b1b0`）

- 材质先 `get` 读比对（flags / multiply / screen），变化才 `get_mut`；
- 顶点直写 mesh 缓冲，删除 `Local` scratch 中转；
- 新增 `PetDrawable` Component，查询收窄到 `With<PetDrawable>`；
- 新增 `gate_pet_cameras` 系统，按 `PetDisplayNode` / `HeadDisplay`
  可见性翻 `Camera::is_active`（Bevy 0.19 `camera.rs:391` 确认字段存在），
  只在目标状态变化时写，避免觸發 change detection；
- `pet.rs`：`let empty_ids: Vec<String> = Vec::new()` →
  `let empty_ids: &[String] = &[]`（`do_update_motion` 只读）。

### 4.3 效果

BindGroup churn 掉出 top，`sync_live2d` 自身仅 0.42~0.63%。
`cargo check -p n3ri-live2d` / `-p n3ri-minimal` 通过。

## 5. 第二轮：定凶 libc 与驱动 memset（确认开关在我们手上）

### 5.1 `0x176ed3` 定性

最初只能看到归属 `libc.so.6` 的 `0x176ed3`，无法下手。
`eu-addr2line` 定位到 `memset-vec-unaligned-erms.S:321`——是 `memset`，
不是 libc 自身开销，而是“有人在替它每帧清零新分配的大块内存”。

### 5.2 调用栈（带 dwarf 重采，`RUSTFLAGS="-C force-frame-pointers=yes"`）

- `submit → maintain → drop EncoderInFlight → libvulkan_intel → memset`（~1.2%）
- `camera_driver → encode_render_pass → begin_encoding → libvulkan_intel → memset`（~2.2%）

`0x7f6a38…` 经 `--sort dso,symbol` + `perf script` 确认为
`libvulkan_intel.so`（stripped，故 `[unknown]`，到 DSO 即止）。
结论：驱动在创建/销毁 command encoder 与开启 pass 时清零，
**随 pass 数线性涨——开关在我们手上，不在驱动手上**。

### 5.3 伪影教训（重点）

`FreeListAllocator::allocate` 13.9% 那棵子树把驱动裸地址画成了
allocator 的“调用者”——驱动不可能调用 Rust 分配器，这是 dwarf 穿过
stripped 驱动帧时的展开伪影。`DynQueue::submit` 被画成 slab allocate
调用者同理。均不可采信，靠旁证定罪：
slab allocate + hashbrown 全套 + `write_buffer` 系 + page fault
**全在 render world**，主世界侧干净 → 剩下的就是每帧 GPU 缓冲建销流量。

## 6. 第三轮：mask 通道打包（25 pass → 7 pass）

### 6.1 根因

启动日志：`live2d mask groups: 25 groups, mask sources: 27`。
几乎每个被遮罩 drawable 独占一套全屏 RTT + 相机 + pass——每帧 25 次
全屏 clear + 28 套 encoder/submit/maintain。驱动 memset 和 FreeList
大头都是被它乘出来的。

### 6.2 改法（提交 `f34dff9`）

4 个 mask 组打包进一张 RTT 的 RGBA 四通道：

- `BlendKind::MaskFbo` → `MaskLane(u8)`，按 `lane % 4` 分管线；
  `specialize` 里对 mask 管线设 `write_mask`（R/G/B/A），同单元 4 组
  各写各通道，互不干扰；
- setup 按 `group_sources.chunks(4)` 建 RTT/相机：层号 `10+g` → `10+p`，
  order `-(30+g)` → `-(30+p)`，只用 7 层；
- 被遮罩 drawable 的 `viewport.z` 记录 lane 号，shader 按 `vp.z`
  选择 `m.r/g/b/a` 采样；混合约定不变（clear 白 = 隐藏，`1-alpha` = 可见）；
- `refit_pet_view` 只改 `viewport.xy`、**保留 `z` 的通道号**——否则第一次
  refit 即穿帮（眼白、口内异常）。

### 6.3 中间插曲（语义先行）

曾想让 shader 按通道输出 `(c.a,1,1,1)`，实为错误：同单元四组是串行画进
同一张 RTT 的，全量输出会把别组通道冲掉。正确做法是 shader 原样输出
`(0,0,0,texAlpha)`、靠管线 `write_mask` 做通道路由——因为
`write_mask` 是“限输出”，不是“搬值”。混合 math：本通道值 =
`1-texAlpha`，clear 白保证未触通道 = 1（= 隐藏），四组互不冲。
**教训：不动手前先想清楚混合 math / 脏标记语义，否则 shader 必返工。**

### 6.4 效果

submit / maintain 固定开销同比掉，GPU 上传瓶颈解除，可打满。

## 7. 第四轮：tick 降频（帧率 +30%）

### 7.1 根因

主线程 `tick_pet → csmiUpdateModel` 4.2%
（`WarpDeformer` / `Interpolate` 全闭源 Core），单次 cost 无解；
且每次 tick 触发全量顶点上传。

### 7.2 改法（提交 `fb94711`）

- 新增 `PetTickState { acc, ticked }` + `PET_TICK_INTERVAL = 1/30`，
  `tick_pet` 累积 dt、30Hz 推进一次。累积量一次性推进，
  总量 == 逐帧之和，**动作速度不变**；
- `sync_live2d` 加 `pet_tick_done` 同拍门控
  （`run_if(pet_display_on.and_eager(pet_tick_done))`，
  注意 `and` 已 deprecated，用 `and_eager`），只在 tick 帧上传；
- 交互系统（`detect_petting` / `track_clicks` / `check_idle_timeout`）
  保持逐帧，不受影响；快速表情切换最多半帧延迟，肉眼无感。

### 7.3 效果

`csmiUpdateModel` 4.2% → ~2%，顶点上传流量减半，帧率 +30%。
`cargo check` / `clippy` / `cargo test -p n3ri-live2d` 全过。

## 8. 经验与教训

1. **先数据后动手，每轮一个假设**：写放大 → 定凶 → 砍 pass → 降频，顺序不可乱。
2. **self 占比是线索不是判决**，FreeList 的驱动调用边即反例；看子树 + 旁证。
3. **语义先行**：`write_mask` 限输出不搬值；`AssetMut::DerefMut` 一触即脏。
   不想清楚就写 shader / 资产代码必返工。
4. **小门控大收益**：`is_active`、比对写入、静态空切片——几行改动、零风险。
5. **不追项**：futex 6~8% 是 park 空等；`batch_and_prepare` /
   `depth_textures` 0.05% 级是噪音。

## 9. 地板与后续方向

当前地板（接受，不再追）：

- FreeList ~10%：每帧 CPU 蒙皮顶点上传刚需；
- render 线程单核架构：submit / maintain / allocator 全串行，拆不开；
- `renderer_extract` 4.1%：drawable 数量决定的 draw call 税。

可选但 ROI 递减：

- mask RTT 降分辨率——只省显存 / fill，不省 pass 数；
- 睡眠时停 tick（已有 `pet_display_on` 门控，可再激进）；
- 合并 drawable——大改（重排 mesh / 遮罩 / 混合），不建议。

验收建议：肉眼优先（正常显示 + 遮罩无穿帮 + 表情延迟无感），
perf 只看趋势不追小数。复测重点看 `FreeListAllocator::allocate`
是否维持个位数、主线程是否出现新的 >3% 粗项。
