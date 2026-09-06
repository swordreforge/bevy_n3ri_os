//! The animated pet state: Cubism model + motion queues + physics + breath.
//!
//! Frame flow mirrors the Cubism Framework (`CubismUserModel::Update`):
//!   1. advance motion queue clocks
//!   2. LoadParameters  (restore last frame's values)
//!   3. evaluate all motion queues onto parameters / part opacities
//!   4. breath (additive sinusoidal idle sway)
//!   5. physics evaluate
//!   6. clamp into parameter ranges, SaveParameters
//!   7. `model.update()` — Core recomputes drawable vertices

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use live2d_core::canvas::CanvasInfo;
use live2d_core::model::Model;
use live2d_motion::breath::Breath;
use live2d_motion::{ExpressionManager, ExpressionMotion};
use live2d_motion::motion::CubismMotion;
use live2d_motion::physics::{PhysicsEngine, PhysicsParams};
use live2d_motion::queue::MotionQueueManager;

use crate::renderer::{HeadDisplay, PetMapping, PetViewSize};

pub const IDLE_QUEUE_INDEX: usize = 0;
const SLEEP_TIMEOUT: f32 = 15.0;
const PET_COOLDOWN: f32 = 3.0;
const PETTING_EXPRESSIONS: &[&str] = &["02_Dizzy", "04_Shy", "07_Smile", "13_Happy"];

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

/// Non-Send (raw FFI pointers) — only touched from main-thread systems.
pub struct Live2dPet {
    pub model: Model<'static>,
    pub canvas: CanvasInfo,
    pub texture_paths: Vec<String>,

    pub(crate) param_ids: Vec<String>,
    pub(crate) param_lookup: HashMap<String, usize>,
    pub(crate) part_lookup: HashMap<String, usize>,
    pub(crate) mins: Vec<f32>,
    pub(crate) maxs: Vec<f32>,
    #[allow(dead_code)]
    pub(crate) defaults: Vec<f32>,

    pub(crate) saved_params: Vec<f32>,
    pub(crate) saved_parts: Vec<f32>,

    pub(crate) queues: Vec<MotionQueueManager>,
    pub(crate) physics: Option<PhysicsEngine>,
    pub(crate) breath: Breath,

    pub idle_motion: Option<CubismMotion>,
    pub sleep_motion: Option<CubismMotion>,

    pub expressions: HashMap<String, ExpressionMotion>,
    pub expression_manager: ExpressionManager,
}

impl Live2dPet {
    pub fn tick(&mut self, dt: f32, user_time: f32) {
        if dt <= 0.0 {
            return;
        }

        for q in self.queues.iter_mut() {
            q.advance_time(dt);
        }

        {
            let mut params = self.model.parameters();
            let mut parts = self.model.parts();
            let mut vals = params.values_mut();
            let pops = parts.opacities_mut();

            let v = vals.as_mut_slice();
            v.copy_from_slice(&self.saved_params);
            pops.copy_from_slice(&self.saved_parts);

            // 每 tick 一次的空切片：`do_update_motion` 只读，直接借用静态空切片，
            // 不再每帧堆分配两个 `Vec<String>`。
            let empty_ids: &[String] = &[];
            for q in self.queues.iter_mut() {
                q.do_update_motion(
                    &self.param_lookup,
                    v,
                    empty_ids,
                    empty_ids,
                    &self.part_lookup,
                    pops,
                );
            }

            self.breath.update(dt, v, &self.param_lookup);

            self.expression_manager.apply(&self.param_lookup, v, user_time);

            if let Some(physics) = self.physics.as_mut() {
                physics.evaluate(
                    &mut PhysicsParams {
                        values: v,
                        minimums: &self.mins,
                        maximums: &self.maxs,
                        defaults: &self.defaults,
                        names: &self.param_ids,
                    },
                    dt,
                );
            }

            for (i, val) in v.iter_mut().enumerate() {
                *val = val.clamp(self.mins[i], self.maxs[i]);
            }
            self.saved_params.copy_from_slice(v);
            self.saved_parts.copy_from_slice(pops);
        }

        self.model.reset_dynamic_flags();
        self.model.update();
    }

    pub fn drawable_count(&self) -> usize {
        self.model.drawables().len()
    }

    pub fn switch_to_idle(&mut self) {
        if let Some(motion) = self.idle_motion.clone() {
            self.queues[IDLE_QUEUE_INDEX].stop_all_motions();
            self.queues[IDLE_QUEUE_INDEX].start_motion(motion, None);
        }
    }

    pub fn switch_to_sleep(&mut self) {
        if let Some(motion) = self.sleep_motion.clone() {
            self.queues[IDLE_QUEUE_INDEX].stop_all_motions();
            self.queues[IDLE_QUEUE_INDEX].start_motion(motion, None);
        }
    }

    pub fn start_expression(&mut self, name: &str, user_time: f32) -> bool {
        if let Some(expr) = self.expressions.get(name) {
            self.expression_manager.start_expression(expr.clone(), user_time);
            true
        } else {
            false
        }
    }

    pub fn clear_expression(&mut self) {
        self.expression_manager.clear();
    }

    pub fn vertex_bbox(&self) -> [f32; 4] {
        let d = self.model.drawables();
        let counts = d.vertex_counts();
        let positions = d.vertex_positions();
        let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
        let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
        for i in 0..d.len() {
            let ptr = positions[i];
            for vi in 0..counts[i] as usize {
                let p = unsafe { *ptr.add(vi) };
                min_x = min_x.min(p.X);
                min_y = min_y.min(p.Y);
                max_x = max_x.max(p.X);
                max_y = max_y.max(p.Y);
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
    mut pet: NonSendMut<Live2dPet>,
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
    mut pet: NonSendMut<Live2dPet>,
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
    mut pet: NonSendMut<Live2dPet>,
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
            pet.start_expression(expr_name, time.elapsed_secs());
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
    mut pet: NonSendMut<Live2dPet>,
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
            pet.start_expression(expr_name, time.elapsed_secs());
        }
    }

    if state.is_petting && !in_head {
        state.is_petting = false;
        pet.clear_expression();
    }

    state.last_mouse_pos = Some(cursor);
}
