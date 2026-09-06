use bevy::prelude::*;
use serde::Deserialize;
use crate::font::N3riFonts;
use crate::scroll::{ScrollableArea, ScrollContent};
use crate::window::spawn_window;

const FOLDERS: [&str; 3] = ["收件箱", "已发送", "已归档"];

const SIDEBAR_BG: Color = Color::srgb(0.05, 0.08, 0.13);
const LIST_HL: Color = Color::srgba(0.16, 0.25, 0.33, 0.85);
const TEXT_MAIN: Color = Color::srgb(0.86, 0.93, 0.93);
const TEXT_DIM: Color = Color::srgb(0.45, 0.55, 0.62);
const ACCENT: Color = Color::srgb(0.45, 0.76, 0.83);
const HR: Color = Color::srgba(0.5, 0.6, 0.7, 0.25);

#[derive(Deserialize)]
struct MailAttachment {
    #[serde(default)]
    filename: String,
    #[serde(default)]
    size: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    #[allow(dead_code)]
    path: String,
}

#[derive(Deserialize)]
struct MailJson {
    subject: String,
    from: String,
    to: String,
    date: String,
    body: String,
    #[serde(default)]
    attachments: Vec<MailAttachment>,
}

type Span = (String, Option<Color>);

struct MailEntry {
    mail: MailJson,
    lines: Vec<Span>,
}

#[derive(Resource, Default)]
struct MailState {
    folder: usize,
    selected: usize,
}

#[derive(Component)]
struct MailFolderBtn(usize);

#[derive(Component)]
struct MailItemBtn {
    index: usize,
}

#[derive(Component)]
struct MailListContent;

#[derive(Component)]
struct MailViewContent;

#[derive(Component)]
struct MailFolderCount(usize);

#[derive(Component)]
struct MailNeedsSync;

pub struct MailPlugin;

impl Plugin for MailPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MailState>().add_systems(
            Update,
            (mail_folder_click, mail_item_click, mail_sync_ui),
        );
    }
}

fn parse_rich(body: &str) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    let mut rest = body;
    while let Some(pos) = rest.find("[RGB(") {
        if pos > 0 {
            out.push((rest[..pos].to_string(), None));
        }
        let after = &rest[pos + 5..];
        let end_spec = after
            .find(")]")
            .map(|p| (p, 2))
            .or_else(|| after.find(']').map(|p| (p, 1)));
        let Some((sp, skip)) = end_spec else {
            break;
        };
        let rgb: Vec<f32> = after[..sp]
            .split(',')
            .filter_map(|s| s.trim().parse::<f32>().ok())
            .collect();
        if rgb.len() < 3 {
            break;
        }
        let color = Color::srgb(rgb[0] / 255.0, rgb[1] / 255.0, rgb[2] / 255.0);
        let after_spec = &after[sp + skip..];
        match after_spec.find("[/RGB]") {
            Some(ep) => {
                out.push((after_spec[..ep].to_string(), Some(color)));
                rest = &after_spec[ep + 6..];
            }
            None => {
                out.push((after_spec.to_string(), Some(color)));
                rest = "";
            }
        }
    }
    if !rest.is_empty() {
        out.push((rest.to_string(), None));
    }
    out
}

fn split_from(from: &str) -> (String, Option<String>) {
    if let Some(i) = from.find(" <") {
        (
            from[..i].to_string(),
            Some(from[i + 2..].trim_end_matches('>').to_string()),
        )
    } else {
        (from.to_string(), None)
    }
}

