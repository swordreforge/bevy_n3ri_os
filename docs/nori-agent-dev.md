# Nori Agent 开发文档（DEV）

> 对应计划文档：`docs/nori-agent-plan.md`（目标 / 里程碑 / 验收）。
> 本文档是实现手册：类型、system、存储格式、接线位置、prompt 组装、门控伪代码、测试点。
> 参考机制来源：`reference/N.E.K.O`（`docs/architecture/*.md` + `memory/` + `main_logic/activity|topic|core/tool_calling.py` + `brain/task_executor.py` + `config/*settings.py`）。
> Bevy 约束总纲：`docs/bevy-0.19-dev-knowledge.md`（Message 非 Event、`WindowFocusSet` 顺序、`CursorPosition` 契约、B0001、change-guard）。

## 0. 依赖方向（先看这节，否则会循环依赖）

```text
n3ri-llm ──► (无 Bevy, 只加 tool wire 类型)
n3ri-core ──► (Messages: AppLaunch/Notification/MusicCommand, 无渲染)
n3ri-agent ─► 依赖 { n3ri-core, n3ri-llm }，不依赖 n3ri-ui / n3ri-live2d
n3ri-ui ────► 依赖 { n3ri-core, n3ri-llm, n3ri-agent }（bridge + 表现层）
examples/minimal ─► 组装一切（也可只在 minimal 加 AgentPlugin，见 plan Q1）
```

规则：

1. Agent 内的所有类型**不得**引用 `n3ri-ui` 的 `FocusedTitle/AppWindow/CursorPosition/ChatHistory`。
   需要的窗口/光标/音乐信息，由 `n3ri-ui` 侧的薄 bridge system 翻译成下面的
   `AgentWorldView`（纯数据）再喂给 Agent。
2. Agent 向 UI 说话只经过两样东西：`n3ri-core` 的 `Message`
   （`NotificationEvent` / `AppLaunchEvent` / `MusicCommand`）和自己定义的
   `AgentSayEvent`（`chat_capsule` 侧读到后进气泡队列 + `ChatEmotionEvent`）。
3. `n3ri-llm` 保持无 Bevy：tool wire 类型 + 纯函数解析放这里，可 `cargo test` 单测。

```rust
// crates/n3ri-agent/src/lib.rs
pub struct AgentPlugin;
impl Plugin for AgentPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AgentConfig>()
            .init_resource::<AgentWorldView>()
            .init_resource::<HotMemory>()
            .init_resource::<SchedulerState>()
            .init_resource::<AgentTurn>()       // 单 flight（被动+主动共享，见 §5）
            .init_resource::<BubbleOutbox>();   // AgentSayEvent 的落地队列（ui 侧消费）
        app.add_message::<AgentSayEvent>();
        app.add_systems(Update, (
            context_tick,            // 1s：AgentWorldView -> ContextSnapshot
            scheduler_tick.after(context_tick),
            agent_dispatch,          // 有待发请求 -> 起后台线程
            agent_poll,              // 收线程结果 -> 出泡/tool/记忆落盘
            memory_maintenance_tick, // 低频：摘要/抽取/合成/归档（带预算）
        ));
    }
}
```

`n3ri-ui` 侧新增一个小文件 `apps/agent_bridge.rs`（`AgentBridgePlugin`）：

- `world_view_bridge.after(WindowFocusSet)`：读 `FocusedTitle` / `AppWindow{z,app_id}+AppVisible+Visibility` /
  `CursorPosition` / `UiArea` / `MusicStatus` / `TextInputOwner` / `Time`，写 `AgentWorldView`（只在变化时写）。
- `agent_say_bridge`：读 `AgentSayEvent`，把文本推进现有气泡队列（复用 `ChatBubbleState::queue`，
  或 M2 起独立的 `BubbleOutbox`；v1 建议复用，前提是加“主动”标记位，见 §5.3）。
- `AppLaunchEvent` / `NotificationEvent` 补 reader（二选一：dock 侧读，或 bridge 侧读并调 dock 现有 spawn 函数；plan Q2，默认推荐 bridge 侧读，dock 不动）。

## 1. 新 crate 与模块划分

```toml
# crates/n3ri-agent/Cargo.toml
[package]
name = "n3ri-agent"
version = "0.1.0"
edition = "2021"

[dependencies]
bevy = { workspace = true, features = ["bevy_asset", "bevy_log", "bevy_state"] }
n3ri-core = { path = "../n3ri-core" }
n3ri-llm = { path = "../n3ri-llm" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
chrono = { workspace = true }
```

> bevy features 对齐 `n3ri-core`（render-free）。Agent 不做任何渲染，只用 ECS + log + state。
> chrono workspace 已有（topbar 在用）。

```text
crates/n3ri-agent/src/
  lib.rs        # AgentPlugin + 重导出
  config.rs     # AgentConfig + load/save (~/.config/n3ri_os/agent/config.json)
  world.rs      # AgentWorldView（ui 侧写入）+ ContextSnapshot（1s 聚合）+ Presence + daypart()
  scheduler.rs  # HookKind + SchedulerState + 门控纯函数 + scheduler_tick
  memory.rs     # HotMemory + MemoryStore（文件）+ build_agent_context + BM25 recall
  tools.rs      # ToolRegistry + 内置 tool + tool-loop（调 LlmClient）
  tools/niri.rs # niri msg 子进程封装 + 白名单（§4.6，纯函数可单测）
  turn.rs       # AgentTurn（单 flight 后台线程范式，抄 chat_llm_dispatch/poll）
  prompt.rs     # system 组装（worldview + <context> + 记忆段）+ 预算截断
```

