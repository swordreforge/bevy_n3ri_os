//! 纯净模式：只剩静态壁纸/背景 shader + Live2D，dock/顶栏/聊天胶囊全部淡出隐藏。
//!
//! 进入纯净模式保留壁纸背景（`DesktopBackground` + 宠物节点不动），
//! dock/顶栏/chat 用 `Visibility::Hidden` 收起（UI 焦点系统自动跳过隐藏节点，
//! 无需改交互逻辑）。屏幕右侧一个小悬浮球（`PureExitBall`），点击即退出。
//! 过渡动画走 `UiTransform` scale（bevy_tweening 已在 workspace，零新依赖）：
//! 进入 0.24s 缩到 0.9 + 透明感（Visibility 二值，无渐隐，只做 scale 缩放），
//! 退出反向恢复。悬浮球常驻纯净模式，`Visibility::Hidden` ↔ `Inherited` 切换。

use std::time::Duration;

use bevy::prelude::*;
use bevy_tweening::{Lens, Tween, TweenAnim};

use crate::chat_capsule::{ChatCapsuleRoot, PureShortcutBadge};
use crate::dock::Dock;
use crate::topbar::TopBar;
use crate::window::AppWindow;

const PURE_SECS: f32 = 0.24;
const PURE_EASE: EaseFunction = EaseFunction::CubicOut;

/// 纯净模式开关资源：`true` = 纯净模式（dock/顶栏/chat 隐藏，悬浮球可见）。
#[derive(Resource, Default)]
pub struct PureMode(pub bool);

/// 纯净模式变化事件：发送即翻转 `PureMode`，由 `apply_pure_mode` 落到 Visibility。
#[derive(Message)]
pub struct TogglePureMode;

/// 右侧悬浮退出球。
#[derive(Component)]
pub struct PureExitBall;

struct ScaleLens {
    from: f32,
    to: f32,
}

impl Lens<UiTransform> for ScaleLens {
    fn lerp(&mut self, mut target: Mut<UiTransform>, ratio: f32) {
        target.scale = Vec2::splat(self.from + (self.to - self.from) * ratio);
    }
}

fn pure_tween(from: f32, to: f32) -> Tween {
    Tween::new(
        PURE_EASE,
        Duration::from_secs_f32(PURE_SECS),
        ScaleLens { from, to },
    )
}

pub struct PureModePlugin;

impl Plugin for PureModePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PureMode>()
            .add_message::<TogglePureMode>()
            .add_systems(Startup, spawn_exit_ball)
            .add_systems(
                Update,
                (
                    toggle_pure_mode,
                    exit_ball_click,
                    pure_shortcut,
                    pure_badge_sync,
                ),
            );
    }
}

/// 悬浮球：右侧居中，平时隐藏（纯净模式才出现）。
/// 字体不用 `N3riFonts`（它在同一 Startup 批次里初始化，顺序不定会 panic）——
/// 用默认字体（bevy 内置 FiraMono 子集），"x" 是 ASCII 安全字符。
fn spawn_exit_ball(mut commands: Commands) {
    commands
        .spawn((
            PureExitBall,
            Button,
            Visibility::Hidden,
            UiTransform::default(),
            GlobalZIndex(200),
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(16.0),
                top: Val::Percent(50.0),
                width: Val::Px(44.0),
                height: Val::Px(44.0),
                border_radius: BorderRadius::all(Val::Px(22.0)),
                display: Display::Flex,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.10, 0.14, 0.20, 0.72)),
            BorderColor::all(Color::srgba(0.4, 0.95, 1.0, 0.55)),
        ))
        .with_children(|ball| {
            ball.spawn((
                Text::new("x"),
                TextFont {
                    font_size: FontSize::Px(18.0),
                    ..default()
                },
                TextColor(Color::srgba(0.75, 0.95, 1.0, 0.95)),
            ));
        });
}

