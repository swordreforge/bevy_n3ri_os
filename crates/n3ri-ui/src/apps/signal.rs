use bevy::prelude::*;
use serde::Deserialize;
use crate::font::N3riFonts;
use crate::scroll::{ScrollableArea, ScrollContent};
use crate::window::spawn_window;

const SIDEBAR_BG: Color = Color::srgb(0.05, 0.08, 0.13);
const LIST_HL: Color = Color::srgba(0.16, 0.25, 0.33, 0.85);
const TEXT_MAIN: Color = Color::srgb(0.86, 0.93, 0.93);
const TEXT_DIM: Color = Color::srgb(0.45, 0.55, 0.62);
const ACCENT: Color = Color::srgb(0.45, 0.76, 0.83);
const BUBBLE_OTHER: Color = Color::srgba(0.17, 0.23, 0.31, 0.92);
const BUBBLE_ME: Color = Color::srgba(0.24, 0.32, 0.41, 0.95);
const AVATAR_BG: Color = Color::srgba(0.2, 0.35, 0.42, 0.9);
const CARD_BG: Color = Color::srgba(0.12, 0.18, 0.25, 0.6);

#[derive(Deserialize)]
struct SignalMsgJson {
    timestamp: String,
    sender: String,
    #[serde(default)]
    #[allow(dead_code)]
    r#type: String,
    content: String,
}

#[derive(Deserialize)]
struct SignalChatJson {
    chat_history: Vec<SignalMsgJson>,
}

#[derive(Deserialize)]
struct OtherSession {
    contact: String,
    date: String,
    messages: Vec<SignalMsgJson>,
}

type OtherJson = Vec<OtherSession>;

#[derive(Clone, Copy, PartialEq)]
enum MsgKind {
    Me,
    Other,
    System,
    Attachment,
    Image,
}

struct SignalMsg {
    date: String,
    time: String,
    kind: MsgKind,
    lines: Vec<String>,
}

struct ChatSession {
    contact: String,
    date: String,
    time: String,
    preview: String,
    messages: Vec<SignalMsg>,
}

#[derive(Resource, Default)]
struct SignalState {
    selected: usize,
}

#[derive(Component)]
struct SignalItemBtn {
    index: usize,
}

#[derive(Component)]
struct SignalListContent;

#[derive(Component)]
struct SignalViewContent;

#[derive(Component)]
struct SignalNeedsSync;

#[derive(Component)]
struct SignalHeaderName;

#[derive(Component)]
struct SignalHeaderManaged;

pub struct SignalPlugin;

impl Plugin for SignalPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SignalState>()
            .add_systems(Update, (signal_item_click, signal_sync_ui));
    }
}

fn date_key(date: &str) -> (u32, u32) {
    let nums: Vec<u32> = date
        .split(|c: char| !c.is_ascii_digit())
        .filter_map(|s| s.parse().ok())
        .collect();
    match nums.as_slice() {
        [m, d] => (*m, *d),
        _ => (0, 0),
    }
}

fn wrap_line(line: &str, max_chars: usize) -> Vec<String> {
    let chars: Vec<char> = line.chars().collect();
    if chars.len() <= max_chars {
        return vec![line.to_string()];
    }
    chars
        .chunks(max_chars)
        .map(|c| c.iter().collect())
        .collect()
}

fn split_ts(ts: &str) -> (String, String) {
    match ts.split_once(' ') {
        Some((d, t)) => (d.to_string(), t.to_string()),
        None => (ts.to_string(), String::new()),
    }
}

fn to_kind(sender: &str, ty: &str, contact: &str) -> MsgKind {
    match ty {
        "system_notice" => MsgKind::System,
        "attachment" => MsgKind::Attachment,
        "image" => MsgKind::Image,
        _ => {
            if sender == "我" {
                MsgKind::Me
            } else if sender == contact {
                MsgKind::Other
            } else {
                MsgKind::System
            }
        }
    }
}

