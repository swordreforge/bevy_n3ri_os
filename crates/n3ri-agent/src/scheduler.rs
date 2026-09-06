//! 定时钩子调度（M2）：4 触发器 + 门控纯函数 + scheduler.json 持久化。
//!
//! 数字出处见 `docs/nori-agent-dev.md` §6 对照表；频次总纲（保守值）见 §3.2。
//! 门控/记账全部纯函数（不碰 ECS），方便单测。

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

use crate::config::AgentConfig;
use crate::world::{Activity, ContextSnapshot, Presence};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HookKind {
    Idle,
    Hourly,
    Break,
    Startup,
}

#[derive(Debug, Clone)]
pub struct HookFire {
    pub kind: HookKind,
    pub reason: String,
}

/// 门控拒绝码（N.E.K.O `reason_code` 词表子集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropReason {
    Disabled,
    NotDesktop,
    Busy,
    Closed,
    Restricted,
    Cooldown,
    Quota,
    MinGap,
    Duplicate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateResult {
    Pass,
    Drop(DropReason),
}

pub const STARTUP_GAP_SECS: f64 = 900.0;
pub const STARTUP_BURST_SECS: f64 = 1800.0;
pub const RESPONSE_WINDOW_SECS: f64 = 600.0;
pub const REASON_DEDUP_SECS: f64 = 48.0 * 3600.0;
pub const RECENT_REASONS_MAX: usize = 32;
pub const UNANSWERED_WEIGHT: f32 = 1.0 / 3.0;
pub const ANSWERED_WEIGHT: f32 = 1.0;

pub fn hook_cooldown_secs(kind: HookKind, cfg: &AgentConfig) -> f64 {
    match kind {
        HookKind::Idle => cfg.idle_cooldown_secs as f64,
        HookKind::Hourly => 3600.0,
        HookKind::Break => 14400.0,
        HookKind::Startup => STARTUP_BURST_SECS,
    }
}

fn reason_hash(reason: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    reason.hash(&mut h);
    h.finish()
}

/// 调度器状态（内存 + scheduler.json 持久化子集）。
#[derive(Resource, Debug, Clone, Default, Serialize, Deserialize)]
pub struct SchedulerState {
    pub day: String,
    pub weight_sum: f32,
    #[serde(default)]
    pub last_fire: HashMap<String, f64>,
    #[serde(default)]
    pub last_any_fire: f64,
    #[serde(default)]
    pub recent_reasons: VecDeque<(u64, f64)>,
    #[serde(default)]
    pub pending_upgrade: Option<(f64, f32)>,
    /// 运行期累计（不落盘）：break 专注计时、`last_hour` 整点沿。
    #[serde(skip)]
    pub focus_acc: f32,
    #[serde(skip)]
    pub last_hour: Option<u32>,
    #[serde(skip)]
    pub last_startup_greeting: f64,
    #[serde(skip)]
    pub last_session_at: f64,
}

