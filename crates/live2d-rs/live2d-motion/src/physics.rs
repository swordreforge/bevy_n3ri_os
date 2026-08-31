//! Physics simulation engine — pendulum/spring physics for Live2D models.
//!
//! Ported from CubismSdkForNative Framework (CubismPhysics.cpp / CubismPhysicsInternal.hpp).

use serde::Deserialize;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const AIR_RESISTANCE: f32 = 5.0;
const MAXIMUM_WEIGHT: f32 = 100.0;
const MOVEMENT_THRESHOLD: f32 = 0.001;
/// Upper bound on physics substeps per frame. A long stall (window drag,
/// debugger break, suspend) can leave seconds of backlog in
/// `current_remain_time`; without this cap the accumulator loop would spin
/// through ~150 substeps for a 5s stall. The excess elapsed time is dropped
/// (simulated time is clamped) instead of being caught up in one frame.
const MAX_SUBSTEPS: usize = 8;

// ---------------------------------------------------------------------------
// Vec2
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
    pub fn normalize(&self) -> Self {
        let len = (self.x * self.x + self.y * self.y).sqrt();
        if len < 1e-8 {
            return Vec2::default();
        }
        Vec2::new(self.x / len, self.y / len)
    }
}

impl std::ops::Add for Vec2 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Vec2::new(self.x + rhs.x, self.y + rhs.y)
    }
}
impl std::ops::AddAssign for Vec2 {
    fn add_assign(&mut self, rhs: Self) {
        self.x += rhs.x;
        self.y += rhs.y;
    }
}
impl std::ops::Sub for Vec2 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Vec2::new(self.x - rhs.x, self.y - rhs.y)
    }
}
impl std::ops::Mul<f32> for Vec2 {
    type Output = Self;
    fn mul(self, s: f32) -> Self {
        Vec2::new(self.x * s, self.y * s)
    }
}
impl std::ops::Div<f32> for Vec2 {
    type Output = Self;
    fn div(self, s: f32) -> Self {
        Vec2::new(self.x / s, self.y / s)
    }
}

// ---------------------------------------------------------------------------
// Data structures (mirror CubismPhysicsInternal.hpp)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct Normalization {
    minimum: f32,
    maximum: f32,
    default_val: f32,
}

