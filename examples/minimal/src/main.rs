use bevy::audio::GlobalVolume;
use bevy::prelude::*;
use n3ri_core::prelude::*;
use n3ri_live2d::{
    HeadDisplay, Live2dPet,
    spawn_head_display, spawn_pet_display, HeadDisplayWanted, PetDisplayImage, PetDisplayNode,
    PetHeadImage,
};
use n3ri_ui::desktop::DesktopBackgroundMaterial;
use n3ri_ui::font::N3riFonts;
use n3ri_ui::window::AppWindow;
use n3ri_ui::N3riUiPlugin;
use n3ri_ui::chat_capsule::{ChatEmotionEvent, ChatRise};

mod focus;

const BG_DARK: Color = Color::srgb(0.02, 0.05, 0.1);
const CYAN: Color = Color::srgba(0.4, 0.95, 1.0, 1.0);
const TRACK_COLOR: Color = Color::srgba(0.15, 0.2, 0.25, 0.5);

#[derive(Component)]
struct BgmMusic;

fn main() {
    let mut app = App::new();

    // ReplaceDefault 必须先于 AssetPlugin 注册（之后添加会 panic），资源路径调用点零改动
    #[cfg(feature = "embed-assets")]
    app.add_plugins(bevy_embedded_assets::EmbeddedAssetPlugin {
        mode: bevy_embedded_assets::PluginMode::ReplaceDefault,
    });

    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "n3ri_os".into(),
                    resolution: (1920u32, 1080u32).into(),
                    ..default()
                }),
                ..default()
            })
            .set(AssetPlugin {
                file_path: "../../assets".into(),
                ..default()
            }),
    )
        .add_plugins(N3riCorePlugin::default())
        .add_plugins(N3riUiPlugin)
        .add_plugins(n3ri_live2d::N3riLive2dPlugin)
        .add_plugins(focus::FocusPlugin)
        .add_systems(Startup, spawn_camera)
        .add_systems(Update, (chat_rise_sync, chat_emotion_bridge))
        .add_systems(OnEnter(OsState::Boot), spawn_boot_screen)
        .add_systems(
            Update,
            update_boot_screen.run_if(in_state(OsState::Boot)),
        )
        .add_systems(OnEnter(OsState::Loading), spawn_loading_screen)
        .add_systems(
            Update,
            update_loading_screen.run_if(in_state(OsState::Loading)),
        )
        .add_systems(OnEnter(OsState::Desktop), (spawn_desktop_screen, start_bgm))
        .add_systems(Update, (toggle_head_display, update_bgm_volume))
        .run();
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}

// ── Phase 1: Boot ────────────────────────────────────────────────────

#[derive(Component)]
struct BootScreen;

#[derive(Component)]
struct BootBarFill;

fn spawn_boot_screen(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.spawn((
        BootScreen,
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            row_gap: Val::Px(40.0),
            ..default()
        },
        BackgroundColor(BG_DARK),
    ))
    .with_children(|parent| {
        // Logo
        parent.spawn((
            ImageNode::new(asset_server.load("nori/icon_00.png")),
            Node {
                width: Val::Px(120.0),
                height: Val::Px(120.0),
                ..default()
            },
        ));

        // Linear progress bar (track + fill)
        parent
            .spawn(Node {
                width: Val::Px(200.0),
                height: Val::Px(3.0),
                border_radius: BorderRadius::all(Val::Px(1.5)),
                overflow: Overflow::clip(),
                ..default()
            })
            .with_children(|bar| {
                bar.spawn((
                    BootBarFill,
                    Node {
                        width: Val::Px(0.0),
                        height: Val::Px(3.0),
                        ..default()
                    },
                    BackgroundColor(CYAN),
                ));
            });
    });
}

fn update_boot_screen(
    time: Res<Time>,
    mut boot_state: ResMut<BootState>,
    mut next_state: ResMut<NextState<OsState>>,
    mut fill_q: Query<&mut Node, With<BootBarFill>>,
) {
    boot_state.elapsed += time.delta_secs();
    boot_state.progress = (boot_state.elapsed / 2.0).min(1.0);

    for mut node in fill_q.iter_mut() {
        node.width = Val::Px(boot_state.progress * 200.0);
    }

    if boot_state.progress >= 1.0 {
        next_state.set(OsState::Loading);
    }
}

