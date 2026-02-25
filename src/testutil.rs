use rand::SeedableRng;
use rand::rngs::SmallRng;

use crate::model::*;
use crate::scenario::Scenario;
use crate::sim::{
    ActionSystem, AgencySystem, BuildingSystem, ConflictSystem, CrimeSystem, CultureSystem,
    DemographicsSystem, DiseaseSystem, EconomySystem, EnvironmentSystem, ItemSystem,
    KnowledgeSystem, MigrationSystem, PoliticsSystem, ReligionSystem, ReputationSystem, Signal,
    SignalKind, SimConfig, SimSystem, TickContext, run,
};
use crate::worldgen::{self, config::WorldGenConfig};

// ---------------------------------------------------------------------------
// Tick execution helpers
// ---------------------------------------------------------------------------

/// Run a single system tick at the start of the given year. Returns emitted signals.
pub fn tick_system(
    world: &mut World,
    system: &mut dyn SimSystem,
    year: u32,
    seed: u64,
) -> Vec<Signal> {
    tick_system_at(world, system, SimTimestamp::from_year(year), seed)
}

/// Run a single system tick at a specific timestamp. Returns emitted signals.
pub fn tick_system_at(
    world: &mut World,
    system: &mut dyn SimSystem,
    time: SimTimestamp,
    seed: u64,
) -> Vec<Signal> {
    world.current_time = time;
    let mut rng = SmallRng::seed_from_u64(seed);
    let mut signals = Vec::new();
    let mut ctx = TickContext {
        world,
        rng: &mut rng,
        signals: &mut signals,
        inbox: &[],
    };
    system.tick(&mut ctx);
    signals
}

/// Run a system's handle_signals with the given inbox. Returns newly emitted signals.
pub fn deliver_signals(
    world: &mut World,
    system: &mut dyn SimSystem,
    inbox: &[Signal],
    seed: u64,
) -> Vec<Signal> {
    let mut rng = SmallRng::seed_from_u64(seed);
    let mut signals = Vec::new();
    let mut ctx = TickContext {
        world,
        rng: &mut rng,
        signals: &mut signals,
        inbox,
    };
    system.handle_signals(&mut ctx);
    signals
}

/// Run a full tick + handle_signals cycle for a single system. Returns all signals.
pub fn full_tick(
    world: &mut World,
    system: &mut dyn SimSystem,
    year: u32,
    seed: u64,
) -> Vec<Signal> {
    let tick_signals = tick_system(world, system, year, seed);
    if tick_signals.is_empty() {
        return tick_signals;
    }
    let reaction_signals = deliver_signals(world, system, &tick_signals, seed);
    let mut all = tick_signals;
    all.extend(reaction_signals);
    all
}

/// Run multiple years using the standard simulation loop.
pub fn run_years(world: &mut World, systems: &mut [Box<dyn SimSystem>], num_years: u32, seed: u64) {
    let start_year = world.current_time.year();
    let _ = run(world, systems, SimConfig::new(start_year, num_years, seed));
}

/// Generate a world from worldgen and run the simulation with the given systems.
pub fn generate_and_run(seed: u64, num_years: u32, mut systems: Vec<Box<dyn SimSystem>>) -> World {
    let config = WorldGenConfig {
        seed,
        ..WorldGenConfig::default()
    };
    let mut world = worldgen::generate_world(config);
    run(&mut world, &mut systems, SimConfig::new(1, num_years, seed))
        .expect("simulation flush failed");
    world
}

// ---------------------------------------------------------------------------
// System set constructors
// ---------------------------------------------------------------------------

/// Core systems: Demographics + Economy + Politics.
pub fn core_systems() -> Vec<Box<dyn SimSystem>> {
    vec![
        Box::new(DemographicsSystem),
        Box::new(EconomySystem),
        Box::new(PoliticsSystem),
    ]
}

/// Core systems plus Conflicts (with Environment for seasonal modifiers).
pub fn combat_systems() -> Vec<Box<dyn SimSystem>> {
    vec![
        Box::new(EnvironmentSystem),
        Box::new(DemographicsSystem),
        Box::new(EconomySystem),
        Box::new(ConflictSystem),
        Box::new(PoliticsSystem),
    ]
}

/// All systems in canonical tick order.
pub fn all_systems() -> Vec<Box<dyn SimSystem>> {
    vec![
        Box::new(EnvironmentSystem),
        Box::new(DemographicsSystem),
        Box::new(BuildingSystem),
        Box::new(EconomySystem),
        Box::new(ConflictSystem),
        Box::new(MigrationSystem),
        Box::new(CrimeSystem),
        Box::new(DiseaseSystem),
        Box::new(CultureSystem),
        Box::new(ReligionSystem),
        Box::new(PoliticsSystem),
        Box::new(ReputationSystem),
        Box::new(ItemSystem),
        Box::new(AgencySystem::new()),
        Box::new(ActionSystem),
        Box::new(KnowledgeSystem),
    ]
}