/// `TogglePureMode` → 翻转资源：dock/顶栏/chat/窗口显隐切换 + scale 过渡 + 悬浮球显隐。
/// 壁纸背景（`DesktopBackground`）与 Live2D 宠物节点不受影响——纯净模式的意义
/// 就是只剩壁纸。窗口（`AppWindow`）同样隐藏：纯净 = 无干扰展示。
fn toggle_pure_mode(
    mut events: MessageReader<TogglePureMode>,
    mut pure: ResMut<PureMode>,
    mut commands: Commands,
    dock: Query<Entity, With<Dock>>,
    topbar: Query<Entity, With<TopBar>>,
    chat: Query<Entity, With<ChatCapsuleRoot>>,
    windows: Query<Entity, With<AppWindow>>,
    ball: Query<Entity, With<PureExitBall>>,
) {
    if events.read().next().is_none() {
        return;
    }
    pure.0 = !pure.0;
    let entering = pure.0;
    for e in dock
        .iter()
        .chain(topbar.iter())
        .chain(chat.iter())
        .chain(windows.iter())
    {
        if entering {
            commands.entity(e).insert(Visibility::Hidden);
        } else {
            commands
                .entity(e)
                .insert(Visibility::Inherited)
                .insert(TweenAnim::new(pure_tween(0.9, 1.0)));
        }
    }
    for e in ball.iter() {
        if entering {
            commands
                .entity(e)
                .insert(Visibility::Inherited)
                .insert(TweenAnim::new(pure_tween(0.6, 1.0)));
        } else {
            commands.entity(e).insert(Visibility::Hidden);
        }
    }
}

/// 悬浮球点击 → 退出纯净模式（复用同一翻转事件）。
fn exit_ball_click(
    mouse: Res<ButtonInput<MouseButton>>,
    ball: Query<&Interaction, With<PureExitBall>>,
    mut events: MessageWriter<TogglePureMode>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for interaction in ball.iter() {
        if *interaction == Interaction::Pressed {
            events.write(TogglePureMode);
        }
    }
}

/// 全局快捷键：Ctrl+K（macOS Cmd+K 同键位）在正常/纯净模式间翻转；
/// 纯净模式下 Esc 直接退出。聊天胶囊的输入系统在 active 时消费键盘，
/// 纯净模式下胶囊隐藏、`chat_capsule_input` 早退，快捷键自然让渡给本系统，无冲突。
fn pure_shortcut(
    keys: Res<ButtonInput<KeyCode>>,
    pure: Res<PureMode>,
    mut events: MessageWriter<TogglePureMode>,
) {
    let ctrl = keys.pressed(KeyCode::ControlLeft)
        || keys.pressed(KeyCode::ControlRight)
        || keys.pressed(KeyCode::SuperLeft)
        || keys.pressed(KeyCode::SuperRight);
    let enter = ctrl && keys.just_pressed(KeyCode::KeyK);
    let exit_esc = pure.0 && keys.just_pressed(KeyCode::Escape);
    if enter || exit_esc {
        events.write(TogglePureMode);
    }
}

/// 徽标文案跟随模式：正常 `Ctrl + K` → 进纯净；纯净中 `Esc / Ctrl+K` → 退出。
fn pure_badge_sync(pure: Res<PureMode>, mut badge: Query<&mut Text, With<PureShortcutBadge>>) {
    if !pure.is_changed() {
        return;
    }
    let label = if pure.0 { "Esc 退出" } else { "Ctrl + K" };
    for mut text in badge.iter_mut() {
        if text.as_str() != label {
            text.clear();
            text.push_str(label);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::AppWindow;

    #[test]
    fn pure_mode_defaults_off() {
        assert!(!PureMode::default().0);
    }

    #[test]
    fn toggle_message_flips_and_hides() {
        let mut app = App::new();
        app.add_plugins(PureModePlugin);
        let dock_e = app.world_mut().spawn(Dock).id();
        let win_e = app
            .world_mut()
            .spawn(AppWindow {
                title: "t".into(),
                app_id: "t".into(),
                z: 0,
            })
            .id();
        app.world_mut()
            .resource_mut::<Messages<TogglePureMode>>()
            .write(TogglePureMode);
        app.update();
        assert!(app.world().resource::<PureMode>().0);
        assert_eq!(
            *app.world().get::<Visibility>(dock_e).unwrap(),
            Visibility::Hidden
        );
        assert_eq!(
            *app.world().get::<Visibility>(win_e).unwrap(),
            Visibility::Hidden
        );
        app.world_mut()
            .resource_mut::<Messages<TogglePureMode>>()
            .write(TogglePureMode);
        app.update();
        assert!(!app.world().resource::<PureMode>().0);
        assert_eq!(
            *app.world().get::<Visibility>(dock_e).unwrap(),
            Visibility::Inherited
        );
        assert_eq!(
            *app.world().get::<Visibility>(win_e).unwrap(),
            Visibility::Inherited
        );
    }
}
