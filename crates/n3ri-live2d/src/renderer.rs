//! Offscreen rendering of the Live2D model.
//!
//! Pipeline (mirrors live2d-viewer's FBO masking, rebuilt on Bevy/wgpu):
//!
//! ```text
//! mask cameras (per-group, order -(30+g)) ──▶ mask RTTs  (one per mask group)
//! pet camera   (order -10, layer 1)        ──▶ pet RTT   (drawables sorted by z,
//!                                                          masked ones sample group RTT)
//! UI ImageNode ───────────────────────────▶ displays pet RTT below app windows
//! ```
//!
//! Each Core drawable owns one Mesh2d + one [`Live2dDrawableMaterial`]
//! instance. Per frame we copy CPU-computed vertex positions into the meshes
//! and opacity/tints into the materials; blending mode (normal / additive /
//! multiplicative) is baked into the pipeline via `bind_group_data`
//! specialization since `AlphaMode2d` lacks Add/Multiply variants.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::camera::ClearColorConfig;
use bevy::camera::RenderTarget;
use bevy::camera::ScalingMode;
use bevy::camera::visibility::RenderLayers;
use bevy::image::{Image, ImageSampler};
use bevy::math::{Vec2, Vec4};
use bevy::mesh::{Indices, Mesh, Mesh2d, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, Extent3d,
    RenderPipelineDescriptor, SpecializedMeshPipelineError, TextureDimension, TextureFormat,
};
use bevy::render::view::Msaa;
use bevy::shader::ShaderRef;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dKey, MeshMaterial2d};
use bevy::mesh::MeshVertexBufferLayoutRef;

use bevy::window::PrimaryWindow;

use crate::pet::Live2dPet;

// ── constants ──

const DRAW_LAYER: usize = 1;
const FIT_MARGIN_X: f32 = 0.92;
const FIT_MARGIN_Y: f32 = 0.94;
const OPACITY_EPSILON: f32 = 0.001;
pub const DISPLAY_SCALE: f32 = 0.5;

#[derive(Resource, Clone, Copy)]
pub struct PetViewSize {
    pub w: u32,
    pub h: u32,
}

// ── material ──

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[allow(dead_code)]
enum BlendKind {
    Normal,
    Additive,
    Multiplicative,
    MaskFbo,
}

impl BlendKind {
    fn from_core(mode: i32) -> Self {
        match mode {
            1 => Self::Additive,
            // TEMP hack for hollow-interior diagnosis; revert to Self::Multiplicative.
            2 => Self::Normal,
            _ => Self::Normal,
        }
    }

    fn state(self) -> BlendState {
        match self {
            Self::Normal => BlendState {
                // SrcAlpha here double-multiplies: WGSL already outputs premultiplied c.rgb*alpha.
                color: BlendComponent {
                    src_factor: BlendFactor::One,
                    dst_factor: BlendFactor::OneMinusSrcAlpha,
                    operation: BlendOperation::Add,
                },
                alpha: BlendComponent {
                    src_factor: BlendFactor::One,
                    dst_factor: BlendFactor::OneMinusSrcAlpha,
                    operation: BlendOperation::Add,
                },
            },
            Self::Additive => BlendState {
                color: BlendComponent {
                    src_factor: BlendFactor::One,
                    dst_factor: BlendFactor::One,
                    operation: BlendOperation::Add,
                },
                alpha: BlendComponent {
                    src_factor: BlendFactor::One,
                    dst_factor: BlendFactor::OneMinusSrcAlpha,
                    operation: BlendOperation::Add,
                },
            },
            Self::Multiplicative => BlendState {
                color: BlendComponent {
                    src_factor: BlendFactor::Dst,
                    dst_factor: BlendFactor::OneMinusSrcAlpha,
                    operation: BlendOperation::Add,
                },
                alpha: BlendComponent {
                    src_factor: BlendFactor::Zero,
                    dst_factor: BlendFactor::OneMinusSrcAlpha,
                    operation: BlendOperation::Add,
                },
            },
            Self::MaskFbo => BlendState {
                color: BlendComponent {
                    src_factor: BlendFactor::One,
                    dst_factor: BlendFactor::OneMinusSrcAlpha,
                    operation: BlendOperation::Add,
                },
                alpha: BlendComponent {
                    src_factor: BlendFactor::Zero,
                    dst_factor: BlendFactor::OneMinusSrcAlpha,
                    operation: BlendOperation::Add,
                },
            },
        }
    }
}

