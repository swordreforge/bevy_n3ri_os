//! Overlay toolbar for the sctk (wlr-layer-shell) pet surface.
//!
//! Renders a semi-transparent button column on the right edge of the GL
//! viewport using raw OpenGL (via glow).  Auto-hides: fades in when the
//! pointer is near the right edge, fades out otherwise.
//!
//! All icons are drawn as geometric primitives (triangles, rectangles)
//! — no font or texture dependency.

use glow::*;

/// Action triggered by a toolbar button click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarAction {
    PrevModel,
    NextModel,
    ResetCamera,
    ZoomIn,
    ZoomOut,
    Search,
    ExitPet,
}

const TOOLBAR_W: f32 = 56.0; // px
const BTN_H: f32 = 34.0;
const BTN_SPACING: f32 = 6.0;
const SEP_GAP: f32 = 10.0;
const FADE_SPEED: f32 = 0.15;
const HOVER_ZONE: f32 = 65.0; // px from right edge to show toolbar

fn ndc(sx: f32, sy: f32, w: f32, h: f32) -> [f32; 2] {
    [
        (sx / w) * 2.0 - 1.0,
        1.0 - (sy / h) * 2.0, // screen Y-down → NDC Y-up
    ]
}

pub(crate) fn ndc_pos(sx: f32, sy: f32, w: f32, h: f32) -> [f32; 2] {
    ndc(sx, sy, w, h)
}

/// 2D overlay toolbar rendered with glow.
pub struct ToolbarOverlay {
    /// Current opacity (0 = hidden, 1 = fully visible), for smooth fade.
    pub alpha: f32,
    target_alpha: f32,
    /// Index of the hovered button, if any.
    pub hover_idx: Option<usize>,

    // GL resources
    program: NativeProgram,
    vao: NativeVertexArray,
    vbo: NativeBuffer,
}

impl ToolbarOverlay {
    /// Compile the overlay shader and create VAO/VBO.
    ///
    /// # Safety
    ///
    /// Requires an active GL context.
    pub unsafe fn new(gl: &Context) -> Result<Self, String> {
        let program = compile_overlay_program(gl)?;

        let vao = gl.create_vertex_array().map_err(|e| format!("vao: {e}"))?;
        let vbo = gl.create_buffer().map_err(|e| format!("vbo: {e}"))?;

        gl.bind_vertex_array(Some(vao));
        gl.bind_buffer(ARRAY_BUFFER, Some(vbo));
        let stride = 24; // 6 × f32: (pos2, color4)
        gl.vertex_attrib_pointer_f32(0, 2, FLOAT, false, stride, 0);
        gl.enable_vertex_attrib_array(0);
        gl.vertex_attrib_pointer_f32(1, 4, FLOAT, false, stride, 8);
        gl.enable_vertex_attrib_array(1);
        gl.bind_vertex_array(None);

        Ok(Self {
            alpha: 0.0,
            target_alpha: 0.0,
            hover_idx: None,
            program,
            vao,
            vbo,
        })
    }

    /// Update fade state and hover detection.  Call once per frame.
    pub fn update(&mut self, pointer_x: f64, pointer_y: f64, vp_w: u32, vp_h: u32) {
        let near_right = pointer_x >= (vp_w as f64 - HOVER_ZONE as f64) && pointer_x <= vp_w as f64;
        self.target_alpha = if near_right { 1.0 } else { 0.0 };

        // Smooth transition toward target
        if self.alpha < self.target_alpha {
            self.alpha = (self.alpha + FADE_SPEED).min(1.0);
        } else if self.alpha > self.target_alpha {
            self.alpha = (self.alpha - FADE_SPEED).max(0.0);
        }

        self.hover_idx = if near_right && self.alpha > 0.01 {
            Self::hit_test_impl(pointer_x as f32, pointer_y as f32, vp_w, vp_h)
        } else {
            None
        };
    }

    /// Returns the `ToolbarAction` if the pointer is on a visible button,
    /// or `None` if it missed or the toolbar is faded out.
    pub fn handle_click(
        &self,
        pointer_x: f64,
        pointer_y: f64,
        vp_w: u32,
        vp_h: u32,
    ) -> Option<ToolbarAction> {
        // Allow clicks as soon as the toolbar is rendered at all (alpha >= 0.01
        // matches the render cull check).  A higher threshold (0.5) caused
        // missed clicks during the 3–4 frame fade-in window (~50-67ms).
        if self.alpha < 0.01 {
            return None;
        }
        let idx = Self::hit_test_impl(pointer_x as f32, pointer_y as f32, vp_w, vp_h)?;
        Some(Self::button_order()[idx])
    }

    // ──── hit test helper ────

