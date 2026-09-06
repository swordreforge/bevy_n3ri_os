use crate::cursor::{CursorPosition, UiArea};
use crate::dock::{AppVisible, IsDragging};
use crate::font::N3riFonts;
use bevy::ecs::relationship::Relationship;
use bevy::prelude::*;

use crate::apps::terminal::TerminalState;
use crate::topbar::FocusedTitle;

pub const MAX_WINDOW_Z: i32 = 32;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct WindowFocusSet;

pub struct WindowPlugin;

impl Plugin for WindowPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                promote_added_windows.before(window_focus_system),
                window_focus_system.in_set(WindowFocusSet),
                window_focus_validate
                    .after(window_focus_system)
                    .in_set(WindowFocusSet),
                window_drag_start,
                window_drag_apply,
                window_drag_end,
                handle_close_button,
                handle_minimize_button,
                handle_maximize_button,
            ),
        );
    }
}

fn promote_added_windows(
    mut windows: Query<(Entity, &mut AppWindow, &mut GlobalZIndex)>,
    mut focused: ResMut<FocusedTitle>,
) {
    let mut added = Vec::new();
    let mut existing = Vec::new();
    let mut next_z = 1;
    for (entity, window, _) in windows.iter_mut() {
        if window.is_added() {
            added.push(entity);
        } else {
            existing.push((entity, window.z));
            next_z = next_z.max(window.z + 1);
        }
    }
    if added.is_empty() {
        return;
    }

    if next_z + added.len() as i32 - 1 > MAX_WINDOW_Z {
        existing.sort_by_key(|(_, z)| *z);
        for (index, (entity, _)) in existing.iter().enumerate() {
            if let Ok((_, mut window, mut global_z)) = windows.get_mut(*entity) {
                window.z = index as i32 + 1;
                global_z.0 = window.z;
            }
        }
        next_z = existing.len() as i32 + 1;
    }

    for entity in added {
        if let Ok((_, mut window, mut global_z)) = windows.get_mut(entity) {
            window.z = next_z;
            global_z.0 = next_z;
            focused.entity = Some(entity);
            focused.title.clone_from(&window.title);
            next_z += 1;
        }
    }
}

const TITLE_BAR_HEIGHT: f32 = 32.0;
const TOPBAR_HEIGHT: f32 = 32.0;
const DOCK_BOTTOM_MARGIN: f32 = 8.0;
const DOCK_ICON_SIZE: f32 = 48.0;
const DOCK_TOTAL_HEIGHT: f32 = DOCK_BOTTOM_MARGIN + DOCK_ICON_SIZE + 10.0;
const WINDOW_BG: Color = Color::srgba(0.08, 0.12, 0.2, 0.95);
const TITLE_BAR_BG: Color = Color::srgba(0.05, 0.08, 0.14, 0.98);

#[derive(Component)]
pub struct AppWindow {
    pub title: String,
    pub app_id: String,
    pub z: i32,
}

#[derive(Component)]
pub struct TitleBar;

#[derive(Component)]
pub struct CloseButton;

#[derive(Component)]
pub struct MinimizeButton;

#[derive(Component)]
pub struct MaximizeButton;

#[derive(Component)]
pub struct WindowDrag {
    offset: Vec2,
}

#[derive(Component)]
pub struct WindowOriginalLayout {
    left: Val,
    top: Val,
    width: Val,
    height: Val,
}

#[derive(Component)]
pub struct Maximized;

/// 凑近状态锁：带此标记的窗口不可拖拽/缩放/最小化/最大化/吸附，仅可关闭
#[derive(Component)]
pub struct CinematicLocked;

pub fn spawn_window(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    app_id: &str,
    width: f32,
    height: f32,
    fonts: &N3riFonts,
) -> Entity {
    spawn_window_with_options(parent, title, app_id, width, height, fonts, true)
}

