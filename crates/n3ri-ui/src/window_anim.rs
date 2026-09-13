//! 窗口开/关/最小化/恢复的 Apple 风格动效（bevy_tweening + `UiTransform`）。
//!
//! 三条约定：
//! - 动画只写 `UiTransform`（scale + translation），**绝不写 `Node`**——
//!   `Node.left/top/width/height` 仍是拖拽/吸附/缩放/最大化的唯一真源。
//! - `UiTransform` 是层级式的：`ui_layout_system` 里
//!   `inherited_transform *= local_transform`，且缩放绕节点中心，
//!   因此只缩放窗口根节点的 `UiTransform` 即可缩放整窗子树。
//! - `UiTransform.translation` 是逻辑像素（`Val::Px` 按 scale_factor 解析），
//!   `UiGlobalTransform.translation` 是物理像素（节点中心）；
//!   跨两者换算乘 `ComputedNode.inverse_scale_factor`。

use std::time::Duration;

use bevy::prelude::*;
use bevy_tweening::{AnimCompletedEvent, Lens, Tween, TweenAnim};

use crate::apps::terminal::TerminalState;
use crate::dock::{AppVisible, DockIcon};
use crate::window::AppWindow;

// ── 动效参数 ──
const OPEN_SECS: f32 = 0.30;
const CLOSE_SECS: f32 = 0.18;
const MINIMIZE_SECS: f32 = 0.34;
const RESTORE_SECS: f32 = 0.34;

/// 入场起始缩放：0.92 → 1.0。
pub const OPEN_FROM_SCALE: f32 = 0.92;
/// 入场起始下移（逻辑 px）：+16 → 0，给一点「落定」的纵深。
pub const OPEN_FROM_OFFSET_Y: f32 = 16.0;
/// 退场结束缩放：原地收缩到 0.94。
const CLOSE_TO_SCALE: f32 = 0.94;
/// 最小化结束缩放：缩进 dock 图标。
const MINIMIZE_TO_SCALE: f32 = 0.10;

// Apple 观感：入场快起慢停（BackOut 带极轻过冲），退场慢起快收。
const OPEN_EASE: EaseFunction = EaseFunction::BackOut;
const CLOSE_EASE: EaseFunction = EaseFunction::QuadraticIn;
const MINIMIZE_EASE: EaseFunction = EaseFunction::CubicIn;
const RESTORE_EASE: EaseFunction = EaseFunction::QuinticOut;

/// 打开动画进行中（仅用于屏蔽重复触发）。
#[derive(Component)]
pub struct WindowAnimating;

/// 关闭动画进行中：结束后 despawn。
#[derive(Component)]
pub struct WindowClosing;

/// 最小化动画进行中：结束后隐藏。
#[derive(Component)]
pub struct WindowMinimizing;

/// 同时 lerp `UiTransform` 的 scale 与 translation。
struct WindowAnimLens {
    from_scale: f32,
    to_scale: f32,
    from_t: Vec2,
    to_t: Vec2,
}

impl Lens<UiTransform> for WindowAnimLens {
    fn lerp(&mut self, mut target: Mut<UiTransform>, ratio: f32) {
        target.scale =
            Vec2::splat(self.from_scale + (self.to_scale - self.from_scale) * ratio);
        let t = self.from_t.lerp(self.to_t, ratio);
        target.translation = Val2::px(t.x, t.y);
    }
}

fn px_of(val: Val) -> f32 {
    match val {
        Val::Px(px) => px,
        _ => 0.0,
    }
}

fn translation_of(transform: &UiTransform) -> Vec2 {
    Vec2::new(
        px_of(transform.translation.x),
        px_of(transform.translation.y),
    )
}

/// 入场初始 `UiTransform`：必须与 [`open_tween`] 的起点一致，否则首帧会闪一帧全尺寸。
pub fn open_transform() -> UiTransform {
    UiTransform {
        translation: Val2::px(0.0, OPEN_FROM_OFFSET_Y),
        scale: Vec2::splat(OPEN_FROM_SCALE),
        ..default()
    }
}

/// 打开：从 [`open_transform`] 缩放到 1.0 并落回原位。
pub fn open_tween() -> Tween {
    Tween::new(
        OPEN_EASE,
        Duration::from_secs_f32(OPEN_SECS),
        WindowAnimLens {
            from_scale: OPEN_FROM_SCALE,
            to_scale: 1.0,
            from_t: Vec2::new(0.0, OPEN_FROM_OFFSET_Y),
            to_t: Vec2::ZERO,
        },
    )
}

