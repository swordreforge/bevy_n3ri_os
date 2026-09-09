//! The animated pet state: mocari runtime + motion players + breath.
//!
//! Frame flow mirrors the old Cubism Framework (`CubismUserModel::Update`),
//! mapped onto mocari's explicit API:
//!   1. tick motion players (advance clocks)
//!   2. reset parameters to defaults
//!   3. apply all motion players onto parameters / part opacities
//!   4. breath (additive sinusoidal idle sway, ported from live2d-motion)
//!   5. expression manager apply
//!   6. physics evaluate (internally clamps via model ranges)
//!   7. `update_meshes()` — mocari recomputes drawable vertices on CPU

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use mocari::{
    ExpressionManager,
    json::{Expression3, Motion3},
    motion::MotionPlayer,
    runtime::ModelRuntime,
};

use crate::renderer::{HeadDisplay, PetMapping, PetViewSize};

pub const IDLE_QUEUE_INDEX: usize = 0;
const SLEEP_TIMEOUT: f32 = 15.0;
const PET_COOLDOWN: f32 = 3.0;
const PETTING_EXPRESSIONS: &[&str] = &["02_Dizzy", "04_Shy", "07_Smile", "13_Happy"];

/// 表情自动回收时长（秒）。mocari/FFI 的 ExpressionManager 都没有"播完自动
/// 消失"语义——player 一旦 fade-in 完成就以 weight=1 常驻，覆盖对应参数直到
/// 下一次 play/stop。触发侧（摸头/AI 事件）只管 play 不管收，就会"一直不恢复"。
/// 这里触发后计时，到时自动 stop_all（0.5s 淡出），回到 idle 基线。
const EXPRESSION_HOLD_SECS: f32 = 3.0;

/// 动作切换交叉淡化时长（秒）。FFI 时代走 MotionQueueManager（queue 语义，
/// 新旧动作重叠混合）；mocari 的 MotionPlayer 没有 queue 语义（fade 秒数
/// 硬编码 0 → 首帧即全权重），单槽替换会瞬切。这里手动补回交叉淡化。
/// 时长取 model3.json 对应条目的 FadeInTime（sleep=10s / idle=5s），
/// 缺省回退 0.5s——与 FFI 时代 `unwrap_or(1.0)` 不同，0.5s 更接近 queue
/// 对缺省条目的实际体感（queue 的淡入按目标动作首段曲线斜率自然展开）。
pub const MOTION_CROSSFADE_FALLBACK_SECS: f32 = 0.5;

/// model3.json  motion 条目：动作 + 声明的淡入秒数（FFI 时代传给
/// CubismMotion::new 的 fade_in_time，mocari manifest 会丢掉， stash 在此）。
#[derive(Clone)]
pub struct PetMotion {
    pub motion: Motion3,
    pub fade_in_secs: f32,
}

/// 正在淡出的旧动作：与新动作并行 tick+apply，权重 start_w→0、新动作 0→1，
/// apply 顺序旧先新后，逐帧交叉混合。
struct FadingMotion {
    player: MotionPlayer,
    t: f32,
    dur: f32,
    start_w: f32,
}

/// 交叉淡化权重：旧 start_w→0、新 0→1（线性；0.5s 内人眼对线性/正弦无可辨
/// 差异，线性保证首帧即动、无 sine 头段"静止期"）。
fn crossfade_weights(t: f32, dur: f32, start_w: f32) -> (f32, f32) {
    let k = (t / dur).clamp(0.0, 1.0);
    (start_w * (1.0 - k), k)
}

/// Breath parameter definition, ported from live2d-motion's `Breath`
/// (LAppModel defaults). Adds subtle sinusoidal oscillation, applied as
/// additive deltas after motions.
struct BreathParam {
    id: &'static str,
    offset: f32,
    peak: f32,
    cycle: f32,
    weight: f32,
    prev_raw: f32,
}

pub(crate) struct BreathParamState {
    time: f32,
    params: Vec<BreathParam>,
}

impl BreathParamState {
    fn new() -> Self {
        Self {
            time: 0.0,
            params: vec![
                BreathParam { id: "ParamAngleX", offset: 0.0, peak: 15.0, cycle: 6.5345, weight: 0.5, prev_raw: 0.0 },
                BreathParam { id: "ParamAngleY", offset: 0.0, peak: 8.0, cycle: 3.5345, weight: 0.5, prev_raw: 0.0 },
                BreathParam { id: "ParamAngleZ", offset: 0.0, peak: 10.0, cycle: 5.5345, weight: 0.5, prev_raw: 0.0 },
                BreathParam { id: "ParamBodyAngleX", offset: 0.0, peak: 4.0, cycle: 15.5345, weight: 0.5, prev_raw: 0.0 },
                BreathParam { id: "ParamBreath", offset: 0.5, peak: 0.5, cycle: 3.2345, weight: 0.5, prev_raw: 0.0 },
            ],
        }
    }

