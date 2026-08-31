//! 凑近状态（Focus Mode）：打开特定游戏窗口时，将主窗口重排到左侧 4/7，
//! 隐藏 Dock 与最小化/最大化按钮，整体推镜（背景 + Live2D 人物一起缩放右移），
//! 人物凑近取景：头顶贴屏顶、裙摆下缘贴屏底（上半身 4 份 / 腿部 3 份），
//! 腿部自然落到屏幕外；关闭该窗口时退出并恢复原状。
//!
//! 本模块位于 examples/minimal（而非 n3ri-ui），因为只有该 crate 同时依赖
//! n3ri-ui 与 n3ri-live2d，可访问 Dock / CinematicLocked / PetDisplayNode /
//! DesktopBackgroundMaterial 等。
//!
//! 锁定方案：进入凑近后给窗口实体插入 `CinematicLocked`，window.rs / resize.rs /
//! snap.rs 的系统遇到该组件即跳过（拖拽、缩放、最小化、最大化、吸附全部失效），
//! 仅保留关闭按钮 —— 关闭窗口（despawn）即触发退出并恢复原状。

use bevy::ecs::relationship::Relationship;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use n3ri_live2d::PetDisplayNode;
use n3ri_ui::desktop::{DesktopBackground, DesktopBackgroundMaterial};
use n3ri_ui::dock::Dock;
use n3ri_ui::window::{AppWindow, CinematicLocked, MaximizeButton, MinimizeButton};

/// 触发凑近状态的游戏 app_id 列表（后续游戏模式追加到此处）
const GAME_APP_IDS: &[&str] = &["chess", "pictionary", "codenames", "cakeduel"];

/// 凑近动画时长（秒）
const FOCUS_DURATION: f32 = 0.6;

/// 人物取景比例：上半身（头→裙摆下缘）4 份，腿部 3 份。
/// 凑近帧 = 头顶贴屏顶、裙摆下缘贴屏底，腿部在屏幕外（纯放大，不裁剪）。
const UPPER_BODY_PARTS: f32 = 4.0;
const LOWER_BODY_PARTS: f32 = 3.0;

/// 人物在节点内的垂直位置（renderer 的 PetMapping 将人物 bbox 垂直居中，
/// 高度占节点 FIT_MARGIN_Y=0.94 → 头顶在 3% 处，脚底在 97% 处）
const PET_HEAD_RATIO: f32 = 0.03;
const PET_FULL_RATIO: f32 = 0.94;

/// 背景推镜：放大倍数与内容偏移（uv 空间，offset.x 负值 → 内容右移）
const BG_FOCUS_ZOOM: f32 = 1.12;
const BG_FOCUS_OFFSET: Vec2 = Vec2::new(-0.05, 0.0);

const TITLE_BAR_HEIGHT: f32 = 32.0;

pub struct FocusPlugin;

impl Plugin for FocusPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FocusState>()
            .add_systems(Update, (focus_watch, focus_animate));
    }
}

#[derive(Default, PartialEq, Clone, Copy)]
enum FocusPhase {
    #[default]
    None,
    Entering,
    Active,
    Exiting,
}

#[derive(Resource, Default)]
pub struct FocusState {
    phase: FocusPhase,
    progress: f32,
    window: Option<Entity>,
    app_id: Option<String>,
    saved_window_node: Option<Node>,
    saved_pet_node: Option<Node>,
    saved_dock_visibility: Option<Visibility>,
    saved_bg_zoom: f32,
    saved_bg_offset: Vec2,
}

impl FocusState {
    fn is_active(&self) -> bool {
        self.phase != FocusPhase::None
    }
}

