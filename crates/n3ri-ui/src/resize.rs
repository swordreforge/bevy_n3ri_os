use bevy::prelude::*;
use bevy::window::{CursorIcon, SystemCursorIcon};

use crate::dock::IsDragging;
use crate::window::{AppWindow, CinematicLocked};

// allow: SIZE_OK — spec-driven resize module; edge geometry + cursor mapping + 4 systems form one cohesive unit.

pub struct ResizePlugin;

impl Plugin for ResizePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ResizeState::default()).add_systems(
            Update,
            (
                resize_hover_cursor,
                resize_start,
                resize_apply.after(resize_start),
                resize_end.after(resize_start),
            ),
        );
    }
}

const RESIZE_HANDLE_SIZE: f32 = 8.0; // pixels from edge to detect resize
const MIN_WINDOW_WIDTH: f32 = 200.0;
const MIN_WINDOW_HEIGHT: f32 = 100.0;
const TOPBAR_HEIGHT: f32 = 32.0;

#[derive(Debug, Default, PartialEq, Clone, Copy)]
enum ResizeEdge {
    #[default]
    None,
    Top,
    Bottom,
    Left,
    Right,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Resource, Default)]
pub struct ResizeState {
    active_edge: ResizeEdge,
    start_cursor: Vec2,
    start_left: f32,
    start_top: f32,
    start_width: f32,
    start_height: f32,
    resizing_window: Option<Entity>,
}

fn node_bounds(node: &Node) -> (f32, f32, f32, f32) {
    let left = match node.left {
        Val::Px(px) => px,
        _ => 0.0,
    };
    let top = match node.top {
        Val::Px(px) => px,
        _ => 0.0,
    };
    let width = match node.width {
        Val::Px(px) => px,
        _ => 0.0,
    };
    let height = match node.height {
        Val::Px(px) => px,
        _ => 0.0,
    };
    (left, top, width, height)
}

fn detect_edge(cursor: Vec2, left: f32, top: f32, width: f32, height: f32) -> ResizeEdge {
    let right = left + width;
    let bottom = top + height;
    let handle = RESIZE_HANDLE_SIZE;
    let title_bar_bottom = top + TOPBAR_HEIGHT;

    // Corners take priority over edges. Top corners are only active below the
    // title bar so they don't fight with title-bar dragging.
    if cursor.x < left + handle
        && cursor.y > title_bar_bottom
        && cursor.y < title_bar_bottom + handle
    {
        ResizeEdge::TopLeft
    } else if cursor.x > right - handle
        && cursor.y > title_bar_bottom
        && cursor.y < title_bar_bottom + handle
    {
        ResizeEdge::TopRight
    } else if cursor.x < left + handle && cursor.y > bottom - handle {
        ResizeEdge::BottomLeft
    } else if cursor.x > right - handle && cursor.y > bottom - handle {
        ResizeEdge::BottomRight
    } else if cursor.x < left + handle {
        ResizeEdge::Left
    } else if cursor.x > right - handle {
        ResizeEdge::Right
    } else if cursor.y > title_bar_bottom && cursor.y < title_bar_bottom + handle {
        ResizeEdge::Top
    } else if cursor.y > bottom - handle {
        ResizeEdge::Bottom
    } else {
        ResizeEdge::None
    }
}

fn edge_to_cursor_icon(edge: ResizeEdge) -> Option<SystemCursorIcon> {
    match edge {
        ResizeEdge::Left | ResizeEdge::Right => Some(SystemCursorIcon::EwResize),
        ResizeEdge::Top | ResizeEdge::Bottom => Some(SystemCursorIcon::NsResize),
        ResizeEdge::TopLeft | ResizeEdge::BottomRight => Some(SystemCursorIcon::NwseResize),
        ResizeEdge::TopRight | ResizeEdge::BottomLeft => Some(SystemCursorIcon::NeswResize),
        ResizeEdge::None => None,
    }
}

fn resize_hover_cursor(
    windows: Query<(Entity, &Window)>,
    window_query: Query<(Entity, &Node, &Visibility, &AppWindow), With<AppWindow>>,
    is_dragging: Res<IsDragging>,
    resize_state: Res<ResizeState>,
    mut commands: Commands,
) {
    if is_dragging.0 || resize_state.resizing_window.is_some() {
        return;
    }
    let Ok((window_entity, bevy_window)) = windows.single() else {
        return;
    };
    let Some(cursor) = bevy_window.cursor_position() else {
        commands.entity(window_entity).remove::<CursorIcon>();
        return;
    };

    let mut candidates: Vec<_> = window_query.iter().collect();
    candidates.sort_by_key(|(_, _, _, a)| -a.z);

    let mut detected_edge = ResizeEdge::None;
    for (_entity, node, visibility, _) in candidates {
        if *visibility != Visibility::Visible {
            continue;
        }
        let (left, top, width, height) = node_bounds(node);
        let edge = detect_edge(cursor, left, top, width, height);
        if edge != ResizeEdge::None {
            detected_edge = edge;
            break;
        }
    }

    match edge_to_cursor_icon(detected_edge) {
        Some(icon) => {
            commands
                .entity(window_entity)
                .insert(CursorIcon::System(icon));
        }
        None => {
            commands.entity(window_entity).remove::<CursorIcon>();
        }
    }
}

