use std::collections::VecDeque;

use rand::Rng;

use super::context::TickContext;
use super::signal::{Signal, SignalKind};
use super::system::{SimSystem, TickFrequency};
use crate::model::traits::{Trait, has_trait};
use crate::model::{EntityKind, EventKind, ParticipantRole, RelationshipKind, SimTimestamp, World};
use crate::sim::helpers;

// --- Constants ---

/// Fraction of population that flees after conquest.
const CONQUEST_REFUGEE_MIN: f64 = 0.15;
const CONQUEST_REFUGEE_MAX: f64 = 0.30;

/// Fraction of population that emigrates per year from war-zone settlements.
const WAR_ZONE_EMIGRATION_MIN: f64 = 0.03;
const WAR_ZONE_EMIGRATION_MAX: f64 = 0.08;

/// Fraction of population that emigrates per year from low-prosperity settlements.
const LOW_PROSPERITY_EMIGRATION_MIN: f64 = 0.02;
const LOW_PROSPERITY_EMIGRATION_MAX: f64 = 0.05;

/// Prosperity threshold below which economic emigration kicks in.
const LOW_PROSPERITY_THRESHOLD: f64 = 0.3;

/// Maximum BFS hops for destination search.
const MAX_BFS_HOPS: usize = 4;

/// Minimum population before a settlement is considered abandoned.
const ABANDONMENT_THRESHOLD: u32 = 10;

/// NPC flee chances by trait.
const CAUTIOUS_FLEE_CHANCE: f64 = 0.60;
const DEFAULT_FLEE_CHANCE: f64 = 0.30;
const RESISTANT_FLEE_CHANCE: f64 = 0.15;

pub struct MigrationSystem;

impl SimSystem for MigrationSystem {
    fn name(&self) -> &str {
        "migration"
    }

    fn frequency(&self) -> TickFrequency {
        TickFrequency::Yearly
    }

    fn tick(&mut self, ctx: &mut TickContext) {
        let time = ctx.world.current_time;
        let current_year = time.year();

        // Collect all migration sources with their refugee fractions
        let sources = collect_migration_sources(ctx.world, current_year);
        if sources.is_empty() {
            return;
        }

        // Process each source
        for source in sources {
            process_migration(ctx, time, current_year, &source);
        }
    }
}

// --- Source collection ---

struct MigrationSource {
    settlement_id: u64,
    region_id: u64,
    /// The faction refugees identify with (for conquest: the old faction,
    /// for economic migration: the current faction).
    affinity_faction_id: u64,
    fraction_min: f64,
    fraction_max: f64,
    cause_event_id: Option<u64>,
    is_conquest: bool,
}

