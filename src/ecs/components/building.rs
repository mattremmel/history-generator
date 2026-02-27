use bevy_ecs::component::Component;

use crate::ecs::time::SimTime;
use crate::model::{BuildingType, ResourceType};

/// Full building state — single component per building entity.
#[derive(Component, Debug, Clone)]
pub struct BuildingState {
    pub building_type: BuildingType,
    pub output_resource: Option<ResourceType>,
    pub x: f64,
    pub y: f64,
    pub condition: f64,
    pub level: u8,
    pub constructed: SimTime,
}

impl Default for BuildingState {
    fn default() -> Self {
        Self {
            building_type: BuildingType::Mine,
            output_resource: None,
            x: 0.0,
            y: 0.0,
            condition: 1.0,
            level: 0,
            constructed: SimTime::default(),
        }
    }
}
