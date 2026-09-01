use bevy::prelude::*;
use bevy::ecs::relationship::Relationship;

use crate::cursor::{CursorPosition, UiArea};
use crate::font::N3riFonts;
use crate::window::AppWindow;

pub struct DockPlugin;

impl Plugin for DockPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<IsDragging>()
            .add_systems(
                Update,
                (dock_magnification, dock_update, dock_tooltip),
            );
    }
}

#[derive(Resource, Default)]
pub struct IsDragging(pub bool);

const ICON_BASE_SIZE: f32 = 48.0;
const ICON_MAX_SIZE: f32 = 72.0;
const ICON_SPACING: f32 = 4.0;
const ICON_TOTAL: f32 = ICON_BASE_SIZE + ICON_SPACING;
const MAGNETIC_RANGE: f32 = 120.0;
const RUNNING_DOT_COLOR: Color = Color::srgb(0.31, 0.76, 0.97);

#[derive(Component)]
pub struct Dock;

#[derive(Component)]
pub struct DockIcon {
    pub app_name: String,
    icon_a: Handle<Image>,
    icon_b: Option<Handle<Image>>,
    index: usize,
}

#[derive(Component)]
struct DockIconImage;

#[derive(Component)]
struct RunningIndicator;

#[derive(Component)]
struct DockSeparator;

#[derive(Component)]
struct DockTooltip;

#[derive(Component)]
pub struct AppVisible(pub bool);

/// 获取应用的中文显示名称
fn app_display_name(name: &str) -> &str {
    match name {
        "credits" => "致谢",
        "browser" => "浏览器",
        "mail" => "邮件",
        "files" => "文件",
        "signal" => "通讯",
        "pictionary" => "你画我猜",
        "idle" => "算力",
        "chess" => "国际象棋",
        "cakeduel" => "蛋糕对决",
        "codenames" => "森林寻宝",
        "terminal" => "终端",
        "settings" => "设置",
        _ => name,
    }
}

enum DockEntry {
    App {
        name: &'static str,
        has_icon_b: bool,
    },
    Separator,
}

pub fn spawn_dock(parent: &mut ChildSpawnerCommands, asset_server: &AssetServer, fonts: &N3riFonts) {
    let entries: Vec<DockEntry> = vec![
        DockEntry::App { name: "credits", has_icon_b: false },
        DockEntry::App { name: "browser", has_icon_b: true },
        DockEntry::App { name: "mail", has_icon_b: true },
        DockEntry::App { name: "files", has_icon_b: true },
        DockEntry::App { name: "signal", has_icon_b: true },
        DockEntry::App { name: "pictionary", has_icon_b: true },
        DockEntry::App { name: "idle", has_icon_b: true },
        DockEntry::App { name: "chess", has_icon_b: true },
        DockEntry::App { name: "cakeduel", has_icon_b: true },
        DockEntry::App { name: "codenames", has_icon_b: true },
        DockEntry::App { name: "terminal", has_icon_b: true },
        DockEntry::Separator,
        DockEntry::App {
            name: "settings",
            has_icon_b: false,
        },
    ];

    parent
        .spawn((
            Dock,
            GlobalZIndex(50),
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(8.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::FlexEnd,
                padding: UiRect {
                    top: Val::Px(8.0),
                    bottom: Val::Px(4.0),
                    ..default()
                },
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(0.0),
                ..default()
            },
        ))
        .with_children(|dock| {
            let mut icon_index = 0;
            for entry in &entries {
                match entry {
                    DockEntry::Separator => {
                        dock.spawn((
                            DockSeparator,
                            Node {
                                width: Val::Px(1.0),
                                height: Val::Px(40.0),
                                margin: UiRect {
                                    left: Val::Px(8.0),
                                    right: Val::Px(8.0),
                                    top: Val::Px(4.0),
                                    ..default()
                                },
                                ..default()
                            },
                            BackgroundColor(Color::srgba(0.3, 0.4, 0.5, 0.4)),
                        ));
                    }
                    DockEntry::App { name, has_icon_b } => {
                        let icon_a: Handle<Image> =
                            asset_server.load(format!("nori/app-icons/{name}/icon-a.png"));
                        let icon_b: Option<Handle<Image>> = if *has_icon_b {
                            Some(asset_server.load(format!("nori/app-icons/{name}/icon-b.png")))
                        } else {
                            None
                        };

                        let idx = icon_index;
                        icon_index += 1;
                        let initial_image =
                            icon_b.clone().unwrap_or_else(|| icon_a.clone());

                        dock.spawn((
                            DockIcon {
                                app_name: name.to_string(),
                                icon_a: icon_a.clone(),
                                icon_b,
                                index: idx,
                            },
                            Button,
                            Node {
                                flex_direction: FlexDirection::Column,
                                align_items: AlignItems::Center,
                                width: Val::Px(ICON_BASE_SIZE),
                                height: Val::Px(ICON_BASE_SIZE + 10.0),
                                margin: UiRect {
                                    left: Val::Px(2.0),
                                    right: Val::Px(2.0),
                                    ..default()
                                },
                                ..default()
                            },
                        ))
                        .with_children(|icon_slot| {
                            icon_slot.spawn((
                                DockIconImage,
                                ImageNode {
                                    image: initial_image,
                                    ..default()
                                },
                                Node {
                                    width: Val::Px(ICON_BASE_SIZE),
                                    height: Val::Px(ICON_BASE_SIZE),
                                    ..default()
                                },
                            ));

                            icon_slot.spawn((
                                RunningIndicator,
                                Node {
                                    width: Val::Px(6.0),
                                    height: Val::Px(6.0),
                                    margin: UiRect {
                                        top: Val::Px(2.0),
                                        ..default()
                                    },
                                    ..default()
                                },
                                BackgroundColor(RUNNING_DOT_COLOR),
                                Visibility::Hidden,
                            ));

                            icon_slot.spawn((
                                DockTooltip,
                                Text::new(app_display_name(name).to_string()),
                                TextFont {
                                    font: FontSource::Handle(fonts.get(crate::font::FontContext::Dock)),
                                    font_size: FontSize::Px(12.0),
                                    ..default()
                                },
                                TextColor(Color::srgb(0.9, 0.95, 0.95)),
                                Node {
                                    position_type: PositionType::Absolute,
                                    top: Val::Px(-32.0),
                                    padding: UiRect {
                                        left: Val::Px(8.0),
                                        right: Val::Px(8.0),
                                        top: Val::Px(4.0),
                                        bottom: Val::Px(4.0),
                                    },
                                    border_radius: BorderRadius::all(Val::Px(4.0)),
                                    justify_content: JustifyContent::Center,
                                    ..default()
                                },
                                BackgroundColor(Color::srgba(0.1, 0.15, 0.2, 0.9)),
                                Visibility::Hidden,
                            ));
                        });
                    }
                }
            }
        });
}

