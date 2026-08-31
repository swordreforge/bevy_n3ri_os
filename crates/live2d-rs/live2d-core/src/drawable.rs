use crate::model::Model;
use core::ffi::CStr;
use live2d_core_sys as ffi;

pub struct Drawables<'a> {
    model: &'a Model<'a>,
    count: i32,
}

impl<'a> Drawables<'a> {
    pub(crate) fn new(model: &'a Model<'a>) -> Self {
        let count = unsafe { ffi::csmGetDrawableCount(model.as_raw()) };
        Self { model, count }
    }

    pub fn len(&self) -> usize {
        self.count.max(0) as usize
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn ids(&self) -> Vec<&CStr> {
        let ptrs = unsafe { ffi::csmGetDrawableIds(self.model.as_raw()) };
        if ptrs.is_null() {
            return vec![];
        }
        (0..self.len())
            .map(|i| unsafe { CStr::from_ptr(*ptrs.add(i)) })
            .collect()
    }

    pub fn texture_indices(&self) -> &[i32] {
        let ptr = unsafe { ffi::csmGetDrawableTextureIndices(self.model.as_raw()) };
        if ptr.is_null() {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(ptr, self.len()) }
    }

    pub fn opacities(&self) -> &[f32] {
        let ptr = unsafe { ffi::csmGetDrawableOpacities(self.model.as_raw()) };
        if ptr.is_null() {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(ptr, self.len()) }
    }

    pub fn constant_flags(&self) -> &[ffi::csmFlags] {
        let ptr = unsafe { ffi::csmGetDrawableConstantFlags(self.model.as_raw()) };
        if ptr.is_null() {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(ptr, self.len()) }
    }

    pub fn dynamic_flags(&self) -> &[ffi::csmFlags] {
        let ptr = unsafe { ffi::csmGetDrawableDynamicFlags(self.model.as_raw()) };
        if ptr.is_null() {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(ptr, self.len()) }
    }

    pub fn blend_modes(&self) -> &[i32] {
        let ptr = unsafe { ffi::csmGetDrawableBlendModes(self.model.as_raw()) };
        if ptr.is_null() {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(ptr, self.len()) }
    }

    pub fn multiply_colors(&self) -> &[ffi::csmVector4] {
        let ptr = unsafe { ffi::csmGetDrawableMultiplyColors(self.model.as_raw()) };
        if ptr.is_null() {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(ptr, self.len()) }
    }

    pub fn screen_colors(&self) -> &[ffi::csmVector4] {
        let ptr = unsafe { ffi::csmGetDrawableScreenColors(self.model.as_raw()) };
        if ptr.is_null() {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(ptr, self.len()) }
    }

    pub fn mask_counts(&self) -> &[i32] {
        let ptr = unsafe { ffi::csmGetDrawableMaskCounts(self.model.as_raw()) };
        if ptr.is_null() {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(ptr, self.len()) }
    }

    pub fn masks(&self) -> &[*const i32] {
        let ptr = unsafe { ffi::csmGetDrawableMasks(self.model.as_raw()) };
        if ptr.is_null() {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(ptr, self.len()) }
    }

    pub fn vertex_counts(&self) -> &[i32] {
        let ptr = unsafe { ffi::csmGetDrawableVertexCounts(self.model.as_raw()) };
        if ptr.is_null() {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(ptr, self.len()) }
    }

    pub fn vertex_positions(&self) -> &[*const ffi::csmVector2] {
        let ptr = unsafe { ffi::csmGetDrawableVertexPositions(self.model.as_raw()) };
        if ptr.is_null() {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(ptr, self.len()) }
    }

    pub fn vertex_uvs(&self) -> &[*const ffi::csmVector2] {
        let ptr = unsafe { ffi::csmGetDrawableVertexUvs(self.model.as_raw()) };
        if ptr.is_null() {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(ptr, self.len()) }
    }

    pub fn index_counts(&self) -> &[i32] {
        let ptr = unsafe { ffi::csmGetDrawableIndexCounts(self.model.as_raw()) };
        if ptr.is_null() {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(ptr, self.len()) }
    }

    pub fn indices(&self) -> &[*const u16] {
        let ptr = unsafe { ffi::csmGetDrawableIndices(self.model.as_raw()) };
        if ptr.is_null() {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(ptr, self.len()) }
    }

    pub fn parent_part_indices(&self) -> &[i32] {
        let ptr = unsafe { ffi::csmGetDrawableParentPartIndices(self.model.as_raw()) };
        if ptr.is_null() {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(ptr, self.len()) }
    }

    pub fn draw_orders(&self) -> &[i32] {
        let ptr = unsafe { ffi::csmGetDrawableDrawOrders(self.model.as_raw()) };
        if ptr.is_null() {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(ptr, self.len()) }
    }

    pub fn render_order_indices(&self) -> Vec<usize> {
        let n = self.len();
        let orders = self.draw_orders();
        let mut indices: Vec<usize> = (0..n).collect();
        indices.sort_by_key(|&i| orders[i]);
        indices
    }
}