// ---------------------------------------------------------------------------
// Query helpers
// ---------------------------------------------------------------------------

/// Get all living entities of a given kind that have an active relationship of `rel_kind`
/// pointing to `target`. Generic building block for entity queries.
pub fn related_living(
    world: &World,
    target: u64,
    rel_kind: RelationshipKind,
    entity_kind: Option<EntityKind>,
) -> Vec<u64> {
    world
        .entities
        .values()
        .filter(|e| {
            e.is_alive()
                && entity_kind.as_ref().is_none_or(|k| e.kind == *k)
                && e.has_active_rel(rel_kind.clone(), target)
        })
        .map(|e| e.id)
        .collect()
}

/// Find the faction a settlement currently belongs to (active MemberOf relationship).
pub fn settlement_owner(world: &World, settlement: u64) -> Option<u64> {
    crate::sim::helpers::settlement_faction(world, settlement)
}

/// Find the current leader of a faction (active LeaderOf relationship).
pub fn faction_leader(world: &World, faction: u64) -> Option<u64> {
    crate::sim::helpers::faction_leader(world, faction)
}

/// Get all living settlements belonging to a faction.
pub fn faction_settlements(world: &World, faction: u64) -> Vec<u64> {
    crate::sim::helpers::faction_settlements(world, faction)
}

/// Get an entity's extra value as f64, returning 0.0 if not found.
pub fn extra_f64(world: &World, id: u64, key: &str) -> f64 {
    world
        .entities
        .get(&id)
        .map(|e| e.extra_f64_or(key, 0.0))
        .unwrap_or(0.0)
}

/// Get an entity's extra value as bool, returning false if not found.
pub fn extra_bool(world: &World, id: u64, key: &str) -> bool {
    world
        .entities
        .get(&id)
        .map(|e| e.extra_bool(key))
        .unwrap_or(false)
}

/// Get an entity's extra value as a string slice.
pub fn extra_str<'a>(world: &'a World, id: u64, key: &str) -> Option<&'a str> {
    world.entities.get(&id).and_then(|e| e.extra_str(key))
}

/// Check if an entity has a given extra key.
pub fn has_extra(world: &World, id: u64, key: &str) -> bool {
    world
        .entities
        .get(&id)
        .is_some_and(|e| e.extra.contains_key(key))
}

// ---------------------------------------------------------------------------
// Entity liveness helpers
// ---------------------------------------------------------------------------

/// Check if an entity is alive (exists and has no end timestamp).
pub fn is_alive(world: &World, id: u64) -> bool {
    world.entities.get(&id).is_some_and(|e| e.is_alive())
}

/// Get all living entity IDs of a given kind.
pub fn living_entities(world: &World, kind: &EntityKind) -> Vec<u64> {
    world.living_entities(kind)
}

/// Count living entities of a given kind.
pub fn count_living(world: &World, kind: &EntityKind) -> usize {
    world.count_living(kind)
}

// ---------------------------------------------------------------------------
// Relationship query helpers
// ---------------------------------------------------------------------------

/// Check if an entity has an active relationship of a given kind to a target.
pub fn has_relationship(world: &World, source: u64, kind: &RelationshipKind, target: u64) -> bool {
    world.entities.get(&source).is_some_and(|e| {
        e.relationships
            .iter()
            .any(|r| r.kind == *kind && r.target_entity_id == target && r.is_active())
    })
}

/// Get all active relationship targets of a given kind from source.
pub fn relationship_targets(world: &World, source: u64, kind: &RelationshipKind) -> Vec<u64> {
    world
        .entities
        .get(&source)
        .map(|e| {
            e.relationships
                .iter()
                .filter(|r| r.kind == *kind && r.is_active())
                .map(|r| r.target_entity_id)
                .collect()
        })
        .unwrap_or_default()
}

/// Get all living people located in a settlement (via LocatedIn relationship).
pub fn people_in_settlement(world: &World, settlement: u64) -> Vec<u64> {
    related_living(
        world,
        settlement,
        RelationshipKind::LocatedIn,
        Some(EntityKind::Person),
    )
}

/// Get all living armies located in a region (via LocatedIn relationship).
pub fn armies_in_region(world: &World, region: u64) -> Vec<u64> {
    related_living(
        world,
        region,
        RelationshipKind::LocatedIn,
        Some(EntityKind::Army),
    )
}

