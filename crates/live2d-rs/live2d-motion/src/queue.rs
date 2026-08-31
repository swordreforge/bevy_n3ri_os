//! Motion queue management.
//!
//! Mirrors `CubismMotionQueueManager` and `CubismMotionQueueEntry` from the
//! Live2D Cubism Framework. Manages a queue of concurrently active motions
//! and evaluates them each frame.

use super::motion::CubismMotion;
use std::collections::HashMap;

/// A motion queue entry tracks per-instance state for a playing motion.
#[derive(Debug, Clone)]
pub struct MotionQueueEntry {
    /// The motion data (cloned so we can modify loop status independently).
    pub motion: CubismMotion,
    /// Whether this entry should be auto-deleted when finished.
    pub auto_delete: bool,

    // State
    pub available: bool,
    pub finished: bool,
    pub started: bool,
    pub start_time_seconds: f32,
    pub fade_in_start_time_seconds: f32,
    pub end_time_seconds: f32,
    pub state_time_seconds: f32,
    pub state_weight: f32,
    pub last_event_check_seconds: f32,
    pub fade_out_seconds: f32,
    pub is_triggered_fade_out: bool,

    /// Fade weight computed from motion-level fade in/out.
    pub cached_fade_weight: f32,

    /// Maximum number of loops before auto-finish. None = infinite.
    pub max_loop_count: Option<u32>,
}

