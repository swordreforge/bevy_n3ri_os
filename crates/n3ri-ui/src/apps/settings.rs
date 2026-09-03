use bevy::prelude::*;
use n3ri_core::config::UserSettings;
use n3ri_llm::{LlmClient, LlmConfig};
use crate::apps::terminal::paste_text;
use bevy::input::keyboard::{Key, KeyCode, KeyboardInput};
use bevy::window::Ime;
use crate::input_focus::{TextInputFocus, TextInputOwner};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use crate::cursor::CursorPosition;
use crate::font::N3riFonts;
use crate::scroll::{ScrollableArea, ScrollContent};
use crate::window::spawn_window;

const ACCENT: Color = Color::srgb(0.2, 0.5, 1.0);
const TEXT_MAIN: Color = Color::srgb(0.86, 0.93, 0.93);
const TEXT_DIM: Color = Color::srgba(0.6, 0.7, 0.8, 0.75);
const NAV_HIGHLIGHT: Color = Color::srgba(0.2, 0.35, 0.55, 0.6);
const TRACK_BG: Color = Color::srgba(0.25, 0.3, 0.36, 0.8);
const TOGGLE_OFF: Color = Color::srgba(0.3, 0.35, 0.4, 0.9);
const DIVIDER: Color = Color::srgba(0.4, 0.5, 0.6, 0.25);
const OK_GREEN: Color = Color::srgb(0.3, 0.85, 0.5);
const BAD_RED: Color = Color::srgb(0.9, 0.35, 0.35);
const BUTTON_BORDER_COLOR: Color = Color::srgba(0.55, 0.65, 0.75, 0.5);

const QUALITY_OPTIONS: [&str; 3] = ["极限性能", "平衡", "省电"];
const TAB_LABELS: [(&str, &str); 6] = [
    ("sound", "声音"),
    ("display", "显示效果"),
    ("network", "网络"),
    ("touch", "触控"),
    ("system", "系统"),
    ("model", "模型"),
];

#[derive(Clone, Copy, PartialEq)]
enum SettingsTab {
    Sound,
    Display,
    Network,
    Touch,
    System,
    Model,
}

const TAB_ORDER: [SettingsTab; 6] = [
    SettingsTab::Sound,
    SettingsTab::Display,
    SettingsTab::Network,
    SettingsTab::Touch,
    SettingsTab::System,
    SettingsTab::Model,
];

#[derive(Clone)]
enum PingState {
    Idle,
    Running,
    Done(bool, Option<f32>),
}

#[derive(Resource)]
struct SettingsState {
    tab: SettingsTab,
    net_ok: Option<bool>,
    ping_ms: Option<f32>,
    ping: Arc<Mutex<PingState>>,
    cpu: f32,
    mem: f32,
    gpu: Option<f32>,
    prev_cpu: (u64, u64),
    prev_gpu_rc6: Option<(u64, Instant)>,
    llm_form: [String; 3],
    llm_focus: Option<usize>,
    llm_test: Arc<Mutex<LlmTest>>,
}


impl Default for SettingsState {
    fn default() -> Self {
        Self {
            tab: SettingsTab::Sound,
            net_ok: None,
            ping_ms: None,
            ping: Arc::new(Mutex::new(PingState::Idle)),
            cpu: 0.0,
            mem: 0.0,
            gpu: None,
            prev_cpu: (0, 0),
            prev_gpu_rc6: None,
            llm_form: {
                let cfg = n3ri_llm::load_config();
                [cfg.base_url, cfg.model, cfg.api_key]
            },
            llm_focus: None,
            llm_test: Arc::new(Mutex::new(LlmTest::Idle)),
        }
    }
}

#[derive(Resource, Default)]
struct SettingsEntities {
    pages: [Option<Entity>; 6],
    nav_bg: [Option<Entity>; 6],
    nav_text: [Option<Entity>; 6],
    fill: [Option<Entity>; 4],
    knob: [Option<Entity>; 4],
    pct: [Option<Entity>; 4],
    toggle_bg: [Option<Entity>; 4],
    toggle_knob: [Option<Entity>; 4],
    quality_label: Option<Entity>,
    net_status: Option<Entity>,
    net_latency: Option<Entity>,
    llm_text: [Option<Entity>; 3],
    llm_status: Option<Entity>,
    bar_fill: [Option<Entity>; 3],
    bar_pct: [Option<Entity>; 3],
    wallpaper_toggle_bg: Option<Entity>,
    wallpaper_toggle_knob: Option<Entity>,
    natural_scroll_toggle_bg: Option<Entity>,
    natural_scroll_toggle_knob: Option<Entity>,
}

#[derive(Component)]
struct SettingsNavItem(SettingsTab);

#[derive(Component)]
struct SettingsPage;

#[derive(Component)]
struct VolumeSlider(u8);

#[derive(Component)]
struct SettingsToggle(u8);

#[derive(Component)]
struct WallpaperToggle;

#[derive(Component)]
struct NaturalScrollToggle;

#[derive(Component)]
struct QualityButton;

#[derive(Component)]
struct PingButton;

#[derive(Component)]
struct LlmInput(usize);

#[derive(Component)]
struct LlmInputText;

#[derive(Component)]
struct LlmSaveBtn;

#[derive(Component)]
struct LlmTestBtn;

#[derive(Component)]
struct LlmStatusText;

#[derive(Clone)]
enum LlmTest {
    Idle,
    Running,
    Done(Result<String, String>),
}

#[derive(Component)]
struct BarFill;

pub struct SettingsPlugin;

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SettingsState>()
            .init_resource::<SettingsEntities>()
            .add_systems(
                Update,
                (
                    settings_nav,
                    settings_toggle_click,
                    wallpaper_toggle_click,
                    natural_scroll_toggle_click,
                    settings_slider_drag,
                    settings_quality_click,
                    settings_ping_click,
                    settings_poll,
                    settings_sync_ui,
                    settings_llm_click.after(crate::window::WindowFocusSet),
                    settings_llm_input.after(settings_llm_click),
                    settings_llm_ime.after(settings_llm_click),
                    settings_llm_sync,
                ),
            );
    }
}

