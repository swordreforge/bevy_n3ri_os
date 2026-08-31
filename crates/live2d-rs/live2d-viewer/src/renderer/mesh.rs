use glow::*;

pub struct Mesh {
    vao: NativeVertexArray,
    vbo: NativeBuffer,
    ubo: NativeBuffer,
    ebo: NativeBuffer,
    pub vertex_count: i32,
    pub index_count: i32,
}

impl Mesh {
    pub unsafe fn new(gl: &Context) -> Result<Self, String> {
        let vao = gl
            .create_vertex_array()
            .map_err(|e| format!("create vao: {:?}", e))?;
        let vbo = gl
            .create_buffer()
            .map_err(|e| format!("create vbo: {:?}", e))?;
        let ubo = gl
            .create_buffer()
            .map_err(|e| format!("create ubo: {:?}", e))?;
        let ebo = gl
            .create_buffer()
            .map_err(|e| format!("create ebo: {:?}", e))?;
        Ok(Self {
            vao,
            vbo,
            ubo,
            ebo,
            vertex_count: 0,
            index_count: 0,
        })
    }

    /// Upload positions, UVs, and indices to GPU.
    /// Positions and UVs are in separate non-interleaved buffers,
    /// avoiding the per-frame interleave copy from Cubism Core's AoS layout.
    pub unsafe fn upload(&mut self, gl: &Context, positions: &[f32], uvs: &[f32], indices: &[u16]) {
        self.vertex_count = (positions.len() / 2) as i32;
        self.index_count = indices.len() as i32;

        gl.bind_vertex_array(Some(self.vao));

        // Positions (attribute 0)
        gl.bind_buffer(ARRAY_BUFFER, Some(self.vbo));
        gl.buffer_data_u8_slice(
            ARRAY_BUFFER,
            std::slice::from_raw_parts(positions.as_ptr() as *const u8, positions.len() * 4),
            DYNAMIC_DRAW,
        );
        gl.vertex_attrib_pointer_f32(0, 2, FLOAT, false, 8, 0);
        gl.enable_vertex_attrib_array(0);

        // UVs (attribute 1) — separate buffer, non-interleaved
        gl.bind_buffer(ARRAY_BUFFER, Some(self.ubo));
        gl.buffer_data_u8_slice(
            ARRAY_BUFFER,
            std::slice::from_raw_parts(uvs.as_ptr() as *const u8, uvs.len() * 4),
            DYNAMIC_DRAW,
        );
        gl.vertex_attrib_pointer_f32(1, 2, FLOAT, false, 8, 0);
        gl.enable_vertex_attrib_array(1);

        // Indices (EBO)
        gl.bind_buffer(ELEMENT_ARRAY_BUFFER, Some(self.ebo));
        gl.buffer_data_u8_slice(
            ELEMENT_ARRAY_BUFFER,
            std::slice::from_raw_parts(indices.as_ptr() as *const u8, indices.len() * 2),
            DYNAMIC_DRAW,
        );

        gl.bind_vertex_array(None);
    }

    pub unsafe fn draw(&self, gl: &Context) {
        gl.bind_vertex_array(Some(self.vao));
        gl.draw_elements(TRIANGLES, self.index_count, UNSIGNED_SHORT, 0);
        gl.bind_vertex_array(None);
    }
}

impl Drop for Mesh {
    fn drop(&mut self) {
        // NOTE: requires active GL context.  Caller must ensure.
    }
}
