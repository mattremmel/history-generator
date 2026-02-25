use std::collections::{BTreeSet, VecDeque};

use crate::model::World;
use crate::model::entity::{Entity, EntityKind};
use crate::model::entity_data::{GovernmentType, ResourceType};
use crate::model::relationship::RelationshipKind;
use crate::model::timestamp::SimTimestamp;

use super::signal::{Signal, SignalKind};

/// Find all region IDs adjacent to the given region via active AdjacentTo relationships.
pub fn adjacent_regions(world: &World, region_id: u64) -> Vec<u64> {
    world
        .entities
        .get(&region_id)
        .map(|e| e.active_rels(RelationshipKind::AdjacentTo).collect())
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
            && e.is_alive()
            && e.has_active_rel(RelationshipKind::LeaderOf, faction_id)
    })
}

/// Find the faction that owns a settlement (via active MemberOf relationship).
pub fn settlement_faction(world: &World, settlement_id: u64) -> Option<u64> {
    world
        .entities
        .get(&settlement_id)
        .and_then(|e| e.active_rel(RelationshipKind::MemberOf))
}

/// Collect all living settlement IDs belonging to a faction.
pub fn faction_settlements(world: &World, faction_id: u64) -> Vec<u64> {
    world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Settlement
                && e.is_alive()
                && e.has_active_rel(RelationshipKind::MemberOf, faction_id)
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
                && e.is_alive()
                && e.has_active_rel(RelationshipKind::LocatedIn, settlement_id)
        })
        .count()
}

/// Find the first active relationship target of a given kind on an entity.
pub fn active_rel_target(world: &World, entity_id: u64, kind: RelationshipKind) -> Option<u64> {
    world
        .entities
        .get(&entity_id)
        .and_then(|e| e.active_rel(kind))
}

/// Get an entity's name by ID, with a fallback for missing entities.
pub fn entity_name(world: &World, entity_id: u64) -> String {
    world
        .entities
        .get(&entity_id)
        .map(|e| e.name.clone())
        .unwrap_or_else(|| format!("Entity#{entity_id}"))
}

/// End all active relationships on a person (used on death, faction change, etc.).
pub fn end_all_person_relationships(
    world: &mut World,
    person_id: u64,
    time: SimTimestamp,
    event_id: u64,
) {
    let rels: Vec<(u64, RelationshipKind)> = world
        .entities
        .get(&person_id)
        .map(|e| {
            e.relationships
                .iter()
                .filter(|r| r.is_active())
                .map(|r| (r.target_entity_id, r.kind.clone()))
                .collect()
        })
        .unwrap_or_default();

    for (target_id, kind) in rels {
        world.end_relationship(person_id, target_id, kind, time, event_id);
    }
}

/// Check whether two entities share a bidirectional active relationship of the given kind.
pub fn has_active_rel_of_kind(world: &World, a: u64, b: u64, kind: RelationshipKind) -> bool {
    let check = |source: u64, target: u64| -> bool {
        world
            .entities
            .get(&source)
            .is_some_and(|e| e.has_active_rel(kind.clone(), target))
    };
    check(a, b) || check(b, a)
}

/// End an Ally relationship in both directions between two entities.
pub fn end_ally_relationship(world: &mut World, a: u64, b: u64, time: SimTimestamp, event_id: u64) {
    for (src, dst) in [(a, b), (b, a)] {
        let has_rel = world
            .entities
            .get(&src)
            .is_some_and(|e| e.has_active_rel(RelationshipKind::Ally, dst));
        if has_rel {
            world.end_relationship(src, dst, RelationshipKind::Ally, time, event_id);
        }
    }
}

/// Get a faction's stability value.
pub fn faction_stability(world: &World, faction_id: u64) -> f64 {
    world
        .entities
        .get(&faction_id)
        .and_then(|e| e.data.as_faction())
        .map(|f| f.stability)
        .unwrap_or(0.5)
}