pub fn spawn_window_with_options(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    app_id: &str,
    width: f32,
    height: f32,
    fonts: &N3riFonts,
    show_minimize: bool,
) -> Entity {
    let win_height = height + TITLE_BAR_HEIGHT;
    let init_top = TOPBAR_HEIGHT + 40.0;
    parent
        .spawn((
            AppWindow {
                title: title.to_string(),
                app_id: app_id.to_string(),
                z: 1,
            },
            AppVisible(true),
            GlobalZIndex(1),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(width),
                height: Val::Px(win_height),
                flex_direction: FlexDirection::Column,
                top: Val::Px(init_top),
                left: Val::Px(((1920.0 - width) / 2.0).max(0.0)),
                border_radius: BorderRadius::all(Val::Px(10.0)),
                overflow: Overflow::hidden(),
                ..default()
            },
            BackgroundColor(WINDOW_BG),
        ))
        .with_children(|window| {
            window
                .spawn((
                    TitleBar,
                    Button,
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(TITLE_BAR_HEIGHT),
                        padding: UiRect {
                            left: Val::Px(12.0),
                            right: Val::Px(12.0),
                            ..default()
                        },
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::SpaceBetween,
                        display: Display::Flex,
                        border_radius: BorderRadius {
                            top_left: Val::Px(10.0),
                            top_right: Val::Px(10.0),
                            bottom_left: Val::Px(0.0),
                            bottom_right: Val::Px(0.0),
                        },
                        ..default()
                    },
                    BackgroundColor(TITLE_BAR_BG),
                ))
                .with_children(|title_bar| {
                    title_bar
                        .spawn((Node {
                            align_items: AlignItems::Center,
                            column_gap: Val::Px(8.0),
                            display: Display::Flex,
                            ..default()
                        },))
                        .with_children(|buttons| {
                            buttons.spawn((
                                CloseButton,
                                Button,
                                Node {
                                    width: Val::Px(12.0),
                                    height: Val::Px(12.0),
                                    border: UiRect::all(Val::Px(1.0)),
                                    border_radius: BorderRadius::all(Val::Px(6.0)),
                                    ..default()
                                },
                                BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.2)),
                                BackgroundColor(Color::srgb(1.0, 0.37, 0.34)),
                            ));

                            if show_minimize {
                                buttons.spawn((
                                    MinimizeButton,
                                    Button,
                                    Node {
                                        width: Val::Px(12.0),
                                        height: Val::Px(12.0),
                                        border: UiRect::all(Val::Px(1.0)),
                                        border_radius: BorderRadius::all(Val::Px(6.0)),
                                        ..default()
                                    },
                                    BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.2)),
                                    BackgroundColor(Color::srgb(1.0, 0.74, 0.18)),
                                ));
                            }

                            buttons.spawn((
                                MaximizeButton,
                                Button,
                                Node {
                                    width: Val::Px(12.0),
                                    height: Val::Px(12.0),
                                    border: UiRect::all(Val::Px(1.0)),
                                    border_radius: BorderRadius::all(Val::Px(6.0)),
                                    ..default()
                                },
                                BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.2)),
                                BackgroundColor(Color::srgb(0.16, 0.78, 0.25)),
                            ));
                        });

                    title_bar.spawn((
                        Text::new(title),
                        TextFont {
                            font: FontSource::Handle(fonts.default.clone()),
                            font_size: FontSize::Px(13.0),
                            ..default()
                        },
                        TextColor(Color::srgb(0.86, 0.93, 0.93)),
                    ));

                    title_bar.spawn(Node {
                        width: Val::Px(60.0),
                        height: Val::Px(1.0),
                        ..default()
                    });
                });
        })
        .id()
}