pub fn spawn_settings_window(parent: &mut ChildSpawnerCommands, fonts: &N3riFonts) {
    let window_entity = spawn_window(parent, "设置", "settings", 820.0, 620.0, fonts);

    let mut ents = SettingsEntities::default();
    let mut state = SettingsState::default();
    start_ping(&state.ping);

    parent
        .commands()
        .entity(window_entity)
        .with_children(|window| {
            window
                .spawn(Node {
                    width: Val::Percent(100.0),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Row,
                    ..default()
                })
                .with_children(|body| {
                    body.spawn(Node {
                        width: Val::Px(210.0),
                        height: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(Val::Px(12.0)),
                        row_gap: Val::Px(4.0),
                        border: UiRect::right(Val::Px(1.0)),
                        ..default()
                    })
                    .with_children(|sidebar| {
                        for (i, (_id, label)) in TAB_LABELS.into_iter().enumerate() {
                            let (bg_e, text_e) =
                                spawn_nav_item(sidebar, fonts, TAB_ORDER[i], label);
                            ents.nav_bg[i] = Some(bg_e);
                            ents.nav_text[i] = Some(text_e);
                        }
                    });

                    let area_e = body
                        .spawn((
                            ScrollableArea::default(),
                            Node {
                                flex_grow: 1.0,
                                align_items: AlignItems::FlexStart,
                                overflow: Overflow::hidden(),
                                ..default()
                            },
                        ))
                        .id();
                    body.commands().entity(area_e).with_children(|a| {
                        crate::scroll::spawn_scrollbar(a, area_e);
                    });

                    body.commands().entity(area_e).with_children(|area| {
                        area.spawn((
                            ScrollContent,
                            Node {
                                width: Val::Percent(100.0),
                                flex_direction: FlexDirection::Column,
                                padding: UiRect::all(Val::Px(24.0)),
                                row_gap: Val::Px(14.0),
                                ..default()
                            },
                        ))
                        .with_children(|content| {
                        let settings = UserSettings::load();
                        ents.pages[0] =
                            Some(spawn_sound_page(content, fonts, &settings, &mut ents));
                        ents.pages[1] =
                            Some(spawn_display_page(content, fonts, &settings, &mut ents));
                        ents.pages[2] = Some(spawn_network_page(content, fonts, &mut ents));
                        ents.pages[3] =
                            Some(spawn_touch_page(content, fonts, &settings, &mut ents));
                        ents.pages[4] = Some(spawn_system_page(content, fonts, &mut ents));
                        ents.pages[5] =
                            Some(spawn_model_page(content, fonts, &mut ents, &mut state));
                        });
                    });
                });
        });

    parent.commands().insert_resource(ents);
    parent.commands().insert_resource(state);
}

fn spawn_nav_item(
    parent: &mut ChildSpawnerCommands,
    fonts: &N3riFonts,
    tab: SettingsTab,
    label: &str,
) -> (Entity, Entity) {
    let bg = parent
        .spawn((
            Button,
            SettingsNavItem(tab),
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(40.0),
                align_items: AlignItems::Center,
                padding: UiRect::left(Val::Px(14.0)),
                border_radius: BorderRadius::all(Val::Px(8.0)),
                ..default()
            },
            BackgroundColor(Color::NONE),
        ))
        .id();

    let mut text_e = Entity::PLACEHOLDER;
    parent.commands().entity(bg).with_children(|item| {
        text_e = item
            .spawn((
                Text::new(label),
                TextFont {
                    font: FontSource::Handle(fonts.default.clone()),
                    font_size: FontSize::Px(15.0),
                    ..default()
                },
                TextColor(TEXT_DIM),
            ))
            .id();
    });

    (bg, text_e)
}

struct VolumeEnts {
    fill: Entity,
    knob: Entity,
    pct: Entity,
    toggle_bg: Option<Entity>,
    toggle_knob: Option<Entity>,
}

fn spawn_sound_page(
    parent: &mut ChildSpawnerCommands,
    fonts: &N3riFonts,
    settings: &UserSettings,
    ents: &mut SettingsEntities,
) -> Entity {
    let page_e = parent
        .spawn((
            SettingsPage,
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(16.0),
                ..default()
            },
        ))
        .id();

    let rows: [(u8, &str, bool); 4] = [
        (0, "主音量", false),
        (1, "音乐", true),
        (2, "音效", true),
        (3, "语音", true),
    ];

    parent.commands().entity(page_e).with_children(|page| {
        page_header(page, fonts, "声音", "调整音量");

        for (ch, label, with_toggle) in rows {
            let vol = spawn_volume_row(page, fonts, settings, ch, label, with_toggle);
            ents.fill[ch as usize] = Some(vol.fill);
            ents.knob[ch as usize] = Some(vol.knob);
            ents.pct[ch as usize] = Some(vol.pct);
            ents.toggle_bg[ch as usize] = vol.toggle_bg;
            ents.toggle_knob[ch as usize] = vol.toggle_knob;
        }

        divider(page);
    });

    page_e
}

