use bevy::ecs::relationship::Relationship;
use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy::ui::ComputedStackIndex;

use crate::cursor::CursorPosition;
use crate::window::AppWindow;

pub struct ScrollPlugin;

impl Plugin for ScrollPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (scroll_wheel_system, scroll_thumb_drag_system, scroll_sync_system),
        );
    }
}

const WHEEL_SPEED: f32 = 40.0;

#[derive(Component, Default)]
pub struct ScrollableArea {
    pub scroll_offset: f32,
}

#[derive(Component)]
pub struct ScrollContent;

#[derive(Component)]
pub struct ScrollbarTrack;

#[derive(Component)]
pub struct ScrollbarThumb(pub Entity);

#[derive(Component)]
pub struct ScrollTarget(pub Entity);

const SCROLLBAR_WIDTH: f32 = 6.0;
const SCROLLBAR_TRACK_COLOR: Color = Color::srgba(0.15, 0.2, 0.25, 0.3);
const SCROLLBAR_THUMB_COLOR: Color = Color::srgba(0.5, 0.6, 0.7, 0.5);

/// Spawn a scrollbar track + thumb as siblings to a scrollable content area.
pub fn spawn_scrollbar(parent: &mut ChildSpawnerCommands, scroll_entity: Entity) {
    parent
        .spawn((
            ScrollbarTrack,
            ScrollTarget(scroll_entity),
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

fn scroll_wheel_system(
    mut wheel_evr: MessageReader<MouseWheel>,
    cursor: Res<CursorPosition>,
    mut areas: Query<(
        Entity,
        &mut ScrollableArea,
        &ComputedNode,
        &ComputedStackIndex,
        &UiGlobalTransform,
        &InheritedVisibility,
        &Children,
    )>,
    content_computed: Query<&ComputedNode, With<ScrollContent>>,
    mut content_nodes: Query<&mut Node, With<ScrollContent>>,
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
    let mut raw = Vec::new();
    for e in wheel_evr.read() {
        raw.push((e.window, e.y, e.x));
    }
    let delta: f32 = raw.iter().map(|(_, y, _)| y).sum();
    if !raw.is_empty() {
        eprintln!(
            "[scroll] RAW {:?} ({} msgs, sum_y={delta})",
            raw,
            raw.len()
        );
    }
    if delta == 0.0 {
        return;
    }
    if !cursor.active {
        eprintln!("[scroll] delta={delta} SKIP: cursor inactive");
        return;
    }
    let cursor = cursor.physical;
    let target_window = topmost_at_cursor(app_windows.iter().map(
        |(entity, node, stack, transform, visibility)| {
            (
                entity,
                stack.0,
                visibility.get() && node_contains_cursor(node, transform, cursor),
            )
        },
    ));
    let target_area = topmost_at_cursor(areas.iter().map(
        |(entity, _, node, stack, transform, visibility, _)| {
            (
                entity,
                stack.0,
                window_ancestor(entity, &child_of, &app_windows) == target_window
                    && visibility.get()
                    && node_contains_cursor(node, transform, cursor),
            )
        },
    ));
    eprintln!(
        "[scroll] delta={delta} target_window={:?} target_area={:?}",
        target_window, target_area
    );

    for (entity, mut area, node, _, transform, _, children) in areas.iter_mut() {
        if Some(entity) != target_area {
            continue;
        }
        let Some(local) = transform.try_inverse().map(|t| t.transform_point2(cursor)) else {
            continue;
        };
        let half = node.size() * 0.5;
        if local.x.abs() > half.x || local.y.abs() > half.y {
            continue;
        }

        let content_h = children
            .iter()
            .find_map(|c| content_computed.get(c).ok().map(|n| n.size().y))
            .unwrap_or(0.0);
        let viewport_h = node.size().y;
        let max = (content_h - viewport_h).max(0.0);
        eprintln!(
            "[scroll] area={entity:?} content_h={content_h:.0} viewport_h={viewport_h:.0} max={max:.0}"
        );
        if max <= 0.0 {
            continue;
        }

        area.scroll_offset = (area.scroll_offset - delta * WHEEL_SPEED).clamp(0.0, max);
        eprintln!(
            "[scroll] SCROLLED offset={:.0} (content_h={content_h:.0} viewport={viewport_h:.0})",
            area.scroll_offset
        );

        for child in children.iter() {
            if let Ok(mut n) = content_nodes.get_mut(child) {
                let target = Val::Px(-area.scroll_offset);
                if n.top != target {
                    n.top = target;
                }
            }
        }
    }
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
    mut areas: Query<(&mut ScrollableArea, &ComputedNode, &Children)>,
    content_computed: Query<&ComputedNode, With<ScrollContent>>,
) {
    if !mouse.pressed(MouseButton::Left) {
        return;
    }
    if !cursor.active {
        return;
    }
    let cursor = cursor.physical;

    for (interaction, thumb, _, _) in thumbs.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let area_e = thumb.0;
        let Ok(track) = tracks.get(area_e) else {
            continue;
        };
        let Some(local) = track.1.try_inverse().map(|t| t.transform_point2(cursor)) else {
            continue;
        };
        let half = track.0.size() * 0.5;
        let y_from_top = local.y + half.y;
        let ratio = (y_from_top / track.0.size().y).clamp(0.0, 1.0);

        let Ok((mut area, area_node, children)) = areas.get_mut(area_e) else {
            continue;
        };
        let content_h = children
            .iter()
            .find_map(|c| content_computed.get(c).ok().map(|n| n.size().y))
            .unwrap_or(0.0);
        let viewport_h = area_node.size().y;
        let max = (content_h - viewport_h).max(0.0);
        area.scroll_offset = ratio * max;
    }
}

fn scroll_sync_system(
    areas: Query<(Entity, &ScrollableArea, &ComputedNode, &Children)>,
    content_computed: Query<&ComputedNode, With<ScrollContent>>,
    mut node_set: ParamSet<(
        Query<&mut Node, With<ScrollContent>>,
        Query<&mut Node, With<ScrollbarThumb>>,
    )>,
    tracks: Query<(&ScrollTarget, &Children), With<ScrollbarTrack>>,
) {
    for (area_e, area, area_node, children) in areas.iter() {
        let content_h = children
            .iter()
            .find_map(|c| content_computed.get(c).ok().map(|n| n.size().y))
            .unwrap_or(0.0);
        let viewport_h = area_node.size().y;
        let max = (content_h - viewport_h).max(0.0);
        let offset = area.scroll_offset.clamp(0.0, max);

        for child in children.iter() {
            if let Ok(mut n) = node_set.p0().get_mut(child) {
                if let Node {
                    top: Val::Px(top),
                    ..
                } = &mut *n
                {
                    let target = -offset;
                    if (*top - target).abs() > 0.1 {
                        *top = target;
                    }
                }
            }
        }

        for (target, track_children) in tracks.iter() {
            if target.0 != area_e {
                continue;
            }
            for tc in track_children.iter() {
                if let Ok(mut tn) = node_set.p1().get_mut(tc) {
                    if max <= 0.0 {
                        tn.height = Val::Percent(100.0);
                        tn.top = Val::Percent(0.0);
                        continue;
                    }
                    let thumb_pct = (viewport_h / content_h * 100.0).clamp(8.0, 100.0);
                    let travel = 100.0 - thumb_pct;
                    let top_pct = (offset / max) * travel;
                    tn.height = Val::Percent(thumb_pct);
                    tn.top = Val::Percent(top_pct);
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
}