fn dock_magnification(
    cursor: Res<CursorPosition>,
    area: Res<UiArea>,
    is_dragging: Res<IsDragging>,
    mut icon_query: Query<(&DockIcon, &mut Node), Without<crate::scroll::ScrollbarThumb>>,
) {
    if is_dragging.0 {
        return;
    }
    let screen_width = area.x;
    let screen_height = area.y;

    let dock_bottom_y = screen_height - 8.0 - 4.0;

    let icon_count = icon_query.iter().count() as f32;
    let total_icons_width = icon_count * ICON_TOTAL;
    let dock_start_x = (screen_width - total_icons_width) / 2.0;

    if !cursor.active {
        for (_, mut node) in icon_query.iter_mut() {
            node.width = Val::Px(ICON_BASE_SIZE);
            node.height = Val::Px(ICON_BASE_SIZE + 10.0);
        }
        return;
    }
    let cursor = cursor.logical;

    let y_distance = (cursor.y - dock_bottom_y).abs();
    if y_distance > MAGNETIC_RANGE {
        for (_, mut node) in icon_query.iter_mut() {
            node.width = Val::Px(ICON_BASE_SIZE);
            node.height = Val::Px(ICON_BASE_SIZE + 10.0);
        }
        return;
    }

    for (icon, mut node) in icon_query.iter_mut() {
        let icon_center_x =
            dock_start_x + icon.index as f32 * ICON_TOTAL + ICON_BASE_SIZE / 2.0;

        let distance = (cursor.x - icon_center_x).abs();
        let scale = if distance < MAGNETIC_RANGE {
            let t = 1.0 - distance / MAGNETIC_RANGE;
            let t = t * t;
            ICON_BASE_SIZE + (ICON_MAX_SIZE - ICON_BASE_SIZE) * t
        } else {
            ICON_BASE_SIZE
        };

        node.width = Val::Px(scale);
        node.height = Val::Px(scale + 10.0);
    }
}

