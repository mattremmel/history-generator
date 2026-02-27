use bevy_ecs::component::Component;

/// Full army state — single component per army entity.
#[derive(Component, Debug, Clone)]
pub struct ArmyState {
    pub morale: f64,
    pub supply: f64,
    pub strength: u32,
    pub faction_id: u64,
    pub home_region_id: u64,
    pub besieging_settlement_id: Option<u64>,
    pub months_campaigning: u32,
    pub starting_strength: u32,
    pub is_mercenary: bool,
}

impl Default for ArmyState {
    fn default() -> Self {
        Self {
            morale: 0.0,
            supply: 0.0,
            strength: 0,
            faction_id: 0,
            home_region_id: 0,
            besieging_settlement_id: None,
            months_campaigning: 0,
            starting_strength: 0,
            is_mercenary: false,
        }
    }
}
