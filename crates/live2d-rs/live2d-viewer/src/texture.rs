use anyhow::Result;
use glow::*;
use image::GenericImageView;

pub unsafe fn load_texture(gl: &Context, data: &[u8]) -> Result<NativeTexture> {
    let img = image::load_from_memory(data).map_err(|e| anyhow::anyhow!("image load: {}", e))?;
    let (width, height) = img.dimensions();
    let rgba = img.to_rgba8();

    let tex = gl
        .create_texture()
        .map_err(|e| anyhow::anyhow!("create texture: {:?}", e))?;
    gl.bind_texture(TEXTURE_2D, Some(tex));
    gl.tex_parameter_i32(TEXTURE_2D, TEXTURE_MIN_FILTER, LINEAR_MIPMAP_LINEAR as i32);
    gl.tex_parameter_i32(TEXTURE_2D, TEXTURE_MAG_FILTER, LINEAR as i32);
    gl.tex_parameter_i32(TEXTURE_2D, TEXTURE_WRAP_S, CLAMP_TO_EDGE as i32);
    gl.tex_parameter_i32(TEXTURE_2D, TEXTURE_WRAP_T, CLAMP_TO_EDGE as i32);
    gl.tex_image_2d(
        TEXTURE_2D,
        0,
        RGBA as i32,
        width as i32,
        height as i32,
        0,
        RGBA,
        UNSIGNED_BYTE,
        Some(&rgba),
    );
    gl.generate_mipmap(TEXTURE_2D);
    gl.bind_texture(TEXTURE_2D, None);
    Ok(tex)
}
