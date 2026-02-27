use bevy_ecs::component::Component;

use crate::ecs::time::SimTime;

/// Core identity component present on every ECS entity that maps to a simulation entity.
#[derive(Component, Debug, Clone)]
pub struct SimEntity {
    pub id: u64,
    pub name: String,
    pub origin: Option<SimTime>,
    pub end: Option<SimTime>,
}

impl SimEntity {
    pub fn is_alive(&self) -> bool {
        self.end.is_none()
    }
}

// ---------------------------------------------------------------------------
// Marker components — one per EntityKind
// ---------------------------------------------------------------------------

#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Person;

#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Settlement;

#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Faction;

#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Army;

#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Region;

#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Building;

#[derive(Component, Debug, Clone, Copy, Default)]
pub struct ItemMarker;

#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Deity;

#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Creature;

#[derive(Component, Debug, Clone, Copy, Default)]
pub struct River;

#[derive(Component, Debug, Clone, Copy, Default)]
pub struct GeographicFeature;

#[derive(Component, Debug, Clone, Copy, Default)]
pub struct ResourceDeposit;

#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Culture;

#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Disease;

#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Knowledge;

#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Manifestation;

#[derive(Component, Debug, Clone, Copy, Default)]
pub struct ReligionMarker;

// ---------------------------------------------------------------------------
// Meta-markers
// ---------------------------------------------------------------------------

/// Marks a player-controlled entity (meta-flag for external consumers).
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct IsPlayer;