impl SchedulerState {
    fn hook_key(kind: HookKind) -> &'static str {
        match kind {
            HookKind::Idle => "idle",
            HookKind::Hourly => "hourly",
            HookKind::Break => "break",
            HookKind::Startup => "startup",
        }
    }

    pub fn record_fire(&mut self, kind: HookKind, reason: &str, now: f64) {
        self.last_fire
            .insert(Self::hook_key(kind).to_string(), now);
        self.last_any_fire = now;
        if kind == HookKind::Startup {
            self.last_startup_greeting = now;
        }
        let h = reason_hash(reason);
        self.recent_reasons.retain(|(_, ts)| now - ts < REASON_DEDUP_SECS);
        if self.recent_reasons.iter().any(|(e, _)| *e == h) {
            return;
        }
        self.recent_reasons.push_back((h, now));
        while self.recent_reasons.len() > RECENT_REASONS_MAX {
            self.recent_reasons.pop_front();
        }
        self.pending_upgrade = Some((now, UNANSWERED_WEIGHT));
        self.weight_sum += UNANSWERED_WEIGHT;
    }

    /// 用户 10min 内回话 → 1/3 升级为 1.0（抄 `_TOPIC_RESPONSE_WINDOW_SECONDS`）。
    pub fn note_user_reply(&mut self, now: f64) {
        if let Some((fired_at, w)) = self.pending_upgrade.take() {
            if now - fired_at <= RESPONSE_WINDOW_SECS {
                self.weight_sum += ANSWERED_WEIGHT - w;
            }
        }
    }

    fn is_duplicate(&self, reason: &str, now: f64) -> bool {
        let h = reason_hash(reason);
        self.recent_reasons
            .iter()
            .any(|(e, ts)| *e == h && now - ts < REASON_DEDUP_SECS)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Propensity {
    Open,
    Closed,
    Restricted,
}

pub fn propensity(snap: &ContextSnapshot, typing_recent: bool) -> Propensity {
    if snap.activity == Activity::Immersive {
        Propensity::Restricted
    } else if typing_recent {
        Propensity::Closed
    } else {
        Propensity::Open
    }
}

#[allow(clippy::too_many_arguments)]
pub fn gate(
    kind: HookKind,
    reason: &str,
    snap: &ContextSnapshot,
    st: &SchedulerState,
    cfg: &AgentConfig,
    desktop_normal: bool,
    turn_pending: bool,
    typing_recent: bool,
    now: f64,
) -> GateResult {
    if !cfg.enabled {
        return GateResult::Drop(DropReason::Disabled);
    }
    if matches!(kind, HookKind::Hourly) && !cfg.hourly_chime {
        return GateResult::Drop(DropReason::Disabled);
    }
    if matches!(kind, HookKind::Break) && !cfg.break_reminder {
        return GateResult::Drop(DropReason::Disabled);
    }
    if !desktop_normal {
        return GateResult::Drop(DropReason::NotDesktop);
    }
    if turn_pending {
        return GateResult::Drop(DropReason::Busy);
    }
    match propensity(snap, typing_recent) {
        Propensity::Closed => return GateResult::Drop(DropReason::Closed),
        Propensity::Restricted
            if !matches!(kind, HookKind::Break | HookKind::Startup) =>
        {
            return GateResult::Drop(DropReason::Restricted)
        }
        _ => {}
    }
    if let Some(last) = st.last_fire.get(SchedulerState::hook_key(kind)) {
        if now - last < hook_cooldown_secs(kind, cfg) {
            return GateResult::Drop(DropReason::Cooldown);
        }
    }
    if st.weight_sum >= cfg.daily_quota {
        return GateResult::Drop(DropReason::Quota);
    }
    if st.last_any_fire > 0.0 && now - st.last_any_fire < cfg.min_gap_secs as f64 {
        return GateResult::Drop(DropReason::MinGap);
    }
    if st.is_duplicate(reason, now) {
        return GateResult::Drop(DropReason::Duplicate);
    }
    GateResult::Pass
}

/// 触发器扫描（纯逻辑部分）：按优先级 Startup > Break > Hourly > Idle 取首个。
/// 调用方负责 gate() 与 record_fire() + 落盘。
pub fn scan_triggers(
    snap: &ContextSnapshot,
    st: &mut SchedulerState,
    cfg: &AgentConfig,
    now: f64,
    hour: u32,
    desktop_normal: bool,
) -> Option<HookFire> {
    if !cfg.enabled || !desktop_normal {
        return None;
    }
    if snap.activity == Activity::FocusedWork {
        st.focus_acc += 1.0;
    } else {
        st.focus_acc = 0.0;
    }

    if st.last_session_at > 0.0
        && now - st.last_session_at >= STARTUP_GAP_SECS
        && now - st.last_startup_greeting >= STARTUP_BURST_SECS
    {
        let away_mins = ((now - st.last_session_at) / 60.0) as u32;
        st.last_session_at = 0.0;
        return Some(HookFire {
            kind: HookKind::Startup,
            reason: format!("用户离开约 {away_mins} 分钟后回来了，欢迎"),
        });
    }

    if cfg.break_reminder && st.focus_acc >= cfg.break_dwell_secs as f32 {
        st.focus_acc = 0.0;
        return Some(HookFire {
            kind: HookKind::Break,
            reason: "用户专注很久了，提醒休息".to_string(),
        });
    }

    if cfg.hourly_chime {
        if let Some(last) = st.last_hour {
            if hour != last && snap.presence == Presence::Active {
                st.last_hour = Some(hour);
                return Some(HookFire {
                    kind: HookKind::Hourly,
                    reason: "整点报时".to_string(),
                });
            }
        }
        st.last_hour = Some(hour);
    }

    if snap.idle_secs >= cfg.idle_threshold_secs as f32 {
        return Some(HookFire {
            kind: HookKind::Idle,
            reason: format!("用户挂机 {} 分钟了", (snap.idle_secs / 60.0) as u32),
        });
    }
    None
}

/// 进程启动时调用：记录本次会话起点（供 Startup 问候 gap 计算）。
/// 只在内存标记，不落盘（配额/冷却是按天持久化的，问候是按会话的）。
pub fn mark_session_start(st: &mut SchedulerState, now: f64) {
    if st.last_session_at == 0.0 {
        st.last_session_at = now;
    }
}

fn scheduler_file() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("n3ri_os")
        .join("agent")
        .join("scheduler.json")
}

