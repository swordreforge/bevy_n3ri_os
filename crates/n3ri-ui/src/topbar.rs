use bevy::app::AppExit;
use bevy::prelude::*;
use chrono::{Datelike, Local as ChronoLocal, Timelike};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::font::N3riFonts;

const POPUP_BG: Color = Color::srgba(0.03, 0.04, 0.06, 0.96);
const POPUP_BORDER: Color = Color::srgba(0.4, 0.5, 0.6, 0.35);
const TEXT_MAIN: Color = Color::srgb(0.86, 0.93, 0.93);
const TEXT_DIM: Color = Color::srgba(0.6, 0.7, 0.8, 0.75);
const OK_GREEN: Color = Color::srgb(0.3, 0.85, 0.5);
const BAD_RED: Color = Color::srgb(0.9, 0.35, 0.35);

#[derive(Clone, Copy, PartialEq)]
enum PopupKind {
    Power,
    Time,
    Sound,
    Network,
    Battery,
    Cpu,
}

impl PopupKind {
    fn idx(self) -> usize {
        match self {
            PopupKind::Power => 0,
            PopupKind::Time => 1,
            PopupKind::Sound => 2,
            PopupKind::Network => 3,
            PopupKind::Battery => 4,
            PopupKind::Cpu => 5,
        }
    }
}

#[derive(Component)]
pub struct TopBar;

#[derive(Component)]
struct TopbarItem(PopupKind);

#[derive(Component)]
struct PopupPanel;

#[derive(Component)]
struct ExitOptionButton;

#[derive(Resource)]
pub struct FocusedTitle {
    pub title: String,
    pub entity: Option<Entity>,
}

impl Default for FocusedTitle {
    fn default() -> Self {
        Self {
            title: "n3ri_os".to_string(),
            entity: None,
        }
    }
}

#[derive(Resource)]
struct TopbarState {
    cpu: f32,
    battery: Option<(u8, bool)>,
    volume: Arc<Mutex<Option<u8>>>,
    volume_fetched: bool,
    prev_cpu: (u64, u64),
}

impl Default for TopbarState {
    fn default() -> Self {
        Self {
            cpu: 0.0,
            battery: None,
            volume: Arc::new(Mutex::new(None)),
            volume_fetched: false,
            prev_cpu: (0, 0),
        }
    }
}

#[derive(Resource, Default)]
struct TopbarEnts {
    focused_text: Option<Entity>,
    time_text: Option<Entity>,
    sound_text: Option<Entity>,
    net_icon: Option<Entity>,
    battery_text: Option<Entity>,
    cpu_text: Option<Entity>,
    popup_time_date: Option<Entity>,
    popup_time_clock: Option<Entity>,
    popup_sound_val: Option<Entity>,
    popup_net_val: Option<Entity>,
    popup_battery_line1: Option<Entity>,
    popup_battery_line2: Option<Entity>,
    popup_cpu_val: Option<Entity>,
    popups: [Option<Entity>; 6],
}

pub struct TopbarPlugin;

impl Plugin for TopbarPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FocusedTitle>()
            .init_resource::<TopbarState>()
            .init_resource::<TopbarEnts>()
            .add_systems(
                Update,
                (
                    topbar_time,
                    topbar_poll,
                    topbar_popup_toggle,
                    topbar_exit,
                    topbar_focus_sync,
                ),
            );
    }
}

