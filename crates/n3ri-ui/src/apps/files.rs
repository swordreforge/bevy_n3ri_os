use bevy::prelude::*;
use crate::font::N3riFonts;
use crate::scroll::{ScrollableArea, ScrollContent};
use crate::window::spawn_window;
use std::fs;

const TEXT_MAIN: Color = Color::srgb(0.86, 0.93, 0.93);
const TEXT_DIM: Color = Color::srgba(0.6, 0.7, 0.8, 0.75);
const SIDEBAR_BG: Color = Color::srgba(0.06, 0.09, 0.15, 0.95);
const CONTENT_BG: Color = Color::srgba(0.08, 0.12, 0.18, 0.95);
const TOOLBAR_BG: Color = Color::srgba(0.04, 0.07, 0.12, 0.95);
const STATUS_BG: Color = Color::srgba(0.04, 0.07, 0.12, 0.95);

#[derive(Component)]
struct FilesWindow;

#[derive(Component)]
struct FilesSidebar;

#[derive(Component)]
struct FilesContent;

#[derive(Component)]
struct FilesStatusBar;

#[derive(Component)]
struct FilesToolbar;

#[derive(Component)]
struct BackButton;

#[derive(Component)]
struct PathText;

#[derive(Component)]
struct SidebarItem(SidebarNav);

#[derive(Component)]
pub(crate) struct FileItem {
    name: String,
    is_dir: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum SidebarNav {
    Home,
    Documents,
    Downloads,
    RsrchColdVol,
}

const SIDEBAR_ITEMS: &[(SidebarNav, &str, &str)] = &[
    (SidebarNav::Home, "🏠", "本机"),
    (SidebarNav::Documents, "📄", "文稿"),
    (SidebarNav::Downloads, "📥", "下载"),
    (SidebarNav::RsrchColdVol, "📁", "RSRCH-COLD-VOL"),
];

#[derive(Resource)]
struct FilesState {
    current_path: String,
    history: Vec<String>,
}

impl Default for FilesState {
    fn default() -> Self {
        Self {
            current_path: "本机".into(),
            history: Vec::new(),
        }
    }
}

pub struct FilesPlugin;

impl Plugin for FilesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FilesState>()
            .add_systems(Update, (files_sidebar_click, files_back_click, files_item_click, files_sync_ui));
    }
}

pub fn spawn_files(parent: &mut ChildSpawnerCommands, fonts: &N3riFonts) {
    let window_entity = spawn_window(parent, "文件", "files", 800.0, 550.0, fonts);

    parent
        .commands()
        .entity(window_entity)
        .with_children(|window| {
            window
                .spawn((
                    FilesWindow,
                    Node {
                        width: Val::Percent(100.0),
                        flex_grow: 1.0,
                        min_height: Val::Px(0.0),
                        flex_direction: FlexDirection::Row,
                        ..default()
                    },
                ))
                .with_children(|main| {
                    spawn_sidebar(main, fonts);
                    spawn_right_area(main, fonts);
                });
        });
}

fn spawn_sidebar(parent: &mut ChildSpawnerCommands, fonts: &N3riFonts) {
    parent
        .spawn((
            FilesSidebar,
            Node {
                width: Val::Px(160.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(8.0)),
                ..default()
            },
            BackgroundColor(SIDEBAR_BG),
        ))
        .with_children(|sidebar| {
            for (nav, icon, label) in SIDEBAR_ITEMS {
                sidebar
                    .spawn((
                        SidebarItem(*nav),
                        Button,
                        Node {
                            width: Val::Percent(100.0),
                            height: Val::Px(32.0),
                            align_items: AlignItems::Center,
                            padding: UiRect::left(Val::Px(8.0)),
                            border_radius: BorderRadius::all(Val::Px(4.0)),
                            ..default()
                        },
                        BackgroundColor(Color::NONE),
                    ))
                    .with_children(|item| {
                        item.spawn((
                            Text::new(format!("{icon} {label}")),
                            TextFont {
                                font: FontSource::Handle(fonts.default.clone()),
                                font_size: FontSize::Px(13.0),
                                ..default()
                            },
                            TextColor(TEXT_MAIN),
                        ));
                    });
            }
        });
}

