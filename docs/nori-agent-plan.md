# Nori Agent 计划文档（PLAN）

> 状态：草案，待审计。参考实现：`reference/N.E.K.O`（Apache-2.0，仅借鉴机制，不搬代码）。
> 对应开发文档：`docs/nori-agent-dev.md`（类型 / system / 存储 / 接入细节）。

## 1. 背景与目标

现在 `chat_capsule` 是一个“问一句、答一句”的同步问答：`ChatHistory` 内存保留 20 轮，
后台线程调 `LlmClient::send`，气泡逐句显示，情绪标签驱动 Live2D 表情。
它没有三样东西：

1. **上下文敏感**——不知道用户在干什么（哪个窗口在前台、挂机多久、几点、mus 是否在播、壁纸/窗口模式）。
2. **定时钩子**——不会主动开口（N.E.K.O 意义上的 proactive：idle 问候、整点报时、久坐提醒、回来欢迎）。
3. **分层记忆**——关进程即失忆（`ChatHistory` 是内存 `Resource`，重启清空；只有 `Nori_system_prompt.txt` 是持久人格）。

本计划的目标是加一个常驻智能体运行时 **`n3ri-agent`**（下文简称 Agent），做到：

- 被动轮复用同一 LLM 配置（`~/.config/n3ri_os/llm-config.json`）和同一 Nori 人格约束
  （默认 2~3 句、一句一行、末尾情绪标签、无 Emoji），不破坏现有聊天体验。
- 主动轮有完整的“感知 → 门控 → 生成 → 投递”链路，且**默认克制**（频次总纲见 dev §3.2：
  idle 阈值 30min、日配额 1.0、整点报时默认关、Immersive 必跳过——Nori 是安静等待型人格，
  主动开口是例外不是常态，与 N.E.K.O 的积极搭话刻意分歧），
  走 N.E.K.O 验证过的“关系与陪伴为核心，而不是任务自动化”定位。
- 记忆按**热/温/冷**三层落地，v1 先跑通文件 JSON + BM25 召回，不上 embedding/向量库，
  但 schema 预留升级位。

### 1.1 三个关键词的定义（本计划口径）

| 词 | 含义 | 非含义 |
|---|---|---|
| 上下文敏感 | 每次 LLM 请求自动带上轻量 `<context>` 系统块（前台窗口、可见窗口列表、idle 时长、时间、系统状态、音乐状态、模式） | 不是截屏视觉理解（v1 不做截图进 prompt） |
| 定时钩子 | 基于 `Time` 的 Tick 调度：idle 超时、整点、久坐、启动问候四种触发器 + 统一门控 | 不是 cron 表达式引擎 / 用户自定义脚本 |
| 冷温热分层 | 热=内存最近 N 轮原文；温=当日 JSONL + facts/reflections 可检索；冷=profile/persona + 月归档，只进 system 摘要 | 不是向量数据库；v1 无 embedding |

### 1.2 “herms / wayland” 的理解（审计重点）

- **herms**：理解为你在上一轮提到的 Hermes 类通用 Agent（N.E.K.O 文档原话：
  “以完成任务为目标的执行引擎”，N.E.K.O 把它当手脚经 A2A 调用）。
  本计划中 Hermes 是**可选的外部通道**，不是核心：v1 只做本地 tool（开应用、通知、音乐、查记忆）；
  网络型执行通道（browser_use / computer_use / OpenClaw/Hermes A2A）列为 M5 以后，不阻塞前面里程碑。
  如果你的 herms 指别的，请在审计时纠正。
- **wayland**：指壁纸模式（layer-shell surface + 卫星进程 XQueryPointer）下的常驻与 presence 感知。
  Agent 本体不直连 Wayland 协议，只读 `CursorPosition` / `UiArea` / 卫星帧推导的 presence，
  与窗口模式共用同一套 `ContextSnapshot`。

## 2. 现状审计（只列与 Agent 相关的集成点）