/// Get a faction's happiness value.
pub fn faction_happiness(world: &World, faction_id: u64) -> f64 {
    world
        .entities
        .get(&faction_id)
        .and_then(|e| e.data.as_faction())
        .map(|f| f.happiness)
        .unwrap_or(0.5)
}

/// Get a faction's legitimacy value.
pub fn faction_legitimacy(world: &World, faction_id: u64) -> f64 {
    world
        .entities
        .get(&faction_id)
        .and_then(|e| e.data.as_faction())
        .map(|f| f.legitimacy)
        .unwrap_or(0.5)
}

/// BFS to find the next step from `start` toward `goal` over region adjacency.
/// Returns the first region to move to, or None if already at goal or unreachable.
pub fn bfs_next_step(world: &World, start: u64, goal: u64) -> Option<u64> {
    if start == goal {
        return None;
    }
    let mut visited = BTreeSet::new();
    visited.insert(start);
    let mut queue: VecDeque<(u64, u64)> = VecDeque::new(); // (current, first_step)
    for adj in adjacent_regions(world, start) {
        if adj == goal {
            return Some(adj);
        }
        if visited.insert(adj) {
            queue.push_back((adj, adj));
        }
    }
    while let Some((current, first_step)) = queue.pop_front() {
        for adj in adjacent_regions(world, current) {
            if adj == goal {
                return Some(first_step);
            }
            if visited.insert(adj) {
                queue.push_back((adj, first_step));
            }
        }
    }
    None
}

/// BFS from `start` to find the nearest region matching a predicate.
pub fn bfs_nearest(world: &World, start: u64, predicate: impl Fn(u64) -> bool) -> Option<u64> {
    if predicate(start) {
        return Some(start);
    }
    let mut visited = BTreeSet::new();
    visited.insert(start);
    let mut queue: VecDeque<u64> = VecDeque::new();
    for adj in adjacent_regions(world, start) {
        if visited.insert(adj) {
            queue.push_back(adj);
        }
    }
    while let Some(current) = queue.pop_front() {
        if predicate(current) {
            return Some(current);
        }
        for adj in adjacent_regions(world, current) {
            if visited.insert(adj) {
                queue.push_back(adj);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Resource classification helpers
// ---------------------------------------------------------------------------

/// Whether a resource represents a food resource.
pub(crate) fn is_food_resource(resource: &ResourceType) -> bool {
    matches!(
        resource,
        ResourceType::Grain
            | ResourceType::Cattle
            | ResourceType::Sheep
            | ResourceType::Fish
            | ResourceType::Game
            | ResourceType::Freshwater
    )
}

/// Apply a stability delta to a faction with full audit trail (records change).
pub(crate) fn apply_stability_delta(world: &mut World, faction_id: u64, delta: f64, event_id: u64) {
    let (old, new) = {
        let Some(entity) = world.entities.get_mut(&faction_id) else {
            return;
        };
        let Some(fd) = entity.data.as_faction_mut() else {
            return;
        };
        let old = fd.stability;
        fd.stability = (old + delta).clamp(0.0, 1.0);
        (old, fd.stability)
    };
    world.record_change(
        faction_id,
        event_id,
        "stability",
        serde_json::json!(old),
        serde_json::json!(new),
    );
}

/// Find the "capital" settlement of a faction by oldest ID (min entity ID).
/// Used when we just need any canonical settlement for the faction.
pub(crate) fn faction_capital_oldest(world: &World, faction_id: u64) -> Option<u64> {
    world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Settlement
                && e.end.is_none()
                && e.has_active_rel(RelationshipKind::MemberOf, faction_id)
        })
        .min_by_key(|e| e.id)
        .map(|e| e.id)
}

/// Find the "capital" settlement of a faction by largest population.
/// Returns `(settlement_id, region_id)` for the most populous settlement.
pub(crate) fn faction_capital_largest(world: &World, faction_id: u64) -> Option<(u64, u64)> {
    let mut best: Option<(u64, u64, u64)> = None; // (settlement_id, region_id, population)
    for e in world.entities.values() {
        if e.kind != EntityKind::Settlement || e.end.is_some() {
            continue;
        }
        if !e.has_active_rel(RelationshipKind::MemberOf, faction_id) {
            continue;
        }
        let Some(rid) = e.active_rel(RelationshipKind::LocatedIn) else {
            continue;
        };
        let pop = e
            .data
            .as_settlement()
            .map(|s| s.population as u64)
            .unwrap_or(0);
        if best.is_none() || pop > best.unwrap().2 {
            best = Some((e.id, rid, pop));
        }
    }
    best.map(|(sid, rid, _)| (sid, rid))
}

pub(crate) const MINING_RESOURCES: &[ResourceType] = &[
    ResourceType::Iron,
    ResourceType::Stone,
    ResourceType::Copper,
    ResourceType::Gold,
    ResourceType::Gems,
    ResourceType::Obsidian,
    ResourceType::Sulfur,
    ResourceType::Clay,
    ResourceType::Ore,
];

/// Whether a resource represents a mining resource.
pub(crate) fn is_mining_resource(resource: &ResourceType) -> bool {
    MINING_RESOURCES.contains(resource)
}

/// Sum the total population across all settlements belonging to a faction.
pub(crate) fn total_faction_population(world: &World, faction_id: u64) -> u32 {
    world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Settlement
                && e.end.is_none()
                && e.has_active_rel(RelationshipKind::MemberOf, faction_id)
        })
        .filter_map(|e| e.data.as_settlement().map(|s| s.population))
        .sum()
}