fn window_focus_system(
    mouse: Res<ButtonInput<MouseButton>>,
    cursor: Res<CursorPosition>,
    mut app_query: Query<
        (
            Entity,
            &mut AppWindow,
            &mut GlobalZIndex,
            &ComputedNode,
            &UiGlobalTransform,
            &Visibility,
        ),
        With<AppWindow>,
    >,
    titlebar_query: Query<(&Interaction, &ChildOf), With<TitleBar>>,
    mut focused: ResMut<FocusedTitle>,
) {
    let mut target: Option<Entity> = None;

    if mouse.just_pressed(MouseButton::Left) {
        for (interaction, child_of) in titlebar_query.iter() {
            if *interaction == Interaction::Pressed {
                target = Some(child_of.get());
            }
        }

        if target.is_none() && cursor.active {
            let cursor_pos = cursor.physical;
            let mut best: Option<(Entity, i32)> = None;
            for (e, app, _, node, transform, vis) in app_query.iter() {
                if *vis == Visibility::Hidden {
                    continue;
                }
                let Some(local) =
                    transform.try_inverse().map(|t| t.transform_point2(cursor_pos))
                else {
                    continue;
                };
                let half = node.size() * 0.5;
                if local.x.abs() <= half.x && local.y.abs() <= half.y
                    && best.is_none_or(|(_, bz)| app.z > bz) {
                        best = Some((e, app.z));
                    }
            }
            target = best.map(|(e, _)| e);
        }
    }

    let Some(target) = target else {
        return;
    };
    if focused.entity == Some(target) {
        return;
    }

    let max_z = app_query.iter().map(|(_, a, ..)| a.z).max().unwrap_or(0);
    let mut new_z = max_z + 1;

    if new_z > MAX_WINDOW_Z {
        let mut order: Vec<(Entity, i32)> =
            app_query.iter().map(|(e, a, ..)| (e, a.z)).collect();
        order.sort_by_key(|(_, z)| *z);
        for (i, (e, _)) in order.iter().enumerate() {
            let nz = i as i32 + 1;
            if let Ok((_, mut a, mut gz, ..)) = app_query.get_mut(*e) {
                a.z = nz;
                gz.0 = nz;
            }
        }
        new_z = order.len() as i32;
    }

    if let Ok((_, mut a, mut gz, ..)) = app_query.get_mut(target) {
        a.z = new_z;
        gz.0 = new_z;
        focused.entity = Some(target);
        focused.title = a.title.clone();
    }
}

fn window_focus_validate(
    app_query: Query<(Entity, &AppWindow, &Visibility), With<AppWindow>>,
    mut focused: ResMut<FocusedTitle>,
) {
    let Some(fe) = focused.entity else {
        return;
    };

    let invalid = match app_query.get(fe) {
        Err(_) => true,
        Ok((_, _, vis)) => *vis == Visibility::Hidden,
    };
    if !invalid {
        return;
    }

    let mut best: Option<(Entity, i32)> = None;
    for (e, a, vis) in app_query.iter() {
        if *vis == Visibility::Hidden {
            continue;
        }
        if best.is_none_or(|(_, bz)| a.z > bz) {
            best = Some((e, a.z));
        }
    }

    match best {
        Some((e, _)) => {
            focused.entity = Some(e);
            if let Ok((_, a, _)) = app_query.get(e) {
                focused.title = a.title.clone();
            }
        }
        None => {
            focused.entity = None;
            focused.title = "n3ri_os".to_string();
        }
    }
}

fn find_window_entity(
    start: Entity,
    child_of_query: &Query<&ChildOf>,
    window_query: &Query<Entity, With<AppWindow>>,
) -> Option<Entity> {
    let mut current = start;
    loop {
        if window_query.get(current).is_ok() {
            return Some(current);
        }
        match child_of_query.get(current) {
            Ok(parent) => current = parent.get(),
            Err(_) => return None,
        }
    }
}

fn window_drag_start(
    mouse: Res<ButtonInput<MouseButton>>,
    cursor: Res<CursorPosition>,
    title_bars: Query<(&ChildOf, &Interaction), With<TitleBar>>,
    child_of_query: Query<&ChildOf>,
    window_query: Query<Entity, With<AppWindow>>,
    window_nodes: Query<&Node, With<AppWindow>>,
    locked_query: Query<(), With<CinematicLocked>>,
    mut is_dragging: ResMut<IsDragging>,
    mut commands: Commands,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    if is_dragging.0 {
        return;
    }
    if !cursor.active {
        return;
    }
    let cursor = cursor.logical;

    for (parent, interaction) in title_bars.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }

        let title_bar_parent = parent.get();
        let Some(window_entity) =
            find_window_entity(title_bar_parent, &child_of_query, &window_query)
        else {
            continue;
        };

        // 凑近状态锁：锁定窗口不可拖拽
        if locked_query.get(window_entity).is_ok() {
            continue;
        }

        if let Ok(win_node) = window_nodes.get(window_entity) {
            let win_left = match win_node.left {
                Val::Px(px) => px,
                _ => 0.0,
            };
            let win_top = match win_node.top {
                Val::Px(px) => px,
                _ => 0.0,
            };
            let offset = cursor - Vec2::new(win_left, win_top);
            commands
                .entity(window_entity)
                .insert(WindowDrag { offset });
            is_dragging.0 = true;
        }
    }
}

