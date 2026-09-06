use bevy::prelude::*;
use serde::Deserialize;
use crate::font::N3riFonts;
use crate::scroll::{ScrollableArea, ScrollContent};
use crate::window::spawn_window_with_options;
use std::path::Path;

const NORI_BG: Color = Color::srgb(0.655, 0.835, 0.867);   // RGB(167,213,221)
const ADMIN_BG: Color = Color::srgb(0.247, 0.3176, 0.396); // RGB(63,81,101)
const POEM_BG: Color = Color::srgba(0.15, 0.18, 0.25, 0.6); // 诗句背景
const TEXT_DARK: Color = Color::srgb(0.1, 0.12, 0.15);
const TEXT_LIGHT: Color = Color::srgb(0.9, 0.95, 0.95);
const POEM_TEXT: Color = Color::srgb(0.75, 0.88, 0.92); // 诗句文字颜色

#[derive(Deserialize)]
struct DialogueMessage {
    sender: String,
    text: String,
    poem: Option<String>,
    image: Option<String>,
}

pub struct LogViewerPlugin;

impl Plugin for LogViewerPlugin {
    fn build(&self, _app: &mut App) {}
}

pub fn spawn_log_viewer(parent: &mut ChildSpawnerCommands, file_path: &str, fonts: &N3riFonts, asset_server: &AssetServer) {
    let file_name = file_path.rsplit('/').next().unwrap_or("日志");
    let title = format!("{} - 对话查看", file_name);
    let raw = crate::content::read_to_string(file_path).unwrap_or_else(|| "[]".to_string());
    let messages: Vec<DialogueMessage> = serde_json::from_str(&raw).unwrap_or_default();
    let file_dir = Path::new(file_path).parent().unwrap_or(Path::new("")).to_string_lossy().to_string();

    let window_entity = spawn_window_with_options(parent, &title, "log_viewer", 700.0, 600.0, fonts, false);

    parent.commands().entity(window_entity).with_children(|window| {
        let area_e = window.spawn((
            ScrollableArea,
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                align_items: AlignItems::FlexStart,
                overflow: Overflow::hidden(),
                ..default()
            },
        )).id();

        window.commands().entity(area_e).with_children(|a| {
            crate::scroll::spawn_scrollbar(a, area_e);
        });

        window.commands().entity(area_e).with_children(|area| {
            area.spawn((
                ScrollContent,
                Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(16.0)),
                    row_gap: Val::Px(12.0),
                    ..default()
                },
            ))
            .with_children(|content_area| {
                for msg in &messages {
                    let is_nori = msg.sender == "Nori";
                    spawn_chat_bubble(content_area, msg, is_nori, fonts, &file_dir, asset_server);
                }
            });
        });
    });
}

pub fn spawn_log_viewer_direct(commands: &mut Commands, file_path: &str, fonts: &N3riFonts, asset_server: &AssetServer) {
    let file_name = file_path.rsplit('/').next().unwrap_or("日志");
    let title = format!("{} - 对话查看", file_name);
    let raw = crate::content::read_to_string(file_path).unwrap_or_else(|| "[]".to_string());
    let messages: Vec<DialogueMessage> = serde_json::from_str(&raw).unwrap_or_default();
    let fonts_handle = fonts.default.clone();
    let title_clone = title.clone();
    let file_dir = Path::new(file_path).parent().unwrap_or(Path::new("")).to_string_lossy().to_string();

    commands.spawn((
        crate::window::AppWindow {
            title,
            app_id: "log_viewer".to_string(),
            z: 1,
        },
        crate::dock::AppVisible(true),
        GlobalZIndex(1),
        Node {
            position_type: PositionType::Absolute,
            width: Val::Px(700.0),
            height: Val::Px(632.0),
            flex_direction: FlexDirection::Column,
            top: Val::Px(72.0),
            left: Val::Px(560.0),
            border_radius: BorderRadius::all(Val::Px(10.0)),
            overflow: Overflow::hidden(),
            ..default()
        },
        BackgroundColor(Color::srgba(0.08, 0.12, 0.2, 0.95)),
    ))
    .with_children(|window| {
        window.spawn((
            crate::window::TitleBar,
            Button,
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(32.0),
                padding: UiRect { left: Val::Px(12.0), right: Val::Px(12.0), ..default() },
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                display: Display::Flex,
                border_radius: BorderRadius {
                    top_left: Val::Px(10.0), top_right: Val::Px(10.0),
                    bottom_left: Val::Px(0.0), bottom_right: Val::Px(0.0),
                },
                ..default()
            },
            BackgroundColor(Color::srgba(0.05, 0.08, 0.14, 0.98)),
        ))
        .with_children(|title_bar| {
            title_bar.spawn((Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(8.0),
                display: Display::Flex,
                ..default()
            },))
            .with_children(|buttons| {
                buttons.spawn((
                    crate::window::CloseButton, Button,
                    Node { width: Val::Px(12.0), height: Val::Px(12.0), border: UiRect::all(Val::Px(1.0)), border_radius: BorderRadius::all(Val::Px(6.0)), ..default() },
                    BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.2)),
                    BackgroundColor(Color::srgb(1.0, 0.37, 0.34)),
                ));
                buttons.spawn((
                    crate::window::MaximizeButton, Button,
                    Node { width: Val::Px(12.0), height: Val::Px(12.0), border: UiRect::all(Val::Px(1.0)), border_radius: BorderRadius::all(Val::Px(6.0)), ..default() },
                    BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.2)),
                    BackgroundColor(Color::srgb(0.16, 0.78, 0.25)),
                ));
            });

            title_bar.spawn((
                Text::new(title_clone),
                TextFont { font: FontSource::Handle(fonts_handle.clone()), font_size: FontSize::Px(13.0), ..default() },
                TextColor(TEXT_LIGHT),
            ));

            title_bar.spawn(Node { width: Val::Px(60.0), height: Val::Px(1.0), ..default() });
        });

        let area_e = window.spawn((
            ScrollableArea,
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                align_items: AlignItems::FlexStart,
                overflow: Overflow::hidden(),
                ..default()
            },
        )).id();

        window.commands().entity(area_e).with_children(|a| {
            crate::scroll::spawn_scrollbar(a, area_e);
        });

        window.commands().entity(area_e).with_children(|area| {
            area.spawn((
                ScrollContent,
                Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(16.0)),
                    row_gap: Val::Px(12.0),
                    ..default()
                },
            ))
            .with_children(|content_area| {
                for msg in &messages {
                    let is_nori = msg.sender == "Nori";
                    spawn_chat_bubble(content_area, msg, is_nori, fonts, &file_dir, asset_server);
                }
            });
        });
    });
}