/// 1. 触发/退出：新增游戏窗口 → 进入凑近；目标窗口 despawn → 开始退出
fn focus_watch(
    mut commands: Commands,
    mut state: ResMut<FocusState>,
    added_windows: Query<(Entity, &AppWindow), Added<AppWindow>>,
    mut removed: RemovedComponents<AppWindow>,
    window_query: Query<Entity, With<AppWindow>>,
    node_query: Query<&Node, With<AppWindow>>,
    pet_query: Query<(Entity, &Node), With<PetDisplayNode>>,
    dock_query: Query<(Entity, &Visibility), With<Dock>>,
    child_of_query: Query<&ChildOf>,
    buttons: Query<(Entity, &ChildOf), Or<(With<MinimizeButton>, With<MaximizeButton>)>>,
    mut bg_materials: ResMut<Assets<DesktopBackgroundMaterial>>,
    bg_nodes: Query<&MaterialNode<DesktopBackgroundMaterial>, With<DesktopBackground>>,
) {
    // ── 进入 ──────────────────────────────────────────────────────────
    if !state.is_active() {
        if let Some((entity, app)) = added_windows
            .iter()
            .find(|(_, a)| GAME_APP_IDS.contains(&a.app_id.as_str()))
        {
            // 保存窗口原布局
            let saved_window_node = node_query.get(entity).ok().cloned();

            // 保存人物节点原布局（flex 居中，left/top 为 Auto —— 退出时原样恢复）
            let pet_node = match pet_query.single() {
                Ok((_, n)) => n.clone(),
                Err(_) => return,
            };

            // 保存 Dock 可见性并隐藏
            let saved_dock_visibility = dock_query.single().ok().map(|(d, vis)| {
                let saved = *vis;
                commands.entity(d).insert(Visibility::Hidden);
                saved
            });

            // 保存背景 material 并设为推镜目标
            let (saved_bg_zoom, saved_bg_offset) =
                if let Some(mut mat) = bg_nodes
                    .single()
                    .ok()
                    .and_then(|mn| bg_materials.get_mut(&mn.0))
                {
                    let saved = (mat.zoom, mat.offset);
                    mat.zoom = BG_FOCUS_ZOOM;
                    mat.offset = BG_FOCUS_OFFSET;
                    saved
                } else {
                    (1.0, Vec2::ZERO)
                };

            // 锁定窗口（不可拖拽/缩放/最小化/最大化/吸附）
            commands.entity(entity).insert(CinematicLocked);

            // 隐藏该窗口的最小化/最大化按钮（仅保留关闭按钮）
            for (button, first_parent) in buttons.iter() {
                if belongs_to_window(button, first_parent, entity, &child_of_query) {
                    commands.entity(button).insert(Visibility::Hidden);
                }
            }

            state.phase = FocusPhase::Entering;
            state.progress = 0.0;
            state.window = Some(entity);
            state.app_id = Some(app.app_id.clone());
            state.saved_window_node = saved_window_node;
            state.saved_pet_node = Some(pet_node);
            state.saved_dock_visibility = saved_dock_visibility;
            state.saved_bg_zoom = saved_bg_zoom;
            state.saved_bg_offset = saved_bg_offset;
            return;
        }
    }

    // ── 退出：目标窗口被关闭（despawn） ──────────────────────────────
    if state.is_active() && state.phase != FocusPhase::Exiting {
        let window_gone = state
            .window
            .map(|w| window_query.get(w).is_err())
            .unwrap_or(false)
            || removed.read().any(|e| Some(e) == state.window);
        if window_gone {
            state.phase = FocusPhase::Exiting;
            state.progress = 1.0;
        }
    }
}

/// 判断按钮实体是否属于目标窗口（沿 ChildOf 链上溯）
fn belongs_to_window(
    _entity: Entity,
    first_parent: &ChildOf,
    window: Entity,
    child_of_query: &Query<&ChildOf>,
) -> bool {
    let mut current = first_parent.get();
    if current == window {
        return true;
    }
    loop {
        match child_of_query.get(current) {
            Ok(parent) => {
                current = parent.get();
                if current == window {
                    return true;
                }
            }
            Err(_) => return false,
        }
    }
}

