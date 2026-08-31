//! Motion curve interpolation functions.
//!
//! Mirrors the Cubism Framework implementation in CubismMotion.cpp:
//! - LinearEvaluate, BezierEvaluate, SteppedEvaluate
//! - CubismMath::GetEasingSine for fade-in/out

use std::f32::consts::PI;

/// A single control point on a motion curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionPoint {
    pub time: f32,
    pub value: f32,
}

impl MotionPoint {
    pub const fn new(time: f32, value: f32) -> Self {
        Self { time, value }
    }
}

/// Segment types matching CubismMotionSegmentType.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentType {
    Linear = 0,
    Bezier = 1,
    Stepped = 2,
    InverseStepped = 3,
}

impl SegmentType {
    pub fn from_i32(v: i32) -> Self {
        match v {
            0 => SegmentType::Linear,
            1 => SegmentType::Bezier,
            2 => SegmentType::Stepped,
            3 => SegmentType::InverseStepped,
            _ => SegmentType::Linear,
        }
    }
}

/// Linearly interpolate between two points.
pub fn lerp_points(a: MotionPoint, b: MotionPoint, t: f32) -> MotionPoint {
    MotionPoint {
        time: a.time + (b.time - a.time) * t,
        value: a.value + (b.value - a.value) * t,
    }
}

/// Evaluate a linear segment: line from points[0] to points[1].
pub fn linear_evaluate(points: &[MotionPoint], time: f32) -> f32 {
    let dt = points[1].time - points[0].time;
    if dt == 0.0 {
        // Zero-length segment: division below would yield ±inf/NaN; hold start value.
        return points[0].value;
    }
    let t = ((time - points[0].time) / dt).clamp(0.0, 1.0);
    points[0].value + (points[1].value - points[0].value) * t
}

/// Evaluate a bezier segment (simple de Casteljau).
/// points[0] = start, points[1..2] = control, points[3] = end.
pub fn bezier_evaluate_simple(points: &[MotionPoint], time: f32) -> f32 {
    debug_assert!(points.len() >= 4);
    let dt = points[3].time - points[0].time;
    if dt == 0.0 {
        // Zero-length segment: division below would yield ±inf/NaN; hold start value.
        return points[0].value;
    }
    let t = ((time - points[0].time) / dt).clamp(0.0, 1.0);

    let p01 = lerp_points(points[0], points[1], t);
    let p12 = lerp_points(points[1], points[2], t);
    let p23 = lerp_points(points[2], points[3], t);

    let p012 = lerp_points(p01, p12, t);
    let p123 = lerp_points(p12, p23, t);

    lerp_points(p012, p123, t).value
}

/// Evaluate a bezier segment using Cardano's formula (more accurate time remapping).
/// Used when `AreBeziersRestricted` is false.
pub fn bezier_evaluate_cardano(points: &[MotionPoint], time: f32) -> f32 {
    debug_assert!(points.len() >= 4);

    let x = time;
    let x1 = points[0].time;
    let x2 = points[3].time;
    let cx1 = points[1].time;
    let cx2 = points[2].time;

    // Cubic coefficients: a*t^3 + b*t^2 + c*t + d = 0
    // For cubic bezier X = (1-t)^3*x1 + 3*(1-t)^2*t*cx1 + 3*(1-t)*t^2*cx2 + t^3*x2
    // Rearranged: a = x2 - 3*cx2 + 3*cx1 - x1
    let a = x2 - 3.0 * cx2 + 3.0 * cx1 - x1;
    let b = 3.0 * cx2 - 6.0 * cx1 + 3.0 * x1;
    let c = 3.0 * cx1 - 3.0 * x1;
    let d = x1 - x;

    let t = cardano_algorithm(a, b, c, d);

    // de Casteljau with the found t
    let p01 = lerp_points(points[0], points[1], t);
    let p12 = lerp_points(points[1], points[2], t);
    let p23 = lerp_points(points[2], points[3], t);
    let p012 = lerp_points(p01, p12, t);
    let p123 = lerp_points(p12, p23, t);
    lerp_points(p012, p123, t).value
}

