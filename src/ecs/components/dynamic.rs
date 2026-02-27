use bevy_ecs::component::Component;

use crate::ecs::time::SimTime;
use crate::model::DisasterType;

/// Active siege on a settlement — added/removed dynamically.
#[derive(Component, Debug, Clone)]
pub struct EcsActiveSiege {
    pub attacker_army_id: u64,
    pub attacker_faction_id: u64,
    pub started: SimTime,
    pub months_elapsed: u32,
    pub civilian_deaths: u32,
}

/// Active disease outbreak in a settlement — added/removed dynamically.
#[derive(Component, Debug, Clone)]
pub struct EcsActiveDisease {
    pub disease_id: u64,
    pub started: SimTime,
    pub infection_rate: f64,
    pub peak_reached: bool,
    pub total_deaths: u32,
}

/// Active disaster in a settlement — added/removed dynamically.
#[derive(Component, Debug, Clone)]
pub struct EcsActiveDisaster {
    pub disaster_type: DisasterType,
    pub severity: f64,
    pub started: SimTime,
    pub months_remaining: u32,
    pub total_deaths: u32,
}