| 现有物 | 位置 | Agent 复用方式 | 约束 |
|---|---|---|---|
| `LlmClient::send`（阻塞）+ 后台线程 + `mpsc` 轮询 | `crates/n3ri-llm/src/lib.rs`，`chat_capsule.rs:741 chat_llm_dispatch/poll` | 主动轮复用同一调用范式（spawn 线程 → `Mutex<Receiver>` → `poll`）；不引入 async runtime | prompt 注入的上下文块必须藏在 system 里，不能让 Nori 破“2~3 句”约束 |
| `ChatHistory: Vec<Message>`（cap 20） | `chat_capsule.rs:87` | 提升为 Agent 拥有的热记忆，chat 只留显示；超限走摘要（抄 N.E.K.O `recent.json` memo 思路） | system prompt 单独存，不占历史条目（保持现状） |
| `ChatEmotionEvent` → Live2D 表情 | `chat_capsule.rs:677`，`main.rs:773 chat_emotion_bridge` | 主动消息同样走情绪解析 + 同一 Message，保证表情联动不断 | 情绪标签仍只出现一次、放末尾 |
| `Nori_system_prompt.txt`（磁盘优先、编译期兜底） | `assets/prompt/`，`chat_capsule.rs:712` | Agent 的 system 组装 = 世界观 prompt + `<context>` + 记忆段； prompt 改动热重载沿用磁盘优先 | prompt 总长度预算见开发文档 §3 |
| `AppLaunchEvent` / `NotificationEvent` | `n3ri-core/src/events.rs`，已 `add_message` 但**无 reader** | Agent 的动作出口：开应用、发通知直接写这两个 Message，但需补 reader（dock 侧或新 system） | dock 目前是直接 spawn 窗口，不走事件；二选一，不要双写 |
| `MusicCommand` | `n3ri-core/src/music.rs`，`settings.rs:2304` 写、`music_player.rs` 读 | Agent 音乐类 tool（下一首/查状态）复用它 | 只读 `MusicStatus` 做上下文，写命令走 tool |
| `TextInputOwner` / `FocusedTitle` / `AppWindow{z,app_id}` / `AppVisible` | `input_focus.rs`，`topbar.rs:52`，`window.rs:87` | 上下文快照的唯一窗口信息源 | 不得直查 `Window`，只读 `CursorPosition`/`UiArea`（cursor.rs 契约） |
| `CursorPosition{logical,physical,scale,active}` + `UiArea` | `cursor.rs` | idle 计时 + presence 推断的输入 | 物理/逻辑像素别混（bevy-0.19-dev-knowledge §5），命中用 physical，拖拽/样式用 logical |
| topbar 真实系统探针 | `topbar.rs`（/proc/stat、电池、pactl 线程） | 上下文中的 cpu/电量/网络/音量只读 topbar 已有状态，不另起探针 | 非 Linux 降级为 None |
| wallpaper 桥 + 卫星 `x y / w dx dy / s w h` 行流 | `wallpaper_bridge.rs`，`main.rs:257` | 壁纸模式 presence：卫星 pos 变化 = 有人；`pointer.last` 优先 | 卫星只报变化，需自己维护 `last_input` 时间戳 |
| 设置页模型 Tab（llm-config 表单+测试） | `settings.rs:1949` | 新增“智能体”Tab 沿用同一套 `LlmInput/TextInputOwner::Settings(i)` 模式 | focus 索引别与现有 0..5 冲突 |

## 3. 参考映射（N.E.K.O → n3ri-agent：借什么、砍什么）

详见 N.E.K.O 官方文档 `reference/N.E.K.O/docs/architecture/`（memory-system / agent-system /
session-management / three-servers / task-hud-system）。

