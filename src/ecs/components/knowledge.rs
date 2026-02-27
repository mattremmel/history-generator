use bevy_ecs::component::Component;

use crate::ecs::time::SimTime;
use crate::model::{DerivationMethod, KnowledgeCategory, Medium};

/// Full knowledge state — single component per knowledge entity.
#[derive(Component, Debug, Clone)]
pub struct KnowledgeState {
    pub category: KnowledgeCategory,
    pub source_event_id: u64,
    pub origin_settlement_id: u64,
    pub origin_time: SimTime,
    pub significance: f64,
    pub ground_truth: serde_json::Value,
    pub revealed_at: Option<SimTime>,
}

/// Full manifestation state — single component per manifestation entity.
#[derive(Component, Debug, Clone)]
pub struct ManifestationState {
    pub knowledge_id: u64,
    pub medium: Medium,
    pub content: serde_json::Value,
    pub accuracy: f64,
    pub completeness: f64,
    pub distortions: Vec<serde_json::Value>,
    pub derived_from_id: Option<u64>,
    pub derivation_method: DerivationMethod,
    pub condition: f64,
    pub created: SimTime,
}
