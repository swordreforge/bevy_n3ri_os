use bevy::prelude::*;
use bevy::text::Justify;

use crate::apps::terminal::copy_text;
use crate::font::N3riFonts;
use crate::scroll::{ScrollableArea, ScrollContent};
use crate::window::spawn_window;

const STEAM_URL: &str = "https://store.steampowered.com/app/4996280/I_NORI/";
const BILI_URL: &str = "https://space.bilibili.com/326505494/upload/opus";
const QQ_GROUP_1: &str = "1041616195";
const QQ_GROUP_2: &str = "1107531061";

const CARD_BG: Color = Color::srgba(0.13, 0.18, 0.28, 0.9);
const ACCENT_NUM: Color = Color::srgb(0.45, 0.85, 1.0);
const TEXT_MAIN: Color = Color::srgb(0.86, 0.93, 0.93);
const TEXT_DIM: Color = Color::srgba(0.6, 0.7, 0.8, 0.75);
const BUTTON_BORDER: Color = Color::srgba(0.55, 0.65, 0.75, 0.5);

pub struct CreditsPlugin;

impl Plugin for CreditsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, credits_actions);
    }
}

#[derive(Component)]
enum CreditsAction {
    Steam,
    Bili,
    CopyQq(&'static str),
}

fn open_url(url: &str) {
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

fn credits_actions(
    mouse: Res<ButtonInput<MouseButton>>,
    query: Query<(&CreditsAction, &Interaction)>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for (action, interaction) in query.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match action {
            CreditsAction::Steam => open_url(STEAM_URL),
            CreditsAction::Bili => open_url(BILI_URL),
            CreditsAction::CopyQq(num) => copy_text(num),
        }
    }
}

pub fn spawn_credits_window(
    parent: &mut ChildSpawnerCommands,
    asset_server: &AssetServer,
    fonts: &N3riFonts,
) {
    let window_entity = spawn_window(parent, "致谢", "credits", 620.0, 860.0, fonts);

    parent
        .commands()
        .entity(window_entity)
        .with_children(|window| {
            window
                .spawn((Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(230.0),
                    overflow: Overflow::hidden(),
                    ..default()
                },))
                .with_children(|cover| {
                    cover.spawn((
                        ImageNode {
                            image: asset_server.load("nori/steam-capsule-CWHqT-l6.png"),
                            ..default()
                        },
                        Node {
                            width: Val::Percent(100.0),
                            height: Val::Percent(100.0),
                            ..default()
                        },
                    ));

                    for (bottom, height, alpha) in
                        [(0.0, 30.0, 0.55), (30.0, 20.0, 0.32), (50.0, 12.0, 0.16)]
                    {
                        cover.spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                bottom: Val::Px(bottom),
                                width: Val::Percent(100.0),
                                height: Val::Px(height),
                                ..default()
                            },
                            BackgroundColor(Color::srgba(0.08, 0.12, 0.2, alpha)),
                        ));
                    }
                });

            let area_e = window
                .spawn((
                    ScrollableArea::default(),
                    Node {
                        width: Val::Percent(100.0),
                        flex_grow: 1.0,
                        align_items: AlignItems::FlexStart,
                        overflow: Overflow::hidden(),
                        ..default()
                    },
                ))
                .id();
            window.commands().entity(area_e).with_children(|a| {
                crate::scroll::spawn_scrollbar(a, area_e);
            });

            window.commands().entity(area_e).with_children(|area| {
                area.spawn((
                    ScrollContent,
                    Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(Val::Px(20.0)),
                        row_gap: Val::Px(14.0),
                        ..default()
                    },
                ))
                .with_children(|content| {
                    content.spawn((
                        Text::new("谢谢你"),
                        TextFont {
                            font: FontSource::Handle(fonts.default.clone()),
                            font_size: FontSize::Px(28.0),
                            ..default()
                        },
                        TextColor(TEXT_MAIN),
                    ));

                    content.spawn((
                        Text::new("谢谢你通关《I_NORI》先导桌面解谜！我们是一个很小的团队，能陪你走到这里，真的很开心。欢迎来下面这些地方找我们！"),
                        TextFont {
                            font: FontSource::Handle(fonts.default.clone()),
                            font_size: FontSize::Px(14.0),
                            ..default()
                        },
                        TextColor(TEXT_MAIN),
                        TextLayout::default().with_justify(Justify::Left),
                    ));

                    card(
                        content,
                        fonts,
                        asset_server.load("nori/app-icons/credits/steam.png"),
                        "Steam",
                        None,
                        "先导篇的故事会在完整游戏里继续。喜欢的话，麻烦加个 Steam 愿望单啦！",
                        "加入愿望单！",
                        CreditsAction::Steam,
                    );
                    card(
                        content,
                        fonts,
                        asset_server.load("nori/app-icons/credits/QQ.png"),
                        "QQ群1 ",
                        Some(QQ_GROUP_1),
                        "来和开发组、其他玩过的朋友聊聊。",
                        "复制群号",
                        CreditsAction::CopyQq(QQ_GROUP_1),
                    );
                    card(
                        content,
                        fonts,
                        asset_server.load("nori/app-icons/credits/QQ.png"),
                        "QQ群2 ",
                        Some(QQ_GROUP_2),
                        "如果1群已满，请加2群。",
                        "复制群号",
                        CreditsAction::CopyQq(QQ_GROUP_2),
                    );
                    card(
                        content,
                        fonts,
                        asset_server.load("nori/app-icons/credits/bilibili.png"),
                        "哔哩哔哩",
                        None,
                        "关注我们的更新和开发日志。",
                        "打开主页",
                        CreditsAction::Bili,
                    );

                    content.spawn((
                        Text::new("© 2026 NORI LABS"),
                        TextFont {
                            font: FontSource::Handle(fonts.default.clone()),
                            font_size: FontSize::Px(11.0),
                            ..default()
                        },
                        TextColor(TEXT_DIM),
                        TextLayout::default().with_justify(Justify::Center),
                        Node {
                            width: Val::Percent(100.0),
                            margin: UiRect::top(Val::Px(4.0)),
                            ..default()
                        },
                    ));
                });
            });
        });
}

