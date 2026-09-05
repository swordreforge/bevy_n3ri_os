//! Turn 执行（M2）：单 flight 后台线程，抄 `chat_llm_dispatch/poll` 范式。
//!
//! - 被动轮（用户说话）仍走 `chat_capsule` 的 `ChatLlmState` 通道；本模块只跑**主动轮**。
//! - `ProactiveFire`（scheduler 写）→ `proactive_intake`（60s 有效窗 + 被动优先）
//!   → `proactive_dispatch`（起线程）→ `proactive_poll`（收结果进投递桥）。

use std::collections::VecDeque;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Mutex;
use std::thread;

use bevy::prelude::*;
use n3ri_llm::{LlmClient, Message};

use crate::config::AgentConfig;
use crate::emotion::split_sentences;
use crate::memory::{build_memory_block, handle_recall_memory, HotMemory, MemoryStore};
use crate::prompt::build_context_block;
use crate::scheduler::HookKind;
use crate::world::{AgentWorldView, ContextSnapshot};

pub const PROACTIVE_WAIT_SECS: f64 = 60.0;
pub const PROACTIVE_MAX_QUEUE: usize = 4;
pub const TOOL_LOOP_MAX: usize = 3;

pub const PROACTIVE_INSTRUCTION: &str =
    "这是你主动开口，只说 2~3 句，一句一行，保持 Nori 口吻，末尾照常带且仅带一个情绪标签。";

pub const BREAK_FALLBACK: &str = "专注很久啦，喝口水休息一下呀。\n起来活动一下再回来吧。";
pub const STARTUP_FALLBACK: &str = "你回来啦！\nNori真的等你好久了呀。";

/// scheduler → turn 的点火消息（turn 侧排队，被动优先）。
#[derive(Message, Debug, Clone)]
pub struct ProactiveFire {
    pub kind: HookKind,
    pub reason: String,
}

/// 主动 turn 的单 flight 状态（与被动 `pending` 互斥检查，见 `PassivePending`）。
#[derive(Resource, Default)]
pub struct AgentTurn {
    pub pending: bool,
    pub rx: Option<Mutex<Receiver<TurnOutput>>>,
    pub origin: Option<HookKind>,
}

/// 被动轮占用标记：`n3ri-ui` 侧 bridge 每帧镜像 `ChatLlmState::pending`。
#[derive(Resource, Default)]
pub struct PassivePending(pub bool);

/// Agent → UI 的投递桥（`n3ri-ui` 侧 `agent_outbox_bridge` 消费进气泡/情绪）。
#[derive(Resource, Default)]
pub struct AgentOutbox {
    pub sentences: Vec<String>,
    pub emotion: Option<String>,
    pub(crate) dirty: bool,
}

impl AgentOutbox {
    pub fn dirty(&self) -> bool {
        self.dirty
    }

    pub fn take_out(&mut self) -> (Vec<String>, Option<String>) {
        self.dirty = false;
        (
            std::mem::take(&mut self.sentences),
            self.emotion.take(),
        )
    }

    fn push(&mut self, sentences: Vec<String>, emotion: Option<String>) {
        self.sentences.extend(sentences);
        if emotion.is_some() {
            self.emotion = emotion;
        }
        self.dirty = true;
    }
}

#[derive(Debug, Clone)]
struct QueuedFire {
    kind: HookKind,
    reason: String,
    fired_at: f64,
}

#[derive(Resource, Default)]
struct ProactiveQueue {
    items: VecDeque<QueuedFire>,
}

pub struct TurnOutput {
    pub kind: HookKind,
    pub text: Result<String, String>,
}

/// chat 被动侧在用户提交时调用：配额升级（10min 窗口）由 scheduler 状态机记账。
pub fn note_user_reply_now(st: &mut crate::scheduler::SchedulerState, now: f64) {
    st.note_user_reply(now);
}