fn collect_migration_sources(world: &World, current_year: u32) -> Vec<MigrationSource> {
    let mut sources = Vec::new();

    // Gather settlement info
    let settlements: Vec<(u64, u64, u64, f64)> = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
        .filter_map(|e| {
            let region_id = e.active_rel(RelationshipKind::LocatedIn)?;
            let faction_id = e.active_rel(RelationshipKind::MemberOf)?;
            let prosperity = e.data.as_settlement().map(|s| s.prosperity).unwrap_or(0.5);
            Some((e.id, region_id, faction_id, prosperity))
        })
        .collect();

    // Find recently conquered settlements: MemberOf relationship started this year
    for &(sid, region_id, faction_id, prosperity) in &settlements {
        let entity = match world.entities.get(&sid) {
            Some(e) => e,
            None => continue,
        };

        // Check if this settlement has a MemberOf that started this year
        // AND has an ended MemberOf (old faction) — indicating a transfer
        let has_new_membership = entity.relationships.iter().any(|r| {
            r.kind == RelationshipKind::MemberOf
                && r.end.is_none()
                && r.start.year() == current_year
        });
        let had_old_membership = entity.relationships.iter().any(|r| {
            r.kind == RelationshipKind::MemberOf
                && r.end.is_some()
                && r.end.unwrap().year() == current_year
        });

        if has_new_membership && had_old_membership {
            // Find the old faction (the one the residents identify with)
            let old_faction_id = entity
                .relationships
                .iter()
                .find(|r| {
                    r.kind == RelationshipKind::MemberOf
                        && r.end.is_some()
                        && r.end.unwrap().year() == current_year
                })
                .map(|r| r.target_entity_id)
                .unwrap_or(faction_id);

            // Find the conquest event that caused this
            let cause_event = find_conquest_event(world, sid, current_year);
            sources.push(MigrationSource {
                settlement_id: sid,
                region_id,
                affinity_faction_id: old_faction_id,
                fraction_min: CONQUEST_REFUGEE_MIN,
                fraction_max: CONQUEST_REFUGEE_MAX,
                cause_event_id: cause_event,
                is_conquest: true,
            });
            continue; // Don't also add war-zone / low-prosperity for conquest
        }

        // Check if faction is at war
        let faction_at_war = world
            .entities
            .get(&faction_id)
            .is_some_and(|e| e.active_rels(RelationshipKind::AtWar).next().is_some());

        if faction_at_war {
            sources.push(MigrationSource {
                settlement_id: sid,
                region_id,
                affinity_faction_id: faction_id,
                fraction_min: WAR_ZONE_EMIGRATION_MIN,
                fraction_max: WAR_ZONE_EMIGRATION_MAX,
                cause_event_id: None,
                is_conquest: false,
            });
            continue; // Don't stack with low-prosperity
        }

        // Check low prosperity
        if prosperity < LOW_PROSPERITY_THRESHOLD {
            sources.push(MigrationSource {
                settlement_id: sid,
                region_id,
                affinity_faction_id: faction_id,
                fraction_min: LOW_PROSPERITY_EMIGRATION_MIN,
                fraction_max: LOW_PROSPERITY_EMIGRATION_MAX,
                cause_event_id: None,
                is_conquest: false,
            });
        }
    }

    sources
}

fn find_conquest_event(world: &World, settlement_id: u64, current_year: u32) -> Option<u64> {
    world
        .events
        .values()
        .filter(|e| e.kind == EventKind::Conquest && e.timestamp.year() == current_year)
        .find(|e| {
            world
                .event_participants
                .iter()
                .any(|p| p.event_id == e.id && p.entity_id == settlement_id)
        })
        .map(|e| e.id)
}

// --- Destination scoring ---

struct Candidate {
    settlement_id: u64,
    score: f64,
}

fn find_best_destination(world: &World, source: &MigrationSource) -> Option<u64> {
    // BFS over region adjacency to find settlements within MAX_BFS_HOPS
    let reachable_regions = bfs_reachable_regions(world, source.region_id, MAX_BFS_HOPS);

    let mut candidates: Vec<Candidate> = Vec::new();

    for &(region_id, distance) in &reachable_regions {
        // Find settlements in this region
        for entity in world.entities.values() {
            if entity.kind != EntityKind::Settlement || entity.end.is_some() {
                continue;
            }
            if entity.id == source.settlement_id {
                continue;
            }

            if !entity.has_active_rel(RelationshipKind::LocatedIn, region_id) {
                continue;
            }

            let dest_faction = match entity.active_rel(RelationshipKind::MemberOf) {
                Some(f) => f,
                None => continue,
            };

            // Score: faction_affinity * (1.0 / distance) * prosperity * capacity_room
            let faction_affinity =
                compute_faction_affinity(world, source.affinity_faction_id, dest_faction);
            if faction_affinity <= 0.0 {
                continue; // hostile — skip
            }

            let prosperity = entity
                .data
                .as_settlement()
                .map(|s| s.prosperity)
                .unwrap_or(0.3);
            let population = entity
                .data
                .as_settlement()
                .map(|s| s.population)
                .unwrap_or(0);

            // Capacity room: settlements above 2000 are less attractive
            let capacity_room = (1.0 - population as f64 / 3000.0).max(0.1);

            let dist_factor = 1.0 / (distance as f64).max(1.0);
            let score = faction_affinity * dist_factor * (0.3 + prosperity) * capacity_room;

            candidates.push(Candidate {
                settlement_id: entity.id,
                score,
            });
        }
    }

    candidates.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    candidates.first().map(|c| c.settlement_id)
}