fn card(
    parent: &mut ChildSpawnerCommands,
    fonts: &N3riFonts,
    icon: Handle<Image>,
    title: &str,
    title_accent: Option<&str>,
    description: &str,
    button_label: &str,
    action: CreditsAction,
) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(14.0),
                padding: UiRect::all(Val::Px(16.0)),
                border_radius: BorderRadius::all(Val::Px(12.0)),
                ..default()
            },
            BackgroundColor(CARD_BG),
        ))
        .with_children(|card| {
            card.spawn((
                ImageNode {
                    image: icon,
                    ..default()
                },
                Node {
                    width: Val::Px(40.0),
                    height: Val::Px(40.0),
                    border_radius: BorderRadius::all(Val::Px(8.0)),
                    ..default()
                },
            ));

            card.spawn(Node {
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                ..default()
            })
            .with_children(|middle| {
                let mut title_node = middle.spawn((
                    Text::new(title),
                    TextFont {
                        font: FontSource::Handle(fonts.default.clone()),
                        font_size: FontSize::Px(17.0),
                        ..default()
                    },
                    TextColor(TEXT_MAIN),
                ));
                if let Some(accent) = title_accent {
                    title_node.with_children(|t| {
                        t.spawn((
                            TextSpan::new(accent),
                            TextFont {
                                font: FontSource::Handle(fonts.default.clone()),
                                font_size: FontSize::Px(17.0),
                                ..default()
                            },
                            TextColor(ACCENT_NUM),
                        ));
                    });
                }

                middle.spawn((
                    Text::new(description),
                    TextFont {
                        font: FontSource::Handle(fonts.default.clone()),
                        font_size: FontSize::Px(13.0),
                        ..default()
                    },
                    TextColor(TEXT_DIM),
                ));
            });

            card
                .spawn((
                    Button,
                    action,
                    Node {
                        padding: UiRect {
                            left: Val::Px(14.0),
                            right: Val::Px(14.0),
                            top: Val::Px(7.0),
                            bottom: Val::Px(7.0),
                        },
                        border_radius: BorderRadius::all(Val::Px(8.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BorderColor::all(BUTTON_BORDER),
                ))
                .with_children(|button| {
                    button.spawn((
                        Text::new(button_label),
                        TextFont {
                            font: FontSource::Handle(fonts.default.clone()),
                            font_size: FontSize::Px(13.0),
                            ..default()
                        },
                        TextColor(TEXT_MAIN),
                    ));
                });
        });
}