fn proactive_intake(
    time: Res<Time>,
    mut fires: MessageReader<ProactiveFire>,
    mut queue: ResMut<ProactiveQueue>,
) {
    let now = time.elapsed_secs_f64();
    for f in fires.read() {
        queue.items.retain(|q| now - q.fired_at < PROACTIVE_WAIT_SECS);
        if queue.items.len() >= PROACTIVE_MAX_QUEUE {
            queue.items.pop_front();
        }
        if queue.items.iter().any(|q| q.kind == f.kind) {
            continue;
        }
        queue.items.push_back(QueuedFire {
            kind: f.kind,
            reason: f.reason.clone(),
            fired_at: now,
        });
    }
}

pub fn build_proactive_system(
    kind: HookKind,
    reason: &str,
    world_prompt: &str,
    view: &AgentWorldView,
    snap: &ContextSnapshot,
    memory: Option<&str>,
) -> String {
    let ctx = build_context_block(view, snap);
    let hook_line = match kind {
        HookKind::Idle => format!("【主动契机：{reason}，轻轻搭句话】"),
        HookKind::Hourly => format!("【主动契机：{reason}，整点报时顺带一句陪伴】"),
        HookKind::Break => format!("【主动契机：{reason}，提醒休息】"),
        HookKind::Startup => format!("【主动契机：{reason}】"),
    };
    match memory {
        Some(m) if !m.is_empty() => {
            format!("{world_prompt}\n{ctx}\n{m}\n{hook_line}\n{PROACTIVE_INSTRUCTION}")
        }
        _ => format!("{world_prompt}\n{ctx}\n{hook_line}\n{PROACTIVE_INSTRUCTION}"),
    }
}

fn fallback_for(kind: HookKind) -> Option<&'static str> {
    match kind {
        HookKind::Break => Some(BREAK_FALLBACK),
        HookKind::Startup => Some(STARTUP_FALLBACK),
        _ => None,
    }
}

fn load_world_prompt() -> String {
    std::fs::read_to_string("assets/prompt/Nori_system_prompt.txt")
        .or_else(|_| std::fs::read_to_string("../../assets/prompt/Nori_system_prompt.txt"))
        .unwrap_or_else(|_| "你是 Nori,一个被困在蓝色数字空间里的白发 AI 女孩。".to_string())
}

