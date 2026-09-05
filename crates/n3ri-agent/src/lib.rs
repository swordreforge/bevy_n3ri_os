//! n3ri-agent — Nori 聊天智能体运行时（Bevy）。
//!
//! M0 空壳：只注册 `AgentPlugin` 与占位资源，不产生任何行为。
//! 后续里程碑按 `docs/nori-agent-dev.md` 逐模块填充：
//! M1 context/world+prompt，M2 scheduler+turn，M3 memory，M4 tools。

pub mod config;
pub mod emotion;
pub mod memory;
pub mod prompt;
pub mod scheduler;
pub mod tools;
pub mod turn;
pub mod world;

pub use config::AgentConfig;
pub use emotion::{extract_emotion, split_sentences};
pub use prompt::{append_context, build_context_block, daypart};
pub use scheduler::{
    load_scheduler, mark_session_start, save_scheduler, scheduler_tick, GateResult, HookKind,
    SchedulerState,
};
pub use turn::{AgentOutbox, AgentTurn, PassivePending, ProactiveFire, TurnPlugin};
pub use world::{
    classify_activity, classify_presence, context_tick, is_transitioning, AgentWorldView,
    ContextSnapshot, OutsideView, OutsideWindow, WallpaperMode, WindowInfo,
};

use bevy::prelude::*;

/// M2：scheduler tick + turn 执行接入；memory/tools 在后续里程碑接入。
pub struct AgentPlugin;

impl Plugin for AgentPlugin {
    fn build(&self, app: &mut App) {
        let sched = load_scheduler();
        app.init_resource::<AgentConfig>()
            .init_resource::<WallpaperMode>()
            .init_resource::<AgentWorldView>()
            .init_resource::<ContextSnapshot>()
            .insert_resource(sched)
            .add_systems(Update, context_tick)
            .add_plugins(TurnPlugin)
            .add_systems(Update, scheduler_tick.after(context_tick));
    }
}
