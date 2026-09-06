//! n3ri-live2d — Live2D desktop pet for the n3ri_os desktop.
//!
//! Loads a Cubism v3 model via `live2d-core` (FFI to the Cubism 5 Core C
//! API), animates it with `live2d-motion` (idle motions + physics + breath),
//! renders it into an offscreen texture, and displays that texture as a
//! centered UI node one layer below app windows.

// wgpu 深嵌套类型 auto-trait（Send/Sync）求值触发 rustc 递归上限：
// 见 n3ri-ui 同款注释（`recursion_depth_exceeding_limit` 1.100 起转 hard error）。
#![recursion_limit = "512"]

pub mod head_anim;
pub mod loader;
pub mod pet;
pub mod renderer;

/// `embed-model` feature：build.rs 生成的模型文件静态表（rel 路径 → 字节）。
#[cfg(feature = "embed-model")]
mod embed_model {
    include!(concat!(env!("OUT_DIR"), "/embedded_model.rs"));

    pub fn lookup(rel: &str) -> Option<&'static [u8]> {
        EMBEDDED_MODEL_FILES.iter().find(|(k, _)| *k == rel).map(|(_, v)| *v)
    }

    pub fn find_model3() -> Option<(String, &'static [u8])> {
        EMBEDDED_MODEL_FILES
            .iter()
            .find(|(k, _)| k.ends_with(".model3.json"))
            .map(|(k, v)| (k.to_string(), *v))
    }
}

use bevy::prelude::*;
use bevy::sprite_render::Material2dPlugin;
use bevy_tweening::TweeningPlugin;

pub use renderer::{
    spawn_head_display, spawn_pet_display, HeadDisplay, Live2dDrawableMaterial, PetDisplayImage,
    PetDisplayNode, PetDrawable, PetHeadImage, PetMapping, PetRenderConfig, PetTargetArea,
    PetViewSize,
};
pub use head_anim::HeadDisplayWanted;
pub use pet::{HeadHitArea, HeadPettingState, IdleTimer, Live2dPet, PettingState};

pub struct N3riLive2dPlugin;

impl Plugin for N3riLive2dPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<Live2dDrawableMaterial>()
            .init_resource::<PetDisplayImage>()
            .init_resource::<PetHeadImage>()
            .init_resource::<pet::IdleTimer>()
            .init_resource::<pet::HeadHitArea>()
            .init_resource::<pet::PettingState>()
            .init_resource::<pet::HeadPettingState>()
            .init_resource::<head_anim::HeadDisplayWanted>()
            .init_resource::<renderer::PetTargetArea>()
            .init_resource::<renderer::PetRenderConfig>()
            .add_plugins(Material2dPlugin::<Live2dDrawableMaterial>::default())
            .add_plugins(TweeningPlugin)
            .add_systems(Startup, renderer::load_and_setup_pet)
            .add_systems(
                Update,
                (
                    renderer::tick_pet.run_if(renderer::pet_display_on),
                    renderer::refit_pet_view,
                    renderer::gate_pet_cameras,
                    renderer::sync_live2d.run_if(renderer::pet_display_on),
                )
                    .chain(),
            )
            .add_systems(
                Update,
                (
                    pet::track_clicks,
                    pet::check_idle_timeout,
                    pet::detect_petting,
                    pet::detect_head_petting,
                )
                    .chain(),
            )
            .add_systems(
                Update,
                (
                    head_anim::head_display_anim,
                    head_anim::head_slide_out_finished,
                ),
            );
    }
}