    fn update(&mut self, dt: f32, runtime: &mut ModelRuntime) {
        self.time += dt;
        let t = self.time * 2.0 * std::f32::consts::PI;
        for param in &mut self.params {
            let raw = param.offset + param.peak * (t / param.cycle).sin();
            let delta = (raw - param.prev_raw) * param.weight;
            param.prev_raw = raw;
            if delta.abs() < 1e-7 {
                continue;
            }
            let Some(index) = runtime.parameter_index(param.id) else {
                continue;
            };
            let Some(current) = runtime.parameter_value_by_index(index) else {
                continue;
            };
            runtime.set_parameter_by_index(index, current + delta);
        }
    }
}

#[derive(Resource)]
pub struct IdleTimer {
    pub elapsed: f32,
    pub is_sleeping: bool,
}

impl Default for IdleTimer {
    fn default() -> Self {
        Self { elapsed: 0.0, is_sleeping: false }
    }
}

/// Head hit area in model coordinates (y-up, bbox `[-0.3, -0.6, 0.3, 0.9]`).
#[derive(Resource)]
pub struct HeadHitArea {
    pub min_y: f32,
    pub min_x: f32,
    pub max_x: f32,
}

impl Default for HeadHitArea {
    fn default() -> Self {
        Self {
            min_y: 0.2,
            min_x: -1.0,
            max_x: 1.0,
        }
    }
}

#[derive(Resource)]
pub struct PettingState {
    pub is_petting: bool,
    pub cooldown_timer: f32,
    pub last_mouse_pos: Option<Vec2>,
}

impl Default for PettingState {
    fn default() -> Self {
        Self {
            is_petting: false,
            cooldown_timer: 0.0,
            last_mouse_pos: None,
        }
    }
}

/// mocari runtime is fully `Send` (pure-Rust state, no FFI pointers) —
/// a plain `Resource`, touchable from any thread.
#[derive(Resource)]
pub struct Live2dPet {
    pub runtime: ModelRuntime,
    pub texture_paths: Vec<String>,

    pub(crate) players: Vec<MotionPlayer>,
    pub(crate) breath: BreathParamState,
    /// 切换中淡出的旧动作（None = 无切换，稳态单动作）。
    fading: Vec<FadingMotion>,

    pub idle_motion: Option<PetMotion>,
    pub sleep_motion: Option<PetMotion>,

    pub expressions: HashMap<String, Expression3>,
    pub expression_manager: ExpressionManager,
    /// 表情保持计时：start_expression 置为 HOLD 秒，每 tick 递减，
    /// 归零时自动 stop_all（淡出回基线）。None = 当前无激活表情。
    expression_hold: Option<f32>,
}

impl Live2dPet {
    pub(crate) fn new(
        runtime: ModelRuntime,
        texture_paths: Vec<String>,
        players: Vec<MotionPlayer>,
        idle_motion: Option<PetMotion>,
        sleep_motion: Option<PetMotion>,
        expressions: HashMap<String, Expression3>,
    ) -> Self {
        Self {
            runtime,
            texture_paths,
            players,
            breath: BreathParamState::new(),
            fading: Vec::new(),
            idle_motion,
            sleep_motion,
            expressions,
            expression_manager: ExpressionManager::new(),
            expression_hold: None,
        }
    }

