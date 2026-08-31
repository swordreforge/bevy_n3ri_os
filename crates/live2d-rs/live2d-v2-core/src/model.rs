use live2d_v2_core_sys as ffi;
use std::ffi::{c_char, CStr, CString};

/// Error type for V2 model operations.
#[derive(Debug)]
pub enum Error {
    CreateFailed,
    LoadFailed,
    NullPointer(&'static str),
    IndexOutOfBounds { index: usize, count: usize },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CreateFailed => write!(f, "V2 model creation failed"),
            Self::LoadFailed => write!(f, "V2 model load failed"),
            Self::NullPointer(s) => write!(f, "null pointer: {s}"),
            Self::IndexOutOfBounds { index, count } => {
                write!(f, "index {index} out of bounds (count={count})")
            }
        }
    }
}
impl std::error::Error for Error {}

/// Result alias for V2 model operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Safe wrapper around a Live2D Cubism 2.x model.
///
/// ```no_run
/// use live2d_v2_core::Model;
///
/// let mut model = Model::new().expect("create model");
/// model.load_json("/path/to/model.json").expect("load model");
/// model.resize(800, 600);
/// model.update();
/// // ... render frame ...
/// model.draw();
/// // model is destroyed on drop
/// ```
pub struct Model {
    raw: *mut ffi::V2Model,
}

// SAFETY: the raw *mut V2Model handle is owned exclusively and never
// accessed through shared references from other threads, so moving the
// wrapper between threads is safe.
//
// `Sync` is deliberately NOT implemented: the C++ V2Model keeps internal
// mutable scratch state (`last_str` / `last_str_vec` buffers) that is
// rewritten even by const-qualified getters (param_id, hit_part, ...).
// Concurrent calls through &self would be a data race.
unsafe impl Send for Model {}

impl Model {
    /// Create a new V2 model instance.
    pub fn new() -> Result<Self> {
        let raw = unsafe { ffi::v2_model_create() };
        if raw.is_null() {
            return Err(Error::CreateFailed);
        }
        Ok(Self { raw })
    }

    // ──────── Loading ────────

    /// Load a model from a `.model.json` file.
    pub fn load_json(&mut self, path: &str) -> Result<()> {
        let cpath = CString::new(path).map_err(|_| Error::LoadFailed)?;
        let ok = unsafe { ffi::v2_model_load_json(self.raw, cpath.as_ptr()) };
        if ok == 0 {
            return Err(Error::LoadFailed);
        }
        Ok(())
    }

    // ──────── Viewport ────────

    pub fn resize(&mut self, w: i32, h: i32) {
        unsafe { ffi::v2_model_resize(self.raw, w, h) }
    }
    pub fn set_offset(&mut self, dx: f32, dy: f32) {
        unsafe { ffi::v2_model_set_offset(self.raw, dx, dy) }
    }
    pub fn set_scale(&mut self, s: f32) {
        unsafe { ffi::v2_model_set_scale(self.raw, s) }
    }
    pub fn rotate(&mut self, deg: f32) {
        unsafe { ffi::v2_model_rotate(self.raw, deg) }
    }

    // ──────── Interaction ────────

    pub fn drag(&mut self, x: f32, y: f32) {
        unsafe { ffi::v2_model_drag(self.raw, x, y) }
    }
    pub fn is_motion_finished(&self) -> bool {
        unsafe { ffi::v2_model_is_motion_finished(self.raw) != 0 }
    }

    // ──────── Motion info ────────

    /// Returns the motion group selected by the last `start_random_motion` call.
    pub fn current_group(&self) -> String {
        let ptr = unsafe { ffi::v2_model_get_current_group(self.raw) };
        if ptr.is_null() {
            return String::new();
        }
        unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned()
    }

    /// Returns the motion number selected by the last `start_random_motion` call.
    pub fn current_no(&self) -> i32 {
        unsafe { ffi::v2_model_get_current_no(self.raw) }
    }

    // ──────── Parameters ────────

    pub fn param_count(&self) -> i32 {
        unsafe { ffi::v2_model_get_param_count(self.raw) }
    }
    pub fn param_value(&self, index: i32) -> f32 {
        unsafe { ffi::v2_model_get_param_value(self.raw, index) }
    }
    pub fn param_min(&self, index: i32) -> f32 {
        unsafe { ffi::v2_model_get_param_min(self.raw, index) }
    }
    pub fn param_max(&self, index: i32) -> f32 {
        unsafe { ffi::v2_model_get_param_max(self.raw, index) }
    }
    pub fn param_default(&self, index: i32) -> f32 {
        unsafe { ffi::v2_model_get_param_default(self.raw, index) }
    }
    pub fn param_id(&self, index: i32) -> Result<String> {
        let ptr = unsafe { ffi::v2_model_get_param_id(self.raw, index) };
        if ptr.is_null() {
            return Err(Error::NullPointer("param_id"));
        }
        Ok(unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned())
    }
    pub fn set_param_value(&mut self, id: &str, val: f32, weight: f32) {
        let cid = CString::new(id).unwrap();
        unsafe { ffi::v2_model_set_param_value(self.raw, cid.as_ptr(), val, weight) }
    }
    pub fn add_param_value(&mut self, id: &str, val: f32, weight: f32) {
        let cid = CString::new(id).unwrap();
        unsafe { ffi::v2_model_add_param_value(self.raw, cid.as_ptr(), val, weight) }
    }

    // ──────── Parts ────────

    pub fn part_count(&self) -> i32 {
        unsafe { ffi::v2_model_get_part_count(self.raw) }
    }
    pub fn part_id(&self, index: i32) -> Result<String> {
        let ptr = unsafe { ffi::v2_model_get_part_id(self.raw, index) };
        if ptr.is_null() {
            return Err(Error::NullPointer("part_id"));
        }
        Ok(unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned())
    }
    pub fn set_part_opacity(&mut self, index: i32, val: f32) {
        unsafe { ffi::v2_model_set_part_opacity(self.raw, index, val) }
    }

