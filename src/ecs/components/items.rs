use bevy_ecs::component::Component;

use crate::ecs::time::SimTime;
use crate::model::ItemType;

/// Full item state — single component per item entity.
#[derive(Component, Debug, Clone)]
pub struct ItemState {
    pub item_type: ItemType,
    pub material: String,
    pub resonance: f64,
    pub resonance_tier: u8,
    pub condition: f64,
    pub created: SimTime,
    pub last_transferred: Option<SimTime>,
}

impl Default for ItemState {
    fn default() -> Self {
        Self {
            item_type: ItemType::Tool,
            material: String::new(),
            resonance: 0.0,
            resonance_tier: 0,
            condition: 1.0,
            created: SimTime::default(),
            last_transferred: None,
        }
    }
}