/// Cardano's formula for solving cubic bezier time parameter.
fn cardano_algorithm(a: f32, b: f32, c: f32, d: f32) -> f32 {
    // If a is near zero, fall back to quadratic
    if a.abs() < 1e-8 {
        if b.abs() < 1e-8 {
            // Linear or constant
            if c.abs() < 1e-8 {
                return 0.0;
            }
            return (-d / c).clamp(0.0, 1.0);
        }
        // Quadratic: b*t^2 + c*t + d = 0
        let disc = c * c - 4.0 * b * d;
        if disc < 0.0 {
            return 0.0;
        }
        let sqrt_disc = disc.sqrt();
        let t1 = (-c + sqrt_disc) / (2.0 * b);
        let t2 = (-c - sqrt_disc) / (2.0 * b);
        // Return the root in [0, 1], default to 0
        if (0.0..=1.0).contains(&t1) {
            return t1;
        }
        if (0.0..=1.0).contains(&t2) {
            return t2;
        }
        return 0.0;
    }

    // Cubic: Convert to depressed cubic t^3 + p*t + q = 0
    let p = (3.0 * a * c - b * b) / (3.0 * a * a);
    let q = (2.0 * b * b * b - 9.0 * a * b * c + 27.0 * a * a * d) / (27.0 * a * a * a);

    let discriminant = q * q / 4.0 + p * p * p / 27.0;

    if discriminant < 0.0 {
        // Three real roots — use trigonometric solution
        let r = (-p * p * p / 27.0).sqrt();
        // FP rounding can push -q/(2r) marginally outside [-1,1]; clamp so acos
        // never sees it (out-of-range acos yields NaN, which would taint the root).
        let phi = (-q / (2.0 * r)).clamp(-1.0, 1.0).acos();
        let root = -2.0 * r.cbrt() * (phi / 3.0).cos() - b / (3.0 * a);
        return root.clamp(0.0, 1.0);
    }

    // One real root
    let sqrt_disc = discriminant.sqrt();
    let u = (-q / 2.0 + sqrt_disc).cbrt();
    let v = (-q / 2.0 - sqrt_disc).cbrt();
    let root = u + v - b / (3.0 * a);
    root.clamp(0.0, 1.0)
}

/// Stepped segment: holds the first point's value.
pub fn stepped_evaluate(points: &[MotionPoint], _time: f32) -> f32 {
    points[0].value
}

/// Inverse stepped: holds the second point's value.
pub fn inverse_stepped_evaluate(points: &[MotionPoint], _time: f32) -> f32 {
    points[1].value
}

/// Sine-wave easing (0.5 - 0.5*cos(pi*x)).
/// Clamps input to [0, 1] and maps to output [0, 1].
pub fn easing_sine(value: f32) -> f32 {
    if value <= 0.0 {
        0.0
    } else if value >= 1.0 {
        1.0
    } else {
        0.5 - 0.5 * (value * PI).cos()
    }
}

/// High-level dispatch: evaluate a segment type with its control points at the given time.
pub fn evaluate_segment(seg_type: SegmentType, points: &[MotionPoint], time: f32) -> f32 {
    match seg_type {
        SegmentType::Linear => linear_evaluate(points, time),
        SegmentType::Bezier => bezier_evaluate_cardano(points, time),
        SegmentType::Stepped => stepped_evaluate(points, time),
        SegmentType::InverseStepped => inverse_stepped_evaluate(points, time),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_easing_sine_boundaries() {
        assert!((easing_sine(0.0) - 0.0).abs() < 1e-6);
        assert!((easing_sine(1.0) - 1.0).abs() < 1e-6);
        assert!((easing_sine(0.5) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_linear_evaluate() {
        let pts = [MotionPoint::new(0.0, 0.0), MotionPoint::new(1.0, 10.0)];
        assert!((linear_evaluate(&pts, 0.0) - 0.0).abs() < 1e-6);
        assert!((linear_evaluate(&pts, 0.5) - 5.0).abs() < 1e-6);
        assert!((linear_evaluate(&pts, 1.0) - 10.0).abs() < 1e-6);
    }

    #[test]
    fn test_stepped_evaluate() {
        let pts = [MotionPoint::new(0.0, 42.0), MotionPoint::new(1.0, 99.0)];
        assert!((stepped_evaluate(&pts, 0.0) - 42.0).abs() < 1e-6);
        assert!((stepped_evaluate(&pts, 10.0) - 42.0).abs() < 1e-6);
    }
}