`n3ri-llm` 侧增量（`crates/n3ri-llm/src/tools.rs`，无 Bevy）：

```rust
pub struct ToolCallRequest { pub id: String, pub name: String, pub arguments: serde_json::Value }
pub struct AssistantMessage { pub content: String, pub tool_calls: Vec<ToolCallRequest> }
pub struct ToolDef { pub name: &'static str, pub description: &'static str, pub parameters: serde_json::Value }

impl LlmClient {
    /// 带 tools 的阻塞调用。解析 choices[0].message.content + tool_calls。
    /// 现有 send() 保持不动（被动短轮继续用它，零风险）。
    pub fn send_with_tools(&self, messages: &[Message], config: &LlmConfig, tools: &[ToolDef])
        -> Result<AssistantMessage, String>;
}
pub fn recall_memory_schema() -> serde_json::Value; // function parameters schema
pub fn open_app_schema() -> serde_json::Value;
pub fn notify_schema() -> serde_json::Value;
pub fn niri_windows_schema() -> serde_json::Value;
pub fn niri_spawn_schema() -> serde_json::Value;
pub fn niri_window_schema() -> serde_json::Value;
```

wire 示例（抄 N.E.K.O `tool_calling.py` 的 OpenAI-flavoured 形状）：

```json
// 请求 tools 段
{"type":"function","function":{"name":"recall_memory","description":"…",
 "parameters":{"type":"object","properties":{"query":{"type":"string"},"time":{"type":"string"}},"required":[]}}}
// 模型返回
{"role":"assistant","content":"…","tool_calls":[{"id":"call_abc","type":"function",
 "function":{"name":"recall_memory","arguments":"{\"query\":\"用户喜欢什么游戏\"}"}}]}
// 回填
{"role":"tool","tool_call_id":"call_abc","name":"recall_memory","content":"{\"hits\":[…]}"}
```

规则（抄 `omni_offline_client/_tools.py`）：`max_tool_iterations = 3`；`call_id` 原样回传；
`arguments` 解析失败进 `{_raw}` 不抛错；handler 返回 envelope `{output, is_error}` 永不抛异常；
3 轮用完强制去 tools 再调一次拿终答。

## 2. 上下文（world.rs + prompt.rs）

### 2.0 `FocusedTitle`：它实际在干哪几件事（Agent 只借第 1 件）

`FocusedTitle { title: String, entity: Option<Entity> }`（`topbar.rs:52`）是全仓唯一的“谁在前台”事实源。
写方只有 `window.rs` 三处：`promote_added_windows`（新窗即焦点）、`window_focus_system`
（点谁谁置顶 + z 提升）、`window_focus_validate`（焦点窗隐藏/despawn 后回落到最高 z 或 `None`/`"n3ri_os"`）。
读方决定了它的真实职能，按硬度排序：

1. **输入路由总闸**——所有文本/IME 消费者（terminal、browser 三件套、cakeduel、pictionary、seek_treasure…）
   开头都是 `focused.title == "xxx"`，不对就 `reader.clear()`（防旧键漏进新焦点，knowledge §9.5）。
   这是它最硬的职能：整个伪桌面的键盘归属由这一个字符串比较决定。
2. **IME 开关**——`sync_ime_window` 按 title（终端/浏览器）或 owner（chat/settings）决定
   `ime_enabled + ime_position`。
3. **顶栏显示**——`topbar_focus_sync` 把 title 描进左上角（macOS 式当前应用名）。
4. **z-order 副作用载体**——focus 变更总是顺带做 z 提升/压缩（MAX_WINDOW_Z=32）；读 title 的人无感，
   但写方把“置顶”和“聚焦”绑在一起了。
5. **测试锚点**——`window.rs` 两个单测断言 `FocusedTitle.entity == Some(added)`。

隐含不变量与缺口（Agent 设计必须知道）：

- 字符串比较成立全靠 **dock 单实例**（`dock_update` 按 `app_id` 找到就 toggle，不会开第二个同名窗），
  所以 title 事实唯一。一旦将来允许多开同应用，title 比较即失效——Agent 因此**不读 title 做身份判断**。
- `entity` 字段实际被冷落：几乎所有 reader 只比 title。Agent bridge 反其道而行：
  `entity → AppWindow.app_id` 优先，title 只做回退。
- 点桌面空白**不**清焦点（`window_focus_system` 里 `target is None` 直接 return，只有隐藏/despawn 才回落），
  所以 title 可能 stale。Agent 不用它判断“有没有人”（那看 `CursorPosition.active` + idle 计时），
  只用它判断“有人时在干什么”。
- 壁纸模式下同一套 resource 继续工作（focus 覆盖只重算 `Interaction`，不换焦点语义），
  但需实测验证（plan §8 风险项）。

Agent 读取契约：bridge 在 `world_view_bridge.after(WindowFocusSet)` 里读（保证 focus 已结算），
1s 快照一次，变化才写 `AgentWorldView.focused_*`；Agent 内部其它地方禁止直接读 `FocusedTitle`。

### 2.1 `AgentWorldView`（ui → agent 的唯一输入，纯数据，每帧可写但 bridge 做 change-guard）