/// recall tool-loop（被动/主动共用）：`send_with_tools` → 执行 `recall_memory`
/// → 回填 → 终答。max 3 轮；3 轮用完强制去 tools 拿终答。永不抛错。
pub fn run_recall_loop(
    client: &LlmClient,
    history: &[Message],
    config: &n3ri_llm::LlmConfig,
    store: &MemoryStore,
) -> Result<String, String> {
    use n3ri_llm::{assistant_to_wire, tool_to_wire};
    let recall = n3ri_llm::ToolDef {
        name: "recall_memory",
        description: "检索 Nori 的温记忆（facts/reflections）。想不起用户的事时调用。",
        parameters: n3ri_llm::recall_memory_schema(),
    };
    let tools = [recall];
    let wire_history: Vec<serde_json::Value> = history
        .iter()
        .map(|m| {
            serde_json::json!({
                "role": serde_json::to_value(m.role).unwrap_or_default(),
                "content": m.content,
            })
        })
        .collect();
    let mut wire = wire_history;
    let mut final_text: Option<String> = None;
    for _ in 0..TOOL_LOOP_MAX {
        let assistant = client.send_with_tools_wire(&wire, config, &tools)?;
        if assistant.tool_calls.is_empty() {
            final_text = Some(assistant.content);
            break;
        }
        wire.push(assistant_to_wire(&assistant));
        for call in &assistant.tool_calls {
            let env = if call.name == "recall_memory" {
                handle_recall_memory(store, &call.arguments)
            } else {
                n3ri_llm::ToolEnvelope::err(format!("未知工具 {}", call.name))
            };
            wire.push(tool_to_wire(call, &env));
        }
        if !assistant.content.trim().is_empty() {
            final_text = Some(assistant.content.clone());
        }
    }
    match final_text {
        Some(t) if !t.trim().is_empty() => Ok(t),
        _ => {
            let assistant = client.send_with_tools_wire(&wire, config, &[])?;
            if assistant.content.trim().is_empty() {
                Err("模型返回空 content".to_string())
            } else {
                Ok(assistant.content)
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn proactive_dispatch(
    time: Res<Time>,
    cfg: Res<AgentConfig>,
    mut turn: ResMut<AgentTurn>,
    mut queue: ResMut<ProactiveQueue>,
    view: Res<AgentWorldView>,
    snap: Res<ContextSnapshot>,
    hot: Res<HotMemory>,
    store: Res<crate::memory::MemoryStoreRes>,
    passive: Res<PassivePending>,
) {
    if turn.pending || !cfg.enabled {
        return;
    }
    if passive.0 {
        queue.items.retain(|q| time.elapsed_secs_f64() - q.fired_at < PROACTIVE_WAIT_SECS);
        return;
    }
    let now = time.elapsed_secs_f64();
    queue.items.retain(|q| now - q.fired_at < PROACTIVE_WAIT_SECS);
    let Some(next) = queue.items.pop_front() else {
        return;
    };

    let world_prompt = load_world_prompt();
    let mem_block = build_memory_block(&store.store, &hot);
    let mem_opt = if mem_block.is_empty() {
        None
    } else {
        Some(mem_block.as_str())
    };
    let system =
        build_proactive_system(next.kind, &next.reason, &world_prompt, &view, &snap, mem_opt);
    let history = vec![
        Message::system(system),
        Message::user("（等待你的主动开口）"),
    ];
    let (tx, rx) = channel();
    let client = LlmClient::new();
    let config = n3ri_llm::load_config();
    let kind = next.kind;
    thread::spawn(move || {
        let store = MemoryStore::load();
        let text = run_recall_loop(&client, &history, &config, &store);
        let _ = tx.send(TurnOutput { kind, text });
    });
    turn.rx = Some(Mutex::new(rx));
    turn.pending = true;
    turn.origin = Some(kind);
}

fn proactive_poll(mut turn: ResMut<AgentTurn>, mut outbox: ResMut<AgentOutbox>) {
    if !turn.pending {
        return;
    }
    let Some(rx) = turn.rx.as_ref() else {
        turn.pending = false;
        return;
    };
    let recv = rx.lock().unwrap().try_recv();
    match recv {
        Ok(out) => {
            turn.pending = false;
            turn.rx = None;
            turn.origin = None;
            match out.text {
                Ok(response) => {
                    let (cleaned, emotion) = crate::emotion::extract_emotion(&response);
                    let mut sentences = split_sentences(&cleaned);
                    if sentences.is_empty() && !cleaned.trim().is_empty() {
                        sentences.push(cleaned.trim().to_string());
                    }
                    if sentences.is_empty() {
                        sentences.push("……".to_string());
                    }
                    outbox.push(sentences, emotion);
                }
                Err(_) => {
                    if let Some(fb) = fallback_for(out.kind) {
                        outbox.push(split_sentences(fb), None);
                    }
                }
            }
        }
        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
            turn.pending = false;
            turn.rx = None;
            turn.origin = None;
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => {}
    }
}

pub struct TurnPlugin;

impl Plugin for TurnPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AgentTurn>()
            .init_resource::<PassivePending>()
            .init_resource::<AgentOutbox>()
            .init_resource::<ProactiveQueue>()
            .add_message::<ProactiveFire>()
            .add_systems(Update, (proactive_intake, proactive_dispatch, proactive_poll));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proactive_system_contains_all_parts() {
        let view = AgentWorldView {
            now_local: "2026-09-05 周五 14:03".into(),
            ..Default::default()
        };
        let snap = ContextSnapshot::default();
        let s = build_proactive_system(
            HookKind::Idle,
            "用户挂机 32 分钟了",
            "世界观",
            &view,
            &snap,
            None,
        );
        assert!(s.contains("世界观"));
        assert!(s.contains("<context>"));
        assert!(s.contains("用户挂机 32 分钟了"));
        assert!(s.contains(PROACTIVE_INSTRUCTION));
    }

    #[test]
    fn fallbacks_only_for_must_fire() {
        assert!(fallback_for(HookKind::Break).is_some());
        assert!(fallback_for(HookKind::Startup).is_some());
        assert!(fallback_for(HookKind::Idle).is_none());
        assert!(fallback_for(HookKind::Hourly).is_none());
    }
}
