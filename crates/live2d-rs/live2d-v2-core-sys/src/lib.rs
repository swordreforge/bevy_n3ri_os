#![allow(
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    dead_code
)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_destroy() {
        let m = unsafe { v2_model_create() };
        assert!(!m.is_null());
        unsafe {
            v2_model_destroy(m);
        }
    }

    #[test]
    #[ignore = "requires OpenGL context"]
    fn test_gl_init() {
        let ret = unsafe { v2_gl_init() };
        eprintln!("v2_gl_init returned: {ret}");
    }

    #[test]
    #[ignore = "requires OpenGL context"]
    fn test_clear_buffer() {
        unsafe {
            v2_clear_buffer(0.0, 0.0, 0.0, 0.0);
        }
    }

    #[test]
    fn test_create_and_load_nonexistent() {
        let m = unsafe { v2_model_create() };
        assert!(!m.is_null());

        let ok = unsafe { v2_model_load_json(m, c"/nonexistent/model.json".as_ptr()) };
        assert_eq!(ok, 0, "loading nonexistent file should return 0");

        unsafe {
            v2_model_destroy(m);
        }
    }
}
