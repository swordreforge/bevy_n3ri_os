//! 滚动统一调度：bevy 原生 `ScrollPosition` + 中央 dispatcher。
//!
//! 过滚（over-scroll）根因与对策：
//! 1. 单位混用 —— 旧实现里 `ComputedNode.size()` 是物理像素，而滚轮步进按逻辑像素，
//!    scale ≠ 1 时 clamp 天然偏。现在滚轮值一律先归一化成逻辑像素，写入
//!    [`ScrollPosition`]（逻辑像素），由 `ui_layout_system` 用精确 `content_size`
//!    每帧做 clamp（bevy_ui/layout/mod.rs），不再手算 max 上限。
//! 2. 手写位移反馈环 —— 旧实现写 `content Node.top = -offset`，位移参与下一帧
//!    layout，content_h 是位移后的值，max 每帧抖动。现在容器用
//!    `OverflowAxis::Scroll` + [`ScrollPosition`]，layout 只平移渲染几何、不改布局。
//! 3. `MouseScrollUnit` 被忽略 —— Line/Pixel 混同为同一数字。触摸板的高频小增量
//!    Pixel 事件 × 40 会直接飞到底。dispatcher 按单位分别归一化。
//! 4. `MessageReader<MouseWheel>` 是广播语义 —— 谁读都不消耗，跨消费方靠 title
//!    隔离。这里由 dispatcher 独占处理滚轮：hit-test 出光标下的滚动容器链，
//!    内→外冒泡消费，并把“本帧滚轮已被 UI 滚动链接管”写进 [`UiWheelConsumed`]，
//!    terminal / browser 等页面级消费方先查该标记再决定是否响应——一滚只生效一处。

use bevy::ecs::relationship::Relationship;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::ui::ComputedStackIndex;

use crate::cursor::CursorPosition;
use crate::window::AppWindow;

pub struct ScrollPlugin;

impl Plugin for ScrollPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiWheelConsumed>().add_systems(
            Update,
            (
                scroll_area_setup,
                wheel_dispatch.after(scroll_area_setup),
                scroll_thumb_drag_system.after(wheel_dispatch),
                scroll_sync_system.after(scroll_thumb_drag_system),
            ),
        );
    }
}

/// `MouseScrollUnit::Line` 单位每格换算成的逻辑像素（桌面 UI 常见步进）。
const LINE_SCROLL_PX: f32 = 40.0;
/// 冒泡后剩余位移小于该值视为已被消费完毕。
const CONSUME_EPSILON: f32 = 0.01;

/// 标记可滚动视口节点。
///
/// 应用侧仍以 `ScrollableArea::default()` + `Node { overflow: hidden() }` 的形式生成，
/// 由 [`scroll_area_setup`] 在实体生成后把 y 轴 overflow 统一升级为 `OverflowAxis::Scroll`
/// （`ScrollPosition` 由 `Node` 的 `#[require]` 自动附带），无需逐个 app 改动。
#[derive(Component, Default)]
pub struct ScrollableArea;

/// 滚动内容包裹节点（兼容既有布局结构；其自然高度经 taffy 进入容器的
/// `ComputedNode::content_size`，驱动滚动范围）。
#[derive(Component)]
pub struct ScrollContent;

#[derive(Component)]
pub struct ScrollbarTrack;

#[derive(Component)]
pub struct ScrollbarThumb(pub Entity);

#[derive(Component)]
pub struct ScrollTarget(pub Entity);

/// 本帧滚轮是否已被 UI 滚动链消费。
///
/// [`wheel_dispatch`] 每帧开头清零、消费后置位；terminal / browser 等
/// 页面级消费方在读取 `MouseWheel` 前先查此标记，避免同一帧双重响应。
#[derive(Resource, Default)]
pub struct UiWheelConsumed(pub bool);

const SCROLLBAR_WIDTH: f32 = 6.0;
const SCROLLBAR_TRACK_COLOR: Color = Color::srgba(0.15, 0.2, 0.25, 0.3);
const SCROLLBAR_THUMB_COLOR: Color = Color::srgba(0.5, 0.6, 0.7, 0.5);

