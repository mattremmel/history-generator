//! Migration system — migrated from `src/sim/migration.rs`.
//!
//! One reaction handler (Reactions phase):
//! 1. `handle_settlement_captured_for_migration` — records conquest info into
//!    `ConquestMigrationQueue` resource for the yearly update system.
//!
//! One yearly system (Update phase):
//! 2. `process_migrations` — drains conquest queue, detects war-zone and
//!    low-prosperity settlements, BFS-finds destinations, emits
//!    MigratePopulation / RelocatePerson / AbandonSettlement commands.

use std::collections::{BTreeSet, VecDeque};

use bevy_app::App;
use bevy_ecs::entity::Entity;
use bevy_ecs::message::{MessageReader, MessageWriter};
use bevy_ecs::query::With;
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Query, Res, ResMut};
use rand::Rng;

use crate::ecs::clock::SimClock;
use crate::ecs::commands::{SimCommand, SimCommandKind};
use crate::ecs::components::{
    EcsBuildingBonuses, Faction, Person, PersonCore, Region, RegionState, Settlement,
    SettlementCore, SettlementTrade, SimEntity,
};
use crate::ecs::conditions::yearly;
use crate::ecs::events::SimReactiveEvent;
use crate::ecs::relationships::{
    LocatedIn, LocatedInSources, MemberOf, RegionAdjacency, RelationshipGraph,
};
use crate::ecs::resources::SimRng;
use crate::ecs::schedule::{SimPhase, SimTick};
use crate::model::event::{EventKind, ParticipantRole};
use crate::model::traits::Trait;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

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

/// Score multiplier for destination settlements with a port.
const PORT_DESTINATION_BONUS: f64 = 1.3;

/// Minimum population before a settlement is considered abandoned.
const ABANDONMENT_THRESHOLD: u32 = 10;

/// NPC flee chances by trait.
const CAUTIOUS_FLEE_CHANCE: f64 = 0.60;
const DEFAULT_FLEE_CHANCE: f64 = 0.30;
const RESISTANT_FLEE_CHANCE: f64 = 0.15;

// ---------------------------------------------------------------------------
// Conquest migration queue resource
// ---------------------------------------------------------------------------

/// Info about a recently captured settlement, populated by the reaction handler.
#[derive(Debug, Clone)]
pub struct ConquestInfo {
    pub settlement: Entity,
    pub old_faction: Option<Entity>,
    pub event_id: u64,
}

/// Queue of recent conquests awaiting migration processing on the next yearly tick.
#[derive(Resource, Debug, Clone, Default)]
pub struct ConquestMigrationQueue(pub Vec<ConquestInfo>);

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

struct MigrationSource {
    settlement: Entity,
    region: Entity,
    /// Faction refugees identify with (old faction for conquest, current for economic).
    affinity_faction: Entity,
    fraction_min: f64,
    fraction_max: f64,
    cause_event_id: Option<u64>,
    is_conquest: bool,
}

struct SettlementInfo {
    entity: Entity,
    region: Entity,
    faction: Entity,
    population: u32,
    prosperity: f64,
    has_port: bool,
}

struct Candidate {
    entity: Entity,
    score: f64,
}

// ---------------------------------------------------------------------------
// Plugin registration
// ---------------------------------------------------------------------------

pub fn add_migration_systems(app: &mut App) {
    app.init_resource::<ConquestMigrationQueue>();
    app.add_systems(
        SimTick,
        process_migrations.run_if(yearly).in_set(SimPhase::Update),
    );
    app.add_systems(
        SimTick,
        handle_settlement_captured_for_migration.in_set(SimPhase::Reactions),
    );
}

// ---------------------------------------------------------------------------
// System 1: Reaction handler — record conquests
// ---------------------------------------------------------------------------

