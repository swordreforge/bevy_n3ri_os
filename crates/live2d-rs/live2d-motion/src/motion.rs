//! Motion data and parameter evaluation.
//!
//! Mirrors `CubismMotion::DoUpdateParameters` from the Live2D Cubism Framework.
//! Evaluates all curves in a parsed motion against model parameters.

use std::collections::HashMap;
use std::sync::Arc;

use super::curve::{self, SegmentType};
use super::json::{ParsedCurve, ParsedMotion};

/// Constants for special curve target IDs.
const TARGET_NAME_MODEL: &str = "Model";
const TARGET_NAME_PARAMETER: &str = "Parameter";
const TARGET_NAME_PART_OPACITY: &str = "PartOpacity";
const ID_NAME_OPACITY: &str = "Opacity";

/// Flag values for tracking which params eye blink / lip sync were applied to.
const MAX_TARGET_BITS: usize = 64;

/// A single motion instance. Holds the parsed motion data (immutable, shared via Arc).
#[derive(Debug, Clone)]
pub struct CubismMotion {
    pub data: Arc<ParsedMotion>,
    pub fade_in_seconds: f32,
    pub fade_out_seconds: f32,
    pub weight: f32,
    pub is_loop: bool,
    pub is_loop_fade_in: bool,
}

impl CubismMotion {
    /// Create a new motion instance from parsed JSON.
    ///
    /// `default_fade_in` / `default_fade_out` come from the model3.json `MotionRef`
    /// or fall back to 1.0 seconds (per Framework default).
    pub fn new(data: ParsedMotion, default_fade_in: f32, default_fade_out: f32) -> Self {
        let fade_in_seconds = if default_fade_in < 0.0 {
            1.0
        } else {
            default_fade_in
        };
        let fade_out_seconds = if default_fade_out < 0.0 {
            1.0
        } else {
            default_fade_out
        };

        let is_loop = data.r#loop;
        Self {
            data: Arc::new(data),
            fade_in_seconds,
            fade_out_seconds,
            weight: 1.0,
            is_loop,
            is_loop_fade_in: true,
        }
    }

    /// Duration in seconds. Returns -1.0 if looping.
    pub fn duration(&self) -> f32 {
        if self.is_loop {
            -1.0
        } else {
            self.data.duration
        }
    }

    /// Loop duration (always positive even for looping motions).
    pub fn loop_duration(&self) -> f32 {
        self.data.duration
    }

    /// Check if this motion has a model-opacity curve.
    pub fn has_model_opacity_curve(&self) -> bool {
        self.data
            .curves
            .iter()
            .any(|c| c.target == TARGET_NAME_MODEL && c.id == ID_NAME_OPACITY)
    }

