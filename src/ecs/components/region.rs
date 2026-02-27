use bevy_ecs::component::Component;

use crate::model::{ResourceType, Terrain, TerrainTag};

/// Full region state — single component per region entity.
#[derive(Component, Debug, Clone)]
pub struct RegionState {
    pub terrain: Terrain,
    pub terrain_tags: Vec<TerrainTag>,
    pub x: f64,
    pub y: f64,
    pub resources: Vec<ResourceType>,
}

impl Default for RegionState {
    fn default() -> Self {
        Self {
            terrain: Terrain::Plains,
            terrain_tags: Vec::new(),
            x: 0.0,
            y: 0.0,
            resources: Vec::new(),
        }
    }
}