fn handle_settlement_captured_for_migration(
    mut events: MessageReader<SimReactiveEvent>,
    mut queue: ResMut<ConquestMigrationQueue>,
) {
    for event in events.read() {
        if let SimReactiveEvent::SettlementCaptured {
            event_id,
            settlement,
            old_faction,
            ..
        } = event
        {
            queue.0.push(ConquestInfo {
                settlement: *settlement,
                old_faction: *old_faction,
                event_id: *event_id,
            });
        }
    }
}

// ---------------------------------------------------------------------------
// System 2: Yearly migration processing
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn process_migrations(
    mut conquest_queue: ResMut<ConquestMigrationQueue>,
    mut rng: ResMut<SimRng>,
    clock: Res<SimClock>,
    adjacency: Res<RegionAdjacency>,
    rel_graph: Res<RelationshipGraph>,
    settlements: Query<
        (
            Entity,
            &SimEntity,
            &SettlementCore,
            &LocatedIn,
            &MemberOf,
            &SettlementTrade,
            &EcsBuildingBonuses,
        ),
        With<Settlement>,
    >,
    regions: Query<(&RegionState, Option<&LocatedInSources>), With<Region>>,
    persons: Query<
        (
            Entity,
            &SimEntity,
            &PersonCore,
            &LocatedIn,
            Option<&MemberOf>,
        ),
        With<Person>,
    >,
    factions: Query<Entity, With<Faction>>,
    mut commands: MessageWriter<SimCommand>,
) {
    let current_year = clock.time.year();

    // 1. Collect all live settlement info
    let settlement_infos: Vec<SettlementInfo> = settlements
        .iter()
        .filter(|(_, sim, ..)| sim.end.is_none())
        .map(
            |(entity, _, core, loc, member, _trade, bonuses)| SettlementInfo {
                entity,
                region: loc.0,
                faction: member.0,
                population: core.population,
                prosperity: core.prosperity,
                has_port: bonuses.port_trade > 0.0,
            },
        )
        .collect();

    // 2. Drain conquest queue and build migration sources
    let mut sources: Vec<MigrationSource> = Vec::new();
    let conquest_entries: Vec<ConquestInfo> = conquest_queue.0.drain(..).collect();

    // Track which settlements are conquest sources so we don't double-count
    let conquest_settlements: BTreeSet<Entity> =
        conquest_entries.iter().map(|c| c.settlement).collect();

    for conquest in &conquest_entries {
        // Skip if there was no previous faction (unowned settlement)
        let Some(old_faction) = conquest.old_faction else {
            continue;
        };
        // Find the settlement info (must still be alive)
        let Some(info) = settlement_infos
            .iter()
            .find(|s| s.entity == conquest.settlement)
        else {
            continue;
        };
        sources.push(MigrationSource {
            settlement: conquest.settlement,
            region: info.region,
            affinity_faction: old_faction,
            fraction_min: CONQUEST_REFUGEE_MIN,
            fraction_max: CONQUEST_REFUGEE_MAX,
            cause_event_id: Some(conquest.event_id),
            is_conquest: true,
        });
    }

    // 3. Check war-zone and low-prosperity sources
    for info in &settlement_infos {
        if conquest_settlements.contains(&info.entity) {
            continue; // Already a conquest source
        }

        // Check if faction is at war
        let faction_at_war = factions.iter().any(|other_faction| {
            other_faction != info.faction && rel_graph.are_at_war(info.faction, other_faction)
        });

        if faction_at_war {
            sources.push(MigrationSource {
                settlement: info.entity,
                region: info.region,
                affinity_faction: info.faction,
                fraction_min: WAR_ZONE_EMIGRATION_MIN,
                fraction_max: WAR_ZONE_EMIGRATION_MAX,
                cause_event_id: None,
                is_conquest: false,
            });
            continue; // Don't stack with low-prosperity
        }

        // Check low prosperity
        if info.prosperity < LOW_PROSPERITY_THRESHOLD {
            sources.push(MigrationSource {
                settlement: info.entity,
                region: info.region,
                affinity_faction: info.faction,
                fraction_min: LOW_PROSPERITY_EMIGRATION_MIN,
                fraction_max: LOW_PROSPERITY_EMIGRATION_MAX,
                cause_event_id: None,
                is_conquest: false,
            });
        }
    }

    if sources.is_empty() {
        return;
    }

    // 4. Process each migration source
    let rng = &mut rng.0;
    for source in &sources {
        // Find best destination via BFS
        let reachable =
            bfs_reachable_regions(source.region, &adjacency, &regions, &settlement_infos);

        let dest = find_best_destination(source, &reachable, &settlement_infos, &rel_graph);

        let Some(dest_entity) = dest else {
            continue;
        };

        // Get current source population
        let Some(source_info) = settlement_infos
            .iter()
            .find(|s| s.entity == source.settlement)
        else {
            continue;
        };
        if source_info.population == 0 {
            continue;
        }

        // Compute refugee count
        let fraction: f64 = rng.random_range(source.fraction_min..source.fraction_max);
        let refugee_count = ((source_info.population as f64) * fraction).round() as u32;
        if refugee_count == 0 {
            continue;
        }

        // Get names for event description
        let source_name = settlements
            .get(source.settlement)
            .map(|(_, sim, ..)| sim.name.clone())
            .unwrap_or_else(|_| "Unknown".to_string());
        let dest_name = settlements
            .get(dest_entity)
            .map(|(_, sim, ..)| sim.name.clone())
            .unwrap_or_else(|_| "Unknown".to_string());

        // Emit MigratePopulation command
        let description = if source.cause_event_id.is_some() {
            format!(
                "{refugee_count} refugees fled from {source_name} to {dest_name} in year {current_year}"
            )
        } else {
            format!(
                "{refugee_count} people migrated from {source_name} to {dest_name} in year {current_year}"
            )
        };

        let mut cmd = SimCommand::new(
            SimCommandKind::MigratePopulation {
                from_settlement: source.settlement,
                to_settlement: dest_entity,
                count: refugee_count,
            },
            EventKind::Migration,
            &description,
        )
        .with_participant(source.settlement, ParticipantRole::Origin)
        .with_participant(dest_entity, ParticipantRole::Destination);

        if let Some(cause_id) = source.cause_event_id {
            cmd = cmd.caused_by(cause_id);
        }

        commands.write(cmd);

        // Handle NPC migration for conquest refugees
        if source.is_conquest {
            emit_npc_relocations(
                rng,
                source.settlement,
                dest_entity,
                source.cause_event_id,
                current_year,
                &persons,
                &settlements,
                &mut commands,
            );
        }

        // Check for settlement abandonment
        let remaining_pop = source_info.population.saturating_sub(refugee_count);
        if remaining_pop < ABANDONMENT_THRESHOLD {
            let abandon_cmd = SimCommand::new(
                SimCommandKind::AbandonSettlement {
                    settlement: source.settlement,
                },
                EventKind::Abandoned,
                format!("{source_name} abandoned after mass exodus in year {current_year}"),
            )
            .with_participant(source.settlement, ParticipantRole::Subject);

            commands.write(abandon_cmd);
        }
    }
}

