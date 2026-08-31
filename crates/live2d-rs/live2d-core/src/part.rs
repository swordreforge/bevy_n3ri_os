use crate::model::Model;
use core::ffi::CStr;
use live2d_core_sys as ffi;

pub struct Parts<'a> {
    model: &'a Model<'a>,
    count: i32,
}

impl<'a> Parts<'a> {
    pub(crate) fn new(model: &'a Model<'a>) -> Self {
        let count = unsafe { ffi::csmGetPartCount(model.as_raw()) };
        Self { model, count }
    }

    pub fn len(&self) -> usize {
        self.count.max(0) as usize
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn ids(&self) -> Vec<&CStr> {
        let ptrs = unsafe { ffi::csmGetPartIds(self.model.as_raw()) };
        if ptrs.is_null() {
            return vec![];
        }
        (0..self.len())
            .map(|i| unsafe { CStr::from_ptr(*ptrs.add(i)) })
            .collect()
    }

    pub fn opacities(&self) -> &[f32] {
        let ptr = unsafe { ffi::csmGetPartOpacities(self.model.as_raw() as *mut _) };
        if ptr.is_null() {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(ptr, self.len()) }
    }

    pub fn opacities_mut(&mut self) -> &mut [f32] {
        let ptr = unsafe { ffi::csmGetPartOpacities(self.model.as_raw() as *mut _) };
        if ptr.is_null() {
            return &mut [];
        }
        unsafe { core::slice::from_raw_parts_mut(ptr, self.len()) }
    }

    pub fn parent_part_indices(&self) -> &[i32] {
        let ptr = unsafe { ffi::csmGetPartParentPartIndices(self.model.as_raw()) };
        if ptr.is_null() {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(ptr, self.len()) }
    }

    pub fn offscreen_indices(&self) -> &[i32] {
        let ptr = unsafe { ffi::csmGetPartOffscreenIndices(self.model.as_raw()) };
        if ptr.is_null() {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(ptr, self.len()) }
    }
}