fn spawn_volume_row(
    parent: &mut ChildSpawnerCommands,
    fonts: &N3riFonts,
    settings: &UserSettings,
    ch: u8,
    label: &str,
    with_toggle: bool,
) -> VolumeEnts {
    let pct = settings.volumes[ch as usize];
    let mut out = VolumeEnts {
        fill: Entity::PLACEHOLDER,
        knob: Entity::PLACEHOLDER,
        pct: Entity::PLACEHOLDER,
        toggle_bg: None,
        toggle_knob: None,
    };

    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: Val::Px(12.0),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Text::new(label),
                TextFont {
                    font: FontSource::Handle(fonts.default.clone()),
                    font_size: FontSize::Px(14.0),
                    ..default()
                },
                TextColor(TEXT_MAIN),
                Node {
                    width: Val::Px(70.0),
                    ..default()
                },
            ));

            out.pct = row
                .spawn((
                    Text::new(format!("{pct}%")),
                    TextFont {
                        font: FontSource::Handle(fonts.default.clone()),
                        font_size: FontSize::Px(13.0),
                        ..default()
                    },
                    TextColor(TEXT_DIM),
                    Node {
                        width: Val::Px(46.0),
                        justify_content: JustifyContent::FlexEnd,
                        ..default()
                    },
                ))
                .id();

            row.spawn((
                Button,
                VolumeSlider(ch),
                Node {
                    flex_grow: 1.0,
                    height: Val::Px(6.0),
                    border_radius: BorderRadius::all(Val::Px(3.0)),
                    ..default()
                },
                BackgroundColor(TRACK_BG),
            ))
            .with_children(|track| {
                out.fill = track
                    .spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(0.0),
                            top: Val::Px(0.0),
                            width: Val::Percent(pct as f32),
                            height: Val::Percent(100.0),
                            border_radius: BorderRadius::all(Val::Px(3.0)),
                            ..default()
                        },
                        BackgroundColor(ACCENT),
                    ))
                    .id();
                out.knob = track
                    .spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Percent(pct as f32),
                            top: Val::Px(-4.0),
                            width: Val::Px(14.0),
                            height: Val::Px(14.0),
                            border_radius: BorderRadius::all(Val::Px(7.0)),
                            margin: UiRect::left(Val::Px(-7.0)),
                            ..default()
                        },
                        BackgroundColor(Color::WHITE),
                    ))
                    .id();
            });

            if with_toggle {
                let (bg_e, knob_e) = spawn_toggle(row, ch, settings.toggles[ch as usize]);
                out.toggle_bg = Some(bg_e);
                out.toggle_knob = Some(knob_e);
            }
        });

    out
}

fn spawn_toggle(parent: &mut ChildSpawnerCommands, ch: u8, on: bool) -> (Entity, Entity) {
    let bg = parent
        .spawn((
            Button,
            SettingsToggle(ch),
            Node {
                width: Val::Px(40.0),
                height: Val::Px(20.0),
                border_radius: BorderRadius::all(Val::Px(10.0)),
                ..default()
            },
            BackgroundColor(if on { ACCENT } else { TOGGLE_OFF }),
        ))
        .id();

    let mut knob_e = Entity::PLACEHOLDER;
    parent.commands().entity(bg).with_children(|t| {
        knob_e = t
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(if on { 22.0 } else { 2.0 }),
                    top: Val::Px(2.0),
                    width: Val::Px(16.0),
                    height: Val::Px(16.0),
                    border_radius: BorderRadius::all(Val::Px(8.0)),
                    ..default()
                },
                BackgroundColor(Color::WHITE),
            ))
            .id();
    });

    (bg, knob_e)
}

fn spawn_wallpaper_toggle(parent: &mut ChildSpawnerCommands, on: bool) -> (Entity, Entity) {
    let bg = parent
        .spawn((
            Button,
            WallpaperToggle,
            Node {
                width: Val::Px(40.0),
                height: Val::Px(20.0),
                border_radius: BorderRadius::all(Val::Px(10.0)),
                ..default()
            },
            BackgroundColor(if on { ACCENT } else { TOGGLE_OFF }),
        ))
        .id();

    let mut knob_e = Entity::PLACEHOLDER;
    parent.commands().entity(bg).with_children(|t| {
        knob_e = t
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(if on { 22.0 } else { 2.0 }),
                    top: Val::Px(2.0),
                    width: Val::Px(16.0),
                    height: Val::Px(16.0),
                    border_radius: BorderRadius::all(Val::Px(8.0)),
                    ..default()
                },
                BackgroundColor(Color::WHITE),
            ))
            .id();
    });

    (bg, knob_e)
}

fn spawn_display_page(
    parent: &mut ChildSpawnerCommands,
    fonts: &N3riFonts,
    settings: &UserSettings,
    ents: &mut SettingsEntities,
) -> Entity {
    let page_e = parent
        .spawn((
            SettingsPage,
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(16.0),
                ..default()
            },
        ))
        .id();

    let quality = QUALITY_OPTIONS[settings.quality_idx];

    parent.commands().entity(page_e).with_children(|page| {
        page_header(page, fonts, "显示效果", "画质与性能权衡");

        page.spawn((
            Button,
            QualityButton,
            Node {
                width: Val::Px(280.0),
                height: Val::Px(44.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                padding: UiRect::all(Val::Px(12.0)),
                border_radius: BorderRadius::all(Val::Px(8.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(BUTTON_BORDER_COLOR),
        ))
        .with_children(|dd| {
            ents.quality_label = Some(
                dd.spawn((
                    Text::new(quality),
                    TextFont {
                        font: FontSource::Handle(fonts.default.clone()),
                        font_size: FontSize::Px(14.0),
                        ..default()
                    },
                    TextColor(TEXT_MAIN),
                ))
                .id(),
            );
            dd.spawn((
                Text::new("▼"),
                TextFont {
                    font: FontSource::Handle(fonts.default.clone()),
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(TEXT_DIM),
            ));
        });

        page.spawn(Node {
            width: Val::Px(280.0),
            height: Val::Px(44.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            padding: UiRect::all(Val::Px(12.0)),
            border_radius: BorderRadius::all(Val::Px(8.0)),
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Text::new("壁纸模式"),
                TextFont {
                    font: FontSource::Handle(fonts.default.clone()),
                    font_size: FontSize::Px(14.0),
                    ..default()
                },
                TextColor(TEXT_MAIN),
            ));
            row.spawn(Node {
                width: Val::Px(60.0),
                height: Val::Px(1.0),
                ..default()
            });
            let (bg, knob) = spawn_wallpaper_toggle(row, settings.wallpaper_enabled);
            ents.wallpaper_toggle_bg = Some(bg);
            ents.wallpaper_toggle_knob = Some(knob);
        });
    });

    page_e
}

fn spawn_natural_scroll_toggle(parent: &mut ChildSpawnerCommands, on: bool) -> (Entity, Entity) {
    let bg = parent
        .spawn((
            Button,
            NaturalScrollToggle,
            Node {
                width: Val::Px(40.0),
                height: Val::Px(20.0),
                border_radius: BorderRadius::all(Val::Px(10.0)),
                ..default()
            },
            BackgroundColor(if on { ACCENT } else { TOGGLE_OFF }),
        ))
        .id();

    let mut knob_e = Entity::PLACEHOLDER;
    parent.commands().entity(bg).with_children(|t| {
        knob_e = t
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(if on { 22.0 } else { 2.0 }),
                    top: Val::Px(2.0),
                    width: Val::Px(16.0),
                    height: Val::Px(16.0),
                    border_radius: BorderRadius::all(Val::Px(8.0)),
                    ..default()
                },
                BackgroundColor(Color::WHITE),
            ))
            .id();
    });

    (bg, knob_e)
}