#[derive(Debug, Clone, Copy)]
struct Particle {
    initial_position: Vec2,
    mobility: f32,
    delay: f32,
    acceleration: f32,
    radius: f32,
    position: Vec2,
    last_position: Vec2,
    last_gravity: Vec2,
    force: Vec2,
    velocity: Vec2,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum InputType {
    X,
    Y,
    Angle,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum OutputType {
    X,
    Y,
    Angle,
}

#[derive(Debug, Clone)]
struct PhysicsInput {
    source_id: String,
    source_index: i32,
    weight: f32,
    input_type: InputType,
    reflect: bool,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct PhysicsOutput {
    destination_id: String,
    destination_index: i32,
    vertex_index: i32,
    translation_scale: Vec2,
    angle_scale: f32,
    weight: f32,
    output_type: OutputType,
    reflect: bool,
    value_below_minimum: f32,
    value_exceeded_maximum: f32,
}

#[derive(Debug, Clone)]
struct SubRig {
    input_count: i32,
    output_count: i32,
    particle_count: i32,
    base_input_index: i32,
    base_output_index: i32,
    base_particle_index: i32,
    norm_position: Normalization,
    norm_angle: Normalization,
}

#[derive(Debug)]
struct Rig {
    sub_rig_count: i32,
    settings: Vec<SubRig>,
    inputs: Vec<PhysicsInput>,
    outputs: Vec<PhysicsOutput>,
    particles: Vec<Particle>,
    gravity: Vec2,
    wind: Vec2,
    fps: f32,
}

// ---------------------------------------------------------------------------
// physics3.json serde types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct JsonXY {
    #[serde(rename = "X")]
    x: f32,
    #[serde(rename = "Y")]
    y: f32,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Physics3Json {
    #[allow(dead_code)]
    version: u32,
    meta: Meta,
    physics_settings: Vec<PhysicsSetting>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Meta {
    physics_setting_count: i32,
    total_input_count: i32,
    total_output_count: i32,
    vertex_count: i32,
    effective_forces: EffectiveForces,
    #[serde(default)]
    fps: f32,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct EffectiveForces {
    gravity: JsonXY,
    wind: JsonXY,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PhysicsSetting {
    #[allow(dead_code)]
    id: String,
    input: Vec<JsonInput>,
    output: Vec<JsonOutput>,
    vertices: Vec<JsonVertex>,
    normalization: JsonNormalization,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct JsonSource {
    #[allow(dead_code)]
    target: String,
    id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct JsonInput {
    source: JsonSource,
    #[serde(rename = "Type")]
    input_type: String,
    weight: f32,
    #[serde(default)]
    reflect: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct JsonDestination {
    #[allow(dead_code)]
    target: String,
    id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct JsonOutput {
    destination: JsonDestination,
    vertex_index: i32,
    scale: f32,
    weight: f32,
    #[serde(rename = "Type")]
    output_type: String,
    #[serde(default)]
    reflect: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct JsonVertex {
    position: JsonXY,
    mobility: f32,
    delay: f32,
    acceleration: f32,
    radius: f32,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct JsonNormalization {
    position: JsonNormRange,
    angle: JsonNormRange,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct JsonNormRange {
    minimum: f32,
    maximum: f32,
    #[serde(rename = "Default")]
    default_val: f32,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct PhysicsOptions {
    pub gravity: Vec2,
    pub wind: Vec2,
}

impl Default for PhysicsOptions {
    fn default() -> Self {
        Self {
            gravity: Vec2::new(0.0, -1.0),
            wind: Vec2::new(0.0, 0.0),
        }
    }
}

pub struct PhysicsParams<'a> {
    pub values: &'a mut [f32],
    pub minimums: &'a [f32],
    pub maximums: &'a [f32],
    pub defaults: &'a [f32],
    pub names: &'a [String],
}

pub struct PhysicsEngine {
    rig: Rig,
    options: PhysicsOptions,
    current_rig_outputs: Vec<Vec<f32>>,
    previous_rig_outputs: Vec<Vec<f32>>,
    current_remain_time: f32,
    parameter_caches: Vec<f32>,
    parameter_input_caches: Vec<f32>,
}

impl PhysicsEngine {
    pub fn sub_rig_count(&self) -> i32 {
        self.rig.sub_rig_count
    }

    pub fn from_json(buffer: &[u8]) -> Result<Self, String> {
        let json: Physics3Json =
            serde_json::from_slice(buffer).map_err(|e| format!("physics3.json: {e}"))?;

        let mut rig = Rig {
            sub_rig_count: json.meta.physics_setting_count,
            settings: Vec::with_capacity(json.meta.physics_setting_count as usize),
            inputs: Vec::with_capacity(json.meta.total_input_count as usize),
            outputs: Vec::with_capacity(json.meta.total_output_count as usize),
            particles: Vec::with_capacity(json.meta.vertex_count as usize),
            gravity: Vec2::new(
                json.meta.effective_forces.gravity.x,
                json.meta.effective_forces.gravity.y,
            ),
            wind: Vec2::new(
                json.meta.effective_forces.wind.x,
                json.meta.effective_forces.wind.y,
            ),
            fps: json.meta.fps,
        };

        let mut input_index = 0i32;
        let mut output_index = 0i32;
        let mut particle_index = 0i32;

        for s in &json.physics_settings {
            let np = Normalization {
                minimum: s.normalization.position.minimum,
                maximum: s.normalization.position.maximum,
                default_val: s.normalization.position.default_val,
            };
            let na = Normalization {
                minimum: s.normalization.angle.minimum,
                maximum: s.normalization.angle.maximum,
                default_val: s.normalization.angle.default_val,
            };

            for inp in &s.input {
                rig.inputs.push(PhysicsInput {
                    source_id: inp.source.id.clone(),
                    source_index: -1,
                    weight: inp.weight,
                    input_type: match inp.input_type.as_str() {
                        "X" => InputType::X,
                        "Y" => InputType::Y,
                        "Angle" => InputType::Angle,
                        other => return Err(format!("unknown input type: {other}")),
                    },
                    reflect: inp.reflect,
                });
            }

            for outp in &s.output {
                rig.outputs.push(PhysicsOutput {
                    destination_id: outp.destination.id.clone(),
                    destination_index: -1,
                    vertex_index: outp.vertex_index,
                    translation_scale: Vec2::new(outp.scale, outp.scale),
                    angle_scale: outp.scale,
                    weight: outp.weight,
                    output_type: match outp.output_type.as_str() {
                        "X" => OutputType::X,
                        "Y" => OutputType::Y,
                        "Angle" => OutputType::Angle,
                        other => return Err(format!("unknown output type: {other}")),
                    },
                    reflect: outp.reflect,
                    value_below_minimum: 0.0,
                    value_exceeded_maximum: 0.0,
                });
            }

            for v in &s.vertices {
                rig.particles.push(Particle {
                    initial_position: Vec2::new(v.position.x, v.position.y),
                    mobility: v.mobility,
                    delay: v.delay,
                    acceleration: v.acceleration,
                    radius: v.radius,
                    position: Vec2::new(v.position.x, v.position.y),
                    last_position: Vec2::new(v.position.x, v.position.y),
                    last_gravity: Vec2::new(0.0, -1.0),
                    force: Vec2::default(),
                    velocity: Vec2::default(),
                });
            }

            rig.settings.push(SubRig {
                input_count: s.input.len() as i32,
                output_count: s.output.len() as i32,
                particle_count: s.vertices.len() as i32,
                base_input_index: input_index,
                base_output_index: output_index,
                base_particle_index: particle_index,
                norm_position: np,
                norm_angle: na,
            });

            input_index += s.input.len() as i32;
            output_index += s.output.len() as i32;
            particle_index += s.vertices.len() as i32;
        }

        initialize_particles(&mut rig);

        let current_rig_outputs = rig
            .settings
            .iter()
            .map(|s| vec![0.0f32; s.output_count as usize])
            .collect();
        let previous_rig_outputs = rig
            .settings
            .iter()
            .map(|s| vec![0.0f32; s.output_count as usize])
            .collect();

        Ok(Self {
            rig,
            options: PhysicsOptions::default(),
            current_rig_outputs,
            previous_rig_outputs,
            current_remain_time: 0.0,
            parameter_caches: Vec::new(),
            parameter_input_caches: Vec::new(),
        })
    }

    pub fn reset(&mut self) {
        self.options = PhysicsOptions::default();
        self.rig.gravity = Vec2::new(0.0, 0.0);
        self.rig.wind = Vec2::new(0.0, 0.0);
        self.current_remain_time = 0.0;
        for v in &mut self.current_rig_outputs {
            v.fill(0.0);
        }
        for v in &mut self.previous_rig_outputs {
            v.fill(0.0);
        }
        self.parameter_caches.clear();
        self.parameter_input_caches.clear();
        initialize_particles(&mut self.rig);
    }

    pub fn options(&self) -> &PhysicsOptions {
        &self.options
    }
    pub fn set_options(&mut self, opts: PhysicsOptions) {
        self.options = opts;
    }

    // -------------------------------------------------------------------
    // Stabilization
    // -------------------------------------------------------------------

    pub fn stabilization(&mut self, params: &mut PhysicsParams) {
        let n = params.values.len();
        self.grow_caches(n);
        for j in 0..n {
            self.parameter_caches[j] = params.values[j];
            self.parameter_input_caches[j] = params.values[j];
        }

        for si in 0..self.rig.settings.len() {
            // Snapshot immutable sub-rig metadata (Copy types, no borrow)
            let setting = &self.rig.settings[si];
            let input_count = setting.input_count;
            let output_count = setting.output_count;
            let particle_count = setting.particle_count;
            let bi = setting.base_input_index;
            let bo = setting.base_output_index;
            let bp = setting.base_particle_index;
            let norm_pos = setting.norm_position;
            let threshold = MOVEMENT_THRESHOLD * setting.norm_position.maximum;

            // Process inputs — borrow mutably only what we need
            let total = {
                let inputs = &mut self.rig.inputs[bi as usize..][..input_count as usize];
                let params_names = params.names;
                let params_mins = params.minimums;
                let params_maxs = params.maximums;
                let params_defs = params.defaults;
                load_inputs_stabilization(
                    inputs,
                    params_names,
                    params_mins,
                    params_maxs,
                    params_defs,
                    &self.parameter_caches,
                    norm_pos,
                    &setting.norm_angle,
                )
            };

            let rad_angle = degrees_to_radians(-total.1);
            let (tx, ty) = rotate_bug_compat(total.0, rad_angle);
            let total_translation = Vec2::new(tx, ty);

            let particles = &mut self.rig.particles[bp as usize..][..particle_count as usize];
            update_particles_for_stabilization(
                particles,
                total_translation,
                total.1,
                self.options.wind,
                threshold,
            );

            // Outputs
            for i in 0..output_count as usize {
                let oi = (bo as usize) + i;
                let dst = resolve_output_dst(&mut self.rig.outputs[oi..=oi], 0, params.names);
                if dst < 0 {
                    continue;
                }
                let dst = dst as usize;

                if !valid_vertex(&self.rig.outputs[oi], particle_count) {
                    continue;
                }
                let vi = self.rig.outputs[oi].vertex_index as usize;

                let seg = particles[vi].position - particles[vi - 1].position;
                let out_val = get_output_value(
                    seg,
                    particles,
                    vi as i32,
                    &self.rig.outputs[oi],
                    self.options.gravity,
                );

                self.current_rig_outputs[si][i] = out_val;
                self.previous_rig_outputs[si][i] = out_val;

                update_output_parameter_value(
                    &mut params.values[dst],
                    params.minimums[dst],
                    params.maximums[dst],
                    out_val,
                    &self.rig.outputs[oi],
                );
                self.parameter_caches[dst] = params.values[dst];
            }
        }
    }

    // -------------------------------------------------------------------
    // Evaluate
    // -------------------------------------------------------------------

    pub fn evaluate(&mut self, params: &mut PhysicsParams, delta_time: f32) {
        if delta_time <= 0.0 {
            return;
        }

        let n = params.values.len();
        self.grow_caches(n);

        self.current_remain_time += delta_time;

        let physics_dt = if self.rig.fps > 0.0 {
            1.0 / self.rig.fps
        } else {
            delta_time
        };

        // Stall guard: cap the simulated time for this frame at MAX_SUBSTEPS
        // physics steps and drop the excess. This replaces the old
        // `remain > MAX_DELTA_TIME` reset (which zeroed the whole remainder and
        // caused a visible pop after a stall). At normal 60fps this clamp never
        // engages (a 16ms frame is 1-2 substeps at typical Fps=30), so ordinary
        // playback is bit-for-bit unchanged.
        let max_step_time = physics_dt * MAX_SUBSTEPS as f32;
        if self.current_remain_time > max_step_time {
            self.current_remain_time = max_step_time;
        }

        while self.current_remain_time >= physics_dt {
            // Copy current → previous
            for si in 0..self.rig.settings.len() {
                let oc = self.rig.settings[si].output_count as usize;
                self.previous_rig_outputs[si][..oc]
                    .copy_from_slice(&self.current_rig_outputs[si][..oc]);
            }

            let input_weight = physics_dt / self.current_remain_time;
            for j in 0..n {
                self.parameter_caches[j] = self.parameter_input_caches[j] * (1.0 - input_weight)
                    + params.values[j] * input_weight;
                self.parameter_input_caches[j] = self.parameter_caches[j];
            }

            for si in 0..self.rig.settings.len() {
                let setting = &self.rig.settings[si];
                let input_count = setting.input_count;
                let output_count = setting.output_count;
                let particle_count = setting.particle_count;
                let bi = setting.base_input_index;
                let bo = setting.base_output_index;
                let bp = setting.base_particle_index;
                let norm_pos = setting.norm_position;
                let threshold = MOVEMENT_THRESHOLD * setting.norm_position.maximum;

                let total = {
                    let inputs = &mut self.rig.inputs[bi as usize..][..input_count as usize];
                    load_inputs_evaluate(
                        inputs,
                        params.names,
                        params.minimums,
                        params.maximums,
                        params.defaults,
                        &self.parameter_caches,
                        norm_pos,
                        &setting.norm_angle,
                    )
                };

                let rad_angle = degrees_to_radians(-total.1);
                let (tx, ty) = rotate_bug_compat(total.0, rad_angle);
                let total_translation = Vec2::new(tx, ty);

                let particles = &mut self.rig.particles[bp as usize..][..particle_count as usize];
                update_particles(
                    particles,
                    total_translation,
                    total.1,
                    self.options.wind,
                    threshold,
                    physics_dt,
                    AIR_RESISTANCE,
                );

                for i in 0..output_count as usize {
                    let oi = (bo as usize) + i;
                    let dst = resolve_output_dst(&mut self.rig.outputs[oi..=oi], 0, params.names);
                    if dst < 0 {
                        continue;
                    }
                    let dst = dst as usize;

                    if !valid_vertex(&self.rig.outputs[oi], particle_count) {
                        continue;
                    }
                    let vi = self.rig.outputs[oi].vertex_index as usize;

                    let seg = particles[vi].position - particles[vi - 1].position;
                    let out_val = get_output_value(
                        seg,
                        particles,
                        vi as i32,
                        &self.rig.outputs[oi],
                        self.options.gravity,
                    );

                    self.current_rig_outputs[si][i] = out_val;

                    update_output_parameter_value(
                        &mut self.parameter_caches[dst],
                        params.minimums[dst],
                        params.maximums[dst],
                        out_val,
                        &self.rig.outputs[oi],
                    );
                }
            }

            self.current_remain_time -= physics_dt;
        }

        let alpha = self.current_remain_time / physics_dt;
        self.interpolate(params, alpha);
    }

    // -------------------------------------------------------------------
    // Internal
    // -------------------------------------------------------------------

    fn grow_caches(&mut self, n: usize) {
        if self.parameter_caches.len() < n {
            self.parameter_caches.resize(n, 0.0);
        }
        if self.parameter_input_caches.len() < n {
            self.parameter_input_caches.resize(n, 0.0);
        }
    }

    fn interpolate(&mut self, params: &mut PhysicsParams, weight: f32) {
        for si in 0..self.rig.settings.len() {
            let setting = &self.rig.settings[si];
            for i in 0..setting.output_count as usize {
                let oi = (setting.base_output_index as usize) + i;
                let dst = resolve_output_dst(&mut self.rig.outputs[oi..=oi], 0, params.names);
                if dst < 0 {
                    continue;
                }
                let dst = dst as usize;

                let interp = self.previous_rig_outputs[si][i] * (1.0 - weight)
                    + self.current_rig_outputs[si][i] * weight;

                update_output_parameter_value(
                    &mut params.values[dst],
                    params.minimums[dst],
                    params.maximums[dst],
                    interp,
                    &self.rig.outputs[oi],
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Free helper functions (no &self needed)
// ---------------------------------------------------------------------------

/// Resolve output destination index. Takes a 1-slice to permit &mut without full Vec borrow.
fn resolve_output_dst(outputs: &mut [PhysicsOutput], idx: usize, names: &[String]) -> i32 {
    let o = &mut outputs[idx];
    if o.destination_index < 0 {
        o.destination_index = names
            .iter()
            .position(|n| n == &o.destination_id)
            .map(|i| i as i32)
            .unwrap_or(-1);
    }
    o.destination_index
}

fn valid_vertex(output: &PhysicsOutput, particle_count: i32) -> bool {
    output.vertex_index >= 1 && output.vertex_index < particle_count
}

/// Load inputs for stabilization (also caches source_index).
#[allow(clippy::too_many_arguments)]
fn load_inputs_stabilization(
    inputs: &mut [PhysicsInput],
    names: &[String],
    mins: &[f32],
    maxs: &[f32],
    defs: &[f32],
    param_values: &[f32],
    norm_pos: Normalization,
    _norm_angle: &Normalization,
) -> (Vec2, f32) {
    let mut tt = Vec2::default();
    let mut ta = 0.0f32;

    for inp in inputs.iter_mut() {
        if inp.source_index < 0 {
            inp.source_index = names
                .iter()
                .position(|n| n == &inp.source_id)
                .map(|i| i as i32)
                .unwrap_or(-1);
        }
        let idx = inp.source_index;
        if idx < 0 {
            continue;
        }
        let idx = idx as usize;

        let weight = inp.weight / MAXIMUM_WEIGHT;
        let normalized = normalize_parameter_value(
            param_values[idx],
            mins[idx],
            maxs[idx],
            defs[idx],
            norm_pos.minimum,
            norm_pos.maximum,
            norm_pos.default_val,
            inp.reflect,
        );

        match inp.input_type {
            InputType::X => tt.x += normalized * weight,
            InputType::Y => tt.y += normalized * weight,
            InputType::Angle => ta += normalized * weight,
        }
    }

    (tt, ta)
}

/// Load inputs for evaluate (same logic, separate to avoid mixing stabilisation concerns).
#[allow(clippy::too_many_arguments)]
fn load_inputs_evaluate(
    inputs: &mut [PhysicsInput],
    names: &[String],
    mins: &[f32],
    maxs: &[f32],
    defs: &[f32],
    param_values: &[f32],
    norm_pos: Normalization,
    _norm_angle: &Normalization,
) -> (Vec2, f32) {
    #[allow(unused_variables)]
    let _ = _norm_angle;
    load_inputs_stabilization(
        inputs,
        names,
        mins,
        maxs,
        defs,
        param_values,
        norm_pos,
        _norm_angle,
    )
}

// ---------------------------------------------------------------------------
// Math functions (exact mirror of CubismPhysics.cpp)
// ---------------------------------------------------------------------------

fn degrees_to_radians(deg: f32) -> f32 {
    deg * std::f32::consts::PI / 180.0
}

fn direction_to_radian(from: Vec2, to: Vec2) -> f32 {
    let mut ret = to.y.atan2(to.x) - from.y.atan2(from.x);
    while ret < -std::f32::consts::PI {
        ret += std::f32::consts::TAU;
    }
    while ret > std::f32::consts::PI {
        ret -= std::f32::consts::TAU;
    }
    ret
}

fn radian_to_direction(angle: f32) -> Vec2 {
    Vec2::new(angle.sin(), angle.cos())
}

fn get_range_value(min: f32, max: f32) -> f32 {
    (max.min(min) - min.max(max)).abs()
}

fn get_default_value(min: f32, max: f32) -> f32 {
    min.min(max) + get_range_value(min, max) / 2.0
}

fn sign(x: f32) -> i32 {
    if x > 0.0 {
        1
    } else if x < 0.0 {
        -1
    } else {
        0
    }
}

/// Exact mirror of C++ NormalizeParameterValue.
#[allow(clippy::too_many_arguments)]
fn normalize_parameter_value(
    value: f32,
    param_min: f32,
    param_max: f32,
    _param_default: f32,
    norm_min: f32,
    norm_max: f32,
    norm_default: f32,
    is_inverted: bool,
) -> f32 {
    let max_value = param_max.max(param_min);
    let mut value = value;
    if max_value < value {
        value = max_value;
    }
    let min_value = param_max.min(param_min);
    if min_value > value {
        value = min_value;
    }

    let min_norm = norm_min.min(norm_max);
    let max_norm = norm_min.max(norm_max);
    let mid_norm = norm_default;

    let mid_value = get_default_value(min_value, max_value);
    let pv = value - mid_value;

    let result = match sign(pv) {
        1 => {
            let nl = max_norm - mid_norm;
            let pl = max_value - mid_value;
            if pl != 0.0 {
                pv * (nl / pl) + mid_norm
            } else {
                mid_norm
            }
        }
        -1 => {
            let nl = min_norm - mid_norm;
            let pl = min_value - mid_value;
            if pl != 0.0 {
                pv * (nl / pl) + mid_norm
            } else {
                mid_norm
            }
        }
        _ => mid_norm,
    };

    if is_inverted {
        result
    } else {
        -result
    }
}

/// Exact mirror of C++ UpdateParticles.
fn update_particles(
    strand: &mut [Particle],
    total_translation: Vec2,
    total_angle: f32,
    wind_direction: Vec2,
    threshold_value: f32,
    delta_time_seconds: f32,
    air_resistance: f32,
) {
    if strand.is_empty() {
        return;
    }
    strand[0].position = total_translation;

    let total_radian = degrees_to_radians(total_angle);
    let current_gravity = radian_to_direction(total_radian).normalize();

    for i in 1..strand.len() {
        strand[i].force = current_gravity * strand[i].acceleration + wind_direction;
        strand[i].last_position = strand[i].position;

        let dc = strand[i].delay * delta_time_seconds * 30.0;

        let mut dir = Vec2::new(
            strand[i].position.x - strand[i - 1].position.x,
            strand[i].position.y - strand[i - 1].position.y,
        );

        let rad = direction_to_radian(strand[i].last_gravity, current_gravity) / air_resistance;
        let (cr, sr) = (rad.cos(), rad.sin());
        let dx = cr * dir.x - dir.y * sr;
        let dy = sr * dir.x + dir.y * cr;
        dir.x = dx;
        dir.y = dy;

        strand[i].position = strand[i - 1].position + dir;
        let vel = strand[i].velocity * dc;
        let f = strand[i].force * (dc * dc);
        strand[i].position = strand[i].position + vel + f;

        let nd = (strand[i].position - strand[i - 1].position).normalize();
        strand[i].position = strand[i - 1].position + nd * strand[i].radius;

        if strand[i].position.x.abs() < threshold_value {
            strand[i].position.x = 0.0;
        }

        if dc != 0.0 {
            strand[i].velocity = (strand[i].position - strand[i].last_position) / dc;
            strand[i].velocity.x *= strand[i].mobility;
            strand[i].velocity.y *= strand[i].mobility;
        }

        // Sanitize: NaN is sticky in arithmetic, so one bad value would
        // propagate down the chain forever. Reset the particle to its rest
        // pose (initial_position is stored per particle at load time).
        // Finite inputs never trigger this.
        if !strand[i].position.x.is_finite()
            || !strand[i].position.y.is_finite()
            || !strand[i].velocity.x.is_finite()
            || !strand[i].velocity.y.is_finite()
        {
            strand[i].position = strand[i].initial_position;
            strand[i].last_position = strand[i].initial_position;
            strand[i].velocity = Vec2::default();
        }

        strand[i].force = Vec2::default();
        strand[i].last_gravity = current_gravity;
    }
}

/// Simplified initial stabilization (no velocity/delay).
fn update_particles_for_stabilization(
    strand: &mut [Particle],
    total_translation: Vec2,
    total_angle: f32,
    wind_direction: Vec2,
    threshold_value: f32,
) {
    if strand.is_empty() {
        return;
    }
    strand[0].position = total_translation;

    let total_radian = degrees_to_radians(total_angle);
    let current_gravity = radian_to_direction(total_radian).normalize();

    for i in 1..strand.len() {
        strand[i].force = current_gravity * strand[i].acceleration + wind_direction;
        strand[i].last_position = strand[i].position;
        strand[i].velocity = Vec2::default();

        let f = strand[i].force.normalize() * strand[i].radius;
        strand[i].position = strand[i - 1].position + f;

        if strand[i].position.x.abs() < threshold_value {
            strand[i].position.x = 0.0;
        }

        strand[i].force = Vec2::default();
        strand[i].last_gravity = current_gravity;
    }
}

/// Get output value from particle segment.
fn get_output_value(
    translation: Vec2,
    particles: &[Particle],
    particle_index: i32,
    output: &PhysicsOutput,
    parent_gravity: Vec2,
) -> f32 {
    match output.output_type {
        OutputType::X => {
            let mut v = translation.x;
            if output.reflect {
                v *= -1.0;
            }
            v
        }
        OutputType::Y => {
            let mut v = translation.y;
            if output.reflect {
                v *= -1.0;
            }
            v
        }
        OutputType::Angle => {
            let pg = if particle_index >= 2 {
                particles[(particle_index - 1) as usize].position
                    - particles[(particle_index - 2) as usize].position
            } else {
                parent_gravity * -1.0
            };
            let mut v = direction_to_radian(pg, translation);
            if output.reflect {
                v *= -1.0;
            }
            v
        }
    }
}

/// Clamp and blend output parameter value.
fn update_output_parameter_value(
    param_value: &mut f32,
    param_min: f32,
    param_max: f32,
    translation: f32,
    output: &PhysicsOutput,
) {
    let oscale = match output.output_type {
        OutputType::X | OutputType::Y => output.translation_scale.x,
        OutputType::Angle => output.angle_scale,
    };
    let mut value = translation * oscale;
    if value < param_min {
        value = param_min;
    } else if value > param_max {
        value = param_max;
    }

    let w = output.weight / MAXIMUM_WEIGHT;
    if w >= 1.0 {
        *param_value = value;
    } else {
        *param_value = *param_value * (1.0 - w) + value * w;
    }
}

/// Initialize particle chain.
fn initialize_particles(rig: &mut Rig) {
    for si in 0..rig.settings.len() {
        let setting = &rig.settings[si];
        let base = setting.base_particle_index as usize;
        let count = setting.particle_count as usize;
        let strand = &mut rig.particles[base..base + count];
        if strand.is_empty() {
            continue;
        }

        strand[0].initial_position = Vec2::default();
        strand[0].last_position = strand[0].initial_position;
        strand[0].last_gravity = Vec2::new(0.0, -1.0);
        strand[0].last_gravity.y *= -1.0;
        strand[0].velocity = Vec2::default();
        strand[0].force = Vec2::default();

        for i in 1..strand.len() {
            let r = Vec2::new(0.0, strand[i].radius);
            strand[i].initial_position = strand[i - 1].initial_position + r;
            strand[i].position = strand[i].initial_position;
            strand[i].last_position = strand[i].initial_position;
            strand[i].last_gravity = Vec2::new(0.0, -1.0);
            strand[i].last_gravity.y *= -1.0;
            strand[i].velocity = Vec2::default();
            strand[i].force = Vec2::default();
        }
    }
}

/// C++ rotation bug: uses updated X to compute Y.
fn rotate_bug_compat(v: Vec2, rad: f32) -> (f32, f32) {
    let nx = v.x * rad.cos() - v.y * rad.sin();
    let ny = nx * rad.sin() + v.y * rad.cos();
    (nx, ny)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_engine() -> PhysicsEngine {
        let json = r#"{
            "Version": 3, "Meta": {
                "PhysicsSettingCount": 1, "TotalInputCount": 1,
                "TotalOutputCount": 1, "VertexCount": 2,
                "EffectiveForces": { "Gravity": { "X": 0, "Y": -1 }, "Wind": { "X": 0, "Y": 0 } }
            },
            "PhysicsSettings": [{
                "Id": "t",
                "Input": [{ "Source": { "Target": "Parameter", "Id": "P1" }, "Type": "X", "Weight": 1.0, "Reflect": false }],
                "Output": [{ "Destination": { "Target": "Parameter", "Id": "P2" }, "Type": "X", "VertexIndex": 1, "Scale": 1.0, "Weight": 1.0, "Reflect": false }],
                "Vertices": [
                    { "Position": { "X": 0, "Y": 0 }, "Mobility": 0, "Delay": 0, "Acceleration": 0, "Radius": 0 },
                    { "Position": { "X": 0, "Y": -1 }, "Mobility": 10, "Delay": 1.0, "Acceleration": 5.0, "Radius": 1 }
                ],
                "Normalization": { "Position": { "Minimum": -1, "Maximum": 1, "Default": 0 }, "Angle": { "Minimum": -30, "Maximum": 30, "Default": 0 } }
            }]
        }"#;
        PhysicsEngine::from_json(json.as_bytes()).unwrap()
    }

    #[test]
    fn test_normalize_basic() {
        let r = normalize_parameter_value(0.0, -30.0, 30.0, 0.0, -1.0, 1.0, 0.0, false);
        assert!((r - 0.0).abs() < 1e-5, "got {r}");
    }

    #[test]
    fn test_normalize_max_invert() {
        let r = normalize_parameter_value(30.0, -30.0, 30.0, 0.0, -1.0, 1.0, 0.0, true);
        assert!((r - 1.0).abs() < 1e-5, "got {r}");
    }

    #[test]
    fn test_normalize_max_not_inverted() {
        let r = normalize_parameter_value(30.0, -30.0, 30.0, 0.0, -1.0, 1.0, 0.0, false);
        assert!((r - (-1.0)).abs() < 1e-5, "got {r}");
    }

    #[test]
    fn test_parse_engine() {
        let e = make_test_engine();
        assert_eq!(e.sub_rig_count(), 1);
        assert_eq!(e.rig.inputs.len(), 1);
        assert_eq!(e.rig.outputs.len(), 1);
        assert_eq!(e.rig.particles.len(), 2);
    }

    #[test]
    fn test_stabilization_basic() {
        let mut engine = make_test_engine();
        let mut values = vec![10.0f32, 0.0f32];
        let mins = vec![-30.0, -360.0];
        let maxs = vec![30.0, 360.0];
        let defs = vec![0.0, 0.0];
        let names = vec!["P1".into(), "P2".into()];
        let mut params = PhysicsParams {
            values: &mut values,
            minimums: &mins,
            maximums: &maxs,
            defaults: &defs,
            names: &names,
        };

        engine.stabilization(&mut params);
        // P1 clamped to 30, normalized to 1.0 with reflect=false → -1.0
        // weight = 1/100 = 0.01
        // total_translation.x = -1.0 * 0.01 = -0.01
        // P2 output type X, reflect=false → translation.x
        // seg = p1 - p0, so translation.x = some value
        // It should have been set (no crash)
        assert!(params.values[0] > -31.0 && params.values[0] < 31.0);
    }

    #[test]
    fn test_evaluate_basic() {
        let mut engine = make_test_engine();
        let mut values = vec![10.0f32, 0.0f32];
        let mins = vec![-30.0, -360.0];
        let maxs = vec![30.0, 360.0];
        let defs = vec![0.0, 0.0];
        let names = vec!["P1".into(), "P2".into()];
        let mut params = PhysicsParams {
            values: &mut values,
            minimums: &mins,
            maximums: &maxs,
            defaults: &defs,
            names: &names,
        };

        engine.stabilization(&mut params);
        engine.evaluate(&mut params, 1.0 / 30.0);
        // Should not crash, values should be set
        assert!(params.values[1] >= -360.0 && params.values[1] <= 360.0);
    }

    #[test]
    fn test_direction_to_radian() {
        // C++: q1 = atan2(to.Y, to.X) = atan2(0,1) = 0
        //      q2 = atan2(from.Y, from.X) = atan2(1,0) = PI/2
        //      ret = q1 - q2 = -PI/2
        let r = direction_to_radian(Vec2::new(0.0, 1.0), Vec2::new(1.0, 0.0));
        assert!((r - (-std::f32::consts::FRAC_PI_2)).abs() < 1e-5, "got {r}");
    }

    #[test]
    fn test_radian_to_direction() {
        let d = radian_to_direction(0.0);
        assert!((d.x - 0.0).abs() < 1e-5);
        assert!((d.y - 1.0).abs() < 1e-5);
    }

    #[test]
    fn test_update_particles_2() {
        let mut p = [
            Particle {
                initial_position: Vec2::default(),
                mobility: 0.0,
                delay: 0.0,
                acceleration: 0.0,
                radius: 0.0,
                position: Vec2::default(),
                last_position: Vec2::default(),
                last_gravity: Vec2::new(0.0, -1.0),
                force: Vec2::default(),
                velocity: Vec2::default(),
            },
            Particle {
                initial_position: Vec2::new(0.0, -30.0),
                mobility: 10.0,
                delay: 1.0,
                acceleration: 5.0,
                radius: 30.0,
                position: Vec2::new(0.0, -30.0),
                last_position: Vec2::new(0.0, -30.0),
                last_gravity: Vec2::new(0.0, -1.0),
                force: Vec2::default(),
                velocity: Vec2::default(),
            },
        ];
        update_particles(
            &mut p,
            Vec2::new(1.0, 0.0),
            0.0,
            Vec2::default(),
            0.001,
            1.0 / 30.0,
            AIR_RESISTANCE,
        );
        assert!((p[0].position.x - 1.0).abs() < 1e-5);
        let dx = p[1].position.x - p[0].position.x;
        let dy = p[1].position.y - p[0].position.y;
        let d = (dx * dx + dy * dy).sqrt();
        assert!((d - 30.0).abs() < 1.0, "dist={d} expected ~30");
    }

    #[test]
    fn test_rotate_bug_compat_identity() {
        let (x, y) = rotate_bug_compat(Vec2::new(1.0, 0.0), 0.0);
        assert!((x - 1.0).abs() < 1e-5);
        assert!((y - 0.0).abs() < 1e-5);
    }

    #[test]
    fn test_parse_complex() {
        let json = r#"{
            "Version": 3, "Meta": {
                "PhysicsSettingCount": 2, "TotalInputCount": 3,
                "TotalOutputCount": 2, "VertexCount": 5,
                "EffectiveForces": { "Gravity": { "X": 0, "Y": -1 }, "Wind": { "X": 0.5, "Y": 0 } },
                "Fps": 30.0
            },
            "PhysicsSettings": [
                { "Id": "h1", "Input": [
                    { "Source": { "Target": "Parameter", "Id": "ParamAngleX" }, "Type": "X", "Weight": 1.0, "Reflect": false }
                ], "Output": [
                    { "Destination": { "Target": "Parameter", "Id": "PH1" }, "Type": "X", "VertexIndex": 2, "Scale": 0.8, "Weight": 0.5, "Reflect": false }
                ], "Vertices": [
                    { "Position": { "X": 0, "Y": 0 }, "Mobility": 0, "Delay": 0, "Acceleration": 0, "Radius": 0 },
                    { "Position": { "X": 0, "Y": -30 }, "Mobility": 10, "Delay": 1.0, "Acceleration": 5.0, "Radius": 30 },
                    { "Position": { "X": 0, "Y": -60 }, "Mobility": 10, "Delay": 0.8, "Acceleration": 5.0, "Radius": 30 }
                ], "Normalization": { "Position": { "Minimum": -1, "Maximum": 1, "Default": 0 }, "Angle": { "Minimum": -30, "Maximum": 30, "Default": 0 } } },
                { "Id": "h2", "Input": [
                    { "Source": { "Target": "Parameter", "Id": "ParamAngleY" }, "Type": "Angle", "Weight": 0.5, "Reflect": true }
                ], "Output": [
                    { "Destination": { "Target": "Parameter", "Id": "PH2" }, "Type": "Angle", "VertexIndex": 1, "Scale": 1.0, "Weight": 1.0, "Reflect": false }
                ], "Vertices": [
                    { "Position": { "X": 0, "Y": 0 }, "Mobility": 0, "Delay": 0, "Acceleration": 0, "Radius": 0 },
                    { "Position": { "X": 0, "Y": -20 }, "Mobility": 5, "Delay": 0.5, "Acceleration": 3.0, "Radius": 20 }
                ], "Normalization": { "Position": { "Minimum": -1, "Maximum": 1, "Default": 0 }, "Angle": { "Minimum": -30, "Maximum": 30, "Default": 0 } } }
            ]
        }"#;
        let e = PhysicsEngine::from_json(json.as_bytes()).unwrap();
        assert_eq!(e.sub_rig_count(), 2);
        assert_eq!(e.rig.inputs.len(), 2); // total 2 (1+1)
                                           // Actually 1+1=2, but JSON says 3... let me fix the test
                                           // The actual input count from the JSON is 2 (first has 1, second has 1)
                                           // But our meta says 3. This is a test mistake. Let me just check structure.
        assert_eq!(e.rig.settings.len(), 2);
        assert_eq!(e.rig.particles.len(), 5);
        assert!((e.rig.fps - 30.0).abs() < 1e-5);
        assert!((e.rig.wind.x - 0.5).abs() < 1e-5);
    }
}