fn compute_faction_affinity(world: &World, source_faction: u64, dest_faction: u64) -> f64 {
    if source_faction == dest_faction {
        return 1.0; // Same faction — strong preference
    }

    let source_entity = match world.entities.get(&source_faction) {
        Some(e) => e,
        None => return 0.2,
    };

    if source_entity.has_active_rel(RelationshipKind::AtWar, dest_faction)
        || source_entity.has_active_rel(RelationshipKind::Enemy, dest_faction)
    {
        return 0.0;
    }
    if source_entity.has_active_rel(RelationshipKind::Ally, dest_faction) {
        return 0.7;
    }

    // Check reverse direction (dest might have relationship to source)
    if let Some(dest_entity) = world.entities.get(&dest_faction) {
        if dest_entity.has_active_rel(RelationshipKind::AtWar, source_faction)
            || dest_entity.has_active_rel(RelationshipKind::Enemy, source_faction)
        {
            return 0.0;
        }
        if dest_entity.has_active_rel(RelationshipKind::Ally, source_faction) {
            return 0.7;
        }
    }

    0.2 // Neutral faction
}

fn bfs_reachable_regions(world: &World, start: u64, max_hops: usize) -> Vec<(u64, usize)> {
    let mut result = Vec::new();
    let mut visited = std::collections::BTreeSet::from([start]);
    let mut queue: VecDeque<(u64, usize)> = VecDeque::new();

    // Start region is distance 0
    result.push((start, 1)); // distance 1 so same-region settlements still get a score

    for adj in helpers::adjacent_regions(world, start) {
        if visited.insert(adj) {
            queue.push_back((adj, 1));
            result.push((adj, 1));
        }
    }

    while let Some((current, depth)) = queue.pop_front() {
        if depth >= max_hops {
            continue;
        }
        for adj in helpers::adjacent_regions(world, current) {
            if visited.insert(adj) {
                queue.push_back((adj, depth + 1));
                result.push((adj, depth + 1));
            }
        }
    }

    result
}

// --- Migration processing ---

fn process_migration(
    ctx: &mut TickContext,
    time: SimTimestamp,
    current_year: u32,
    source: &MigrationSource,
) {
    // Find a destination
    let dest_id = match find_best_destination(ctx.world, source) {
        Some(id) => id,
        None => return, // No valid destination
    };

    // Compute refugee fraction
    let fraction: f64 = ctx
        .rng
        .random_range(source.fraction_min..source.fraction_max);

    // Check source still exists and has population
    let source_pop = match ctx.world.entities.get(&source.settlement_id) {
        Some(e) if e.end.is_none() => e.data.as_settlement().map(|s| s.population).unwrap_or(0),
        _ => return,
    };
    if source_pop == 0 {
        return;
    }

    // Subtract population from source
    let removed = {
        let entity = match ctx.world.entities.get_mut(&source.settlement_id) {
            Some(e) => e,
            None => return,
        };
        let settlement = match entity.data.as_settlement_mut() {
            Some(s) => s,
            None => return,
        };
        let removed = settlement
            .population_breakdown
            .subtract_fraction(fraction, ctx.rng);
        settlement.population = settlement.population_breakdown.total();
        removed
    };

    let refugee_count = removed.total();
    if refugee_count == 0 {
        return;
    }

    // Add population to destination
    let dest_pop_before = {
        let entity = match ctx.world.entities.get_mut(&dest_id) {
            Some(e) => e,
            None => return,
        };
        let settlement = match entity.data.as_settlement_mut() {
            Some(s) => s,
            None => return,
        };
        let old_pop = settlement.population;
        settlement.population_breakdown += &removed;
        settlement.population = settlement.population_breakdown.total();
        old_pop
    };
    let dest_pop_after = ctx.world.settlement(dest_id).population;

    // Get names for event description
    let source_name = helpers::entity_name(ctx.world, source.settlement_id);
    let dest_name = helpers::entity_name(ctx.world, dest_id);

    // Create migration event
    let ev = if let Some(cause_id) = source.cause_event_id {
        ctx.world.add_caused_event(
            EventKind::Migration,
            time,
            format!(
                "{refugee_count} refugees fled from {source_name} to {dest_name} in year {current_year}"
            ),
            cause_id,
        )
    } else {
        ctx.world.add_event(
            EventKind::Migration,
            time,
            format!(
                "{refugee_count} people migrated from {source_name} to {dest_name} in year {current_year}"
            ),
        )
    };
    ctx.world
        .add_event_participant(ev, source.settlement_id, ParticipantRole::Origin);
    ctx.world
        .add_event_participant(ev, dest_id, ParticipantRole::Destination);

    // Record population changes
    ctx.world.record_change(
        source.settlement_id,
        ev,
        "population",
        serde_json::json!(source_pop),
        serde_json::json!(source_pop - refugee_count),
    );
    ctx.world.record_change(
        dest_id,
        ev,
        "population",
        serde_json::json!(dest_pop_before),
        serde_json::json!(dest_pop_after),
    );

    // Emit RefugeesArrived signal
    ctx.signals.push(Signal {
        event_id: ev,
        kind: SignalKind::RefugeesArrived {
            settlement_id: dest_id,
            source_settlement_id: source.settlement_id,
            count: refugee_count,
        },
    });

    // Handle NPC migration for conquest refugees
    if source.is_conquest {
        migrate_npcs(ctx, time, current_year, source.settlement_id, dest_id, ev);
    }

    // Check for settlement abandonment
    let remaining_pop = ctx
        .world
        .entities
        .get(&source.settlement_id)
        .and_then(|e| e.data.as_settlement())
        .map(|s| s.population)
        .unwrap_or(0);

    if remaining_pop < ABANDONMENT_THRESHOLD {
        let abandon_ev = ctx.world.add_caused_event(
            EventKind::Abandoned,
            time,
            format!("{source_name} abandoned after mass exodus in year {current_year}"),
            ev,
        );
        ctx.world
            .add_event_participant(abandon_ev, source.settlement_id, ParticipantRole::Subject);
        ctx.world.end_entity(source.settlement_id, time, abandon_ev);
    }
}