fn spawn_touch_page(
    parent: &mut ChildSpawnerCommands,
    fonts: &N3riFonts,
    settings: &UserSettings,
    ents: &mut SettingsEntities,
) -> Entity {
    let page_e = parent
        .spawn((
            SettingsPage,
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(16.0),
                ..default()
            },
        ))
        .id();

    parent.commands().entity(page_e).with_children(|page| {
        page_header(page, fonts, "触控", "触摸板滚动方向");

        page.spawn(Node {
            width: Val::Px(280.0),
            height: Val::Px(44.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            padding: UiRect::all(Val::Px(12.0)),
            border_radius: BorderRadius::all(Val::Px(8.0)),
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Text::new("自然滚动"),
                TextFont {
                    font: FontSource::Handle(fonts.default.clone()),
                    font_size: FontSize::Px(14.0),
                    ..default()
                },
                TextColor(TEXT_MAIN),
            ));
            row.spawn(Node {
                width: Val::Px(60.0),
                height: Val::Px(1.0),
                ..default()
            });
            let (bg, knob) = spawn_natural_scroll_toggle(row, settings.natural_scroll);
            ents.natural_scroll_toggle_bg = Some(bg);
            ents.natural_scroll_toggle_knob = Some(knob);
        });
    });

    page_e
}

fn spawn_network_page(
    parent: &mut ChildSpawnerCommands,
    fonts: &N3riFonts,
    ents: &mut SettingsEntities,
) -> Entity {
    let page_e = parent
        .spawn((
            SettingsPage,
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(18.0),
                ..default()
            },
        ))
        .id();

    parent.commands().entity(page_e).with_children(|page| {
        page_header(page, fonts, "网络", "连接状态与线路质量");

        ents.net_status = Some(spawn_net_row(page, fonts, "状态", "检测中…", TEXT_DIM));
        ents.net_latency = Some(spawn_net_row(page, fonts, "延迟", "--", TEXT_DIM));

        page.spawn((
            Button,
            PingButton,
            Node {
                width: Val::Px(120.0),
                height: Val::Px(36.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(Val::Px(8.0)),
                border: UiRect::all(Val::Px(1.0)),
                margin: UiRect::top(Val::Px(6.0)),
                ..default()
            },
            BorderColor::all(BUTTON_BORDER_COLOR),
        ))
        .with_children(|b| {
            b.spawn((
                Text::new("检测线路"),
                TextFont {
                    font: FontSource::Handle(fonts.default.clone()),
                    font_size: FontSize::Px(13.0),
                    ..default()
                },
                TextColor(TEXT_MAIN),
            ));
        });
    });

    page_e
}

fn spawn_net_row(
    parent: &mut ChildSpawnerCommands,
    fonts: &N3riFonts,
    label: &str,
    value: &str,
    value_color: Color,
) -> Entity {
    let row = parent
        .spawn(Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            ..default()
        })
        .id();

    let mut value_e = Entity::PLACEHOLDER;
    parent.commands().entity(row).with_children(|r| {
        r.spawn((
            Text::new(label),
            TextFont {
                font: FontSource::Handle(fonts.default.clone()),
                font_size: FontSize::Px(14.0),
                ..default()
            },
            TextColor(TEXT_MAIN),
        ));
        value_e = r
            .spawn((
                Text::new(value),
                TextFont {
                    font: FontSource::Handle(fonts.default.clone()),
                    font_size: FontSize::Px(14.0),
                    ..default()
                },
                TextColor(value_color),
            ))
            .id();
    });

    value_e
}

fn spawn_system_page(
    parent: &mut ChildSpawnerCommands,
    fonts: &N3riFonts,
    ents: &mut SettingsEntities,
) -> Entity {
    let page_e = parent
        .spawn((
            SettingsPage,
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(18.0),
                ..default()
            },
        ))
        .id();

    parent.commands().entity(page_e).with_children(|page| {
        page_header(page, fonts, "系统", "实时资源占用");

        for (i, label) in ["CPU", "内存", "GPU"].into_iter().enumerate() {
            page.spawn(Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                ..default()
            })
            .with_children(|block| {
                block
                    .spawn(Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Row,
                        justify_content: JustifyContent::SpaceBetween,
                        ..default()
                    })
                    .with_children(|line| {
                        line.spawn((
                            Text::new(label),
                            TextFont {
                                font: FontSource::Handle(fonts.default.clone()),
                                font_size: FontSize::Px(14.0),
                                ..default()
                            },
                            TextColor(TEXT_MAIN),
                        ));
                        ents.bar_pct[i] = Some(
                            line.spawn((
                                Text::new("--"),
                                TextFont {
                                    font: FontSource::Handle(fonts.default.clone()),
                                    font_size: FontSize::Px(13.0),
                                    ..default()
                                },
                                TextColor(TEXT_DIM),
                            ))
                            .id(),
                        );
                    });

                block
                    .spawn(Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(10.0),
                        border_radius: BorderRadius::all(Val::Px(5.0)),
                        ..default()
                    })
                    .with_children(|track| {
                        ents.bar_fill[i] = Some(
                            track
                                .spawn((
                                    BarFill,
                                    Node {
                                        position_type: PositionType::Absolute,
                                        left: Val::Px(0.0),
                                        top: Val::Px(0.0),
                                        width: Val::Percent(0.0),
                                        height: Val::Percent(100.0),
                                        border_radius: BorderRadius::all(Val::Px(5.0)),
                                        ..default()
                                    },
                                    BackgroundColor(ACCENT),
                                ))
                                .id(),
                        );
                    });
            });
        }
    });

    page_e
}

fn page_header(parent: &mut ChildSpawnerCommands, fonts: &N3riFonts, title: &str, subtitle: &str) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(4.0),
            ..default()
        })
        .with_children(|h| {
            h.spawn((
                Text::new(title),
                TextFont {
                    font: FontSource::Handle(fonts.default.clone()),
                    font_size: FontSize::Px(20.0),
                    ..default()
                },
                TextColor(TEXT_MAIN),
            ));
            h.spawn((
                Text::new(subtitle),
                TextFont {
                    font: FontSource::Handle(fonts.default.clone()),
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(TEXT_DIM),
            ));
        });
}

