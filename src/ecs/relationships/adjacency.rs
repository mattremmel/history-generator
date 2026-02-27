use std::collections::BTreeMap;

use bevy_ecs::entity::Entity;
use bevy_ecs::resource::Resource;

/// Region adjacency graph — bidirectional, sorted neighbor lists.
///
/// BTreeMap for deterministic iteration.
#[derive(Resource, Debug, Clone, Default)]
pub struct RegionAdjacency {
    adjacency: BTreeMap<Entity, Vec<Entity>>,
}

impl RegionAdjacency {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a bidirectional edge. Maintains sorted neighbor lists.
    pub fn add_edge(&mut self, a: Entity, b: Entity) {
        let a_neighbors = self.adjacency.entry(a).or_default();
        if let Err(pos) = a_neighbors.binary_search(&b) {
            a_neighbors.insert(pos, b);
        }

        let b_neighbors = self.adjacency.entry(b).or_default();
        if let Err(pos) = b_neighbors.binary_search(&a) {
            b_neighbors.insert(pos, a);
        }
    }

    /// Get sorted neighbors of a region.
    pub fn neighbors(&self, region: Entity) -> &[Entity] {
        self.adjacency.get(&region).map_or(&[], |v| v.as_slice())
    }

    /// Check if two regions are adjacent.
    pub fn are_adjacent(&self, a: Entity, b: Entity) -> bool {
        self.adjacency
            .get(&a)
            .is_some_and(|neighbors| neighbors.binary_search(&b).is_ok())
    }
}