fn migrate_npcs(
    ctx: &mut TickContext,
    time: SimTimestamp,
    current_year: u32,
    source_settlement_id: u64,
    dest_settlement_id: u64,
    cause_event_id: u64,
) {
    // Find the destination's faction
    let dest_faction_id = ctx
        .world
        .entities
        .get(&dest_settlement_id)
        .and_then(|e| e.active_rel(RelationshipKind::MemberOf));

    // Find NPCs located in the source settlement, with their current faction
    let npcs: Vec<(u64, f64, Option<u64>)> = ctx
        .world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Person
                && e.end.is_none()
                && e.has_active_rel(RelationshipKind::LocatedIn, source_settlement_id)
        })
        .map(|e| {
            let flee_chance = if has_trait(e, &Trait::Cautious) {
                CAUTIOUS_FLEE_CHANCE
            } else if has_trait(e, &Trait::Aggressive) || has_trait(e, &Trait::Honorable) {
                RESISTANT_FLEE_CHANCE
            } else {
                DEFAULT_FLEE_CHANCE
            };
            let npc_faction = e.active_rel(RelationshipKind::MemberOf);
            (e.id, flee_chance, npc_faction)
        })
        .collect();

    for (npc_id, flee_chance, npc_faction) in npcs {
        if ctx.rng.random_range(0.0..1.0) >= flee_chance {
            continue;
        }

        let npc_name = helpers::entity_name(ctx.world, npc_id);
        let dest_name = helpers::entity_name(ctx.world, dest_settlement_id);

        // Create individual migration event
        let ev = ctx.world.add_caused_event(
            EventKind::Migration,
            time,
            format!("{npc_name} fled to {dest_name} in year {current_year}"),
            cause_event_id,
        );
        ctx.world
            .add_event_participant(ev, npc_id, ParticipantRole::Subject);
        ctx.world
            .add_event_participant(ev, source_settlement_id, ParticipantRole::Origin);
        ctx.world
            .add_event_participant(ev, dest_settlement_id, ParticipantRole::Destination);

        // End old LocatedIn
        ctx.world.end_relationship(
            npc_id,
            source_settlement_id,
            RelationshipKind::LocatedIn,
            time,
            ev,
        );

        // Add new LocatedIn
        ctx.world.add_relationship(
            npc_id,
            dest_settlement_id,
            RelationshipKind::LocatedIn,
            time,
            ev,
        );

        // Switch faction if NPC's current faction differs from destination's faction
        if let (Some(old_fid), Some(new_fid)) = (npc_faction, dest_faction_id)
            && old_fid != new_fid
        {
            ctx.world
                .end_relationship(npc_id, old_fid, RelationshipKind::MemberOf, time, ev);
            ctx.world
                .add_relationship(npc_id, new_fid, RelationshipKind::MemberOf, time, ev);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::population::PopulationBreakdown;
    use crate::model::{EntityData, PersonData, Role, Sex};
    use crate::sim::runner::{SimConfig, run};
    use crate::sim::system::SimSystem;
    use crate::sim::{ConflictSystem, DemographicsSystem, EconomySystem, PoliticsSystem};
    use crate::testutil::migration_scenario;
    use crate::worldgen::{self, config::WorldGenConfig};
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    // --- Unit-level helpers ---

    fn ts(year: u32) -> SimTimestamp {
        SimTimestamp::from_year(year)
    }

    /// Simulate a conquest by ending old MemberOf and adding new MemberOf in the given year.
    fn simulate_conquest(
        world: &mut World,
        settlement_id: u64,
        old_faction: u64,
        new_faction: u64,
        year: u32,
    ) {
        let t = ts(year);
        let ev = world.add_event(EventKind::Conquest, t, "Settlement captured".to_string());
        world.add_event_participant(ev, settlement_id, ParticipantRole::Object);
        world.end_relationship(
            settlement_id,
            old_faction,
            RelationshipKind::MemberOf,
            t,
            ev,
        );
        world.add_relationship(
            settlement_id,
            new_faction,
            RelationshipKind::MemberOf,
            t,
            ev,
        );
    }

    // --- Tests ---

    #[test]
    fn scenario_conquest_triggers_refugee_flow() {
        let m = migration_scenario();
        let (mut world, source, dest, old_faction) = (m.world, m.source, m.dest, m.faction);

        // Create a new faction (the conqueror)
        let t5 = ts(5);
        let ev = world.add_event(EventKind::FactionFormed, t5, "new faction".to_string());
        let new_faction = world.add_entity(
            EntityKind::Faction,
            "Conquerors".to_string(),
            None,
            EntityData::default_for_kind(EntityKind::Faction),
            ev,
        );

        let source_pop_before = world
            .entities
            .get(&source)
            .unwrap()
            .data
            .as_settlement()
            .unwrap()
            .population;

        // Simulate conquest of source in year 5
        simulate_conquest(&mut world, source, old_faction, new_faction, 5);

        // Run migration system at year 5
        world.current_time = ts(5);
        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals = Vec::new();
        let mut system = MigrationSystem;
        let mut ctx = TickContext {
            world: &mut world,
            rng: &mut rng,
            signals: &mut signals,
            inbox: &[],
        };
        system.tick(&mut ctx);

        // Source should have lost population
        let source_pop_after = world
            .entities
            .get(&source)
            .and_then(|e| e.data.as_settlement())
            .map(|s| s.population)
            .unwrap_or(0);
        assert!(
            source_pop_after < source_pop_before,
            "source should lose population: before={source_pop_before}, after={source_pop_after}"
        );

        // Dest should have gained population
        let dest_pop = world
            .entities
            .get(&dest)
            .unwrap()
            .data
            .as_settlement()
            .unwrap()
            .population;
        assert!(
            dest_pop > 300,
            "destination should gain population: {dest_pop}"
        );

        // Migration event should exist
        let migration_events: Vec<_> = world
            .events
            .values()
            .filter(|e| e.kind == EventKind::Migration)
            .collect();
        assert!(!migration_events.is_empty(), "should have migration events");

        // RefugeesArrived signal should have been emitted
        assert!(
            signals.iter().any(|s| matches!(&s.kind, SignalKind::RefugeesArrived { settlement_id, .. } if *settlement_id == dest)),
            "should emit RefugeesArrived signal"
        );
    }

    #[test]
    fn scenario_refugees_prefer_same_faction() {
        let m = migration_scenario();
        let (mut world, source, same_faction_dest, old_faction, region_a) =
            (m.world, m.source, m.dest, m.faction, m.region_a);
        let t = ts(1);

        // Add a third region and a different-faction settlement
        let ev = world.add_event(EventKind::SettlementFounded, t, "init".to_string());
        let region_c = world.add_entity(
            EntityKind::Region,
            "RegionC".to_string(),
            None,
            EntityData::default_for_kind(EntityKind::Region),
            ev,
        );
        world.add_relationship(region_a, region_c, RelationshipKind::AdjacentTo, t, ev);
        world.add_relationship(region_c, region_a, RelationshipKind::AdjacentTo, t, ev);

        let other_faction = world.add_entity(
            EntityKind::Faction,
            "OtherFaction".to_string(),
            None,
            EntityData::default_for_kind(EntityKind::Faction),
            ev,
        );

        // This settlement is closer (same adjacency) but different faction
        let other_settlement = world.add_entity(
            EntityKind::Settlement,
            "OtherTown".to_string(),
            Some(t),
            EntityData::default_for_kind(EntityKind::Settlement),
            ev,
        );
        if let Some(sd) = world
            .entities
            .get_mut(&other_settlement)
            .and_then(|e| e.data.as_settlement_mut())
        {
            sd.population = 400;
            sd.population_breakdown = PopulationBreakdown::from_total(400);
            sd.prosperity = 0.8;
        }
        world.add_relationship(
            other_settlement,
            region_c,
            RelationshipKind::LocatedIn,
            t,
            ev,
        );
        world.add_relationship(
            other_settlement,
            other_faction,
            RelationshipKind::MemberOf,
            t,
            ev,
        );

        // Create conqueror faction and simulate conquest
        let conqueror_faction = world.add_entity(
            EntityKind::Faction,
            "Conquerors".to_string(),
            None,
            EntityData::default_for_kind(EntityKind::Faction),
            ev,
        );
        simulate_conquest(&mut world, source, old_faction, conqueror_faction, 1);

        // Run migration
        world.current_time = ts(1);
        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals = Vec::new();
        let mut system = MigrationSystem;
        let mut ctx = TickContext {
            world: &mut world,
            rng: &mut rng,
            signals: &mut signals,
            inbox: &[],
        };
        system.tick(&mut ctx);

        // Same-faction destination should have gained population
        let same_faction_pop = world
            .entities
            .get(&same_faction_dest)
            .unwrap()
            .data
            .as_settlement()
            .unwrap()
            .population;

        // Other faction destination should NOT have gained (or gained less)
        let other_pop = world
            .entities
            .get(&other_settlement)
            .unwrap()
            .data
            .as_settlement()
            .unwrap()
            .population;

        assert!(
            same_faction_pop > 300,
            "same-faction settlement should gain refugees: {same_faction_pop}"
        );
        assert_eq!(
            other_pop, 400,
            "different-faction settlement should not gain refugees when same-faction available: {other_pop}"
        );
    }

    #[test]
    fn scenario_npc_migration_creates_events() {
        let m = migration_scenario();
        let (mut world, source, old_faction) = (m.world, m.source, m.faction);

        // Add NPCs at the source settlement
        let t = ts(1);
        let ev = world.add_event(EventKind::Birth, t, "born".to_string());
        for i in 0..5 {
            let npc = world.add_entity(
                EntityKind::Person,
                format!("NPC_{i}"),
                Some(t),
                EntityData::Person(PersonData {
                    born: SimTimestamp::default(),
                    sex: Sex::Male,
                    role: Role::Common,
                    traits: vec![Trait::Cautious], // High flee chance
                    last_action: SimTimestamp::default(),
                    culture_id: None,
                    prestige: 0.0,
                    grievances: std::collections::BTreeMap::new(),
                    secrets: std::collections::BTreeMap::new(),
                    claims: std::collections::BTreeMap::new(),
                    prestige_tier: 0,
                    widowed_at: None,
                }),
                ev,
            );
            world.add_relationship(npc, source, RelationshipKind::LocatedIn, t, ev);
            world.add_relationship(npc, old_faction, RelationshipKind::MemberOf, t, ev);
        }

        // Create conqueror and simulate conquest in year 5
        let t5 = ts(5);
        let ev5 = world.add_event(EventKind::FactionFormed, t5, "formed".to_string());
        let new_faction = world.add_entity(
            EntityKind::Faction,
            "Conquerors".to_string(),
            None,
            EntityData::default_for_kind(EntityKind::Faction),
            ev5,
        );
        simulate_conquest(&mut world, source, old_faction, new_faction, 5);

        // Run migration
        world.current_time = ts(5);
        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals = Vec::new();
        let mut system = MigrationSystem;
        let mut ctx = TickContext {
            world: &mut world,
            rng: &mut rng,
            signals: &mut signals,
            inbox: &[],
        };
        system.tick(&mut ctx);

        // Should have individual NPC migration events
        let npc_migration_events: Vec<_> = world
            .events
            .values()
            .filter(|e| e.kind == EventKind::Migration && e.description.contains("fled to"))
            .collect();
        assert!(
            !npc_migration_events.is_empty(),
            "should have NPC migration events"
        );

        // Check that migration events have Origin and Destination participants
        for mev in &npc_migration_events {
            let participants: Vec<_> = world
                .event_participants
                .iter()
                .filter(|p| p.event_id == mev.id)
                .collect();
            assert!(
                participants
                    .iter()
                    .any(|p| p.role == ParticipantRole::Origin),
                "migration event should have Origin participant"
            );
            assert!(
                participants
                    .iter()
                    .any(|p| p.role == ParticipantRole::Destination),
                "migration event should have Destination participant"
            );
        }
    }

    #[test]
    fn scenario_lowprosperity_causes_emigration() {
        let m = migration_scenario();
        let (mut world, source) = (m.world, m.source);

        // Set source to low prosperity
        {
            let entity = world.entities.get_mut(&source).unwrap();
            entity.data.as_settlement_mut().unwrap().prosperity = 0.2;
        }

        let source_pop_before = world
            .entities
            .get(&source)
            .unwrap()
            .data
            .as_settlement()
            .unwrap()
            .population;

        // Run migration at year 1 (no conquest, just low prosperity)
        world.current_time = ts(1);
        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals = Vec::new();
        let mut system = MigrationSystem;
        let mut ctx = TickContext {
            world: &mut world,
            rng: &mut rng,
            signals: &mut signals,
            inbox: &[],
        };
        system.tick(&mut ctx);

        let source_pop_after = world
            .entities
            .get(&source)
            .unwrap()
            .data
            .as_settlement()
            .unwrap()
            .population;
        assert!(
            source_pop_after < source_pop_before,
            "low prosperity should cause emigration: before={source_pop_before}, after={source_pop_after}"
        );
    }

    #[test]
    fn scenario_abandoned_when_depopulated() {
        let m = migration_scenario();
        let (mut world, source, old_faction) = (m.world, m.source, m.faction);

        // Make source tiny — conquest removes 15-30%, so 8 pop → drops below threshold.
        {
            let entity = world.entities.get_mut(&source).unwrap();
            let sd = entity.data.as_settlement_mut().unwrap();
            sd.population = 8;
            sd.population_breakdown = PopulationBreakdown::from_total(8);
        }

        // Create conqueror and simulate conquest — this should trigger large refugee fraction
        let t5 = ts(5);
        let ev5 = world.add_event(EventKind::FactionFormed, t5, "formed".to_string());
        let new_faction = world.add_entity(
            EntityKind::Faction,
            "Conquerors".to_string(),
            None,
            EntityData::default_for_kind(EntityKind::Faction),
            ev5,
        );
        simulate_conquest(&mut world, source, old_faction, new_faction, 5);

        // Run migration
        world.current_time = ts(5);
        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals = Vec::new();
        let mut system = MigrationSystem;
        let mut ctx = TickContext {
            world: &mut world,
            rng: &mut rng,
            signals: &mut signals,
            inbox: &[],
        };
        system.tick(&mut ctx);

        // Source should be ended (abandoned)
        let source_entity = world.entities.get(&source).unwrap();
        assert!(
            source_entity.end.is_some(),
            "settlement with < 10 pop should be abandoned"
        );

        // Abandoned event should exist
        assert!(
            world
                .events
                .values()
                .any(|e| e.kind == EventKind::Abandoned),
            "should have abandonment event"
        );
    }

    #[test]
    fn scenario_population_brackets_conserved() {
        let m = migration_scenario();
        let (mut world, source, dest, old_faction) = (m.world, m.source, m.dest, m.faction);

        let total_before = {
            let sp = world
                .entities
                .get(&source)
                .unwrap()
                .data
                .as_settlement()
                .unwrap();
            let dp = world
                .entities
                .get(&dest)
                .unwrap()
                .data
                .as_settlement()
                .unwrap();
            sp.population + dp.population
        };

        // Create conqueror and simulate conquest
        let t5 = ts(5);
        let ev5 = world.add_event(EventKind::FactionFormed, t5, "formed".to_string());
        let new_faction = world.add_entity(
            EntityKind::Faction,
            "Conquerors".to_string(),
            None,
            EntityData::default_for_kind(EntityKind::Faction),
            ev5,
        );
        simulate_conquest(&mut world, source, old_faction, new_faction, 5);

        // Run migration
        world.current_time = ts(5);
        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals = Vec::new();
        let mut system = MigrationSystem;
        let mut ctx = TickContext {
            world: &mut world,
            rng: &mut rng,
            signals: &mut signals,
            inbox: &[],
        };
        system.tick(&mut ctx);

        let total_after = {
            let sp = world
                .entities
                .get(&source)
                .unwrap()
                .data
                .as_settlement()
                .unwrap();
            let dp = world
                .entities
                .get(&dest)
                .unwrap()
                .data
                .as_settlement()
                .unwrap();
            sp.population + dp.population
        };

        assert_eq!(
            total_before, total_after,
            "total population should be conserved: before={total_before}, after={total_after}"
        );
    }

    #[test]
    fn scenario_war_produces_migration_events() {
        use crate::scenario::Scenario;

        // War-zone emigration: settlements in factions with AtWar relationships
        // lose 3-8% population per year to migration. Refugees flee to friendly
        // destinations, so we need a second settlement in the defender's faction.
        let mut s = Scenario::at_year(100);
        let war = s.add_war_between("Aggressor", "Defender", 30);

        // Ensure populations are large enough to trigger visible migration
        s.modify_settlement(war.attacker.settlement, |sd| sd.population = 500);
        s.modify_settlement(war.defender.settlement, |sd| sd.population = 500);

        // Add a rear settlement in the defender's faction so war-zone refugees
        // have a same-faction destination (faction_affinity = 1.0)
        let rear_region = s.add_region("Rear Region");
        s.make_adjacent(rear_region, war.defender.region);
        s.settlement("Rear Town", war.defender.faction, rear_region)
            .population(200)
            .prosperity(0.5)
            .id();

        let mut systems: Vec<Box<dyn SimSystem>> = vec![
            Box::new(DemographicsSystem),
            Box::new(EconomySystem),
            Box::new(ConflictSystem),
            Box::new(MigrationSystem),
            Box::new(PoliticsSystem),
        ];
        let world = s.run(&mut systems, 10, 42);

        let migration_count = world
            .events
            .values()
            .filter(|e| e.kind == EventKind::Migration)
            .count();

        assert!(
            migration_count > 0,
            "expected migration events in 10-year war scenario, got {migration_count}"
        );
    }

    #[test]
    fn scenario_migration_records_destination_population() {
        use crate::scenario::Scenario;
        use crate::testutil;

        // Create a war scenario that forces migration to a rear settlement
        let mut s = Scenario::at_year(100);
        let war = s.add_war_between("Attacker", "Defender", 100);
        // Rear settlement as migration destination
        let rear_region = s.add_region("Rear Region");
        s.make_adjacent(rear_region, war.defender.region);
        let dest = s
            .settlement("Rear Town", war.defender.faction, rear_region)
            .population(200)
            .prosperity(0.5)
            .id();

        let mut systems: Vec<Box<dyn SimSystem>> = vec![
            Box::new(DemographicsSystem),
            Box::new(EconomySystem),
            Box::new(ConflictSystem),
            Box::new(MigrationSystem),
            Box::new(PoliticsSystem),
        ];
        let world = s.run(&mut systems, 10, 42);

        let migration_count = world
            .events
            .values()
            .filter(|e| e.kind == EventKind::Migration)
            .count();

        if migration_count > 0 {
            // Destination should have a population record_change
            testutil::assert_property_changed(&world, dest, "population");
        }
    }
}