/// Collect the set of resource types present across all settlements of a faction.
pub(crate) fn faction_resource_set(world: &World, faction_id: u64) -> BTreeSet<ResourceType> {
    let mut resources = BTreeSet::new();
    for e in world.entities.values() {
        if e.kind == EntityKind::Settlement
            && e.end.is_none()
            && e.has_active_rel(RelationshipKind::MemberOf, faction_id)
            && let Some(sd) = e.data.as_settlement()
        {
            for r in &sd.resources {
                resources.insert(r.clone());
            }
        }
    }
    resources
}

/// Collect all region IDs that contain settlements of a faction.
pub(crate) fn collect_faction_region_ids(world: &World, faction_id: u64) -> Vec<u64> {
    let mut seen = BTreeSet::new();
    for e in world.entities.values() {
        if e.kind == EntityKind::Settlement
            && e.end.is_none()
            && e.has_active_rel(RelationshipKind::MemberOf, faction_id)
            && let Some(region_id) = e.active_rel(RelationshipKind::LocatedIn)
        {
            seen.insert(region_id);
        }
    }
    seen.into_iter().collect()
}

/// Check if two factions have settlements in adjacent (or same) regions.
pub(crate) fn factions_are_adjacent(world: &World, a: u64, b: u64) -> bool {
    let regions_a = collect_faction_region_ids(world, a);
    let regions_b = collect_faction_region_ids(world, b);

    for &ra in &regions_a {
        for &rb in &regions_b {
            if ra == rb {
                return true;
            }
            if world
                .entities
                .get(&ra)
                .is_some_and(|entity| entity.has_active_rel(RelationshipKind::AdjacentTo, rb))
            {
                return true;
            }
        }
    }
    false
}

