use std::collections::BTreeMap;

use bevy_ecs::entity::Entity;
use bevy_ecs::resource::Resource;

/// Bidirectional mapping between simulation IDs (u64) and Bevy entities.
#[derive(Resource, Debug, Clone, Default)]
pub struct SimEntityMap {
    to_bevy: BTreeMap<u64, Entity>,
    to_sim: BTreeMap<Entity, u64>,
}

impl SimEntityMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a mapping. Panics if the sim_id is already registered.
    pub fn insert(&mut self, sim_id: u64, entity: Entity) {
        let prev = self.to_bevy.insert(sim_id, entity);
        assert!(prev.is_none(), "duplicate sim_id {sim_id} in SimEntityMap");
        self.to_sim.insert(entity, sim_id);
    }

    /// Look up a Bevy entity by sim ID.
    pub fn get_bevy(&self, sim_id: u64) -> Option<Entity> {
        self.to_bevy.get(&sim_id).copied()
    }

    /// Look up a Bevy entity by sim ID. Panics if not found.
    pub fn bevy(&self, sim_id: u64) -> Entity {
        *self
            .to_bevy
            .get(&sim_id)
            .unwrap_or_else(|| panic!("no Bevy entity for sim_id {sim_id}"))
    }

    /// Look up a sim ID by Bevy entity.
    pub fn get_sim(&self, entity: Entity) -> Option<u64> {
        self.to_sim.get(&entity).copied()
    }

    /// Look up a sim ID by Bevy entity. Panics if not found.
    pub fn sim(&self, entity: Entity) -> u64 {
        *self
            .to_sim
            .get(&entity)
            .unwrap_or_else(|| panic!("no sim_id for entity {entity:?}"))
    }

    pub fn len(&self) -> usize {
        self.to_bevy.len()
    }

    pub fn is_empty(&self) -> bool {
        self.to_bevy.is_empty()
    }
}