#[derive(bevy::render::render_resource::ShaderType, Clone, Copy)]
struct Live2dUniforms {
    /// x = opacity, y = masked flag, z = solid flag, w unused.
    flags: Vec4,
    multiply_color: Vec4,
    screen_color: Vec4,
    /// xy = mask RTT pixel size.
    viewport: Vec4,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Live2dPipelineKey {
    blend: BlendKind,
}

impl From<&Live2dDrawableMaterial> for Live2dPipelineKey {
    fn from(m: &Live2dDrawableMaterial) -> Self {
        Self { blend: m.blend }
    }
}

#[derive(Asset, TypePath, Clone, AsBindGroup)]
#[bind_group_data(Live2dPipelineKey)]
pub struct Live2dDrawableMaterial {
    #[uniform(0)]
    uniforms: Live2dUniforms,
    #[texture(1)]
    #[sampler(2)]
    texture: Handle<Image>,
    #[texture(3)]
    #[sampler(4)]
    mask_texture: Handle<Image>,
    blend: BlendKind,
}

impl Material2d for Live2dDrawableMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/live2d_drawable.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }

    fn specialize(
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: Material2dKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let blend_state = key.bind_group_data.blend.state();
        descriptor.primitive.cull_mode = None;
        if let Some(fragment) = descriptor.fragment.as_mut() {
            if let Some(Some(target)) = fragment.targets.first_mut() {
                target.blend = Some(blend_state);
            }
        }
        Ok(())
    }
}

// ── scene bookkeeping ──

#[derive(Resource, Clone, Copy)]
pub struct PetMapping {
    pub scale: f32,
    pub center_model: Vec2,
    pub center_view: Vec2,
}

impl PetMapping {
    fn compute(bbox: [f32; 4], view_w: u32, view_h: u32) -> Self {
        let [min_x, min_y, max_x, max_y] = bbox;
        let bw = (max_x - min_x).max(f32::EPSILON);
        let bh = (max_y - min_y).max(f32::EPSILON);
        let sx = view_w as f32 * FIT_MARGIN_X / bw;
        let sy = view_h as f32 * FIT_MARGIN_Y / bh;
        Self {
            scale: sx.min(sy),
            center_model: Vec2::new((min_x + max_x) * 0.5, (min_y + max_y) * 0.5),
            center_view: Vec2::new(view_w as f32 * 0.5, view_h as f32 * 0.5),
        }
    }

    #[inline]
    fn apply(&self, x: f32, y: f32) -> [f32; 3] {
        [
            self.center_view.x + (x - self.center_model.x) * self.scale,
            self.center_view.y + (y - self.center_model.y) * self.scale,
            0.0,
        ]
    }

    #[inline]
    pub fn inverse(&self, view_x: f32, view_y: f32) -> Vec2 {
        Vec2::new(
            (view_x - self.center_view.x) / self.scale + self.center_model.x,
            -(view_y - self.center_view.y) / self.scale + self.center_model.y,
        )
    }
}

type Slot = (Entity, usize, Handle<Mesh>, Handle<Live2dDrawableMaterial>);

struct MaskGroup {
    mask_source_indices: Vec<usize>,
    _layer: usize,
    _camera_entity: Entity,
    _mask_entities: Vec<(Entity, usize)>,
    _rtt_handle: Handle<Image>,
}

#[derive(Resource)]
struct Live2dRenderRig {
    slots: Vec<Slot>,
    _mask_groups: Vec<MaskGroup>,
}

#[derive(Resource, Default)]
pub struct PetDisplayImage(pub Option<Handle<Image>>);

#[derive(Resource, Clone)]
pub struct PetHeadImage(pub Option<Handle<Image>>);

#[derive(Component)]
pub struct PetDisplayNode;

impl Default for PetHeadImage {
    fn default() -> Self {
        Self(None)
    }
}