    /// Evaluate curves and apply parameter values and part opacities.
    ///
    /// `eye_blink_indices` and `lip_sync_indices` are pre-computed parameter indices
    /// (built once per frame in `MotionQueueManager::do_update_motion`) to avoid
    /// redundant hashmap lookups per motion entry.
    #[allow(clippy::too_many_arguments)]
    pub fn do_update_parameters(
        &self,
        param_lookup: &HashMap<String, usize>,
        param_values: &mut [f32],
        user_time_seconds: f32,
        fade_weight: f32,
        entry_start_time: f32,
        entry_fade_in_start_time: f32,
        entry_end_time: f32,
        eye_blink_param_ids: &[String],
        lip_sync_param_ids: &[String],
        part_lookup: &HashMap<String, usize>,
        part_opacities: &mut [f32],
        eye_blink_indices: &[usize],
        lip_sync_indices: &[usize],
    ) -> f32 {
        let time_offset_seconds = (user_time_seconds - entry_start_time).max(0.0);

        let mut lip_sync_value = f32::MAX;
        let mut eye_blink_value = f32::MAX;
        let mut model_opacity = 1.0;

        let tmp_fade_in = if self.fade_in_seconds <= 0.0 {
            1.0
        } else {
            let t = (user_time_seconds - entry_fade_in_start_time) / self.fade_in_seconds;
            curve::easing_sine(t)
        };

        let tmp_fade_out = if self.fade_out_seconds <= 0.0 || entry_end_time < 0.0 {
            1.0
        } else {
            let t = (entry_end_time - user_time_seconds) / self.fade_out_seconds;
            curve::easing_sine(t)
        };

        // Wrap time for looping (V2 behavior)
        let duration = if self.is_loop && self.data.duration > 0.0 {
            self.data.duration + 1.0 / self.data.fps
        } else {
            self.data.duration
        };

        let mut time = time_offset_seconds;
        if self.is_loop && duration > 0.0 {
            while time > duration {
                time -= duration;
            }
        }

        let is_correction = self.is_loop;

        let mut eye_blink_flags: u64 = 0;
        let mut lip_sync_flags: u64 = 0;

        // 1. Evaluate Model curves
        for curve in &self.data.curves {
            if curve.target != TARGET_NAME_MODEL {
                continue;
            }
            let Some(value) = evaluate_curve(curve, time, is_correction, duration) else {
                continue;
            };

            match curve.id.as_str() {
                ID_NAME_OPACITY => {
                    model_opacity = value;
                }
                id => {
                    if eye_blink_param_ids.iter().any(|eid| eid == id) {
                        eye_blink_value = value;
                    }
                    if lip_sync_param_ids.iter().any(|lid| lid == id) {
                        lip_sync_value = value;
                    }
                }
            }
        }

        // 2. Evaluate Parameter curves
        for curve in &self.data.curves {
            if curve.target != TARGET_NAME_PARAMETER {
                continue;
            }
            let param_idx = match param_lookup.get(&curve.id) {
                Some(&idx) => idx,
                None => continue,
            };

            let source_value = param_values[param_idx];
            let Some(value) = evaluate_curve(curve, time, is_correction, duration) else {
                continue;
            };

            let mut final_value = value;
            if eye_blink_value != f32::MAX {
                for (bi, &eidx) in eye_blink_indices.iter().enumerate() {
                    if bi >= MAX_TARGET_BITS {
                        break;
                    }
                    if param_idx == eidx {
                        final_value *= eye_blink_value;
                        eye_blink_flags |= 1u64 << bi;
                        break;
                    }
                }
            }

            if lip_sync_value != f32::MAX {
                for (bi, &lidx) in lip_sync_indices.iter().enumerate() {
                    if bi >= MAX_TARGET_BITS {
                        break;
                    }
                    if param_idx == lidx {
                        final_value += lip_sync_value;
                        lip_sync_flags |= 1u64 << bi;
                        break;
                    }
                }
            }

            let v = if curve.has_per_param_fade {
                let fin = if curve.fade_in_time < 0.0 {
                    tmp_fade_in
                } else if curve.fade_in_time == 0.0 {
                    1.0
                } else {
                    let t = (user_time_seconds - entry_fade_in_start_time) / curve.fade_in_time;
                    curve::easing_sine(t)
                };

                let fout = if curve.fade_out_time < 0.0 {
                    tmp_fade_out
                } else if curve.fade_out_time == 0.0 || entry_end_time < 0.0 {
                    1.0
                } else {
                    let t = (entry_end_time - user_time_seconds) / curve.fade_out_time;
                    curve::easing_sine(t)
                };

                let param_weight = self.weight * fin * fout;
                source_value + (final_value - source_value) * param_weight
            } else {
                source_value + (final_value - source_value) * fade_weight
            };

            param_values[param_idx] = v;
        }

        // Apply eye blink to params not overwritten by motion curves
        if eye_blink_value != f32::MAX {
            for (bi, &eidx) in eye_blink_indices.iter().enumerate() {
                if bi >= MAX_TARGET_BITS {
                    break;
                }
                if (eye_blink_flags >> bi) & 1 != 0 {
                    continue;
                }
                let source_value = param_values[eidx];
                param_values[eidx] = source_value + (eye_blink_value - source_value) * fade_weight;
            }
        }

        // Apply lip sync to params not overwritten by motion curves
        if lip_sync_value != f32::MAX {
            for (bi, &lidx) in lip_sync_indices.iter().enumerate() {
                if bi >= MAX_TARGET_BITS {
                    break;
                }
                if (lip_sync_flags >> bi) & 1 != 0 {
                    continue;
                }
                let source_value = param_values[lidx];
                param_values[lidx] = source_value + (lip_sync_value - source_value) * fade_weight;
            }
        }

        // 3. Evaluate PartOpacity curves
        for curve in &self.data.curves {
            if curve.target != TARGET_NAME_PART_OPACITY {
                continue;
            }
            let part_idx = match part_lookup.get(&curve.id) {
                Some(&idx) => idx,
                None => continue,
            };
            let Some(target) = evaluate_curve(curve, time, is_correction, duration) else {
                continue;
            };
            if part_idx < part_opacities.len() {
                let source = part_opacities[part_idx];
                part_opacities[part_idx] = source + (target - source) * fade_weight;
            }
        }

        model_opacity
    }
}

/// Evaluate a single curve at the given time.
///
/// Returns `None` when the curve has no evaluable data (empty segment list or
/// a segment with no points, only possible with malformed motion3.json — the
/// parser always emits >= 2 points per segment). Callers skip the curve's
/// contribution entirely instead of blending in a garbage value.
fn evaluate_curve(
    curve: &ParsedCurve,
    time: f32,
    is_correction: bool,
    end_time: f32,
) -> Option<f32> {
    let segments = &curve.segments;
    if segments.is_empty() {
        return None;
    }

    // Find which segment contains `time`
    let mut target_seg_idx = None;

    for (i, seg) in segments.iter().enumerate() {
        // A segment with no points cannot contain `time`; skip it.
        let Some(end_pt) = seg.points.last() else {
            continue;
        };
        if end_pt.time > time {
            target_seg_idx = Some(i);
            break;
        }
    }

    let value = match target_seg_idx {
        Some(idx) => evaluate_segment_value(&segments[idx], time)?,
        None => {
            if is_correction && time < end_time {
                // Loop wrap correction
                correct_end_point(curve, time, end_time)?
            } else {
                // Time is past the last segment: hold the last known value.
                segments.iter().rev().find_map(|s| s.points.last())?.value
            }
        }
    };

    Some(value)
}