    fn hit_test_impl(px: f32, py: f32, vp_w: u32, vp_h: u32) -> Option<usize> {
        let vp_w = vp_w as f32;
        let vp_h = vp_h as f32;
        let toolbar_x = vp_w - TOOLBAR_W;
        if px < toolbar_x || px > vp_w {
            return None;
        }
        let (_start_y, button_ys) = Self::button_layout(vp_h);
        for (i, (y0, y1)) in button_ys.iter().enumerate() {
            if py >= *y0 && py <= *y1 {
                return Some(i);
            }
        }
        None
    }

    /// Returns `(start_y, Vec<(top, bottom)>)` for all buttons.
    fn button_layout(vp_h: f32) -> (f32, Vec<(f32, f32)>) {
        let n = 7u32;
        let total = n as f32 * BTN_H + (n - 1) as f32 * BTN_SPACING + 2.0 * SEP_GAP;
        let start_y = (vp_h - total) / 2.0;
        let mut ys = Vec::with_capacity(n as usize);
        let mut y = start_y;
        for i in 0..n as usize {
            if i == 2 || i == 5 {
                y += SEP_GAP;
            }
            ys.push((y, y + BTN_H));
            y += BTN_H + BTN_SPACING;
        }
        (start_y, ys)
    }

    fn button_order() -> Vec<ToolbarAction> {
        vec![
            ToolbarAction::PrevModel,
            ToolbarAction::NextModel,
            ToolbarAction::ResetCamera,
            ToolbarAction::ZoomIn,
            ToolbarAction::ZoomOut,
            ToolbarAction::Search,
            ToolbarAction::ExitPet,
        ]
    }

    // ──── Render ────