```rust
#[derive(Resource, Default, Clone)]
pub struct AgentWorldView {
    pub focused_title: String,          // 来自 FocusedTitle.title
    pub focused_app_id: Option<String>, // 同 entity 反查 AppWindow.app_id（找不到则 None）
    pub visible_windows: Vec<WindowInfo>, // 可见窗口（AppVisible.0 && Visibility != Hidden）
    pub typing: bool,                   // TextInputOwner.0 != None（正占输入框即 typing）
    pub cursor_moved: bool,             // bridge 内差分 cursor.logical，本帧是否变化
    pub input_event: bool,              // bridge 内合并 KeyboardInput/MouseButtonInput 是否有新事件
    pub music_playing: bool,            // MusicStatus.playing
    pub music_title: Option<String>,    // MusicLibrary[current].title（有则填）
    pub immersive: bool,                // CinematicLocked 窗口可见（focus.rs 凑近态，§2.2 跳 Immersive 用）
    pub outside: Option<OutsideView>,   // niri 真实桌面投影（§4.6，无 niri 会话则 None）
    pub wallpaper_mode: bool,           // minimal 启动参数透进来（windowed=false）
    pub now_local: String,              // "2026-09-05 周五 14:03"（chrono，bridge 每秒刷一次即可）
    pub cpu_pct: Option<f32>,           // 有现成状态就带，没有填 None（不另起探针）
    pub net_online: Option<bool>,
    pub battery_pct: Option<u8>,
}
#[derive(Clone)]
pub struct WindowInfo { pub app_id: String, pub title: String, pub z: i32 }
#[derive(Clone)]
pub struct OutsideView { // niri 真实桌面投影（§4.6，5s 缓存，bridge 侧填）
    pub workspace: String,               // 如 "1 （eDP-1)"
    pub windows: Vec<OutsideWindow>,     // 投影 4 字段，focused 置顶
    pub available: bool,                 // niri msg 可用？否 -> prompt 静默降级
}
#[derive(Clone)]
pub struct OutsideWindow { pub id: u64, pub title: String, pub app_id: String, pub focused: bool }
```

bridge 注意：命中用 `cursor.physical`，样式读写用 `cursor.logical`（knowledge §5）；
`visible_windows` 按 `z` 升序；`input_event` 的 reader 必须与其它消费者共存
（`MessageReader` 可多读，不清空）。

### 2.2 `ContextSnapshot`（agent 内 1s tick 聚合，prompt 的直接原料）

```rust
#[derive(Resource)]
pub struct ContextSnapshot {
    pub updated_at: f64,            // Time::elapsed_secs
    pub last_active_at: f64,        // 最后一次 input_event/cursor_moved
    pub idle_secs: f32,
    pub presence: Presence,         // Active | Idle | Away
    pub activity: Activity,         // Free | FocusedWork | Immersive
    pub view: AgentWorldView,       // 最新一份拷贝
    pub focus_dwell: (String, f32), // (当前前台 app_id, 持续秒数)
    pub focus_trail: VecDeque<(String, f64)>, // 最近窗口序列（最多 8，用于 transition 判断）
}
pub enum Presence { Active, Idle, Away }        // idle>=300 -> Idle；>=900 -> Away（抄 N.E.K.O）
pub enum Activity { Free, FocusedWork, Immersive }
```

- `context_tick`：`Local<f32>` 累 1s 才跑一次；`input_event||cursor_moved` → `last_active_at=now`。
- `Activity` 判定（简化版 state_machine，先够用）：
  - 前台 `focus_dwell >= 90s` 且（typing 或 input 频繁）→ `FocusedWork`。
  - 前台 app_id 在 `{browser}` 且 dwell>=120s，或 `CinematicLocked` 窗口可见 → `Immersive`
    （对应 N.E.K.O `restricted_screen_only`：v1 在此态**必跳过**一切非 must-fire 主动，
    §3.2；`CinematicLocked` 输入由 bridge 经 `AgentWorldView.immersive: bool` 透进来，
    Agent 不读 ui 的组件，只读这个 bool）。
  - 5min 内切过 ≥5 个不同窗口 → 视为 transitioning，按 `Free` 处理但 idle 钩子延迟。
  - 其余 `Free`。
- propensity 映射（抄 N.E.K.O 三档）：`Closed`（typing 且 60s 内有输入事件 → 别打断）/
  `Restricted`（Immersive → 只允许 must-fire 之外的静默）/ `Open`（可搭话）。

### 2.3 prompt 组装（prompt.rs；预算用字符数，Rust 无 tiktoken，按 N.E.K.O 的 char-cap 思路）

```text
system = 世界观(Nori_system_prompt.txt 磁盘优先, 复用 chat 侧 loader 逻辑, agent 内自带一份)
       + EMOTION_PROMPT（末尾情绪标签，复用同一常量语义）
       + "<context>\n{context_block}\n</context>"（§2.4）
       + "<memory>\n{memory_block}\n</memory>"（§4.3，无记忆时整段省略）
```

`context_block` 示例（`build_agent_context` 输出，字符上限 800，超了先砍 `visible_windows` 尾部，
再砍 `outside.windows` 尾部）：

```text
时间: 2026-09-05 周五 14:03（下午）
前台: 终端 (terminal, 已聚焦 12 分钟)
可见窗口: 终端, 浏览器, 设置
外面: niri 工作区 1, 火狐 x2(其中一个在前台), 终端(kitty), 文件(nautilus)
状态: 专注中(typing)/空闲 40 秒/音乐播放中《xxx》
模式: 窗口模式
```

时间感知规则（§3.2）：首行必带本地时间 + 时段词（深夜 0–6 / 凌晨 6–8 / 上午 / 中午 / 下午 / 傍晚 / 夜里）
+ 星期；深夜叠加 system 约束“语气放轻、只许一条、不追问”。时段词纯函数 `daypart(hour)`，单测覆盖。

硬约束：context/memory 块只进 system，不进可见气泡；Nori 输出约束（2~3 句、一句一行、
无 Emoji）在主动轮同样生效——靠沿用同一世界观 prompt + 同一情绪解析保证。