impl MotionQueueEntry {
    pub fn new(motion: CubismMotion) -> Self {
        Self {
            motion,
            auto_delete: true,
            available: true,
            finished: false,
            started: false,
            start_time_seconds: 0.0,
            fade_in_start_time_seconds: 0.0,
            end_time_seconds: -1.0,
            state_time_seconds: 0.0,
            state_weight: 0.0,
            last_event_check_seconds: 0.0,
            fade_out_seconds: 0.0,
            is_triggered_fade_out: false,
            cached_fade_weight: 0.0,
            max_loop_count: None,
        }
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    pub fn is_started(&self) -> bool {
        self.started
    }

    pub fn is_available(&self) -> bool {
        self.available
    }

    pub fn set_finished(&mut self, f: bool) {
        self.finished = f;
    }

    pub fn set_started(&mut self, f: bool) {
        self.started = f;
    }

    pub fn set_fadeout(&mut self, fade_out_seconds: f32) {
        self.fade_out_seconds = fade_out_seconds;
    }

    pub fn start_fadeout(&mut self, fade_out_seconds: f32, user_time_seconds: f32) {
        self.fade_out_seconds = fade_out_seconds;
        self.end_time_seconds = user_time_seconds + fade_out_seconds;
        self.is_triggered_fade_out = true;
    }

    pub fn is_triggered_fade_out(&self) -> bool {
        self.is_triggered_fade_out
    }
}

/// Manages a queue of concurrent motion instances.
#[derive(Default)]
pub struct MotionQueueManager {
    /// Currently active (or fading out) motion entries.
    pub entries: Vec<MotionQueueEntry>,
    pub user_time_seconds: f32,
}

/// Handle returned when starting a motion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MotionQueueEntryHandle(pub usize);

impl MotionQueueManager {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            user_time_seconds: 0.0,
        }
    }

    /// Advance the global time for this manager.
    pub fn advance_time(&mut self, delta_seconds: f32) -> f32 {
        self.user_time_seconds += delta_seconds;
        self.user_time_seconds
    }

    /// Start a new motion. Sets existing motions to start fading out (anti-conflict).
    /// `max_loops` limits the number of loops for looping motions (None = infinite).
    /// Returns a handle that can be used to track this motion.
    ///
    /// Stack safety: silently refuses if total entries ≥ 3 (overflow guard).
    pub fn start_motion(
        &mut self,
        motion: CubismMotion,
        max_loops: Option<u32>,
    ) -> MotionQueueEntryHandle {
        // Safety cap: prevent stack overflow beyond 3 total entries
        if self.entries.len() >= 3 {
            return MotionQueueEntryHandle(self.entries.len());
        }

        // Anti-conflict: fade out all currently playing entries
        for entry in &mut self.entries {
            if entry.is_available() && !entry.is_finished() {
                entry.set_fadeout(motion.fade_out_seconds);
            }
        }

        let handle = MotionQueueEntryHandle(self.entries.len());
        let mut queue_entry = MotionQueueEntry::new(motion);
        queue_entry.set_finished(false);
        queue_entry.set_started(false);
        queue_entry.max_loop_count = max_loops;
        self.entries.push(queue_entry);

        handle
    }

    /// Update all active motions, evaluating curves and writing to parameters and part opacities.
    #[allow(clippy::too_many_arguments)]
    pub fn do_update_motion(
        &mut self,
        param_lookup: &HashMap<String, usize>,
        param_values: &mut [f32],
        eye_blink_param_ids: &[String],
        lip_sync_param_ids: &[String],
        part_lookup: &HashMap<String, usize>,
        part_opacities: &mut [f32],
    ) -> bool {
        let mut updated = false;
        let user_time = self.user_time_seconds;
        let mut finished_indices: Vec<usize> = Vec::new();

        // Pre-compute eye blink / lip sync parameter indices once per frame
        // (avoids redundant Hashmap lookups per motion entry per frame)
        let eye_blink_indices: Vec<usize> = eye_blink_param_ids
            .iter()
            .filter_map(|id| param_lookup.get(id).copied())
            .collect();
        let lip_sync_indices: Vec<usize> = lip_sync_param_ids
            .iter()
            .filter_map(|id| param_lookup.get(id).copied())
            .collect();

        // Process all entries in place using indices
        for i in 0..self.entries.len() {
            let entry = &mut self.entries[i];

            if !entry.is_available() || entry.is_finished() {
                finished_indices.push(i);
                continue;
            }

            // Setup motion on first update
            if !entry.is_started() {
                entry.set_started(true);
                entry.start_time_seconds = user_time;
                entry.fade_in_start_time_seconds = user_time;
                if entry.end_time_seconds < 0.0 {
                    let duration = entry.motion.duration();
                    // Looping motions have NO scheduled end (-1 sentinel):
                    // a finite end_time would ramp the fade-out weight to 0
                    // at the end of the first loop, leaving the motion playing
                    // invisibly forever. Fade-out for loops happens only via
                    // explicit set_fadeout (anti-conflict).
                    entry.end_time_seconds = if duration <= 0.0 || entry.motion.is_loop {
                        -1.0
                    } else {
                        user_time + duration
                    };
                }
            }

            // Compute fade weight (read-only on entry after this)
            let fade_weight = {
                let mut fw = entry.motion.weight;
                if entry.motion.fade_in_seconds != 0.0 {
                    let t = (user_time - entry.fade_in_start_time_seconds)
                        / entry.motion.fade_in_seconds;
                    fw *= super::curve::easing_sine(t);
                }
                if entry.motion.fade_out_seconds != 0.0 && entry.end_time_seconds >= 0.0 {
                    let t = (entry.end_time_seconds - user_time) / entry.motion.fade_out_seconds;
                    fw *= super::curve::easing_sine(t);
                }
                entry.cached_fade_weight = fw;
                fw
            };

            let entry_start_time = entry.start_time_seconds;
            let entry_fade_in_start = entry.fade_in_start_time_seconds;
            let entry_end_time = entry.end_time_seconds;
            let is_loop = entry.motion.is_loop;
            let duration = entry.motion.data.duration;

            entry.motion.do_update_parameters(
                param_lookup,
                param_values,
                user_time,
                fade_weight,
                entry_start_time,
                entry_fade_in_start,
                entry_end_time,
                eye_blink_param_ids,
                lip_sync_param_ids,
                part_lookup,
                part_opacities,
                &eye_blink_indices,
                &lip_sync_indices,
            );

            updated = true;

            // Check if finished
            let time_offset = user_time - entry_start_time;
            let is_done = !is_loop && time_offset >= duration;

            #[allow(clippy::if_same_then_else)]
            if is_done {
                entry.set_finished(true);
                finished_indices.push(i);
            } else if entry.is_triggered_fade_out() && entry_end_time < user_time {
                entry.set_finished(true);
                finished_indices.push(i);
            } else if is_loop && duration > 0.0 {
                // Enforce max loop count for looping motions
                if let Some(max_loops) = entry.max_loop_count {
                    if (time_offset / duration) as u32 >= max_loops {
                        entry.set_finished(true);
                        finished_indices.push(i);
                    }
                }
            }

            entry.last_event_check_seconds = user_time;
        }

        // Remove finished entries in reverse order
        for &idx in finished_indices.iter().rev() {
            self.entries.swap_remove(idx);
        }

        updated
    }

    /// Stop all motions immediately.
    pub fn stop_all_motions(&mut self) {
        self.entries.clear();
    }

    /// Check if all motions are finished.
    pub fn is_finished(&self) -> bool {
        self.entries.iter().all(|e| e.is_finished())
    }
}
