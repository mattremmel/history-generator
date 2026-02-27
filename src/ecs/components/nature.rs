use bevy_ecs::component::Component;

use crate::model::{FeatureType, ResourceType};

/// Full river state — single component per river entity.
#[derive(Component, Debug, Clone)]
pub struct RiverState {
    pub region_path: Vec<u64>,
    pub length: u32,
}

/// Full geographic feature state — single component per feature entity.
#[derive(Component, Debug, Clone)]
pub struct GeographicFeatureState {
    pub feature_type: FeatureType,
    pub x: f64,
    pub y: f64,
}

/// Full resource deposit state — single component per deposit entity.
#[derive(Component, Debug, Clone)]
pub struct ResourceDepositState {
    pub resource_type: ResourceType,
    pub quantity: u32,
    pub quality: f64,
    pub discovered: bool,
    pub x: f64,
    pub y: f64,
}
