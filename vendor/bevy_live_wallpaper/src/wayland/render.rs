use std::collections::HashMap;

use bevy::{
    asset::RenderAssetUsages,
    log::{debug, error, warn},
    prelude::{Assets, Handle, Image, Res, ResMut, Resource},
    render::{
        extract_resource::ExtractResource,
        render_asset::RenderAssets,
        render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages},
        renderer::{RenderAdapter, RenderDevice, RenderInstance, RenderQueue},
        texture::GpuImage,
    },
};
use wgpu::{
    CommandEncoderDescriptor, CompositeAlphaMode, CurrentSurfaceTexture, Origin3d, PresentMode,
    SurfaceConfiguration, SurfaceTargetUnsafe, TextureAspect,
};

use crate::wayland::surface::WaylandSurfaceHandles;

pub(crate) const WAYLAND_SURFACE_FORMAT: TextureFormat = TextureFormat::Bgra8UnormSrgb;

pub(crate) fn create_wayland_image(images: &mut Assets<Image>) -> Handle<Image> {
    let size = Extent3d {
        width: 1,
        height: 1,
        depth_or_array_layers: 1,
    };
    let mut image = Image::new_fill(
        size,
        TextureDimension::D2,
        &[0, 0, 0, 255],
        WAYLAND_SURFACE_FORMAT,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.usage =
        TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_SRC;
    images.add(image)
}

#[derive(Resource, ExtractResource, Clone, Debug)]
pub(crate) struct WaylandSurfaceDescriptor {
    pub surfaces: Vec<SurfaceDescriptorEntry>,
    pub generation: u64,
    /// Uniform buffer-space scale (num/den) applied to the shared render image.
    /// Equals the max effective scale across surfaces; surfaces whose own scale
    /// is lower are supersampled (the compositor downsamples via viewport dest).
    pub image_scale_num: u32,
    pub image_scale_den: u32,
}

impl Default for WaylandSurfaceDescriptor {
    fn default() -> Self {
        Self::new()
    }
}

impl WaylandSurfaceDescriptor {
    pub(crate) fn new() -> Self {
        Self {
            surfaces: Vec::new(),
            generation: 0,
            image_scale_num: 1,
            image_scale_den: 1,
        }
    }

    pub(crate) fn upsert_surface(&mut self, config: crate::wayland::WaylandSurfaceConfig) {
        if let Some(entry) = self
            .surfaces
            .iter_mut()
            .find(|entry| entry.output == config.output)
        {
            entry.handles = Some(config.handles);
            entry.width = config.width;
            entry.height = config.height;
            entry.offset_x = config.offset_x;
            entry.offset_y = config.offset_y;
        } else {
            self.surfaces.push(SurfaceDescriptorEntry {
                output: config.output,
                handles: Some(config.handles),
                width: config.width,
                height: config.height,
                offset_x: config.offset_x,
                offset_y: config.offset_y,
                buf_x: 0,
                buf_y: 0,
                buf_w: config.width.max(1),
                buf_h: config.height.max(1),
            });
        }
    }

    /// Logical (surface-local) union bounds across all live surfaces.
    /// Unaffected by scaling; used by the cursor/UI bridge.
    pub(crate) fn overall_bounds(&self) -> Option<(i32, i32, u32, u32)> {
        let mut iter_all = self.surfaces.iter().filter(|s| s.handles.is_some());
        let first = iter_all.next()?;

        let mut min_x = first.offset_x;
        let mut min_y = first.offset_y;
        let mut max_x = first.offset_x + first.width as i32;
        let mut max_y = first.offset_y + first.height as i32;

        for s in iter_all {
            min_x = min_x.min(s.offset_x);
            min_y = min_y.min(s.offset_y);
            max_x = max_x.max(s.offset_x + s.width as i32);
            max_y = max_y.max(s.offset_y + s.height as i32);
        }

        let width = (max_x - min_x).max(1) as u32;
        let height = (max_y - min_y).max(1) as u32;

        Some((min_x, min_y, width, height))
    }

    /// Buffer-space (physical) union bounds of the shared render image.
    fn scaled_edge(coord: i64, num: u64, den: u64) -> u64 {
        // ceil(coord * num / den) for coord >= 0
        ((coord as u64).saturating_mul(num).saturating_add(den - 1)) / den
    }

    fn scaled_offset(coord: i64, num: u64, den: u64) -> u64 {
        // floor(coord * num / den) for coord >= 0
        (coord as u64).saturating_mul(num) / den
    }

    /// Recompute per-surface buffer rects from the given per-output effective
    /// scale, then set the descriptor-level image scale to their max.
    /// Returns true if anything changed.
    pub(crate) fn recompute_buffer_layout(
        &mut self,
        effective_scale: impl Fn(u32) -> (u32, u32),
    ) -> bool {
        let mut changed = false;

        let live: Vec<u32> = self
            .surfaces
            .iter()
            .filter(|s| s.handles.is_some())
            .map(|s| s.output)
            .collect();
        if live.is_empty() {
            let old_num = self.image_scale_num;
            let old_den = self.image_scale_den;
            self.image_scale_num = 1;
            self.image_scale_den = 1;
            return old_num != 1 || old_den != 1 || changed;
        }

        // image scale = max over live surfaces of (num/den), compared cross-multiplied
        let mut img_num = 1u64;
        let mut img_den = 1u64;
        for output in &live {
            let (num, den) = effective_scale(*output);
            if (num as u64) * img_den > img_num * (den as u64) {
                img_num = num as u64;
                img_den = den as u64;
            }
        }
        if self.image_scale_num as u64 != img_num || self.image_scale_den as u64 != img_den {
            self.image_scale_num = img_num as u32;
            self.image_scale_den = img_den as u32;
            changed = true;
        }

        let logical_min_x = live
            .iter()
            .filter_map(|o| {
                self.surfaces
                    .iter()
                    .find(|s| s.output == *o)
                    .map(|s| s.offset_x)
            })
            .min()
            .unwrap_or(0) as i64;
        let logical_min_y = live
            .iter()
            .filter_map(|o| {
                self.surfaces
                    .iter()
                    .find(|s| s.output == *o)
                    .map(|s| s.offset_y)
            })
            .min()
            .unwrap_or(0) as i64;

        for entry in self.surfaces.iter_mut().filter(|s| s.handles.is_some()) {
            let dx = (entry.offset_x as i64) - logical_min_x;
            let dy = (entry.offset_y as i64) - logical_min_y;
            let x0 = Self::scaled_offset(dx, img_num, img_den);
            let y0 = Self::scaled_offset(dy, img_num, img_den);
            let x1 = Self::scaled_edge(dx + entry.width as i64, img_num, img_den);
            let y1 = Self::scaled_edge(dy + entry.height as i64, img_num, img_den);
            let buf_x = x0 as u32;
            let buf_y = y0 as u32;
            let buf_w = x1.saturating_sub(x0).max(1) as u32;
            let buf_h = y1.saturating_sub(y0).max(1) as u32;
            if entry.buf_x != buf_x
                || entry.buf_y != buf_y
                || entry.buf_w != buf_w
                || entry.buf_h != buf_h
            {
                entry.buf_x = buf_x;
                entry.buf_y = buf_y;
                entry.buf_w = buf_w;
                entry.buf_h = buf_h;
                changed = true;
            }
        }

        changed
    }

    /// Buffer-space size of the shared render image (union of surface buf rects).
    pub(crate) fn buffer_bounds(&self) -> Option<(u32, u32)> {
        let mut iter_all = self.surfaces.iter().filter(|s| s.handles.is_some());
        let first = iter_all.next()?;
        let mut max_x = first.buf_x + first.buf_w;
        let mut max_y = first.buf_y + first.buf_h;
        for s in iter_all {
            max_x = max_x.max(s.buf_x + s.buf_w);
            max_y = max_y.max(s.buf_y + s.buf_h);
        }
        Some((max_x.max(1), max_y.max(1)))
    }

    pub(crate) fn image_scale_f32(&self) -> f32 {
        self.image_scale_num as f32 / self.image_scale_den as f32
    }

    pub(crate) fn bump_generation(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SurfaceDescriptorEntry {
    pub output: u32,
    pub handles: Option<WaylandSurfaceHandles>,
    pub width: u32,
    pub height: u32,
    pub offset_x: i32,
    pub offset_y: i32,
    /// Buffer-space rect of this surface within the shared image (set by
    /// `recompute_buffer_layout`). All surfaces live in one image space scaled
    /// by the descriptor-level image_scale, so copies are 1:1 texel transfers.
    pub buf_x: u32,
    pub buf_y: u32,
    pub buf_w: u32,
    pub buf_h: u32,
}

#[derive(Resource, ExtractResource, Clone, Debug)]
pub(crate) struct WaylandRenderTarget {
    pub image: Handle<Image>,
    pub last_applied_generation: u64,
}

impl WaylandRenderTarget {
    pub(crate) fn new(image: Handle<Image>) -> Self {
        Self {
            image,
            last_applied_generation: 0,
        }
    }
}

#[derive(Resource, Default)]
pub(crate) struct WaylandGpuSurfaceState {
    pub surfaces: HashMap<u32, WaylandGpuPerSurface>,
}

#[derive(Default)]
pub(crate) struct WaylandGpuPerSurface {
    pub surface: Option<wgpu::Surface<'static>>,
    pub config: Option<SurfaceConfiguration>,
    pub last_applied_generation: u64,
}

pub(crate) fn prepare_wayland_surface(
    descriptor: Res<WaylandSurfaceDescriptor>,
    mut state: ResMut<WaylandGpuSurfaceState>,
    render_instance: Res<RenderInstance>,
    render_adapter: Res<RenderAdapter>,
    render_device: Res<RenderDevice>,
) {
    let valid_outputs: Vec<u32> = descriptor.surfaces.iter().map(|s| s.output).collect();
    state
        .surfaces
        .retain(|output, _| valid_outputs.contains(output));

    for surf_desc in descriptor.surfaces.iter().filter(|s| s.handles.is_some()) {
        let entry = state.surfaces.entry(surf_desc.output).or_default();

        let needs_recreate =
            entry.surface.is_none() || entry.last_applied_generation != descriptor.generation;

        if needs_recreate {
            let handles = surf_desc.handles.expect("handles exist");
            let raw_display_handle = handles.raw_display_handle();
            let raw_window_handle = handles.raw_window_handle();
            let instance = render_instance.0.as_ref();
            let surface = unsafe {
                instance
                    .create_surface_unsafe(SurfaceTargetUnsafe::RawHandle {
                        raw_display_handle: Some(raw_display_handle),
                        raw_window_handle,
                    })
                    .expect("failed to create Wayland wgpu surface")
            };
            entry.surface = Some(surface);
        }

        let Some(surface) = entry.surface.as_ref() else {
            continue;
        };

        let width = surf_desc.buf_w.max(1);
        let height = surf_desc.buf_h.max(1);

        let needs_reconfigure = entry
            .config
            .as_ref()
            .map(|config| config.width != width || config.height != height)
            .unwrap_or(true);

        if needs_reconfigure || needs_recreate {
            let capabilities = surface.get_capabilities(render_adapter.0.as_ref());
            if capabilities.formats.is_empty() {
                warn!("Wayland surface reported no supported formats; retrying later");
                entry.surface = None;
                entry.config = None;
                entry.last_applied_generation = 0;
                continue;
            }

            let format = capabilities
                .formats
                .iter()
                .copied()
                .find(|fmt| *fmt == WAYLAND_SURFACE_FORMAT)
                .or_else(|| capabilities.formats.first().copied())
                .expect("Wayland surface has no supported formats");

            let present_mode = capabilities
                .present_modes
                .iter()
                .copied()
                .find(|mode| matches!(mode, PresentMode::Fifo))
                .or_else(|| capabilities.present_modes.first().copied())
                .expect("Wayland surface has no supported present mode");

            let alpha_mode = capabilities
                .alpha_modes
                .iter()
                .copied()
                .find(|mode| matches!(mode, CompositeAlphaMode::Opaque))
                .unwrap_or(capabilities.alpha_modes[0]);

            let mut usage = TextureUsages::RENDER_ATTACHMENT;
            if capabilities.usages.contains(TextureUsages::COPY_DST) {
                usage |= TextureUsages::COPY_DST;
            }

            let config = SurfaceConfiguration {
                usage,
                format,
                width,
                height,
                present_mode,
                alpha_mode,
                view_formats: vec![],
                desired_maximum_frame_latency: 1,
            };

            render_device.configure_surface(surface, &config);

            entry.config = Some(config);
        }

        entry.last_applied_generation = descriptor.generation;
    }
}

pub(crate) fn present_wayland_surface(
    mut state: ResMut<WaylandGpuSurfaceState>,
    target: Option<Res<WaylandRenderTarget>>,
    images: Res<RenderAssets<GpuImage>>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    descriptor: Res<WaylandSurfaceDescriptor>,
) {
    let Some(target) = target else { return };

    let Some(gpu_image) = images.get(&target.image) else {
        return;
    };

    for (output, entry) in state.surfaces.iter_mut() {
        let Some(surface) = entry.surface.as_ref() else {
            continue;
        };
        let Some(config) = entry.config.as_ref() else {
            continue;
        };

        let Some(desc_entry) = descriptor
            .surfaces
            .iter()
            .find(|s| s.output == *output && s.handles.is_some())
        else {
            continue;
        };

        let extent = Extent3d {
            width: config.width.min(gpu_image.texture_descriptor.size.width),
            height: config.height.min(gpu_image.texture_descriptor.size.height),
            depth_or_array_layers: 1,
        };

        let surface_texture = match surface.get_current_texture() {
            CurrentSurfaceTexture::Success(texture)
            | CurrentSurfaceTexture::Suboptimal(texture) => texture,
            CurrentSurfaceTexture::Outdated => {
                debug!(
                    "Wayland surface for output {} outdated; scheduling reconfigure",
                    output
                );
                entry.config = None;
                entry.last_applied_generation = 0;
                continue;
            }
            CurrentSurfaceTexture::Lost => {
                warn!(
                    "Wayland surface for output {} lost; scheduling recreate",
                    output
                );
                entry.surface = None;
                entry.config = None;
                entry.last_applied_generation = 0;
                continue;
            }
            CurrentSurfaceTexture::Timeout | CurrentSurfaceTexture::Occluded => {
                debug!("Wayland surface acquire timeout (output {})", output);
                continue;
            }
            CurrentSurfaceTexture::Validation => {
                error!("Wayland surface validation failed (output {})", output);
                continue;
            }
        };

        let mut encoder = render_device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("wayland-surface-present"),
        });

        let src_origin = Origin3d {
            x: desc_entry.buf_x,
            y: desc_entry.buf_y,
            z: 0,
        };

        let mut src = gpu_image.texture.as_image_copy();
        src.origin = src_origin;

        let dst = wgpu::TexelCopyTextureInfo {
            texture: &surface_texture.texture,
            mip_level: 0,
            origin: Origin3d::ZERO,
            aspect: TextureAspect::All,
        };

        encoder.copy_texture_to_texture(src, dst, extent);

        render_queue.submit(Some(encoder.finish()));
        surface_texture.present();
    }
}