## 3. 调度器（scheduler.rs）

### 3.1 触发器（v1 只有 4 种）

```rust
pub enum HookKind { Idle, Hourly, Break, Startup }
pub struct HookFire { pub kind: HookKind, pub reason: String } // reason 进 prompt，告诉模型为啥开口
```

| Hook | 触发条件 | prompt reason 示例 | must-fire? |
|---|---|---|---|
| Idle | `idle_secs >= idle_threshold`（默认 1800s，§3.2） | “用户挂机 32 分钟了，轻轻搭句话” | 否 |
| Hourly | 整点跨过（`last_hour != now.hour`）且 presence != Away；**默认关闭** | “整点报时，顺带一句陪伴” | 否 |
| Break | `activity==FocusedWork` 累计 >=2700s（45min，§3.2） | “用户专注 45 分钟了，提醒喝水/休息” | 是（短路模板，不走 paraphrase 挑选） |
| Startup | 启动且 `now - last_session_at >= 900s`（gap 抄 greeting policy） | “用户回来了（离开 X），欢迎” | 是 |

`burst` 去重：`now - last_startup_greeting < 1800s` 则 Startup 不发（抄 30min burst）。

### 3.2 门控（按顺序，任一失败即 pass，抄 proactive 五层中的四层；turn 互斥见 §5）

> 主动频次总纲（与 N.E.K.O 的分歧点）：N.E.K.O 是“主动陪伴”产品，默认积极搭话；
> Nori 默认是**安静等待型**人格（世界观：“等待用户是身份的一部分”），主动开口是例外不是常态。
> 所以门控数字只抄 N.E.K.O 的**上限语义**，默认阈值全部调保守，见下表。所有值进 `AgentConfig`，
> 设置页可调（plan 已有“智能体”Tab 项）。

| 项 | N.E.K.O 值 | Nori 默认值 | 说明 |
|---|---|---|---|
| idle 搭话阈值 | 心跳 20s / 信号 idle 5min 起算 | **30min**（`idle_threshold_secs=1800`） | 5min 太扰；Nori 挂机时“安静等”符合人设 |
| idle 搭话冷却 | topic min_gap 4h | **6h**（同 hook 距上次 fire） | 一天最多 2~3 次 idle 搭话 |
| topic 日配额 | 2.0/天（权重） | **1.0/天** | 权重记账照抄（未回应 1/3，回应 1.0，窗口 10min） |
| 整点报时 | 无（仅时间 hint 进 prompt） | **默认关**（`hourly_chime=false`） | 开了也是整点 ±5min 窗口内一条，且 presence 必须 Active |
| 久坐提醒 | 30min（must-fire） | **45min，默认开**，冷却 4h | 健康类提醒是 must-fire 中唯一保留的 |
| 启动问候 | gap 15min / burst 30min | 照抄（900s/1800s） | 回来欢迎符合“等待”人设，不砍 |
| skip dice | immersive 0.3 | `Immersive → 1.0`（即**必跳**，除 Break/Startup） | 游戏/视频/凑近态（`CinematicLocked` 可见）下只字不提 |
| 时间感知 | hour/weekday/period + holiday hint 进 prompt | 照抄：`<context>` 首行永远是本地时间 + 时段词（深夜/凌晨/清晨/上午/…​）+ 星期 | 深夜（0–6 点）叠加一条：语气放轻、只许一条且不再追问（system 指令约束） |

```rust
pub fn gate(fire: &HookFire, snap: &ContextSnapshot, st: &SchedulerState, cfg: &AgentConfig) -> GateResult;
pub enum GateResult { Pass, Drop(&'static str) } // reason_code 照抄 N.E.K.O 词表子集：
// PASS_BUSY / PASS_CLOSED / PASS_RESTRICTED / PASS_COOLDOWN / PASS_QUOTA / PASS_DUPLICATE
```

1. 总开关 `cfg.enabled`，记忆相关 hook 另看 `cfg.memory_enabled`。
2. `DesktopState == Normal` 才发（菜单/通知/设置覆盖层打开时不打扰；读 `DesktopState` 需 agent 加
   `bevy_state` 依赖，已在 Cargo 模板里）。
3. propensity：`Closed → Drop(PASS_CLOSED)`；`Restricted → 仅 Break/Startup 可过，其余 Drop(PASS_RESTRICTED)`。
4. 单 flight：`AgentTurn::pending → Drop(PASS_BUSY)`（被动聊天进行中主动让路，plan Q3 默认方案）。
5. 冷却：同 hook 距上次 fire `< hook_cooldown`（Idle 21600s / Hourly 3600s / Break 14400s / Startup 1800s）→ Drop。
6. topic 配额（抄 `pipeline.py:64`，阈值见 §3.2 总纲）：当日权重和 `>= daily_quota(1.0)` → Drop(PASS_QUOTA)；
   投递后记账：用户 10min 内回话权重 1.0，否则 1/3；min_gap 4h（距上次任意主动投递）。
7. 去重：`reason` 文本 48h 内字面重复（hash）→ Drop(PASS_DUPLICATE)。
8. skip dice：`Free → 0`；`Immersive → 1.0`（游戏/视频/凑近态必跳，Break/Startup 除外，§3.2）。
   `Immersive` 判定输入多一路：`CinematicLocked` 窗口可见（focus.rs 凑近态，游戏开局即免打扰）。

`SchedulerState` 持久化一小部分（`agent/scheduler.json`）：`{day: "2026-09-05", weight_sum: f32,
last_fire: {hook: ts}, last_any_fire: ts, recent_reasons: [{hash, ts}]}`，原子写。

