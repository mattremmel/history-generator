use std::collections::BTreeMap;

use super::effect::{EventEffect, StateChange};
use super::entity::{Entity, EntityKind};
use super::event::{Event, EventKind, EventParticipant, ParticipantRole};
use super::relationship::{Relationship, RelationshipKind};
use super::timestamp::SimTimestamp;
use crate::id::IdGenerator;

#[derive(Debug)]
pub struct World {
    pub entities: BTreeMap<u64, Entity>,
    pub events: BTreeMap<u64, Event>,
    pub event_participants: Vec<EventParticipant>,
    pub event_effects: Vec<EventEffect>,
    pub id_gen: IdGenerator,
    pub current_time: SimTimestamp,
}

impl World {
    pub fn new() -> Self {
        Self {
            entities: BTreeMap::new(),
            events: BTreeMap::new(),
            event_participants: Vec::new(),
            event_effects: Vec::new(),
            id_gen: IdGenerator::new(),
            current_time: SimTimestamp::from_year(0),
        }
    }

    /// Add an event to the world, assigning it a unique ID.
    /// Returns the assigned ID.
    pub fn add_event(
        &mut self,
        kind: EventKind,
        timestamp: SimTimestamp,
        description: String,
    ) -> u64 {
        let id = self.id_gen.next_id();
        let event = Event {
            id,
            kind,
            timestamp,
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

    /// Add an entity to the world, assigning it a unique ID.
    /// Records an `EntityCreated` effect linked to the given event.
    /// Returns the assigned ID.
    ///
    /// # Panics
    /// Panics if `event_id` does not exist in the world.
    pub fn add_entity(
        &mut self,
        kind: EntityKind,
        name: String,
        origin: Option<SimTimestamp>,
        event_id: u64,
    ) -> u64 {
        assert!(
            self.events.contains_key(&event_id),
            "add_entity: event {event_id} not found"
        );
        let id = self.id_gen.next_id();
        let entity = Entity {
            id,
            kind: kind.clone(),
            name: name.clone(),
            origin,
            end: None,
            relationships: Vec::new(),
        };
        self.entities.insert(id, entity);
        self.event_effects.push(EventEffect {
            event_id,
            entity_id: id,
            effect: StateChange::EntityCreated { kind, name },
        });
        id
    }

    /// Add a relationship between two entities (stored inline on the source entity).
    /// Records a `RelationshipStarted` effect linked to the given event.
    ///
    /// # Panics
    /// Panics if `source_id` or `event_id` does not exist in the world.
    pub fn add_relationship(
        &mut self,
        source_id: u64,
        target_id: u64,
        kind: RelationshipKind,
        start: SimTimestamp,
        event_id: u64,
    ) {
        assert!(
            self.events.contains_key(&event_id),
            "add_relationship: event {event_id} not found"
        );
        let rel = Relationship {
            source_entity_id: source_id,
            target_entity_id: target_id,
            kind: kind.clone(),
            start,
            end: None,
        };
        let entity = self
            .entities
            .get_mut(&source_id)
            .unwrap_or_else(|| panic!("add_relationship: source entity {source_id} not found"));
        entity.relationships.push(rel);
        self.event_effects.push(EventEffect {
            event_id,
            entity_id: source_id,
            effect: StateChange::RelationshipStarted {
                target_entity_id: target_id,
                kind,
            },
        });
    }

    /// Rename an entity. Records a `NameChanged` effect.
    ///
    /// # Panics
    /// Panics if `entity_id` or `event_id` does not exist in the world.
    pub fn rename_entity(&mut self, entity_id: u64, new_name: String, event_id: u64) {
        assert!(
            self.events.contains_key(&event_id),
            "rename_entity: event {event_id} not found"
        );
        let entity = self
            .entities
            .get_mut(&entity_id)
            .unwrap_or_else(|| panic!("rename_entity: entity {entity_id} not found"));
        let old_name = std::mem::replace(&mut entity.name, new_name.clone());
        self.event_effects.push(EventEffect {
            event_id,
            entity_id,
            effect: StateChange::NameChanged {
                old: old_name,
                new: new_name,
            },
        });
    }

    /// End an entity (set its end timestamp). Records an `EntityEnded` effect.
    ///
    /// # Panics
    /// Panics if `entity_id` or `event_id` does not exist in the world.
    pub fn end_entity(&mut self, entity_id: u64, timestamp: SimTimestamp, event_id: u64) {
        assert!(
            self.events.contains_key(&event_id),
            "end_entity: event {event_id} not found"
        );
        let entity = self
            .entities
            .get_mut(&entity_id)
            .unwrap_or_else(|| panic!("end_entity: entity {entity_id} not found"));
        entity.end = Some(timestamp);
        self.event_effects.push(EventEffect {
            event_id,
            entity_id,
            effect: StateChange::EntityEnded,
        });
    }

    /// End a relationship. Records a `RelationshipEnded` effect.
    ///
    /// # Panics
    /// Panics if `source_id` or `event_id` does not exist, or if no matching relationship is found.
    pub fn end_relationship(
        &mut self,
        source_id: u64,
        target_id: u64,
        kind: &RelationshipKind,
        timestamp: SimTimestamp,
        event_id: u64,
    ) {
        assert!(
            self.events.contains_key(&event_id),
            "end_relationship: event {event_id} not found"
        );
        let entity = self
            .entities
            .get_mut(&source_id)
            .unwrap_or_else(|| panic!("end_relationship: source entity {source_id} not found"));
        let rel = entity
            .relationships
            .iter_mut()
            .find(|r| r.target_entity_id == target_id && &r.kind == kind && r.end.is_none())
            .unwrap_or_else(|| {
                panic!("end_relationship: no active relationship from {source_id} to {target_id}")
            });
        rel.end = Some(timestamp);
        self.event_effects.push(EventEffect {
            event_id,
            entity_id: source_id,
            effect: StateChange::RelationshipEnded {
                target_entity_id: target_id,
                kind: kind.clone(),
            },
        });
    }

    /// Extract all inline relationships from entities as an iterator.
    /// Used at flush time to normalize relationships for JSONL output.
    pub fn collect_relationships(&self) -> impl Iterator<Item = &Relationship> {
        self.entities.values().flat_map(|e| &e.relationships)
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

    fn ts(year: u32) -> SimTimestamp {
        SimTimestamp::from_year(year)
    }

    #[test]
    fn add_entity_assigns_unique_ids() {
        let mut world = World::new();
        let ev = world.add_event(EventKind::Birth, ts(100), "Born".to_string());
        let id1 = world.add_entity(EntityKind::Person, "Alice".to_string(), Some(ts(100)), ev);
        let ev2 = world.add_event(EventKind::Birth, ts(105), "Born".to_string());
        let id2 = world.add_entity(EntityKind::Person, "Bob".to_string(), Some(ts(105)), ev2);
        assert_ne!(id1, id2);
        assert_eq!(world.entities.len(), 2);
    }

    #[test]
    fn add_entity_stores_correctly() {
        let mut world = World::new();
        let ev = world.add_event(EventKind::SettlementFounded, ts(0), "Founded".to_string());
        let id = world.add_entity(EntityKind::Settlement, "Ironhold".to_string(), None, ev);
        let entity = &world.entities[&id];
        assert_eq!(entity.name, "Ironhold");
        assert_eq!(entity.kind, EntityKind::Settlement);
        assert_eq!(entity.origin, None);
    }

    #[test]
    fn add_entity_records_effect() {
        let mut world = World::new();
        let ev = world.add_event(EventKind::Birth, ts(100), "Born".to_string());
        let id = world.add_entity(EntityKind::Person, "Alice".to_string(), Some(ts(100)), ev);
        assert_eq!(world.event_effects.len(), 1);
        assert_eq!(world.event_effects[0].event_id, ev);
        assert_eq!(world.event_effects[0].entity_id, id);
        assert_eq!(
            world.event_effects[0].effect,
            StateChange::EntityCreated {
                kind: EntityKind::Person,
                name: "Alice".to_string(),
            }
        );
    }

    #[test]
    fn add_relationship_stored_on_source() {
        let mut world = World::new();
        let ev = world.add_event(EventKind::Birth, ts(0), "Born".to_string());
        let a = world.add_entity(EntityKind::Person, "A".to_string(), None, ev);
        let ev2 = world.add_event(EventKind::Birth, ts(0), "Born".to_string());
        let b = world.add_entity(EntityKind::Person, "B".to_string(), None, ev2);
        let ev3 = world.add_event(EventKind::Marriage, ts(100), "Married".to_string());
        world.add_relationship(a, b, RelationshipKind::Parent, ts(100), ev3);
        assert_eq!(world.entities[&a].relationships.len(), 1);
        assert_eq!(world.entities[&b].relationships.len(), 0);
    }

    #[test]
    fn add_relationship_records_effect() {
        let mut world = World::new();
        let ev = world.add_event(EventKind::Birth, ts(0), "Born".to_string());
        let a = world.add_entity(EntityKind::Person, "A".to_string(), None, ev);
        let ev2 = world.add_event(EventKind::Birth, ts(0), "Born".to_string());
        let b = world.add_entity(EntityKind::Person, "B".to_string(), None, ev2);
        let ev3 = world.add_event(EventKind::Marriage, ts(100), "Married".to_string());
        world.add_relationship(a, b, RelationshipKind::Spouse, ts(100), ev3);
        // 2 entity created effects + 1 relationship started effect
        assert_eq!(world.event_effects.len(), 3);
        let last = &world.event_effects[2];
        assert_eq!(last.event_id, ev3);
        assert_eq!(last.entity_id, a);
        assert_eq!(
            last.effect,
            StateChange::RelationshipStarted {
                target_entity_id: b,
                kind: RelationshipKind::Spouse,
            }
        );
    }

    #[test]
    fn collect_relationships_extracts_all() {
        let mut world = World::new();
        let ev = world.add_event(EventKind::Birth, ts(0), "Born".to_string());
        let a = world.add_entity(EntityKind::Person, "A".to_string(), None, ev);
        let ev2 = world.add_event(EventKind::Birth, ts(0), "Born".to_string());
        let b = world.add_entity(EntityKind::Person, "B".to_string(), None, ev2);
        let ev3 = world.add_event(EventKind::Birth, ts(0), "Born".to_string());
        let c = world.add_entity(EntityKind::Person, "C".to_string(), None, ev3);
        let ev4 = world.add_event(EventKind::Marriage, ts(100), "Rel".to_string());
        world.add_relationship(a, b, RelationshipKind::Parent, ts(100), ev4);
        let ev5 = world.add_event(EventKind::Marriage, ts(150), "Rel".to_string());
        world.add_relationship(b, c, RelationshipKind::Ally, ts(150), ev5);
        assert_eq!(world.collect_relationships().count(), 2);
    }

    #[test]
    fn ids_shared_across_types() {
        let mut world = World::new();
        let event_id = world.add_event(EventKind::Birth, ts(100), "Born".to_string());
        let entity_id = world.add_entity(EntityKind::Person, "A".to_string(), None, event_id);
        // IDs come from the same generator, so they must differ
        assert_ne!(entity_id, event_id);
    }

    #[test]
    fn add_event_participant() {
        let mut world = World::new();
        let evid = world.add_event(EventKind::Birth, ts(100), "Born".to_string());
        let eid = world.add_entity(EntityKind::Person, "A".to_string(), None, evid);
        world.add_event_participant(evid, eid, ParticipantRole::Subject);
        assert_eq!(world.event_participants.len(), 1);
        assert_eq!(world.event_participants[0].event_id, evid);
        assert_eq!(world.event_participants[0].entity_id, eid);
    }

    #[test]
    fn rename_entity_records_effect() {
        let mut world = World::new();
        let ev = world.add_event(EventKind::SettlementFounded, ts(0), "Founded".to_string());
        let id = world.add_entity(
            EntityKind::Settlement,
            "Ironhold".to_string(),
            Some(ts(0)),
            ev,
        );
        let ev2 = world.add_event(EventKind::SettlementFounded, ts(50), "Renamed".to_string());
        world.rename_entity(id, "Ironhaven".to_string(), ev2);
        assert_eq!(world.entities[&id].name, "Ironhaven");

        let last = world.event_effects.last().unwrap();
        assert_eq!(last.event_id, ev2);
        assert_eq!(last.entity_id, id);
        assert_eq!(
            last.effect,
            StateChange::NameChanged {
                old: "Ironhold".to_string(),
                new: "Ironhaven".to_string(),
            }
        );
    }

    #[test]
    fn end_entity_records_effect() {
        let mut world = World::new();
        let ev = world.add_event(EventKind::Birth, ts(100), "Born".to_string());
        let id = world.add_entity(EntityKind::Person, "Alice".to_string(), Some(ts(100)), ev);
        let ev2 = world.add_event(EventKind::Death, ts(170), "Died".to_string());
        world.end_entity(id, ts(170), ev2);
        assert_eq!(world.entities[&id].end, Some(ts(170)));

        let last = world.event_effects.last().unwrap();
        assert_eq!(last.event_id, ev2);
        assert_eq!(last.entity_id, id);
        assert_eq!(last.effect, StateChange::EntityEnded);
    }

    #[test]
    fn end_relationship_records_effect() {
        let mut world = World::new();
        let ev = world.add_event(EventKind::Birth, ts(0), "Born".to_string());
        let a = world.add_entity(EntityKind::Person, "A".to_string(), None, ev);
        let ev2 = world.add_event(EventKind::Birth, ts(0), "Born".to_string());
        let b = world.add_entity(EntityKind::Person, "B".to_string(), None, ev2);
        let ev3 = world.add_event(EventKind::Marriage, ts(100), "Allied".to_string());
        world.add_relationship(a, b, RelationshipKind::Ally, ts(100), ev3);
        let ev4 = world.add_event(EventKind::Death, ts(200), "War".to_string());
        world.end_relationship(a, b, &RelationshipKind::Ally, ts(200), ev4);

        let rel = &world.entities[&a].relationships[0];
        assert_eq!(rel.end, Some(ts(200)));

        let last = world.event_effects.last().unwrap();
        assert_eq!(last.event_id, ev4);
        assert_eq!(last.entity_id, a);
        assert_eq!(
            last.effect,
            StateChange::RelationshipEnded {
                target_entity_id: b,
                kind: RelationshipKind::Ally,
            }
        );
    }
}