    /// Draw the toolbar overlay (after the model).
    ///
    /// # Safety
    ///
    /// Requires an active GL context.  Disables depth test and uses
    /// premultiplied-alpha blending; caller should restore state as needed.
    pub unsafe fn render(&self, gl: &Context, vp_w: u32, vp_h: u32) {
        if self.alpha < 0.01 {
            return;
        }
        let w = vp_w as f32;
        let h = vp_h as f32;
        let tb_x = w - TOOLBAR_W;

        // ── setup ──
        gl.enable(BLEND);
        gl.blend_func_separate(SRC_ALPHA, ONE_MINUS_SRC_ALPHA, ONE, ONE_MINUS_SRC_ALPHA);
        gl.disable(DEPTH_TEST);
        gl.use_program(Some(self.program));
        gl.bind_vertex_array(Some(self.vao));

        // Collect interleaved vertex data: [x, y, r, g, b, a] in screen space.
        let mut verts: Vec<f32> = Vec::new();

        // ── background ──
        let bg_a = 0.55 * self.alpha;
        Self::push_rect(&mut verts, tb_x, 0.0, TOOLBAR_W, h, 0.0, 0.0, 0.0, bg_a);

        // ── buttons ──
        let (_start_y, button_ys) = Self::button_layout(h);
        let btn_w = TOOLBAR_W - 4.0; // 2px inset each side
        let btn_x = tb_x + 2.0;

        for (i, action) in Self::button_order().iter().enumerate() {
            let (y0, y1) = button_ys[i];
            let icon_cx = btn_x + btn_w / 2.0;
            let icon_cy = (y0 + y1) / 2.0;
            let ih = 6.0; // icon half-size

            // hover highlight
            if self.hover_idx == Some(i) {
                Self::push_rect(
                    &mut verts,
                    btn_x,
                    y0,
                    btn_w,
                    y1 - y0,
                    0.3,
                    0.3,
                    0.3,
                    0.7 * self.alpha,
                );
            }

            let ic = [1.0, 1.0, 1.0, 0.85 * self.alpha]; // icon colour

            match action {
                ToolbarAction::PrevModel => {
                    // ◀ left-pointing triangle
                    Self::push_tri(
                        &mut verts,
                        icon_cx - ih,
                        icon_cy,
                        icon_cx + ih,
                        icon_cy - ih,
                        icon_cx + ih,
                        icon_cy + ih,
                        ic,
                    );
                }
                ToolbarAction::NextModel => {
                    // ▶ right-pointing triangle
                    Self::push_tri(
                        &mut verts,
                        icon_cx + ih,
                        icon_cy,
                        icon_cx - ih,
                        icon_cy - ih,
                        icon_cx - ih,
                        icon_cy + ih,
                        ic,
                    );
                }
                ToolbarAction::ResetCamera => {
                    // ↺ filled circle (triangle fan via 8 slices) + centre dot
                    let segments = 8;
                    let r = ih * 0.85;
                    for j in 0..segments {
                        let a1 = j as f32 * std::f32::consts::TAU / segments as f32;
                        let a2 = (j + 1) as f32 * std::f32::consts::TAU / segments as f32;
                        Self::push_tri(
                            &mut verts,
                            icon_cx,
                            icon_cy,
                            icon_cx + a1.cos() * r,
                            icon_cy + a1.sin() * r,
                            icon_cx + a2.cos() * r,
                            icon_cy + a2.sin() * r,
                            ic,
                        );
                    }
                }
                ToolbarAction::ZoomIn => {
                    // + horizontal + vertical bar (each as a thin rect)
                    let bw = 2.5; // bar width
                    let bl = ih * 1.3; // bar length
                    Self::push_rect(
                        &mut verts,
                        icon_cx - bl,
                        icon_cy - bw / 2.0,
                        bl * 2.0,
                        bw,
                        ic[0],
                        ic[1],
                        ic[2],
                        ic[3],
                    );
                    Self::push_rect(
                        &mut verts,
                        icon_cx - bw / 2.0,
                        icon_cy - bl,
                        bw,
                        bl * 2.0,
                        ic[0],
                        ic[1],
                        ic[2],
                        ic[3],
                    );
                }
                ToolbarAction::ZoomOut => {
                    // - single horizontal bar
                    let bw = 2.0;
                    let bl = ih * 1.3;
                    Self::push_rect(
                        &mut verts,
                        icon_cx - bl,
                        icon_cy - bw / 2.0,
                        bl * 2.0,
                        bw,
                        ic[0],
                        ic[1],
                        ic[2],
                        ic[3],
                    );
                }
                ToolbarAction::Search => {
                    // magnifying glass: circle (lens, 3/4 arc) + handle (45° line)
                    let r = ih * 0.65;
                    let (cx, cy) = (icon_cx - 0.8, icon_cy - 0.8);
                    let segments = 10;
                    // ¾ arc from -135° to 135° (skips bottom-left for handle)
                    let start_rad = -135_f32.to_radians();
                    let end_rad = 135_f32.to_radians();
                    let step = (end_rad - start_rad) / segments as f32;
                    for j in 0..segments {
                        let a1 = start_rad + step * j as f32;
                        let a2 = start_rad + step * (j + 1) as f32;
                        Self::push_tri(
                            &mut verts,
                            cx,
                            cy,
                            cx + a1.cos() * r,
                            cy + a1.sin() * r,
                            cx + a2.cos() * r,
                            cy + a2.sin() * r,
                            ic,
                        );
                    }
                    // handle: 45° line from bottom-right of lens outward
                    let hcx = cx + 0.707 * r;
                    let hcy = cy + 0.707 * r;
                    let hw = 1.5;
                    let hlen = ih * 1.0;
                    Self::push_quad(
                        &mut verts,
                        (hcx - hw, hcy - hw),
                        (hcx + hw, hcy + hw),
                        (hcx + hw + hlen, hcy + hw + hlen),
                        (hcx - hw + hlen, hcy - hw + hlen),
                        ic,
                    );
                    Self::push_quad(
                        &mut verts,
                        (hcx - hw, hcy - hw),
                        (hcx - hw + hlen, hcy - hw + hlen),
                        (hcx + hw + hlen, hcy + hw + hlen),
                        (hcx + hw, hcy + hw),
                        ic,
                    );
                }
                ToolbarAction::ExitPet => {
                    // ✖ two crossing thin rects at 45°
                    let half_len = ih * 0.8;
                    let hw = 1.6; // half-width (perpendicular)
                    let (cx, cy) = (icon_cx, icon_cy);
                    let (p1x, p1y) = (cx - half_len - hw, cy - half_len + hw);
                    let (p2x, p2y) = (cx - half_len + hw, cy - half_len - hw);
                    let (p3x, p3y) = (cx + half_len + hw, cy + half_len - hw);
                    let (p4x, p4y) = (cx + half_len - hw, cy + half_len + hw);
                    Self::push_quad(
                        &mut verts,
                        (p1x, p1y),
                        (p4x, p4y),
                        (p3x, p3y),
                        (p2x, p2y),
                        ic,
                    );
                    // Bar /
                    let (q1x, q1y) = (cx + half_len + hw, cy - half_len + hw);
                    let (q2x, q2y) = (cx + half_len - hw, cy - half_len - hw);
                    let (q3x, q3y) = (cx - half_len - hw, cy + half_len - hw);
                    let (q4x, q4y) = (cx - half_len + hw, cy + half_len + hw);
                    Self::push_quad(
                        &mut verts,
                        (q1x, q1y),
                        (q4x, q4y),
                        (q3x, q3y),
                        (q2x, q2y),
                        ic,
                    );
                }
            }
        }

        // Convert all screen-space vertices to NDC and upload as a single buffer.
        let mut ndc_buf: Vec<f32> = Vec::with_capacity(verts.len());
        for chunk in verts.chunks_exact(6) {
            let [nx, ny] = ndc(chunk[0], chunk[1], w, h);
            ndc_buf.push(nx);
            ndc_buf.push(ny);
            ndc_buf.extend_from_slice(&chunk[2..6]); // colour
        }

        gl.bind_buffer(ARRAY_BUFFER, Some(self.vbo));
        gl.buffer_data_u8_slice(
            ARRAY_BUFFER,
            core::slice::from_raw_parts(ndc_buf.as_ptr() as *const u8, ndc_buf.len() * 4),
            STREAM_DRAW,
        );

        gl.draw_arrays(TRIANGLES, 0, (verts.len() / 6) as i32);

        // teardown
        gl.disable(BLEND);
        gl.enable(DEPTH_TEST);
        gl.use_program(None);
        gl.bind_vertex_array(None);
    }