fn divider(parent: &mut ChildSpawnerCommands) {
    parent.spawn((
        Node {
            width: Val::Percent(100.0),
            height: Val::Px(1.0),
            ..default()
        },
        BackgroundColor(DIVIDER),
    ));
}

fn start_ping(ping: &Arc<Mutex<PingState>>) {
    {
        let mut guard = ping.lock().unwrap();
        if matches!(&*guard, PingState::Running) {
            return;
        }
        *guard = PingState::Running;
    }
    let shared = ping.clone();
    thread::spawn(move || {
        let output = std::process::Command::new("ping")
            .args(["-c", "1", "-W", "3", "cn.bing.com"])
            .env("LC_ALL", "C")
            .output();
        match output {
            Ok(out) => {
                let text = String::from_utf8_lossy(&out.stdout);
                let ok = out.status.success();
                let ms = parse_ping_ms(&text);
                *shared.lock().unwrap() = PingState::Done(ok, ms);
            }
            Err(_) => {
                *shared.lock().unwrap() = PingState::Done(false, None);
            }
        }
    });
}

fn parse_ping_ms(text: &str) -> Option<f32> {
    let line = text.lines().find(|l| l.contains("time="))?;
    let idx = line.find("time=")? + 5;
    let rest = &line[idx..];
    let end = rest.find(' ').unwrap_or(rest.len());
    rest[..end].parse().ok()
}

fn read_cpu_sample() -> Option<(u64, u64)> {
    let stat = std::fs::read_to_string("/proc/stat").ok()?;
    let cpu_line = stat.lines().next()?;
    let fields: Vec<u64> = cpu_line
        .split_whitespace()
        .skip(1)
        .filter_map(|v| v.parse().ok())
        .collect();
    let idle = fields.get(3)? + fields.get(4).unwrap_or(&0);
    let total: u64 = fields.iter().sum();
    Some((total, idle))
}

fn read_mem_percent() -> Option<f32> {
    let info = std::fs::read_to_string("/proc/meminfo").ok()?;
    let get = |key: &str| -> Option<u64> {
        info.lines()
            .find(|l| l.starts_with(key))?
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()
    };
    let total = get("MemTotal:")?;
    let available = get("MemAvailable:")?;
    Some(((total - available) as f32 / total as f32) * 100.0)
}

/// amdgpu 专有接口 gpu_busy_percent（0-100，直接可用；Intel i915 无此文件）。
fn read_amd_gpu_busy() -> Option<f32> {
    for entry in std::fs::read_dir("/sys/class/drm").ok()? {
        let path = entry.ok()?.path().join("device/gpu_busy_percent");
        if let Ok(text) = std::fs::read_to_string(&path) {
            return text.trim().parse().ok();
        }
    }
    None
}

/// Intel i915 替代方案：RC6 省电驻留累计毫秒（主渲染 GT=gt0），单调递增，需差分。
fn read_intel_rc6_ms() -> Option<u64> {
    for entry in std::fs::read_dir("/sys/class/drm").ok()? {
        let gt_path = entry.ok()?.path().join("gt/gt0/rc6_residency_ms");
        if let Ok(text) = std::fs::read_to_string(&gt_path) {
            return text.trim().parse().ok();
        }
    }
    None
}

fn read_gpu_percent(prev: &mut Option<(u64, Instant)>) -> Option<f32> {
    if let Some(busy) = read_amd_gpu_busy() {
        return Some(busy);
    }
    // busy% = 100 * (1 - Δrc6_ms / Δwall_ms)，首次采样仅建基线返回 None
    let rc6 = read_intel_rc6_ms()?;
    let now = Instant::now();
    let pct = match *prev {
        Some((prev_rc6, prev_t)) => {
            let d_rc6 = rc6.saturating_sub(prev_rc6);
            let d_wall_ms = now.duration_since(prev_t).as_secs_f64() * 1000.0;
            if d_wall_ms > 0.0 {
                Some((100.0 * (1.0 - d_rc6 as f64 / d_wall_ms)).clamp(0.0, 100.0) as f32)
            } else {
                None
            }
        }
        None => None,
    };
    *prev = Some((rc6, now));
    pct
}

fn settings_nav(
    mouse: Res<ButtonInput<MouseButton>>,
    nav_query: Query<(&SettingsNavItem, &Interaction)>,
    mut state: ResMut<SettingsState>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for (nav, interaction) in nav_query.iter() {
        if *interaction == Interaction::Pressed {
            state.tab = nav.0;
        }
    }
}

fn settings_toggle_click(
    mouse: Res<ButtonInput<MouseButton>>,
    toggle_query: Query<(&SettingsToggle, &Interaction)>,
    mut settings: ResMut<UserSettings>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for (toggle, interaction) in toggle_query.iter() {
        if *interaction == Interaction::Pressed {
            let ch = toggle.0 as usize;
            settings.toggles[ch] = !settings.toggles[ch];
            settings.save();
        }
    }
}

/// 壁纸模式开关：翻转后保存，并自我重启进入目标模式（新进程先起，旧进程随即退出）。
fn wallpaper_toggle_click(
    mouse: Res<ButtonInput<MouseButton>>,
    toggle_query: Query<&Interaction, With<WallpaperToggle>>,
    mut settings: ResMut<UserSettings>,
    mut exit: MessageWriter<AppExit>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for interaction in toggle_query.iter() {
        if *interaction == Interaction::Pressed {
            settings.wallpaper_enabled = !settings.wallpaper_enabled;
            settings.save();
            relaunch_into_mode(settings.wallpaper_enabled);
            exit.write(AppExit::Success);
        }
    }
}

fn relaunch_into_mode(wallpaper: bool) {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let mut command = std::process::Command::new(exe);
    if wallpaper {
        command.arg("--wallpaper");
    }
    let _ = command.spawn();
}

