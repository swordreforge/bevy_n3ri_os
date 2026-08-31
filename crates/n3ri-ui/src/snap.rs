use bevy::prelude::*;

use crate::dock::IsDragging;
use crate::window::{AppWindow, CinematicLocked, WindowDrag};

pub struct SnapPlugin;

impl Plugin for SnapPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(SnapState::default()).add_systems(
            Update,
            (
                detect_snap_zone,
                update_snap_preview.after(detect_snap_zone),
                apply_snap.after(detect_snap_zone),
            ),
        );
    }
}

const EDGE_THRESHOLD: f32 = 20.0;
const CORNER_THRESHOLD: f32 = 80.0;
const TOPBAR_HEIGHT: f32 = 32.0;
const DOCK_TOTAL_HEIGHT: f32 = 66.0;
const PREVIEW_BG: Color = Color::srgba(0.2, 0.5, 1.0, 0.3);
const PREVIEW_BORDER: Color = Color::srgba(0.2, 0.5, 1.0, 0.6);

#[derive(Component)]
struct SnapPreview;

#[derive(Debug, Default, PartialEq, Clone, Copy)]
enum SnapZone {
    #[default]
    None,
    LeftHalf,
    RightHalf,
    Maximize,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Debug, Clone, Copy)]
struct SnapTarget {
    left: f32,
    top: f32,
    width: f32,
    height: f32,
}

#[derive(Resource, Default)]
struct SnapState {
    active_zone: SnapZone,
    preview_entity: Option<Entity>,
    target: Option<SnapTarget>,
    dragged_window: Option<Entity>,
}

fn detect_snap_zone(
    windows: Query<&Window>,
    is_dragging: Res<IsDragging>,
    drag_query: Query<(Entity, &WindowDrag), With<AppWindow>>,
    mut snap_state: ResMut<SnapState>,
) {
    if !is_dragging.0 {
        if snap_state.active_zone != SnapZone::None {
            snap_state.active_zone = SnapZone::None;
            snap_state.target = None;
        }
        return;
    }

    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let screen_w = window.resolution.width();
    let screen_h = window.resolution.height();
    let usable_top = TOPBAR_HEIGHT;
    let usable_h = (screen_h - DOCK_TOTAL_HEIGHT - TOPBAR_HEIGHT).max(0.0);
    let half_w = screen_w / 2.0;
    let half_h = usable_h / 2.0;

    let (zone, target) = if cursor.x < CORNER_THRESHOLD && cursor.y < TOPBAR_HEIGHT + CORNER_THRESHOLD {
        (
            SnapZone::TopLeft,
            SnapTarget {
                left: 0.0,
                top: usable_top,
                width: half_w,
                height: half_h,
            },
        )
    } else if cursor.x > screen_w - CORNER_THRESHOLD && cursor.y < TOPBAR_HEIGHT + CORNER_THRESHOLD {
        (
            SnapZone::TopRight,
            SnapTarget {
                left: half_w,
                top: usable_top,
                width: half_w,
                height: half_h,
            },
        )
    } else if cursor.x < CORNER_THRESHOLD && cursor.y > screen_h - DOCK_TOTAL_HEIGHT - CORNER_THRESHOLD {
        (
            SnapZone::BottomLeft,
            SnapTarget {
                left: 0.0,
                top: usable_top + half_h,
                width: half_w,
                height: half_h,
            },
        )
    } else if cursor.x > screen_w - CORNER_THRESHOLD && cursor.y > screen_h - DOCK_TOTAL_HEIGHT - CORNER_THRESHOLD {
        (
            SnapZone::BottomRight,
            SnapTarget {
                left: half_w,
                top: usable_top + half_h,
                width: half_w,
                height: half_h,
            },
        )
    } else if cursor.x < EDGE_THRESHOLD {
        (
            SnapZone::LeftHalf,
            SnapTarget {
                left: 0.0,
                top: usable_top,
                width: half_w,
                height: usable_h,
            },
        )
    } else if cursor.x > screen_w - EDGE_THRESHOLD {
        (
            SnapZone::RightHalf,
            SnapTarget {
                left: half_w,
                top: usable_top,
                width: half_w,
                height: usable_h,
            },
        )
    } else if cursor.y < TOPBAR_HEIGHT + EDGE_THRESHOLD {
        (
            SnapZone::Maximize,
            SnapTarget {
                left: 0.0,
                top: usable_top,
                width: screen_w,
                height: usable_h,
            },
        )
    } else {
        (
            SnapZone::None,
            SnapTarget {
                left: 0.0,
                top: 0.0,
                width: 0.0,
                height: 0.0,
            },
        )
    };

    snap_state.active_zone = zone;
    snap_state.target = if zone != SnapZone::None {
        Some(target)
    } else {
        None
    };

    if let Some((entity, _drag)) = drag_query.iter().next() {
        snap_state.dragged_window = Some(entity);
    }
}

fn update_snap_preview(
    mut commands: Commands,
    mut snap_state: ResMut<SnapState>,
    preview_query: Query<Entity, With<SnapPreview>>,
) {
    if let Some(entity) = snap_state.preview_entity.take() {
        if preview_query.get(entity).is_ok() {
            commands.entity(entity).despawn();
        }
    }

    if snap_state.active_zone == SnapZone::None || snap_state.target.is_none() {
        return;
    }

    let target = snap_state.target.unwrap();

    let entity = commands
        .spawn((
            SnapPreview,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(target.left),
                top: Val::Px(target.top),
                width: Val::Px(target.width),
                height: Val::Px(target.height),
                border: UiRect::all(Val::Px(2.0)),
                ..default()
            },
            BackgroundColor(PREVIEW_BG),
            BorderColor::all(PREVIEW_BORDER),
            GlobalZIndex(-1),
        ))
        .id();
    snap_state.preview_entity = Some(entity);
}

fn apply_snap(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    mut snap_state: ResMut<SnapState>,
    mut window_query: Query<&mut Node, With<AppWindow>>,
    locked_query: Query<(), With<CinematicLocked>>,
    preview_query: Query<Entity, With<SnapPreview>>,
) {
    if !mouse.just_released(MouseButton::Left) {
        return;
    }

    if let (Some(target), Some(window_entity)) = (snap_state.target, snap_state.dragged_window) {
        // 凑近状态锁：锁定窗口不可吸附
        if locked_query.get(window_entity).is_err() {
            if let Ok(mut node) = window_query.get_mut(window_entity) {
                node.left = Val::Px(target.left);
                node.top = Val::Px(target.top);
                node.width = Val::Px(target.width);
                node.height = Val::Px(target.height);
            }
        }
    }

    if let Some(entity) = snap_state.preview_entity {
        if preview_query.get(entity).is_ok() {
            commands.entity(entity).despawn();
        }
    }

    snap_state.active_zone = SnapZone::None;
    snap_state.target = None;
    snap_state.preview_entity = None;
    snap_state.dragged_window = None;
}
