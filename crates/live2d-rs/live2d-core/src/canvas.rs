use crate::model::Model;
use live2d_core_sys as ffi;

#[derive(Debug, Clone, Copy)]
pub struct CanvasInfo {
    pub size_in_pixels: ffi::csmVector2,
    pub origin_in_pixels: ffi::csmVector2,
    pub pixels_per_unit: f32,
}

impl CanvasInfo {
    pub fn read(model: &Model) -> Self {
        let mut size = ffi::csmVector2 { X: 0.0, Y: 0.0 };
        let mut origin = ffi::csmVector2 { X: 0.0, Y: 0.0 };
        let mut ppu = 0.0f32;
        unsafe {
            ffi::csmReadCanvasInfo(model.as_raw(), &mut size, &mut origin, &mut ppu);
        }
        Self {
            size_in_pixels: size,
            origin_in_pixels: origin,
            pixels_per_unit: ppu,
        }
    }
}
