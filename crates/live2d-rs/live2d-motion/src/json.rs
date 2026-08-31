//! Deserializers for `.motion3.json` and `.exp3.json` files.
//!
//! The motion3.json `Segments` field is a flat `Vec<f32>` with the format:
//!
//! **First segment:** `[baseTime, baseValue, type, ...segmentPoints]`
//!   - Linear (type=0):  `[baseT, baseV, 0, endT, endV]` → 5 entries
//!   - Bezier (type=1):  `[baseT, baseV, 1, c1T, c1V, c2T, c2V, endT, endV]` → 9 entries
//!   - Stepped (type=2): `[baseT, baseV, 2, endT, endV]` → 5 entries
//!
//! **Subsequent segments:** (reuse previous segment's endpoint, so no baseTime/baseValue)
//!   - Linear (type=0):  `[0, endT, endV]` → 3 entries
//!   - Bezier (type=1):  `[1, c1T, c1V, c2T, c2V, endT, endV]` → 7 entries
//!   - Stepped (type=2): `[2, endT, endV]` → 3 entries
//!
//! After parsing, each segment stores its FULL set of points (including the base).
//! So a segment always has at least 2 points (base + end) except Bezier which has 4 (base + c1 + c2 + end).

use serde::de::{self, Deserializer};
use serde::Deserialize;

use super::curve::MotionPoint;

/// Deserialize `UserData` field leniently.
///
/// Official Cubism 3 motion3.json spec:
///   `"UserData": { "TotalUserDataSize": 0, "UserData": [...] }`
///
/// Some non-official models have `"UserData": []` (empty array) instead.
/// Return `None` for non-object values so the motion still loads.
fn deserialize_user_data<'de, D>(d: D) -> Result<Option<RawUserData>, D::Error>
where
    D: Deserializer<'de>,
{
    let v = serde_json::Value::deserialize(d)?;
    match v {
        serde_json::Value::Object(_) => serde_json::from_value(v)
            .map(Some)
            .map_err(de::Error::custom),
        _ => Ok(None),
    }
}