### 3.3 投递

- 非 must-fire：走正常 LLM 生成（`build_agent_context` + 短 system 指令“主动搭一句 2~3 句”），结果进 `BubbleOutbox`。
- must-fire（Break/Startup）：同样走 LLM，但失败时回退到静态模板一句（如“专注很久啦，喝口水休息一下呀。”），保证提醒必达。
- 成功投递后：`weight` 先按 1/3 记，10min 窗口内 `AgentWorldView.input_event` 且是用户对 Nori 说话
  （chat `submitted` 非空）则升级为 1.0（抄 `_TOPIC_RESPONSE_WINDOW_SECONDS = 600`）。

## 4. 记忆（memory.rs）

### 4.1 文件布局（`~/.config/n3ri_os/agent/`，与 `llm-config.json` 同父目录，`dirs::config_dir()`）

```text
agent/
  config.json              # AgentConfig（§6）
  scheduler.json           # §3.2
  profile.json             # 冷：用户画像（名字/喜好/习惯，LLM 抽取 + 用户可改）
  facts.json               # 温：[{Fact}]（活跃）
  facts_archive.json       # 温：被吸收且 >7d 的 facts
  reflections.json         # 温：[{Reflection}]
  reflection_archive/      # 温：终态 >30d 分片（shard-YYYY-MM.json，最多 500 条/片，抄 ARCHIVE_FILE_MAX_ENTRIES）
  persona.json             # 冷：{user:[], nori:[], relationship:[]}（persona 整段不进 recall 池，抄 N.E.K.O）
  episodes-YYYY-MM-DD.jsonl# 温：当日 turn 原文（append-only，每行一轮）
  cursors.json             # {last_fact_turn: u64, last_synth_ts: i64, turn_seq: u64}
```

全部 JSON 写盘原子写（tmp + rename，抄 `save_config`）。读失败 → 默认空，不阻塞聊天。

### 4.2 Schema（v1 简化版，字段名与 N.E.K.O 对齐以便将来对照，砍 embedding/scopes/trust）

```rust
pub struct Fact { pub id: String,          // "fact_20260905140311_ab12cd34"
    pub text: String, pub importance: u8, // 1..10，<5 只存档不进合成
    pub kind: String,                     // "preference" | "event" | "trait" | "other"
    pub created_at: String,               // ISO8601 local
    pub absorbed: bool, pub hash: String }// sha256(归一化小写去标点) 前 16
pub struct Reflection { pub id: String,   // "ref_" + sha256(排序后source ids)前16（幂等，抄 rid）
    pub text: String, pub status: ReflStatus, // Pending|Confirmed|Promoted|Denied|Archived
    pub source_fact_ids: Vec<String>,
    pub reinforcement: f32, pub disputation: f32, pub created_at: String }
pub struct PersonaEntry { pub id: String, pub text: String,
    pub source: String,                   // "manual" | "reflection" | "import"
    pub protected: bool,                  // 手写/角色卡永不淘汰
    pub created_at: String }
```

evidence 简化：`score = reinforcement - disputation`；`>=1 → Confirmed`，`>=2 → Promoted（进 persona）`，
`<= -2 → 归档候选`（阈值抄 `EVIDENCE_*_THRESHOLD`）。信号来源 v1 只有两条：
用户明确肯定/否定（+1/-1）和 fact 文本蕴含（+0.5，抄 `USER_FACT_REINFORCE_DELTA`）。
半衰期 v1 不做，用 30 天无引用自然归档代替（`reflection_archive/`）。

### 4.3 写路径（全部后台线程 + 失败降级，抄 memory-system §Write path）

```text
turn 结束(agent_poll 收到 Ok) ──► append episodes-*.jsonl + HotMemory push
HotMemory len > 20 ──► 摘要任务：取最旧 10 轮 -> LlmClient::send(低配 clone: max_tokens=512)
   ──► memo 替换头部（HotMemory { memo: String, tail: Vec<Message>(10) }），抄 recent.json
turn_seq % 10 == 0 或 idle>5min ──► Stage-1 抽取：最近窗口 -> JSON [{text,importance,kind}]
   ──► hash 精确去重 + BM25 overlap 去重 -> facts.json（importance<5 照存，打标不参合成）
unabsorbed(importance>=5 && !absorbed) >= 5 ──► 合成：top20(importance↓,created↑) -> LLM
   ──► Reflection(Pending, rid 幂等) + 源 facts absorbed=true（LLM 在锁外跑，落盘时重读+CAS，抄 synthesis）
Confirmed 且 score>=2 ──► 晋升 persona（直接追加条目；矛盾合并 LLM 判 v1 砍掉，只做字面包含检查）
每日一次 sweep ──► absorbed 且 >7d facts -> facts_archive.json；终态 >30d reflections -> 分片
```

LLM 并发上限：同一时刻最多 1 个记忆后台任务（与 turn 单 flight 共用计数器，见 §5），
`MEMORY_LLM_HARD_TIMEOUT` 靠 `LlmClient` 的 120s + 线程自然结束（不做取消，v1 可接受）。

### 4.4 读路径（两条，边界抄 N.E.K.O，一字不改）

- **自动段**（每次请求都带）：protected persona 全量 + 普通 persona 按 recency 取到预算 +
  Pending/Confirmed reflections 取 top3 + `memo + tail(10)`。persona 预算 2000 字符、
  reflection 预算 2000 字符（N.E.K.O 是 token，Rust 侧按字符 1:1 近似，CJK 为主时偏保守，可接受）。
