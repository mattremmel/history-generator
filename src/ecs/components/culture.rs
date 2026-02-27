use bevy_ecs::component::Component;

use crate::model::{CulturalValue, NamingStyle};

/// Full culture state — single component per culture entity.
#[derive(Component, Debug, Clone)]
pub struct CultureState {
    pub values: Vec<CulturalValue>,
    pub naming_style: NamingStyle,
    pub resistance: f64,
}