fn list_time(date: &str) -> String {
    date.split_whitespace()
        .last()
        .unwrap_or(date)
        .to_string()
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

fn date_key(date: &str) -> (u32, u32, u32, u32) {
    let nums: Vec<u32> = date
        .split(|c: char| !c.is_ascii_digit())
        .filter_map(|s| s.parse().ok())
        .collect();
    match nums.as_slice() {
        [m, d, h, min] => (*m, *d, *h, *min),
        _ => (0, 0, 0, 0),
    }
}

fn read_folder(folder: usize) -> Vec<MailEntry> {
    let dir_rel = format!("nori/app-icons/mail/{}", FOLDERS[folder]);
    let mut names: Vec<String> = crate::content::list_dir(&dir_rel)
        .into_iter()
        .filter(|(n, is_dir)| !is_dir && n.ends_with(".json"))
        .map(|(n, _)| n)
        .collect();
    names.sort();

    let mut out = Vec::new();
    for name in names {
        let Some(raw) = crate::content::read_to_string(&format!("{dir_rel}/{name}")) else {
            continue;
        };
        let Ok(mail) = serde_json::from_str::<MailJson>(&raw) else {
            continue;
        };
        let spans = parse_rich(&mail.body);
        let mut lines = Vec::new();
        for (text, color) in spans {
            for part in text.split('\n') {
                for chunk in wrap_line(part, 80) {
                    lines.push((chunk, color));
                }
            }
        }
        out.push(MailEntry { mail, lines });
    }
    out.sort_by(|a, b| date_key(&b.mail.date).cmp(&date_key(&a.mail.date)));
    out
}

pub fn spawn_mail(parent: &mut ChildSpawnerCommands, fonts: &N3riFonts) {
    let window_entity = spawn_window(parent, "邮件", "mail", 1000.0, 680.0, fonts);
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
                        flex_direction: FlexDirection::Row,
                        min_height: Val::Px(0.0),
                        ..default()
                    },
                ))
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
                                    Text::new("收件箱"),
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

                        for (i, folder) in FOLDERS.iter().enumerate() {
                            sidebar
                                .spawn((
                                    MailFolderBtn(i),
                                    Button,
                                    Node {
                                        width: Val::Percent(100.0),
                                        height: Val::Px(36.0),
                                        align_items: AlignItems::Center,
                                        justify_content: JustifyContent::SpaceBetween,
                                        padding: UiRect::horizontal(Val::Px(10.0)),
                                        border_radius: BorderRadius::all(Val::Px(6.0)),
                                        column_gap: Val::Px(8.0),
                                        ..default()
                                    },
                                    BackgroundColor(Color::NONE),
                                ))
                                .with_children(|fb| {
                                    fb.spawn((
                                        Text::new((*folder).to_string()),
                                        TextFont {
                                            font: FontSource::Handle(font.clone()),
                                            font_size: FontSize::Px(15.0),
                                            ..default()
                                        },
                                        TextColor(TEXT_MAIN),
                                    ));
                                    fb.spawn((
                                        MailFolderCount(i),
                                        Text::new("0"),
                                        TextFont {
                                            font: FontSource::Handle(font.clone()),
                                            font_size: FontSize::Px(13.0),
                                            ..default()
                                        },
                                        TextColor(TEXT_DIM),
                                    ));
                                });
                        }

                        let area_e = sidebar
                            .spawn((
                                ScrollableArea,
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
                                MailListContent,
                                MailNeedsSync,
                                Node {
                                    width: Val::Percent(100.0),
                                    min_height: Val::Px(0.0),
                                    flex_direction: FlexDirection::Column,
                                    row_gap: Val::Px(4.0),
                                    ..default()
                                },
                            ));
                        });
                    });

                    let view_e = row
                        .spawn((
                            Node {
                                flex_grow: 1.0,
                                height: Val::Percent(100.0),
                                min_height: Val::Px(0.0),
                                min_width: Val::Px(0.0),
                                ..default()
                            },
                        ))
                        .id();

                    row.commands().entity(view_e).with_children(|v| {
                        let area_e = v
                            .spawn((
                                ScrollableArea,
                                Node {
                                    width: Val::Percent(100.0),
                                    height: Val::Percent(100.0),
                                    min_height: Val::Px(0.0),
                                    align_items: AlignItems::FlexStart,
                                    overflow: Overflow::hidden(),
                                    ..default()
                                },
                            ))
                            .id();

                        v.commands().entity(area_e).with_children(|a| {
                            crate::scroll::spawn_scrollbar(a, area_e);
                        });

                        v.commands().entity(area_e).with_children(|area| {
                            area.spawn((
                                ScrollContent,
                                MailViewContent,
                                MailNeedsSync,
                                Node {
                                    width: Val::Percent(100.0),
                                    min_height: Val::Px(0.0),
                                    flex_direction: FlexDirection::Column,
                                    padding: UiRect {
                                        left: Val::Px(28.0),
                                        right: Val::Px(28.0),
                                        top: Val::Px(24.0),
                                        bottom: Val::Px(24.0),
                                    },
                                    row_gap: Val::Px(10.0),
                                    ..default()
                                },
                            ));
                        });
                    });
                });
        });
}