    /// Destroy GL resources.  Call before dropping the GL context.
    ///
    /// # Safety
    ///
    /// Requires an active GL context.
    pub unsafe fn destroy(self, gl: &Context) {
        gl.delete_program(self.program);
        gl.delete_vertex_array(self.vao);
        gl.delete_buffer(self.vbo);
    }

    pub fn program_id(&self) -> NativeProgram {
        self.program
    }
    pub fn vao_id(&self) -> NativeVertexArray {
        self.vao
    }
    pub fn vbo_id(&self) -> NativeBuffer {
        self.vbo
    }

    // ──── Geometry helpers (screen space, 6 f32 per vertex) ────

    fn push_vert(buf: &mut Vec<f32>, x: f32, y: f32, r: f32, g: f32, b: f32, a: f32) {
        buf.push(x);
        buf.push(y);
        buf.push(r);
        buf.push(g);
        buf.push(b);
        buf.push(a);
    }
    #[allow(clippy::too_many_arguments)]
    fn push_tri(
        buf: &mut Vec<f32>,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        x3: f32,
        y3: f32,
        c: [f32; 4],
    ) {
        Self::push_vert(buf, x1, y1, c[0], c[1], c[2], c[3]);
        Self::push_vert(buf, x2, y2, c[0], c[1], c[2], c[3]);
        Self::push_vert(buf, x3, y3, c[0], c[1], c[2], c[3]);
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn push_rect(
        buf: &mut Vec<f32>,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        g: f32,
        b: f32,
        a: f32,
    ) {
        // Two triangles: (x,y) → (x+w,y) → (x,y+h) and (x+w,y) → (x+w,y+h) → (x,y+h)
        Self::push_vert(buf, x, y, r, g, b, a);
        Self::push_vert(buf, x + w, y, r, g, b, a);
        Self::push_vert(buf, x, y + h, r, g, b, a);

        Self::push_vert(buf, x + w, y, r, g, b, a);
        Self::push_vert(buf, x + w, y + h, r, g, b, a);
        Self::push_vert(buf, x, y + h, r, g, b, a);
    }

    /// Quad from 4 corner points (ordered CCW or CW, two triangles).
    fn push_quad(
        buf: &mut Vec<f32>,
        p1: (f32, f32),
        p2: (f32, f32),
        p3: (f32, f32),
        p4: (f32, f32),
        c: [f32; 4],
    ) {
        Self::push_vert(buf, p1.0, p1.1, c[0], c[1], c[2], c[3]);
        Self::push_vert(buf, p2.0, p2.1, c[0], c[1], c[2], c[3]);
        Self::push_vert(buf, p3.0, p3.1, c[0], c[1], c[2], c[3]);

        Self::push_vert(buf, p1.0, p1.1, c[0], c[1], c[2], c[3]);
        Self::push_vert(buf, p3.0, p3.1, c[0], c[1], c[2], c[3]);
        Self::push_vert(buf, p4.0, p4.1, c[0], c[1], c[2], c[3]);
    }
}

// ──── shader compilation ────

unsafe fn compile_overlay_program(gl: &Context) -> Result<NativeProgram, String> {
    let vs_src = r#"#version 330 core
layout(location=0) in vec2 aPos;
layout(location=1) in vec4 aColor;
out vec4 vColor;
void main() {
    gl_Position = vec4(aPos, 0.0, 1.0);
    vColor = aColor;
}"#;

    let fs_src = r#"#version 330 core
in vec4 vColor;
out vec4 FragColor;
void main() {
    FragColor = vColor;
}"#;

    let program = gl
        .create_program()
        .map_err(|e| format!("create program: {e}"))?;

    let vs = gl
        .create_shader(VERTEX_SHADER)
        .map_err(|e| format!("create vs: {e}"))?;
    gl.shader_source(vs, vs_src);
    gl.compile_shader(vs);
    if !gl.get_shader_compile_status(vs) {
        return Err(format!("vs compile: {}", gl.get_shader_info_log(vs)));
    }

    let fs = gl
        .create_shader(FRAGMENT_SHADER)
        .map_err(|e| format!("create fs: {e}"))?;
    gl.shader_source(fs, fs_src);
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