// ---------------------------------------------------------------------------
// BFS reachable regions
// ---------------------------------------------------------------------------

fn bfs_reachable_regions(
    start: Entity,
    adjacency: &RegionAdjacency,
    regions: &Query<(&RegionState, Option<&LocatedInSources>), With<Region>>,
    settlement_infos: &[SettlementInfo],
) -> Vec<(Entity, usize)> {
    let mut result = Vec::new();
    let mut visited = BTreeSet::from([start]);
    let mut queue: VecDeque<(Entity, usize)> = VecDeque::new();

    let source_has_port = region_has_port(start, settlement_infos);

    // Start region is distance 1 (so same-region settlements still get scored)
    result.push((start, 1));

    for &adj in adjacency.neighbors(start) {
        // Block water regions unless source has a port
        if is_water_region(adj, regions) && !source_has_port {
            continue;
        }
        if visited.insert(adj) {
            queue.push_back((adj, 1));
            result.push((adj, 1));
        }
    }

    while let Some((current, depth)) = queue.pop_front() {
        if depth >= MAX_BFS_HOPS {
            continue;
        }
        let current_is_water = is_water_region(current, regions);
        for &adj in adjacency.neighbors(current) {
            let adj_is_water = is_water_region(adj, regions);
            // Water -> land: only at port regions
            if current_is_water && !adj_is_water && !region_has_port(adj, settlement_infos) {
                continue;
            }
            // Land -> water: only from port regions
            if !current_is_water && adj_is_water && !region_has_port(current, settlement_infos) {
                continue;
            }
            if visited.insert(adj) {
                queue.push_back((adj, depth + 1));
                result.push((adj, depth + 1));
            }
        }
    }

    result
}

