use bevy::prelude::*;
use crate::font::N3riFonts;
use crate::scroll::{ScrollableArea, ScrollContent};
use std::path::Path;

const TEXT_LIGHT: Color = Color::srgb(0.86, 0.93, 0.93);

pub struct ImageViewerPlugin;

impl Plugin for ImageViewerPlugin {
    fn build(&self, _app: &mut App) {}
}

fn image_size(data: &[u8]) -> Option<(f32, f32)> {
    if data.len() > 24 && &data[0..8] == b"\x89PNG\r\n\x1a\n" {
        let w = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
        let h = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
        return Some((w as f32, h as f32));
    }
    if data.len() > 4 && &data[0..2] == b"\xff\xd8" {
        let mut i = 2usize;
        while i + 9 < data.len() {
            if data[i] != 0xFF {
                i += 1;
                continue;
            }
            let marker = data[i + 1];
            if (0xC0..=0xCF).contains(&marker) && ![0xC4, 0xC8, 0xCC].contains(&marker) {
                let h = u16::from_be_bytes([data[i + 5], data[i + 6]]) as f32;
                let w = u16::from_be_bytes([data[i + 7], data[i + 8]]) as f32;
                return Some((w, h));
            }
            if !matches!(marker, 0xD8 | 0x01) && !(0xD0..=0xD7).contains(&marker) {
                let len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
                i += 2 + len;
            } else {
                i += 2;
            }
        }
    }
    None
}

pub fn spawn_image_viewer_direct(commands: &mut Commands, file_path: &str, fonts: &N3riFonts, asset_server: &AssetServer) {
    let file_name = file_path.rsplit('/').next().unwrap_or("图片");
    let title = format!("{} - 图片查看", file_name);
    let img_path = Path::new(file_path);
    let assets_dir = std::env::current_dir()
        .unwrap_or_default()
        .join("assets");
    let relative_path = img_path
        .strip_prefix(&assets_dir)
        .unwrap_or(img_path)
        .to_string_lossy()
        .to_string();
    let handle = asset_server.load(&relative_path);
    let aspect_ratio = crate::content::read_bytes(file_path)
        .as_deref()
        .and_then(image_size)
        .map(|(w, h)| w / h);
    let fonts_handle = fonts.default.clone();
    let title_clone = title.clone();

    commands.spawn((
        crate::window::AppWindow {
            title,
            app_id: "image_viewer".to_string(),
            z: 1,
        },
        crate::dock::AppVisible(true),
        GlobalZIndex(1),
        Node {
            position_type: PositionType::Absolute,
            width: Val::Px(560.0),
            height: Val::Px(552.0),
            flex_direction: FlexDirection::Column,
            top: Val::Px(112.0),
            left: Val::Px(640.0),
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
            ScrollableArea::default(),
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
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
                    min_height: Val::Px(0.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(8.0)),
                    ..default()
                },
            ))
            .with_children(|content| {
                content.spawn((
                    ImageNode {
                        image: handle,
                        ..default()
                    },
                    Node {
                        width: Val::Percent(100.0),
                        min_height: Val::Px(0.0),
                        aspect_ratio,
                        ..default()
                    },
                ));
            });
        });
    });
}