- **按需段**（`recall_memory` tool）：BM25 池 = `facts.json + reflections.json + facts_archive.json`
  （persona 不入池，archive 可搜——抄 HYBRID 配池）；阈值 0.1（小池保护，抄注释里的教训）；
  top4 融合后最多 8 条；渲染 `N. [fact 2026-09-05] text`；单条截 400 字符。
  无 embedding 时就是纯 BM25（N.E.K.O 的官方降级行为）。

BM25 实现：不引新 crate（仓库离线可构建优先），手写 ~80 行 Okapi；
分词：ASCII 按空白/标点切，CJK 按 2-gram（抄 N.E.K.O 的 CJK tokenizer 口径）。
`reflection_archive/` 分片不入召回池（v1）。

### 4.5 `recall_memory` 的 handler（tools.rs，同步函数，永不抛错）

```rust
pub fn handle_recall_memory(store: &MemoryStore, args: &serde_json::Value) -> ToolEnvelope {
    // args: {query?: string, time?: string}
    // query only -> BM25；time only -> 按 event/created 时间窗最近 8 条；both -> 时间窗内 BM25
    // 失败 -> {hits: [], note: "记忆暂不可用"}（空结果继续对话，抄 N.E.K.O）
}
```

### 4.6 niri 特化 tools（ArchLinux + niri WM，聊天 agent 的轻量手脚）

定位先说死：这是**聊天 agent**，不是 CUA/desktop-use。重型浏览器操纵（截图→VLM→点坐标 50 步循环）
整段不做（plan §5 非目标第 4 条）。niri tools 只给 Nori 三个“知道外面世界 + 顺手帮小忙”的能力：
**看**（窗口/工作区列表）、**开**（spawn 应用）、**切**（focus 指定窗口）。依据是本机实测：

- `niri msg -j windows` 返回每窗 `{id, title, app_id, pid, workspace_id, is_focused, is_floating,
  layout{...}, focus_timestamp}`；`focused-window` / `workspaces` 同理，都是稳定 JSON。
- `niri msg action <ACTION>` 全是声明式动作，无坐标点击：`spawn` / `spawn-sh` 开应用，
  `focus-window`（按 id）切窗口，`close-window` 关前台窗。完整表见 `niri msg action --help`。

```rust
// 三个 tool 的 schema（参数极简，防模型瞎填）:
niri_windows() -> {windows: [{id, title, app_id, focused}]}
  // 只读：niri msg -j windows，过滤投影 4 字段。handler 常驻缓存 5s（防连打）。
niri_spawn(command: string) -> {ok, pid?}
  // 写：niri msg action spawn -- <argv...>。command 白名单（见下），不在表里直接 is_error。
niri_window(action: "focus"|"close", window_id?: u64) -> {ok}
  // 写：niri msg action focus-window --id <id> / close-window。
  // close 无 id 即关前台，需二次确认语义：模型必须先在正文里问过用户（system 指令约束，不做代码态机）。
```

执行面（`tools/niri.rs`，纯函数 + 薄线程封装，方便单测）：

```rust
pub struct NiriCtl { pub sock: Option<PathBuf> } // 默认走 `niri msg` 子进程（实现最简，5s 缓存下开销可忽略）
pub fn niri_windows() -> Result<Vec<NiriWindow>, String>;   // 解析 -j JSON，失败回 Err(原文截 300ch)
pub fn niri_spawn(argv: &[String]) -> Result<String, String>;
pub fn niri_focus_window(id: u64) -> Result<(), String>;
```

- 白名单（`config.rs: NIRI_SPAWN_ALLOW: &[&str] = &["firefox","kitty","alacritty","nautilus","code","spotify","mpv"]`；
  用户可在设置页追加一行一个）。`spawn-sh` 整段禁用（shell 注入面太大；要管道请走伪桌面内置终端）。
- 安全分级：`niri_windows` 只读免确认；`niri_spawn` 白名单内免确认；`niri_window(close)` 高危——
  v1 直接不暴露 close（只给 focus），等 M5 再议确认 UX。对应 N.E.K.O 的 approve-gate 思想，但用“不给”代替“确认框”。
- 与伪桌面工具的关系：`open_app`（开伪桌面内置应用窗口，走 `AppLaunchEvent`）和 `niri_spawn`
  （开真实系统应用）并存；system 指令里写死区分话术：“伪桌面里的东西用 open_app，外面的真实应用用 niri_spawn”。
- 环境感知：`<context>` 块追加两行（`niri_windows` 5s 缓存的投影 + 当前登录会话 `XDG_SESSION_TYPE/wayland`），
  让 Nori“知道系统大致环境”：

```text
外面: niri 工作区 1, 火狐 x2(其中一个在前台), 终端(kitty), 文件(nautilus)
```

失败降级：`niri msg` 不可用（非 niri 会话）→ handler 返回 `{available: false}`，
system 指令要求模型不再提外部窗口话题（静默降级，不弹错）。

单测：`niri_windows` 的 JSON 解析用本机实测样例做快照测试（`windows` 数组字段投影）；
白名单拒绝走纯函数测试；子进程调用本身不单测（走 M5 手动走查：窗口/壁纸双模式）。

## 5. Turn 执行（turn.rs：单 flight，被动+主动共享）

抄 `chat_llm_dispatch / chat_llm_poll` 的线程范式，一字不差地复用成功经验：