/// Spawn a scrollbar track + thumb as siblings to a scrollable content area.
pub fn spawn_scrollbar(parent: &mut ChildSpawnerCommands, scroll_entity: Entity) {
    parent
        .spawn((
            ScrollbarTrack,
            ScrollTarget(scroll_entity),
            // 滚动条是滚动容器的直接子节点，会随父滚动平移；沿滚动轴(y)忽略父
            // ScrollPosition，使其始终钉在视口右侧不动。
            IgnoreScroll(BVec2::new(false, true)),
            ZIndex(1),
            Node {
                width: Val::Px(SCROLLBAR_WIDTH),
                height: Val::Percent(100.0),
                margin: UiRect {
                    left: Val::Px(2.0),
                    right: Val::Px(2.0),
                    top: Val::Px(4.0),
                    bottom: Val::Px(4.0),
                },
                border_radius: BorderRadius::all(Val::Px(3.0)),
                overflow: Overflow::hidden(),
                position_type: PositionType::Absolute,
                right: Val::Px(0.0),
                top: Val::Px(0.0),
                ..default()
            },
            BackgroundColor(SCROLLBAR_TRACK_COLOR),
        ))
        .with_children(|track| {
            track.spawn((
                Button,
                ScrollbarThumb(scroll_entity),
                Interaction::None,
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    border_radius: BorderRadius::all(Val::Px(3.0)),
                    position_type: PositionType::Absolute,
                    top: Val::Px(0.0),
                    ..default()
                },
                BackgroundColor(SCROLLBAR_THUMB_COLOR),
            ));
        });
}

/// 把 [`ScrollableArea`] 视口节点的 y 轴 overflow 升为 `Scroll`。
///
/// `ScrollPosition` 由 `Node` 的 `#[require]` 自动附带（bevy 0.19 每个 UI 节点
/// 出生即有 `ScrollPosition`），这里**不能**用 `Without<ScrollPosition>` 过滤——
/// 那会永远匹配不到。只要把 overflow 切到 Scroll，layout 就会让该轴使用
/// `ScrollPosition` 做 clamp/平移（非 Scroll 轴则强制归零）。x 轴保持 app 原样
/// （现有布局全部为 hidden），仅 y 轴开启滚动。
fn scroll_area_setup(mut areas: Query<&mut Node, With<ScrollableArea>>) {
    for mut node in &mut areas {
        if node.overflow.y != OverflowAxis::Scroll {
            node.overflow.y = OverflowAxis::Scroll;
        }
    }
}

/// 中央滚轮 dispatcher：独占消费 `MouseWheel` 的 UI 滚动部分。
///
/// 流程：归一化（单位 → 逻辑像素）→ 找链（光标下同窗口的滚动容器祖先链，
/// 内→外）→ 冒泡消费（每层只吃能吃的部分，剩余冒泡给外层；全到边界才丢弃）。
/// 只要链上有容器真正吃掉了位移，本帧即标记 [`UiWheelConsumed`]，页面级消费方
/// （终端 / 浏览器）不再重复响应。
/// 作为 ordering 锚点暴露给 terminal / browser 插件（`.after(wheel_dispatch)`）。
#[allow(clippy::too_many_arguments)]
pub(crate) fn wheel_dispatch(
    mut wheel_evr: MessageReader<MouseWheel>,
    cursor: Res<CursorPosition>,
    mut consumed: ResMut<UiWheelConsumed>,
    areas: Query<
        (
            Entity,
            &ComputedNode,
            &ComputedStackIndex,
            &UiGlobalTransform,
            &InheritedVisibility,
        ),
        With<ScrollableArea>,
    >,
    mut scroll_pos: Query<&mut ScrollPosition, With<ScrollableArea>>,
    scroll_nodes: Query<&ComputedNode, With<ScrollableArea>>,
    child_of: Query<&ChildOf>,
    app_windows: Query<
        (
            Entity,
            &ComputedNode,
            &ComputedStackIndex,
            &UiGlobalTransform,
            &InheritedVisibility,
        ),
        With<AppWindow>,
    >,
) {
    consumed.0 = false;
    if !cursor.active {
        return;
    }
    let cursor_pos = cursor.physical;
    let target_window = topmost_at_cursor(app_windows.iter().map(
        |(entity, node, stack, transform, visibility)| {
            (
                entity,
                stack.0,
                visibility.get() && node_contains_cursor(node, transform, cursor_pos),
            )
        },
    ));
    let Some(target_window) = target_window else {
        return;
    };

    // 该窗口内、光标下的所有滚动容器（嵌套时内层 z 更高）。
    let mut under: Vec<(Entity, u32)> = areas
        .iter()
        .filter_map(|(entity, node, stack, transform, visibility)| {
            if !visibility.get() || !node_contains_cursor(node, transform, cursor_pos) {
                return None;
            }
            if window_ancestor(entity, &child_of, &app_windows) != Some(target_window) {
                return None;
            }
            Some((entity, stack.0))
        })
        .collect();
    if under.is_empty() {
        return;
    }
    under.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.index().cmp(&b.0.index())));
    let innermost = under[0].0;

    // 内 → 外 祖先链：只保留真正是祖先的滚动容器，忽略可能重叠的兄弟节点。
    let mut chain = vec![innermost];
    let mut cur = innermost;
    while let Ok(parent) = child_of.get(cur).map(Relationship::get) {
        if app_windows.get(parent).is_ok() {
            break;
        }
        if under.iter().any(|(e, _)| *e == parent) {
            chain.push(parent);
        }
        cur = parent;
    }

    for event in wheel_evr.read() {
        if event.y == 0.0 {
            continue;
        }
        let mut delta_px = match event.unit {
            MouseScrollUnit::Line => event.y * LINE_SCROLL_PX,
            // Pixel 增量是物理像素；ScrollPosition 是逻辑像素，先折算。
            MouseScrollUnit::Pixel => {
                let Ok(node) = scroll_nodes.get(innermost) else {
                    continue;
                };
                event.y * node.inverse_scale_factor
            }
        };

        let mut handled = false;
        for &area_e in &chain {
            let Ok(node) = scroll_nodes.get(area_e) else {
                continue;
            };
            let Ok(mut pos) = scroll_pos.get_mut(area_e) else {
                continue;
            };
            let before = pos.y;
            delta_px = feed_scroll_layer(&node, &mut pos, delta_px);
            if (before - pos.y).abs() > CONSUME_EPSILON {
                handled = true;
            }
            if delta_px.abs() <= CONSUME_EPSILON {
                break;
            }
        }
        if handled {
            consumed.0 = true;
        }
    }
}