fn is_water_region(
    region: Entity,
    regions: &Query<(&RegionState, Option<&LocatedInSources>), With<Region>>,
) -> bool {
    regions
        .get(region)
        .map(|(state, _)| state.terrain.is_water())
        .unwrap_or(false)
}

fn region_has_port(region: Entity, settlement_infos: &[SettlementInfo]) -> bool {
    settlement_infos
        .iter()
        .any(|s| s.region == region && s.has_port)
}

// ---------------------------------------------------------------------------
// Destination scoring
// ---------------------------------------------------------------------------

fn find_best_destination(
    source: &MigrationSource,
    reachable: &[(Entity, usize)],
    settlement_infos: &[SettlementInfo],
    rel_graph: &RelationshipGraph,
) -> Option<Entity> {
    let mut candidates: Vec<Candidate> = Vec::new();

    for &(region, distance) in reachable {
        for info in settlement_infos {
            if info.entity == source.settlement {
                continue;
            }
            if info.region != region {
                continue;
            }

            let faction_affinity =
                compute_faction_affinity(source.affinity_faction, info.faction, rel_graph);
            if faction_affinity <= 0.0 {
                continue; // hostile — skip
            }

            // Capacity room: settlements above 2000 are less attractive
            let capacity_room = (1.0 - info.population as f64 / 3000.0).max(0.1);

            let port_mult = if info.has_port {
                PORT_DESTINATION_BONUS
            } else {
                1.0
            };

            let dist_factor = 1.0 / (distance as f64).max(1.0);
            let score = faction_affinity
                * dist_factor
                * (0.3 + info.prosperity)
                * capacity_room
                * port_mult;

            candidates.push(Candidate {
                entity: info.entity,
                score,
            });
        }
    }

    candidates.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    candidates.first().map(|c| c.entity)
}

fn compute_faction_affinity(
    source_faction: Entity,
    dest_faction: Entity,
    rel_graph: &RelationshipGraph,
) -> f64 {
    if source_faction == dest_faction {
        return 1.0; // Same faction — strong preference
    }

    if rel_graph.are_at_war(source_faction, dest_faction)
        || rel_graph.are_enemies(source_faction, dest_faction)
    {
        return 0.0;
    }
    if rel_graph.are_allies(source_faction, dest_faction) {
        return 0.7;
    }

    0.2 // Neutral faction
}