```rust
#[derive(Resource, Default)]
pub struct AgentTurn {
    pub pending: bool,
    pub rx: Option<Mutex<Receiver<TurnOutput>>>,
    pub origin: TurnOrigin, // User | Hook(HookKind) | Maintenance
}
pub struct TurnRequest { pub messages: Vec<Message>, pub tools: Vec<ToolDef>,
    pub allow_tools: bool, pub system: String }
pub struct TurnOutput { pub text: String, pub tool_trace: Vec<String> }
```

- `agent_dispatch`：若 `pending` 直接返回；取队首 `TurnRequest`（被动优先：用户 `submitted` 永远插队到主动之前；
  主动请求等待最多 60s，超时丢弃并记 `PASS_BUSY`）。
- 线程内：`client.send_with_tools` → 若有 `tool_calls`（≤3 轮）：执行 `ToolRegistry::execute`
 （`open_app` 写 `AppLaunchEvent` 进队列——注意线程内不能碰 ECS，
  所以 tool 执行分两段：纯计算类如 `recall_memory` 在线程内做，需要写 ECS 的如 `open_app/notify`
  只生成 `PendingEffect` 带回主线程由 `agent_poll` 落 `MessageWriter`）→ 回填 → 终答。
  文本过 `extract_emotion`（复用同一函数语义：情绪标签剥离 + `ChatEmotionEvent`）。
- `agent_poll`：`try_recv` → 成功：热记忆 push + episodes 落盘 + 文本进 `BubbleOutbox`
  （被动轮同时维持现有 `ChatHistory` 行为，M1 起 `ChatHistory` 改为 `HotMemory` 的视图，见 §7）；
  失败：错误文本只进一条气泡（“呜……信号断断续续的”同风格），maintenance 来源失败则静默丢弃。

### 5.1 被动轮改造（M1，最小侵入）

`chat_capsule::chat_llm_dispatch` 内组装 `req` 时，在 system 后追加
`build_agent_context()` 的 `<context>` + `<memory>` 段（Agent 提供纯函数，chat 侧只调函数，
不依赖 Agent 的 systems）。`ChatHistory` 本体 M1 不动；M3 起把 `ChatHistory.messages`
换成 `HotMemory` 的读写代理（`push/cap/memo` 行为一致，cap 20→“memo+tail10”）。

### 5.2 主动轮文案约束

主动 system 指令固定追加：“这是你主动开口，只说 2~3 句，一句一行，保持 Nori 口吻，
末尾照常带且仅带一个情绪标签。”

### 5.3 气泡队列复用

`ChatBubbleState::queue: VecDeque<String>` 照单收主动文本；逐句揭示/3 条上限/5s 寿命逻辑零改。
唯一新增：`BubbleOutbox` 记 `origin` 用于配额记账（用户 10min 回话窗口升级权重需要区分被动/主动）。

## 6. 配置（config.rs）

```rust
#[derive(Serialize, Deserialize, Clone)]
pub struct AgentConfig {
    pub enabled: bool,            // 总开关，默认 true（plan Q5 待拍板）
    pub idle_threshold_secs: u64, // 默认 1800（30min，§3.2 保守值，不是 N.E.K.O 的 300）
    pub idle_cooldown_secs: u64,  // 默认 21600（6h）
    pub daily_quota: f32,         // 默认 1.0（N.E.K.O 2.0 的一半）
    pub min_gap_secs: u64,        // 默认 14400
    pub memory_enabled: bool,     // 默认 true
    pub hourly_chime: bool,       // 默认 false
    pub break_reminder: bool,     // 默认 true（45min，见 break_dwell_secs）
    pub break_dwell_secs: u64,    // 默认 2700
    pub niri_tools: bool,         // 默认 true（非 niri 会话自动降级，§4.6）
    pub niri_spawn_allow: Vec<String>, // 默认 ["firefox","kitty","alacritty","nautilus","code"]
}
// 路径 ~/.config/n3ri_os/agent/config.json；load() 缺失/损坏 -> default；save() 原子写。
```

调参对照表（默认值出处，审计用）：

| 常量 | 值 | 出处 |
|---|---|---|
| away / idle 阈值 | 900s / 300s | `state_machine.py`（away 15min）、`EVIDENCE_SIGNAL_CHECK_IDLE_MINUTES=5` |
| 心跳/快照 tick | 20s 心跳 → 本实现 1s tick（ECS 便宜，逻辑等价） | `tracker.py:1110` |
| focused_work dwell | 90s | `state_machine.py` |
| break 累计 | 30min | `tracker.py WorkBreakPending` |
| topic 日配额/权重/min_gap/去重窗 | 2.0 / 1 与 1/3 / 4h / 48h | `topic/pipeline.py:64` |
| startup gap / burst | 900s / 1800s | `greeting.py`、`GET /last_conversation_gap` |
| recent cap/压缩阈值/留尾 | 20 / 20 / 10 | 现有 `ChatHistory` cap 20 + `RECENT_*` |
| synth 最小 unabsorbed / 上限 | 5 / 20 | `MIN_FACTS`、 `REFLECTION_SYNTHESIS_FACTS_MAX` |
| recall 每路/总量/阈值 | 4 / 8 / BM25 0.1 | `HYBRID_RECALL_*` |
| tool 循环 | 3 | `_tools.py max_tool_iterations` |
| 归档 | facts 7d / reflections 终态 30d / 分片 500 | `save_facts`、`EVIDENCE_ARCHIVE_DAYS`、`ARCHIVE_FILE_MAX_ENTRIES` |

## 7. 分里程碑接线清单（对照 plan §6）

- **M0**：`n3ri-llm/src/tools.rs` + 单测；`crates/n3ri-agent` 空壳 + workspace members 追加；
  `N3riUiPlugin` 或 minimal 加 `AgentPlugin`（二选一，默认 minimal，plan Q1）。
  不碰 dock/window/chat。