// ── Phase 2: Loading ─────────────────────────────────────────────────

#[derive(Component)]
struct LoadingScreen;

#[derive(Component)]
struct LoadingRing;

fn spawn_loading_screen(
    mut commands: Commands,
    query: Query<Entity, With<BootScreen>>,
    asset_server: Res<AssetServer>,
) {
    for entity in query.iter() {
        commands.entity(entity).despawn();
    }

    commands.spawn((
        LoadingScreen,
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            row_gap: Val::Px(40.0),
            ..default()
        },
        BackgroundColor(BG_DARK),
    ))
    .with_children(|parent| {
        // Flower icon
        parent.spawn((
            ImageNode::new(asset_server.load("nori/icon.png")),
            Node {
                width: Val::Px(120.0),
                height: Val::Px(120.0),
                ..default()
            },
        ));

        // Circular progress ring — single circle with per-side border colors
        parent.spawn((
            LoadingRing,
            Node {
                width: Val::Px(48.0),
                height: Val::Px(48.0),
                border_radius: BorderRadius::all(Val::Px(24.0)),
                border: UiRect::all(Val::Px(3.0)),
                ..default()
            },
            BorderColor::all(TRACK_COLOR),
        ));
    });
}

fn update_loading_screen(
    time: Res<Time>,
    mut load_state: ResMut<LoadState>,
    mut next_state: ResMut<NextState<OsState>>,
    mut ring_q: Query<&mut BorderColor, With<LoadingRing>>,
) {
    load_state.elapsed += time.delta_secs();
    load_state.progress = (load_state.elapsed / 2.0).min(1.0);

    let arc_center = (load_state.elapsed * 360.0) % 360.0;

    for mut border in ring_q.iter_mut() {
        border.top = arc_side_color(arc_center, 0.0);
        border.right = arc_side_color(arc_center, 90.0);
        border.bottom = arc_side_color(arc_center, 180.0);
        border.left = arc_side_color(arc_center, 270.0);
    }

    if load_state.progress >= 1.0 {
        next_state.set(OsState::Desktop);
    }
}

fn arc_side_color(arc_center: f32, side_center: f32) -> Color {
    let diff = angle_diff(arc_center, side_center).abs();
    // Arc is 90° wide, each side spans 90°. Coverage = overlap / 90°.
    let coverage = (1.0 - diff / 90.0).clamp(0.0, 1.0);
    lerp_color(TRACK_COLOR, CYAN, coverage)
}

fn angle_diff(from: f32, to: f32) -> f32 {
    let mut d = (to - from) % 360.0;
    if d > 180.0 { d -= 360.0; }
    if d < -180.0 { d += 360.0; }
    d
}

fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    let a = a.to_srgba();
    let b = b.to_srgba();
    Color::srgba(
        a.red + (b.red - a.red) * t,
        a.green + (b.green - a.green) * t,
        a.blue + (b.blue - a.blue) * t,
        1.0,
    )
}

// ── Desktop ──────────────────────────────────────────────────────────

#[derive(Component)]
struct DesktopScreen;

fn spawn_desktop_screen(
    mut commands: Commands,
    query: Query<Entity, With<LoadingScreen>>,
    asset_server: Res<AssetServer>,
    fonts: Res<N3riFonts>,
    pet_image: Res<PetDisplayImage>,
    pet_head: Res<PetHeadImage>,
    view_size: Res<n3ri_live2d::PetViewSize>,
    mut images: ResMut<Assets<Image>>,
    mut bg_materials: ResMut<Assets<DesktopBackgroundMaterial>>,
) {
    for entity in query.iter() {
        commands.entity(entity).despawn();
    }

    commands
        .spawn((
            DesktopScreen,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                display: Display::Flex,
                ..default()
            },
            BackgroundColor(BG_DARK),
        ))
        .with_children(|parent| {
            n3ri_ui::desktop::spawn_desktop_background(
                parent,
                &asset_server,
                &mut images,
                &mut bg_materials,
            );
            if let Some(image) = pet_image.0.clone() {
                spawn_pet_display(parent, &image, &view_size);
            }
            n3ri_ui::topbar::spawn_topbar(parent, &asset_server, &fonts);
            n3ri_ui::dock::spawn_dock(parent, &asset_server, &fonts);
            n3ri_ui::chat_capsule::spawn_chat_capsule(parent, &fonts);
            if let Some(head_image) = pet_head.0.clone() {
                spawn_head_display(parent, &head_image);
            }
        });
}