    // ──────── Motion / Expression ────────

    pub fn start_motion(&mut self, group: &str, no: i32, priority: i32) {
        let cg = CString::new(group).unwrap();
        unsafe { ffi::v2_model_start_motion(self.raw, cg.as_ptr(), no, priority) }
    }
    pub fn start_random_motion(&mut self, group: &str, priority: i32) {
        let cg = CString::new(group).unwrap();
        unsafe { ffi::v2_model_start_random_motion(self.raw, cg.as_ptr(), priority) }
    }
    pub fn clear_motions(&mut self) {
        unsafe { ffi::v2_model_clear_motions(self.raw) }
    }
    pub fn reset_pose(&mut self) {
        unsafe { ffi::v2_model_reset_pose(self.raw) }
    }
    pub fn set_expression(&mut self, name: &str) {
        let cn = CString::new(name).unwrap();
        unsafe { ffi::v2_model_set_expression(self.raw, cn.as_ptr()) }
    }
    pub fn set_random_expression(&mut self) {
        unsafe { ffi::v2_model_set_random_expression(self.raw) }
    }
    pub fn reset_expression(&mut self) {
        unsafe { ffi::v2_model_reset_expression(self.raw) }
    }

    // ──────── Auto ────────

    pub fn auto_breath(&self) -> bool {
        unsafe { ffi::v2_model_get_auto_breath(self.raw) != 0 }
    }
    pub fn set_auto_breath(&mut self, v: bool) {
        unsafe { ffi::v2_model_set_auto_breath(self.raw, v as i32) }
    }
    pub fn auto_blink(&self) -> bool {
        unsafe { ffi::v2_model_get_auto_blink(self.raw) != 0 }
    }
    pub fn set_auto_blink(&mut self, v: bool) {
        unsafe { ffi::v2_model_set_auto_blink(self.raw, v as i32) }
    }

    // ──────── Update / Draw ────────

    pub fn update(&mut self) {
        unsafe { ffi::v2_model_update(self.raw) }
    }
    pub fn draw(&mut self) {
        unsafe { ffi::v2_model_draw(self.raw) }
    }

    // ──────── Canvas ────────

    pub fn canvas_width(&self) -> f32 {
        unsafe { ffi::v2_model_get_canvas_width(self.raw) }
    }
    pub fn canvas_height(&self) -> f32 {
        unsafe { ffi::v2_model_get_canvas_height(self.raw) }
    }
    pub fn pixels_per_unit(&self) -> i32 {
        unsafe { ffi::v2_model_get_pixels_per_unit(self.raw) }
    }

    // ──────── Hit test ────────

    /// Returns true if the named hit area was hit.
    pub fn hit_test(&self, area: &str, x: f32, y: f32) -> bool {
        let ca = CString::new(area).unwrap();
        unsafe { ffi::v2_model_hit_test(self.raw, ca.as_ptr(), x, y) != 0 }
    }

    /// Returns IDs of parts hit at the given screen coordinates.
    /// `top_only` restricts to the topmost part.
    pub fn hit_part(&self, x: f32, y: f32, top_only: bool) -> Vec<String> {
        let mut out_ids: *mut *const c_char = std::ptr::null_mut();
        let mut out_count: i32 = 0;
        let n = unsafe {
            ffi::v2_model_hit_part(
                self.raw,
                x,
                y,
                top_only as i32,
                &mut out_ids as *mut *mut *const c_char,
                &mut out_count,
            )
        };
        if n <= 0 {
            return Vec::new();
        }
        let slice = unsafe { std::slice::from_raw_parts(out_ids, out_count as usize) };
        slice
            .iter()
            .map(|&p| {
                unsafe { CStr::from_ptr(p as *const c_char) }
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }

    // ──────── Part color ────────

    pub fn set_part_screen_color(&mut self, idx: i32, r: f32, g: f32, b: f32, a: f32) {
        unsafe { ffi::v2_model_set_part_screen_color(self.raw, idx, r, g, b, a) }
    }
    pub fn set_part_multiply_color(&mut self, idx: i32, r: f32, g: f32, b: f32, a: f32) {
        unsafe { ffi::v2_model_set_part_multiply_color(self.raw, idx, r, g, b, a) }
    }
    pub fn part_screen_color(&self, idx: i32) -> [f32; 4] {
        let mut out = [0.0f32; 4];
        unsafe { ffi::v2_model_get_part_screen_color(self.raw, idx, out.as_mut_ptr()) }
        out
    }
    pub fn part_multiply_color(&self, idx: i32) -> [f32; 4] {
        let mut out = [0.0f32; 4];
        unsafe { ffi::v2_model_get_part_multiply_color(self.raw, idx, out.as_mut_ptr()) }
        out
    }

    // ──────── Texture ────────

    pub fn set_texture(&mut self, no: i32, tex_id: i32) {
        unsafe { ffi::v2_model_set_texture(self.raw, no, tex_id) }
    }
}

impl Drop for Model {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe { ffi::v2_model_destroy(self.raw) }
        }
    }
}

// ──────── Module-level functions ────────

/// Initialize OpenGL function pointers via glad.
/// Requires an active OpenGL context. Returns 0 on failure.
pub fn gl_init() -> i32 {
    unsafe { ffi::v2_gl_init() }
}

/// Clear the color buffer with the given RGBA values.
pub fn clear_buffer(r: f32, g: f32, b: f32, a: f32) {
    unsafe { ffi::v2_clear_buffer(r, g, b, a) }
}
