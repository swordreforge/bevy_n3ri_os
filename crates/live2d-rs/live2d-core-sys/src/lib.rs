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
    use core::ffi::CStr;

    #[test]
    fn test_get_version() {
        let version = unsafe { csmGetVersion() };
        let major = (version >> 24) & 0xFF;
        let minor = (version >> 16) & 0xFF;
        let patch = version & 0xFF;
        eprintln!(
            "Core version: {}.{}.{} (0x{version:08X})",
            major, minor, patch
        );
        assert!(version > 0, "csmGetVersion() returned 0");
    }

    #[test]
    fn test_get_latest_moc_version() {
        let ver = unsafe { csmGetLatestMocVersion() };
        eprintln!("Latest moc version: {ver}");
        assert!(ver >= csmMocVersion_53);
    }

    #[test]
    fn test_log_function_roundtrip() {
        extern "C" fn handler(msg: *const core::ffi::c_char) {
            if !msg.is_null() {
                let s = unsafe { CStr::from_ptr(msg) }.to_string_lossy();
                eprintln!("[Core log] {s}");
            }
        }
        unsafe {
            let prev = csmGetLogFunction();
            csmSetLogFunction(Some(handler));
            let cur = csmGetLogFunction();
            assert!(cur.is_some());
            csmSetLogFunction(prev);
        }
    }
}
