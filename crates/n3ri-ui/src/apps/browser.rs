//! 浏览器 —— 占位窗口：告知用户 Web 视图未实现（见 AGENTS.md Non-Goals）。

use bevy::prelude::*;
use bevy::text::{FontSource, FontSize};

use crate::font::N3riFonts;
use crate::window::spawn_window;

const WIN_W: f32 = 480.0;
const WIN_H: f32 = 320.0;

const CK_BROWN: Color = Color::srgb(0.42, 0.28, 0.19);
const CK_DIM: Color = Color::srgba(0.42, 0.28, 0.19, 0.65);
const CK_AMBER: Color = Color::srgb(0.84, 0.55, 0.32);

pub fn spawn_browser(parent: &mut ChildSpawnerCommands, fonts: &N3riFonts) {
    let window_e = spawn_window(parent, "浏览器", "browser", WIN_W, WIN_H, fonts);
    parent.commands().entity(window_e).with_children(|win| {
        win.spawn((Node {
            width: Val::Percent(100.0),
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            row_gap: Val::Px(10.0),
            padding: UiRect::horizontal(Val::Px(24.0)),
            ..default()
        }, BackgroundColor(Color::srgba(1.0, 0.97, 0.94, 0.96))))
        .with_children(|page| {
            page.spawn((
                Text::new("浏览器暂未实现"),
                TextFont { font: ck_font(fonts), font_size: FontSize::Px(22.0), ..default() },
                TextColor(CK_BROWN),
            ));
            page.spawn((
                Text::new("n3ri_os 未内嵌 Web 视图——复刻官方内嵌浏览器需要 bevy_cef / bevy_wry，成本过高，暂不引入。"),
                TextFont { font: ck_font(fonts), font_size: FontSize::Px(13.0), ..default() },
                TextColor(CK_DIM),
            ));
            page.spawn((
                Text::new("先去玩玩蛋糕对决、森林寻宝和国际象棋吧。"),
                TextFont { font: ck_font(fonts), font_size: FontSize::Px(13.0), ..default() },
                TextColor(CK_AMBER),
            ));
        });
    });
}

fn ck_font(fonts: &N3riFonts) -> FontSource {
    FontSource::Handle(fonts.default.clone())
}
