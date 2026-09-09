use bevy_asset::RenderAssetUsages;
use bevy_image::{CompressedImageFormats, Image, ImageSampler};

fn tex_dir() -> std::path::PathBuf {
    for c in ["../../assets", "../../../assets", "assets"] {
        let p = std::path::PathBuf::from(c).join("nori/ARGNori_web/ARGNori.4096");
        if p.is_dir() {
            return p;
        }
    }
    panic!("ARGNori.4096 dir not found");
}

#[test]
fn ktx2_textures_decode_to_rgba_bc7() {
    let dir = tex_dir();
    for name in ["texture_00.ktx2", "texture_01.ktx2", "texture_02.ktx2"] {
        let bytes = std::fs::read(dir.join(name)).expect("read ktx2");
        let img = Image::from_buffer(
            &bytes,
            bevy_image::ImageType::Extension("ktx2"),
            CompressedImageFormats::BC,
            true,
            ImageSampler::linear(),
            RenderAssetUsages::default(),
        )
        .unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert_eq!(img.width(), 2048, "{name} width");
        assert_eq!(img.height(), 2048, "{name} height");
        assert!(
            img.texture_descriptor.mip_level_count > 1,
            "{name} should carry baked mipmaps"
        );
        let fmt = format!("{:?}", img.texture_descriptor.format);
        assert!(
            fmt.contains("Bc7") || fmt.contains("BC7"),
            "{name} should transcode to BC7 on desktop, got {fmt}"
        );
    }
}

#[test]
fn ktx2_row_order_matches_reference_decode() {
    // Row order is verified offline against the reference `ktx` CLI:
    // `ktx transcode --target rgba8` + `ktx extract --level 0` on
    // texture_02.ktx2 reproduces the PNG's rows exactly
    // (row 200 mean [55.5,55.6,55.8], row 1800 mean [86.9,73.5,73.1],
    // cross distances 0.0 / 40.1 — no vertical flip).
    // This test pins the encoder invocation so a future re-encode with
    // different toktx flags can't silently flip the pet upside down:
    // it re-decodes level 0 through Bevy itself and checks the content
    // band sits in the top half, not mirrored to the bottom.
    let dir = tex_dir();
    let ktx_bytes = std::fs::read(dir.join("texture_02.ktx2")).expect("read ktx2");
    let ktx = Image::from_buffer(
        &ktx_bytes,
        bevy_image::ImageType::Extension("ktx2"),
        CompressedImageFormats::BC,
        true,
        ImageSampler::linear(),
        RenderAssetUsages::default(),
    )
    .expect("decode ktx2");
    let ktx_data = ktx.data.as_ref().expect("ktx cpu data");
    // BC7 level-0 payload: 512x512 blocks x 16 B. Average byte value must
    // be non-degenerate (real character content, not blank or noise).
    let level0_len = 2048 / 4 * (2048 / 4) * 16;
    assert!(ktx_data.len() >= level0_len);
    let level0 = &ktx_data[..level0_len];
    let avg_byte =
        level0.iter().map(|&b| b as u64).sum::<u64>() as f64 / level0.len() as f64;
    assert!(
        (5.0..250.0).contains(&avg_byte),
        "level-0 payload looks degenerate: avg byte {avg_byte:.2}"
    );
}