| N.E.K.O 机制 | n3ri 对应 | v1 取舍 | 出处 |
|---|---|---|---|
| 五维记忆 recent/facts/reflections/persona + `new_dialog` 渲染 | 热/温/冷三层 + `build_agent_context()` | 合并维度（单用户、无群聊 scope），砍 embedding/向量、砍 evidence 半衰期为简化计数 | memory-system.md，`memory/*.py`，`config/memory_settings.py` |
| 自动上下文（persona+reflection+recent 进 system）vs 按需 `recall_memory` tool（BM25+cosine RRF，不含 persona，不 LLM rerank） | 照抄这条边界：自动段只放摘要，`recall_memory` 走 BM25（v1 纯 BM25，不上 cosine） | RRF 在只有一路时退化为 BM25 排序；top-K=4/总量 8 照抄 | memory-system.md §Recall |
| Activity 跟踪（5s 系统信号 + 20s 心跳 + 状态机 away/private/gaming/focused_work…+ propensity open/restricted/closed + skip dice） | `ContextSnapshot` 1s tick + 简化状态机（active/idle/away/focused_video? 先只做 active/idle/away + focused_work） | 砍屏幕截图视觉源、砍 LLM activity_guess 叙述、砍 gaming 检测（无探针） | activity/`tracker.py`、`state_machine.py`、`activity_guess_gate.py` |
| 防打扰五层：turn 互斥 → 播放 pacing → activity 门 → source 轮换 → topic 配额（2/天、min_gap 4h、48h 去重、权重 1/3） | `SchedulerState`：单 flight + 气泡占用检查 + propensity 门 + 每 hook 冷却 + topic 日配额 | 数字照抄（2/天、4h、48h、1/3），source 轮换 v1 只做字面去重 | topic/`pipeline.py:64`，proactive_settings |
| 两阶段 proactive（Phase-1 选源 → Phase-2 生成）+ must-fire（break/问候）短路 | v1 砍 Phase-1（无外部源可挑），must-fire 只保留启动问候 + 久坐提醒走短路模板 | 有外部源（热搜/音乐/梗图）之前不做 Phase-1 | `proactive_chat/service.py` |
| Tool 协议（`ToolDefinition` + Chat Completions `tool_calls` 循环 max 3 + 结果回填 `tool` role + 错误包 envelope 不抛异常） | `n3ri-llm` 加 wire 类型 + `ToolRegistry`（emoclass，无 Bevy 依赖） | 砍图片 tool、砍 realtime 方言、砍远程 plugin 通道 | tool_calling.py，`brain/task_executor.py`，`config/agent_settings.py` |
| Agent Server 独立进程 + ZMQ 三端口 + registry + Task HUD | 单进程内 `Message` + 内存 registry；HUD = 聊天气泡 + `NotificationEvent`，不做独立 HUD 页 | 砍 ZMQ/HTTP/多进程、砍 deferred task | three-servers.md，agent-system.md，task-hud-system.md |
| 启动问候（gap 15min 跳过 / burst 30min 去重 / variant 轮换 / 原子预约） | 照抄数字：`last_session` 文件 + gap/burst + 原子 `pending` 互斥 | 问候文案走正常 Nori 生成，不做模板 | `startup_greeting_policy.py` |
| 隐私与降级（本地存、处理走配置的云端、失败降级空结果、遥测 opt-out） | 同样的声明：记忆文件本地，LLM 处理走用户自己的 base_url；失败只打一次性气泡，不阻塞聊天 | 设置页给总开关 + 清除按钮 | memory-system.md §Privacy |

## 4. 总体架构（单进程三层）

```text
                    ┌──────────── n3ri-agent (NEW, Bevy) ────────────┐
                    │  context   scheduler   memory_store   tools    │
                    │    │           │            │            │      │
bevy world ─────────┼────┼───────────┼────────────┼────────────┼──────┤
 Cursor/UiArea ─────┼───►│           │            │            │      │
 FocusedTitle ──────┼───►│ ContextSnapshot (1s tick)          │      │
 AppWindow list ────┼───►│           │            │            │      │
 MusicStatus ───────┼───►│           │            │            │      │
                    │    │           ▼            │            │      │
                    │    │   AgentScheduler tick (1s)          │      │
                    │    │    hook fire? ──gates──► dispatch  │      │
                    │    │                        │           │      │
                    │    │                        ▼           │      │
                    │    │              build_agent_context() │      │
                    │    │              (worldview+context+   │      │
                    │    │               memory summary)      │      │
                    │    │                        │           │      │
                    │    │                        ▼           │      │
                    │    │              thread: LlmClient::send│      │
                    │    │              (tool loop ≤3)   ◄────┼──────┤
                    │    │                        │      recall_memory│
                    │    │                        ▼            │      │
                    │    │              ChatBubbleQueue / NotificationEvent
                    │    │              (+ ChatEmotionEvent → Live2D)
                    └────┼───────────────────┼────────────────┼──────┘
                         │                   │                │
              n3ri-llm (协议+client, 无Bevy)  │     n3ri-core (Messages)
              +ToolCall wire types            │     AppLaunch/Notification/Music
```

