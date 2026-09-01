//! 壁纸模式输入桥接。
//!
//! 壁纸模式下没有 winit 主窗，三路输入全部改道：
//! 1. 光标：[`sync_cursor_from_wallpaper`] 把 layer-shell 指针（可见时给绝对位置，
//!    并重定卫星基线防漂移）与卫星进程 delta（被遮挡时外推）合并进 [`CursorPosition`]；
//! 2. 按钮：指针按下/松开 diff 成 `MouseButtonInput` 消息，由 bevy_input 原版
//!    `mouse_button_input_system` 统一消费，避免直接写 `ButtonInput` 与每帧 clear 竞态；
//! 3. `Interaction`：bevy 0.19 的 `ui_focus_system` 对 Image 目标相机直接跳过
//!    （"Interactions are only supported for cameras rendering to a window"），
//!    [`wallpaper_ui_focus_system`] 复刻同一算法、光标取自 [`CursorPosition`]，
//!    在原版之后运行并覆盖其重置结果。
//!
//! 仅在壁纸模式注册（见 examples/minimal）；窗口模式完全不加载本插件。

use std::collections::HashSet;
use std::sync::Mutex;

use bevy::ecs::query::QueryData;
use bevy::input::mouse::MouseButtonInput;
use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::ui::{clip_check_recursive, ui_focus_system};
use bevy::ui::{
    ComputedNode, FocusPolicy, Interaction, Node, OverrideClip, RelativeCursorPosition,
    UiGlobalTransform, UiStack,
};
use bevy_live_wallpaper::{PointerSample, WallpaperPointerState, WallpaperSurfaceInfo};

use crate::cursor::{CursorPosition, UiArea};

/// 卫星进程上行样本：指针绝对位置（XQueryPointer，X 屏物理坐标）/ X 屏物理尺寸（开机首行）。
pub enum SatelliteSample {
    Pos(Vec2),
    Screen(Vec2),
}

/// 卫星通道（mpsc 由二进制侧创建并注入；Receiver 非 Sync，Mutex 包裹）。
#[derive(Resource)]
pub struct SatelliteDeltaChannel(pub Mutex<std::sync::mpsc::Receiver<SatelliteSample>>);

/// 本帧排干的卫星位置（First 内先于光标合并）。
#[derive(Resource, Default)]
pub struct SatelliteFrame {
    pub pos: Option<Vec2>,
}

/// 卫星上报的 X 屏物理尺寸（持久，用于推算合成器缩放 = 物理尺寸 / surface 逻辑尺寸）。
#[derive(Resource, Default)]
pub struct SatelliteScreen(pub Vec2);

pub struct WallpaperInputBridgePlugin;

impl Plugin for WallpaperInputBridgePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SatelliteFrame>()
            .init_resource::<SatelliteScreen>()
            .add_systems(First, (drain_satellite, sync_cursor_from_wallpaper).chain())
            .add_systems(First, inject_mouse_buttons)
            .add_systems(Update, wallpaper_ui_focus_system.after(ui_focus_system));
    }
}

fn drain_satellite(
    channel: Option<Res<SatelliteDeltaChannel>>,
    mut frame: ResMut<SatelliteFrame>,
    mut screen: ResMut<SatelliteScreen>,
) {
    frame.pos = None;
    let Some(channel) = channel else {
        return;
    };
    let Ok(rx) = channel.0.lock() else {
        return;
    };
    while let Ok(sample) = rx.try_recv() {
        match sample {
            SatelliteSample::Pos(p) => frame.pos = Some(p),
            SatelliteSample::Screen(s) => screen.0 = s,
        }
    }
}

fn sync_cursor_from_wallpaper(
    pointer: Res<WallpaperPointerState>,
    surface: Res<WallpaperSurfaceInfo>,
    frame: Res<SatelliteFrame>,
    screen: Res<SatelliteScreen>,
    mut cursor: ResMut<CursorPosition>,
    mut area: ResMut<UiArea>,
) {
    area.0 = surface.size;

    // 合成器缩放 = X 屏物理宽度 / surface 逻辑宽度（卫星缺失或异常时回退 1.0）
    let scale = if screen.0.x > 0.0 && surface.size.x > 0.0 {
        (screen.0.x / surface.size.x).max(1.0)
    } else {
        1.0
    };
    cursor.scale = scale;

    // 优先 layer-shell 指针（按钮状态可信的判定窗口，逻辑坐标）；
    // 否则用卫星绝对位置（XQueryPointer 物理坐标，被遮挡时依然有效，零漂移）。
    // physical 语义 = UI 渲染空间像素：壁纸 UI 目标是逻辑尺寸的 Image（bevy scale=1.0），
    // 因此两条路径的 physical 都等于 logical，绝不乘合成器 scale（那是 pet RTT 专用）。
    match pointer.last.as_ref() {
        Some(sample) => {
            let logical = sample.position - surface.offset_position;
            cursor.logical = logical;
            cursor.physical = logical;
            cursor.active = true;
        }
        None => match frame.pos {
            Some(pos) => {
                let logical = (pos - surface.offset_position) / scale;
                cursor.logical = logical;
                cursor.physical = logical;
                cursor.active = true;
            }
            None => cursor.active = false,
        },
    }
}

