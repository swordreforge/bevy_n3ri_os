//! Text renderer for the sctk layer-shell overlay.
//!
//! Uses `ab_glyph` to rasterise TrueType glyphs on demand into a
//! dynamic texture atlas.  Anti-aliased, any font, any size — no more
//! hand-crafted 5×7 pixel font.
//!
//! Tries to load a system monospace font (DejaVu / Liberation / Noto).
//! Falls back to … well, it doesn't.  Fix your font installation.  ;)

use std::collections::HashMap;
use std::path::Path;

use ab_glyph::{Font, FontVec, PxScale, ScaleFont};
use glow::*;

// ---------------------------------------------------------------------------
// Shader sources
// ---------------------------------------------------------------------------

const VS_SRC: &str = r#"#version 330 core
layout(location=0) in vec2 aPos;
layout(location=1) in vec2 aTexCoord;
layout(location=2) in vec4 aColor;
out vec2 vTexCoord;
out vec4 vColor;
void main() {
    gl_Position = vec4(aPos, 0.0, 1.0);
    vTexCoord = aTexCoord;
    vColor = aColor;
}"#;

const FS_SRC: &str = r#"#version 330 core
in vec2 vTexCoord;
in vec4 vColor;
out vec4 FragColor;
uniform sampler2D uTexture;
void main() {
    float a = texture(uTexture, vTexCoord).r;
    FragColor = vec4(vColor.rgb, vColor.a * a);
}"#;

// ---------------------------------------------------------------------------
// Atlas
// ---------------------------------------------------------------------------

const ATLAS_SIZE: u32 = 512;
const GLYPH_PAD: u32 = 2;

#[derive(Clone, Copy)]
struct GlyphSlot {
    atlas_x: u32,
    atlas_y: u32,
    width: u32,
    height: u32,
    advance: f32,
    bearing_x: f32,
    bearing_y: f32,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn ndc(sx: f32, sy: f32, vp_w: f32, vp_h: f32) -> [f32; 2] {
    [(sx / vp_w) * 2.0 - 1.0, 1.0 - (sy / vp_h) * 2.0]
}

unsafe fn compile_program(gl: &Context) -> Result<NativeProgram, String> {
    let program = gl
        .create_program()
        .map_err(|e| format!("create program: {e}"))?;

    let vs = gl
        .create_shader(VERTEX_SHADER)
        .map_err(|e| format!("create vs: {e}"))?;
    gl.shader_source(vs, VS_SRC);
    gl.compile_shader(vs);
    if !gl.get_shader_compile_status(vs) {
        return Err(format!("vs compile: {}", gl.get_shader_info_log(vs)));
    }

    let fs = gl
        .create_shader(FRAGMENT_SHADER)
        .map_err(|e| format!("create fs: {e}"))?;
    gl.shader_source(fs, FS_SRC);
    gl.compile_shader(fs);
    if !gl.get_shader_compile_status(fs) {
        return Err(format!("fs compile: {}", gl.get_shader_info_log(fs)));
    }

    gl.attach_shader(program, vs);
    gl.attach_shader(program, fs);
    gl.link_program(program);
    if !gl.get_program_link_status(program) {
        return Err(format!("link: {}", gl.get_program_info_log(program)));
    }

    gl.delete_shader(vs);
    gl.delete_shader(fs);
    Ok(program)
}

fn find_monospace_font() -> Result<Vec<u8>, String> {
    let candidates = [
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
        "/usr/share/fonts/truetype/noto/NotoMono-Regular.ttf",
        "/usr/share/fonts/noto/NotoMono-Regular.ttf",
    ];
    for p in &candidates {
        if Path::new(p).exists() {
            return std::fs::read(p).map_err(|e| format!("read {p}: {e}"));
        }
    }
    Err("no monospace font found".into())
}

// ---------------------------------------------------------------------------
// TextRenderer
// ---------------------------------------------------------------------------

pub struct TextRenderer {
    program: NativeProgram,
    vao: NativeVertexArray,
    vbo: NativeBuffer,
    font_tex: NativeTexture,
    font: FontVec,
    font_size: f32,
    cache: HashMap<char, GlyphSlot>,
    cursor_x: u32,
    cursor_y: u32,
    row_h: u32,
    line_h: f32,
}

impl TextRenderer {
    /// Create the renderer with a system monospace font at `px_size` pixels.
    ///
    /// # Safety
    ///
    /// Requires an active GL context.
    pub unsafe fn new(gl: &Context, px_size: f32) -> Result<Self, String> {
        let font_data = find_monospace_font()?;
        let font = FontVec::try_from_vec(font_data).map_err(|e| format!("parse font: {e:?}"))?;

        let font_scaled = font.as_scaled(PxScale::from(px_size));
        let line_h = font_scaled.height() + font_scaled.line_gap();

        // --- Atlas texture ---
        let font_tex = gl
            .create_texture()
            .map_err(|e| format!("create tex: {e}"))?;
        gl.bind_texture(TEXTURE_2D, Some(font_tex));
        gl.tex_parameter_i32(TEXTURE_2D, TEXTURE_MIN_FILTER, LINEAR as i32);
        gl.tex_parameter_i32(TEXTURE_2D, TEXTURE_MAG_FILTER, LINEAR as i32);
        gl.tex_parameter_i32(TEXTURE_2D, TEXTURE_WRAP_S, CLAMP_TO_EDGE as i32);
        gl.tex_parameter_i32(TEXTURE_2D, TEXTURE_WRAP_T, CLAMP_TO_EDGE as i32);
        // Pre-allocate RGBA atlas
        gl.tex_image_2d(
            TEXTURE_2D,
            0,
            RGBA as i32,
            ATLAS_SIZE as i32,
            ATLAS_SIZE as i32,
            0,
            RGBA,
            UNSIGNED_BYTE,
            None,
        );

        // --- Shader ---
        let program = compile_program(gl)?;

        // --- VAO / VBO ---
        let vao = gl.create_vertex_array().map_err(|e| format!("vao: {e}"))?;
        let vbo = gl.create_buffer().map_err(|e| format!("vbo: {e}"))?;

        gl.bind_vertex_array(Some(vao));
        gl.bind_buffer(ARRAY_BUFFER, Some(vbo));
        let stride: i32 = 32; // pos2 + uv2 + color4 = 8 × f32
        gl.vertex_attrib_pointer_f32(0, 2, FLOAT, false, stride, 0);
        gl.enable_vertex_attrib_array(0);
        gl.vertex_attrib_pointer_f32(1, 2, FLOAT, false, stride, 8);
        gl.enable_vertex_attrib_array(1);
        gl.vertex_attrib_pointer_f32(2, 4, FLOAT, false, stride, 16);
        gl.enable_vertex_attrib_array(2);
        gl.bind_vertex_array(None);
        gl.bind_texture(TEXTURE_2D, None);
        gl.use_program(None);

        Ok(Self {
            program,
            vao,
            vbo,
            font_tex,
            font,
            font_size: px_size,
            cache: HashMap::new(),
            cursor_x: 0,
            cursor_y: 0,
            row_h: 0,
            line_h,
        })
    }

