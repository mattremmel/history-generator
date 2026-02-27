use std::collections::BTreeMap;

use bevy_ecs::entity::Entity;
use bevy_ecs::resource::Resource;

use crate::ecs::time::SimTime;

/// Metadata for a graph relationship (alliance, enmity, war, spouse).
#[derive(Debug, Clone)]
pub struct RelationshipMeta {
    pub start: SimTime,
    pub end: Option<SimTime>,
}

impl RelationshipMeta {
    pub fn new(start: SimTime) -> Self {
        Self { start, end: None }
    }

    pub fn is_active(&self) -> bool {
        self.end.is_none()
    }
}

/// Metadata for a trade route relationship.
#[derive(Debug, Clone)]
pub struct TradeRouteData {
    pub path: Vec<Entity>,
    pub distance: u32,
    pub resource: String,
    pub start: SimTime,
}

/// Graph-based relationships that don't map to Bevy structural relationships.
///
/// Keyed by canonical entity pairs (min entity first) for symmetric relations.
/// BTreeMap for deterministic iteration.
#[derive(Resource, Debug, Clone, Default)]
pub struct RelationshipGraph {
    pub allies: BTreeMap<(Entity, Entity), RelationshipMeta>,
    pub enemies: BTreeMap<(Entity, Entity), RelationshipMeta>,
    pub at_war: BTreeMap<(Entity, Entity), RelationshipMeta>,
    pub parent_child: BTreeMap<Entity, Vec<Entity>>,
    pub spouses: BTreeMap<(Entity, Entity), RelationshipMeta>,
    pub trade_routes: BTreeMap<(Entity, Entity), TradeRouteData>,
}

impl RelationshipGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the canonical pair ordering (smaller entity first).
    pub fn canonical_pair(a: Entity, b: Entity) -> (Entity, Entity) {
        if a <= b { (a, b) } else { (b, a) }
    }

    pub fn are_allies(&self, a: Entity, b: Entity) -> bool {
        let pair = Self::canonical_pair(a, b);
        self.allies.get(&pair).is_some_and(|m| m.is_active())
    }

    pub fn are_enemies(&self, a: Entity, b: Entity) -> bool {
        let pair = Self::canonical_pair(a, b);
        self.enemies.get(&pair).is_some_and(|m| m.is_active())
    }

    pub fn are_at_war(&self, a: Entity, b: Entity) -> bool {
        let pair = Self::canonical_pair(a, b);
        self.at_war.get(&pair).is_some_and(|m| m.is_active())
    }

    pub fn are_spouses(&self, a: Entity, b: Entity) -> bool {
        let pair = Self::canonical_pair(a, b);
        self.spouses.get(&pair).is_some_and(|m| m.is_active())
    }

    pub fn children_of(&self, parent: Entity) -> &[Entity] {
        self.parent_child.get(&parent).map_or(&[], |v| v.as_slice())
    }
}
