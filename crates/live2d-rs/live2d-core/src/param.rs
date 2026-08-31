use crate::model::Model;
use core::ffi::CStr;
use live2d_core_sys as ffi;

pub struct Parameters<'a> {
    model: &'a Model<'a>,
    count: i32,
}

impl<'a> Parameters<'a> {
    pub(crate) fn new(model: &'a Model<'a>) -> Self {
        let count = unsafe { ffi::csmGetParameterCount(model.as_raw()) };
        Self { model, count }
    }

    pub fn len(&self) -> usize {
        self.count.max(0) as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn ids(&self) -> Vec<&CStr> {
        let ptrs = unsafe { ffi::csmGetParameterIds(self.model.as_raw()) };
        if ptrs.is_null() {
            return vec![];
        }
        (0..self.len())
            .map(|i| unsafe { CStr::from_ptr(*ptrs.add(i)) })
            .collect()
    }

    pub fn values(&self) -> &[f32] {
        let ptr = unsafe { ffi::csmGetParameterValues(self.model.as_raw() as *mut _) };
        if ptr.is_null() {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(ptr, self.len()) }
    }

    pub fn values_mut(&mut self) -> ParameterValuesMut<'_> {
        let ptr = unsafe { ffi::csmGetParameterValues(self.model.as_raw() as *mut _) };
        ParameterValuesMut {
            ptr,
            len: self.len(),
            _phantom: core::marker::PhantomData,
        }
    }

    pub fn default_values(&self) -> &[f32] {
        let ptr = unsafe { ffi::csmGetParameterDefaultValues(self.model.as_raw()) };
        if ptr.is_null() {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(ptr, self.len()) }
    }

    pub fn minimum_values(&self) -> &[f32] {
        let ptr = unsafe { ffi::csmGetParameterMinimumValues(self.model.as_raw()) };
        if ptr.is_null() {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(ptr, self.len()) }
    }

    pub fn maximum_values(&self) -> &[f32] {
        let ptr = unsafe { ffi::csmGetParameterMaximumValues(self.model.as_raw()) };
        if ptr.is_null() {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(ptr, self.len()) }
    }

    pub fn types(&self) -> &[ffi::csmParameterType] {
        let ptr = unsafe { ffi::csmGetParameterTypes(self.model.as_raw()) };
        if ptr.is_null() {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(ptr, self.len()) }
    }
}

pub struct ParameterValuesMut<'a> {
    ptr: *mut f32,
    len: usize,
    _phantom: core::marker::PhantomData<&'a ()>,
}

impl ParameterValuesMut<'_> {
    pub fn as_slice(&self) -> &[f32] {
        if self.ptr.is_null() {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(self.ptr, self.len) }
    }

    pub fn as_mut_slice(&mut self) -> &mut [f32] {
        if self.ptr.is_null() {
            return &mut [];
        }
        unsafe { core::slice::from_raw_parts_mut(self.ptr, self.len) }
    }

    pub fn set(&mut self, index: usize, value: f32) {
        if index < self.len && !self.ptr.is_null() {
            unsafe {
                *self.ptr.add(index) = value;
            }
        }
    }
}