fn build_messages(
    raw: &[SignalMsgJson],
    contact: &str,
    date_override: Option<&str>,
) -> Vec<SignalMsg> {
    raw.iter()
        .map(|m| {
            let (date, time) = match date_override {
                Some(d) => (d.to_string(), m.timestamp.clone()),
                None => split_ts(&m.timestamp),
            };
            let kind = to_kind(&m.sender, &m.r#type, contact);
            let lines = m
                .content
                .split('\n')
                .flat_map(|l| wrap_line(l, 50))
                .collect();
            SignalMsg {
                date,
                time,
                kind,
                lines,
            }
        })
        .collect()
}

fn load_sessions() -> Vec<ChatSession> {
    let dir = "nori/app-icons/signal/聊天记录";
    let mut sessions: Vec<ChatSession> = Vec::new();

    if let Some(raw) = crate::content::read_to_string(&format!("{dir}/丹尼尔.chat.json")) {
        if let Ok(chat) = serde_json::from_str::<SignalChatJson>(&raw) {
            let messages = build_messages(&chat.chat_history, "丹尼尔", None);
            let date = messages
                .last()
                .map(|m| m.date.clone())
                .unwrap_or_default();
            let time = messages
                .last()
                .map(|m| m.time.clone())
                .unwrap_or_default();
            let preview = messages
                .last()
                .map(|m| m.lines.join(""))
                .unwrap_or_default();
            sessions.push(ChatSession {
                contact: "丹尼尔".to_string(),
                date,
                time,
                preview,
                messages,
            });
        }
    }

    if let Some(raw) = crate::content::read_to_string(&format!("{dir}/other.chat.json")) {
        if let Ok(other) = serde_json::from_str::<OtherJson>(&raw) {
            for c in &other {
                let messages = build_messages(&c.messages, &c.contact, Some(&c.date));
                let date = c.date.clone();
                let time = messages
                    .last()
                    .map(|m| m.time.clone())
                    .unwrap_or_default();
                let preview = messages
                    .last()
                    .map(|m| m.lines.join(""))
                    .unwrap_or_default();
                sessions.push(ChatSession {
                    contact: c.contact.clone(),
                    date,
                    time,
                    preview,
                    messages,
                });
            }
        }
    }

    sessions.sort_by(|a, b| date_key(&b.date).cmp(&date_key(&a.date)));
    sessions
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let t: String = s.chars().take(n).collect();
        format!("{}…", t)
    }
}

