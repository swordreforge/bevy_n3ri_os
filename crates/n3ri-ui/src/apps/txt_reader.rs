use bevy::prelude::*;
use crate::font::N3riFonts;
use crate::scroll::{ScrollableArea, ScrollContent};
use crate::window::spawn_window_with_options;

const TEXT_MAIN: Color = Color::srgb(0.86, 0.93, 0.93);

pub struct TxtReaderPlugin;

impl Plugin for TxtReaderPlugin {
    fn build(&self, _app: &mut App) {}
}

pub fn spawn_txt_reader(parent: &mut ChildSpawnerCommands, file_path: &str, fonts: &N3riFonts) {
    let file_name = file_path.rsplit('/').next().unwrap_or("文本");
    let title = format!("{} - 文本查看", file_name);
    let raw = crate::content::read_to_string(file_path).unwrap_or_else(|| "无法读取文件内容".to_string());
    let content = wrap_lines(&raw, 200);

    let window_entity = spawn_window_with_options(parent, &title, "txt_reader", 600.0, 500.0, fonts, false);

    parent.commands().entity(window_entity).with_children(|window| {
        let area_e = window.spawn((
            ScrollableArea::default(),
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
                    row_gap: Val::Px(2.0),
                    ..default()
                },
            ))
            .with_children(|content_area| {
                for line in content.lines() {
                    content_area.spawn((
                        Text::new(line),
                        TextFont {
                            font: FontSource::Handle(fonts.default.clone()),
                            font_size: FontSize::Px(14.0),
                            ..default()
                        },
                        TextColor(TEXT_MAIN),
                    ));
                }
            });
        });
    });
}

pub fn spawn_txt_reader_direct(commands: &mut Commands, file_path: &str, fonts: &N3riFonts) {
    let file_name = file_path.rsplit('/').next().unwrap_or("文本");
    let title = format!("{} - 文本查看", file_name);
    let raw = crate::content::read_to_string(file_path).unwrap_or_else(|| "无法读取文件内容".to_string());
    let content = wrap_lines(&raw, 200);
    let fonts_handle = fonts.default.clone();
    let title_clone = title.clone();

    commands.spawn((
        crate::window::AppWindow {
            title,
            app_id: "txt_reader".to_string(),
            z: 1,
        },
        crate::dock::AppVisible(true),
        GlobalZIndex(1),
        Node {
            position_type: PositionType::Absolute,
            width: Val::Px(600.0),
            height: Val::Px(532.0),
            flex_direction: FlexDirection::Column,
            top: Val::Px(72.0),
            left: Val::Px(660.0),
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
                TextColor(Color::srgb(0.86, 0.93, 0.93)),
            ));

            title_bar.spawn(Node { width: Val::Px(60.0), height: Val::Px(1.0), ..default() });
        });

        let area_e = window.spawn((
            ScrollableArea::default(),
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
                    row_gap: Val::Px(2.0),
                    ..default()
                },
            ))
            .with_children(|content_area| {
                for line in content.lines() {
                    content_area.spawn((
                        Text::new(line),
                        TextFont { font: FontSource::Handle(fonts_handle.clone()), font_size: FontSize::Px(14.0), ..default() },
                        TextColor(TEXT_MAIN),
                    ));
                }
            });
        });
    });
}

fn wrap_lines(text: &str, max_len: usize) -> String {
    let mut result = String::with_capacity(text.len());
    for line in text.lines() {
        if line.len() <= max_len {
            result.push_str(line);
        } else {
            let mut start = 0;
            for (i, _) in line.char_indices() {
                if i - start >= max_len {
                    result.push_str(&line[start..i]);
                    result.push('\n');
                    start = i;
                }
            }
            result.push_str(&line[start..]);
        }
        result.push('\n');
    }
    result
}
