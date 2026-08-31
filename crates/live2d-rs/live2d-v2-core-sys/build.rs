use std::env;
use std::path::PathBuf;

fn main() {
    // Re-run this script when the V2 artifact location changes. Emitting any
    // rerun-if-* directive switches cargo into explicit tracking mode, so the
    // existing rerun-if-changed directive below is kept.
    println!("cargo:rerun-if-env-changed=V2_PY_BUILD_DIR");

    // Path to the live2d-py build artifacts
    // Use V2_PY_BUILD_DIR env var, or derive relative to this source tree
    let env_override = env::var("V2_PY_BUILD_DIR").ok();
    // Default: assume live2d-py lives next to the Rust workspace root
    // CARGO_MANIFEST_DIR = .../live2d-rs/live2d-v2-core-sys
    // parent = .../live2d-rs  (workspace root, sibling to live2d-py/)
    let fallback_py_build_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
        .parent()
        .unwrap()
        .join("live2d-py")
        .join("build");
    let py_build_dir = env_override
        .clone()
        .map(PathBuf::from)
        .unwrap_or_else(|| fallback_py_build_dir.clone());

    // Library search path
    let glad_dir = py_build_dir.join("Live2D").join("Glad");
    let v2_dir = py_build_dir.join("Live2D").join("V2");
    let c_api_dir = py_build_dir.join("v2_c_api");

    // Header path for bindgen (lives in the live2d-py source tree, one level
    // above the build dir)
    let header = py_build_dir
        .parent()
        .unwrap()
        .join("v2_c_api")
        .join("v2_c_api.h");

    let glad_lib = glad_dir.join("libglad.a");
    let v2_lib = v2_dir.join("libV2.a");
    let c_api_lib = c_api_dir.join("libv2_c_api.a");

    if !py_build_dir.is_dir()
        || !glad_lib.is_file()
        || !v2_lib.is_file()
        || !c_api_lib.is_file()
        || !header.is_file()
    {
        let env_str = env_override
            .as_deref()
            .map(|v| format!("`{v}`"))
            .unwrap_or_else(|| "(not set)".to_string());
        let check = |label: &str, ok: bool, path: &PathBuf| {
            format!(
                "    {label:<16} [{}] {}",
                if ok { "OK" } else { "MISSING" },
                path.display()
            )
        };
        panic!(
            "\nlive2d-py build artifacts not found or incomplete.\n\
             \n\
             V2_PY_BUILD_DIR = {}\n\
             fallback path   = {}\n\
             resolved dir    = {}\n\
             \n\
             Checked paths:\n{}\n{}\n{}\n{}\n{}\n\
             \n\
             Build the live2d-py submodule first:\n\
             \n    cd live2d-py && mkdir -p build && cd build && cmake .. && make\n\
             \n\
             or set V2_PY_BUILD_DIR to the directory containing the `Live2D/`\n\
             and `v2_c_api/` build outputs.\n",
            env_str,
            fallback_py_build_dir.display(),
            py_build_dir.display(),
            check("build dir", py_build_dir.is_dir(), &py_build_dir),
            check("libglad.a", glad_lib.is_file(), &glad_lib),
            check("libV2.a", v2_lib.is_file(), &v2_lib),
            check("libv2_c_api.a", c_api_lib.is_file(), &c_api_lib),
            check("v2_c_api.h", header.is_file(), &header),
        );
    }

    println!("cargo:rustc-link-search=native={}", glad_dir.display());
    println!("cargo:rustc-link-search=native={}", v2_dir.display());
    println!("cargo:rustc-link-search=native={}", c_api_dir.display());

    // Link libraries
    println!("cargo:rustc-link-lib=static=v2_c_api");
    println!("cargo:rustc-link-lib=static=V2");
    println!("cargo:rustc-link-lib=static=glad");

    // System libraries needed on Linux
    if cfg!(target_os = "linux") {
        println!("cargo:rustc-link-lib=dylib=GL");
        println!("cargo:rustc-link-lib=dylib=stdc++fs");
        println!("cargo:rustc-link-lib=dylib=stdc++");
        println!("cargo:rustc-link-lib=dylib=m");
    }
    if cfg!(target_os = "macos") {
        println!("cargo:rustc-link-lib=dylib=c++");
        println!("cargo:rustc-link-lib=dylib=framework=OpenGL");
    }

    println!("cargo:rerun-if-changed={}", header.display());

    // Generate bindings
    let bindings = bindgen::Builder::default()
        .header(header.to_string_lossy())
        .allowlist_function("v2_.*")
        .allowlist_type("V2Model")
        .opaque_type("V2Model")
        .derive_default(true)
        .derive_debug(true)
        .use_core()
        .generate()
        .expect("bindgen failed to generate V2 C API bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Could not write V2 bindings");
}
