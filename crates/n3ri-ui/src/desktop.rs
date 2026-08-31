use bevy::asset::Assets;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;
use bevy::window::PrimaryWindow;

#[derive(AsBindGroup, Asset, TypePath, Debug, Clone)]
pub struct DesktopBackgroundMaterial {
    #[uniform(0)]
    pub mouse_pos: Vec2,
    #[uniform(0)]
    pub time: f32,
    #[uniform(0)]
    pub zoom: f32,
    #[uniform(0)]
    pub offset: Vec2,
    #[texture(1)]
    #[sampler(2)]
    pub water_normal: Handle<Image>,
    #[texture(3)]
    #[sampler(4)]
    pub noise: Handle<Image>,
}

impl UiMaterial for DesktopBackgroundMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/desktop_background.wgsl".into()
    }
}

pub struct DesktopFxPlugin;

impl Plugin for DesktopFxPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(UiMaterialPlugin::<DesktopBackgroundMaterial>::default())
            .add_systems(Update, animate_desktop_background);
    }
}

#[derive(Component)]
pub struct DesktopBackground;

fn animate_desktop_background(
    time: Res<Time>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut materials: ResMut<Assets<DesktopBackgroundMaterial>>,
    nodes: Query<&MaterialNode<DesktopBackgroundMaterial>, With<DesktopBackground>>,
) {
    let mouse_pos = windows
        .single()
        .ok()
        .and_then(|w| {
            let size = w.size();
            w.cursor_position().map(|p| {
                Vec2::new(
                    (p.x / size.x) * 2.0 - 1.0,
                    (p.y / size.y) * 2.0 - 1.0,
                )
            })
        })
        .unwrap_or(Vec2::ZERO);

    for m in nodes.iter() {
        if let Some(mut mat) = materials.get_mut(&m.0) {
            mat.time += time.delta_secs();
            mat.mouse_pos = mouse_pos;
        }
    }
}

pub fn spawn_desktop_background(
    parent: &mut ChildSpawnerCommands,
    asset_server: &AssetServer,
    images: &mut Assets<Image>,
    materials: &mut Assets<DesktopBackgroundMaterial>,
) -> Entity {
    let water = asset_server.load("nori/ocean/water-normal.png");
    let noise = asset_server.load("nori/ocean/gradient-noise.jpg");

    for handle in [&water, &noise] {
        if let Some(mut img) = images.get_mut(handle) {
            img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                address_mode_u: ImageAddressMode::Repeat,
                address_mode_v: ImageAddressMode::Repeat,
                ..default()
            });
        }
    }

    let handle = materials.add(DesktopBackgroundMaterial {
        mouse_pos: Vec2::ZERO,
        time: 0.0,
        zoom: 1.0,
        offset: Vec2::ZERO,
        water_normal: water,
        noise,
    });

    parent
        .spawn((
            DesktopBackground,
            MaterialNode::<DesktopBackgroundMaterial>(handle),
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                ..default()
            },
        ))
        .id()
}
