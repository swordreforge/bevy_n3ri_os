//! Breath controller — matches CubismFramework's CubismBreath.
//! Adds subtle sinusoidal oscillation to angle/breath parameters.

use std::collections::HashMap;

/// A single breath parameter definition.
#[derive(Debug, Clone)]
pub struct BreathParam {
    /// Parameter ID string (e.g. "ParamAngleX")
    pub id: String,
    /// Sine wave offset
    pub offset: f32,
    /// Sine wave peak amplitude
    pub peak: f32,
    /// Sine wave period (seconds)
    pub cycle: f32,
    /// Blend weight
    pub weight: f32,
    /// Previous frame's raw breath value (for delta computation)
    prev_raw: f32,
}

/// Breath controller: applies sinusoidal oscillation to parameters.
#[derive(Debug)]
pub struct Breath {
    /// Accumulated time (seconds)
    time: f32,
    /// Parameter definitions
    pub params: Vec<BreathParam>,
}

impl Default for Breath {
    fn default() -> Self {
        Self::new()
    }
}

impl Breath {
    /// Create with default parameters matching CubismFramework's LAppModel setup.
    pub fn new() -> Self {
        Self {
            time: 0.0,
            params: vec![
                BreathParam {
                    id: "ParamAngleX".into(),
                    offset: 0.0,
                    peak: 15.0,
                    cycle: 6.5345,
                    weight: 0.5,
                    prev_raw: 0.0,
                },
                BreathParam {
                    id: "ParamAngleY".into(),
                    offset: 0.0,
                    peak: 8.0,
                    cycle: 3.5345,
                    weight: 0.5,
                    prev_raw: 0.0,
                },
                BreathParam {
                    id: "ParamAngleZ".into(),
                    offset: 0.0,
                    peak: 10.0,
                    cycle: 5.5345,
                    weight: 0.5,
                    prev_raw: 0.0,
                },
                BreathParam {
                    id: "ParamBodyAngleX".into(),
                    offset: 0.0,
                    peak: 4.0,
                    cycle: 15.5345,
                    weight: 0.5,
                    prev_raw: 0.0,
                },
                BreathParam {
                    id: "ParamBreath".into(),
                    offset: 0.5,
                    peak: 0.5,
                    cycle: 3.2345,
                    weight: 0.5,
                    prev_raw: 0.0,
                },
            ],
        }
    }

    /// Advance time and compute breath deltas.
    /// Returns Vec of (parameter_name, delta_value) — apply these as additive changes.
    pub fn update(
        &mut self,
        delta_time: f32,
        param_values: &mut [f32],
        param_lookup: &HashMap<String, usize>,
    ) {
        self.time += delta_time;
        let t = self.time * 2.0 * std::f32::consts::PI;

        for param in &mut self.params {
            let raw = param.offset + param.peak * (t / param.cycle).sin();
            let delta = (raw - param.prev_raw) * param.weight;
            param.prev_raw = raw;

            if delta.abs() < 1e-7 {
                continue;
            }

            if let Some(&idx) = param_lookup.get(&param.id) {
                if idx < param_values.len() {
                    param_values[idx] = (param_values[idx] + delta).clamp(-100.0, 100.0);
                }
            }
        }
    }
}
