use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A single motion entry in V2 model.json.
#[derive(Debug, Deserialize)]
pub struct V2MotionEntry {
    pub file: String,
    pub sound: Option<String>,
}

/// Parse the `motions` section of a V2 model.json into a lookup map.
/// Returns `group_name -> Vec<(mtn_filename, Option<absolute_sound_path>)>`.
/// Sound paths are resolved to absolute paths relative to `base_dir`.
pub fn parse_v2_motions(
    json_text: &str,
    base_dir: &Path,
) -> HashMap<String, Vec<(String, Option<PathBuf>)>> {
    // Derive a local struct to deserialize just what we need.
    #[derive(Deserialize)]
    struct V2MotionsJson {
        motions: Option<HashMap<String, Vec<V2MotionEntry>>>,
    }

    let Ok(parsed) = serde_json::from_str::<V2MotionsJson>(json_text) else {
        return HashMap::new();
    };

    let Some(motions) = parsed.motions else {
        return HashMap::new();
    };

    motions
        .into_iter()
        .map(|(group, entries)| {
            let resolved: Vec<(String, Option<PathBuf>)> = entries
                .into_iter()
                .map(|e| {
                    let sound = e.sound.map(|s| base_dir.join(&s));
                    (e.file, sound)
                })
                .collect();
            (group, resolved)
        })
        .collect()
}