// ---------------------------------------------------------------------------
// Event query helpers
// ---------------------------------------------------------------------------

/// Count events of a given kind.
pub fn count_events(world: &World, kind: &EventKind) -> usize {
    world.events.values().filter(|e| e.kind == *kind).count()
}

/// Find all events of a given kind.
pub fn events_of_kind<'a>(world: &'a World, kind: &EventKind) -> Vec<&'a Event> {
    world.events.values().filter(|e| e.kind == *kind).collect()
}

/// Find all events where an entity participated (any role).
pub fn events_involving(world: &World, entity: u64) -> Vec<&Event> {
    let event_ids: std::collections::HashSet<u64> = world
        .event_participants
        .iter()
        .filter(|p| p.entity_id == entity)
        .map(|p| p.event_id)
        .collect();
    world
        .events
        .values()
        .filter(|e| event_ids.contains(&e.id))
        .collect()
}

/// Find all events where an entity participated with a specific role.
pub fn events_with_role<'a>(
    world: &'a World,
    entity: u64,
    role: &ParticipantRole,
) -> Vec<&'a Event> {
    let event_ids: std::collections::HashSet<u64> = world
        .event_participants
        .iter()
        .filter(|p| p.entity_id == entity && p.role == *role)
        .map(|p| p.event_id)
        .collect();
    world
        .events
        .values()
        .filter(|e| event_ids.contains(&e.id))
        .collect()
}

// ---------------------------------------------------------------------------
// Signal helpers
// ---------------------------------------------------------------------------

/// Check if any signal matches the predicate.
pub fn has_signal(signals: &[Signal], predicate: impl Fn(&SignalKind) -> bool) -> bool {
    signals.iter().any(|s| predicate(&s.kind))
}

/// Count signals matching the predicate.
pub fn count_signals(signals: &[Signal], predicate: impl Fn(&SignalKind) -> bool) -> usize {
    signals.iter().filter(|s| predicate(&s.kind)).count()
}

// ---------------------------------------------------------------------------
// Assertion helpers
// ---------------------------------------------------------------------------

/// Assert a float is approximately equal, with a named context message.
pub fn assert_approx(actual: f64, expected: f64, tolerance: f64, msg: &str) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{msg}: expected ~{expected} (+-{tolerance}), got {actual}"
    );
}

/// Assert two worlds produced from the same seed are structurally identical.
/// Checks entity count, event count, event_participants count, and action_results count.
pub fn assert_deterministic(world1: &World, world2: &World) {
    assert_eq!(
        world1.entities.len(),
        world2.entities.len(),
        "entity count mismatch: {} vs {}",
        world1.entities.len(),
        world2.entities.len()
    );
    assert_eq!(
        world1.events.len(),
        world2.events.len(),
        "event count mismatch: {} vs {}",
        world1.events.len(),
        world2.events.len()
    );
    assert_eq!(
        world1.event_participants.len(),
        world2.event_participants.len(),
        "event_participants count mismatch: {} vs {}",
        world1.event_participants.len(),
        world2.event_participants.len()
    );
    assert_eq!(
        world1.action_results.len(),
        world2.action_results.len(),
        "action_results count mismatch: {} vs {}",
        world1.action_results.len(),
        world2.action_results.len()
    );

    // Compare entity kind distributions for stronger determinism check
    let kind_counts = |world: &World| -> std::collections::BTreeMap<EntityKind, usize> {
        let mut counts = std::collections::BTreeMap::new();
        for e in world.entities.values() {
            *counts.entry(e.kind).or_insert(0) += 1;
        }
        counts
    };
    let counts1 = kind_counts(world1);
    let counts2 = kind_counts(world2);
    assert_eq!(
        counts1, counts2,
        "entity kind distribution mismatch:\n  run1: {:?}\n  run2: {:?}",
        counts1, counts2
    );
}

/// Assert that an entity is alive (exists and has no end timestamp).
pub fn assert_alive(world: &World, id: u64) {
    let entity = world
        .entities
        .get(&id)
        .unwrap_or_else(|| panic!("assert_alive: entity {id} not found"));
    assert!(
        entity.end.is_none(),
        "assert_alive: entity {id} ({}) is dead (ended at {:?})",
        entity.name,
        entity.end
    );
}

/// Assert that an entity is dead/ended.
pub fn assert_dead(world: &World, id: u64) {
    let entity = world
        .entities
        .get(&id)
        .unwrap_or_else(|| panic!("assert_dead: entity {id} not found"));
    assert!(
        entity.end.is_some(),
        "assert_dead: entity {id} ({}) is still alive",
        entity.name
    );
}

