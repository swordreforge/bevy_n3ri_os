use glow::*;

/// Mask FBO for rendering clipped drawables.
///
/// Renders mask shapes into the alpha channel of an off-screen framebuffer,
/// then that texture is used when rendering the actual drawable.
///
/// NOTE: Full mask implementation requires multiple FBO passes per drawable.
/// This provides the infrastructure; the actual mask compositing loop
/// is in `Live2dRenderer::render_masked`.
pub struct MaskFbo {
    pub fbo: NativeFramebuffer,
    pub texture: NativeTexture,
    pub width: i32,
    pub height: i32,
}

impl MaskFbo {
    pub unsafe fn new(gl: &Context, width: i32, height: i32) -> Result<Self, String> {
        let tex = gl
            .create_texture()
            .map_err(|e| format!("mask tex: {:?}", e))?;
        gl.bind_texture(TEXTURE_2D, Some(tex));
        gl.tex_image_2d(
            TEXTURE_2D,
            0,
            RGBA as i32,
            width,
            height,
            0,
            RGBA,
            UNSIGNED_BYTE,
            None,
        );
        gl.tex_parameter_i32(TEXTURE_2D, TEXTURE_MIN_FILTER, LINEAR as i32);
        gl.tex_parameter_i32(TEXTURE_2D, TEXTURE_MAG_FILTER, LINEAR as i32);
        gl.tex_parameter_i32(TEXTURE_2D, TEXTURE_WRAP_S, CLAMP_TO_EDGE as i32);
        gl.tex_parameter_i32(TEXTURE_2D, TEXTURE_WRAP_T, CLAMP_TO_EDGE as i32);

        let fbo = gl
            .create_framebuffer()
            .map_err(|e| format!("mask fbo: {:?}", e))?;
        gl.bind_framebuffer(FRAMEBUFFER, Some(fbo));
        gl.framebuffer_texture_2d(FRAMEBUFFER, COLOR_ATTACHMENT0, TEXTURE_2D, Some(tex), 0);
        gl.bind_framebuffer(FRAMEBUFFER, None);
        gl.bind_texture(TEXTURE_2D, None);

        Ok(Self {
            fbo,
            texture: tex,
            width,
            height,
        })
    }

    pub unsafe fn resize(&mut self, gl: &Context, width: i32, height: i32) {
        if width == self.width && height == self.height {
            return;
        }
        self.width = width;
        self.height = height;
        gl.bind_texture(TEXTURE_2D, Some(self.texture));
        gl.tex_image_2d(
            TEXTURE_2D,
            0,
            RGBA as i32,
            width,
            height,
            0,
            RGBA,
            UNSIGNED_BYTE,
            None,
        );
        gl.bind_texture(TEXTURE_2D, None);
    }
}

impl Drop for MaskFbo {
    fn drop(&mut self) {
        // NOTE: requires active GL context
    }
}