// ── setup (exclusive Startup system) ──

pub fn load_and_setup_pet(world: &mut World) {
    let pet = match crate::loader::load_pet() {
        Ok(pet) => pet,
        Err(e) => {
            warn!("live2d pet disabled: {e}");
            return;
        }
    };

    let count = pet.drawable_count();
    info!("live2d pet loaded ({count} drawables)");

    let view_w;
    let view_h;
    {
        let mut query = world.query_filtered::<&Window, With<PrimaryWindow>>();
        if let Ok(window) = query.single(world) {
            view_w = window.physical_width().max(1);
            view_h = window.physical_height().max(1);
        } else {
            view_w = 800;
            view_h = 600;
        }
    }
    info!("live2d RTT: {view_w}x{view_h}");

    let pet_image_h = world
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(
            view_w,
            view_h,
            TextureFormat::Bgra8UnormSrgb,
            None,
        ));
    if let Some(mut img) = world.resource_mut::<Assets<Image>>().get_mut(&pet_image_h) {
        img.sampler = ImageSampler::linear();
    }

    let white_h = world.resource_mut::<Assets<Image>>().add(Image::new_fill(
        Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        TextureDimension::D2,
        &[255, 255, 255, 255],
        TextureFormat::Bgra8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    ));

    let asset_server = world.resource::<AssetServer>().clone();
    let pet_rel = crate::loader::MODEL_DIR;
    let texture_handles: Vec<Handle<Image>> = pet
        .texture_paths
        .iter()
        .map(|rel| asset_server.load(format!("{pet_rel}/{rel}")))
        .collect();

    world.spawn((
        Camera2d,
        Camera {
            order: -10,
            clear_color: ClearColorConfig::Custom(Color::NONE),
            ..default()
        },
        RenderTarget::Image(pet_image_h.clone().into()),
        Msaa::Sample4,
        RenderLayers::layer(DRAW_LAYER),
        Transform::from_xyz(view_w as f32 * 0.5, view_h as f32 * 0.5, 1000.0),
    ));

    // Head RTT — smaller target for the head-only camera.
    let head_size = 256u32;
    let head_image_h = world
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(
            head_size,
            head_size,
            TextureFormat::Bgra8UnormSrgb,
            None,
        ));
    if let Some(mut img) = world.resource_mut::<Assets<Image>>().get_mut(&head_image_h) {
        img.sampler = ImageSampler::linear();
    }

    let head_camera = world.spawn((
        Camera2d,
        Camera {
            order: -9,
            clear_color: ClearColorConfig::Custom(Color::NONE),
            ..default()
        },
        RenderTarget::Image(head_image_h.clone().into()),
        Msaa::Sample4,
        RenderLayers::layer(DRAW_LAYER),
        Transform::from_xyz(
            view_w as f32 * 0.5,
            view_h as f32 * 0.73,
            1000.0,
        ),
    )).id();
    if let Some(mut proj) = world.get_mut::<Projection>(head_camera) {
        if let Projection::Orthographic(ref mut ortho) = *proj {
            ortho.scaling_mode = ScalingMode::Fixed {
                width: view_w as f32,
                height: view_h as f32,
            };
            ortho.scale = 0.38;
        }
    }

    world.insert_resource(PetHeadImage(Some(head_image_h)));

    let mapping = PetMapping::compute(pet.vertex_bbox(), view_w, view_h);
    let d = pet.model.drawables();
    let vcounts = d.vertex_counts().to_vec();
    let uvs_ptrs = d.vertex_uvs().to_vec();
    let idx_counts = d.index_counts().to_vec();
    let idx_ptrs = d.indices().to_vec();
    let tex_idx = d.texture_indices().to_vec();
    let blend_modes = d.blend_modes().to_vec();
    let mask_counts = d.mask_counts().to_vec();
    let masks_ptrs = d.masks().to_vec();

    let mut mask_set_to_group: HashMap<Vec<usize>, usize> = HashMap::new();
    let mut drawable_group: Vec<Option<usize>> = vec![None; count];
    for i in 0..count {
        if mask_counts[i] > 0 {
            let mut set: Vec<usize> = (0..mask_counts[i].max(0) as usize)
                .map(|m| unsafe { *masks_ptrs[i].add(m) as usize })
                .collect();
            set.sort();
            let g = mask_set_to_group.len();
            let entry = mask_set_to_group.entry(set).or_insert(g);
            drawable_group[i] = Some(*entry);
        }
    }
    let num_groups = mask_set_to_group.len();
    info!("live2d mask groups: {num_groups}");

    struct Prepared {
        mesh: Mesh,
        material: Live2dDrawableMaterial,
    }

    let prepared: Vec<Prepared> = (0..count)
        .map(|i| {
            let n = vcounts[i].max(0) as usize;
            let uv_slice = unsafe { std::slice::from_raw_parts(uvs_ptrs[i], n) };
            let uvs: Vec<[f32; 2]> = uv_slice.iter().map(|p| [p.X, 1.0 - p.Y]).collect();

            let idx_n = idx_counts[i].max(0) as usize;
            let indices: Vec<u16> =
                unsafe { std::slice::from_raw_parts(idx_ptrs[i], idx_n).to_vec() };

            let mut mesh = Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
            );
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; n]);
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
            mesh.insert_indices(Indices::U16(indices));

            let tex = texture_handles
                .get(tex_idx[i].max(0) as usize)
                .cloned()
                .unwrap_or_else(|| white_h.clone());

            let material = Live2dDrawableMaterial {
                uniforms: Live2dUniforms {
                    flags: Vec4::new(1.0, 0.0, 0.0, 0.0),
                    multiply_color: Vec4::ONE,
                    screen_color: Vec4::ZERO,
                    viewport: Vec4::new(view_w as f32, view_h as f32, 0.0, 0.0),
                },
                texture: tex,
                mask_texture: white_h.clone(),
                blend: BlendKind::from_core(blend_modes[i]),
            };

            Prepared { mesh, material }
        })
        .collect();

    let (mesh_list, mat_list): (Vec<Mesh>, Vec<Live2dDrawableMaterial>) =
        prepared.into_iter().map(|p| (p.mesh, p.material)).unzip();

    let mesh_handles: Vec<Handle<Mesh>> = {
        let mut meshes = world.resource_mut::<Assets<Mesh>>();
        mesh_list.into_iter().map(|m| meshes.add(m)).collect()
    };
    let mat_handles: Vec<Handle<Live2dDrawableMaterial>> = {
        let mut materials = world.resource_mut::<Assets<Live2dDrawableMaterial>>();
        mat_list.into_iter().map(|m| materials.add(m)).collect()
    };

    let mut slots: Vec<Slot> = Vec::with_capacity(count);
    for i in 0..count {
        let mesh_h = mesh_handles[i].clone();
        let mat_h = mat_handles[i].clone();
        let entity = world
            .spawn((
                Mesh2d(mesh_h.clone()),
                MeshMaterial2d(mat_h.clone()),
                Transform::from_xyz(0.0, 0.0, i as f32),
                RenderLayers::layer(DRAW_LAYER),
            ))
            .id();
        slots.push((entity, i, mesh_h, mat_h));
    }

    let mut mask_groups: Vec<MaskGroup> = Vec::with_capacity(num_groups);

    let mut group_sources: Vec<Vec<usize>> = vec![Vec::new(); num_groups];
    for (set, &g) in &mask_set_to_group {
        group_sources[g] = set.clone();
    }

    let mut group_rtts: Vec<Handle<Image>> = Vec::with_capacity(num_groups);
    for g in 0..num_groups {
        let rtt_h = world
            .resource_mut::<Assets<Image>>()
            .add(Image::new_target_texture(
                view_w,
                view_h,
                TextureFormat::Bgra8UnormSrgb,
                None,
            ));
        if let Some(mut img) = world.resource_mut::<Assets<Image>>().get_mut(&rtt_h) {
            img.sampler = ImageSampler::linear();
        }
        group_rtts.push(rtt_h.clone());

        let layer = 10 + g;
        let cam = world
            .spawn((
                Camera2d,
                Camera {
                    order: -(30 + g as isize),
                    clear_color: ClearColorConfig::Custom(Color::WHITE),
                    ..default()
                },
                RenderTarget::Image(rtt_h.clone().into()),
                Msaa::Sample4,
                RenderLayers::layer(layer),
                Transform::from_xyz(view_w as f32 * 0.5, view_h as f32 * 0.5, 1000.0),
            ))
            .id();

        let mut mask_entities = Vec::with_capacity(group_sources[g].len());
        for &src_idx in &group_sources[g] {
            // Cubism masks sample their own texture alpha — never share one
            // flat-fill material across mask sources.
            let src_tex = texture_handles
                .get(tex_idx[src_idx].max(0) as usize)
                .cloned()
                .unwrap_or_else(|| white_h.clone());
            let mat_h = world
                .resource_mut::<Assets<Live2dDrawableMaterial>>()
                .add(Live2dDrawableMaterial {
                    uniforms: Live2dUniforms {
                        flags: Vec4::new(1.0, 0.0, 1.0, 0.0),
                        multiply_color: Vec4::ONE,
                        screen_color: Vec4::ZERO,
                        viewport: Vec4::new(view_w as f32, view_h as f32, 0.0, 0.0),
                    },
                    texture: src_tex,
                    mask_texture: white_h.clone(),
                    blend: BlendKind::MaskFbo,
                });
            let entity = world
                .spawn((
                    Mesh2d(mesh_handles[src_idx].clone()),
                    MeshMaterial2d(mat_h),
                    Transform::from_xyz(0.0, 0.0, src_idx as f32),
                    RenderLayers::layer(layer),
                ))
                .id();
            mask_entities.push((entity, src_idx));
        }

        mask_groups.push(MaskGroup {
            mask_source_indices: group_sources[g].clone(),
            _layer: layer,
            _camera_entity: cam,
            _mask_entities: mask_entities,
            _rtt_handle: rtt_h,
        });
    }

    {
        let mut materials = world.resource_mut::<Assets<Live2dDrawableMaterial>>();
        for i in 0..count {
            if let Some(g) = drawable_group[i] {
                if let Some(mut mat) = materials.get_mut(&mat_handles[i]) {
                    mat.mask_texture = group_rtts[g].clone();
                }
            }
        }
    }

    info!(
        "live2d mask groups: {} groups, mask sources: {}",
        mask_groups.len(),
        mask_groups.iter().map(|g| g.mask_source_indices.len()).sum::<usize>()
    );

    world.insert_resource(Live2dRenderRig {
        slots,
        _mask_groups: mask_groups,
    });
    world.insert_resource(PetViewSize { w: view_w, h: view_h });
    world.insert_resource(mapping);
    world.insert_resource(PetDisplayImage(Some(pet_image_h)));
    world.insert_non_send(pet);
}