const OCCLUSION_THRESHOLD: f32 = 0.5;

fn toggle_head_display(
    pet: Query<(&ComputedNode, &UiGlobalTransform), With<PetDisplayNode>>,
    app_windows: Query<(&ComputedNode, &UiGlobalTransform, &Visibility), With<AppWindow>>,
    mut wanted: ResMut<HeadDisplayWanted>,
) {
    let occluded = match pet.single() {
        Ok((pet_node, pet_tf)) => {
            let pet_size = pet_node.size();
            let pet_area = pet_size.x * pet_size.y;
            if pet_area <= 0.0 {
                false
            } else {
                let pet_center = pet_tf.to_scale_angle_translation().2;
                let pet_min = pet_center - pet_size * 0.5;
                let pet_max = pet_center + pet_size * 0.5;

                let mut covered = 0.0_f32;
                for (win_node, win_tf, vis) in app_windows.iter() {
                    if *vis == Visibility::Hidden {
                        continue;
                    }
                    let half = win_node.size() * 0.5;
                    let win_center = win_tf.to_scale_angle_translation().2;
                    let overlap = (pet_max.min(win_center + half) - pet_min.max(win_center - half))
                        .max(Vec2::ZERO);
                    covered += overlap.x * overlap.y;
                }
                covered / pet_area >= OCCLUSION_THRESHOLD
            }
        }
        Err(_) => false,
    };
    if wanted.0 != occluded {
        wanted.0 = occluded;
    }
}

fn start_bgm(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    settings: Res<UserSettings>,
) {
    let bgm_handle = asset_server.load("nori/audio/bgm1.ogg");
    let music_vol = settings.volumes[1] as f32 / 100.0;
    commands.spawn((
        BgmMusic,
        AudioPlayer::new(bgm_handle),
        PlaybackSettings::LOOP.with_volume(bevy::audio::Volume::Linear(music_vol)),
    ));
}

fn update_bgm_volume(
    settings: Res<UserSettings>,
    mut sinks: Query<&mut AudioSink, With<BgmMusic>>,
    mut global_volume: ResMut<GlobalVolume>,
) {
    if settings.is_changed() {
        let master_vol = if settings.toggles[0] {
            settings.volumes[0] as f32 / 100.0
        } else {
            0.0
        };
        global_volume.volume = bevy::audio::Volume::Linear(master_vol);

        let music_vol = if settings.toggles[1] {
            settings.volumes[1] as f32 / 100.0
        } else {
            0.0
        };
        for mut sink in sinks.iter_mut() {
            sink.set_volume(bevy::audio::Volume::Linear(music_vol));
        }
    }
}

fn chat_rise_sync(
    head_query: Query<&Visibility, With<HeadDisplay>>,
    mut rise: ResMut<ChatRise>,
) {
    let head_visible = head_query
        .iter()
        .any(|v| *v == Visibility::Inherited || *v == Visibility::Visible);
    let target = if head_visible { 158.0 } else { 0.0 };
    if (rise.0 - target).abs() > 0.5 {
        rise.0 = target;
    }
}

const EMOTION_EXPRESSIONS: [(&str, &str); 9] = [
    ("happy", "07_Smile"),
    ("sad", "05_Dark"),
    ("angry", "03_Angry"),
    ("surprised", "02_Dizzy"),
    ("confused", "06_Speechless"),
    ("proud", "01_KiraKira"),
    ("shy", "04_Shy"),
    ("tired", "09_Troubled"),
    ("neutral", "00_Default"),
];

fn chat_emotion_bridge(
    mut emotion_events: MessageReader<ChatEmotionEvent>,
    mut pet: NonSendMut<Live2dPet>,
    time: Res<Time>,
) {
    for event in emotion_events.read() {
        if let Some((_, name)) = EMOTION_EXPRESSIONS
            .iter()
            .find(|(key, _)| key == &event.0.as_str())
        {
            pet.start_expression(name, time.elapsed_secs());
        }
    }
}