/// Raw motion3.json structure (serde-friendly).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct RawMotion3Json {
    pub version: u32,
    pub meta: RawMeta,
    pub curves: Vec<RawCurve>,
    #[serde(default, deserialize_with = "deserialize_user_data")]
    pub user_data: Option<RawUserData>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct RawMeta {
    pub duration: f32,
    pub fps: f32,
    pub r#loop: bool,
    #[serde(default)]
    pub are_beziers_restricted: bool,
    pub curve_count: i32,
    pub total_segment_count: i32,
    pub total_point_count: i32,
    #[serde(default)]
    pub user_data_count: i32,
    #[serde(default)]
    pub total_user_data_size: i32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct RawCurve {
    pub target: String,
    pub id: String,
    pub segments: Vec<f32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct RawUserData {
    pub total_user_data_size: i32,
    pub user_data: Option<Vec<RawUserDatum>>,
}

#[derive(Debug, Deserialize)]
pub struct RawUserDatum {
    pub time: f32,
    pub value: String,
    #[serde(default)]
    pub id: Option<String>,
}

/// A single parsed segment with ALL points included (base + control + end).
/// To evaluate: points[0] is the base, points[N-1] is the end.
/// For bezier: points = [base, c1, c2, end] (4 points)
/// For linear/stepped: points = [base, end] (2 points)
#[derive(Debug, Clone)]
pub struct ParsedSegment {
    pub segment_type: i32,
    pub points: Vec<MotionPoint>,
}

/// Parsed curve with all segments expanded.
#[derive(Debug, Clone)]
pub struct ParsedCurve {
    pub target: String,
    pub id: String,
    /// Per-segment fade in/out time. None means use motion-level fade.
    pub fade_in_time: f32,
    pub fade_out_time: f32,
    /// Whether this curve has per-parameter fade settings.
    pub has_per_param_fade: bool,
    pub segments: Vec<ParsedSegment>,
}

/// Parsed motion data (post-processing).
#[derive(Debug, Clone)]
pub struct ParsedMotion {
    pub duration: f32,
    pub fps: f32,
    pub r#loop: bool,
    pub are_beziers_restricted: bool,
    pub curves: Vec<ParsedCurve>,
    pub events: Vec<ParsedEvent>,
}

#[derive(Debug, Clone)]
pub struct ParsedEvent {
    pub fire_time: f32,
    pub value: String,
}

/// Flat array index constants for segment parsing.
/// First segment: [baseTime, baseValue, type, ...data...]
/// Subsequent segment: [type, ...data...]
const ENTRY_BASE_TIME: usize = 0;
const ENTRY_BASE_VALUE: usize = 1;
const ENTRY_FIRST_TYPE: usize = 2;

/// Number of entries consumed for segment data, based on segment type.
/// Returns (segment_type, num_entries_to_consume_from_data_start)
fn segment_data_entries(seg_type: i32) -> Option<(i32, usize)> {
    match seg_type {
        0 | 2 | 3 => Some((seg_type, 2)), // Linear/Stepped/InverseStepped: [endT, endV]
        1 => Some((seg_type, 6)),         // Bezier: [c1T, c1V, c2T, c2V, endT, endV]
        _ => None,
    }
}

/// Parse a raw motion3.json buffer into parsed motion data.
pub fn parse_motion_json(buffer: &[u8]) -> anyhow::Result<ParsedMotion> {
    let raw: RawMotion3Json = serde_json::from_slice(buffer)?;

    let mut curve_intermediates: Vec<Vec<f32>> = Vec::with_capacity(raw.meta.curve_count as usize);
    for raw_curve in &raw.curves {
        curve_intermediates.push(raw_curve.segments.clone());
    }

    let mut curves = Vec::with_capacity(curve_intermediates.len());
    for (raw_curve, flat_data) in raw.curves.iter().zip(curve_intermediates) {
        let segments = parse_curve_segments(&flat_data)?;
        curves.push(ParsedCurve {
            target: raw_curve.target.clone(),
            id: raw_curve.id.clone(),
            fade_in_time: -1.0,
            fade_out_time: -1.0,
            has_per_param_fade: false,
            segments,
        });
    }

    let events = match &raw.user_data {
        Some(ud) => ud
            .user_data
            .as_ref()
            .map(|data| {
                data.iter()
                    .map(|d| ParsedEvent {
                        fire_time: d.time,
                        value: d.value.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        None => Vec::new(),
    };

    Ok(ParsedMotion {
        duration: raw.meta.duration,
        fps: raw.meta.fps,
        r#loop: raw.meta.r#loop,
        are_beziers_restricted: raw.meta.are_beziers_restricted,
        curves,
        events,
    })
}

/// Parse the flat `Segments` array into segments with full point arrays.
fn parse_curve_segments(data: &[f32]) -> anyhow::Result<Vec<ParsedSegment>> {
    let mut segments: Vec<ParsedSegment> = Vec::new();
    let mut pos: usize = 0;

    while pos < data.len() {
        if segments.is_empty() {
            // First segment: [baseTime, baseValue, type, ...data]
            if pos + 3 > data.len() {
                anyhow::bail!(
                    "Truncated first segment: need at least 3 entries, have {}",
                    data.len() - pos
                );
            }
            let base_time = data[pos + ENTRY_BASE_TIME];
            let base_value = data[pos + ENTRY_BASE_VALUE];
            let raw_type = data[pos + ENTRY_FIRST_TYPE];

            let (seg_type, data_entries) =
                segment_data_entries(raw_type as i32).ok_or_else(|| {
                    anyhow::anyhow!(
                        "Unknown segment type {} at pos {}",
                        raw_type,
                        pos + ENTRY_FIRST_TYPE
                    )
                })?;

            if pos + ENTRY_FIRST_TYPE + 1 + data_entries > data.len() {
                anyhow::bail!(
                    "Truncated first segment of type {}: need {} data entries, have {}",
                    seg_type,
                    data_entries,
                    data.len() - pos - ENTRY_FIRST_TYPE - 1
                );
            }

            let data_start = pos + ENTRY_FIRST_TYPE + 1;
            let extra_points = extract_extra_points(seg_type, data, data_start)?;

            let mut full_points = vec![MotionPoint::new(base_time, base_value)];
            full_points.extend(extra_points);

            segments.push(ParsedSegment {
                segment_type: seg_type,
                points: full_points,
            });

            pos = data_start + data_entries;
        } else {
            // Subsequent segment: [type, ...data]
            if pos + 1 > data.len() {
                anyhow::bail!("Truncated subsequent segment: need at least type entry");
            }
            let raw_type = data[pos];

            let (seg_type, data_entries) =
                segment_data_entries(raw_type as i32).ok_or_else(|| {
                    anyhow::anyhow!("Unknown segment type {} at pos {}", raw_type, pos)
                })?;

            if pos + 1 + data_entries > data.len() {
                anyhow::bail!(
                    "Truncated subsequent segment of type {}: need {} data entries, have {}",
                    seg_type,
                    data_entries,
                    data.len() - pos - 1
                );
            }

            // Base point is the previous segment's endpoint
            let prev_seg = segments.last().unwrap();
            let base_point = *prev_seg.points.last().unwrap();

            let data_start = pos + 1;
            let extra_points = extract_extra_points(seg_type, data, data_start)?;

            let mut full_points = vec![base_point];
            full_points.extend(extra_points);

            segments.push(ParsedSegment {
                segment_type: seg_type,
                points: full_points,
            });

            pos = data_start + data_entries;
        }
    }

    Ok(segments)
}

/// Extract control/end points for a segment type from the data array.
fn extract_extra_points(
    seg_type: i32,
    data: &[f32],
    start: usize,
) -> anyhow::Result<Vec<MotionPoint>> {
    match seg_type {
        0 | 2 | 3 => {
            // Linear/Stepped/InverseStepped: [endTime, endValue]
            Ok(vec![MotionPoint::new(data[start], data[start + 1])])
        }
        1 => {
            // Bezier: [c1T, c1V, c2T, c2V, endT, endV]
            Ok(vec![
                MotionPoint::new(data[start], data[start + 1]),
                MotionPoint::new(data[start + 2], data[start + 3]),
                MotionPoint::new(data[start + 4], data[start + 5]),
            ])
        }
        _ => anyhow::bail!("Unknown segment type {}", seg_type),
    }
}

// ---------------------------------------------------------------------------
// Expression motion (.exp3.json)
// ---------------------------------------------------------------------------

/// Raw exp3.json structure.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct RawExp3Json {
    #[serde(rename = "Type")]
    pub exp_type: String,
    #[serde(default)]
    pub parameters: Vec<RawExp3Param>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawExp3Param {
    #[serde(rename = "Id")]
    pub id: String,
    #[serde(rename = "Value")]
    pub value: f32,
    #[serde(rename = "Blend", default)]
    pub blend: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpressionBlendMode {
    Override,
    Add,
    Multiply,
}

/// Parsed expression data.
#[derive(Debug, Clone)]
pub struct ParsedExpression {
    pub parameters: Vec<ParsedExp3Param>,
}

#[derive(Debug, Clone)]
pub struct ParsedExp3Param {
    pub id: String,
    pub value: f32,
    pub blend: ExpressionBlendMode,
}

/// Parse a raw exp3.json buffer into parsed expression data.
pub fn parse_expression_json(buffer: &[u8]) -> anyhow::Result<ParsedExpression> {
    let raw: RawExp3Json = serde_json::from_slice(buffer)?;

    let parameters: Vec<ParsedExp3Param> = raw
        .parameters
        .iter()
        .map(|p| {
            let blend = match p.blend.to_lowercase().as_str() {
                "add" => ExpressionBlendMode::Add,
                "multiply" => ExpressionBlendMode::Multiply,
                _ => ExpressionBlendMode::Override,
            };
            ParsedExp3Param {
                id: p.id.clone(),
                value: p.value,
                blend,
            }
        })
        .collect();

    Ok(ParsedExpression { parameters })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_linear() {
        // Curve: target=Model, id=Opacity, 1 linear segment from (0,1) to (10,1)
        let json = br#"{
            "Version": 3,
            "Meta": {
                "Duration": 10,
                "Fps": 30.0,
                "Loop": true,
                "AreBeziersRestricted": true,
                "CurveCount": 1,
                "TotalSegmentCount": 1,
                "TotalPointCount": 2
            },
            "Curves": [
                {
                    "Target": "Model",
                    "Id": "Opacity",
                    "Segments": [0, 1, 0, 10, 1]
                }
            ]
        }"#;

        let parsed = parse_motion_json(json).unwrap();
        assert_eq!(parsed.duration, 10.0);
        assert_eq!(parsed.curves.len(), 1);
        assert_eq!(parsed.curves[0].target, "Model");
        assert_eq!(parsed.curves[0].id, "Opacity");
        assert_eq!(parsed.curves[0].segments.len(), 1);

        let seg0 = &parsed.curves[0].segments[0];
        assert_eq!(seg0.segment_type, 0); // linear
        assert_eq!(seg0.points.len(), 2); // base + end
        assert!((seg0.points[0].time - 0.0).abs() < 1e-6);
        assert!((seg0.points[0].value - 1.0).abs() < 1e-6);
        assert!((seg0.points[1].time - 10.0).abs() < 1e-6);
        assert!((seg0.points[1].value - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_parse_two_linear_segments() {
        // Curve: parameter ParamAngleX, 2 linear segments
        // Seg 1: (0,0) -> (0.5, 15)
        // Seg 2: (0.5, 15) -> (3, 15)
        let json = br#"{
            "Version": 3,
            "Meta": {
                "Duration": 3,
                "Fps": 30.0,
                "Loop": false,
                "AreBeziersRestricted": true,
                "CurveCount": 1,
                "TotalSegmentCount": 2,
                "TotalPointCount": 3
            },
            "Curves": [
                {
                    "Target": "Parameter",
                    "Id": "ParamAngleX",
                    "Segments": [0, 0, 0, 0.5, 15, 0, 3, 15]
                }
            ]
        }"#;

        let parsed = parse_motion_json(json).unwrap();
        assert_eq!(parsed.curves[0].segments.len(), 2);

        let seg0 = &parsed.curves[0].segments[0];
        assert_eq!(seg0.segment_type, 0); // linear
        assert_eq!(seg0.points.len(), 2);
        assert!((seg0.points[0].time - 0.0).abs() < 1e-6);
        assert!((seg0.points[1].time - 0.5).abs() < 1e-6);

        let seg1 = &parsed.curves[0].segments[1];
        assert_eq!(seg1.segment_type, 0); // linear
        assert_eq!(seg1.points.len(), 2);
        assert!((seg1.points[0].time - 0.5).abs() < 1e-6); // inherited base
        assert!((seg1.points[0].value - 15.0).abs() < 1e-6);
        assert!((seg1.points[1].time - 3.0).abs() < 1e-6);
    }

    #[test]
    fn test_parse_two_bezier_segments() {
        // Simulating real motion data: 2 bezier segments
        // Seg 1: base(0,0) bezier to (0.533,-4) with c1(0.178,0) c2(0.356,-4)
        // Seg 2: from (0.533,-4) bezier to (1.9,-4) with c1(0.989,-4) c2(1.444,-4)
        let json = br#"{
            "Version": 3,
            "Meta": {
                "Duration": 6.664,
                "Fps": 30.0,
                "Loop": true,
                "AreBeziersRestricted": true,
                "CurveCount": 1,
                "TotalSegmentCount": 2,
                "TotalPointCount": 7
            },
            "Curves": [
                {
                    "Target": "Parameter",
                    "Id": "ParamAngleX",
                    "Segments": [
                        0, 0, 1, 0.178, 0, 0.356, -4, 0.533, -4,
                        1, 0.989, -4, 1.444, -4, 1.9, -4
                    ]
                }
            ]
        }"#;

        let parsed = parse_motion_json(json).unwrap();
        assert_eq!(parsed.curves[0].segments.len(), 2);

        let seg0 = &parsed.curves[0].segments[0];
        assert_eq!(seg0.segment_type, 1); // bezier
        assert_eq!(seg0.points.len(), 4); // base + c1 + c2 + end
        assert!((seg0.points[0].time - 0.0).abs() < 1e-6);
        assert!((seg0.points[0].value - 0.0).abs() < 1e-6);
        assert!((seg0.points[3].time - 0.533).abs() < 1e-6);
        assert!((seg0.points[3].value - (-4.0)).abs() < 1e-6);

        let seg1 = &parsed.curves[0].segments[1];
        assert_eq!(seg1.segment_type, 1); // bezier
        assert_eq!(seg1.points.len(), 4);
        // Base inherits from seg0 endpoint
        assert!((seg1.points[0].time - 0.533).abs() < 1e-6);
        assert!((seg1.points[0].value - (-4.0)).abs() < 1e-6);
        assert!((seg1.points[3].time - 1.9).abs() < 1e-6);
        assert!((seg1.points[3].value - (-4.0)).abs() < 1e-6);
    }

    #[test]
    fn test_parse_expression_json() {
        let json = br#"{
            "Type": "Live2D Expression",
            "Parameters": [
                {"Id": "ParamMouthForm", "Value": 0.27, "Blend": "Add"}
            ]
        }"#;

        let parsed = parse_expression_json(json).unwrap();
        assert_eq!(parsed.parameters.len(), 1);
        assert_eq!(parsed.parameters[0].id, "ParamMouthForm");
        assert_eq!(parsed.parameters[0].blend, ExpressionBlendMode::Add);
    }
}
