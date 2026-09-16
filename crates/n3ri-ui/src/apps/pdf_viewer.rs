use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use crate::font::N3riFonts;
use crate::scroll::{ScrollableArea, ScrollContent};

const TEXT_LIGHT: Color = Color::srgb(0.86, 0.93, 0.93);
const TEXT_DIM: Color = Color::srgba(0.6, 0.7, 0.8, 0.75);

const BASE_DPI: u32 = 150;
const MAX_DPI: u32 = 300;

#[derive(Component)]
struct PdfDoc {
    bytes: Vec<u8>,
    pages: usize,
    dpi: u32,
}

#[derive(Component)]
struct PdfPage {
    doc: Entity,
    index: usize,
    handle: Handle<Image>,
}

#[derive(Component)]
struct PdfReraster {
    dpi: u32,
    next: usize,
}

pub struct PdfViewerPlugin;

impl Plugin for PdfViewerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (pdf_watch_layout, pdf_reraster_worker).chain());
    }
}

fn desired_dpi(node: &Node, maximized: bool, scale: f32) -> u32 {
    if !maximized {
        return BASE_DPI;
    }
    let logical_w = match node.width {
        Val::Px(w) => w,
        _ => 620.0,
    };
    let content_w = (logical_w - 36.0).max(320.0);
    ((content_w * scale / 595.0 * 72.0 * 1.2).ceil() as u32).clamp(BASE_DPI, MAX_DPI)
}

fn pdf_watch_layout(
    mut commands: Commands,
    docs: Query<(
        Entity,
        &Node,
        &PdfDoc,
        Option<&PdfReraster>,
        Has<crate::window::WindowOriginalLayout>,
    )>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let scale = windows.single().map(|w| w.scale_factor()).unwrap_or(1.0);
    for (e, node, doc, queue, maximized) in docs.iter() {
        let want = desired_dpi(node, maximized, scale.max(0.5));
        let current = queue.map(|q| q.dpi).unwrap_or(doc.dpi);
        if want.abs_diff(current) >= 30 {
            commands.entity(e).insert(PdfReraster { dpi: want, next: 0 });
        }
    }
}

fn pdf_reraster_worker(
    mut commands: Commands,
    mut docs: Query<(Entity, &mut PdfDoc, &mut PdfReraster)>,
    pages: Query<&PdfPage>,
    mut images: ResMut<Assets<Image>>,
) {
    for (doc_e, mut doc, mut job) in docs.iter_mut() {
        if job.next >= doc.pages {
            let dpi = job.dpi;
            doc.dpi = dpi;
            commands.entity(doc_e).remove::<PdfReraster>();
            continue;
        }
        let idx = job.next;
        let Some(page) = pages.iter().find(|p| p.doc == doc_e && p.index == idx) else {
            job.next += 1;
            continue;
        };
        match bevy_document_extend::rasterize_page_to_bevy(&doc.bytes, idx, job.dpi) {
            Ok(new_img) => {
                if let Some(mut slot) = images.get_mut(&page.handle) {
                    *slot = new_img;
                }
                job.next += 1;
                if job.next >= doc.pages {
                    let dpi = job.dpi;
                    doc.dpi = dpi;
                    commands.entity(doc_e).remove::<PdfReraster>();
                }
            }
            Err(_) => {
                commands.entity(doc_e).remove::<PdfReraster>();
            }
        }
    }
}