/// 自然滚动开关：翻转后保存，立即生效（壁纸模式注入路径每帧读取该值）。
fn natural_scroll_toggle_click(
    mouse: Res<ButtonInput<MouseButton>>,
    toggle_query: Query<&Interaction, With<NaturalScrollToggle>>,
    mut settings: ResMut<UserSettings>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for interaction in toggle_query.iter() {
        if *interaction == Interaction::Pressed {
            settings.natural_scroll = !settings.natural_scroll;
            settings.save();
        }
    }
}

fn settings_quality_click(
    mouse: Res<ButtonInput<MouseButton>>,
    quality_query: Query<&Interaction, With<QualityButton>>,
    mut settings: ResMut<UserSettings>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for interaction in quality_query.iter() {
        if *interaction == Interaction::Pressed {
            settings.quality_idx = (settings.quality_idx + 1) % QUALITY_OPTIONS.len();
            settings.save();
        }
    }
}

fn settings_ping_click(
    mouse: Res<ButtonInput<MouseButton>>,
    ping_query: Query<&Interaction, With<PingButton>>,
    state: Res<SettingsState>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for interaction in ping_query.iter() {
        if *interaction == Interaction::Pressed {
            start_ping(&state.ping);
        }
    }
}

fn settings_slider_drag(
    mouse: Res<ButtonInput<MouseButton>>,
    cursor: Res<CursorPosition>,
    slider_query: Query<(
        &VolumeSlider,
        &Interaction,
        &ComputedNode,
        &UiGlobalTransform,
    )>,
    mut settings: ResMut<UserSettings>,
    mut last_save: Local<f32>,
    time: Res<Time>,
) {
    if !mouse.pressed(MouseButton::Left) {
        return;
    }
    if !cursor.active {
        return;
    }
    let cursor = cursor.physical;
    let mut changed = false;
    for (slider, interaction, node, transform) in slider_query.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Some(local) = transform.try_inverse().map(|t| t.transform_point2(cursor)) else {
            continue;
        };
        let half = node.size() * 0.5;
        let x_from_left = local.x + half.x;
        let pct = ((x_from_left / node.size().x) * 100.0).round().clamp(0.0, 100.0);
        let new_val = pct as u8;
        if settings.volumes[slider.0 as usize] != new_val {
            settings.volumes[slider.0 as usize] = new_val;
            changed = true;
        }
    }
    if changed {
        *last_save += time.delta_secs();
        if *last_save > 0.5 {
            settings.save();
            *last_save = 0.0;
        }
    }
}

fn settings_poll(
    time: Res<Time>,
    mut local: Local<f32>,
    mut state: ResMut<SettingsState>,
) {
    *local += time.delta_secs();
    if *local < 0.5 {
        return;
    }
    *local = 0.0;

    if let Some((total, idle)) = read_cpu_sample() {
        if state.prev_cpu.0 > 0 {
            let d_total = total.saturating_sub(state.prev_cpu.0);
            let d_idle = idle.saturating_sub(state.prev_cpu.1);
            if d_total > 0 {
                state.cpu = ((d_total - d_idle) as f32 / d_total as f32) * 100.0;
            }
        }
        state.prev_cpu = (total, idle);
    }

    if let Some(mem) = read_mem_percent() {
        state.mem = mem;
    }
    state.gpu = read_gpu_percent(&mut state.prev_gpu_rc6);

    let done = match &*state.ping.lock().unwrap() {
        PingState::Done(ok, ms) => Some((*ok, *ms)),
        _ => None,
    };
    if let Some((ok, ms)) = done {
        state.net_ok = Some(ok);
        state.ping_ms = ms;
        *state.ping.lock().unwrap() = PingState::Idle;
    }
}

