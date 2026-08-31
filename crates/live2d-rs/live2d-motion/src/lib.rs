//! Motion/animation system for Live2D Cubism models.
//!
//! Implements the motion playback pipeline from the Cubism Framework:
//!
//! 1. **Parser** (`json`) — deserializes `.motion3.json` and `.exp3.json` files
//! 2. **Curve evaluator** (`curve`) — interpolates curve segments (linear, bezier, stepped)
//! 3. **Motion** (`motion`) — evaluates all curves in a motion against model parameters
//! 4. **Queue** (`queue`) — manages concurrent motion playback with fade in/out
//! 5. **Expression** (`expression`) — static parameter overrides with blend modes

pub mod breath;
pub mod curve;
mod expression;
pub mod eye_blink;
pub mod json;
pub mod look;
#[allow(clippy::module_inception)]
pub mod motion;
pub mod physics;
pub mod queue;

pub use curve::*;
pub use expression::*;
pub use json::*;
pub use motion::*;
pub use queue::*;
