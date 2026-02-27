use bevy_ecs::component::Component;

use crate::model::entity_data::{DeityDomain, ReligiousTenet};

/// Full religion state — single component per religion entity.
#[derive(Component, Debug, Clone)]
pub struct ReligionState {
    pub fervor: f64,
    pub proselytism: f64,
    pub orthodoxy: f64,
    pub tenets: Vec<ReligiousTenet>,
}

impl Default for ReligionState {
    fn default() -> Self {
        Self {
            fervor: 0.0,
            proselytism: 0.0,
            orthodoxy: 0.0,
            tenets: Vec::new(),
        }
    }
}

/// Full deity state — single component per deity entity.
#[derive(Component, Debug, Clone)]
pub struct DeityState {
    pub domain: DeityDomain,
    pub worship_strength: f64,
}