pub fn spawn_signal(parent: &mut ChildSpawnerCommands, fonts: &N3riFonts) {
    let window_entity = spawn_window(parent, "消息", "signal", 1000.0, 680.0, fonts);
    let font = fonts.default.clone();

    parent
        .commands()
        .entity(window_entity)
        .with_children(|window| {
            window
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        min_height: Val::Px(0.0),
                        ..default()
                    },
                ))
                .with_children(|col| {
                    col.spawn((Node {
                        width: Val::Percent(100.0),
                        flex_grow: 1.0,
                        min_height: Val::Px(0.0),
                        flex_direction: FlexDirection::Row,
                        ..default()
                    },))
                    .with_children(|row| {
                        row.spawn((
                            Node {
                                width: Val::Px(300.0),
                                height: Val::Percent(100.0),
                                flex_shrink: 0.0,
                                flex_direction: FlexDirection::Column,
                                padding: UiRect::all(Val::Px(10.0)),
                                row_gap: Val::Px(6.0),
                                ..default()
                            },
                            BackgroundColor(SIDEBAR_BG),
                        ))
                        .with_children(|sidebar| {
                            sidebar
                                .spawn((Node {
                                    width: Val::Percent(100.0),
                                    justify_content: JustifyContent::SpaceBetween,
                                    align_items: AlignItems::Center,
                                    padding: UiRect {
                                        left: Val::Px(6.0),
                                        right: Val::Px(6.0),
                                        top: Val::Px(4.0),
                                        bottom: Val::Px(8.0),
                                    },
                                    ..default()
                                },))
                                .with_children(|head| {
                                    head.spawn((
                                        Text::new("对话"),
                                        TextFont {
                                            font: FontSource::Handle(font.clone()),
                                            font_size: FontSize::Px(20.0),
                                            ..default()
                                        },
                                        TextColor(TEXT_MAIN),
                                    ));
                                    head.spawn((
                                        Text::new("✏"),
                                        TextFont {
                                            font: FontSource::Handle(font.clone()),
                                            font_size: FontSize::Px(18.0),
                                            ..default()
                                        },
                                        TextColor(TEXT_DIM),
                                    ));
                                });

                            sidebar.spawn((
                                Node {
                                    width: Val::Percent(100.0),
                                    height: Val::Px(34.0),
                                    align_items: AlignItems::Center,
                                    padding: UiRect::horizontal(Val::Px(10.0)),
                                    border_radius: BorderRadius::all(Val::Px(17.0)),
                                    column_gap: Val::Px(8.0),
                                    ..default()
                                },
                                BackgroundColor(Color::srgba(0.1, 0.15, 0.21, 0.8)),
                            ))
                            .with_children(|search| {
                                search.spawn((
                                    Text::new("🔍 搜索"),
                                    TextFont {
                                        font: FontSource::Handle(font.clone()),
                                        font_size: FontSize::Px(13.0),
                                        ..default()
                                    },
                                    TextColor(TEXT_DIM),
                                ));
                            });

                            let area_e = sidebar
                                .spawn((
                                    ScrollableArea::default(),
                                    Node {
                                        width: Val::Percent(100.0),
                                        flex_grow: 1.0,
                                        min_height: Val::Px(0.0),
                                        align_items: AlignItems::FlexStart,
                                        overflow: Overflow::hidden(),
                                        margin: UiRect::top(Val::Px(8.0)),
                                        ..default()
                                    },
                                ))
                                .id();

                            sidebar.commands().entity(area_e).with_children(|a| {
                                crate::scroll::spawn_scrollbar(a, area_e);
                            });

                            sidebar.commands().entity(area_e).with_children(|area| {
                                area.spawn((
                                    ScrollContent,
                                    SignalListContent,
                                    SignalNeedsSync,
                                    Node {
                                        width: Val::Percent(100.0),
                                        min_height: Val::Px(0.0),
                                        flex_direction: FlexDirection::Column,
                                        row_gap: Val::Px(4.0),
                                        ..default()
                                    },
                                ));
                            });

                            sidebar.spawn((
                                Text::new("仅显示最近 30 天的消息"),
                                TextFont {
                                    font: FontSource::Handle(font.clone()),
                                    font_size: FontSize::Px(11.0),
                                    ..default()
                                },
                                TextColor(TEXT_DIM),
                                Node {
                                    width: Val::Percent(100.0),
                                    margin: UiRect::top(Val::Px(6.0)),
                                    ..default()
                                },
                            ));
                        });

                    let view_e = row
                        .spawn((
                            Node {
                                flex_grow: 1.0,
                                height: Val::Percent(100.0),
                                min_height: Val::Px(0.0),
                                min_width: Val::Px(0.0),
                                flex_direction: FlexDirection::Column,
                                ..default()
                            },
                        ))
                        .id();

                    row.commands().entity(view_e).with_children(|v| {
                        v.spawn((
                            Node {
                                width: Val::Percent(100.0),
                                height: Val::Px(52.0),
                                align_items: AlignItems::Center,
                                padding: UiRect::horizontal(Val::Px(16.0)),
                                column_gap: Val::Px(10.0),
                                ..default()
                            },
                            BackgroundColor(SIDEBAR_BG),
                        ))
                        .with_children(|head| {
                            head.spawn((Node {
                                column_gap: Val::Px(8.0),
                                align_items: AlignItems::Center,
                                ..default()
                            },))
                            .with_children(|h| {
                                h.spawn((
                                    SignalHeaderName,
                                    Text::new(""),
                                    TextFont {
                                        font: FontSource::Handle(font.clone()),
                                        font_size: FontSize::Px(16.0),
                                        ..default()
                                    },
                                    TextColor(TEXT_MAIN),
                                ));
                                h.spawn((
                                    SignalHeaderManaged,
                                    Text::new("☁ OpenFlaw 托管"),
                                    TextFont {
                                        font: FontSource::Handle(font.clone()),
                                        font_size: FontSize::Px(11.0),
                                        ..default()
                                    },
                                    TextColor(ACCENT),
                                    Visibility::Hidden,
                                ));
                            });
                            head.spawn((
                                Text::new("🔒 端到端加密"),
                                TextFont {
                                    font: FontSource::Handle(font.clone()),
                                    font_size: FontSize::Px(11.0),
                                    ..default()
                                },
                                TextColor(TEXT_DIM),
                            ));
                        });

                        let chat_e = v
                            .spawn((
                                ScrollableArea::default(),
                                Node {
                                    width: Val::Percent(100.0),
                                    flex_grow: 1.0,
                                    min_height: Val::Px(0.0),
                                    align_items: AlignItems::FlexStart,
                                    overflow: Overflow::hidden(),
                                    ..default()
                                },
                            ))
                            .id();

                        v.commands().entity(chat_e).with_children(|a| {
                            crate::scroll::spawn_scrollbar(a, chat_e);
                        });

                        v.commands().entity(chat_e).with_children(|area| {
                            area.spawn((
                                ScrollContent,
                                SignalViewContent,
                                SignalNeedsSync,
                                Node {
                                    width: Val::Percent(100.0),
                                    min_height: Val::Px(0.0),
                                    flex_direction: FlexDirection::Column,
                                    padding: UiRect {
                                        left: Val::Px(20.0),
                                        right: Val::Px(20.0),
                                        top: Val::Px(14.0),
                                        bottom: Val::Px(14.0),
                                    },
                                    row_gap: Val::Px(8.0),
                                    ..default()
                                },
                            ));
                        });

                        v.spawn((
                            Node {
                                width: Val::Percent(100.0),
                                height: Val::Px(46.0),
                                align_items: AlignItems::Center,
                                padding: UiRect::horizontal(Val::Px(16.0)),
                                column_gap: Val::Px(10.0),
                                ..default()
                            },
                            BackgroundColor(SIDEBAR_BG),
                        ))
                        .with_children(|input| {
                            input.spawn((
                                Text::new("+"),
                                TextFont {
                                    font: FontSource::Handle(font.clone()),
                                    font_size: FontSize::Px(18.0),
                                    ..default()
                                },
                                TextColor(TEXT_DIM),
                            ));
                            input.spawn((
                                Node {
                                    flex_grow: 1.0,
                                    height: Val::Px(30.0),
                                    align_items: AlignItems::Center,
                                    padding: UiRect::horizontal(Val::Px(12.0)),
                                    border_radius: BorderRadius::all(Val::Px(15.0)),
                                    column_gap: Val::Px(6.0),
                                    ..default()
                                },
                                BackgroundColor(Color::srgba(0.1, 0.15, 0.21, 0.8)),
                            ))
                            .with_children(|field| {
                                field.spawn((
                                    Text::new("🔒 账号受限"),
                                    TextFont {
                                        font: FontSource::Handle(font.clone()),
                                        font_size: FontSize::Px(12.0),
                                        ..default()
                                    },
                                    TextColor(TEXT_DIM),
                                ));
                            });
                            input.spawn((
                                Text::new("➤"),
                                TextFont {
                                    font: FontSource::Handle(font.clone()),
                                    font_size: FontSize::Px(16.0),
                                    ..default()
                                },
                                TextColor(ACCENT),
                            ));
                        });
                    });
                });
        });
    });
}