    /// Rasterise `ch` into the atlas if not already cached.
    /// Returns the glyph slot (atlas position / metrics).
    fn get_or_rasterise(&mut self, gl: &Context, ch: char) -> Option<GlyphSlot> {
        if let Some(&s) = self.cache.get(&ch) {
            return Some(s);
        }

        let scale = PxScale::from(self.font_size);
        let glyph_id = self.font.glyph_id(ch);
        let outlined = self.font.outline_glyph(glyph_id.with_scale(scale))?;
        let bounds = outlined.px_bounds();

        let w = bounds.width().ceil() as u32;
        let h = bounds.height().ceil() as u32;
        let advance = self.font.as_scaled(scale).h_advance(glyph_id);

        if w == 0 || h == 0 {
            let slot = GlyphSlot {
                atlas_x: 0,
                atlas_y: 0,
                width: 0,
                height: 0,
                advance,
                bearing_x: 0.0,
                bearing_y: 0.0,
            };
            self.cache.insert(ch, slot);
            return Some(slot);
        }

        // Atlas packing: simple left-to-right, top-to-bottom
        if self.cursor_x + w + GLYPH_PAD * 2 > ATLAS_SIZE {
            self.cursor_x = 0;
            self.cursor_y += self.row_h + GLYPH_PAD;
            self.row_h = 0;
        }

        // If atlas is full, just skip new glyphs
        if self.cursor_y + h + GLYPH_PAD * 2 > ATLAS_SIZE {
            log::warn!("[text_renderer] atlas full — skipping glyph '{}'", ch);
            self.cache.insert(
                ch,
                GlyphSlot {
                    atlas_x: 0,
                    atlas_y: 0,
                    width: 0,
                    height: 0,
                    advance,
                    bearing_x: 0.0,
                    bearing_y: 0.0,
                },
            );
            return None;
        }

        let ax = self.cursor_x + GLYPH_PAD;
        let ay = self.cursor_y + GLYPH_PAD;

        // Rasterise to RGBA bitmap
        let mut bitmap = vec![0u8; (w * h * 4) as usize];
        outlined.draw(|px, py, c| {
            let idx = (py as usize * w as usize + px as usize) * 4;
            let alpha = (c * 255.0).clamp(0.0, 255.0) as u8;
            // Put coverage in all channels; fragment shader samples .r
            bitmap[idx] = alpha;
            bitmap[idx + 1] = alpha;
            bitmap[idx + 2] = alpha;
            bitmap[idx + 3] = alpha;
        });

        // Upload sub-region
        unsafe {
            gl.bind_texture(TEXTURE_2D, Some(self.font_tex));
            gl.tex_sub_image_2d(
                TEXTURE_2D,
                0,
                ax as i32,
                ay as i32,
                w as i32,
                h as i32,
                RGBA,
                UNSIGNED_BYTE,
                PixelUnpackData::Slice(&bitmap),
            );
        }

        let slot = GlyphSlot {
            atlas_x: ax,
            atlas_y: ay,
            width: w,
            height: h,
            advance,
            bearing_x: bounds.min.x,
            bearing_y: bounds.min.y,
        };
        self.cache.insert(ch, slot);

        self.cursor_x = ax + w;
        self.row_h = self.row_h.max(h);

        self.cache.get(&ch).copied()
    }

