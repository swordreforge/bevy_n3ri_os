use crate::error::{Error, Result};
use crate::moc::Moc;
use live2d_core_sys as ffi;
use std::alloc::{alloc, dealloc, Layout};
use std::marker::PhantomData;
use std::ptr::NonNull;

pub struct Model<'moc> {
    storage: *mut u8,
    storage_size: usize,
    raw: NonNull<ffi::csmModel>,
    _phantom: PhantomData<&'moc Moc>,
}

impl<'moc> Model<'moc> {
    pub fn initialize(moc: &'moc Moc) -> Result<Self> {
        let model_size = moc.sizeof_model() as usize;
        if model_size == 0 {
            return Err(Error::InitModelFailed);
        }

        let layout = Layout::from_size_align(model_size, ffi::csmAlignofModel as usize)
            .map_err(|_| Error::InvalidInput("model alignment overflow"))?;
        let storage = unsafe { alloc(layout) };
        if storage.is_null() {
            return Err(Error::InvalidInput("model alloc returned null"));
        }

        let raw = unsafe {
            ffi::csmInitializeModelInPlace(moc.as_raw(), storage as *mut _, model_size as u32)
        };
        if raw.is_null() {
            unsafe { dealloc(storage, layout) };
            return Err(Error::InitModelFailed);
        }

        Ok(Self {
            storage,
            storage_size: model_size,
            raw: unsafe { NonNull::new_unchecked(raw) },
            _phantom: PhantomData,
        })
    }

    pub fn update(&mut self) {
        unsafe { ffi::csmUpdateModel(self.raw.as_ptr()) }
    }

    pub(crate) fn as_raw(&self) -> *const ffi::csmModel {
        self.raw.as_ptr()
    }

    pub(crate) fn as_raw_mut(&mut self) -> *mut ffi::csmModel {
        self.raw.as_ptr()
    }

    pub fn canvas_info(&self) -> crate::canvas::CanvasInfo {
        crate::canvas::CanvasInfo::read(self)
    }

    pub fn parameters(&self) -> crate::param::Parameters<'_> {
        crate::param::Parameters::new(self)
    }

    pub fn parts(&self) -> crate::part::Parts<'_> {
        crate::part::Parts::new(self)
    }

    pub fn drawables(&self) -> crate::drawable::Drawables<'_> {
        crate::drawable::Drawables::new(self)
    }

    pub fn offscreens(&self) -> crate::offscreen::OffscreenInfos<'_> {
        crate::offscreen::OffscreenInfos::new(self)
    }

    pub fn render_orders(&self) -> &[i32] {
        let n = self.drawables().len() + self.offscreens().len();
        let ptr = unsafe { ffi::csmGetRenderOrders(self.as_raw()) };
        if ptr.is_null() {
            return &[];
        }
        unsafe { std::slice::from_raw_parts(ptr, n) }
    }

    pub fn reset_dynamic_flags(&mut self) {
        unsafe { ffi::csmResetDrawableDynamicFlags(self.as_raw_mut()) }
    }
}

impl Drop for Model<'_> {
    fn drop(&mut self) {
        let layout =
            Layout::from_size_align(self.storage_size, ffi::csmAlignofModel as usize).unwrap();
        unsafe { dealloc(self.storage, layout) };
    }
}
