use bevy_ecs::component::Component;

use crate::model::population::NUM_BRACKETS;

/// Full disease state — single component per disease entity.
#[derive(Component, Debug, Clone)]
pub struct DiseaseState {
    pub virulence: f64,
    pub lethality: f64,
    pub duration_years: u32,
    pub bracket_severity: [f64; NUM_BRACKETS],
}