/// Damage buildings in a settlement. Applies `damage_fn` to each building's condition
/// that passes `filter_fn`, destroys buildings at condition <= 0, and emits BuildingDestroyed
/// signals. Used by both disaster and conquest damage paths.
#[allow(clippy::too_many_arguments)]
pub(crate) fn damage_buildings(
    world: &mut World,
    signals: &mut Vec<Signal>,
    settlement_id: u64,
    time: SimTimestamp,
    cause_event_id: u64,
    mut damage_fn: impl FnMut(f64) -> f64,
    filter_fn: impl Fn(&Entity) -> bool,
    cause: &str,
) {
    let building_ids: Vec<u64> = world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Building
                && e.end.is_none()
                && e.has_active_rel(RelationshipKind::LocatedIn, settlement_id)
                && filter_fn(e)
        })
        .map(|e| e.id)
        .collect();

    for bid in building_ids {
        let (destroyed, old_condition, new_condition) = {
            if let Some(entity) = world.entities.get_mut(&bid)
                && let Some(bd) = entity.data.as_building_mut()
            {
                let old = bd.condition;
                bd.condition = damage_fn(old);
                (bd.condition <= 0.0, old, bd.condition)
            } else {
                continue;
            }
        };

        if destroyed {
            let building_type = world
                .entities
                .get(&bid)
                .and_then(|e| e.data.as_building())
                .map(|bd| bd.building_type);
            let Some(building_type) = building_type else {
                continue;
            };

            world.end_entity(bid, time, cause_event_id);

            signals.push(Signal {
                event_id: cause_event_id,
                kind: SignalKind::BuildingDestroyed {
                    building_id: bid,
                    settlement_id,
                    building_type,
                    cause: cause.to_string(),
                },
            });
        } else {
            world.record_change(
                bid,
                cause_event_id,
                "condition",
                serde_json::json!(old_condition),
                serde_json::json!(new_condition),
            );
        }
    }
}

/// Transfer all living NPCs in a settlement from one faction to another.
/// Ends their MemberOf relationship with the old faction and starts one with the new faction.
pub(crate) fn transfer_settlement_npcs(
    world: &mut World,
    settlement_id: u64,
    old_faction: u64,
    new_faction: u64,
    time: SimTimestamp,
    event_id: u64,
) {
    let npc_ids: Vec<u64> = world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Person
                && e.end.is_none()
                && e.has_active_rel(RelationshipKind::LocatedIn, settlement_id)
                && e.has_active_rel(RelationshipKind::MemberOf, old_faction)
        })
        .map(|e| e.id)
        .collect();
    for npc_id in npc_ids {
        world.end_relationship(
            npc_id,
            old_faction,
            RelationshipKind::MemberOf,
            time,
            event_id,
        );
        world.add_relationship(
            npc_id,
            new_faction,
            RelationshipKind::MemberOf,
            time,
            event_id,
        );
    }
}

// ---------------------------------------------------------------------------
// Faction classification helpers
// ---------------------------------------------------------------------------

/// Returns true if a faction's government type is a non-state actor (BanditClan or MercenaryCompany).
pub fn is_non_state_faction(world: &World, faction_id: u64) -> bool {
    world
        .entities
        .get(&faction_id)
        .and_then(|e| e.data.as_faction())
        .is_some_and(|fd| {
            matches!(
                fd.government_type,
                GovernmentType::BanditClan | GovernmentType::MercenaryCompany
            )
        })
}

/// Returns true if a faction is a mercenary company.
#[allow(dead_code)]
pub(crate) fn is_mercenary_faction(world: &World, faction_id: u64) -> bool {
    world
        .entities
        .get(&faction_id)
        .and_then(|e| e.data.as_faction())
        .is_some_and(|fd| fd.government_type == GovernmentType::MercenaryCompany)
}

/// Find the employer of a mercenary faction (via active HiredBy relationship).
/// Returns None if not a mercenary or not currently hired.
pub(crate) fn mercenary_employer(world: &World, faction_id: u64) -> Option<u64> {
    world
        .entities
        .get(&faction_id)
        .and_then(|e| e.active_rel(RelationshipKind::HiredBy))
}

/// For a faction, return its employer if it's a hired mercenary, otherwise return itself.
/// Used by combat code to resolve "who is this army effectively fighting for?"
pub(crate) fn employer_or_self(world: &World, faction_id: u64) -> u64 {
    mercenary_employer(world, faction_id).unwrap_or(faction_id)
}

/// Get a settlement's literacy rate (0.0-1.0).
pub(crate) fn settlement_literacy(world: &World, settlement_id: u64) -> f64 {
    world
        .entities
        .get(&settlement_id)
        .and_then(|e| e.data.as_settlement())
        .map(|sd| sd.literacy_rate)
        .unwrap_or(0.0)
}