pub fn load_scheduler() -> SchedulerState {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    match std::fs::read_to_string(scheduler_file()) {
        Ok(json) => match serde_json::from_str::<SchedulerState>(&json) {
            Ok(st) if st.day == today => st,
            Ok(_) => SchedulerState {
                day: today,
                ..Default::default()
            },
            Err(_) => SchedulerState {
                day: today,
                ..Default::default()
            },
        },
        Err(_) => SchedulerState {
            day: today,
            ..Default::default()
        },
    }
}

pub fn save_scheduler(st: &SchedulerState) {
    let path = scheduler_file();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_string_pretty(st) {
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, &json).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn scheduler_tick(
    time: Res<Time>,
    cfg: Res<AgentConfig>,
    snap: Res<ContextSnapshot>,
    mut st: ResMut<SchedulerState>,
    desktop: Option<Res<State<n3ri_core::state::DesktopState>>>,
    turn: Res<crate::turn::AgentTurn>,
    mut fire: MessageWriter<crate::turn::ProactiveFire>,
    mut acc: Local<f32>,
    mut saved_day: Local<String>,
) {
    acc_tick(
        &time,
        &cfg,
        &snap,
        &mut st,
        desktop.as_ref().map(|d| **d == n3ri_core::state::DesktopState::Normal),
        turn.pending,
        &mut fire,
        &mut acc,
        &mut saved_day,
    );
}

#[allow(clippy::too_many_arguments)]
fn acc_tick(
    time: &Time,
    cfg: &AgentConfig,
    snap: &ContextSnapshot,
    st: &mut SchedulerState,
    desktop_normal: Option<bool>,
    turn_pending: bool,
    fire: &mut MessageWriter<crate::turn::ProactiveFire>,
    acc: &mut f32,
    saved_day: &mut String,
) {
    *acc += time.delta_secs();
    if *acc < 1.0 {
        return;
    }
    *acc = 0.0;
    let now = time.elapsed_secs_f64();
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    if saved_day.is_empty() {
        *saved_day = st.day.clone();
    }
    if today != st.day {
        let session_marks = (st.last_startup_greeting, st.last_session_at);
        *st = SchedulerState {
            day: today.clone(),
            last_startup_greeting: session_marks.0,
            last_session_at: session_marks.1,
            ..Default::default()
        };
        *saved_day = today;
        save_scheduler(st);
    }

    let hour = chrono::Local::now().format("%H").to_string().parse().unwrap_or(0);
    let desktop_ok = desktop_normal.unwrap_or(true);
    let Some(found) = scan_triggers(snap, st, cfg, now, hour, desktop_ok) else {
        return;
    };
    let typing_recent = snap.idle_secs < 60.0 && snap.presence == Presence::Active;
    if gate(
        found.kind,
        &found.reason,
        snap,
        st,
        cfg,
        desktop_ok,
        turn_pending,
        typing_recent,
        now,
    ) != GateResult::Pass
    {
        return;
    }
    st.record_fire(found.kind, &found.reason, now);
    save_scheduler(st);
    fire.write(crate::turn::ProactiveFire {
        kind: found.kind,
        reason: found.reason,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> AgentConfig {
        AgentConfig::default()
    }

    fn snap() -> ContextSnapshot {
        ContextSnapshot::default()
    }

    #[test]
    fn quota_math() {
        let mut st = SchedulerState::default();
        st.record_fire(HookKind::Idle, "r1", 1000.0);
        assert!((st.weight_sum - UNANSWERED_WEIGHT).abs() < 1e-6);
        st.note_user_reply(1000.0 + RESPONSE_WINDOW_SECS);
        assert!((st.weight_sum - ANSWERED_WEIGHT).abs() < 1e-6);

        let mut st = SchedulerState::default();
        st.record_fire(HookKind::Idle, "r1", 1000.0);
        st.note_user_reply(1000.0 + RESPONSE_WINDOW_SECS + 1.0);
        assert!((st.weight_sum - UNANSWERED_WEIGHT).abs() < 1e-6);
    }

    #[test]
    fn cooldown_and_mingap() {
        let c = cfg();
        let s = snap();
        let mut st = SchedulerState::default();
        st.record_fire(HookKind::Idle, "r", 1000.0);
        assert_eq!(
            gate(
                HookKind::Idle,
                "other",
                &s,
                &st,
                &c,
                true,
                false,
                false,
                1000.0 + 100.0
            ),
            GateResult::Drop(DropReason::Cooldown)
        );
        let mut st = SchedulerState::default();
        st.record_fire(HookKind::Hourly, "r", 1000.0);
        assert_eq!(
            gate(
                HookKind::Idle,
                "other",
                &s,
                &st,
                &c,
                true,
                false,
                false,
                1000.0 + 100.0
            ),
            GateResult::Drop(DropReason::MinGap)
        );
    }

    #[test]
    fn quota_blocks() {
        let mut c = cfg();
        c.daily_quota = 1.0;
        let s = snap();
        let st = SchedulerState {
            weight_sum: 1.0,
            ..Default::default()
        };
        assert_eq!(
            gate(HookKind::Idle, "r", &s, &st, &c, true, false, false, 99999.0),
            GateResult::Drop(DropReason::Quota)
        );
    }

    #[test]
    fn busy_closed_restricted() {
        let c = cfg();
        let s = snap();
        let st = SchedulerState::default();
        assert_eq!(
            gate(HookKind::Idle, "r", &s, &st, &c, true, true, false, 0.0),
            GateResult::Drop(DropReason::Busy)
        );
        assert_eq!(
            gate(HookKind::Idle, "r", &s, &st, &c, true, false, true, 0.0),
            GateResult::Drop(DropReason::Closed)
        );
        let mut imm = snap();
        imm.activity = Activity::Immersive;
        assert_eq!(
            gate(HookKind::Idle, "r", &imm, &st, &c, true, false, false, 0.0),
            GateResult::Drop(DropReason::Restricted)
        );
        assert_eq!(
            gate(HookKind::Startup, "r", &imm, &st, &c, true, false, false, 0.0),
            GateResult::Pass
        );
        assert_eq!(
            gate(HookKind::Break, "r", &imm, &st, &c, true, false, false, 0.0),
            GateResult::Pass
        );
    }

    #[test]
    fn duplicate_reasons_dropped() {
        let c = cfg();
        let s = snap();
        let mut st = SchedulerState::default();
        st.record_fire(HookKind::Idle, "same", 1000.0);
        let mut st2 = SchedulerState {
            last_any_fire: 0.0,
            last_fire: HashMap::new(),
            ..st.clone()
        };
        st2.weight_sum = 0.0;
        assert_eq!(
            gate(HookKind::Idle, "same", &s, &st2, &c, true, false, false, 2000.0),
            GateResult::Drop(DropReason::Duplicate)
        );
    }

    #[test]
    fn startup_gap_and_burst() {
        let c = cfg();
        let s = snap();
        let mut st = SchedulerState {
            last_session_at: 1000.0,
            last_startup_greeting: 0.0,
            ..Default::default()
        };
        let f = scan_triggers(&s, &mut st, &c, 1000.0 + STARTUP_GAP_SECS, 12, true);
        assert!(matches!(f, Some(f) if f.kind == HookKind::Startup));

        let mut st = SchedulerState {
            last_session_at: 1000.0,
            last_startup_greeting: 1000.0 + STARTUP_GAP_SECS,
            ..Default::default()
        };
        let f = scan_triggers(&s, &mut st, &c, 1000.0 + STARTUP_GAP_SECS + 10.0, 12, true);
        assert!(f.map(|f| f.kind) != Some(HookKind::Startup));
    }

    #[test]
    fn disabled_switches() {
        let mut c = cfg();
        c.enabled = false;
        let s = snap();
        let st = SchedulerState::default();
        assert_eq!(
            gate(HookKind::Idle, "r", &s, &st, &c, true, false, false, 0.0),
            GateResult::Drop(DropReason::Disabled)
        );
        let mut c = cfg();
        c.hourly_chime = false;
        assert_eq!(
            gate(HookKind::Hourly, "r", &s, &st, &c, true, false, false, 0.0),
            GateResult::Drop(DropReason::Disabled)
        );
    }
}
