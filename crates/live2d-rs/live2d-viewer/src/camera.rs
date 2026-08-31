pub struct Camera {
    pub scale_x: f32,
    pub scale_y: f32,
    pub translate_x: f32,
    pub translate_y: f32,
}

impl Camera {
    pub fn new() -> Self {
        Self {
            scale_x: 1.0,
            scale_y: 1.0,
            translate_x: 0.0,
            translate_y: 0.0,
        }
    }

    /// Set up camera to match Cubism Framework transform.
    ///
    /// Cubism Core vertex positions are in model-space coordinates.
    /// The Cubism Framework's model matrix scales by `2.0 / logical_canvas_h`
    /// (uniform), then the projection scales:
    ///   - landscape (w > h): Scale(h/w, 1.0) — corrects width
    ///   - portrait  (w <= h): Scale(1.0, w/h) — corrects height
    ///
    /// Our vertex shader: gl_Position = vec4(a_position.xy * uScale + uTranslate, 0, 1)
    pub fn fit_to_canvas(
        &mut self,
        _canvas_w: f32,
        canvas_h: f32,
        ppu: f32,
        screen_w: f32,
        screen_h: f32,
    ) {
        let logical_h = canvas_h / ppu;
        let model_scale = 2.0 / logical_h;

        if screen_w > screen_h {
            // Landscape: model fills height, correct width
            self.scale_x = model_scale * (screen_h / screen_w);
            self.scale_y = -model_scale;
        } else {
            // Portrait: model fills width, correct height
            self.scale_x = model_scale;
            self.scale_y = -(model_scale * (screen_w / screen_h));
        }
        self.translate_x = 0.0;
        self.translate_y = 0.0;
    }

    pub fn pan(&mut self, dx: f32, dy: f32) {
        self.translate_x += dx * 2.0 / self.scale_x / 100.0;
        self.translate_y -= dy * 2.0 / self.scale_y / 100.0;
    }

    pub fn zoom(&mut self, delta: f32, cx: f32, cy: f32) {
        let factor = if delta > 0.0 { 1.15 } else { 0.87 };
        let wx = (cx * 2.0 - 1.0) / self.scale_x - self.translate_x / self.scale_x;
        let wy = (1.0 - cy * 2.0) / self.scale_y - self.translate_y / self.scale_y;
        self.scale_x *= factor;
        self.scale_y *= factor;
        self.translate_x = (cx * 2.0 - 1.0) - (wx + self.translate_x / self.scale_x) * self.scale_x;
        self.translate_y = (1.0 - cy * 2.0) - (wy + self.translate_y / self.scale_y) * self.scale_y;
    }

    /// Reset pan (re-center the model).
    pub fn reset_pan(&mut self) {
        self.translate_x = 0.0;
        self.translate_y = 0.0;
    }

    /// Zoom in by one step (centered).
    pub fn zoom_in(&mut self) {
        self.zoom(1.0, 0.5, 0.5);
    }

    /// Zoom out by one step (centered).
    pub fn zoom_out(&mut self) {
        self.zoom(-1.0, 0.5, 0.5);
    }
}