fn settings_sync_ui(
    state: Res<SettingsState>,
    settings: Res<UserSettings>,
    ents: Res<SettingsEntities>,
    mut page_query: Query<(&SettingsPage, &mut Node)>,
    mut node_bg_query: Query<(&mut Node, &mut BackgroundColor), Without<SettingsPage>>,
    mut text_query: Query<&mut Text>,
    mut color_query: Query<&mut TextColor>,
) {
    for (i, tab) in TAB_ORDER.into_iter().enumerate() {
        if let Some(page_e) = ents.pages[i] {
            if let Ok((_, mut node)) = page_query.get_mut(page_e) {
                let target = if state.tab == tab {
                    Display::Flex
                } else {
                    Display::None
                };
                if node.display != target {
                    node.display = target;
                }
            }
        }
        if let Some(bg_e) = ents.nav_bg[i] {
            if let Ok((_, mut bg)) = node_bg_query.get_mut(bg_e) {
                let target = if state.tab == tab {
                    NAV_HIGHLIGHT
                } else {
                    Color::NONE
                };
                if bg.0 != target {
                    bg.0 = target;
                }
            }
        }
        if let Some(text_e) = ents.nav_text[i] {
            if let Ok(mut color) = color_query.get_mut(text_e) {
                let target = if state.tab == tab { ACCENT } else { TEXT_DIM };
                if color.0 != target {
                    color.0 = target;
                }
            }
        }
    }

    for ch in 0..4usize {
        let pct = settings.volumes[ch] as f32;
        if let Some(fill_e) = ents.fill[ch] {
            if let Ok((mut node, _)) = node_bg_query.get_mut(fill_e) {
                set_percent_width(&mut node, pct);
            }
        }
        if let Some(knob_e) = ents.knob[ch] {
            if let Ok((mut node, _)) = node_bg_query.get_mut(knob_e) {
                set_percent_left(&mut node, pct);
            }
        }
        if let Some(pct_e) = ents.pct[ch] {
            if let Ok(mut text) = text_query.get_mut(pct_e) {
                let target = format!("{}%", settings.volumes[ch]);
                if **text != target {
                    **text = target;
                }
            }
        }
        if let Some(bg_e) = ents.toggle_bg[ch] {
            if let Ok((_, mut bg)) = node_bg_query.get_mut(bg_e) {
                let target = if settings.toggles[ch] { ACCENT } else { TOGGLE_OFF };
                if bg.0 != target {
                    bg.0 = target;
                }
            }
        }
        if let Some(knob_e) = ents.toggle_knob[ch] {
            if let Ok((mut node, _)) = node_bg_query.get_mut(knob_e) {
                set_px_left(&mut node, if settings.toggles[ch] { 22.0 } else { 2.0 });
            }
        }
    }

    if let Some(label_e) = ents.quality_label {
        if let Ok(mut text) = text_query.get_mut(label_e) {
            let target = QUALITY_OPTIONS[settings.quality_idx].to_string();
            if **text != target {
                **text = target;
            }
        }
    }

    if let Some(bg_e) = ents.wallpaper_toggle_bg {
        if let Ok((_, mut bg)) = node_bg_query.get_mut(bg_e) {
            let target = if settings.wallpaper_enabled { ACCENT } else { TOGGLE_OFF };
            if bg.0 != target {
                bg.0 = target;
            }
        }
    }
    if let Some(knob_e) = ents.wallpaper_toggle_knob {
        if let Ok((mut node, _)) = node_bg_query.get_mut(knob_e) {
            set_px_left(&mut node, if settings.wallpaper_enabled { 22.0 } else { 2.0 });
        }
    }

    if let Some(bg_e) = ents.natural_scroll_toggle_bg {
        if let Ok((_, mut bg)) = node_bg_query.get_mut(bg_e) {
            let target = if settings.natural_scroll { ACCENT } else { TOGGLE_OFF };
            if bg.0 != target {
                bg.0 = target;
            }
        }
    }
    if let Some(knob_e) = ents.natural_scroll_toggle_knob {
        if let Ok((mut node, _)) = node_bg_query.get_mut(knob_e) {
            set_px_left(&mut node, if settings.natural_scroll { 22.0 } else { 2.0 });
        }
    }

    if let Some(status_e) = ents.net_status {
        let (target_text, target_color) = match state.net_ok {
            Some(true) => ("● 已连接", OK_GREEN),
            Some(false) => ("● 未连接", BAD_RED),
            None => ("检测中…", TEXT_DIM),
        };
        if let Ok(mut text) = text_query.get_mut(status_e) {
            if **text != target_text {
                **text = target_text.to_string();
            }
        }
        if let Ok(mut color) = color_query.get_mut(status_e) {
            if color.0 != target_color {
                color.0 = target_color;
            }
        }
    }

    if let Some(latency_e) = ents.net_latency {
        if let Ok(mut text) = text_query.get_mut(latency_e) {
            let target = match state.ping_ms {
                Some(ms) => {
                    let grade = if ms < 80.0 { "流畅" } else { "正常" };
                    format!("{:.0}ms（{}）", ms, grade)
                }
                None => "--".to_string(),
            };
            if **text != target {
                **text = target;
            }
        }
    }

    let values = [Some(state.cpu), Some(state.mem), state.gpu];
    for (i, value) in values.into_iter().enumerate() {
        if let Some(fill_e) = ents.bar_fill[i] {
            if let Ok((mut node, _)) = node_bg_query.get_mut(fill_e) {
                set_percent_width(&mut node, value.unwrap_or(0.0).clamp(0.0, 100.0));
            }
        }
        if let Some(pct_e) = ents.bar_pct[i] {
            if let Ok(mut text) = text_query.get_mut(pct_e) {
                let target = match value {
                    Some(v) => format!("{:.0}%", v),
                    None => "N/A".to_string(),
                };
                if **text != target {
                    **text = target;
                }
            }
        }
    }
}

fn set_percent_width(node: &mut Node, pct: f32) {
    if let Node {
        width: Val::Percent(w),
        ..
    } = node
    {
        if (*w - pct).abs() > 0.1 {
            *w = pct;
        }
    }
}

fn set_percent_left(node: &mut Node, pct: f32) {
    if let Node {
        left: Val::Percent(l),
        ..
    } = node
    {
        if (*l - pct).abs() > 0.1 {
            *l = pct;
        }
    }
}

fn set_px_left(node: &mut Node, px: f32) {
    if let Node {
        left: Val::Px(v),
        ..
    } = node
    {
        if (*v - px).abs() > 0.1 {
            *v = px;
        }
    }
}

