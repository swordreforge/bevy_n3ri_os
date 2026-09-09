use bevy::asset::Assets;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

use crate::cursor::{CursorPosition, UiArea};

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
    cursor: Res<CursorPosition>,
    area: Res<UiArea>,
    mut materials: ResMut<Assets<DesktopBackgroundMaterial>>,
    nodes: Query<&MaterialNode<DesktopBackgroundMaterial>, With<DesktopBackground>>,
) {
    let mouse_pos = if cursor.active && area.x > 0.0 && area.y > 0.0 {
        Vec2::new(
            (cursor.logical.x / area.x) * 2.0 - 1.0,
            (cursor.logical.y / area.y) * 2.0 - 1.0,
        )
    } else {
        Vec2::ZERO
    };

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
    let noise = asset_server.load("nori/ocean/gradient-noise.jpg");

    if let Some(mut img) = images.get_mut(&noise) {
        img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            ..default()
        });
    }

    let handle = materials.add(DesktopBackgroundMaterial {
        mouse_pos: Vec2::ZERO,
        time: 0.0,
        zoom: 1.0,
        offset: Vec2::ZERO,
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
