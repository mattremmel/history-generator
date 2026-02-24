use crate::model::entity::{Entity, EntityKind};
use crate::model::relationship::RelationshipKind;
use crate::model::World;

/// Find all region IDs adjacent to the given region via active AdjacentTo relationships.
pub fn adjacent_regions(world: &World, region_id: u64) -> Vec<u64> {
    world
        .entities
        .get(&region_id)
        .map(|e| {
            e.relationships
                .iter()
                .filter(|r| r.kind == RelationshipKind::AdjacentTo && r.end.is_none())
                .map(|r| r.target_entity_id)
                .collect()
        })
        .unwrap_or_default()
}

/// Find the living person who is leader of the given faction.
/// Returns the leader's entity ID, or None if no leader exists.
pub fn faction_leader(world: &World, faction_id: u64) -> Option<u64> {
    faction_leader_entity(world, faction_id).map(|e| e.id)
}

/// Find the living person entity who is leader of the given faction.
pub fn faction_leader_entity(world: &World, faction_id: u64) -> Option<&Entity> {
    world.entities.values().find(|e| {
        e.kind == EntityKind::Person
            && e.end.is_none()
            && e.relationships.iter().any(|r| {
                r.kind == RelationshipKind::LeaderOf
                    && r.target_entity_id == faction_id
                    && r.end.is_none()
            })
    })
}

/// Find the faction that owns a settlement (via active MemberOf relationship).
pub fn settlement_faction(world: &World, settlement_id: u64) -> Option<u64> {
    world.entities.get(&settlement_id).and_then(|e| {
        e.relationships
            .iter()
            .find(|r| r.kind == RelationshipKind::MemberOf && r.end.is_none())
            .map(|r| r.target_entity_id)
    })
}

/// Collect all living settlement IDs belonging to a faction.
pub fn faction_settlements(world: &World, faction_id: u64) -> Vec<u64> {
    world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Settlement
                && e.end.is_none()
                && e.relationships.iter().any(|r| {
                    r.kind == RelationshipKind::MemberOf
                        && r.target_entity_id == faction_id
                        && r.end.is_none()
                })
        })
        .map(|e| e.id)
        .collect()
}

/// Count all living buildings in a settlement (via active LocatedIn relationships).
pub fn settlement_building_count(world: &World, settlement_id: u64) -> usize {
    world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Building
                && e.end.is_none()
                && e.relationships.iter().any(|r| {
                    r.kind == RelationshipKind::LocatedIn
                        && r.target_entity_id == settlement_id
                        && r.end.is_none()
                })
        })
        .count()
}

/// Find the first active relationship target of a given kind on an entity.
pub fn active_rel_target(world: &World, entity_id: u64, kind: RelationshipKind) -> Option<u64> {
    world.entities.get(&entity_id).and_then(|e| {
        e.relationships
            .iter()
            .find(|r| r.kind == kind && r.end.is_none())
            .map(|r| r.target_entity_id)
    })
}
