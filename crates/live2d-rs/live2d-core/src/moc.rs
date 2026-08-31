use crate::error::{Error, Result};
use live2d_core_sys as ffi;
use std::alloc::{alloc, dealloc, Layout};
use std::ptr::NonNull;

/// A revived Live2D Cubism Moc.
///
/// Created from `.moc3` binary data.  The internal buffer is
/// guaranteed to be 64-byte aligned (`csmAlignofMoc`).
pub struct Moc {
    data: *mut u8,
    size: usize,
    raw: NonNull<ffi::csmMoc>,
}

unsafe impl Send for Moc {}
unsafe impl Sync for Moc {}

impl Moc {
    pub fn revive(bytes: &[u8]) -> Result<Self> {
        if bytes.is_empty() {
            return Err(Error::InvalidInput("empty moc3 data"));
        }

        let layout = Layout::from_size_align(bytes.len(), ffi::csmAlignofMoc as usize)
            .map_err(|_| Error::InvalidInput("alignment overflow"))?;
        let data = unsafe { alloc(layout) };
        if data.is_null() {
            return Err(Error::InvalidInput("alloc returned null"));
        }
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), data, bytes.len());
        }

        let consistent = unsafe { ffi::csmHasMocConsistency(data as *mut _, bytes.len() as u32) };
        if consistent == 0 {
            unsafe { dealloc(data, layout) };
            return Err(Error::InvalidMoc);
        }

        let raw = unsafe { ffi::csmReviveMocInPlace(data as *mut _, bytes.len() as u32) };
        if raw.is_null() {
            unsafe { dealloc(data, layout) };
            return Err(Error::ReviveFailed);
        }

        Ok(Self {
            data,
            size: bytes.len(),
            raw: unsafe { NonNull::new_unchecked(raw) },
        })
    }

    pub(crate) fn as_raw(&self) -> *const ffi::csmMoc {
        self.raw.as_ptr()
    }

    pub fn sizeof_model(&self) -> u32 {
        unsafe { ffi::csmGetSizeofModel(self.raw.as_ptr()) }
    }

    pub fn moc_version(bytes: &[u8]) -> ffi::csmMocVersion {
        unsafe { ffi::csmGetMocVersion(bytes.as_ptr() as *const _, bytes.len() as u32) }
    }
}

impl Drop for Moc {
    fn drop(&mut self) {
        let layout = Layout::from_size_align(self.size, ffi::csmAlignofMoc as usize).unwrap();
        unsafe { dealloc(self.data, layout) };
    }
}