/// 关闭：从当前状态原地收缩（不产生位移）。
pub fn close_tween(cur: &UiTransform) -> Tween {
    let t = translation_of(cur);
    Tween::new(
        CLOSE_EASE,
        Duration::from_secs_f32(CLOSE_SECS),
        WindowAnimLens {
            from_scale: cur.scale.x,
            to_scale: CLOSE_TO_SCALE,
            from_t: t,
            to_t: t,
        },
    )
}

/// 最小化：从当前状态缩向 dock 图标；`delta_logical` 是额外位移（逻辑 px，见 [`dock_delta`]）。
pub fn minimize_tween(cur: &UiTransform, delta_logical: Vec2) -> Tween {
    let t = translation_of(cur);
    Tween::new(
        MINIMIZE_EASE,
        Duration::from_secs_f32(MINIMIZE_SECS),
        WindowAnimLens {
            from_scale: cur.scale.x,
            to_scale: MINIMIZE_TO_SCALE,
            from_t: t,
            to_t: t + delta_logical,
        },
    )
}

/// 恢复：从最小化终态放大回原位。
pub fn restore_tween(cur: &UiTransform) -> Tween {
    let t = translation_of(cur);
    Tween::new(
        RESTORE_EASE,
        Duration::from_secs_f32(RESTORE_SECS),
        WindowAnimLens {
            from_scale: cur.scale.x,
            to_scale: 1.0,
            from_t: t,
            to_t: Vec2::ZERO,
        },
    )
}

/// 窗口中心 → dock 图标中心 的额外位移（逻辑 px）。
///
/// `win_center`/`icon_center` 取自 `UiGlobalTransform.translation`（物理像素）。
pub fn dock_delta(inverse_scale_factor: f32, win_center: Vec2, icon_center: Vec2) -> Vec2 {
    (icon_center - win_center) * inverse_scale_factor
}

/// 在 dock 里找 `app_id` 对应图标的物理中心。
pub fn dock_icon_center(
    app_id: &str,
    icons: &Query<(&DockIcon, &UiGlobalTransform)>,
) -> Option<Vec2> {
    icons
        .iter()
        .find(|(icon, _)| icon.app_name == app_id)
        .map(|(_, tf)| tf.translation)
}

pub struct WindowAnimPlugin;

impl Plugin for WindowAnimPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, finish_window_anims);
    }
}

/// 动画结束收尾：关闭的 despawn、最小化的隐藏、打开的清理。
fn finish_window_anims(
    mut completed: MessageReader<AnimCompletedEvent>,
    closing: Query<(), With<WindowClosing>>,
    minimizing: Query<(), With<WindowMinimizing>>,
    apps: Query<&AppWindow>,
    mut visible: Query<&mut AppVisible>,
    mut terminal: ResMut<TerminalState>,
    mut commands: Commands,
) {
    for ev in completed.read() {
        let entity = ev.anim_entity;

        // 关闭优先：被打断的最小化不再隐藏窗口。
        if closing.contains(entity) {
            if let Ok(app) = apps.get(entity) {
                if app.app_id == "terminal" {
                    terminal.reset();
                }
            }
            commands.entity(entity).despawn();
        } else if minimizing.contains(entity) {
            if let Ok(mut vis) = visible.get_mut(entity) {
                vis.0 = false;
            }
            commands
                .entity(entity)
                .insert(Visibility::Hidden)
                .remove::<(WindowMinimizing, WindowAnimating, TweenAnim)>();
        } else {
            commands
                .entity(entity)
                .remove::<(WindowAnimating, TweenAnim)>();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_tweening::TweeningPlugin;

    /// `TweeningPlugin` 是 unique 插件，n3ri-ui 与 n3ri-live2d 都会注册它。
    /// 两处都必须先 `is_plugin_added` 去重，否则第二处 add 会 panic。
    #[test]
    fn tweening_plugin_registration_is_deduped() {
        let mut app = App::new();
        for _ in 0..2 {
            if !app.is_plugin_added::<TweeningPlugin>() {
                app.add_plugins(TweeningPlugin);
            }
        }
        assert!(app.is_plugin_added::<TweeningPlugin>());
    }
}