fn spawn_right_area(parent: &mut ChildSpawnerCommands, fonts: &N3riFonts) {
    parent
        .spawn(Node {
            flex_grow: 1.0,
            height: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|right| {
            spawn_toolbar(right, fonts);
            spawn_content(right, fonts);
            spawn_status_bar(right, fonts);
        });
}

fn spawn_toolbar(parent: &mut ChildSpawnerCommands, fonts: &N3riFonts) {
    parent
        .spawn((
            FilesToolbar,
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(36.0),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(Val::Px(12.0)),
                column_gap: Val::Px(8.0),
                ..default()
            },
            BackgroundColor(TOOLBAR_BG),
        ))
        .with_children(|toolbar| {
            toolbar
                .spawn((
                    BackButton,
                    Button,
                    Text::new("←"),
                    TextFont {
                        font: FontSource::Handle(fonts.default.clone()),
                        font_size: FontSize::Px(16.0),
                        ..default()
                    },
                    TextColor(TEXT_DIM),
                    Node {
                        padding: UiRect::all(Val::Px(4.0)),
                        border_radius: BorderRadius::all(Val::Px(4.0)),
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                ));

            toolbar.spawn((
                PathText,
                Text::new("本机"),
                TextFont {
                    font: FontSource::Handle(fonts.default.clone()),
                    font_size: FontSize::Px(13.0),
                    ..default()
                },
                TextColor(TEXT_DIM),
                Node {
                    flex_grow: 1.0,
                    ..default()
                },
            ));
        });
}

fn spawn_content(parent: &mut ChildSpawnerCommands, fonts: &N3riFonts) {
    let area_e = parent
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

    parent.commands().entity(area_e).with_children(|a| {
        crate::scroll::spawn_scrollbar(a, area_e);
    });

    parent.commands().entity(area_e).with_children(|area| {
        area.spawn((
            FilesContent,
            ScrollContent,
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::all(Val::Px(12.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(4.0),
                ..default()
            },
            BackgroundColor(CONTENT_BG),
        ))
        .with_children(|content| {
            load_directory(content, fonts, "本机");
        });
    });
}

fn load_directory(parent: &mut ChildSpawnerCommands, fonts: &N3riFonts, path: &str) {
    let rel = format!("nori/app-icons/files/{}", path.trim_matches('/'));

    let mut entries: Vec<_> = crate::content::list_dir(&rel)
        .into_iter()
        .filter(|(name, _)| !name.ends_with(".pdf.png"))
        .collect();

    entries.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

    for (name, is_dir) in entries {
        let icon = if is_dir { "📁" } else { "📄" };
        parent
            .spawn((
                FileItem {
                    name: name.clone(),
                    is_dir,
                },
                Button,
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(28.0),
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(Val::Px(8.0)),
                    border_radius: BorderRadius::all(Val::Px(4.0)),
                    ..default()
                },
                BackgroundColor(Color::NONE),
            ))
            .with_children(|item| {
                item.spawn((
                    Text::new(format!("{icon} {name}")),
                    TextFont {
                        font: FontSource::Handle(fonts.default.clone()),
                        font_size: FontSize::Px(13.0),
                        ..default()
                    },
                    TextColor(TEXT_MAIN),
                ));
            });
    }
}

fn spawn_status_bar(parent: &mut ChildSpawnerCommands, fonts: &N3riFonts) {
    parent
        .spawn((
            FilesStatusBar,
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(24.0),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(Val::Px(12.0)),
                ..default()
            },
            BackgroundColor(STATUS_BG),
        ))
        .with_children(|bar| {
            bar.spawn((
                Text::new(""),
                TextFont {
                    font: FontSource::Handle(fonts.default.clone()),
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(TEXT_DIM),
            ));
        });
}

fn files_sidebar_click(
    mouse: Res<ButtonInput<MouseButton>>,
    mut state: ResMut<FilesState>,
    query: Query<(&Interaction, &SidebarItem), With<Button>>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for (interaction, item) in query.iter() {
        if *interaction == Interaction::Pressed {
            let path = match item.0 {
                SidebarNav::Home => "本机",
                SidebarNav::Documents => "本机/文稿",
                SidebarNav::Downloads => "本机/下载",
                SidebarNav::RsrchColdVol => "本机/RSRCH-COLD-VOL",
            };
            let current = state.current_path.clone();
            if current != path {
                state.history.push(current);
                state.current_path = path.to_string();
            }
        }
    }
}

fn files_back_click(
    mouse: Res<ButtonInput<MouseButton>>,
    mut state: ResMut<FilesState>,
    query: Query<&Interaction, With<BackButton>>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for interaction in query.iter() {
        if *interaction == Interaction::Pressed {
            if let Some(prev) = state.history.pop() {
                state.current_path = prev;
            }
        }
    }
}

fn files_item_click(
    mouse: Res<ButtonInput<MouseButton>>,
    mut state: ResMut<FilesState>,
    query: Query<(&Interaction, &FileItem), With<Button>>,
    mut commands: Commands,
    fonts: Res<N3riFonts>,
    asset_server: Res<AssetServer>,
    mut terminal: ResMut<crate::apps::terminal::TerminalState>,
    dock_parent_q: Query<&bevy::prelude::ChildOf, With<crate::dock::Dock>>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for (interaction, item) in query.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }

        if item.is_dir {
            let current = state.current_path.clone();
            let new_path = format!("{}/{}", current, item.name);
            state.history.push(current);
            state.current_path = new_path;
            continue;
        }

        let is_viewable = item.name.ends_with(".txt")
            || item.name.ends_with(".log")
            || item.name.ends_with(".yaml")
            || item.name.ends_with(".jpg")
            || item.name.ends_with(".jpeg")
            || item.name.ends_with(".png")
            || item.name.ends_with(".pdf")
            || !item.name.contains('.');
        if !is_viewable {
            continue;
        }

        let rel = format!(
            "nori/app-icons/files/{}/{}",
            state.current_path.trim_matches('/'),
            item.name
        );
        let fonts_data = N3riFonts {
            default: fonts.default.clone(),
            terminal: fonts.terminal.clone(),
            ui: fonts.ui.clone(),
            dock: fonts.dock.clone(),
        };
        if item.name.ends_with(".log") {
            crate::apps::log_viewer::spawn_log_viewer_direct(&mut commands, &rel, &fonts_data, &asset_server);
        } else if item.name.ends_with(".pdf") {
            let png_rel = format!("{rel}.png");
            if crate::content::exists(&png_rel) {
                crate::apps::image_viewer::spawn_image_viewer_direct(&mut commands, &png_rel, &fonts_data, &asset_server);
            } else {
                let tip = std::env::temp_dir().join("n3ri_pdf_preview_missing.txt");
                fs::write(&tip, "无法预览:此文档已损坏,或缺少预览数据。\n\n……这一份似乎和其余的报告不太一样。建议不要继续查阅。").ok();
                if let Some(tip_str) = tip.to_str() {
                    crate::apps::txt_reader::spawn_txt_reader_direct(&mut commands, &tip_str.to_string(), &fonts_data);
                }
            }
        } else if !item.name.contains('.') {
            // 模拟可执行文件：内容层取字节解压到临时目录执行
            // （嵌入发布下磁盘无此文件；CWD 无关，target/release 启动也能跑）
            let exec_rel = format!(
                "nori/app-icons/files/{}/{}",
                state.current_path.trim_matches('/'),
                item.name
            );
            let tmp_dir = std::env::temp_dir().join("n3ri_files_exec");
            let run = if let Some(bytes) = crate::content::read_bytes(&exec_rel) {
                let _ = std::fs::create_dir_all(&tmp_dir);
                let tmp_file = tmp_dir.join(&item.name);
                if std::fs::write(&tmp_file, &bytes).is_ok() {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let _ = std::fs::set_permissions(
                            &tmp_file,
                            std::fs::Permissions::from_mode(0o755),
                        );
                    }
                }
                format!("cd '{}' && './{}'", tmp_dir.to_string_lossy(), item.name)
            } else {
                // 磁盘兜底（内容层读不到时按原 CWD 逻辑走，shell 报错可见）
                let dir_abs = std::env::current_dir()
                    .unwrap_or_default()
                    .join("assets/nori/app-icons/files")
                    .join(&state.current_path);
                format!("cd '{}' && './{}'", dir_abs.to_string_lossy(), item.name)
            };
            terminal.request_run(&run);
            if let Ok(dock_parent) = dock_parent_q.single() {
                let root_e = dock_parent.0;
                commands.entity(root_e).with_children(|p| {
                    crate::apps::terminal::spawn_terminal(p, &fonts_data);
                });
            }
        } else if item.name.ends_with(".jpg") || item.name.ends_with(".jpeg") || item.name.ends_with(".png") {
            crate::apps::image_viewer::spawn_image_viewer_direct(&mut commands, &rel, &fonts_data, &asset_server);
        } else {
            crate::apps::txt_reader::spawn_txt_reader_direct(&mut commands, &rel, &fonts_data);
        }
    }
}

fn files_sync_ui(
    state: Res<FilesState>,
    mut path_text: Query<&mut Text, With<PathText>>,
    content_query: Query<Entity, With<FilesContent>>,
    mut commands: Commands,
    fonts: Res<N3riFonts>,
) {
    if state.is_changed() {
        if let Ok(mut text) = path_text.single_mut() {
            **text = state.current_path.clone();
        }

        for entity in content_query.iter() {
            commands.entity(entity).despawn_children();
            let path = state.current_path.clone();
            let fonts_data = N3riFonts {
                default: fonts.default.clone(),
                terminal: fonts.terminal.clone(),
                ui: fonts.ui.clone(),
                dock: fonts.dock.clone(),
            };
            commands.entity(entity).with_children(move |c| {
                load_directory(c, &fonts_data, &path);
            });
        }
    }
}