fn window_drag_apply(
    cursor: Res<CursorPosition>,
    area: Res<UiArea>,
    mut drag_query: Query<(Entity, &mut Node, &WindowDrag), With<AppWindow>>,
) {
    if !cursor.active {
        return;
    }
    let cursor = cursor.logical;
    let screen_h = area.y;

    let min_top = TOPBAR_HEIGHT;
    let max_top = (screen_h - DOCK_TOTAL_HEIGHT).max(min_top);

    for (_entity, mut style, drag) in drag_query.iter_mut() {
        let new_left = cursor.x - drag.offset.x;
        let new_top = (cursor.y - drag.offset.y).clamp(min_top, max_top);
        style.left = Val::Px(new_left);
        style.top = Val::Px(new_top);
    }
}

fn window_drag_end(
    mouse: Res<ButtonInput<MouseButton>>,
    drag_query: Query<Entity, With<WindowDrag>>,
    mut is_dragging: ResMut<IsDragging>,
    mut commands: Commands,
) {
    if !mouse.just_released(MouseButton::Left) {
        return;
    }
    for entity in drag_query.iter() {
        commands.entity(entity).remove::<WindowDrag>();
    }
    is_dragging.0 = false;
}

fn handle_close_button(
    mouse: Res<ButtonInput<MouseButton>>,
    query: Query<(Entity, &CloseButton, &Interaction)>,
    child_of_query: Query<&ChildOf>,
    window_query: Query<&AppWindow>,
    mut terminal_state: ResMut<TerminalState>,
    mut commands: Commands,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }

    for (entity, _, interaction) in query.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }

        let mut current = entity;
        let mut window_entity: Option<Entity> = None;
        loop {
            if window_query.get(current).is_ok() {
                window_entity = Some(current);
                break;
            }
            match child_of_query.get(current) {
                Ok(parent) => current = parent.get(),
                Err(_) => break,
            }
        }

        if let Some(window_entity) = window_entity {
            if let Ok(app) = window_query.get(window_entity) {
                if app.app_id == "terminal" {
                    terminal_state.reset();
                }
            }
            commands.entity(window_entity).despawn();
        }
    }
}

fn handle_minimize_button(
    mouse: Res<ButtonInput<MouseButton>>,
    query: Query<(Entity, &MinimizeButton, &Interaction)>,
    child_of_query: Query<&ChildOf>,
    window_query: Query<Entity, With<AppWindow>>,
    locked_query: Query<(), With<CinematicLocked>>,
    mut app_visible_query: Query<&mut AppVisible>,
    mut commands: Commands,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }

    for (entity, _, interaction) in query.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }

        let Some(window_entity) =
            find_window_entity(entity, &child_of_query, &window_query)
        else {
            continue;
        };

        // 凑近状态锁：锁定窗口不可最小化
        if locked_query.get(window_entity).is_ok() {
            continue;
        }

        if let Ok(mut vis) = app_visible_query.get_mut(window_entity) {
            vis.0 = false;
        } else {
            commands.entity(window_entity).insert(AppVisible(false));
        }
        commands.entity(window_entity).insert(Visibility::Hidden);
    }
}