/// Correction for loop wrap.
fn correct_end_point(curve: &ParsedCurve, time: f32, end_time: f32) -> Option<f32> {
    let segments = &curve.segments;
    // Unwrap-free on purpose: missing segment/point data (malformed JSON)
    // yields None so the caller skips the curve instead of panicking.
    let last_seg = segments.last()?;
    let last_pt = last_seg.points.last()?;
    let first_pt = segments[0].points.first()?;

    // Interpolate from last point back to first point across the wrap boundary
    let base_time = last_pt.time;
    let target_time = first_pt.time + end_time;

    let span = target_time - base_time;
    if span == 0.0 {
        // Degenerate wrap span: division below would yield ±inf/NaN; hold the
        // last point's value.
        return Some(last_pt.value);
    }

    let t = ((time + end_time - base_time) / span).clamp(0.0, 1.0);

    Some(last_pt.value + (first_pt.value - last_pt.value) * t)
}

/// Evaluate a single segment at the given time.
fn evaluate_segment_value(seg: &super::json::ParsedSegment, time: f32) -> Option<f32> {
    let seg_type = SegmentType::from_i32(seg.segment_type);
    let pts = &seg.points;
    if pts.is_empty() {
        // No points to evaluate; only possible with malformed motion3.json.
        return None;
    }

    let value = match seg_type {
        SegmentType::Linear => {
            if pts.len() >= 2 {
                curve::linear_evaluate(pts, time)
            } else {
                pts[0].value
            }
        }
        SegmentType::Bezier => {
            if pts.len() >= 4 {
                // Use simple de Casteljau instead of Cardano — avoids
                // sin/cos/sqrt/cbrt per evaluation with negligible precision loss
                curve::bezier_evaluate_simple(pts, time)
            } else {
                pts[0].value
            }
        }
        SegmentType::Stepped => curve::stepped_evaluate(pts, time),
        SegmentType::InverseStepped => curve::inverse_stepped_evaluate(pts, time),
    };

    // Last line of defense: never let NaN/Inf reach model parameters.
    // Falls back to this segment's start value; well-formed inputs are unaffected.
    Some(if value.is_finite() {
        value
    } else {
        pts[0].value
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::parse_motion_json;

    #[test]
    fn test_simple_linear_curve() {
        let json = br#"{
            "Version": 3,
            "Meta": {
                "Duration": 2,
                "Fps": 30,
                "Loop": false,
                "AreBeziersRestricted": true,
                "CurveCount": 1,
                "TotalSegmentCount": 1,
                "TotalPointCount": 2
            },
            "Curves": [
                {
                    "Target": "Parameter",
                    "Id": "ParamA",
                    "Segments": [0, 0, 0, 2, 100]
                }
            ]
        }"#;
        let parsed = parse_motion_json(json).unwrap();
        let motion = CubismMotion::new(parsed, 0.0, 0.0);

        let param_lookup: HashMap<String, usize> =
            [("ParamA".to_string(), 0)].into_iter().collect();
        let mut param_values = vec![0.0f32];
        let eye_blink: Vec<String> = vec![];
        let lip_sync: Vec<String> = vec![];

        let part_lookup: HashMap<String, usize> = HashMap::new();
        let mut part_opacities: Vec<f32> = vec![];

        let empty_indices: &[usize] = &[];
        // At time 0
        motion.do_update_parameters(
            &param_lookup,
            &mut param_values,
            0.0,
            1.0,
            0.0,
            0.0,
            2.0,
            &eye_blink,
            &lip_sync,
            &part_lookup,
            &mut part_opacities,
            empty_indices,
            empty_indices,
        );
        assert!(
            (param_values[0] - 0.0).abs() < 1e-5,
            "t=0 got {}",
            param_values[0]
        );

        // At time 1 (midpoint)
        param_values[0] = 0.0;
        motion.do_update_parameters(
            &param_lookup,
            &mut param_values,
            1.0,
            1.0,
            0.0,
            0.0,
            2.0,
            &eye_blink,
            &lip_sync,
            &part_lookup,
            &mut part_opacities,
            empty_indices,
            empty_indices,
        );
        assert!(
            (param_values[0] - 50.0).abs() < 1e-5,
            "t=1 got {}",
            param_values[0]
        );

        // At time 2 (end)
        param_values[0] = 0.0;
        motion.do_update_parameters(
            &param_lookup,
            &mut param_values,
            2.0,
            1.0,
            0.0,
            0.0,
            2.0,
            &eye_blink,
            &lip_sync,
            &part_lookup,
            &mut part_opacities,
            empty_indices,
            empty_indices,
        );
        assert!(
            (param_values[0] - 100.0).abs() < 1e-5,
            "t=2 got {}",
            param_values[0]
        );
    }
}