pub fn spawn_topbar(
    parent: &mut ChildSpawnerCommands,
    asset_server: &AssetServer,
    fonts: &N3riFonts,
) {
    let mut ents = TopbarEnts::default();

    parent
        .spawn((
            TopBar,
            GlobalZIndex(100),
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(32.0),
                padding: UiRect {
                    left: Val::Px(10.0),
                    right: Val::Px(10.0),
                    ..default()
                },
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                display: Display::Flex,
                ..default()
            },
            BackgroundColor(Color::srgba(0.05, 0.1, 0.18, 0.92)),
        ))
        .with_children(|topbar| {
            topbar
                .spawn((Node {
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(8.0),
                    display: Display::Flex,
                    ..default()
                },))
                .with_children(|left| {
                    let power_popup = spawn_power_popup(left, fonts);
                    ents.popups[PopupKind::Power.idx()] = Some(power_popup);

                    left.spawn((
                        Button,
                        TopbarItem(PopupKind::Power),
                        Node {
                            align_items: AlignItems::Center,
                            column_gap: Val::Px(6.0),
                            display: Display::Flex,
                            padding: UiRect::all(Val::Px(4.0)),
                            border_radius: BorderRadius::all(Val::Px(6.0)),
                            ..default()
                        },
                    ))
                    .with_children(|btn| {
                        btn.spawn((
                            ImageNode {
                                image: asset_server.load("nori/icon.png"),
                                ..default()
                            },
                            Node {
                                width: Val::Px(22.0),
                                height: Val::Px(22.0),
                                ..default()
                            },
                        ));
                    });

                    ents.focused_text = Some(
                        left.spawn((
                            Text::new("n3ri_os"),
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

            topbar
                .spawn((Node {
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(14.0),
                    display: Display::Flex,
                    ..default()
                },))
                .with_children(|right| {
                    let specs: [(PopupKind, Option<Handle<Image>>, &str); 5] = [
                        (PopupKind::Cpu, Some(asset_server.load("nori/cpu.png")), "--"),
                        (PopupKind::Battery, Some(asset_server.load("nori/battery.png")), "--"),
                        (PopupKind::Network, Some(asset_server.load("nori/network.png")), ""),
                        (PopupKind::Sound, Some(asset_server.load("nori/sound.png")), "--"),
                        (PopupKind::Time, None, "--"),
                    ];

                    for (kind, icon, label) in specs {
                        let (item_text_e, popup_e, popup_val_e, popup_line2_e) =
                            spawn_status_item(right, fonts, kind, icon, label);
                        ents.popups[kind.idx()] = Some(popup_e);

                        match kind {
                            PopupKind::Cpu => ents.cpu_text = Some(item_text_e),
                            PopupKind::Battery => ents.battery_text = Some(item_text_e),
                            PopupKind::Network => ents.net_icon = Some(item_text_e),
                            PopupKind::Sound => ents.sound_text = Some(item_text_e),
                            PopupKind::Time => ents.time_text = Some(item_text_e),
                            PopupKind::Power => {}
                        }

                        match kind {
                            PopupKind::Time => {
                                ents.popup_time_date = popup_val_e;
                                ents.popup_time_clock = popup_line2_e;
                            }
                            PopupKind::Sound => ents.popup_sound_val = popup_val_e,
                            PopupKind::Network => ents.popup_net_val = popup_val_e,
                            PopupKind::Battery => {
                                ents.popup_battery_line1 = popup_val_e;
                                ents.popup_battery_line2 = popup_line2_e;
                            }
                            PopupKind::Cpu => ents.popup_cpu_val = popup_val_e,
                            PopupKind::Power => {}
                        }
                    }
                });
        });

    parent.commands().insert_resource(ents);
}

fn spawn_status_item(
    parent: &mut ChildSpawnerCommands,
    fonts: &N3riFonts,
    kind: PopupKind,
    icon: Option<Handle<Image>>,
    label: &str,
) -> (Entity, Entity, Option<Entity>, Option<Entity>) {
    let mut item_text_e = Entity::PLACEHOLDER;
    let mut popup_e = Entity::PLACEHOLDER;
    let mut popup_val_e = None;
    let mut popup_line2_e = None;

    let item = parent
        .spawn((
            Button,
            TopbarItem(kind),
            Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                display: Display::Flex,
                padding: UiRect::all(Val::Px(4.0)),
                border_radius: BorderRadius::all(Val::Px(6.0)),
                ..default()
            },
        ))
        .id();

    parent.commands().entity(item).with_children(|btn| {
        if let Some(icon) = icon {
            btn.spawn((
                ImageNode {
                    image: icon,
                    ..default()
                },
                Node {
                    width: Val::Px(18.0),
                    height: Val::Px(18.0),
                    ..default()
                },
            ));
        }

        item_text_e = btn
            .spawn((
                Text::new(label),
                TextFont {
                    font: FontSource::Handle(fonts.default.clone()),
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(TEXT_MAIN),
            ))
            .id();

        popup_e = btn
            .spawn((
                PopupPanel,
                Visibility::Hidden,
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(36.0),
                    right: Val::Px(0.0),
                    min_width: Val::Px(150.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(6.0),
                    padding: UiRect::all(Val::Px(12.0)),
                    border_radius: BorderRadius::all(Val::Px(10.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(POPUP_BG),
                BorderColor::all(POPUP_BORDER),
            ))
            .with_children(|panel| {
                popup_val_e = Some(
                    panel
                        .spawn((
                            Text::new("…"),
                            TextFont {
                                font: FontSource::Handle(fonts.default.clone()),
                                font_size: if kind == PopupKind::Time {
                                    FontSize::Px(20.0)
                                } else {
                                    FontSize::Px(14.0)
                                },
                                ..default()
                            },
                            TextColor(TEXT_MAIN),
                        ))
                        .id(),
                );

                if kind == PopupKind::Battery {
                    popup_line2_e = Some(
                        panel
                            .spawn((
                                Text::new(""),
                                TextFont {
                                    font: FontSource::Handle(fonts.default.clone()),
                                    font_size: FontSize::Px(12.0),
                                    ..default()
                                },
                                TextColor(TEXT_DIM),
                            ))
                            .id(),
                    );
                }
            })
            .id();
    });

    (item_text_e, popup_e, popup_val_e, popup_line2_e)
}

fn spawn_power_popup(parent: &mut ChildSpawnerCommands, fonts: &N3riFonts) -> Entity {
    parent
        .spawn((
            PopupPanel,
            Visibility::Hidden,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(36.0),
                left: Val::Px(0.0),
                width: Val::Px(160.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(4.0),
                padding: UiRect::all(Val::Px(8.0)),
                border_radius: BorderRadius::all(Val::Px(10.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(POPUP_BG),
            BorderColor::all(POPUP_BORDER),
        ))
        .with_children(|panel| {
            panel
                .spawn((
                    Button,
                    ExitOptionButton,
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(32.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border_radius: BorderRadius::all(Val::Px(6.0)),
                        ..default()
                    },
                ))
                .with_children(|opt| {
                    opt.spawn((
                        Text::new("退出系统"),
                        TextFont {
                            font: FontSource::Handle(fonts.default.clone()),
                            font_size: FontSize::Px(13.0),
                            ..default()
                        },
                        TextColor(TEXT_MAIN),
                    ));
                });
        })
        .id()
}

fn topbar_time(
    time: Res<Time>,
    mut local: Local<f32>,
    ents: Res<TopbarEnts>,
    mut text_query: Query<&mut Text>,
) {
    *local += time.delta_secs();
    if *local < 1.0 {
        return;
    }
    *local = 0.0;

    let now = ChronoLocal::now();
    let weekday =
        ["周日", "周一", "周二", "周三", "周四", "周五", "周六"]
            [now.weekday().num_days_from_sunday() as usize];

    if let Some(e) = ents.time_text {
        if let Ok(mut text) = text_query.get_mut(e) {
            let target = format!(
                "{}月{}日 {} {:02}:{:02}",
                now.month(),
                now.day(),
                weekday,
                now.hour(),
                now.minute()
            );
            if **text != target {
                **text = target;
            }
        }
    }
    if let Some(e) = ents.popup_time_date {
        if let Ok(mut text) = text_query.get_mut(e) {
            let target = format!(
                "{}年{}月{}日 {}",
                now.year(),
                now.month(),
                now.day(),
                weekday
            );
            if **text != target {
                **text = target;
            }
        }
    }
    if let Some(e) = ents.popup_time_clock {
        if let Ok(mut text) = text_query.get_mut(e) {
            let target = format!(
                "{:02}:{:02}:{:02}",
                now.hour(),
                now.minute(),
                now.second()
            );
            if **text != target {
                **text = target;
            }
        }
    }
}

fn topbar_poll(
    time: Res<Time>,
    mut local: Local<f32>,
    mut state: ResMut<TopbarState>,
    ents: Res<TopbarEnts>,
    mut text_query: Query<&mut Text>,
    mut color_query: Query<&mut TextColor>,
    mut image_query: Query<&mut ImageNode>,
) {
    *local += time.delta_secs();
    if *local < 1.0 {
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

    let net = net_online();

    if !state.volume_fetched {
        state.volume_fetched = true;
        let shared = state.volume.clone();
        thread::spawn(move || {
            let pct = std::process::Command::new("pactl")
                .args(["get-sink-volume", "@DEFAULT_SINK@"])
                .env("LC_ALL", "C")
                .output()
                .ok()
                .and_then(|out| {
                    let text = String::from_utf8_lossy(&out.stdout).to_string();
                    parse_volume_pct(&text)
                });
            *shared.lock().unwrap() = pct;
        });
    }
    let volume = *state.volume.lock().unwrap();

    state.battery = read_battery();

    if let Some(e) = ents.cpu_text {
        if let Ok(mut text) = text_query.get_mut(e) {
            let target = format!("{:.0}%", state.cpu);
            if **text != target {
                **text = target;
            }
        }
    }
    if let Some(e) = ents.net_icon {
        if let Ok(mut img) = image_query.get_mut(e) {
            let target = if net { OK_GREEN } else { BAD_RED };
            if img.color != target {
                img.color = target;
            }
        }
    }
    if let Some(e) = ents.battery_text {
        if let Ok(mut text) = text_query.get_mut(e) {
            let target = match state.battery {
                Some((cap, _)) => format!("{}%", cap),
                None => "AC".to_string(),
            };
            if **text != target {
                **text = target;
            }
        }
    }
    if let Some(e) = ents.sound_text {
        if let Ok(mut text) = text_query.get_mut(e) {
            let target = match volume {
                Some(v) => format!("{}%", v),
                None => "--".to_string(),
            };
            if **text != target {
                **text = target;
            }
        }
    }

    if let Some(e) = ents.popup_cpu_val {
        if let Ok(mut text) = text_query.get_mut(e) {
            let target = format!("{:.0}%", state.cpu);
            if **text != target {
                **text = target;
            }
        }
    }
    if let Some(e) = ents.popup_net_val {
        if let Ok(mut text) = text_query.get_mut(e) {
            let target = if net { "已连接" } else { "未连接" }.to_string();
            if **text != target {
                **text = target;
            }
        }
        if let Ok(mut color) = color_query.get_mut(e) {
            let target = if net { OK_GREEN } else { BAD_RED };
            if color.0 != target {
                color.0 = target;
            }
        }
    }
    if let Some(e) = ents.popup_sound_val {
        if let Ok(mut text) = text_query.get_mut(e) {
            let target = match volume {
                Some(v) => format!("{}%", v),
                None => "未获取".to_string(),
            };
            if **text != target {
                **text = target;
            }
        }
    }
    if let Some(e) = ents.popup_battery_line1 {
        if let Ok(mut text) = text_query.get_mut(e) {
            let target = match state.battery {
                Some((cap, _)) => format!("电量 {}%", cap),
                None => "交流供电".to_string(),
            };
            if **text != target {
                **text = target;
            }
        }
    }
    if let Some(e) = ents.popup_battery_line2 {
        if let Ok(mut text) = text_query.get_mut(e) {
            let target = match state.battery {
                Some((_, true)) => "充电中".to_string(),
                Some((_, false)) => "使用电池".to_string(),
                None => String::new(),
            };
            if **text != target {
                **text = target;
            }
        }
    }
}

fn topbar_popup_toggle(
    mouse: Res<ButtonInput<MouseButton>>,
    items: Query<(&TopbarItem, &Interaction)>,
    mut popups: Query<&mut Visibility, With<PopupPanel>>,
    ents: Res<TopbarEnts>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }

    let mut pressed_kind: Option<PopupKind> = None;
    let mut was_open = false;
    for (item, interaction) in items.iter() {
        if *interaction == Interaction::Pressed {
            pressed_kind = Some(item.0);
            if let Some(e) = ents.popups[item.0.idx()] {
                was_open = popups
                    .get(e)
                    .map(|v| *v == Visibility::Inherited)
                    .unwrap_or(false);
            }
        }
    }

    for (kind_idx, e_opt) in ents.popups.iter().enumerate() {
        if let Some(e) = e_opt {
            if let Ok(mut vis) = popups.get_mut(*e) {
                let should_open = pressed_kind
                    .map(|k| k.idx() == kind_idx && !was_open)
                    .unwrap_or(false);
                let target = if should_open {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
                if *vis != target {
                    *vis = target;
                }
            }
        }
    }
}

fn topbar_exit(
    mouse: Res<ButtonInput<MouseButton>>,
    exit_query: Query<&Interaction, With<ExitOptionButton>>,
    mut exit_writer: MessageWriter<AppExit>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for interaction in exit_query.iter() {
        if *interaction == Interaction::Pressed {
            exit_writer.write(AppExit::Success);
        }
    }
}

fn topbar_focus_sync(
    focused: Res<FocusedTitle>,
    ents: Res<TopbarEnts>,
    mut text_query: Query<&mut Text>,
) {
    if !focused.is_changed() {
        return;
    }
    if let Some(e) = ents.focused_text {
        if let Ok(mut text) = text_query.get_mut(e) {
            if **text != focused.title {
                **text = focused.title.clone();
            }
        }
    }
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

fn net_online() -> bool {
    std::fs::read_to_string("/proc/net/route")
        .map(|content| {
            content
                .lines()
                .skip(1)
                .any(|l| l.split_whitespace().nth(1) == Some("00000000"))
        })
        .unwrap_or(false)
}

fn read_battery() -> Option<(u8, bool)> {
    let dir = std::fs::read_dir("/sys/class/power_supply").ok()?;
    for entry in dir.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("BAT") {
            let base = format!("/sys/class/power_supply/{name}");
            let cap = std::fs::read_to_string(format!("{base}/capacity"))
                .ok()?
                .trim()
                .parse()
                .ok()?;
            let status = std::fs::read_to_string(format!("{base}/status"))
                .unwrap_or_default();
            return Some((cap, status.contains("Charging")));
        }
    }
    None
}

fn parse_volume_pct(text: &str) -> Option<u8> {
    let idx = text.find('%')?;
    let before = &text[..idx];
    let digits: String = before
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.chars().rev().collect::<String>().parse().ok()
}
