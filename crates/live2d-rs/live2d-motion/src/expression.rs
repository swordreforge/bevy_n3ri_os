//! Expression motion support (.exp3.json).
//!
//! Expressions are simpler than full motions — they directly set parameter values
//! with a blend mode (Override, Add, Multiply). They have no curves, no segments,
//! no timing — they are a static set of parameter overrides.

use super::json::{ExpressionBlendMode, ParsedExpression};
use std::collections::HashMap;

/// An expression instance. Applies a set of parameter overrides.
#[derive(Debug, Clone)]
pub struct ExpressionMotion {
    /// Parsed expression data.
    pub data: ParsedExpression,
    /// Fade-in time in seconds (expressions can fade).
    pub fade_in_seconds: f32,
    /// Current weight for blending.
    pub weight: f32,
}

impl ExpressionMotion {
    pub fn new(data: ParsedExpression) -> Self {
        Self {
            data,
            fade_in_seconds: 0.5,
            weight: 1.0,
        }
    }

    /// Apply expression parameters.
    ///
    /// * `param_lookup` — parameter ID -> index map
    /// * `param_values` — mutable parameter value buffer (modified in place)
    /// * `fade_weight` — combined fade weight (easing sine)
    pub fn apply(
        &self,
        param_lookup: &HashMap<String, usize>,
        param_values: &mut [f32],
        fade_weight: f32,
    ) {
        for param in &self.data.parameters {
            let idx = match param_lookup.get(&param.id) {
                Some(&i) => i,
                None => continue,
            };

            let current = param_values[idx];
            let target = param.value;

            let blended = match param.blend {
                ExpressionBlendMode::Override => current + (target - current) * fade_weight,
                ExpressionBlendMode::Add => current + target * fade_weight,
                ExpressionBlendMode::Multiply => current * (1.0 + (target - 1.0) * fade_weight),
            };

            param_values[idx] = blended;
        }
    }
}

/// Manages expression state — a single active expression with fade.
#[derive(Clone)]
pub struct ExpressionManager {
    pub current_expression: Option<ExpressionMotion>,
    pub expression_start_time: f32,
    pub is_active: bool,
}

impl Default for ExpressionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ExpressionManager {
    pub fn new() -> Self {
        Self {
            current_expression: None,
            expression_start_time: 0.0,
            is_active: false,
        }
    }

    /// Start a new expression, replacing any current one.
    pub fn start_expression(&mut self, expression: ExpressionMotion, user_time: f32) {
        self.current_expression = Some(expression);
        self.expression_start_time = user_time;
        self.is_active = true;
    }

    /// Clear the current expression.
    pub fn clear(&mut self) {
        self.current_expression = None;
        self.is_active = false;
    }

    /// Apply the expression with current time.
    pub fn apply(
        &mut self,
        param_lookup: &HashMap<String, usize>,
        param_values: &mut [f32],
        user_time: f32,
    ) {
        if !self.is_active {
            return;
        }

        let expression = match &self.current_expression {
            Some(e) => e,
            None => return,
        };

        let elapsed = user_time - self.expression_start_time;
        if elapsed < 0.0 {
            return;
        }

        // Fade in over the expression's fade time
        let fade_weight = if expression.fade_in_seconds > 0.0 {
            let t = (elapsed / expression.fade_in_seconds).min(1.0);
            super::curve::easing_sine(t)
        } else {
            1.0
        };

        expression.apply(param_lookup, param_values, fade_weight * expression.weight);
    }
}