fn handle_maximize_button(
    mouse: Res<ButtonInput<MouseButton>>,
    area: Res<UiArea>,
    query: Query<(Entity, &MaximizeButton, &Interaction)>,
    child_of_query: Query<&ChildOf>,
    window_query: Query<Entity, With<AppWindow>>,
    locked_query: Query<(), With<CinematicLocked>>,
    mut node_query: Query<&mut Node, With<AppWindow>>,
    original_layout_query: Query<&WindowOriginalLayout>,
    mut commands: Commands,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }

    for (entity, _, interaction) in query.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }

        let Some(window_entity) =
            find_window_entity(entity, &child_of_query, &window_query)
        else {
            continue;
        };

        // 凑近状态锁：锁定窗口不可最大化
        if locked_query.get(window_entity).is_ok() {
            continue;
        }

        if original_layout_query.get(window_entity).is_ok() {
            if let Ok(layout) = original_layout_query.get(window_entity) {
                if let Ok(mut node) = node_query.get_mut(window_entity) {
                    node.left = layout.left;
                    node.top = layout.top;
                    node.width = layout.width;
                    node.height = layout.height;
                }
                commands.entity(window_entity).remove::<WindowOriginalLayout>();
            }
        } else {
            if let Ok(mut node) = node_query.get_mut(window_entity) {
                let orig_left = node.left;
                let orig_top = node.top;
                let orig_width = node.width;
                let orig_height = node.height;

                commands.entity(window_entity).insert(WindowOriginalLayout {
                    left: orig_left,
                    top: orig_top,
                    width: orig_width,
                    height: orig_height,
                });

                let screen_w = area.x;
                let screen_h = area.y;
                let usable_top = TOPBAR_HEIGHT;
                let usable_h = (screen_h - DOCK_TOTAL_HEIGHT - TOPBAR_HEIGHT).max(0.0);
                node.left = Val::Px(0.0);
                node.top = Val::Px(usable_top);
                node.width = Val::Px(screen_w);
                node.height = Val::Px(usable_h);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn added_window_is_promoted_above_existing_window() {
        // Given
        let mut app = App::new();
        let existing = app
            .world_mut()
            .spawn((
                AppWindow {
                    title: "旧窗口".to_string(),
                    app_id: "old".to_string(),
                    z: 7,
                },
                GlobalZIndex(7),
            ))
            .id();
        app.init_resource::<FocusedTitle>()
            .add_systems(Update, promote_added_windows);
        app.update();
        app.world_mut().get_mut::<AppWindow>(existing).unwrap().z = 7;
        app.world_mut().get_mut::<GlobalZIndex>(existing).unwrap().0 = 7;

        // When
        let added = app
            .world_mut()
            .spawn((
                AppWindow {
                    title: "新窗口".to_string(),
                    app_id: "new".to_string(),
                    z: 1,
                },
                GlobalZIndex(1),
            ))
            .id();
        app.update();

        // Then
        let existing_z = app.world().get::<AppWindow>(existing).unwrap().z;
        let added_z = app.world().get::<AppWindow>(added).unwrap().z;
        assert!(
            added_z > existing_z,
            "new window z ({added_z}) must be above existing window z ({existing_z})"
        );
        assert_eq!(app.world().resource::<FocusedTitle>().entity, Some(added));
    }

    #[test]
    fn added_window_compacts_z_order_before_exceeding_limit() {
        // Given
        let mut app = App::new();
        let existing = app
            .world_mut()
            .spawn((
                AppWindow {
                    title: "旧窗口".to_string(),
                    app_id: "old".to_string(),
                    z: MAX_WINDOW_Z,
                },
                GlobalZIndex(MAX_WINDOW_Z),
            ))
            .id();
        app.init_resource::<FocusedTitle>()
            .add_systems(Update, promote_added_windows);
        app.update();
        app.world_mut().get_mut::<AppWindow>(existing).unwrap().z = MAX_WINDOW_Z;
        app.world_mut().get_mut::<GlobalZIndex>(existing).unwrap().0 = MAX_WINDOW_Z;

        // When
        let added = app
            .world_mut()
            .spawn((
                AppWindow {
                    title: "新窗口".to_string(),
                    app_id: "new".to_string(),
                    z: 1,
                },
                GlobalZIndex(1),
            ))
            .id();
        app.update();

        // Then
        let existing_z = app.world().get::<AppWindow>(existing).unwrap().z;
        let added_z = app.world().get::<AppWindow>(added).unwrap().z;
        assert!(added_z > existing_z);
        assert!(added_z <= MAX_WINDOW_Z);
    }
}