/// 2. 每帧动画：progress 0→1 进入，1→0 退出；应用窗口重排、人物节点、背景推镜
#[allow(clippy::too_many_arguments)]
fn focus_animate(
    time: Res<Time>,
    mut state: ResMut<FocusState>,
    mut window_nodes: Query<(Entity, &mut Node), (With<AppWindow>, Without<PetDisplayNode>)>,
    mut pet_nodes: Query<(Entity, &mut Node), (With<PetDisplayNode>, Without<AppWindow>)>,
    mut bg_materials: ResMut<Assets<DesktopBackgroundMaterial>>,
    bg_nodes: Query<&MaterialNode<DesktopBackgroundMaterial>, With<DesktopBackground>>,
    dock_query: Query<Entity, With<Dock>>,
    primary_window: Query<&Window, With<PrimaryWindow>>,
    mut commands: Commands,
) {
    let phase = state.phase;
    if phase == FocusPhase::None {
        return;
    }

    // ── 进度推进 ─────────────────────────────────────────────────────
    let dt = time.delta_secs();
    let raw = match phase {
        FocusPhase::Entering => (state.progress + dt / FOCUS_DURATION).min(1.0),
        FocusPhase::Exiting => (state.progress - dt / FOCUS_DURATION).max(0.0),
        _ => state.progress,
    };
    state.progress = raw;
    let t = raw * raw * (3.0 - 2.0 * raw); // smoothstep 缓动

    // ── 进入完成：切换为保持态（继续应用目标几何） ────────────────────
    if phase == FocusPhase::Entering && raw >= 1.0 {
        state.phase = FocusPhase::Active;
    }

    // ── 退出完成：恢复原状 ───────────────────────────────────────────
    if phase == FocusPhase::Exiting && raw <= 0.0 {
        if let Some(vis) = state.saved_dock_visibility {
            if let Ok(dock) = dock_query.single() {
                commands.entity(dock).insert(vis);
            }
        }
        if let Some(saved) = state.saved_pet_node.clone() {
            if let Ok((_, mut node)) = pet_nodes.single_mut() {
                *node = saved;
            }
        }
        if let Some(mut mat) = bg_nodes
            .single()
            .ok()
            .and_then(|mn| bg_materials.get_mut(&mn.0))
        {
            mat.zoom = state.saved_bg_zoom;
            mat.offset = state.saved_bg_offset;
        }
        state.phase = FocusPhase::None;
        state.window = None;
        state.app_id = None;
        state.saved_window_node = None;
        state.saved_pet_node = None;
        state.saved_dock_visibility = None;
        return;
    }

    let Ok(screen) = primary_window.single() else {
        return;
    };
    let screen_w = screen.resolution.width();
    let screen_h = screen.resolution.height();

    // ── 窗口重排：进入 1/7 空隙 + 4/7 宽 + 顶栏下方铺满 ─────────────
    if let (Some(entity), Some(saved)) = (state.window, state.saved_window_node.clone()) {
        if let Ok((_, mut node)) = window_nodes.get_mut(entity) {
            let target = Node {
                left: Val::Px(screen_w / 7.0),
                top: Val::Px(TITLE_BAR_HEIGHT),
                width: Val::Px(screen_w * 4.0 / 7.0),
                height: Val::Px(screen_h - TITLE_BAR_HEIGHT),
                ..saved.clone()
            };
            *node = lerp_node(&saved, &target, t);
        }
    }

    // ── 人物节点：从原始居中位置放大到目标几何 ────────────────────────
    if let Some(saved) = state.saved_pet_node.clone() {
        if let Ok((_, mut node)) = pet_nodes.single_mut() {
            let (base_w, base_h) = match (saved.width, saved.height) {
                (Val::Px(w), Val::Px(h)) => (w, h),
                _ => return,
            };
            // 人物在节点内的实际位置（PetMapping 垂直居中，高度占 94%）：
            //   头顶 y = base_h * PET_HEAD_RATIO
            //   裙摆下缘 y = 头顶 + 全身高 * 4/7（上半身 4 份 / 腿部 3 份）
            let head_y = base_h * PET_HEAD_RATIO;
            let full_h = base_h * PET_FULL_RATIO;
            let skirt_y = head_y + full_h * (UPPER_BODY_PARTS / (UPPER_BODY_PARTS + LOWER_BODY_PARTS));
            // 放大倍数：让「头顶 → 裙摆下缘」这段正好铺满屏幕高度
            let scale = screen_h / (skirt_y - head_y);
            let target_w = base_w * scale;
            let target_h = base_h * scale;
            // 水平居中于右侧 2/7 区中心（右 2/7 区 = [5/7·W, W]，中心 = W - W/7）
            let zone_center_x = screen_w - screen_w / 7.0;
            let target_left = zone_center_x - target_w * 0.5;
            // 头顶贴屏幕顶部（顶部留 0；头部恰好在屏幕最上缘）
            let target_top = -head_y * scale;
            let target = Node {
                position_type: PositionType::Absolute,
                left: Val::Px(target_left),
                top: Val::Px(target_top),
                width: Val::Px(target_w),
                height: Val::Px(target_h),
                ..saved.clone()
            };
            // 起点：saved 的 left/top 是 Val::Auto（flex 居中），插值前规范化为
            // 屏幕居中的实际像素坐标，避免 lerp_val(Auto, Px, t) 恒为 0 导致从左上角起步
            let mut from = saved.clone();
            if matches!(from.left, Val::Auto) {
                from.left = Val::Px((screen_w - base_w) * 0.5);
            }
            if matches!(from.top, Val::Auto) {
                from.top = Val::Px((screen_h - base_h) * 0.5);
            }
            *node = lerp_node(&from, &target, t);
        }
    }

    // ── 背景推镜 ─────────────────────────────────────────────────────
    if let Some(mut mat) = bg_nodes
        .single()
        .ok()
        .and_then(|mn| bg_materials.get_mut(&mn.0))
    {
        mat.zoom = state.saved_bg_zoom + (BG_FOCUS_ZOOM - state.saved_bg_zoom) * t;
        mat.offset = state.saved_bg_offset.lerp(BG_FOCUS_OFFSET, t);
    }
}

/// 对两个 Node 的 left/top/width/height 做线性插值（其余字段取 to）
fn lerp_node(from: &Node, to: &Node, t: f32) -> Node {
    let mut out = to.clone();
    out.left = Val::Px(lerp_val(from.left, to.left, t));
    out.top = Val::Px(lerp_val(from.top, to.top, t));
    out.width = Val::Px(lerp_val(from.width, to.width, t));
    out.height = Val::Px(lerp_val(from.height, to.height, t));
    out
}

fn lerp_val(from: Val, to: Val, t: f32) -> f32 {
    let (Val::Px(a), Val::Px(b)) = (from, to) else {
        return 0.0;
    };
    a + (b - a) * t
}