fn resize_start(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    window_query: Query<(Entity, &Node, &Visibility, &AppWindow), With<AppWindow>>,
    locked_query: Query<(), With<CinematicLocked>>,
    mut resize_state: ResMut<ResizeState>,
    mut is_dragging: ResMut<IsDragging>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    // 防止拖拽和缩放同时触发
    if is_dragging.0 {
        return;
    }
    let Ok(bevy_window) = windows.single() else {
        return;
    };
    let Some(cursor) = bevy_window.cursor_position() else {
        return;
    };

    let mut candidates: Vec<_> = window_query.iter().collect();
    candidates.sort_by_key(|(_, _, _, a)| -a.z);

    for (entity, node, visibility, _) in candidates {
        if *visibility != Visibility::Visible {
            continue;
        }
        // 凑近状态锁：锁定窗口不可缩放
        if locked_query.get(entity).is_ok() {
            continue;
        }
        let (left, top, width, height) = node_bounds(node);
        let edge = detect_edge(cursor, left, top, width, height);
        if edge != ResizeEdge::None {
            resize_state.active_edge = edge;
            resize_state.start_cursor = cursor;
            resize_state.start_left = left;
            resize_state.start_top = top;
            resize_state.start_width = width;
            resize_state.start_height = height;
            resize_state.resizing_window = Some(entity);
            is_dragging.0 = true;
            return;
        }
    }
}

fn resize_apply(
    windows: Query<&Window>,
    resize_state: Res<ResizeState>,
    mut window_query: Query<&mut Node, With<AppWindow>>,
) {
    let Some(window_entity) = resize_state.resizing_window else {
        return;
    };
    let Ok(bevy_window) = windows.single() else {
        return;
    };
    let Some(cursor) = bevy_window.cursor_position() else {
        return;
    };
    let Ok(mut node) = window_query.get_mut(window_entity) else {
        return;
    };

    let delta = cursor - resize_state.start_cursor;
    let start_left = resize_state.start_left;
    let start_top = resize_state.start_top;
    let start_width = resize_state.start_width;
    let start_height = resize_state.start_height;

    let (new_left, new_top, new_width, new_height) = match resize_state.active_edge {
        ResizeEdge::Right => (
            start_left,
            start_top,
            (start_width + delta.x).max(MIN_WINDOW_WIDTH),
            start_height,
        ),
        ResizeEdge::Left => {
            let clamped_delta =
                delta.x.clamp(-start_left, start_width - MIN_WINDOW_WIDTH);
            (
                start_left + clamped_delta,
                start_top,
                start_width - clamped_delta,
                start_height,
            )
        }
        ResizeEdge::Bottom => (
            start_left,
            start_top,
            start_width,
            (start_height + delta.y).max(MIN_WINDOW_HEIGHT),
        ),
        ResizeEdge::Top => {
            let clamped_delta =
                delta.y.clamp(-start_top + TOPBAR_HEIGHT, start_height - MIN_WINDOW_HEIGHT);
            (
                start_left,
                start_top + clamped_delta,
                start_width,
                start_height - clamped_delta,
            )
        }
        ResizeEdge::TopLeft => {
            let clamped_dx = delta.x.clamp(-start_left, start_width - MIN_WINDOW_WIDTH);
            let clamped_dy =
                delta.y.clamp(-start_top + TOPBAR_HEIGHT, start_height - MIN_WINDOW_HEIGHT);
            (
                start_left + clamped_dx,
                start_top + clamped_dy,
                start_width - clamped_dx,
                start_height - clamped_dy,
            )
        }
        ResizeEdge::TopRight => {
            let new_width = (start_width + delta.x).max(MIN_WINDOW_WIDTH);
            let clamped_dy =
                delta.y.clamp(-start_top + TOPBAR_HEIGHT, start_height - MIN_WINDOW_HEIGHT);
            (
                start_left,
                start_top + clamped_dy,
                new_width,
                start_height - clamped_dy,
            )
        }
        ResizeEdge::BottomLeft => {
            let clamped_dx = delta.x.clamp(-start_left, start_width - MIN_WINDOW_WIDTH);
            let new_height = (start_height + delta.y).max(MIN_WINDOW_HEIGHT);
            (
                start_left + clamped_dx,
                start_top,
                start_width - clamped_dx,
                new_height,
            )
        }
        ResizeEdge::BottomRight => (
            start_left,
            start_top,
            (start_width + delta.x).max(MIN_WINDOW_WIDTH),
            (start_height + delta.y).max(MIN_WINDOW_HEIGHT),
        ),
        ResizeEdge::None => (start_left, start_top, start_width, start_height),
    };

    node.left = Val::Px(new_left);
    node.top = Val::Px(new_top);
    node.width = Val::Px(new_width);
    node.height = Val::Px(new_height);
}

fn resize_end(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<Entity, With<Window>>,
    mut resize_state: ResMut<ResizeState>,
    mut is_dragging: ResMut<IsDragging>,
    mut commands: Commands,
) {
    if !mouse.just_released(MouseButton::Left) {
        return;
    }
    if resize_state.resizing_window.is_some() {
        *resize_state = ResizeState::default();
        is_dragging.0 = false;
        if let Ok(os_window) = windows.single() {
            commands.entity(os_window).remove::<CursorIcon>();
        }
    }
}
