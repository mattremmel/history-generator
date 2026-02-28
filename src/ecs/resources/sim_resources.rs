use bevy_ecs::resource::Resource;
use rand::rngs::SmallRng;

use crate::IdGenerator;
use crate::model::action::{Action, ActionResult};

/// Simulation configuration (start year, duration, output settings).
#[derive(Resource, Debug, Clone)]
pub struct EcsSimConfig {
    pub start_year: u32,
    pub num_years: u32,
    pub seed: u64,
    pub flush_interval: u32,
    pub output_dir: String,
}

impl Default for EcsSimConfig {
    fn default() -> Self {
        Self {
            start_year: 0,
            num_years: 1000,
            seed: 42,
            flush_interval: 50,
            output_dir: "output".to_string(),
        }
    }
}

/// Deterministic RNG for the simulation.
#[derive(Resource)]
pub struct SimRng(pub SmallRng);

/// Global ID generator for simulation entities.
#[derive(Resource, Default)]
pub struct EcsIdGenerator(pub IdGenerator);

/// Actions queued for processing this tick.
#[derive(Resource, Debug, Clone, Default)]
pub struct PendingActions(pub Vec<Action>);

/// Results from processed actions.
#[derive(Resource, Debug, Clone, Default)]
pub struct ActionResults(pub Vec<ActionResult>);

/// Captures reactive events from the current tick for Agency to consume next tick.
/// Mirrors the old `AgencySystem.recent_signals` pattern.
#[derive(Resource, Debug, Clone, Default)]
pub struct AgencyMemory(pub Vec<crate::ecs::events::SimReactiveEvent>);