fn spawn_model_page(
    parent: &mut ChildSpawnerCommands,
    fonts: &N3riFonts,
    ents: &mut SettingsEntities,
    state: &mut SettingsState,
) -> Entity {
    let page_e = parent
        .spawn((
            SettingsPage,
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(18.0),
                ..default()
            },
        ))
        .id();

    parent.commands().entity(page_e).with_children(|page| {
        page_header(page, fonts, "模型", "AI 接口配置(OpenAI 兼容)");

        let fields = [(0usize, "接口地址"), (1, "模型名称"), (2, "API Key")];
        for (i, label) in fields {
            let focused = state.llm_focus == Some(i);
            page.spawn((
                Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(10.0),
                    ..default()
                },
            ))
            .with_children(|row| {
                row.spawn((
                    Text::new(label),
                    TextFont {
                        font: FontSource::Handle(fonts.default.clone()),
                        font_size: FontSize::Px(13.0),
                        ..default()
                    },
                    TextColor(TEXT_MAIN),
                    Node {
                        width: Val::Px(80.0),
                        ..default()
                    },
                ));
                let mut shown = state.llm_form[i].clone();
                if focused {
                    shown.push('▏');
                }
                row.spawn((
                    LlmInput(i),
                    Button,
                    Node {
                        flex_grow: 1.0,
                        height: Val::Px(32.0),
                        align_items: AlignItems::Center,
                        padding: UiRect::left(Val::Px(10.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(6.0)),
                        overflow: Overflow::hidden(),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.04, 0.08, 0.14, 0.8)),
                    BorderColor::all(if focused {
                        ACCENT
                    } else {
                        BUTTON_BORDER_COLOR
                    }),
                ))
                .with_children(|box_| {
                    let t_e = box_
                        .spawn((
                            LlmInputText,
                            Text::new(shown),
                            TextFont {
                                font: FontSource::Handle(fonts.default.clone()),
                                font_size: FontSize::Px(13.0),
                                ..default()
                            },
                            TextColor(TEXT_MAIN),
                        ))
                        .id();
                    ents.llm_text[i] = Some(t_e);
                });
            });
        }

        page.spawn((
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(10.0),
                margin: UiRect::top(Val::Px(4.0)),
                ..default()
            },
        ))
        .with_children(|row| {
            let save_e = row
                .spawn((
                    LlmSaveBtn,
                    Button,
                    Node {
                        width: Val::Px(120.0),
                        height: Val::Px(36.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border_radius: BorderRadius::all(Val::Px(8.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    BorderColor::all(BUTTON_BORDER_COLOR),
                ))
                .with_children(|b| {
                    b.spawn((
                        Text::new("保存配置"),
                        TextFont {
                            font: FontSource::Handle(fonts.default.clone()),
                            font_size: FontSize::Px(13.0),
                            ..default()
                        },
                        TextColor(TEXT_MAIN),
                    ));
                })
                .id();
            let test_e = row
                .spawn((
                    LlmTestBtn,
                    Button,
                    Node {
                        width: Val::Px(120.0),
                        height: Val::Px(36.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border_radius: BorderRadius::all(Val::Px(8.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    BorderColor::all(BUTTON_BORDER_COLOR),
                ))
                .with_children(|b| {
                    b.spawn((
                        Text::new("测试连接"),
                        TextFont {
                            font: FontSource::Handle(fonts.default.clone()),
                            font_size: FontSize::Px(13.0),
                            ..default()
                        },
                        TextColor(TEXT_MAIN),
                    ));
                })
                .id();
            let _ = (save_e, test_e);
        });

        let status_e = page
            .spawn((
                LlmStatusText,
                Text::new(" "),
                TextFont {
                    font: FontSource::Handle(fonts.default.clone()),
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(TEXT_DIM),
            ))
            .id();
        ents.llm_status = Some(status_e);
    });

    page_e
}

fn llm_config_from_form(form: &[String; 3]) -> LlmConfig {
    LlmConfig {
        base_url: form[0].clone(),
        model: form[1].clone(),
        api_key: form[2].clone(),
        ..n3ri_llm::load_config()
    }
}

fn settings_llm_click(
    mouse: Res<ButtonInput<MouseButton>>,
    inputs: Query<(&LlmInput, &Interaction)>,
    save_btn: Query<&Interaction, With<LlmSaveBtn>>,
    test_btn: Query<&Interaction, With<LlmTestBtn>>,
    mut state: ResMut<SettingsState>,
    mut owner: ResMut<TextInputOwner>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let mut clicked_input = false;
    for (LlmInput(i), interaction) in inputs.iter() {
        if matches!(interaction, Interaction::Pressed | Interaction::Hovered) {
            state.llm_focus = Some(*i);
            owner.0 = TextInputFocus::Settings(*i);
            clicked_input = true;
        }
    }
    if !clicked_input {
        state.llm_focus = None;
        if matches!(owner.0, TextInputFocus::Settings(_)) {
            owner.0 = TextInputFocus::None;
        }
    }
    for interaction in save_btn.iter() {
        if *interaction == Interaction::Pressed {
            n3ri_llm::save_config(&llm_config_from_form(&state.llm_form));
        }
    }
    for interaction in test_btn.iter() {
        if *interaction == Interaction::Pressed {
            {
                let mut guard = state.llm_test.lock().unwrap();
                if matches!(&*guard, LlmTest::Running) {
                    return;
                }
                *guard = LlmTest::Running;
            }
            let cfg = llm_config_from_form(&state.llm_form);
            let shared = state.llm_test.clone();
            thread::spawn(move || {
                let result = LlmClient::new().test_connection(&cfg);
                *shared.lock().unwrap() = LlmTest::Done(result);
            });
        }
    }
}

fn settings_llm_input(
    mut keyboard_inputs: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<SettingsState>,
    mut owner: ResMut<TextInputOwner>,
) {
    let Some(focus) = state.llm_focus.filter(|index| owner.is(TextInputFocus::Settings(*index))) else {
        keyboard_inputs.clear();
        return;
    };
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    for event in keyboard_inputs.read() {
        if !event.state.is_pressed() {
            continue;
        }
        match &event.logical_key {
            Key::Character(ch) => {
                let raw = ch.chars().next().unwrap_or(' ');
                if ctrl && (raw == 'v' || raw == 'V') {
                    if let Some(text) = paste_text() {
                        state.llm_form[focus].push_str(&text);
                    }
                } else if !raw.is_control() {
                    state.llm_form[focus].push(raw);
                }
            }
            Key::Space => state.llm_form[focus].push(' '),
            Key::Backspace => {
                state.llm_form[focus].pop();
            }
            Key::Enter => {
                state.llm_focus = None;
                owner.0 = TextInputFocus::None;
            }
            _ => {}
        }
    }
}

fn settings_llm_ime(
    mut ime_events: MessageReader<Ime>,
    mut state: ResMut<SettingsState>,
    owner: Res<TextInputOwner>,
) {
    let Some(focus) = state
        .llm_focus
        .filter(|index| owner.is(TextInputFocus::Settings(*index)))
    else {
        ime_events.clear();
        return;
    };

    for event in ime_events.read() {
        match event {
            Ime::Commit { value, .. } => state.llm_form[focus].push_str(value),
            Ime::Preedit { .. } | Ime::Enabled { .. } | Ime::Disabled { .. } => {}
        }
    }
}

fn settings_llm_sync(
    state: Res<SettingsState>,
    ents: Res<SettingsEntities>,
    mut text_query: Query<&mut Text>,
) {
    for i in 0..3usize {
        if let Some(e) = ents.llm_text[i] {
            if let Ok(mut text) = text_query.get_mut(e) {
                let focused = state.llm_focus == Some(i);
                let mut shown = if i == 2 {
                    "•".repeat(state.llm_form[2].chars().count())
                } else {
                    state.llm_form[i].clone()
                };
                if focused {
                    shown.push('▏');
                }
                if **text != shown {
                    **text = shown;
                }
            }
        }
    }
    if let Some(e) = ents.llm_status {
        if let Ok(mut text) = text_query.get_mut(e) {
            let target = match &*state.llm_test.lock().unwrap() {
                LlmTest::Idle => " ".to_string(),
                LlmTest::Running => "测试中…".to_string(),
                LlmTest::Done(Ok(s)) => format!("连接成功:{s}"),
                LlmTest::Done(Err(e)) => format!("连接失败:{e}"),
            };
            if **text != target {
                **text = target;
            }
        }
    }
}
