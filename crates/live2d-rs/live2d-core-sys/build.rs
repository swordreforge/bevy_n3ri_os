use std::env;
use std::path::PathBuf;

fn fallback_sdk_root_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("CubismSdkForNative-5-r.5")
}

fn sdk_root_dir() -> PathBuf {
    if let Ok(val) = env::var("LIVE2D_SDK_ROOT") {
        return PathBuf::from(val);
    }
    fallback_sdk_root_dir()
}

fn main() {
    // Re-run this script when the SDK location changes. Emitting any
    // rerun-if-* directive switches cargo into explicit tracking mode, so the
    // existing rerun-if-changed directive below is kept.
    println!("cargo:rerun-if-env-changed=LIVE2D_SDK_ROOT");

    let sdk_root = sdk_root_dir();
    let fallback_root = fallback_sdk_root_dir();
    let env_override = env::var("LIVE2D_SDK_ROOT").ok();

    let static_link = cfg!(feature = "static-link");

    let (lib_subdir, link_kind) = if static_link {
        ("lib", "static")
    } else {
        ("dll", "dylib")
    };

    let core_lib_dir = sdk_root
        .join("Core")
        .join(lib_subdir)
        .join("linux")
        .join("x86_64");

    let core_include = sdk_root.join("Core").join("include");
    let core_header = core_include.join("Live2DCubismCore.h");
    let core_lib = core_lib_dir.join(if static_link {
        "libLive2DCubismCore.a"
    } else {
        "libLive2DCubismCore.so"
    });

    if !sdk_root.is_dir() || !core_header.is_file() || !core_lib.is_file() {
        let env_str = env_override
            .as_deref()
            .map(|v| format!("`{v}`"))
            .unwrap_or_else(|| "(not set)".to_string());
        let check = |label: &str, ok: bool, path: &PathBuf| {
            format!(
                "    {label:<10} [{}] {}",
                if ok { "OK" } else { "MISSING" },
                path.display()
            )
        };
        panic!(
            "\nCubism SDK for Native not found or incomplete ({} linking \
             requires libLive2DCubismCore.{}).\n\
             \n\
             LIVE2D_SDK_ROOT  = {}\n\
             fallback path    = {}\n\
             resolved root    = {}\n\
             \n\
             Checked paths:\n{}\n{}\n{}\n\
             \n\
             Download the Cubism SDK for Native 5.x from\n\
             \n    https://www.live2d.com/download/cubism-sdk/download-native/\n\
             \n\
             then either extract it as `CubismSdkForNative-5-r.5/` under the\n\
             workspace root, or set LIVE2D_SDK_ROOT to the extracted SDK root.\n",
            if static_link { "static" } else { "dynamic" },
            if static_link { "a" } else { "so" },
            env_str,
            fallback_root.display(),
            sdk_root.display(),
            check("root", sdk_root.is_dir(), &sdk_root),
            check("header", core_header.is_file(), &core_header),
            check("core lib", core_lib.is_file(), &core_lib),
        );
    }

    println!("cargo:rustc-link-search=native={}", core_lib_dir.display());
    println!("cargo:rustc-link-lib={}=Live2DCubismCore", link_kind);

    if static_link {
        println!("cargo:rustc-link-lib=dylib=m");
    }

    println!(
        "cargo:rerun-if-changed={}",
        core_include.join("Live2DCubismCore.h").display()
    );

    // Generate bindings
    let bindings = bindgen::Builder::default()
        .header(core_include.join("Live2DCubismCore.h").to_string_lossy())
        .allowlist_function("csm.*")
        .allowlist_type("csm.*")
        .allowlist_var("csm.*")
        .opaque_type("csmMoc")
        .opaque_type("csmModel")
        .derive_default(true)
        .derive_debug(true)
        .use_core()
        .generate()
        .expect("bindgen failed to generate bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Could not write bindings");
}