    pub fn tick(&mut self, dt: f32) {
        if dt <= 0.0 {
            return;
        }

        for p in self.players.iter_mut() {
            p.tick(dt);
        }
        for f in self.fading.iter_mut() {
            f.player.tick(dt);
            f.t += dt;
        }
        self.expression_manager.tick(dt);

        // 表情自动回收：保持计时归零 → stop_all 淡出（0.5s）回基线。
        // 续播（新 start_expression）会重置计时，不会中途被收。
        if let Some(hold) = self.expression_hold.as_mut() {
            *hold -= dt;
            if *hold <= 0.0 {
                self.expression_hold = None;
                self.expression_manager.stop_all();
            }
        }

        // LoadParameters: restart the frame from model defaults so one-shot
        // and looping motions blend the same way every frame.
        self.runtime.reset_parameters();

        // 淡出栈先 apply（旧权重），再 apply 新动作——后写的覆盖/混合，
        // 与 FFI queue 的"新动作压住旧动作"语义一致。
        for f in self.fading.iter_mut() {
            let (old_w, _) = crossfade_weights(f.t, f.dur, f.start_w);
            f.player.set_weight(old_w);
            f.player.apply(&mut self.runtime);
        }
        // 新动作权重 0→1：有淡出栈时按最长者推进，无切换时恒 1。
        if let Some(f) = self.fading.iter().max_by(|a, b| {
            a.dur
                .partial_cmp(&b.dur)
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
            let (_, new_w) = crossfade_weights(f.t, f.dur, 1.0);
            self.players[IDLE_QUEUE_INDEX].set_weight(new_w);
        } else if !self.players.is_empty() {
            self.players[IDLE_QUEUE_INDEX].set_weight(1.0);
        }
        for p in self.players.iter() {
            p.apply(&mut self.runtime);
        }
        self.fading.retain(|f| f.t < f.dur);

        self.breath.update(dt, &mut self.runtime);

        self.expression_manager.apply(&mut self.runtime);

        self.runtime.apply_pose(dt);
        self.runtime.apply_physics(dt);

        self.runtime.update_meshes();
    }

    pub fn drawable_count(&self) -> usize {
        self.runtime.meshes().len()
    }

    fn play_idle_slot(&mut self, motion: Option<PetMotion>) {
        let Some(pet_motion) = motion else {
            return;
        };
        if self.players.is_empty() {
            self.players.push(MotionPlayer::new(pet_motion.motion.clone()));
        }
        // 旧动作进淡出栈（记录当前权重，与新动作交叉混合 dur 秒）；
        // 新动作权重从 0 起淡入。dur 取 model3.json 声明的 FadeInTime，
        // 与 FFI queue 语义一致（sleep=10s / idle=5s）。
        // 稳态（无切换）保持 weight=1 零开销。
        let old = std::mem::replace(
            &mut self.players[IDLE_QUEUE_INDEX],
            MotionPlayer::new(pet_motion.motion),
        );
        let start_w = old.weight();
        self.players[IDLE_QUEUE_INDEX].set_weight(0.0);
        self.fading.push(FadingMotion {
            player: old,
            t: 0.0,
            dur: pet_motion.fade_in_secs.max(0.0),
            start_w,
        });
    }

    pub fn switch_to_idle(&mut self) {
        self.play_idle_slot(self.idle_motion.clone());
    }

    pub fn switch_to_sleep(&mut self) {
        self.play_idle_slot(self.sleep_motion.clone());
    }

    pub fn start_expression(&mut self, name: &str) -> bool {
        if let Some(expr) = self.expressions.get(name).cloned() {
            self.expression_manager.play(expr);
            // 续播重置保持计时：3 秒无新触发才淡出回收。
            self.expression_hold = Some(EXPRESSION_HOLD_SECS);
            true
        } else {
            false
        }
    }

    pub fn clear_expression(&mut self) {
        self.expression_hold = None;
        self.expression_manager.stop_all();
    }

    /// 当前是否有激活表情（保持计时中或正在淡出）。
    pub fn has_active_expression(&self) -> bool {
        self.expression_hold.is_some() || !self.expression_manager.is_empty()
    }

    pub fn vertex_bbox(&self) -> [f32; 4] {
        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_y = f32::MIN;
        for m in self.runtime.meshes() {
            for v in m.vertices() {
                let [x, y] = v.position();
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
        if min_x > max_x {
            return [-1.0, -1.0, 1.0, 1.0];
        }
        [min_x, min_y, max_x, max_y]
    }
}

pub fn track_clicks(
    mouse: Res<ButtonInput<MouseButton>>,
    mut pet: ResMut<Live2dPet>,
    mut timer: ResMut<IdleTimer>,
) {
    if mouse.just_pressed(MouseButton::Left) || mouse.just_pressed(MouseButton::Right) {
        timer.elapsed = 0.0;
        if timer.is_sleeping {
            pet.switch_to_idle();
            timer.is_sleeping = false;
        }
    }
}

pub fn check_idle_timeout(
    mut pet: ResMut<Live2dPet>,
    time: Res<Time>,
    mut timer: ResMut<IdleTimer>,
) {
    timer.elapsed += time.delta_secs();
    if timer.elapsed >= SLEEP_TIMEOUT && !timer.is_sleeping {
        pet.switch_to_sleep();
        timer.is_sleeping = true;
    }
}

#[allow(clippy::too_many_arguments)]
pub fn detect_petting(
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    display: Query<(&ComputedNode, &UiGlobalTransform), With<crate::renderer::PetDisplayNode>>,
    mapping: Option<Res<PetMapping>>,
    view_size: Option<Res<PetViewSize>>,
    hit_area: Res<HeadHitArea>,
    mut state: ResMut<PettingState>,
    mut pet: ResMut<Live2dPet>,
    time: Res<Time>,
) {
    state.cooldown_timer = (state.cooldown_timer - time.delta_secs()).max(0.0);

    let Ok(window) = windows.single() else {
        return;
    };

    let Some(cursor) = window.cursor_position() else {
        state.last_mouse_pos = None;
        return;
    };

    let Some(mapping) = mapping else {
        return;
    };

    let Some(view_size) = view_size else {
        return;
    };

    // 以真实渲染节点矩形做命中检测（任意缩放/refit 结果下都正确；
    // 旧的手算 view×DISPLAY_SCALE 在非 1.5 缩放下与节点错位）
    let Ok((node, tf)) = display.single() else {
        return;
    };
    let size = node.size();
    if size.x <= 0.0 || size.y <= 0.0 {
        return;
    }
    let center = tf.to_scale_angle_translation().2;
    let rect_min = center - size * 0.5;

    let rtt_x = (cursor.x - rect_min.x) / size.x * view_size.w as f32;
    let rtt_y = (cursor.y - rect_min.y) / size.y * view_size.h as f32;

    let model_pos = mapping.inverse(rtt_x, rtt_y);

    let in_head = model_pos.y >= hit_area.min_y
        && model_pos.x >= hit_area.min_x
        && model_pos.x <= hit_area.max_x;

    if let Some(last) = state.last_mouse_pos {
        let velocity = (cursor - last).length();
        let is_moving = velocity > 2.0;

        if in_head && is_moving && state.cooldown_timer <= 0.0 && !state.is_petting {
            state.is_petting = true;
            state.cooldown_timer = PET_COOLDOWN;

            let expr_idx = (time.elapsed_secs() * 7.0) as usize % PETTING_EXPRESSIONS.len();
            let expr_name = PETTING_EXPRESSIONS[expr_idx];
            pet.start_expression(expr_name);
        }
    }

    if state.is_petting && !in_head {
        state.is_petting = false;
        pet.clear_expression();
    }

    state.last_mouse_pos = Some(cursor);
}

#[derive(Resource, Default)]
pub struct HeadPettingState {
    pub is_petting: bool,
    pub cooldown_timer: f32,
    pub last_mouse_pos: Option<Vec2>,
}

pub fn detect_head_petting(
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    head: Query<(&Node, &Visibility), With<HeadDisplay>>,
    mut state: ResMut<HeadPettingState>,
    mut pet: ResMut<Live2dPet>,
    time: Res<Time>,
) {
    state.cooldown_timer = (state.cooldown_timer - time.delta_secs()).max(0.0);

    let Ok(window) = windows.single() else {
        return;
    };

    let Some(cursor) = window.cursor_position() else {
        state.last_mouse_pos = None;
        return;
    };

    let Ok((node, vis)) = head.single() else {
        return;
    };

    if *vis == Visibility::Hidden {
        state.last_mouse_pos = None;
        return;
    }

    let screen_w = window.width();
    let screen_h = window.height();

    let node_w = match node.width {
        Val::Px(w) => w,
        _ => 150.0,
    };
    let node_h = match node.height {
        Val::Px(h) => h,
        _ => 150.0,
    };

    let right = match node.right {
        Val::Px(r) => r,
        _ => 24.0,
    };
    let bottom = match node.bottom {
        Val::Px(b) => b,
        _ => 64.0,
    };

    let node_left = screen_w - right - node_w;
    let node_top = screen_h - bottom - node_h;

    let in_head = cursor.x >= node_left
        && cursor.x <= node_left + node_w
        && cursor.y >= node_top
        && cursor.y <= node_top + node_h;

    if let Some(last) = state.last_mouse_pos {
        let velocity = (cursor - last).length();
        let is_moving = velocity > 2.0;

        if in_head && is_moving && state.cooldown_timer <= 0.0 && !state.is_petting {
            state.is_petting = true;
            state.cooldown_timer = PET_COOLDOWN;

            let expr_idx = (time.elapsed_secs() * 7.0) as usize % PETTING_EXPRESSIONS.len();
            let expr_name = PETTING_EXPRESSIONS[expr_idx];
            pet.start_expression(expr_name);
        }
    }

    if state.is_petting && !in_head {
        state.is_petting = false;
        pet.clear_expression();
    }

    state.last_mouse_pos = Some(cursor);
}
