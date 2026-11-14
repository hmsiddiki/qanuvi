//! Which model takes a step (ported from `src/lib/plan/routing.ts`).

use serde::{Deserialize, Serialize};

/// One model's record on one kind of work.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelProfile {
    pub model: String,
    pub tries: u32,
    pub successes: u32,
    pub tokens_per_success: f64,
    pub cost_per_success: f64,
    /// Burns far more tokens than the lightest model that also finishes this kind of work (1.5x the tokens per
    /// finished step, with at least three tries): picked only when a step really needs it.
    pub heavy: bool,
}

pub const HEAVY_FACTOR: f64 = 1.5;