fn spawn_text_lines(
    parent: &mut ChildSpawnerCommands,
    lines: &[String],
    color: Color,
    size: f32,
    font: &Handle<Font>,
) {
    for line in lines {
        parent.spawn((
            Text::new(line.clone()),
            TextFont {
                font: FontSource::Handle(font.clone()),
                font_size: FontSize::Px(size),
                ..default()
            },
            TextColor(color),
        ));
    }
}

fn signal_item_click(
    interaction_q: Query<(&Interaction, &SignalItemBtn), With<Button>>,
    mut state: ResMut<SignalState>,
) {
    for (interaction, ib) in interaction_q.iter() {
        if *interaction == Interaction::Pressed && state.selected != ib.index {
            state.selected = ib.index;
        }
    }
}

fn signal_sync_ui(
    state: Res<SignalState>,
    list_q: Query<Entity, With<SignalListContent>>,
    view_q: Query<Entity, With<SignalViewContent>>,
    needs_q: Query<Entity, With<SignalNeedsSync>>,
    child_of_q: Query<&bevy::prelude::ChildOf>,
    mut view_areas: Query<&mut ScrollPosition, With<ScrollableArea>>,
    mut header_name_q: Query<&mut Text, With<SignalHeaderName>>,
    mut header_managed_q: Query<&mut Visibility, With<SignalHeaderManaged>>,
    mut commands: Commands,
    fonts: Res<N3riFonts>,
) {
    if !state.is_changed() && needs_q.is_empty() {
        return;
    }
    for e in needs_q.iter() {
        commands.entity(e).remove::<SignalNeedsSync>();
    }
    let font = fonts.default.clone();
    let sessions = load_sessions();
    let selected = state.selected.min(sessions.len().saturating_sub(1));

    if let Some(session) = sessions.get(selected) {
        if let Ok(mut text) = header_name_q.single_mut() {
            **text = session.contact.clone();
        }
        if let Ok(mut vis) = header_managed_q.single_mut() {
            *vis = if session.contact == "丹尼尔" {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
    }

    for list_e in list_q.iter() {
        commands.entity(list_e).despawn_children();
        let font = font.clone();
        commands.entity(list_e).with_children(|list| {
            for (i, s) in sessions.iter().enumerate() {
                let bg = if i == selected { LIST_HL } else { Color::NONE };
                list.spawn((
                    SignalItemBtn { index: i },
                    Button,
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(54.0),
                        column_gap: Val::Px(10.0),
                        padding: UiRect::all(Val::Px(8.0)),
                        border_radius: BorderRadius::all(Val::Px(6.0)),
                        align_items: AlignItems::Center,
                        overflow: Overflow::hidden(),
                        ..default()
                    },
                    BackgroundColor(bg),
                ))
                .with_children(|item| {
                    item.spawn((
                        Node {
                            width: Val::Px(38.0),
                            height: Val::Px(38.0),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            border_radius: BorderRadius::all(Val::Px(19.0)),
                            ..default()
                        },
                        BackgroundColor(AVATAR_BG),
                    ))
                    .with_children(|avatar| {
                        avatar.spawn((
                            Text::new(
                                s.contact
                                    .chars()
                                    .next()
                                    .map(|c| c.to_string())
                                    .unwrap_or_else(|| "?".to_string()),
                            ),
                            TextFont {
                                font: FontSource::Handle(font.clone()),
                                font_size: FontSize::Px(16.0),
                                ..default()
                            },
                            TextColor(ACCENT),
                        ));
                    });

                    item.spawn((Node {
                        flex_grow: 1.0,
                        min_width: Val::Px(0.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(2.0),
                        ..default()
                    },))
                    .with_children(|info| {
                        info.spawn((Node {
                            width: Val::Percent(100.0),
                            justify_content: JustifyContent::SpaceBetween,
                            align_items: AlignItems::Center,
                            column_gap: Val::Px(8.0),
                            ..default()
                        },))
                        .with_children(|l1| {
                            l1.spawn((
                                Text::new(truncate(&s.contact, 8)),
                                TextFont {
                                    font: FontSource::Handle(font.clone()),
                                    font_size: FontSize::Px(15.0),
                                    ..default()
                                },
                                TextColor(if i == selected { TEXT_MAIN } else { ACCENT }),
                            ));
                            l1.spawn((
                                Text::new(if s.time.is_empty() {
                                    s.date.clone()
                                } else {
                                    s.time.clone()
                                }),
                                TextFont {
                                    font: FontSource::Handle(font.clone()),
                                    font_size: FontSize::Px(11.0),
                                    ..default()
                                },
                                TextColor(TEXT_DIM),
                            ));
                        });
                        info.spawn((
                            Text::new(truncate(&s.preview, 24)),
                            TextFont {
                                font: FontSource::Handle(font.clone()),
                                font_size: FontSize::Px(12.0),
                                ..default()
                            },
                            TextColor(TEXT_DIM),
                        ));
                    });
                });
            }
        });
    }

    for view_e in view_q.iter() {
        commands.entity(view_e).despawn_children();

        let mut cur = child_of_q.get(view_e).ok().map(|c| c.0);
        while let Some(p) = cur {
            if let Ok(mut pos) = view_areas.get_mut(p) {
                pos.y = 0.0;
                break;
            }
            cur = child_of_q.get(p).ok().map(|c| c.0);
        }

        let Some(session) = sessions.get(selected) else {
            continue;
        };

        commands.entity(view_e).with_children(|view| {
            let mut last_date = String::new();
            for m in &session.messages {
                if m.date != last_date {
                    last_date = m.date.clone();
                    view.spawn((Node {
                        width: Val::Percent(100.0),
                        justify_content: JustifyContent::Center,
                        padding: UiRect::vertical(Val::Px(6.0)),
                        ..default()
                    },))
                    .with_children(|drow| {
                        drow.spawn((
                            Text::new(m.date.clone()),
                            TextFont {
                                font: FontSource::Handle(font.clone()),
                                font_size: FontSize::Px(12.0),
                                ..default()
                            },
                            TextColor(TEXT_DIM),
                        ));
                    });
                }

                match m.kind {
                    MsgKind::System => {
                        view.spawn((Node {
                            width: Val::Percent(100.0),
                            justify_content: JustifyContent::Center,
                            padding: UiRect::vertical(Val::Px(4.0)),
                            column_gap: Val::Px(6.0),
                            ..default()
                        },))
                        .with_children(|sys| {
                            sys.spawn((
                                Text::new(format!("⊘ {}", m.lines.join(""))),
                                TextFont {
                                    font: FontSource::Handle(font.clone()),
                                    font_size: FontSize::Px(12.0),
                                    ..default()
                                },
                                TextColor(TEXT_DIM),
                            ));
                        });
                    }
                    MsgKind::Attachment => {
                        view.spawn((Node {
                            width: Val::Percent(100.0),
                            justify_content: JustifyContent::FlexStart,
                            padding: UiRect::vertical(Val::Px(2.0)),
                            ..default()
                        },))
                        .with_children(|arow| {
                            arow.spawn((Node {
                                column_gap: Val::Px(10.0),
                                align_items: AlignItems::Center,
                                padding: UiRect::all(Val::Px(10.0)),
                                border_radius: BorderRadius::all(Val::Px(8.0)),
                                ..default()
                            }, BackgroundColor(CARD_BG)))
                            .with_children(|card| {
                                card.spawn((
                                    Text::new("📄"),
                                    TextFont {
                                        font: FontSource::Handle(font.clone()),
                                        font_size: FontSize::Px(16.0),
                                        ..default()
                                    },
                                    TextColor(TEXT_MAIN),
                                ));
                                card.spawn((
                                    Text::new(m.lines.join("")),
                                    TextFont {
                                        font: FontSource::Handle(font.clone()),
                                        font_size: FontSize::Px(14.0),
                                        ..default()
                                    },
                                    TextColor(TEXT_MAIN),
                                ));
                                card.spawn((
                                    Text::new("✓"),
                                    TextFont {
                                        font: FontSource::Handle(font.clone()),
                                        font_size: FontSize::Px(14.0),
                                        ..default()
                                    },
                                    TextColor(ACCENT),
                                ));
                            });
                        });
                    }
                    MsgKind::Me | MsgKind::Other | MsgKind::Image => {
                        let (justify, bubble_bg, text_color) = if m.kind == MsgKind::Me {
                            (
                                JustifyContent::FlexEnd,
                                BUBBLE_ME,
                                TEXT_MAIN,
                            )
                        } else {
                            (
                                JustifyContent::FlexStart,
                                BUBBLE_OTHER,
                                TEXT_MAIN,
                            )
                        };
                        let color = if m.kind == MsgKind::Image {
                            TEXT_DIM
                        } else {
                            text_color
                        };
                        let time_suffix = m.time.clone();

                        view.spawn((Node {
                            width: Val::Percent(100.0),
                            justify_content: justify,
                            padding: UiRect::vertical(Val::Px(2.0)),
                            ..default()
                        },))
                        .with_children(|brow| {
                            brow.spawn((Node {
                                max_width: Val::Percent(78.0),
                                flex_direction: FlexDirection::Column,
                                row_gap: Val::Px(2.0),
                                padding: UiRect::all(Val::Px(10.0)),
                                border_radius: BorderRadius {
                                    top_left: Val::Px(10.0),
                                    top_right: Val::Px(10.0),
                                    bottom_left: Val::Px(4.0),
                                    bottom_right: Val::Px(10.0),
                                },
                                ..default()
                            }, BackgroundColor(bubble_bg)))
                            .with_children(|bubble| {
                                spawn_text_lines(bubble, &m.lines, color, 14.0, &font);
                                bubble.spawn((Node {
                                    width: Val::Percent(100.0),
                                    justify_content: JustifyContent::FlexEnd,
                                    ..default()
                                },))
                                .with_children(|t| {
                                    t.spawn((
                                        Text::new(time_suffix),
                                        TextFont {
                                            font: FontSource::Handle(font.clone()),
                                            font_size: FontSize::Px(10.0),
                                            ..default()
                                        },
                                        TextColor(TEXT_DIM),
                                    ));
                                });
                            });
                        });
                    }
                }
            }
        });
    }
}