fn spawn_chat_bubble(parent: &mut ChildSpawnerCommands, msg: &DialogueMessage, is_nori: bool, fonts: &N3riFonts, file_dir: &str, asset_server: &AssetServer) {
    let bg_color = if is_nori { NORI_BG } else { ADMIN_BG };
    let text_color = if is_nori { TEXT_DARK } else { TEXT_LIGHT };

    let (justify, border_radius) = if is_nori {
        (JustifyContent::FlexStart, BorderRadius {
            top_left: Val::Px(4.0),
            top_right: Val::Px(12.0),
            bottom_left: Val::Px(12.0),
            bottom_right: Val::Px(12.0),
        })
    } else {
        (JustifyContent::FlexEnd, BorderRadius {
            top_left: Val::Px(12.0),
            top_right: Val::Px(4.0),
            bottom_left: Val::Px(12.0),
            bottom_right: Val::Px(12.0),
        })
    };

    parent.spawn(Node {
        width: Val::Percent(100.0),
        justify_content: justify,
        ..default()
    })
    .with_children(|row| {
        row.spawn((
            Node {
                max_width: Val::Percent(75.0),
                padding: UiRect::all(Val::Px(12.0)),
                border_radius,
                ..default()
            },
            BackgroundColor(bg_color),
        ))
        .with_children(|bubble| {
            bubble.spawn((
                Text::new(&msg.text),
                TextFont { font: FontSource::Handle(fonts.default.clone()), font_size: FontSize::Px(14.0), ..default() },
                TextColor(text_color),
            ));
        });
    });

    if let Some(poem) = &msg.poem {
        parent.spawn(Node {
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            padding: UiRect::vertical(Val::Px(8.0)),
            ..default()
        })
        .with_children(|poem_row| {
            poem_row.spawn((
                Node {
                    padding: UiRect::all(Val::Px(16.0)),
                    border_radius: BorderRadius::all(Val::Px(8.0)),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(4.0),
                    ..default()
                },
                BackgroundColor(POEM_BG),
            ))
            .with_children(|poem_block| {
                for line in poem.lines() {
                    poem_block.spawn((
                        Text::new(line),
                        TextFont { font: FontSource::Handle(fonts.default.clone()), font_size: FontSize::Px(15.0), ..default() },
                        TextColor(POEM_TEXT),
                    ));
                }
            });
        });
    }

    if let Some(image) = &msg.image {
        let img_path = Path::new(file_dir).join(image);
        let assets_dir = std::env::current_dir()
            .unwrap_or_default()
            .join("assets");
        let relative_path = img_path.strip_prefix(&assets_dir)
            .unwrap_or(&img_path)
            .to_string_lossy()
            .to_string();

        parent.spawn(Node {
            width: Val::Percent(100.0),
            justify_content: if is_nori { JustifyContent::FlexStart } else { JustifyContent::FlexEnd },
            padding: UiRect::vertical(Val::Px(4.0)),
            ..default()
        })
        .with_children(|img_row| {
            img_row.spawn((
                Node {
                    width: Val::Px(280.0),
                    height: Val::Px(200.0),
                    border_radius: BorderRadius::all(Val::Px(8.0)),
                    overflow: Overflow::hidden(),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.1, 0.12, 0.15, 0.4)),
            ))
            .with_children(|img_container| {
                img_container.spawn((
                    ImageNode {
                        image: asset_server.load(&relative_path),
                        ..default()
                    },
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                ));
            });
        });
    }
}