Crate 归属原则：`n3ri-llm` 保持无 Bevy（只加 tool wire 类型 + 解析函数，可单元测）；
所有 Bevy `Resource/System/Message` 进 `n3ri-agent`；`chat_capsule` 退化为表现层。

## 5. 非目标（明确不做）

1. 截图/视觉理解进 prompt（v1 无屏幕感知源；窗口标题即全部视觉上下文）。
2. embedding/向量召回、onnxruntime 本地模型（目录与字段预留，v1 BM25）。
3. 语音/Realtime/TTS（N.E.K.O 的 audio 路径整段不碰）。
4. 桌面操控（CUA 50 步循环）、浏览器自动化、OpenClaw/Hermes 外部通道（M5 之后）。
5. 多角色/群聊 scope、云同步、跨设备记忆（单用户单机）。
6. 用户自定义 hook 脚本/cron 表达式（只给 4 种内置触发器 + 常量调参）。
7. 独立 Task HUD 页面（v1 用气泡 + 通知即可）。

## 6. 里程碑与验收

### M0 — 协议与骨架（n3ri-llm tool wire + n3ri-agent 空壳）

- 工作：`ToolCall/ToolDef/ToolResult` 类型 + JSON 解析/序列化 + `recall_memory` 的 function schema；
  新 crate `n3ri-agent`（`lib.rs` + `AgentPlugin` 空实现）接入 workspace，接线到 `N3riUiPlugin` 或 minimal（待审计定）。
- 验收：`cargo test -p n3ri-llm` 有 tool 解析往返测试；`cargo run -p n3ri-minimal` 行为零变化；
  不动的 do-not-touch 项（glslopt patch、vendored wallpaper、Servo readback）零改动。

### M1 — 上下文敏感（ContextSnapshot + prompt 注入）

- 工作：`ContextSnapshot` resource（1s tick 聚合窗口/idle/时间/系统/音乐/模式）+
  `build_agent_context()` + 被动轮（用户说话时）自动带 `<context>` 块。
- 验收：开着终端/浏览器/音乐分别聊天，Nori 能正确说出“你在用终端”“音乐正播着”类感知回复；
  Nori 输出仍满足 2~3 句、一句一行、情绪标签末尾；上下文块不污染可见气泡；
  scale≠100% 下窗口判断仍正确（沿用 physical 命中规则）。

### M2 — 定时钩子（Scheduler + 4 触发器 + 门控）

- 工作：`IdleHook(默认 5min)`、`HourlyChime`、`BreakReminder(专注 30min)`、`StartupGreeting`；
  `SchedulerState`（单 flight、hook 冷却、topic 日配额 2、min_gap 4h、48h 去重、权重 1/3）；
  投递走气泡队列 + `ChatEmotionEvent`。
- 验收：挂机 5min 收到一条 idle 搭话（且 1h 内不重复）；专注（同一前台窗口 ≥30min）收到久坐提醒；
  重启 15min 内无问候、超 15min 有问候；游戏/视频类可预期地**不**打扰（propensity 门）；
  被动聊天进行中（`ChatLlmState::pending`）主动轮自动让路。

### M3 — 温冷记忆（文件存储 + 摘要 + BM25 召回）

- 工作：`~/.config/n3ri_os/agent/` 下 `episodes-YYYY-MM-DD.jsonl`、`facts.json`、`reflections.json`、
  `persona.json`、`profile.json`、`cursors.json`；turn 落盘 → 阈值摘要（20 轮触发、留 10）→
  fact 抽取（Stage-1 LLM）→ reflection 合成（unabsorbed≥5）→ persona 晋升（计数确认）；
  `recall_memory` tool 接 BM25。