fn inject_mouse_buttons(
    pointer: Res<WallpaperPointerState>,
    mut prev_pressed: Local<HashSet<bevy::input::mouse::MouseButton>>,
    mut events: MessageWriter<MouseButtonInput>,
) {
    // 按钮状态仅在指针位于 surface 上时可信（Wayland 协议限制）
    let current: HashSet<bevy::input::mouse::MouseButton> = pointer
        .last
        .as_ref()
        .map(|s: &PointerSample| s.pressed.clone())
        .unwrap_or_default();

    if pointer.last.is_some() {
        for button in current.difference(&prev_pressed) {
            events.write(MouseButtonInput {
                button: *button,
                state: ButtonState::Pressed,
                window: Entity::PLACEHOLDER,
            });
        }
        for button in prev_pressed.difference(&current) {
            events.write(MouseButtonInput {
                button: *button,
                state: ButtonState::Released,
                window: Entity::PLACEHOLDER,
            });
        }
    }
    *prev_pressed = current;
}

#[derive(QueryData)]
#[query_data(mutable)]
struct FocusNodeQuery {
    entity: Entity,
    node: &'static ComputedNode,
    transform: &'static UiGlobalTransform,
    interaction: Option<&'static mut Interaction>,
    relative_cursor_position: Option<&'static mut RelativeCursorPosition>,
    focus_policy: Option<&'static FocusPolicy>,
    inherited_visibility: Option<&'static InheritedVisibility>,
}

/// [`ui_focus_system`] 的壁纸模式复刻：光标来自 [`CursorPosition`] 而非窗口查询。
/// 与原版逐段对齐（复位、按下/悬停判定、FocusPolicy 捕获、裁剪递归），仅相机
/// 光标映射替换为全局资源（壁纸应用单相机，光标对全部 UI 节点生效）。
fn wallpaper_ui_focus_system(
    mut hovered_nodes: Local<Vec<Entity>>,
    mut entities_to_reset: Local<Vec<Entity>>,
    cursor: Res<CursorPosition>,
    mouse_button_input: Res<ButtonInput<MouseButton>>,
    ui_stack: Res<UiStack>,
    mut node_query: Query<FocusNodeQuery>,
    clipping_query: Query<(&ComputedNode, &UiGlobalTransform, &Node)>,
    child_of_query: Query<&ChildOf, Without<OverrideClip>>,
) {
    for entity in entities_to_reset.drain(..) {
        if let Ok(item) = node_query.get_mut(entity) {
            if let Some(mut interaction) = item.interaction {
                *interaction = Interaction::None;
            }
        }
    }

    let mouse_released = mouse_button_input.just_released(MouseButton::Left);
    if mouse_released {
        for node in &mut node_query {
            if let Some(mut interaction) = node.interaction {
                if *interaction == Interaction::Pressed {
                    *interaction = Interaction::None;
                }
            }
        }
    }

    let mouse_clicked = mouse_button_input.just_pressed(MouseButton::Left);
    let cursor_position = cursor.active.then_some(cursor.physical);

    hovered_nodes.clear();
    for uinodes in ui_stack
        .partition
        .iter()
        .rev()
        .map(|range| &ui_stack.uinodes[range.clone()])
    {
        for entity in uinodes.iter().rev().cloned() {
            let Ok(node) = node_query.get_mut(entity) else {
                continue;
            };

            let Some(inherited_visibility) = node.inherited_visibility else {
                continue;
            };

            if !inherited_visibility.get() {
                if let Some(mut interaction) = node.interaction {
                    interaction.set_if_neq(Interaction::None);
                }
                continue;
            }

            let contains_cursor = cursor_position.is_some_and(|point| {
                node.node.contains_point(*node.transform, point)
                    && clip_check_recursive(point, entity, &clipping_query, &child_of_query)
            });

            let normalized_cursor_position = cursor_position.and_then(|cursor_position| {
                node.node.normalize_point(*node.transform, cursor_position)
            });

            let relative_cursor_position_component = RelativeCursorPosition {
                cursor_over: contains_cursor,
                normalized: normalized_cursor_position,
            };

            if let Some(mut node_relative_cursor_position_component) =
                node.relative_cursor_position
            {
                node_relative_cursor_position_component
                    .set_if_neq(relative_cursor_position_component);
            }

            if contains_cursor {
                hovered_nodes.push(entity);
            } else if let Some(mut interaction) = node.interaction {
                let stale = *interaction == Interaction::Hovered
                    || normalized_cursor_position.is_none();
                if stale {
                    interaction.set_if_neq(Interaction::None);
                }
            }
        }
    }

    let mut hovered_iter = hovered_nodes.iter();
    let mut iter = node_query.iter_many_mut(hovered_iter.by_ref());
    while let Some(node) = iter.fetch_next() {
        if let Some(mut interaction) = node.interaction {
            if mouse_clicked {
                if *interaction != Interaction::Pressed {
                    *interaction = Interaction::Pressed;
                    if mouse_released {
                        entities_to_reset.push(node.entity);
                    }
                }
            } else if *interaction == Interaction::None {
                *interaction = Interaction::Hovered;
            }
        }

        match node.focus_policy.unwrap_or(&FocusPolicy::Block) {
            FocusPolicy::Block => break,
            FocusPolicy::Pass => {}
        }
    }

    let mut iter = node_query.iter_many_mut(hovered_nodes.iter());
    while let Some(node) = iter.fetch_next() {
        if let Some(mut interaction) = node.interaction {
            if *interaction != Interaction::Pressed {
                interaction.set_if_neq(Interaction::None);
            }
        }
    }
}