fn spawn_line(
    parent: &mut ChildSpawnerCommands,
    text: &str,
    color: Color,
    size: f32,
    font: &Handle<Font>,
) {
    parent.spawn((
        Text::new(text.to_string()),
        TextFont {
            font: FontSource::Handle(font.clone()),
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
    ));
}

fn mail_folder_click(
    interaction_q: Query<(&Interaction, &MailFolderBtn), With<Button>>,
    mut state: ResMut<MailState>,
) {
    for (interaction, fb) in interaction_q.iter() {
        if *interaction == Interaction::Pressed && state.folder != fb.0 {
            state.folder = fb.0;
            state.selected = 0;
        }
    }
}

fn mail_item_click(
    interaction_q: Query<(&Interaction, &MailItemBtn), With<Button>>,
    mut state: ResMut<MailState>,
) {
    for (interaction, ib) in interaction_q.iter() {
        if *interaction == Interaction::Pressed && state.selected != ib.index {
            state.selected = ib.index;
        }
    }
}

fn mail_sync_ui(
    state: Res<MailState>,
    list_q: Query<Entity, With<MailListContent>>,
    view_q: Query<Entity, With<MailViewContent>>,
    needs_q: Query<Entity, With<MailNeedsSync>>,
    mut count_q: Query<(&MailFolderCount, &mut Text)>,
    child_of_q: Query<&bevy::prelude::ChildOf>,
    mut areas: Query<&mut ScrollPosition, With<ScrollableArea>>,
    mut commands: Commands,
    mut sync_count: Local<u32>,
    fonts: Res<N3riFonts>,
) {
    if !state.is_changed() && needs_q.is_empty() {
        return;
    }
    *sync_count += 1;
    let t0 = std::time::Instant::now();
    debug!("[mailsync] trigger changed={} needs={}", state.is_changed(), needs_q.iter().count());
    for e in needs_q.iter() {
        commands.entity(e).remove::<MailNeedsSync>();
    }
    let font = fonts.default.clone();

    let folder_mails: Vec<Vec<MailEntry>> = (0..FOLDERS.len())
        .map(read_folder)
        .collect();

    for (fc, mut text) in count_q.iter_mut() {
        **text = folder_mails[fc.0].len().to_string();
    }

    let mails = folder_mails
        .get(state.folder)
        .map(|v| v.as_slice())
        .unwrap_or(&[]);
    let selected = state.selected.min(mails.len().saturating_sub(1));

    for list_e in list_q.iter() {
        commands.entity(list_e).despawn_children();
        let font = font.clone();
        commands.entity(list_e).with_children(move |list| {
            for (i, entry) in mails.iter().enumerate() {
                let (name, addr) = split_from(&entry.mail.from);
                let bg = if i == selected { LIST_HL } else { Color::NONE };
                list.spawn((
                    MailItemBtn { index: i },
                    Button,
                    Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(3.0),
                        padding: UiRect::all(Val::Px(10.0)),
                        border_radius: BorderRadius::all(Val::Px(6.0)),
                        ..default()
                    },
                    BackgroundColor(bg),
                ))
                .with_children(|item| {
                    item.spawn((Node {
                        width: Val::Percent(100.0),
                        justify_content: JustifyContent::SpaceBetween,
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(8.0),
                        ..default()
                    },))
                    .with_children(|l1| {
                        l1.spawn((
                            Text::new(name),
                            TextFont {
                                font: FontSource::Handle(font.clone()),
                                font_size: FontSize::Px(15.0),
                                ..default()
                            },
                            TextColor(if i == selected { TEXT_MAIN } else { ACCENT }),
                        ));
                        l1.spawn((
                            Text::new(list_time(&entry.mail.date)),
                            TextFont {
                                font: FontSource::Handle(font.clone()),
                                font_size: FontSize::Px(12.0),
                                ..default()
                            },
                            TextColor(TEXT_DIM),
                        ));
                    });
                    item.spawn((
                        Text::new(entry.mail.subject.clone()),
                        TextFont {
                            font: FontSource::Handle(font.clone()),
                            font_size: FontSize::Px(14.0),
                            ..default()
                        },
                        TextColor(TEXT_MAIN),
                        Node {
                            width: Val::Percent(100.0),
                            ..default()
                        },
                    ));
                    item.spawn((
                        Text::new(addr.unwrap_or_default()),
                        TextFont {
                            font: FontSource::Handle(font.clone()),
                            font_size: FontSize::Px(12.0),
                            ..default()
                        },
                        TextColor(TEXT_DIM),
                    ));
                });
            }
        });
    }

    for view_e in view_q.iter() {
        commands.entity(view_e).despawn_children();

        let mut cur = child_of_q.get(view_e).ok().map(|c| c.0);
        while let Some(p) = cur {
            if let Ok(mut pos) = areas.get_mut(p) {
                pos.y = 0.0;
                break;
            }
            cur = child_of_q.get(p).ok().map(|c| c.0);
        }

        let Some(entry) = mails.get(selected) else {
            continue;
        };
        let (name, addr) = split_from(&entry.mail.from);
        let mail = &entry.mail;

        commands.entity(view_e).with_children(|view| {
            view.spawn((
                Text::new(mail.subject.clone()),
                TextFont {
                    font: FontSource::Handle(font.clone()),
                    font_size: FontSize::Px(24.0),
                    ..default()
                },
                TextColor(TEXT_MAIN),
                Node {
                    width: Val::Percent(100.0),
                    ..default()
                },
            ));

            view.spawn((Node {
                width: Val::Percent(100.0),
                column_gap: Val::Px(14.0),
                align_items: AlignItems::FlexStart,
                margin: UiRect::top(Val::Px(6.0)),
                ..default()
            },))
            .with_children(|head| {
                head.spawn((
                    Node {
                        width: Val::Px(42.0),
                        height: Val::Px(42.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border_radius: BorderRadius::all(Val::Px(21.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.2, 0.35, 0.42, 0.9)),
                ))
                .with_children(|avatar| {
                    avatar.spawn((
                        Text::new(
                            name.chars()
                                .next()
                                .map(|c| c.to_string())
                                .unwrap_or_else(|| "?".to_string()),
                        ),
                        TextFont {
                            font: FontSource::Handle(font.clone()),
                            font_size: FontSize::Px(18.0),
                            ..default()
                        },
                        TextColor(ACCENT),
                    ));
                });

                head.spawn((Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(2.0),
                    ..default()
                },))
                .with_children(|meta| {
                    meta.spawn((Node {
                        column_gap: Val::Px(8.0),
                        align_items: AlignItems::Center,
                        ..default()
                    },))
                    .with_children(|l| {
                        l.spawn((
                            Text::new(name),
                            TextFont {
                                font: FontSource::Handle(font.clone()),
                                font_size: FontSize::Px(15.0),
                                ..default()
                            },
                            TextColor(TEXT_MAIN),
                        ));
                        if let Some(a) = &addr {
                            l.spawn((
                                Text::new(format!("<{}>", a)),
                                TextFont {
                                    font: FontSource::Handle(font.clone()),
                                    font_size: FontSize::Px(14.0),
                                    ..default()
                                },
                                TextColor(ACCENT),
                            ));
                        }
                    });
                    meta.spawn((
                        Text::new(format!("收件人: {}", mail.to)),
                        TextFont {
                            font: FontSource::Handle(font.clone()),
                            font_size: FontSize::Px(13.0),
                            ..default()
                        },
                        TextColor(TEXT_DIM),
                    ));
                    meta.spawn((
                        Text::new(mail.date.clone()),
                        TextFont {
                            font: FontSource::Handle(font.clone()),
                            font_size: FontSize::Px(13.0),
                            ..default()
                        },
                        TextColor(TEXT_DIM),
                    ));
                });
            });

            view.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(1.0),
                    margin: UiRect::vertical(Val::Px(8.0)),
                    ..default()
                },
                BackgroundColor(HR),
            ));

            for (line, color) in &entry.lines {
                let c = color.unwrap_or(TEXT_MAIN);
                if line.is_empty() {
                    view.spawn((Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(12.0),
                        ..default()
                    },));
                } else {
                    spawn_line(view, line, c, 15.0, &font);
                }
            }

            for att in &mail.attachments {
                if att.filename.is_empty() {
                    continue;
                }
                view.spawn((Node {
                    width: Val::Percent(100.0),
                    column_gap: Val::Px(10.0),
                    align_items: AlignItems::Center,
                    padding: UiRect::all(Val::Px(8.0)),
                    margin: UiRect::top(Val::Px(6.0)),
                    border_radius: BorderRadius::all(Val::Px(6.0)),
                    ..default()
                }, BackgroundColor(Color::srgba(0.12, 0.18, 0.25, 0.6))))
                .with_children(|arow| {
                    arow.spawn((
                        Text::new("📎"),
                        TextFont {
                            font: FontSource::Handle(font.clone()),
                            font_size: FontSize::Px(14.0),
                            ..default()
                        },
                        TextColor(TEXT_DIM),
                    ));
                    arow.spawn((
                        Text::new(att.filename.clone()),
                        TextFont {
                            font: FontSource::Handle(font.clone()),
                            font_size: FontSize::Px(14.0),
                            ..default()
                        },
                        TextColor(TEXT_MAIN),
                    ));
                    let extra = [att.size.as_str(), att.status.as_str()]
                        .iter()
                        .filter(|s| !s.is_empty())
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(" · ");
                    if !extra.is_empty() {
                        arow.spawn((
                            Text::new(extra),
                            TextFont {
                                font: FontSource::Handle(font.clone()),
                                font_size: FontSize::Px(12.0),
                                ..default()
                            },
                            TextColor(TEXT_DIM),
                        ));
                    }
                });
            }
        });
    }
    debug!("[mailsync] done in {:?}", t0.elapsed());
}
