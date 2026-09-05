//! n3ri-agent — Nori 聊天智能体运行时（Bevy）。
//!
//! M0 空壳：只注册 `AgentPlugin` 与占位资源，不产生任何行为。
//! 后续里程碑按 `docs/nori-agent-dev.md` 逐模块填充：
//! M1 context/world+prompt，M2 scheduler+turn，M3 memory，M4 tools。

pub mod config;
pub mod memory;
pub mod prompt;
pub mod scheduler;
pub mod tools;
pub mod turn;
pub mod world;

pub use config::AgentConfig;
pub use scheduler::{HookKind, SchedulerState};
pub use world::{AgentWorldView, ContextSnapshot};

use bevy::prelude::*;

/// M0 空壳插件：资源初始化占位，system 在后续里程碑接入。
pub struct AgentPlugin;

impl Plugin for AgentPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AgentConfig>()
            .init_resource::<AgentWorldView>()
            .init_resource::<ContextSnapshot>()
            .init_resource::<SchedulerState>();
    }
}