/// 向单层滚动容器喂入逻辑像素位移，返回未能吃掉、留给外层冒泡的剩余量。
///
/// 符号约定：`ScrollPosition.y` 增大 = 内容上移（滚向末尾），所以
/// `new = old - delta`。delta 符号与 `MouseWheel.y` 一致（正 = 滚向开头）。
fn feed_scroll_layer(node: &ComputedNode, pos: &mut ScrollPosition, delta_px: f32) -> f32 {
    let max_logical = ((node.content_size().y - node.size().y).max(0.0))
        * node.inverse_scale_factor;
    if max_logical <= 0.0 {
        return delta_px;
    }
    let old = pos.y;
    let new = (old - delta_px).clamp(0.0, max_logical);
    pos.y = new;
    // 实际吃掉的位移 = old - new（符号与 delta 相同），剩余 = delta - 吃掉。
    delta_px - (old - new)
}

fn node_contains_cursor(node: &ComputedNode, transform: &UiGlobalTransform, cursor: Vec2) -> bool {
    transform
        .try_inverse()
        .map(|inverse| inverse.transform_point2(cursor))
        .is_some_and(|local| {
            let half = node.size() * 0.5;
            local.x.abs() <= half.x && local.y.abs() <= half.y
        })
}

fn window_ancestor(
    start: Entity,
    child_of: &Query<&ChildOf>,
    app_windows: &Query<
        (
            Entity,
            &ComputedNode,
            &ComputedStackIndex,
            &UiGlobalTransform,
            &InheritedVisibility,
        ),
        With<AppWindow>,
    >,
) -> Option<Entity> {
    let mut current = start;
    loop {
        if app_windows.get(current).is_ok() {
            return Some(current);
        }
        current = child_of.get(current).ok()?.get();
    }
}

fn topmost_at_cursor<T>(areas: impl IntoIterator<Item = (T, u32, bool)>) -> Option<T> {
    areas
        .into_iter()
        .filter(|(_, _, contains_cursor)| *contains_cursor)
        .max_by_key(|(_, z, _)| *z)
        .map(|(window, _, _)| window)
}

fn scroll_thumb_drag_system(
    mouse: Res<ButtonInput<MouseButton>>,
    cursor: Res<CursorPosition>,
    thumbs: Query<(
        &Interaction,
        &ScrollbarThumb,
        &ComputedNode,
        &UiGlobalTransform,
    )>,
    tracks: Query<(&ComputedNode, &UiGlobalTransform), With<ScrollbarTrack>>,
    areas: Query<(Entity, &ComputedNode), With<ScrollableArea>>,
    mut scroll_pos: Query<&mut ScrollPosition, With<ScrollableArea>>,
) {
    if !mouse.pressed(MouseButton::Left) {
        return;
    }
    if !cursor.active {
        return;
    }
    let cursor_pos = cursor.physical;

    for (interaction, thumb, _, _) in thumbs.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let area_e = thumb.0;
        let Ok((track_node, track_tf)) = tracks.get(area_e) else {
            continue;
        };
        let Some(local) = track_tf.try_inverse().map(|t| t.transform_point2(cursor_pos)) else {
            continue;
        };
        let half = track_node.size() * 0.5;
        let y_from_top = local.y + half.y;
        let ratio = (y_from_top / track_node.size().y).clamp(0.0, 1.0);

        let Ok((_, area_node)) = areas.get(area_e) else {
            continue;
        };
        let Ok(mut pos) = scroll_pos.get_mut(area_e) else {
            continue;
        };
        let max_logical = ((area_node.content_size().y - area_node.size().y).max(0.0))
            * area_node.inverse_scale_factor;
        pos.y = ratio * max_logical;
    }
}