- **M1**：`world.rs`（view+snapshot）+ `apps/agent_bridge.rs`（view 写入）+ `prompt.rs`（context 块）+
  chat 被动轮追加调用。验收探针：`N3RI_AGENT_DEBUG=1` 时每 10s 打一行 context 摘要进 log（topbar 同风格）。
- **M2**：`scheduler.rs` + `turn.rs` 主动路径 + `agent_say_bridge` + `scheduler.json`。
  `DesktopState::Normal` 门需 `use n3ri_core::state::DesktopState`。
- **M3**：`memory.rs` 全量 + `HotMemory` 接管 `ChatHistory` + episodes/facts 落盘 + `recall_memory`（BM25，
  此时可先不接 tool-loop，只让模型在需要时由被动轮手动触发？不——M3 直接进 tool-loop，
  M4 只补 `open_app/notify`）。
- **M4**：`tools.rs` 全量 + `tools/niri.rs`（§4.6：windows/spawn/focus，只读先行，close 不暴露）+
  `AppLaunchEvent`/`NotificationEvent` reader（bridge 侧）+ `send_with_tools` 切主动/被动全量。
- **M5**：设置页“智能体”Tab。沿用 `LlmInput(i)` 模式但索引从 5 起（现有 0..4 为模型/音乐/搜索，
  需把 `llm_form: [String; 5]` 扩成 8 或另起 `AgentForm` resource——推荐另起，避免 `llm_config_from_form` 误读），
  开关组件复用 `spawn_toggle` 样式函数；`TextInputFocus::Settings(usize)` 天然支持新索引。

## 8. 测试

```bash
cargo test -p n3ri-llm        # tool wire 往返：tool_calls 解析、arguments 非法回退、schema 快照
cargo test -p n3ri-agent      # BM25 排序单测、门控纯函数（quota/cooldown/dedup）、rid 幂等、配额权重 math、
                              # daypart() 时段词、niri -j 投影解析 + 白名单拒绝（§4.6 样例来自本机实测）
cargo test -p n3ri-ui         # 现有窗口测试不受影响（promote/compact）
cargo run -p n3ri-minimal     # M1 起三场景走查：终端前台 / 音乐播放 / 壁纸模式
```

门控/配额/BM25 必须写成**纯函数**（输入 struct，输出 enum），不碰 ECS，方便单测——这是 N.E.K.O
把决策做成可测小函数的 Rust 翻版。

## 9. 失败与降级矩阵（抄 N.E.K.O §Privacy and failure behavior）

| 故障 | 行为 |
|---|---|
| LLM 不可达（被动轮） | 现有错误气泡（不动） |
| LLM 不可达（主动轮） | 静默丢弃，不弹错误气泡（must-fire 用静态模板） |
| 记忆 LLM（摘要/抽取/合成）失败 | 重试 ≤3 次指数退避，仍失败则下轮继续；`HotMemory` 保留原文 + 硬 cap（字符 60000，抄 `RECENT_HARD_CAP_TOKENS`） |
| 记忆文件损坏 | 当层视为空，不删文件，打 warn；`scheduler.json` 损坏 → 配额重置为 0（宁可多说一条，不可静默失忆） |
| recall 失败 | 空 hits 继续对话 |
| tool 参数非法 | envelope `is_error=true` 回模型自我纠正一次，再失败则一条道歉气泡 |
| `DesktopState` 非 Normal / typing 中 | 主动轮 Drop，不累积欠账（除 Break 外，Break 顺延 5min，抄 expiry） |

## 10. Do-not-touch（与 AGENTS.md 叠加）

1. 根 `Cargo.toml` 的 glslopt patch、workspace resolver/members 除追加 `crates/n3ri-agent` 外不动。
2. `vendor/bevy_live_wallpaper`（axis 补丁）、卫星行流协议（`x y / w dx dy / s w h`）不动；
   Agent 只消费聚合后的 `CursorPosition`。
3. Servo readback、Live2D RTT、`.cargo/config.toml` 不动。
4. `bevy = default-features=false + bevy_ui_render` 的 feature 拓扑不动；
   `n3ri-agent` 只取 render-free features。
5. 所有 UI 写操作保持 change-guard（`if **text != target` 类），`IsDragging` 手势互斥不绕过。
6. 物理/逻辑像素：新增命中一律 `cursor.physical` + `try_inverse`；样式读写一律 logical。

## 附录 A. N.E.K.O 出处索引（审计回查用）

- 记忆五层与 new_dialog 渲染：`reference/N.E.K.O/docs/architecture/memory-system.md`
- 自动 vs 按需召回边界、persona 不入池、BM25 阈值教训：同上 §Recall + `config/memory_settings.py:361`
- evidence 阈值/权重/半衰期：`config/memory_settings.py:29,48`
- activity 信号/状态机/门控退避：`main_logic/activity/{system_signals,tracker,state_machine,snapshot}.py`、
  `activity_guess_gate.py`
- 防打扰/配额/去重数字：`main_logic/topic/pipeline.py:64`、`config/proactive_settings.py`
- 两阶段 proactive 与 must-fire：`main_logic/proactive_chat/service.py`（经 subagent 摘要）
- tool 协议与 3 轮循环：`main_logic/tool_calling.py`、`main_logic/omni_offline_client/_tools.py`、
  `main_logic/core/tool_calling.py`（`recall_memory` 注册）
- 三进程/ZMQ/HUD（v1 砍掉的部分，留作 M5+ 参考）：`docs/architecture/{three-servers,agent-system,task-hud-system}.md`