/// Assert that an active relationship exists from source to target.
pub fn assert_related(world: &World, source: u64, kind: &RelationshipKind, target: u64) {
    assert!(
        has_relationship(world, source, kind, target),
        "assert_related: no active {:?} from {source} to {target}",
        kind
    );
}

/// Assert that at least one event of the given kind exists.
pub fn assert_event_exists(world: &World, kind: &EventKind) {
    assert!(
        world.events.values().any(|e| e.kind == *kind),
        "assert_event_exists: no event of kind {:?} found ({} total events)",
        kind,
        world.events.len()
    );
}

/// Assert that an event of the given kind exists with a specific entity+role participation.
pub fn assert_event_with_participant(
    world: &World,
    kind: &EventKind,
    entity: u64,
    role: &ParticipantRole,
) {
    let matching_events: Vec<u64> = world
        .events
        .values()
        .filter(|e| e.kind == *kind)
        .map(|e| e.id)
        .collect();
    assert!(
        !matching_events.is_empty(),
        "assert_event_with_participant: no events of kind {:?}",
        kind
    );
    let has_participation = world
        .event_participants
        .iter()
        .any(|p| matching_events.contains(&p.event_id) && p.entity_id == entity && p.role == *role);
    assert!(
        has_participation,
        "assert_event_with_participant: entity {entity} not found as {:?} in any {:?} event",
        role, kind
    );
}

// ---------------------------------------------------------------------------
// Event effect helpers
// ---------------------------------------------------------------------------

/// Get all PropertyChanged effects for a given entity and field name.
pub fn property_changes<'a>(world: &'a World, entity_id: u64, field: &str) -> Vec<&'a EventEffect> {
    world
        .event_effects
        .iter()
        .filter(|e| {
            e.entity_id == entity_id
                && matches!(
                    &e.effect,
                    StateChange::PropertyChanged { field: f, .. } if f == field
                )
        })
        .collect()
}

/// Assert that at least one PropertyChanged effect exists for the given entity and field.
pub fn assert_property_changed(world: &World, entity_id: u64, field: &str) {
    let changes = property_changes(world, entity_id, field);
    assert!(
        !changes.is_empty(),
        "assert_property_changed: no PropertyChanged effect for entity {entity_id}, field \"{field}\" \
         (total effects: {})",
        world.event_effects.len()
    );
}

// ---------------------------------------------------------------------------
// Composite scenarios
// ---------------------------------------------------------------------------

pub struct MigrationSetup {
    pub world: World,
    pub source: u64,
    pub dest: u64,
    pub faction: u64,
    pub region_a: u64,
    pub region_b: u64,
}

/// Two adjacent regions, one faction, two settlements. Useful for migration tests.
pub fn migration_scenario() -> MigrationSetup {
    let mut s = Scenario::new();
    let region_a = s.add_region("RegionA");
    let region_b = s.add_region("RegionB");
    s.make_adjacent(region_a, region_b);

    let faction = s.add_faction("TestFaction");

    let source = s
        .settlement("SourceTown", faction, region_a)
        .population(500)
        .id();
    let dest = s
        .settlement("DestTown", faction, region_b)
        .population(300)
        .prosperity(0.6)
        .id();

    MigrationSetup {
        world: s.build(),
        source,
        dest,
        faction,
        region_a,
        region_b,
    }
}

pub struct WarSetup {
    pub world: World,
    pub army: u64,
    pub target_settlement: u64,
    pub attacker_faction: u64,
    pub defender_faction: u64,
    pub attacker_region: u64,
    pub defender_region: u64,
}

/// Two factions at war: 2 adjacent regions, 2 factions (at war), 1 settlement each,
/// 1 army belonging to attacker stationed in the defender's region.
pub fn war_scenario(fort_level: u8, army_strength: u32) -> WarSetup {
    let mut s = Scenario::at_year(10);
    let region_a = s.add_region("Attacker Region");
    let region_b = s.add_region("Defender Region");
    s.make_adjacent(region_a, region_b);

    let attacker = s.add_faction("Attacker");
    let defender = s.add_faction("Defender");
    s.make_at_war(attacker, defender);

    s.settlement("Attacker Town", attacker, region_a)
        .population(1000)
        .id();

    let target = s
        .settlement("Target Town", defender, region_b)
        .population(500)
        .fortification_level(fort_level)
        .id();

    let army = s.add_army("Attacker Army", attacker, region_b, army_strength);

    WarSetup {
        world: s.build(),
        army,
        target_settlement: target,
        attacker_faction: attacker,
        defender_faction: defender,
        attacker_region: region_a,
        defender_region: region_b,
    }
}

