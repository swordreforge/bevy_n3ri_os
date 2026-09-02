use bevy::prelude::*;

/// Combined wallpaper surface extents in logical coordinates.
///
/// On Wayland, this is derived from layer-surface configure events and output
/// logical positions (xdg-output / wl_output). On other platforms it currently
/// stays at the default value unless implemented.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct WallpaperSurfaceInfo {
    /// Logical top-left of the wallpaper area (e.g., min x/y across outputs).
    pub offset_position: Vec2,
    /// Logical width/height of the wallpaper area.
    pub size: Vec2,
    /// UI render-target scale factor applied to the wallpaper image
    /// (logical × scale = buffer pixels). Matches the camera's
    /// `ImageRenderTarget.scale_factor`, so UI hit-testing must feed
    /// `logical × scale` coordinates.
    pub scale: f32,
}

impl Default for WallpaperSurfaceInfo {
    fn default() -> Self {
        Self {
            offset_position: Vec2::ZERO,
            size: Vec2::ZERO,
            // No fractional scaling → UI coordinates equal buffer pixels.
            scale: 1.0,
        }
    }
}

impl WallpaperSurfaceInfo {
    pub fn set(&mut self, offset_x: i32, offset_y: i32, width: u32, height: u32) {
        self.offset_position = Vec2::new(offset_x as f32, offset_y as f32);
        self.size = Vec2::new(width as f32, height as f32);
    }
}
