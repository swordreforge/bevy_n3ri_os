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
    // Re-run this script when the SDK location changes (matters for the
    // dynamic-link rpath below).
    println!("cargo:rerun-if-env-changed=LIVE2D_SDK_ROOT");

    let static_link = cfg!(feature = "static-link");
    if !static_link {
        let sdk_root = sdk_root_dir();
        let fallback_root = fallback_sdk_root_dir();
        let env_override = env::var("LIVE2D_SDK_ROOT").ok();
        let core_lib_dir = sdk_root
            .join("Core")
            .join("dll")
            .join("linux")
            .join("x86_64");
        let core_lib = core_lib_dir.join("libLive2DCubismCore.so");

        if !sdk_root.is_dir() || !core_lib.is_file() {
            let env_str = env_override
                .as_deref()
                .map(|v| format!("`{v}`"))
                .unwrap_or_else(|| "(not set)".to_string());
            let check = |label: &str, ok: bool, path: &PathBuf| {
                format!(
                    "    {label:<12} [{}] {}",
                    if ok { "OK" } else { "MISSING" },
                    path.display()
                )
            };
            panic!(
                "\nCubism SDK for Native not found or incomplete (dynamic linking \
                 needs libLive2DCubismCore.so for -rpath).\n\
                 \n\
                 LIVE2D_SDK_ROOT  = {}\n\
                 fallback path    = {}\n\
                 resolved root    = {}\n\
                 \n\
                 Checked paths:\n{}\n{}\n\
                 \n\
                 Download the Cubism SDK for Native 5.x from\n\
                 \n    https://www.live2d.com/download/cubism-sdk/download-native/\n\
                 \n\
                 then either extract it as `CubismSdkForNative-5-r.5/` under the\n\
                 workspace root, or set LIVE2D_SDK_ROOT to the extracted SDK root.\n",
                env_str,
                fallback_root.display(),
                sdk_root.display(),
                check("root", sdk_root.is_dir(), &sdk_root),
                check("core .so", core_lib.is_file(), &core_lib),
            );
        }

        println!(
            "cargo:rustc-link-arg-bin=live2d-viewer=-Wl,-rpath,{}",
            core_lib_dir.display()
        );
    }
}