pub struct EconomicSetup {
    pub world: World,
    pub settlement: u64,
    pub faction: u64,
    pub region: u64,
}

/// Single faction with one settlement. Useful for economy/building tests.
pub fn economic_scenario(population: u32, treasury: f64) -> EconomicSetup {
    let mut s = Scenario::at_year(100);
    let region = s.add_region("Plains");
    let faction = s.faction("Kingdom").treasury(treasury).id();
    let settlement = s
        .settlement("Market Town", faction, region)
        .population(population)
        .prosperity(0.7)
        .id();
    EconomicSetup {
        world: s.build(),
        settlement,
        faction,
        region,
    }
}

/// Minimal world with a player-actor person (no faction). Useful for action system tests.
pub fn action_scenario() -> (World, u64) {
    let mut s = Scenario::at_year(100);
    let actor = s.add_person_standalone("Dorian");
    s.make_player(actor);
    (s.build(), actor)
}

pub struct MercenarySetup {
    pub world: World,
    pub attacker_faction: u64,
    pub defender_faction: u64,
    pub merc_faction: u64,
    pub merc_army: u64,
    pub merc_leader: u64,
    pub attacker_region: u64,
    pub defender_region: u64,
    pub merc_region: u64,
}

/// Two factions at war, with a mercenary company hired by the attacker.
/// Attacker is wealthy but has small population; defender is larger but poorer.
pub fn mercenary_scenario() -> MercenarySetup {
    use crate::scenario::MercenarySetup as MercSetup;

    let mut s = Scenario::at_year(10);
    let region_a = s.add_region("Attacker Region");
    let region_b = s.add_region("Defender Region");
    let region_m = s.add_region("Merc Region");
    s.make_adjacent(region_a, region_b);
    s.make_adjacent(region_a, region_m);
    s.make_adjacent(region_b, region_m);

    let attacker = s
        .faction("Attacker")
        .treasury(500.0)
        .stability(0.7)
        .happiness(0.6)
        .legitimacy(0.7)
        .id();
    let defender = s
        .faction("Defender")
        .treasury(50.0)
        .stability(0.6)
        .happiness(0.5)
        .legitimacy(0.6)
        .id();

    s.settlement("Attacker Town", attacker, region_a)
        .population(200)
        .id();
    s.settlement("Defender Town", defender, region_b)
        .population(500)
        .id();

    // Attacker has a leader
    let atk_leader = s
        .person("Attacker Leader", attacker)
        .role(crate::model::entity_data::Role::Warrior)
        .id();
    s.make_leader(atk_leader, attacker);

    // Defender has a leader
    let def_leader = s
        .person("Defender Leader", defender)
        .role(crate::model::entity_data::Role::Warrior)
        .id();
    s.make_leader(def_leader, defender);

    // Create mercenary company and hire for the attacker
    let MercSetup {
        faction: merc_faction,
        army: merc_army,
        leader: merc_leader,
    } = s.add_mercenary_company("Iron Hawks", region_m, 80);
    s.hire_mercenary(merc_faction, attacker);

    // Set initial loyalty of mercs toward employer
    s.modify_faction(merc_faction, |fd| {
        fd.loyalty.insert(attacker, 0.7);
    });

    s.make_at_war(attacker, defender);
    s.make_enemies(attacker, defender);

    MercenarySetup {
        world: s.build(),
        attacker_faction: attacker,
        defender_faction: defender,
        merc_faction,
        merc_army,
        merc_leader,
        attacker_region: region_a,
        defender_region: region_b,
        merc_region: region_m,
    }
}

pub struct PoliticalSetup {
    pub world: World,
    pub faction: u64,
    pub leader: u64,
    pub settlement: u64,
}

/// Faction with a leader and one settlement. Useful for politics/reputation tests.
pub fn political_scenario() -> PoliticalSetup {
    let mut s = Scenario::at_year(100);
    let region = s.add_region("Heartland");
    let faction = s
        .faction("Kingdom")
        .stability(0.7)
        .happiness(0.7)
        .legitimacy(0.8)
        .id();
    let settlement = s
        .settlement("Capital", faction, region)
        .population(500)
        .prosperity(0.6)
        .id();
    let leader = s
        .person("King", faction)
        .role(Role::Warrior)
        .traits(vec![Trait::Ambitious, Trait::Charismatic])
        .id();
    s.make_leader(leader, faction);
    PoliticalSetup {
        world: s.build(),
        faction,
        leader,
        settlement,
    }
}