// ---------------------------------------------------------------------------
// NPC relocation
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn emit_npc_relocations(
    rng: &mut impl Rng,
    source_settlement: Entity,
    dest_settlement: Entity,
    cause_event_id: Option<u64>,
    current_year: u32,
    persons: &Query<
        (
            Entity,
            &SimEntity,
            &PersonCore,
            &LocatedIn,
            Option<&MemberOf>,
        ),
        With<Person>,
    >,
    settlements: &Query<
        (
            Entity,
            &SimEntity,
            &SettlementCore,
            &LocatedIn,
            &MemberOf,
            &SettlementTrade,
            &EcsBuildingBonuses,
        ),
        With<Settlement>,
    >,
    commands: &mut MessageWriter<SimCommand>,
) {
    let dest_name = settlements
        .get(dest_settlement)
        .map(|(_, sim, ..)| sim.name.clone())
        .unwrap_or_else(|_| "Unknown".to_string());

    // Find NPCs in the source settlement
    let npcs: Vec<(Entity, String, f64)> = persons
        .iter()
        .filter(|(_, sim, _, loc, _)| sim.end.is_none() && loc.0 == source_settlement)
        .map(|(entity, sim, core, _, _)| {
            let flee_chance = if core.traits.contains(&Trait::Cautious) {
                CAUTIOUS_FLEE_CHANCE
            } else if core.traits.contains(&Trait::Aggressive)
                || core.traits.contains(&Trait::Honorable)
            {
                RESISTANT_FLEE_CHANCE
            } else {
                DEFAULT_FLEE_CHANCE
            };
            (entity, sim.name.clone(), flee_chance)
        })
        .collect();

    for (npc_entity, npc_name, flee_chance) in npcs {
        if rng.random_range(0.0..1.0) >= flee_chance {
            continue;
        }

        let description = format!("{npc_name} fled to {dest_name} in year {current_year}");
        let mut cmd = SimCommand::new(
            SimCommandKind::RelocatePerson {
                person: npc_entity,
                to_settlement: dest_settlement,
            },
            EventKind::Migration,
            description,
        )
        .with_participant(npc_entity, ParticipantRole::Subject)
        .with_participant(source_settlement, ParticipantRole::Origin)
        .with_participant(dest_settlement, ParticipantRole::Destination);

        if let Some(cause_id) = cause_event_id {
            cmd = cmd.caused_by(cause_id);
        }

        commands.write(cmd);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::app::build_sim_app;
    use crate::ecs::clock::SimClock;
    use crate::ecs::components::{
        EcsBuildingBonuses, EcsSeasonalModifiers, FactionCore, FactionDiplomacy, FactionMilitary,
        PersonEducation, PersonReputation, PersonSocial, SettlementCrime, SettlementCulture,
        SettlementDisease, SettlementEducation, SettlementMilitary, SettlementTrade,
    };
    use crate::ecs::relationships::{
        MemberOf, RegionAdjacency, RelationshipGraph, RelationshipMeta,
    };
    use crate::ecs::resources::EventLog;
    use crate::ecs::schedule::SimTick;
    use crate::ecs::spawn;
    use crate::ecs::time::{MINUTES_PER_MONTH, SimTime};
    use crate::model::event::EventKind;
    use crate::model::traits::Trait;
    use crate::model::{PopulationBreakdown, Terrain};

    fn setup_app() -> bevy_app::App {
        let mut app = build_sim_app(100);
        app.insert_resource(RegionAdjacency::new());
        app.insert_resource(ConquestMigrationQueue::default());
        add_migration_systems(&mut app);
        app
    }

    fn tick_years(app: &mut bevy_app::App, years: u32) {
        let start_year = app.world().resource::<SimClock>().time.year();
        for y in 0..years {
            for m in 1..=12u32 {
                let time = SimTime::from_year_month(start_year + y, m);
                app.world_mut().resource_mut::<SimClock>().time = time;
                app.world_mut().run_schedule(SimTick);
            }
        }
    }

    fn spawn_faction(app: &mut bevy_app::App, sim_id: u64) -> Entity {
        spawn::spawn_faction(
            app.world_mut(),
            sim_id,
            format!("Faction {sim_id}"),
            Some(SimTime::from_year(50)),
            FactionCore {
                stability: 0.5,
                happiness: 0.5,
                legitimacy: 0.5,
                treasury: 100.0,
                ..FactionCore::default()
            },
            FactionDiplomacy::default(),
            FactionMilitary::default(),
        )
    }

    fn spawn_region(app: &mut bevy_app::App, sim_id: u64, terrain: Terrain) -> Entity {
        spawn::spawn_region(
            app.world_mut(),
            sim_id,
            format!("Region {sim_id}"),
            Some(SimTime::from_year(0)),
            RegionState {
                terrain,
                ..RegionState::default()
            },
        )
    }

    fn spawn_settlement_with(
        app: &mut bevy_app::App,
        sim_id: u64,
        name: &str,
        population: u32,
        prosperity: f64,
        faction: Entity,
        region: Entity,
        port_trade: f64,
    ) -> Entity {
        let entity = spawn::spawn_settlement(
            app.world_mut(),
            sim_id,
            name.to_string(),
            Some(SimTime::from_year(50)),
            SettlementCore {
                population,
                population_breakdown: PopulationBreakdown::from_total(population),
                prosperity,
                ..SettlementCore::default()
            },
            SettlementCulture::default(),
            SettlementDisease::default(),
            SettlementTrade::default(),
            SettlementMilitary::default(),
            SettlementCrime::default(),
            SettlementEducation::default(),
            EcsSeasonalModifiers::default(),
            EcsBuildingBonuses {
                port_trade,
                ..EcsBuildingBonuses::default()
            },
        );
        app.world_mut()
            .entity_mut(entity)
            .insert((LocatedIn(region), MemberOf(faction)));
        entity
    }

    fn spawn_person_with_traits(
        app: &mut bevy_app::App,
        sim_id: u64,
        name: &str,
        traits: Vec<Trait>,
        faction: Entity,
        settlement: Entity,
    ) -> Entity {
        let entity = spawn::spawn_person(
            app.world_mut(),
            sim_id,
            name.to_string(),
            Some(SimTime::from_year(80)),
            PersonCore {
                traits,
                ..PersonCore::default()
            },
            PersonReputation::default(),
            PersonSocial::default(),
            PersonEducation::default(),
        );
        app.world_mut()
            .entity_mut(entity)
            .insert((LocatedIn(settlement), MemberOf(faction)));
        entity
    }

    // -----------------------------------------------------------------------
    // Test: conquest triggers migration
    // -----------------------------------------------------------------------

    #[test]
    fn conquest_triggers_migrate_command() {
        let mut app = setup_app();

        // Create two regions, two factions, two settlements
        let region_a = spawn_region(&mut app, 1, Terrain::Plains);
        let region_b = spawn_region(&mut app, 2, Terrain::Plains);

        // Make regions adjacent
        {
            let mut adj = app.world_mut().resource_mut::<RegionAdjacency>();
            adj.add_edge(region_a, region_b);
        }

        let old_faction = spawn_faction(&mut app, 10);
        let new_faction = spawn_faction(&mut app, 11);

        let source = spawn_settlement_with(
            &mut app,
            100,
            "Source Town",
            500,
            0.6,
            new_faction,
            region_a,
            0.0,
        );
        let _dest = spawn_settlement_with(
            &mut app,
            101,
            "Dest Town",
            300,
            0.7,
            old_faction,
            region_b,
            0.0,
        );

        // Simulate conquest by inserting into the ConquestMigrationQueue
        app.world_mut()
            .resource_mut::<ConquestMigrationQueue>()
            .0
            .push(ConquestInfo {
                settlement: source,
                old_faction: Some(old_faction),
                event_id: 999,
            });

        // Tick one year to trigger process_migrations
        tick_years(&mut app, 1);

        // After tick, the applicator should have processed the MigratePopulation command.
        // Check that source settlement lost population.
        let source_pop = app
            .world()
            .get::<SettlementCore>(source)
            .unwrap()
            .population;
        assert!(
            source_pop < 500,
            "source should lose population after conquest migration: got {source_pop}"
        );

        // Check event log for migration event
        let event_log = app.world().resource::<EventLog>();
        let migration_events: Vec<_> = event_log
            .events
            .iter()
            .filter(|e| e.kind == EventKind::Migration)
            .collect();
        assert!(
            !migration_events.is_empty(),
            "should have migration events in the event log"
        );
    }

    // -----------------------------------------------------------------------
    // Test: low prosperity triggers emigration
    // -----------------------------------------------------------------------

    #[test]
    fn low_prosperity_triggers_emigration() {
        let mut app = setup_app();

        let region_a = spawn_region(&mut app, 1, Terrain::Plains);
        let region_b = spawn_region(&mut app, 2, Terrain::Plains);

        {
            let mut adj = app.world_mut().resource_mut::<RegionAdjacency>();
            adj.add_edge(region_a, region_b);
        }

        let faction = spawn_faction(&mut app, 10);

        // Source: low prosperity
        let source =
            spawn_settlement_with(&mut app, 100, "Poor Town", 500, 0.1, faction, region_a, 0.0);
        // Dest: high prosperity
        let _dest =
            spawn_settlement_with(&mut app, 101, "Rich Town", 300, 0.8, faction, region_b, 0.0);

        tick_years(&mut app, 1);

        let source_pop = app
            .world()
            .get::<SettlementCore>(source)
            .unwrap()
            .population;
        assert!(
            source_pop < 500,
            "low prosperity should cause emigration: got {source_pop}"
        );
    }

    // -----------------------------------------------------------------------
    // Test: abandonment emitted for tiny settlements
    // -----------------------------------------------------------------------

    #[test]
    fn abandonment_emitted_for_tiny_settlements() {
        let mut app = setup_app();

        let region_a = spawn_region(&mut app, 1, Terrain::Plains);
        let region_b = spawn_region(&mut app, 2, Terrain::Plains);

        {
            let mut adj = app.world_mut().resource_mut::<RegionAdjacency>();
            adj.add_edge(region_a, region_b);
        }

        let old_faction = spawn_faction(&mut app, 10);
        let new_faction = spawn_faction(&mut app, 11);

        // Source: very small population — conquest will push it below threshold
        let source = spawn_settlement_with(
            &mut app,
            100,
            "Tiny Village",
            8,
            0.5,
            new_faction,
            region_a,
            0.0,
        );
        let _dest = spawn_settlement_with(
            &mut app,
            101,
            "Dest Town",
            300,
            0.7,
            old_faction,
            region_b,
            0.0,
        );

        // Simulate conquest
        app.world_mut()
            .resource_mut::<ConquestMigrationQueue>()
            .0
            .push(ConquestInfo {
                settlement: source,
                old_faction: Some(old_faction),
                event_id: 888,
            });

        tick_years(&mut app, 1);

        // Source settlement should be ended (abandoned)
        let source_entity = app.world().get::<SimEntity>(source).unwrap();
        assert!(
            source_entity.end.is_some(),
            "settlement with tiny pop should be abandoned after conquest"
        );

        // Abandoned event should exist in event log
        let event_log = app.world().resource::<EventLog>();
        let has_abandoned = event_log
            .events
            .iter()
            .any(|e| e.kind == EventKind::Abandoned);
        assert!(has_abandoned, "should have abandonment event in event log");
    }

    // -----------------------------------------------------------------------
    // Test: NPC relocation on conquest
    // -----------------------------------------------------------------------

    #[test]
    fn conquest_relocates_npcs() {
        let mut app = setup_app();

        let region_a = spawn_region(&mut app, 1, Terrain::Plains);
        let region_b = spawn_region(&mut app, 2, Terrain::Plains);

        {
            let mut adj = app.world_mut().resource_mut::<RegionAdjacency>();
            adj.add_edge(region_a, region_b);
        }

        let old_faction = spawn_faction(&mut app, 10);
        let new_faction = spawn_faction(&mut app, 11);

        let source = spawn_settlement_with(
            &mut app,
            100,
            "Source",
            500,
            0.6,
            new_faction,
            region_a,
            0.0,
        );
        let dest =
            spawn_settlement_with(&mut app, 101, "Dest", 300, 0.7, old_faction, region_b, 0.0);

        // Add cautious NPCs (high flee chance = 0.60)
        for i in 0..5 {
            spawn_person_with_traits(
                &mut app,
                200 + i,
                &format!("NPC_{i}"),
                vec![Trait::Cautious],
                old_faction,
                source,
            );
        }

        // Simulate conquest
        app.world_mut()
            .resource_mut::<ConquestMigrationQueue>()
            .0
            .push(ConquestInfo {
                settlement: source,
                old_faction: Some(old_faction),
                event_id: 777,
            });

        tick_years(&mut app, 1);

        // Some NPCs should have relocated to dest
        let npcs_at_dest: usize = app
            .world_mut()
            .query_filtered::<&LocatedIn, With<Person>>()
            .iter(app.world())
            .filter(|loc| loc.0 == dest)
            .count();

        // With 5 cautious NPCs (60% flee chance each), we expect some to have moved.
        // Seed 42 should produce at least 1 relocation.
        assert!(
            npcs_at_dest > 0,
            "at least some cautious NPCs should flee to destination: got {npcs_at_dest}"
        );
    }

    // -----------------------------------------------------------------------
    // Test: war zone triggers emigration
    // -----------------------------------------------------------------------

    #[test]
    fn war_zone_triggers_emigration() {
        let mut app = setup_app();

        let region_a = spawn_region(&mut app, 1, Terrain::Plains);
        let region_b = spawn_region(&mut app, 2, Terrain::Plains);

        {
            let mut adj = app.world_mut().resource_mut::<RegionAdjacency>();
            adj.add_edge(region_a, region_b);
        }

        let faction_a = spawn_faction(&mut app, 10);
        let faction_b = spawn_faction(&mut app, 11);

        // Put factions at war
        {
            let pair = RelationshipGraph::canonical_pair(faction_a, faction_b);
            let mut rel_graph = app.world_mut().resource_mut::<RelationshipGraph>();
            rel_graph
                .at_war
                .insert(pair, RelationshipMeta::new(SimTime::from_year(99)));
        }

        let source = spawn_settlement_with(
            &mut app,
            100,
            "Warzone Town",
            500,
            0.5,
            faction_a,
            region_a,
            0.0,
        );
        let _dest = spawn_settlement_with(
            &mut app,
            101,
            "Safe Town",
            300,
            0.7,
            faction_a,
            region_b,
            0.0,
        );

        tick_years(&mut app, 1);

        let source_pop = app
            .world()
            .get::<SettlementCore>(source)
            .unwrap()
            .population;
        assert!(
            source_pop < 500,
            "war zone should cause emigration: got {source_pop}"
        );
    }

    // -----------------------------------------------------------------------
    // Test: BFS respects water/port rules
    // -----------------------------------------------------------------------

    #[test]
    fn bfs_blocks_water_without_port() {
        let mut app = setup_app();

        let region_land = spawn_region(&mut app, 1, Terrain::Plains);
        let region_water = spawn_region(&mut app, 2, Terrain::ShallowWater);
        let region_far = spawn_region(&mut app, 3, Terrain::Plains);

        {
            let mut adj = app.world_mut().resource_mut::<RegionAdjacency>();
            adj.add_edge(region_land, region_water);
            adj.add_edge(region_water, region_far);
        }

        let faction = spawn_faction(&mut app, 10);

        // Source on land, no port — can't cross water
        let source = spawn_settlement_with(
            &mut app,
            100,
            "Inland Town",
            500,
            0.1,
            faction,
            region_land,
            0.0,
        );
        // Destination across water
        let dest = spawn_settlement_with(
            &mut app, 101, "Far Town", 300, 0.8, faction, region_far, 0.0,
        );

        tick_years(&mut app, 1);

        // Source should not lose population because no reachable destination
        // (water blocks BFS without a port)
        let source_pop = app
            .world()
            .get::<SettlementCore>(source)
            .unwrap()
            .population;

        let dest_pop = app.world().get::<SettlementCore>(dest).unwrap().population;

        // Destination should not have gained population
        assert_eq!(
            dest_pop, 300,
            "destination across water without port should not gain population: got {dest_pop}"
        );
    }
}