fn dock_update(
    mouse: Res<ButtonInput<MouseButton>>,
    icon_query: Query<(&DockIcon, &Interaction, &Children)>,
    mut window_query: Query<(Entity, &AppWindow, &mut AppVisible)>,
    dock_query: Query<&ChildOf, With<Dock>>,
    mut indicator_query: Query<&mut Visibility, With<RunningIndicator>>,
    mut image_query: Query<&mut ImageNode>,
    fonts: Res<N3riFonts>,
    asset_server: Res<AssetServer>,
    mut commands: Commands,
) {
    if mouse.just_pressed(MouseButton::Left) {
        for (icon, interaction, _) in icon_query.iter() {
            if *interaction != Interaction::Pressed {
                continue;
            }

            let mut found = false;
            for (entity, app_window, mut visible) in window_query.iter_mut() {
                if app_window.app_id == icon.app_name {
                    visible.0 = !visible.0;
                    let new_vis = if visible.0 {
                        Visibility::Inherited
                    } else {
                        Visibility::Hidden
                    };
                    commands.entity(entity).insert(new_vis);
                    found = true;
                    break;
                }
            }

            if !found {
                let Ok(dock_parent) = dock_query.single() else {
                    continue;
                };
                let parent_entity = dock_parent.get();

                    match icon.app_name.as_str() {
                        "credits" => {
                            commands.entity(parent_entity).with_children(|parent| {
                                crate::apps::credits::spawn_credits_window(
                                    parent,
                                    &asset_server,
                                    &fonts,
                                );
                            });
                        }
                        "settings" => {
                            commands.entity(parent_entity).with_children(|parent| {
                                crate::apps::settings::spawn_settings_window(parent, &fonts);
                            });
                        }
                        "terminal" => {
                            commands.entity(parent_entity).with_children(|parent| {
                                crate::apps::terminal::spawn_terminal(parent, &fonts);
                            });
                        }
                        "files" => {
                            commands.entity(parent_entity).with_children(|parent| {
                                crate::apps::files::spawn_files(parent, &fonts);
                            });
                        }
                        "mail" => {
                            commands.entity(parent_entity).with_children(|parent| {
                                crate::apps::mail::spawn_mail(parent, &fonts);
                            });
                        }
                        "signal" => {
                            commands.entity(parent_entity).with_children(|parent| {
                                crate::apps::signal::spawn_signal(parent, &fonts);
                            });
                        }
                        "idle" => {
                            commands.entity(parent_entity).with_children(|parent| {
                                crate::apps::clicker::spawn_clicker(parent, &asset_server, &fonts);
                            });
                        }
                        "chess" => {
                            commands.entity(parent_entity).with_children(|parent| {
                                crate::apps::international_chess::spawn_international_chess(
                                    parent,
                                    &asset_server,
                                    &fonts,
                                );
                            });
                        }
                        "pictionary" => {
                            commands.entity(parent_entity).with_children(|parent| {
                                crate::apps::pictionary::spawn_pictionary(parent, &fonts);
                            });
                        }
                        "codenames" => {
                            commands.entity(parent_entity).with_children(|parent| {
                                crate::apps::seek_treasure::spawn_seek_treasure(parent, &fonts);
                            });
                        }
                        "cakeduel" => {
                            commands.entity(parent_entity).with_children(|parent| {
                                crate::apps::cakeduel::spawn_cakeduel(parent, &fonts);
                            });
                        }
                        "browser" => {
                            commands.entity(parent_entity).with_children(|parent| {
                                crate::apps::browser::spawn_browser(parent, &fonts);
                            });
                        }
                        _ => {}
                    }
            }
        }
    }

    for (icon, _, children) in icon_query.iter() {
        let is_running = window_query.iter().any(|(_, w, vis)| {
            w.app_id == icon.app_name && vis.0
        });

        let target = if is_running {
            icon.icon_a.clone()
        } else {
            icon.icon_b.clone().unwrap_or_else(|| icon.icon_a.clone())
        };

        for child in children.iter() {
            if let Ok(mut vis) = indicator_query.get_mut(child) {
                *vis = if is_running {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
            }
            if let Ok(mut img) = image_query.get_mut(child) {
                if img.image != target {
                    img.image = target.clone();
                }
            }
        }
    }
}

fn dock_tooltip(
    icon_query: Query<(&Interaction, &Children), With<DockIcon>>,
    mut tooltip_query: Query<&mut Visibility, With<DockTooltip>>,
) {
    for (interaction, children) in icon_query.iter() {
        let show = *interaction == Interaction::Hovered;
        for child in children.iter() {
            if let Ok(mut vis) = tooltip_query.get_mut(child) {
                *vis = if show {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
            }
        }
    }
}
