use crate::model::Model;
use live2d_core_sys as ffi;

pub struct OffscreenInfos<'a> {
    model: &'a Model<'a>,
    count: i32,
}

impl<'a> OffscreenInfos<'a> {
    pub(crate) fn new(model: &'a Model<'a>) -> Self {
        let count = unsafe { ffi::csmGetOffscreenCount(model.as_raw()) };
        Self { model, count }
    }

    pub fn len(&self) -> usize {
        self.count.max(0) as usize
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn blend_modes(&self) -> &[i32] {
        let ptr = unsafe { ffi::csmGetOffscreenBlendModes(self.model.as_raw()) };
        if ptr.is_null() {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(ptr, self.len()) }
    }

    pub fn opacities(&self) -> &[f32] {
        let ptr = unsafe { ffi::csmGetOffscreenOpacities(self.model.as_raw()) };
        if ptr.is_null() {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(ptr, self.len()) }
    }

    pub fn owner_indices(&self) -> &[i32] {
        let ptr = unsafe { ffi::csmGetOffscreenOwnerIndices(self.model.as_raw()) };
        if ptr.is_null() {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(ptr, self.len()) }
    }

    pub fn multiply_colors(&self) -> &[ffi::csmVector4] {
        let ptr = unsafe { ffi::csmGetOffscreenMultiplyColors(self.model.as_raw()) };
        if ptr.is_null() {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(ptr, self.len()) }
    }

    pub fn screen_colors(&self) -> &[ffi::csmVector4] {
        let ptr = unsafe { ffi::csmGetOffscreenScreenColors(self.model.as_raw()) };
        if ptr.is_null() {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(ptr, self.len()) }
    }

    pub fn mask_counts(&self) -> &[i32] {
        let ptr = unsafe { ffi::csmGetOffscreenMaskCounts(self.model.as_raw()) };
        if ptr.is_null() {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(ptr, self.len()) }
    }

    pub fn masks(&self) -> &[*const i32] {
        let ptr = unsafe { ffi::csmGetOffscreenMasks(self.model.as_raw()) };
        if ptr.is_null() {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(ptr, self.len()) }
    }

    pub fn constant_flags(&self) -> &[ffi::csmFlags] {
        let ptr = unsafe { ffi::csmGetOffscreenConstantFlags(self.model.as_raw()) };
        if ptr.is_null() {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(ptr, self.len()) }
    }
}