/// 滚动条 thumb 外观同步：长度 = 视口/内容比例，位置 = 当前 ScrollPosition 比例。
fn scroll_sync_system(
    areas: Query<(Entity, &ScrollPosition, &ComputedNode), With<ScrollableArea>>,
    tracks: Query<(&ScrollTarget, &Children), With<ScrollbarTrack>>,
    mut thumbs: Query<&mut Node, With<ScrollbarThumb>>,
) {
    for (area_e, pos, area_node) in areas.iter() {
        let content_h = area_node.content_size().y;
        let viewport_h = area_node.size().y;
        let max_logical = (content_h - viewport_h).max(0.0) * area_node.inverse_scale_factor;
        let offset = pos.y.clamp(0.0, max_logical);

        for (target, track_children) in tracks.iter() {
            if target.0 != area_e {
                continue;
            }
            for tc in track_children.iter() {
                if let Ok(mut tn) = thumbs.get_mut(tc) {
                    if max_logical <= 0.0 || content_h <= 0.0 {
                        if tn.height != Val::Percent(100.0) {
                            tn.height = Val::Percent(100.0);
                        }
                        if tn.top != Val::Percent(0.0) {
                            tn.top = Val::Percent(0.0);
                        }
                        continue;
                    }
                    let thumb_pct = (viewport_h / content_h * 100.0).clamp(8.0, 100.0);
                    let travel = 100.0 - thumb_pct;
                    let top_pct = (offset / max_logical) * travel;
                    if tn.height != Val::Percent(thumb_pct) {
                        tn.height = Val::Percent(thumb_pct);
                    }
                    if tn.top != Val::Percent(top_pct) {
                        tn.top = Val::Percent(top_pct);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_target_is_highest_overlapping_window() {
        // Given
        let lower = Entity::from_raw_u32(1).unwrap();
        let upper = Entity::from_raw_u32(2).unwrap();
        let windows = [(lower, 4, true), (upper, 9, true)];

        // When
        let target = topmost_at_cursor(windows);

        // Then
        assert_eq!(target, Some(upper));
    }

    #[test]
    fn wheel_target_ignores_higher_window_outside_cursor() {
        // Given
        let under_cursor = Entity::from_raw_u32(1).unwrap();
        let elsewhere = Entity::from_raw_u32(2).unwrap();
        let windows = [(under_cursor, 4, true), (elsewhere, 9, false)];

        // When
        let target = topmost_at_cursor(windows);

        // Then
        assert_eq!(target, Some(under_cursor));
    }

    #[test]
    fn scroll_layer_clamps_and_bubbles_remainder() {
        // Given: 内容 1000px / 视口 400px @ scale 1 → max_logical = 600
        let mut node = ComputedNode::default();
        node.content_size = Vec2::new(200.0, 1000.0);
        node.size = Vec2::new(200.0, 400.0);
        node.inverse_scale_factor = 1.0;
        let mut pos = ScrollPosition(Vec2::ZERO);

        // When: 向下滚 100（delta = -100，滚向末尾）
        let rem = feed_scroll_layer(&node, &mut pos, -100.0);

        // Then: 位置推进、无剩余
        assert_eq!(pos.y, 100.0);
        assert!(rem.abs() < CONSUME_EPSILON);
    }

    #[test]
    fn scroll_layer_boundary_keeps_remainder_for_parent() {
        // Given: 内容 600 / 视口 400 → max = 200，当前已在底部
        let mut node = ComputedNode::default();
        node.content_size = Vec2::new(200.0, 600.0);
        node.size = Vec2::new(200.0, 400.0);
        node.inverse_scale_factor = 1.0;
        let mut pos = ScrollPosition(Vec2::new(0.0, 200.0));

        // When: 继续向下滚 100（-100 已过界）
        let rem = feed_scroll_layer(&node, &mut pos, -100.0);

        // Then: 位置顶在 max，剩余 -100 冒泡给外层
        assert_eq!(pos.y, 200.0);
        assert!((rem - (-100.0)).abs() < CONSUME_EPSILON);
    }

    #[test]
    fn scroll_layer_respects_scale_factor() {
        // Given: 内容 1000 / 视口 400 物理像素 @ scale 2 → max_logical = 300
        let mut node = ComputedNode::default();
        node.content_size = Vec2::new(400.0, 1000.0);
        node.size = Vec2::new(400.0, 400.0);
        node.inverse_scale_factor = 0.5;
        let mut pos = ScrollPosition(Vec2::ZERO);

        // When: 逻辑像素位移 -300（滚向末尾）
        let rem = feed_scroll_layer(&node, &mut pos, -300.0);

        // Then: 只吃 300 逻辑像素（物理 600px），不越界
        assert_eq!(pos.y, 300.0);
        assert!(rem.abs() < CONSUME_EPSILON);
    }
}