    /// Draw a string at screen coordinates `(x, y)`.
    ///
    /// `y` is the *baseline* position (not top-left).
    /// `alpha_mult` multiplies the per-glyph colour alpha.
    ///
    /// # Safety
    ///
    /// Requires an active GL context.  The caller should enable blending
    /// with `gl.blend_func(SRC_ALPHA, ONE_MINUS_SRC_ALPHA)`.
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn draw_text(
        &mut self,
        gl: &Context,
        text: &str,
        mut x: f32,
        y: f32,
        color: [f32; 4],
        vp_w: u32,
        vp_h: u32,
        alpha_mult: f32,
    ) {
        let vpw = vp_w as f32;
        let vph = vp_h as f32;
        let atlas_f = ATLAS_SIZE as f32;
        let a = color[3] * alpha_mult;

        let mut verts: Vec<f32> = Vec::new();
        let start_x = x;

        for ch in text.chars() {
            if ch == '\n' {
                x = start_x;
                continue; // no multi-line for this panel
            }

            let slot = match self.get_or_rasterise(gl, ch) {
                Some(s) => s,
                None => {
                    x += self.line_h * 0.3;
                    continue;
                }
            };

            if slot.width == 0 || slot.height == 0 {
                x += slot.advance;
                continue;
            }

            // Quad position (pixel-accurate, anchored at bearing baseline)
            let qx = x + slot.bearing_x;
            let qy = y + slot.bearing_y;
            let qw = slot.width as f32;
            let qh = slot.height as f32;

            let u0 = slot.atlas_x as f32 / atlas_f;
            let v0 = slot.atlas_y as f32 / atlas_f;
            let u1 = (slot.atlas_x + slot.width) as f32 / atlas_f;
            let v1 = (slot.atlas_y + slot.height) as f32 / atlas_f;

            let [tlx, tly] = ndc(qx, qy, vpw, vph);
            let [trx, try_] = ndc(qx + qw, qy, vpw, vph);
            let [blx, bly] = ndc(qx, qy + qh, vpw, vph);
            let [brx, bry] = ndc(qx + qw, qy + qh, vpw, vph);

            // Triangle 1: TL, TR, BL
            verts.extend_from_slice(&[
                tlx, tly, u0, v0, color[0], color[1], color[2], a, trx, try_, u1, v0, color[0],
                color[1], color[2], a, blx, bly, u0, v1, color[0], color[1], color[2], a,
            ]);
            // Triangle 2: TR, BR, BL
            verts.extend_from_slice(&[
                trx, try_, u1, v0, color[0], color[1], color[2], a, brx, bry, u1, v1, color[0],
                color[1], color[2], a, blx, bly, u0, v1, color[0], color[1], color[2], a,
            ]);

            x += slot.advance;
        }

        if verts.is_empty() {
            return;
        }

        gl.bind_vertex_array(Some(self.vao));
        gl.bind_buffer(ARRAY_BUFFER, Some(self.vbo));
        let bytes = std::slice::from_raw_parts(verts.as_ptr() as *const u8, verts.len() * 4);
        gl.buffer_data_u8_slice(ARRAY_BUFFER, bytes, STREAM_DRAW);

        gl.use_program(Some(self.program));
        gl.active_texture(0);
        gl.bind_texture(TEXTURE_2D, Some(self.font_tex));
        if let Some(loc) = gl.get_uniform_location(self.program, "uTexture") {
            gl.uniform_1_i32(Some(&loc), 0);
        }

        gl.draw_arrays(TRIANGLES, 0, verts.len() as i32 / 8);

        gl.bind_vertex_array(None);
        gl.bind_texture(TEXTURE_2D, None);
        gl.use_program(None);
    }

    /// Height of a text line in screen pixels.
    pub fn line_height(&self) -> f32 {
        self.line_h
    }
}