pub fn spawn_pdf_viewer_direct(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    file_path: &str,
    fonts: &N3riFonts,
) -> bool {
    let Some(bytes) = crate::content::read_bytes(file_path) else {
        return false;
    };
    let meta = match bevy_document_extend::probe(&bytes) {
        Ok(m) => m,
        Err(_) => return false,
    };
    if meta.pages == 0 {
        return false;
    }
    let mut handles = Vec::with_capacity(meta.pages);
    let mut ratios = Vec::with_capacity(meta.pages);
    for page in 0..meta.pages {
        match bevy_document_extend::rasterize_page_to_bevy(&bytes, page, BASE_DPI) {
            Ok(img) => {
                ratios.push(img.width() as f32 / img.height().max(1) as f32);
                handles.push(images.add(img));
            }
            Err(_) => return false,
        }
    }

    let file_name = file_path.rsplit('/').next().unwrap_or("文档");
    let title = format!("{} - PDF文档（{}页）", file_name, meta.pages);
    let title_clone = title.clone();
    let fonts_handle = fonts.default.clone();

    let pages_total = meta.pages;
    let doc_e = commands.spawn_empty().id();
    commands.entity(doc_e).insert((
            PdfDoc {
                bytes,
                pages: meta.pages,
                dpi: BASE_DPI,
            },
            crate::window::AppWindow {
                title,
                app_id: "pdf_viewer".to_string(),
                z: 1,
            },
            crate::dock::AppVisible(true),
            GlobalZIndex(1),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(620.0),
                height: Val::Px(640.0),
                flex_direction: FlexDirection::Column,
                top: Val::Px(80.0),
                left: Val::Px(600.0),
                border_radius: BorderRadius::all(Val::Px(10.0)),
                overflow: Overflow::hidden(),
                ..default()
            },
            BackgroundColor(Color::srgba(0.08, 0.12, 0.2, 0.95)),
        ));
    commands.entity(doc_e)
        .with_children(|window| {
            window
                .spawn((
                    crate::window::TitleBar,
                    Button,
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(32.0),
                        padding: UiRect {
                            left: Val::Px(12.0),
                            right: Val::Px(12.0),
                            ..default()
                        },
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::SpaceBetween,
                        display: Display::Flex,
                        border_radius: BorderRadius {
                            top_left: Val::Px(10.0),
                            top_right: Val::Px(10.0),
                            bottom_left: Val::Px(0.0),
                            bottom_right: Val::Px(0.0),
                        },
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.05, 0.08, 0.14, 0.98)),
                ))
                .with_children(|title_bar| {
                    title_bar
                        .spawn((Node {
                            align_items: AlignItems::Center,
                            column_gap: Val::Px(8.0),
                            display: Display::Flex,
                            ..default()
                        },))
                        .with_children(|buttons| {
                            buttons.spawn((
                                crate::window::CloseButton,
                                Button,
                                Node {
                                    width: Val::Px(12.0),
                                    height: Val::Px(12.0),
                                    border: UiRect::all(Val::Px(1.0)),
                                    border_radius: BorderRadius::all(Val::Px(6.0)),
                                    ..default()
                                },
                                BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.2)),
                                BackgroundColor(Color::srgb(1.0, 0.37, 0.34)),
                            ));
                            buttons.spawn((
                                crate::window::MaximizeButton,
                                Button,
                                Node {
                                    width: Val::Px(12.0),
                                    height: Val::Px(12.0),
                                    border: UiRect::all(Val::Px(1.0)),
                                    border_radius: BorderRadius::all(Val::Px(6.0)),
                                    ..default()
                                },
                                BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.2)),
                                BackgroundColor(Color::srgb(0.16, 0.78, 0.25)),
                            ));
                        });

                    title_bar.spawn((
                        Text::new(title_clone),
                        TextFont {
                            font: FontSource::Handle(fonts_handle.clone()),
                            font_size: FontSize::Px(13.0),
                            ..default()
                        },
                        TextColor(TEXT_LIGHT),
                    ));

                    title_bar.spawn(Node {
                        width: Val::Px(60.0),
                        height: Val::Px(1.0),
                        ..default()
                    });
                });

            let area_e = window
                .spawn((
                    ScrollableArea,
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
                        padding: UiRect::all(Val::Px(12.0)),
                        row_gap: Val::Px(10.0),
                        ..default()
                    },
                ))
                .with_children(|content| {
                    for (i, handle) in handles.into_iter().enumerate() {
                        let ratio = ratios.get(i).copied();
                        let handle_clone = handle.clone();
                        content.spawn((
                            Text::new(format!("第 {} / {} 页", i + 1, pages_total)),
                            TextFont {
                                font: FontSource::Handle(fonts_handle.clone()),
                                font_size: FontSize::Px(11.0),
                                ..default()
                            },
                            TextColor(TEXT_DIM),
                        ));
                        content.spawn((
                            PdfPage {
                                doc: doc_e,
                                index: i,
                                handle: handle_clone.clone(),
                            },
                            ImageNode {
                                image: handle_clone,
                                ..default()
                            },
                            Node {
                                width: Val::Percent(100.0),
                                min_height: Val::Px(0.0),
                                aspect_ratio: ratio,
                                ..default()
                            },
                        ));
                    }
                });
            });
        });
    true
}