// ── per-frame systems ──

pub fn tick_pet(mut pet: NonSendMut<Live2dPet>, time: Res<Time>) {
    pet.tick(time.delta_secs(), time.elapsed_secs());
}

struct FrameDrawData {
    pos_ptrs: Vec<*const f32>,
    counts: Vec<i32>,
    opacities: Vec<f32>,
    mult_ptrs: Vec<*const f32>,
    scr_ptrs: Vec<*const f32>,
    orders: Vec<i32>,
    masked: Vec<bool>,
    inverted: Vec<bool>,
    visible: Vec<bool>,
}

fn read_vec4(ptr: *const f32) -> Vec4 {
    unsafe { Vec4::from_array([*ptr, *ptr.add(1), *ptr.add(2), *ptr.add(3)]) }
}

pub fn sync_live2d(world: &mut World) {
    let Some(rig) = world.get_resource::<Live2dRenderRig>() else {
        return;
    };
    let Some(mapping) = world.get_resource::<PetMapping>() else {
        return;
    };
    let slots = rig.slots.clone();
    let mapping = *mapping;

    let frame = {
        let pet = world.non_send::<Live2dPet>();
        let d = pet.model.drawables();
        let count = d.len();
        let dyn_flags = d.dynamic_flags();
        let const_flags = d.constant_flags();
        let all_render_orders = pet.model.render_orders();
        let render_orders: Vec<i32> = all_render_orders[..count].to_vec();
        FrameDrawData {
            pos_ptrs: d.vertex_positions()[..count]
                .iter()
                .map(|p| *p as *const f32)
                .collect(),
            counts: d.vertex_counts().to_vec(),
            opacities: d.opacities().to_vec(),
            mult_ptrs: d.multiply_colors()[..count]
                .iter()
                .map(|p| std::ptr::from_ref(p) as *const f32)
                .collect(),
            scr_ptrs: d.screen_colors()[..count]
                .iter()
                .map(|p| std::ptr::from_ref(p) as *const f32)
                .collect(),
            orders: render_orders,
            masked: d.mask_counts().iter().map(|&c| c > 0).collect(),
            // csmIsInvertedMask = 1 << 3 in the Cubism Core header (no named
            // constant in our bindings): inverted → visible INSIDE mask shape.
            inverted: const_flags[..count].iter().map(|&f| f & 8 != 0).collect(),
            visible: dyn_flags.iter().map(|&f| f & 1 != 0).collect(),
        }
    };

    {
        let mut meshes = world.resource_mut::<Assets<Mesh>>();
        for (_, i, mesh_h, _) in &slots {
            let n = frame.counts[*i].max(0) as usize;
            if n == 0 {
                continue;
            }
            let positions: Vec<[f32; 3]> = (0..n)
                .map(|vi| unsafe {
                    mapping.apply(
                        *frame.pos_ptrs[*i].add(vi * 2),
                        *frame.pos_ptrs[*i].add(vi * 2 + 1),
                    )
                })
                .collect();
            if let Some(mut mesh) = meshes.get_mut(mesh_h) {
                mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
            }
        }
    }

    {
        let mut materials = world.resource_mut::<Assets<Live2dDrawableMaterial>>();
        for (_, i, _, mat_h) in &slots {
            if let Some(mut mat) = materials.get_mut(mat_h) {
                mat.uniforms.flags = Vec4::new(
                    frame.opacities[*i],
                    frame.masked[*i] as u32 as f32,
                    0.0,
                    frame.inverted[*i] as u32 as f32,
                );
                mat.uniforms.multiply_color = read_vec4(frame.mult_ptrs[*i]);
                mat.uniforms.screen_color = read_vec4(frame.scr_ptrs[*i]);
            }
        }
    }

    for (entity, i, _, _) in &slots {
        let visible = frame.opacities[*i] >= OPACITY_EPSILON && frame.visible[*i];
        let Ok(mut ent) = world.get_entity_mut(*entity) else { continue };
        let cur_visible =
            ent.get::<Visibility>().map(|v| *v != Visibility::Hidden).unwrap_or(true);
        if visible != cur_visible {
            ent.insert(if visible { Visibility::Visible } else { Visibility::Hidden });
        }
        let want_z = frame.orders[*i] as f32;
        if let Some(mut t) = ent.get_mut::<Transform>() {
            if t.translation.z != want_z {
                t.translation.z = want_z;
            }
        }
    }
}

// ── desktop UI integration ──

pub fn spawn_pet_display(
    parent: &mut ChildSpawnerCommands<'_>,
    image: &Handle<Image>,
    view_size: &PetViewSize,
) {
    parent
        .spawn(Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Px(0.0),
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_children(|center| {
            center.spawn((
                PetDisplayNode,
                ImageNode {
                    image: image.clone(),
                    ..default()
                },
                Node {
                    width: Val::Px(view_size.w as f32 * DISPLAY_SCALE),
                    height: Val::Px(view_size.h as f32 * DISPLAY_SCALE),
                    ..default()
                },
            ));
        });
}

pub fn spawn_head_display(
    parent: &mut ChildSpawnerCommands<'_>,
    image: &Handle<Image>,
) {
    parent.spawn((
        HeadDisplay,
        ImageNode {
            image: image.clone(),
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(64.0),
            right: Val::Px(24.0),
            width: Val::Px(150.0),
            height: Val::Px(150.0),
            ..default()
        },
        GlobalZIndex(100),
        Visibility::Hidden,
    ));
}

#[derive(Component)]
pub struct HeadDisplay;
