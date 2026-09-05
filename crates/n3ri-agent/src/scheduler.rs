//! 定时钩子调度（M0 占位）。

use bevy::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookKind {
    Idle,
    Hourly,
    Break,
    Startup,
}

#[derive(Resource, Debug, Clone, Default)]
pub struct SchedulerState {
    pub day: String,
    pub weight_sum: f32,
    pub last_any_fire: f64,
}