- 验收：聊“记住我喜欢玩国际象棋”→ 删 `ChatHistory`（模拟重启）→ 问“你还记得我喜欢什么吗”，
  能经 recall 答对；`facts.json` 可读可审计；LLM 挂掉时聊天不受影响（降级空结果 + 错误只进气泡一次）；
  设置页有关闭记忆的开关（关 = 不写盘 + 不召回）。

### M4 — 工具与动作（registry + 3 内置 tool）

- 工作：`ToolRegistry`（注册/执行永不抛异常）+ `recall_memory` / `open_app` / `notify`（+ 可选 `music_next`）；
  Chat Completions tool 循环（max 3 + 强制终答）；`AppLaunchEvent`/`NotificationEvent` 补 reader。
- 验收：“帮我打开终端”→ 终端窗口真的打开；“提醒我喝水”→ 通知出现；
  tool 参数非法/目标应用不存在时模型能自我纠正一次，失败只进一条错误气泡；
  无确认的高危动作不存在（v1 tool 全是安全动作）。

### M5 — 打磨（设置页 + 配额调参 + 文档）

- 工作：设置页“智能体”Tab（总开关、idle 阈值、日配额、记忆开关、清除按钮、状态行）；
  常量集中到 `config.rs`（数字默认抄 N.E.K.O，见开发文档附录）；补 `docs/` 与 AGENTS.md 段落。
- 验收：所有开关即时生效且持久化；全量 `cargo test` + 手动三模式（窗口/壁纸/关 LLM）走查通过。

## 7. 配置与隐私

- 新增 `~/.config/n3ri_os/agent/config.json`（总开关默认开？建议默认开但钩子保守，待审计拍板）+
  同目录记忆文件；写盘沿用原子写（tmp + rename），与 `llm-config.json` 同风格。
- 隐私声明（抄 N.E.K.O 口径）：记忆**存储**在本地，但记忆**处理**（摘要/抽取/合成）会把相关文本
  发给用户自己配置的 LLM 服务；`recall_memory` 命中的片段会作为 tool 结果再发给对话模型。
  设置页必须有一行说明 + 一键清除。

## 8. 风险与开放问题（请审计时拍板）

1. **Q1**：`n3ri-agent` 挂在哪？建议 `N3riUiPlugin` 内 `add_plugins(AgentPlugin)`（与 ChatCapsule 同级），
   还是只在 `examples/minimal` 组装？前者方便复用窗口资源，后者保持 crates 解耦。
   **A1**:在 `N3riUiPlugin` 内 `add_plugins(AgentPlugin)`（与 ChatCapsule 同级）组装
2. **Q2**：`AppLaunchEvent`/`NotificationEvent` 目前无 reader，是给 dock 补 reader，还是 Agent 直接调
   `dock::request_*` 式函数？建议前者（消息语义干净），但要动 dock。
   **A2**：dock 补 reader
3. **Q3**：主动轮与被动轮共享一个 `pending` 互斥，还是各一个？建议共享（N.E.K.O turn 互斥），
   代价是主动轮可能饿死——用“被动优先 + 主动重试 60s”解决，是否接受？
   **A3**：建议共享，但是主动轮次预留接口
4. **Q4**：Hermes 外部通道是否真的要？v1 完全不做是否接受？如果要，优先 browser_use 还是 Hermes A2A？
   **A4**：并不需要,我们只是一个聊天，顺带做下记忆的agent
5. **Q5**：记忆默认开还是默认关？默认开体验完整但有隐私顾虑；N.E.K.O 默认开。建议默认开 + 首次在设置页提示。
   **A5**：默认开启 + 首次在设置页提示。
6. **风险**：LLM 后台任务（摘要/抽取）烧 token 不可见，需在设置页状态行显示“昨日记忆 token”计数吗？v1 可先只记次数。
   **A6**：采纳建议
7. **风险**：壁纸模式无主窗，`FocusedTitle` 更新路径与窗口模式是否一致？需实测，否则 presence 会误判。
   **A7**：理应一致，设计上也是一致的
