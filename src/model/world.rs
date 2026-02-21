use std::collections::BTreeMap;

use super::entity::{Entity, EntityKind};
use super::event::{Event, EventKind, EventParticipant, ParticipantRole};
use super::relationship::{Relationship, RelationshipKind};
use crate::id::IdGenerator;

#[derive(Debug)]
pub struct World {
    pub entities: BTreeMap<u64, Entity>,
    pub events: BTreeMap<u64, Event>,
    pub event_participants: Vec<EventParticipant>,
    pub id_gen: IdGenerator,
    pub current_year: i32,
}

impl World {
    pub fn new() -> Self {
        Self {
            entities: BTreeMap::new(),
            events: BTreeMap::new(),
            event_participants: Vec::new(),
            id_gen: IdGenerator::new(),
            current_year: 0,
        }
    }

    /// Add an entity to the world, assigning it a unique ID.
    /// Returns the assigned ID.
    pub fn add_entity(&mut self, kind: EntityKind, name: String, birth_year: Option<i32>) -> u64 {
        let id = self.id_gen.next_id();
        let entity = Entity {
            id,
            kind,
            name,
            birth_year,
            death_year: None,
            relationships: Vec::new(),
        };
        self.entities.insert(id, entity);
        id
    }

    /// Add a relationship between two entities (stored inline on the source entity).
    ///
    /// # Panics
    /// Panics if `source_id` does not exist in the world.
    pub fn add_relationship(
        &mut self,
        source_id: u64,
        target_id: u64,
        kind: RelationshipKind,
        start_year: i32,
    ) {
        let rel = Relationship {
            source_entity_id: source_id,
            target_entity_id: target_id,
            kind,
            start_year,
            end_year: None,
        };
        let entity = self
            .entities
            .get_mut(&source_id)
            .unwrap_or_else(|| panic!("add_relationship: source entity {source_id} not found"));
        entity.relationships.push(rel);
    }

    /// Add an event to the world, assigning it a unique ID.
    /// Returns the assigned ID.
    pub fn add_event(&mut self, kind: EventKind, year: i32, description: String) -> u64 {
        let id = self.id_gen.next_id();
        let event = Event {
            id,
            kind,
            year,
            description,
        };
        self.events.insert(id, event);
        id
    }

    /// Add a participant to an event.
    ///
    /// # Panics
    /// Panics if `event_id` or `entity_id` does not exist in the world.
    pub fn add_event_participant(&mut self, event_id: u64, entity_id: u64, role: ParticipantRole) {
        assert!(
            self.events.contains_key(&event_id),
            "add_event_participant: event {event_id} not found"
        );
        assert!(
            self.entities.contains_key(&entity_id),
            "add_event_participant: entity {entity_id} not found"
        );
        self.event_participants.push(EventParticipant {
            event_id,
            entity_id,
            role,
        });
    }

    /// Extract all inline relationships from entities as an iterator.
    /// Used at flush time to normalize relationships for JSONL output.
    pub fn collect_relationships(&self) -> impl Iterator<Item = &Relationship> {
        self.entities
            .values()
            .flat_map(|e| &e.relationships)
    }
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_entity_assigns_unique_ids() {
        let mut world = World::new();
        let id1 = world.add_entity(EntityKind::Person, "Alice".to_string(), Some(100));
        let id2 = world.add_entity(EntityKind::Person, "Bob".to_string(), Some(105));
        assert_ne!(id1, id2);
        assert_eq!(world.entities.len(), 2);
    }

    #[test]
    fn add_entity_stores_correctly() {
        let mut world = World::new();
        let id = world.add_entity(EntityKind::Settlement, "Ironhold".to_string(), None);
        let entity = &world.entities[&id];
        assert_eq!(entity.name, "Ironhold");
        assert_eq!(entity.kind, EntityKind::Settlement);
        assert_eq!(entity.birth_year, None);
    }

    #[test]
    fn add_relationship_stored_on_source() {
        let mut world = World::new();
        let a = world.add_entity(EntityKind::Person, "A".to_string(), None);
        let b = world.add_entity(EntityKind::Person, "B".to_string(), None);
        world.add_relationship(a, b, RelationshipKind::Parent, 100);
        assert_eq!(world.entities[&a].relationships.len(), 1);
        assert_eq!(world.entities[&b].relationships.len(), 0);
    }

    #[test]
    fn collect_relationships_extracts_all() {
        let mut world = World::new();
        let a = world.add_entity(EntityKind::Person, "A".to_string(), None);
        let b = world.add_entity(EntityKind::Person, "B".to_string(), None);
        let c = world.add_entity(EntityKind::Person, "C".to_string(), None);
        world.add_relationship(a, b, RelationshipKind::Parent, 100);
        world.add_relationship(b, c, RelationshipKind::Ally, 150);
        assert_eq!(world.collect_relationships().count(), 2);
    }

    #[test]
    fn ids_shared_across_types() {
        let mut world = World::new();
        let entity_id = world.add_entity(EntityKind::Person, "A".to_string(), None);
        let event_id = world.add_event(EventKind::Birth, 100, "Born".to_string());
        // IDs come from the same generator, so they must differ
        assert_ne!(entity_id, event_id);
    }

    #[test]
    fn add_event_participant() {
        let mut world = World::new();
        let eid = world.add_entity(EntityKind::Person, "A".to_string(), None);
        let evid = world.add_event(EventKind::Birth, 100, "Born".to_string());
        world.add_event_participant(evid, eid, ParticipantRole::Subject);
        assert_eq!(world.event_participants.len(), 1);
        assert_eq!(world.event_participants[0].event_id, evid);
        assert_eq!(world.event_participants[0].entity_id, eid);
    }
}
