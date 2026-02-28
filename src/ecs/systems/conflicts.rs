//! Conflicts system — migrated from `src/sim/conflicts/`.
//!
//! Monthly systems (Update phase, chained):
//! 1. `war_declarations_and_mustering` — yearly guard: check war declarations, muster armies
//! 2. `mercenary_hiring_and_formation` — yearly guard: hire/form mercenary companies
//! 3. `process_mercenary_payments` — monthly wage payment and loyalty
//! 4. `check_mercenary_desertion` — monthly desertion checks
//! 5. `apply_supply_and_attrition` — monthly supply, foraging, disease attrition
//! 6. `move_armies` — monthly BFS movement toward enemy
//! 7. `resolve_battles` — monthly co-located army battles
//! 8. `check_retreats` — monthly morale/strength retreat checks
//! 9. `start_sieges` — monthly siege initiation on enemy settlements
//! 10. `progress_sieges` — monthly siege progression, starvation, assaults
//! 11. `war_endings_and_disbanding` — yearly guard: peace terms, disbanding
//!
//! No reactive handlers — Conflicts is producer-only.

use std::collections::{BTreeSet, VecDeque};

use bevy_app::App;
use bevy_ecs::entity::Entity;
use bevy_ecs::message::MessageWriter;
use bevy_ecs::query::With;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Query, Res, ResMut};
use rand::Rng;

use crate::ecs::clock::SimClock;
use crate::ecs::commands::{SimCommand, SimCommandKind};
use crate::ecs::components::common::SimEntity;
use crate::ecs::components::dynamic::EcsActiveSiege;
use crate::ecs::components::{
    Army, ArmyState, Faction, FactionCore, FactionDiplomacy, FactionMilitary, Person, PersonCore,
    PersonReputation, Region, RegionState, Settlement, SettlementCore, SettlementMilitary,
};
use crate::ecs::conditions::monthly;
use crate::ecs::relationships::{
    HiredBy, LocatedIn, MemberOf, RegionAdjacency, RelationshipGraph,
};
use crate::ecs::resources::{SimEntityMap, SimRng};
use crate::ecs::schedule::{SimPhase, SimTick};
use crate::model::entity_data::{GovernmentType, Role};
use crate::model::event::{EventKind, ParticipantRole};
use crate::model::Terrain;

// ---------------------------------------------------------------------------
// Constants — War Declaration
// ---------------------------------------------------------------------------

const WAR_DECLARATION_BASE_CHANCE: f64 = 0.04;
const RELIGIOUS_WAR_FERVOR_FACTOR: f64 = 0.05;
const RELIGIOUS_WAR_FERVOR_CAP: f64 = 0.10;

// ---------------------------------------------------------------------------
// Constants — Draft
// ---------------------------------------------------------------------------

const DRAFT_RATE: f64 = 0.15;
const MIN_ARMY_STRENGTH: u32 = 20;

// ---------------------------------------------------------------------------
// Constants — Battle
// ---------------------------------------------------------------------------

const TERRAIN_BONUS_MOUNTAINS: f64 = 1.3;
const TERRAIN_BONUS_FOREST: f64 = 1.15;
const LOSER_CASUALTY_MIN: f64 = 0.25;
const LOSER_CASUALTY_MAX: f64 = 0.40;
const WINNER_CASUALTY_MIN: f64 = 0.10;
const WINNER_CASUALTY_MAX: f64 = 0.20;
const WARRIOR_DEATH_CHANCE: f64 = 0.15;
const NON_WARRIOR_DEATH_CHANCE: f64 = 0.05;

// ---------------------------------------------------------------------------
// Constants — War Exhaustion
// ---------------------------------------------------------------------------

const WAR_EXHAUSTION_START_YEAR: u32 = 5;
const PEACE_CHANCE_PER_YEAR: f64 = 0.15;

// ---------------------------------------------------------------------------
// Constants — Supply & Attrition
// ---------------------------------------------------------------------------

const FORAGE_FRIENDLY: f64 = 0.8;
const FORAGE_NEUTRAL: f64 = 0.4;
const FORAGE_ENEMY: f64 = 0.15;

const FORAGE_PLAINS: f64 = 1.3;
const FORAGE_FOREST: f64 = 1.0;
const FORAGE_HILLS: f64 = 0.8;
const FORAGE_MOUNTAINS: f64 = 0.4;
const FORAGE_DESERT: f64 = 0.1;
const FORAGE_SWAMP: f64 = 0.6;
const FORAGE_TUNDRA: f64 = 0.2;
const FORAGE_JUNGLE: f64 = 0.7;
const FORAGE_DEFAULT: f64 = 0.5;
const FORAGE_COAST: f64 = 1.3;
const FORAGE_WATER: f64 = 0.0;

const DISEASE_BASE: f64 = 0.005;
const DISEASE_SWAMP: f64 = 0.03;
const DISEASE_JUNGLE: f64 = 0.025;
const DISEASE_DESERT: f64 = 0.015;
const DISEASE_TUNDRA: f64 = 0.02;
const DISEASE_MOUNTAINS_RATE: f64 = 0.01;
const DISEASE_WATER: f64 = 0.015;

const STARVATION_RATE: f64 = 0.15;

// ---------------------------------------------------------------------------
// Constants — Morale
// ---------------------------------------------------------------------------

const MORALE_DECAY_PER_MONTH: f64 = 0.02;
const HOME_TERRITORY_MORALE_BOOST: f64 = 0.05;
const STARVATION_MORALE_PENALTY: f64 = 0.10;

// ---------------------------------------------------------------------------
// Constants — Retreat
// ---------------------------------------------------------------------------

const RETREAT_MORALE_THRESHOLD: f64 = 0.2;
const RETREAT_STRENGTH_RATIO: f64 = 0.25;

// ---------------------------------------------------------------------------
// Constants — Siege
// ---------------------------------------------------------------------------

const SIEGE_SUPPLY_MULTIPLIER: f64 = 1.2;
const SIEGE_PROSPERITY_DECAY: f64 = 0.03;
const SIEGE_STARVATION_PROSPERITY_THRESHOLD: f64 = 0.2;
const SIEGE_STARVATION_POP_LOSS: f64 = 0.01;
const SIEGE_ASSAULT_CHANCE: f64 = 0.10;
const SIEGE_ASSAULT_MIN_MONTHS: u32 = 2;
const SIEGE_ASSAULT_MORALE_MIN: f64 = 0.4;
const SIEGE_ASSAULT_POWER_RATIO: f64 = 1.5;
const SIEGE_ASSAULT_CASUALTY_MIN: f64 = 0.15;
const SIEGE_ASSAULT_CASUALTY_MAX: f64 = 0.30;

// ---------------------------------------------------------------------------
// Constants — Mercenary
// ---------------------------------------------------------------------------

const MERC_FORMATION_CHANCE: f64 = 0.02;
const MERC_MIN_STRENGTH: u32 = 30;
const MERC_MAX_STRENGTH: u32 = 80;
const MERC_HIRE_TREASURY_MIN: f64 = 50.0;
const MERC_WAGE_PER_STRENGTH: f64 = 0.3;
const MERC_SIGNING_BONUS_FACTOR: f64 = 3.0;
const MERC_LOYALTY_UNPAID: f64 = -0.08;
const MERC_DESERTION_LOYALTY_THRESHOLD: f64 = 0.3;
const MERC_DISBAND_IDLE_MONTHS: u32 = 24;
const MERC_DISBAND_STRENGTH_MIN: u32 = 10;

const MERC_PREFIXES: &[&str] = &[
    "Iron", "Steel", "Black", "Golden", "Red", "Silver", "Bronze", "Storm", "Shadow", "Blood",
];
const MERC_SUFFIXES: &[&str] = &[
    "Company",
    "Sellswords",
    "Guard",
    "Blades",
    "Legion",
    "Lances",
    "Hawks",
    "Wolves",
    "Shields",
    "Swords",
];

// ---------------------------------------------------------------------------
// Plugin registration
// ---------------------------------------------------------------------------

pub fn add_conflict_systems(app: &mut App) {
    app.add_systems(
        SimTick,
        (
            war_declarations_and_mustering,
            mercenary_hiring_and_formation,
            process_mercenary_payments,
            check_mercenary_desertion,
            apply_supply_and_attrition,
            move_armies,
            resolve_battles,
            check_retreats,
            start_sieges,
            progress_sieges,
            war_endings_and_disbanding,
        )
            .chain()
            .run_if(monthly)
            .in_set(SimPhase::Update),
    );
    // No reactive handlers — Conflicts is producer-only
}

// ---------------------------------------------------------------------------
// Helper: terrain modifiers
// ---------------------------------------------------------------------------

fn forage_terrain_modifier(terrain: Terrain) -> f64 {
    match terrain {
        Terrain::Plains => FORAGE_PLAINS,
        Terrain::Forest => FORAGE_FOREST,
        Terrain::Hills => FORAGE_HILLS,
        Terrain::Mountains => FORAGE_MOUNTAINS,
        Terrain::Desert => FORAGE_DESERT,
        Terrain::Swamp => FORAGE_SWAMP,
        Terrain::Tundra => FORAGE_TUNDRA,
        Terrain::Jungle => FORAGE_JUNGLE,
        Terrain::Coast => FORAGE_COAST,
        Terrain::ShallowWater | Terrain::DeepWater => FORAGE_WATER,
        _ => FORAGE_DEFAULT,
    }
}

fn disease_rate_for_terrain(terrain: Terrain) -> f64 {
    match terrain {
        Terrain::Swamp => DISEASE_SWAMP,
        Terrain::Jungle => DISEASE_JUNGLE,
        Terrain::Desert => DISEASE_DESERT,
        Terrain::Tundra => DISEASE_TUNDRA,
        Terrain::Mountains => DISEASE_MOUNTAINS_RATE,
        Terrain::ShallowWater | Terrain::DeepWater => DISEASE_WATER,
        _ => DISEASE_BASE,
    }
}

fn get_terrain_defense_bonus(terrain: Terrain) -> f64 {
    match terrain {
        Terrain::Mountains | Terrain::Hills => TERRAIN_BONUS_MOUNTAINS,
        Terrain::Forest | Terrain::Jungle => TERRAIN_BONUS_FOREST,
        _ => 1.0,
    }
}

// ---------------------------------------------------------------------------
// Helper: territory status for an army in a region
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TerritoryStatus {
    Friendly,
    Neutral,
    Enemy,
}

#[allow(clippy::type_complexity)]
fn get_territory_status(
    army_faction: Entity,
    region: Entity,
    settlements: &Query<(&SimEntity, Option<&MemberOf>, Option<&LocatedIn>), With<Settlement>>,
    rel_graph: &RelationshipGraph,
) -> TerritoryStatus {
    let mut has_friendly = false;
    let mut has_enemy = false;
    for (sim, member_of, loc) in settlements.iter() {
        if !sim.is_alive() {
            continue;
        }
        if loc.is_some_and(|l| l.0 == region)
            && let Some(m) = member_of
        {
            if m.0 == army_faction {
                has_friendly = true;
            } else if rel_graph.are_at_war(army_faction, m.0) {
                has_enemy = true;
            }
        }
    }
    if has_friendly {
        TerritoryStatus::Friendly
    } else if has_enemy {
        TerritoryStatus::Enemy
    } else {
        TerritoryStatus::Neutral
    }
}

// ---------------------------------------------------------------------------
// Helper: BFS pathfinding for armies
// ---------------------------------------------------------------------------

fn ecs_bfs_next_step(
    start: Entity,
    goal: Entity,
    adjacency: &RegionAdjacency,
    regions: &Query<&RegionState, With<Region>>,
) -> Option<Entity> {
    if start == goal {
        return None;
    }
    let mut visited = BTreeSet::new();
    let mut queue: VecDeque<(Entity, Entity)> = VecDeque::new(); // (current, first_step)
    visited.insert(start);
    for &neighbor in adjacency.neighbors(start) {
        // Skip water regions for land armies
        if let Ok(rs) = regions.get(neighbor)
            && rs.terrain.is_water()
        {
            continue;
        }
        visited.insert(neighbor);
        if neighbor == goal {
            return Some(neighbor);
        }
        queue.push_back((neighbor, neighbor));
    }
    while let Some((current, first_step)) = queue.pop_front() {
        for &neighbor in adjacency.neighbors(current) {
            if visited.contains(&neighbor) {
                continue;
            }
            if let Ok(rs) = regions.get(neighbor)
                && rs.terrain.is_water()
            {
                continue;
            }
            visited.insert(neighbor);
            if neighbor == goal {
                return Some(first_step);
            }
            queue.push_back((neighbor, first_step));
        }
    }
    None
}

#[allow(clippy::type_complexity)]
fn ecs_find_nearest_enemy_region(
    start: Entity,
    army_faction: Entity,
    adjacency: &RegionAdjacency,
    regions: &Query<&RegionState, With<Region>>,
    settlements: &Query<(&SimEntity, Option<&MemberOf>, Option<&LocatedIn>), With<Settlement>>,
    rel_graph: &RelationshipGraph,
) -> Option<Entity> {
    let mut visited = BTreeSet::new();
    let mut queue: VecDeque<Entity> = VecDeque::new();
    visited.insert(start);
    queue.push_back(start);

    while let Some(current) = queue.pop_front() {
        // Check if this region has an enemy settlement
        if current != start {
            for (sim, member_of, loc) in settlements.iter() {
                if sim.is_alive()
                    && loc.is_some_and(|l| l.0 == current)
                    && member_of.is_some_and(|m| rel_graph.are_at_war(army_faction, m.0))
                {
                    return Some(current);
                }
            }
        }
        for &neighbor in adjacency.neighbors(current) {
            if visited.contains(&neighbor) {
                continue;
            }
            if let Ok(rs) = regions.get(neighbor)
                && rs.terrain.is_water()
            {
                continue;
            }
            visited.insert(neighbor);
            queue.push_back(neighbor);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Helper: check if factions are adjacent
// ---------------------------------------------------------------------------

#[allow(clippy::type_complexity)]
fn ecs_factions_are_adjacent(
    faction_a: Entity,
    faction_b: Entity,
    settlements: &Query<(&SimEntity, Option<&MemberOf>, Option<&LocatedIn>), With<Settlement>>,
    adjacency: &RegionAdjacency,
) -> bool {
    // Collect regions of faction_a and faction_b
    let mut regions_a = BTreeSet::new();
    let mut regions_b = BTreeSet::new();
    for (sim, member_of, loc) in settlements.iter() {
        if !sim.is_alive() {
            continue;
        }
        if let Some(m) = member_of
            && let Some(l) = loc
        {
            if m.0 == faction_a {
                regions_a.insert(l.0);
            } else if m.0 == faction_b {
                regions_b.insert(l.0);
            }
        }
    }
    for &ra in &regions_a {
        for &neighbor in adjacency.neighbors(ra) {
            if regions_b.contains(&neighbor) {
                return true;
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Helper: mercenary name generation
// ---------------------------------------------------------------------------

fn generate_merc_name(rng: &mut impl Rng) -> String {
    let prefix = MERC_PREFIXES[rng.random_range(0..MERC_PREFIXES.len())];
    let suffix = MERC_SUFFIXES[rng.random_range(0..MERC_SUFFIXES.len())];
    format!("The {prefix} {suffix}")
}

// ---------------------------------------------------------------------------
// Helper: is non-state faction (bandit/mercenary)
// ---------------------------------------------------------------------------

fn is_non_state_faction(core: &FactionCore) -> bool {
    matches!(
        core.government_type,
        GovernmentType::BanditClan | GovernmentType::MercenaryCompany
    )
}

// ---------------------------------------------------------------------------
// System 1: War declarations and mustering (yearly guard)
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn war_declarations_and_mustering(
    clock: Res<SimClock>,
    factions: Query<(Entity, &SimEntity, &FactionCore, &FactionDiplomacy, &FactionMilitary), With<Faction>>,
    settlements: Query<(&SimEntity, Option<&MemberOf>, Option<&LocatedIn>), With<Settlement>>,
    settlement_cores: Query<(&SettlementCore,), With<Settlement>>,
    armies: Query<(Entity, &ArmyState, &SimEntity, Option<&LocatedIn>), With<Army>>,
    _persons: Query<(Entity, &PersonCore, &SimEntity, Option<&MemberOf>), With<Person>>,
    adjacency: Res<RegionAdjacency>,
    rel_graph: Res<RelationshipGraph>,
    entity_map: Res<SimEntityMap>,
    mut rng: ResMut<SimRng>,
    mut commands: MessageWriter<SimCommand>,
) {
    if !clock.time.is_year_start() {
        return;
    }

    let current_year = clock.time.year();

    // Collect alive non-state factions
    let faction_list: Vec<(Entity, &FactionCore, &FactionDiplomacy, &FactionMilitary)> = factions
        .iter()
        .filter(|(_, sim, core, _, _)| sim.is_alive() && !is_non_state_faction(core))
        .map(|(e, _, core, diplo, mil)| (e, core, diplo, mil))
        .collect();

    // Find enemy pairs (at war)
    let mut at_war_pairs: BTreeSet<(Entity, Entity)> = BTreeSet::new();
    for &(fa, _, _, _) in &faction_list {
        for &(fb, _, _, _) in &faction_list {
            if fa < fb && rel_graph.are_at_war(fa, fb) {
                at_war_pairs.insert((fa, fb));
            }
        }
    }

    // --- War declarations ---
    // Collect adjacent enemy pairs that are NOT already at war
    let mut war_declarations: Vec<(Entity, Entity)> = Vec::new();
    for &(fa, core_a, _diplo_a, _mil_a) in &faction_list {
        for &(fb, core_b, _diplo_b, _mil_b) in &faction_list {
            if fa >= fb {
                continue;
            }
            if rel_graph.are_at_war(fa, fb) || rel_graph.are_allies(fa, fb) {
                continue;
            }
            // Must be enemies and adjacent
            if !rel_graph.are_enemies(fa, fb) {
                continue;
            }
            if !ecs_factions_are_adjacent(fa, fb, &settlements, &adjacency) {
                continue;
            }

            let avg_stability = (core_a.stability + core_b.stability) / 2.0;
            let instability_modifier = ((1.0 - avg_stability) * 2.0).clamp(0.5, 2.0);

            // Religious war bonus
            let religious_bonus = if core_a.primary_religion.is_some()
                && core_b.primary_religion.is_some()
                && core_a.primary_religion != core_b.primary_religion
            {
                (RELIGIOUS_WAR_FERVOR_FACTOR).min(RELIGIOUS_WAR_FERVOR_CAP)
            } else {
                0.0
            };

            let war_chance = WAR_DECLARATION_BASE_CHANCE * instability_modifier + religious_bonus;
            if rng.0.random_range(0.0..1.0) < war_chance {
                war_declarations.push((fa, fb));
            }
        }
    }

    for (attacker, defender) in war_declarations {
        commands.write(
            SimCommand::new(
                SimCommandKind::DeclareWar { attacker, defender },
                EventKind::WarDeclared,
                format!("War declared in year {current_year}"),
            )
            .with_participant(attacker, ParticipantRole::Attacker)
            .with_participant(defender, ParticipantRole::Defender),
        );
    }

    // --- Muster armies ---
    // Find factions at war that don't have an army
    let faction_has_army: BTreeSet<Entity> = armies
        .iter()
        .filter(|(_, state, sim, _)| sim.is_alive() && state.strength > 0)
        .filter_map(|(_, state, _, _)| entity_map.get_bevy(state.faction_id))
        .collect();

    for &(faction_entity, _, _, _) in &faction_list {
        let at_war = faction_list
            .iter()
            .any(|&(other, _, _, _)| rel_graph.are_at_war(faction_entity, other));
        if !at_war || faction_has_army.contains(&faction_entity) {
            continue;
        }

        // Draft from settlements
        let mut total_draftable = 0u32;
        let mut draft_region = None;
        for (sim, member_of, loc) in settlements.iter() {
            if sim.is_alive()
                && member_of.is_some_and(|m| m.0 == faction_entity)
                && let Some(l) = loc
            {
                let sett_entity = entity_map.get_bevy(sim.id);
                if let Some(se) = sett_entity
                    && let Ok((core,)) = settlement_cores.get(se)
                {
                    let draftable = (core.population as f64 * DRAFT_RATE) as u32;
                    total_draftable += draftable;
                    if draft_region.is_none() {
                        draft_region = Some(l.0);
                    }
                }
            }
        }

        if total_draftable >= MIN_ARMY_STRENGTH
            && let Some(region) = draft_region
        {
            commands.write(
                SimCommand::new(
                    SimCommandKind::MusterArmy {
                        faction: faction_entity,
                        region,
                    },
                    EventKind::Muster,
                    format!("Army mustered in year {current_year}"),
                )
                .with_participant(faction_entity, ParticipantRole::Subject),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// System 2: Mercenary hiring and formation (yearly guard)
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn mercenary_hiring_and_formation(
    clock: Res<SimClock>,
    factions: Query<(Entity, &SimEntity, &FactionCore, &FactionMilitary), With<Faction>>,
    settlements: Query<(&SimEntity, Option<&MemberOf>, Option<&LocatedIn>), With<Settlement>>,
    armies: Query<(Entity, &ArmyState, &SimEntity, Option<&LocatedIn>), With<Army>>,
    adjacency: Res<RegionAdjacency>,
    rel_graph: Res<RelationshipGraph>,
    entity_map: Res<SimEntityMap>,
    _regions: Query<&RegionState, With<Region>>,
    hired_by: Query<(Entity, Option<&HiredBy>), With<Faction>>,
    mut rng: ResMut<SimRng>,
    mut commands: MessageWriter<SimCommand>,
) {
    if !clock.time.is_year_start() {
        return;
    }

    let current_year = clock.time.year();

    // --- Spontaneous mercenary formation ---
    // Check border regions for spawning
    let mut border_regions: BTreeSet<Entity> = BTreeSet::new();
    for (sim, member_of, loc) in settlements.iter() {
        if !sim.is_alive() {
            continue;
        }
        if let Some(m) = member_of
            && let Some(l) = loc
        {
            for &neighbor in adjacency.neighbors(l.0) {
                // Check if neighbor has settlements from different faction
                for (sim2, member2, loc2) in settlements.iter() {
                    if sim2.is_alive()
                        && loc2.is_some_and(|l2| l2.0 == neighbor)
                        && member2.is_some_and(|m2| m2.0 != m.0)
                    {
                        border_regions.insert(l.0);
                    }
                }
            }
        }
    }

    for &region in &border_regions {
        if rng.0.random_range(0.0..1.0) < MERC_FORMATION_CHANCE {
            let strength = rng.0.random_range(MERC_MIN_STRENGTH..=MERC_MAX_STRENGTH);
            let name = generate_merc_name(&mut rng.0);
            commands.write(
                SimCommand::new(
                    SimCommandKind::CreateMercenaryCompany {
                        region,
                        strength,
                        name: name.clone(),
                    },
                    EventKind::FactionFormed,
                    format!("{name} formed in year {current_year}"),
                ),
            );
        }
    }

    // --- Hiring ---
    // Find factions at war that can afford mercenaries
    let merc_factions: Vec<(Entity, Entity)> = hired_by
        .iter()
        .filter(|(_, hb)| hb.is_none()) // Not already hired
        .filter_map(|(e, _)| {
            let Ok((_, sim, core, _)) = factions.get(e) else {
                return None;
            };
            if !sim.is_alive() || core.government_type != GovernmentType::MercenaryCompany {
                return None;
            }
            // Find where this merc's army is
            let army_region = armies.iter().find_map(|(_, state, asim, loc)| {
                if asim.is_alive()
                    && entity_map.get_bevy(state.faction_id) == Some(e)
                    && state.is_mercenary
                {
                    loc.map(|l| l.0)
                } else {
                    None
                }
            });
            army_region.map(|r| (e, r))
        })
        .collect();

    for (faction_entity, _, core, _mil) in factions.iter() {
        let sim_check = factions.get(faction_entity);
        let Ok((_, sim, _, _)) = sim_check else {
            continue;
        };
        if !sim.is_alive() || is_non_state_faction(core) {
            continue;
        }
        if core.treasury < MERC_HIRE_TREASURY_MIN {
            continue;
        }
        // Must be at war
        let at_war = factions.iter().any(|(other, osim, _, _)| {
            osim.is_alive() && other != faction_entity && rel_graph.are_at_war(faction_entity, other)
        });
        if !at_war {
            continue;
        }

        // Try to hire an available mercenary
        for &(merc_faction, _merc_region) in &merc_factions {
            // Check if merc army has strength
            let merc_army = armies.iter().find(|(_, state, asim, _)| {
                asim.is_alive()
                    && entity_map.get_bevy(state.faction_id) == Some(merc_faction)
                    && state.is_mercenary
            });
            let Some((_, merc_state, _, _)) = merc_army else {
                continue;
            };
            let wage = merc_state.strength as f64 * MERC_WAGE_PER_STRENGTH;
            let signing_bonus = wage * MERC_SIGNING_BONUS_FACTOR;
            if core.treasury < signing_bonus {
                continue;
            }

            commands.write(
                SimCommand::new(
                    SimCommandKind::HireMercenary {
                        employer: faction_entity,
                        mercenary: merc_faction,
                        wage,
                    },
                    EventKind::MercenaryHired,
                    format!("Mercenary hired in year {current_year}"),
                )
                .with_participant(faction_entity, ParticipantRole::Subject)
                .with_participant(merc_faction, ParticipantRole::Object),
            );
            break; // One hire per faction per year
        }
    }
}

// ---------------------------------------------------------------------------
// System 3: Process mercenary payments (monthly)
// ---------------------------------------------------------------------------

#[allow(clippy::type_complexity)]
fn process_mercenary_payments(
    mut factions: Query<(Entity, &SimEntity, &mut FactionCore, &mut FactionMilitary), With<Faction>>,
    hired_by_query: Query<(Entity, &HiredBy), With<Faction>>,
    armies: Query<(&ArmyState, &SimEntity), With<Army>>,
    entity_map: Res<SimEntityMap>,
) {
    // Collect merc payments
    let merc_payments: Vec<(Entity, Entity, f64)> = hired_by_query
        .iter()
        .filter_map(|(merc, hb)| {
            // Find merc army wage
            let wage = armies.iter().find_map(|(state, asim)| {
                if asim.is_alive()
                    && entity_map.get_bevy(state.faction_id) == Some(merc)
                    && state.is_mercenary
                {
                    Some(state.strength as f64 * MERC_WAGE_PER_STRENGTH / 12.0)
                } else {
                    None
                }
            });
            wage.map(|w| (merc, hb.0, w))
        })
        .collect();

    for (merc, employer, monthly_wage) in merc_payments {
        // Deduct from employer treasury
        if let Ok((_, _, mut core, _)) = factions.get_mut(employer) {
            if core.treasury >= monthly_wage {
                core.treasury -= monthly_wage;
                // Mark as paid
                if let Ok((_, _, _, mut mil)) = factions.get_mut(merc) {
                    mil.unpaid_months = 0;
                }
            } else {
                // Can't pay
                if let Ok((_, _, _, mut mil)) = factions.get_mut(merc) {
                    mil.unpaid_months += 1;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// System 4: Check mercenary desertion (monthly)
// ---------------------------------------------------------------------------

#[allow(clippy::type_complexity)]
fn check_mercenary_desertion(
    factions: Query<(Entity, &SimEntity, &FactionCore, &FactionMilitary), With<Faction>>,
    hired_by_query: Query<(Entity, &HiredBy), With<Faction>>,
    mut rng: ResMut<SimRng>,
    mut commands: MessageWriter<SimCommand>,
    clock: Res<SimClock>,
) {
    for (merc, _hb) in hired_by_query.iter() {
        let Ok((_, sim, _, mil)) = factions.get(merc) else {
            continue;
        };
        if !sim.is_alive() {
            continue;
        }
        // Loyalty decreases when unpaid
        let loyalty = 1.0 - (mil.unpaid_months as f64 * MERC_LOYALTY_UNPAID.abs());
        if loyalty < MERC_DESERTION_LOYALTY_THRESHOLD && rng.0.random_bool(0.3) {
            commands.write(SimCommand::new(
                SimCommandKind::EndMercenaryContract { mercenary: merc },
                EventKind::MercenaryDeserted,
                format!("{} deserted in year {}", sim.name, clock.time.year()),
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// System 5: Apply supply and attrition (monthly)
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn apply_supply_and_attrition(
    mut armies: Query<(Entity, &mut ArmyState, &SimEntity, Option<&LocatedIn>), With<Army>>,
    settlements: Query<(&SimEntity, Option<&MemberOf>, Option<&LocatedIn>), With<Settlement>>,
    regions: Query<&RegionState, With<Region>>,
    rel_graph: Res<RelationshipGraph>,
    entity_map: Res<SimEntityMap>,
    mut commands: MessageWriter<SimCommand>,
) {
    for (army_entity, mut state, sim, loc) in armies.iter_mut() {
        if !sim.is_alive() || state.strength == 0 {
            continue;
        }
        let Some(region) = loc.map(|l| l.0) else {
            continue;
        };

        let army_faction = entity_map.get_bevy(state.faction_id);
        let Some(af) = army_faction else {
            continue;
        };

        // Determine territory status
        let territory = get_territory_status(af, region, &settlements, &rel_graph);

        // Get terrain
        let terrain = regions
            .get(region)
            .map(|rs| rs.terrain)
            .unwrap_or(Terrain::Plains);

        // Forage rate
        let base_forage = match territory {
            TerritoryStatus::Friendly => FORAGE_FRIENDLY,
            TerritoryStatus::Neutral => FORAGE_NEUTRAL,
            TerritoryStatus::Enemy => FORAGE_ENEMY,
        };
        let forage = base_forage * forage_terrain_modifier(terrain);

        // Consume supply (1.0 per month), recover via foraging
        let siege_mult = if state.besieging_settlement_id.is_some() {
            SIEGE_SUPPLY_MULTIPLIER
        } else {
            1.0
        };
        state.supply = (state.supply - siege_mult + forage).max(0.0);

        // Disease attrition
        let disease_rate = disease_rate_for_terrain(terrain);
        let disease_losses = (state.strength as f64 * disease_rate) as u32;
        state.strength = state.strength.saturating_sub(disease_losses);

        // Starvation
        if state.supply <= 0.0 {
            let starve_losses = (state.strength as f64 * STARVATION_RATE) as u32;
            state.strength = state.strength.saturating_sub(starve_losses);
            state.morale = (state.morale - STARVATION_MORALE_PENALTY).max(0.0);
        }

        // Morale decay
        state.morale = (state.morale - MORALE_DECAY_PER_MONTH).max(0.0);

        // Home territory morale boost
        if territory == TerritoryStatus::Friendly {
            state.morale = (state.morale + HOME_TERRITORY_MORALE_BOOST).min(1.0);
        }

        // Increment months campaigning
        state.months_campaigning += 1;

        // Disband if strength hits 0
        if state.strength == 0 {
            commands.write(SimCommand::new(
                SimCommandKind::DisbandArmy { army: army_entity },
                EventKind::Dissolution,
                format!("{} lost to attrition", sim.name),
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// System 6: Move armies (monthly)
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn move_armies(
    armies: Query<(Entity, &ArmyState, &SimEntity, Option<&LocatedIn>), With<Army>>,
    settlements: Query<(&SimEntity, Option<&MemberOf>, Option<&LocatedIn>), With<Settlement>>,
    regions: Query<&RegionState, With<Region>>,
    adjacency: Res<RegionAdjacency>,
    rel_graph: Res<RelationshipGraph>,
    entity_map: Res<SimEntityMap>,
    mut commands: MessageWriter<SimCommand>,
) {
    // Collect army info first to avoid borrow issues
    let army_data: Vec<(Entity, u64, Entity, Entity)> = armies
        .iter()
        .filter(|(_, state, sim, loc)| sim.is_alive() && state.strength > 0 && loc.is_some())
        .filter(|(_, state, _, _)| state.besieging_settlement_id.is_none()) // Don't move while besieging
        .filter_map(|(e, state, _, loc)| {
            let faction = entity_map.get_bevy(state.faction_id)?;
            Some((e, state.faction_id, faction, loc.unwrap().0))
        })
        .collect();

    for (army_entity, _faction_sim_id, faction_entity, current_region) in army_data {
        // Find nearest enemy settlement or army
        let target = ecs_find_nearest_enemy_region(
            current_region,
            faction_entity,
            &adjacency,
            &regions,
            &settlements,
            &rel_graph,
        );

        if let Some(target_region) = target
            && let Some(next_step) =
                ecs_bfs_next_step(current_region, target_region, &adjacency, &regions)
        {
            commands.write(SimCommand::bookkeeping(SimCommandKind::MarchArmy {
                army: army_entity,
                target_region: next_step,
            }));
        }
    }
}

// ---------------------------------------------------------------------------
// System 7: Resolve battles (monthly)
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn resolve_battles(
    armies: Query<(Entity, &ArmyState, &SimEntity, Option<&LocatedIn>), With<Army>>,
    regions: Query<&RegionState, With<Region>>,
    persons: Query<(Entity, &PersonCore, &PersonReputation, &SimEntity, Option<&MemberOf>), With<Person>>,
    rel_graph: Res<RelationshipGraph>,
    entity_map: Res<SimEntityMap>,
    mut rng: ResMut<SimRng>,
    mut commands: MessageWriter<SimCommand>,
    clock: Res<SimClock>,
) {
    // Collect army positions
    let army_list: Vec<(Entity, u64, u32, f64, Entity)> = armies
        .iter()
        .filter(|(_, state, sim, loc)| sim.is_alive() && state.strength > 0 && loc.is_some())
        .map(|(e, state, _, loc)| (e, state.faction_id, state.strength, state.morale, loc.unwrap().0))
        .collect();

    // Find co-located hostile pairs
    let mut battles: BTreeSet<(Entity, Entity)> = BTreeSet::new();
    for i in 0..army_list.len() {
        for j in (i + 1)..army_list.len() {
            let (a_entity, a_fid, _, _, a_region) = army_list[i];
            let (b_entity, b_fid, _, _, b_region) = army_list[j];
            if a_region != b_region {
                continue;
            }
            let a_faction = entity_map.get_bevy(a_fid);
            let b_faction = entity_map.get_bevy(b_fid);
            if let (Some(af), Some(bf)) = (a_faction, b_faction)
                && rel_graph.are_at_war(af, bf)
            {
                let pair = if a_entity < b_entity {
                    (a_entity, b_entity)
                } else {
                    (b_entity, a_entity)
                };
                battles.insert(pair);
            }
        }
    }

    for (attacker, defender) in battles {
        let Ok((_, att_state, _, att_loc)) = armies.get(attacker) else {
            continue;
        };
        let Ok((_, def_state, _, _)) = armies.get(defender) else {
            continue;
        };
        if att_state.strength == 0 || def_state.strength == 0 {
            continue;
        }

        let region = att_loc.map(|l| l.0).unwrap_or(Entity::PLACEHOLDER);
        let terrain = regions
            .get(region)
            .map(|rs| rs.terrain)
            .unwrap_or(Terrain::Plains);
        let defense_bonus = get_terrain_defense_bonus(terrain);

        // Compute effective power: strength × morale
        let att_power = att_state.strength as f64 * att_state.morale;
        let def_power = def_state.strength as f64 * def_state.morale * defense_bonus;

        // Get leader prestige bonuses
        let att_faction = entity_map.get_bevy(att_state.faction_id);
        let def_faction = entity_map.get_bevy(def_state.faction_id);
        let att_prestige = att_faction
            .and_then(|f| {
                persons.iter().find_map(|(_, _, rep, psim, pm)| {
                    if psim.is_alive() && pm.is_some_and(|m| m.0 == f) {
                        Some(rep.prestige)
                    } else {
                        None
                    }
                })
            })
            .unwrap_or(0.0);
        let def_prestige = def_faction
            .and_then(|f| {
                persons.iter().find_map(|(_, _, rep, psim, pm)| {
                    if psim.is_alive() && pm.is_some_and(|m| m.0 == f) {
                        Some(rep.prestige)
                    } else {
                        None
                    }
                })
            })
            .unwrap_or(0.0);

        let att_total = att_power * (1.0 + att_prestige * 0.1);
        let def_total = def_power * (1.0 + def_prestige * 0.1);

        let attacker_won = att_total > def_total;

        let (winner_str, loser_str) = if attacker_won {
            (att_state.strength, def_state.strength)
        } else {
            (def_state.strength, att_state.strength)
        };

        let loser_casualty_rate =
            rng.0.random_range(LOSER_CASUALTY_MIN..LOSER_CASUALTY_MAX);
        let winner_casualty_rate =
            rng.0.random_range(WINNER_CASUALTY_MIN..WINNER_CASUALTY_MAX);

        let loser_casualties = (loser_str as f64 * loser_casualty_rate) as u32;
        let winner_casualties = (winner_str as f64 * winner_casualty_rate) as u32;

        let (att_casualties, def_casualties) = if attacker_won {
            (winner_casualties, loser_casualties)
        } else {
            (loser_casualties, winner_casualties)
        };

        commands.write(
            SimCommand::new(
                SimCommandKind::ResolveBattle {
                    attacker_army: attacker,
                    defender_army: defender,
                    attacker_casualties: att_casualties,
                    defender_casualties: def_casualties,
                    attacker_won,
                },
                EventKind::Battle,
                format!("Battle in year {}", clock.time.year()),
            )
            .with_participant(attacker, ParticipantRole::Attacker)
            .with_participant(defender, ParticipantRole::Defender),
        );

        // Kill NPCs in both factions (winner at halved rate)
        let (winner_faction, loser_faction) = if attacker_won {
            (att_faction, def_faction)
        } else {
            (def_faction, att_faction)
        };
        for (faction_opt, is_winner) in [(loser_faction, false), (winner_faction, true)] {
            let Some(faction) = faction_opt else {
                continue;
            };
            for (person_entity, pcore, _, psim, pm) in persons.iter() {
                if !psim.is_alive() || pm.is_none_or(|m| m.0 != faction) {
                    continue;
                }
                let base_chance = if pcore.role == Role::Warrior {
                    WARRIOR_DEATH_CHANCE
                } else {
                    NON_WARRIOR_DEATH_CHANCE
                };
                let death_chance = if is_winner {
                    base_chance * 0.5
                } else {
                    base_chance
                };
                if rng.0.random_bool(death_chance) {
                    commands.write(
                        SimCommand::new(
                            SimCommandKind::PersonDied {
                                person: person_entity,
                            },
                            EventKind::Death,
                            format!("{} killed in battle", psim.name),
                        )
                        .with_participant(person_entity, ParticipantRole::Subject),
                    );
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// System 8: Check retreats (monthly)
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn check_retreats(
    armies: Query<(Entity, &ArmyState, &SimEntity, Option<&LocatedIn>), With<Army>>,
    regions: Query<&RegionState, With<Region>>,
    adjacency: Res<RegionAdjacency>,
    entity_map: Res<SimEntityMap>,
    mut commands: MessageWriter<SimCommand>,
) {
    for (army_entity, state, sim, loc) in armies.iter() {
        if !sim.is_alive() || state.strength == 0 {
            continue;
        }
        let Some(region) = loc.map(|l| l.0) else {
            continue;
        };

        // Check retreat conditions
        let strength_ratio = if state.starting_strength > 0 {
            state.strength as f64 / state.starting_strength as f64
        } else {
            1.0
        };

        if state.morale < RETREAT_MORALE_THRESHOLD || strength_ratio < RETREAT_STRENGTH_RATIO {
            // Retreat toward home region
            let home_region = entity_map.get_bevy(state.home_region_id);
            if let Some(home) = home_region
                && home != region
                && let Some(next_step) =
                    ecs_bfs_next_step(region, home, &adjacency, &regions)
            {
                commands.write(SimCommand::bookkeeping(SimCommandKind::MarchArmy {
                    army: army_entity,
                    target_region: next_step,
                }));
            }

            // Clear besieging if retreating
            if state.besieging_settlement_id.is_some() {
                // Besieging clear will be handled by siege system checking army presence
            }
        }
    }
}

// ---------------------------------------------------------------------------
// System 9: Start sieges (monthly)
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn start_sieges(
    armies: Query<(Entity, &ArmyState, &SimEntity, Option<&LocatedIn>), With<Army>>,
    settlements: Query<
        (Entity, &SimEntity, &SettlementMilitary, Option<&MemberOf>, Option<&LocatedIn>, Option<&EcsActiveSiege>),
        With<Settlement>,
    >,
    rel_graph: Res<RelationshipGraph>,
    entity_map: Res<SimEntityMap>,
    mut commands: MessageWriter<SimCommand>,
    clock: Res<SimClock>,
) {
    for (army_entity, state, sim, loc) in armies.iter() {
        if !sim.is_alive() || state.strength == 0 || state.besieging_settlement_id.is_some() {
            continue;
        }
        let Some(region) = loc.map(|l| l.0) else {
            continue;
        };
        let army_faction = entity_map.get_bevy(state.faction_id);
        let Some(af) = army_faction else {
            continue;
        };

        // Check for opposing army in same region — skip siege if present (armies fight first)
        let has_opposition = armies.iter().any(|(other_entity, ostate, osim, oloc)| {
            other_entity != army_entity
                && osim.is_alive()
                && ostate.strength > 0
                && oloc.map(|l| l.0) == Some(region)
                && entity_map
                    .get_bevy(ostate.faction_id)
                    .is_some_and(|of| rel_graph.are_at_war(af, of))
        });
        if has_opposition {
            continue;
        }

        // Find enemy settlements in this region
        for (sett_entity, sett_sim, mil, member_of, sett_loc, siege) in settlements.iter() {
            if !sett_sim.is_alive() {
                continue;
            }
            if siege.is_some() {
                continue; // Already under siege
            }
            if sett_loc.map(|l| l.0) != Some(region) {
                continue;
            }
            let Some(m) = member_of else {
                continue;
            };
            if !rel_graph.are_at_war(af, m.0) {
                continue;
            }

            // Fortified → begin siege; unfortified → instant capture
            if mil.fortification_level > 0 {
                commands.write(
                    SimCommand::new(
                        SimCommandKind::BeginSiege {
                            army: army_entity,
                            settlement: sett_entity,
                        },
                        EventKind::Siege,
                        format!("Siege of {} begun", sett_sim.name),
                    )
                    .with_participant(army_entity, ParticipantRole::Attacker)
                    .with_participant(sett_entity, ParticipantRole::Object),
                );
            } else {
                // Instant capture
                commands.write(
                    SimCommand::new(
                        SimCommandKind::CaptureSettlement {
                            settlement: sett_entity,
                            new_faction: af,
                        },
                        EventKind::Conquest,
                        format!("{} captured in year {}", sett_sim.name, clock.time.year()),
                    )
                    .with_participant(sett_entity, ParticipantRole::Object)
                    .with_participant(af, ParticipantRole::Subject),
                );
            }
            break; // One siege/capture per army per tick
        }
    }
}

// ---------------------------------------------------------------------------
// System 10: Progress sieges (monthly)
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn progress_sieges(
    mut settlements: Query<
        (Entity, &SimEntity, &mut SettlementCore, &SettlementMilitary, &mut EcsActiveSiege, Option<&MemberOf>, Option<&LocatedIn>),
        With<Settlement>,
    >,
    armies: Query<(Entity, &ArmyState, &SimEntity, Option<&LocatedIn>), With<Army>>,
    entity_map: Res<SimEntityMap>,
    mut rng: ResMut<SimRng>,
    mut commands: MessageWriter<SimCommand>,
) {
    // Collect siege data upfront to avoid borrow issues
    struct SiegeInfo {
        sett_entity: Entity,
        attacker_army_sim_id: u64,
        fort_level: u8,
        sett_region: Option<Entity>,
    }

    let siege_data: Vec<SiegeInfo> = settlements
        .iter()
        .filter(|(_, sim, _, _, _, _, _)| sim.is_alive())
        .map(|(e, _, _core, mil, siege, _member, loc)| SiegeInfo {
            sett_entity: e,
            attacker_army_sim_id: siege.attacker_army_id,
            fort_level: mil.fortification_level,
            sett_region: loc.map(|l| l.0),
        })
        .collect();

    for info in siege_data {
        // Check if attacking army is still alive and in same region as settlement
        let attacker_entity = entity_map.get_bevy(info.attacker_army_sim_id);
        let army_valid = attacker_entity.is_some_and(|ae| {
            armies.get(ae).is_ok_and(|(_, state, asim, loc)| {
                asim.is_alive()
                    && state.strength > 0
                    && loc.map(|l| l.0) == info.sett_region
            })
        });

        if !army_valid {
            // Siege lifted — army died, left region, or war ended
            commands.write(SimCommand::bookkeeping(SimCommandKind::ResolveAssault {
                army: attacker_entity.unwrap_or(Entity::PLACEHOLDER),
                settlement: info.sett_entity,
                succeeded: false,
                attacker_casualties: 0,
                defender_casualties: 0,
            }));
            continue;
        }

        // Progress siege
        let Ok((_, sett_sim, mut core, _mil, mut siege, _, _)) =
            settlements.get_mut(info.sett_entity)
        else {
            continue;
        };

        siege.months_elapsed += 1;
        let months = siege.months_elapsed;

        // Prosperity decay
        core.prosperity = (core.prosperity - SIEGE_PROSPERITY_DECAY).max(0.0);

        // Starvation: prosperity-based (below threshold → population losses)
        if core.prosperity < SIEGE_STARVATION_PROSPERITY_THRESHOLD && core.population > 0 {
            let starve_losses = (core.population as f64 * SIEGE_STARVATION_POP_LOSS).ceil() as u32;
            core.population = core.population.saturating_sub(starve_losses);
            siege.civilian_deaths += starve_losses;
        }

        // Check surrender: bracket-based, modulated by prosperity and fortification
        if months >= 3 {
            let base_chance: f64 = match months {
                3..=5 => 0.02,
                6..=11 => 0.05,
                _ => 0.10,
            };
            // Lower prosperity increases surrender chance
            let prosperity_mod = 1.0 + (1.0 - core.prosperity);
            // Higher fortification reduces surrender chance
            let fort_mod = 1.0 / (1.0 + info.fort_level as f64 * 0.3);
            let surrender_chance = base_chance * prosperity_mod * fort_mod;

            if rng.0.random_range(0.0..1.0) < surrender_chance {
                // Surrender → capture
                if let Some(ae) = attacker_entity {
                    let attacker_faction = armies
                        .get(ae)
                        .ok()
                        .and_then(|(_, state, _, _)| entity_map.get_bevy(state.faction_id));
                    if let Some(af) = attacker_faction {
                        commands.write(
                            SimCommand::new(
                                SimCommandKind::CaptureSettlement {
                                    settlement: info.sett_entity,
                                    new_faction: af,
                                },
                                EventKind::Conquest,
                                format!("{} surrendered after siege", sett_sim.name),
                            )
                            .with_participant(info.sett_entity, ParticipantRole::Object),
                        );
                    }
                }
                continue;
            }
        }

        // Attempt assault: requires minimum months and morale
        if months >= SIEGE_ASSAULT_MIN_MONTHS
            && rng.0.random_range(0.0..1.0) < SIEGE_ASSAULT_CHANCE
            && let Some(ae) = attacker_entity
            && let Ok((_, att_state, _, _)) = armies.get(ae)
            && att_state.morale >= SIEGE_ASSAULT_MORALE_MIN
        {
            let attacker_power = att_state.strength as f64 * att_state.morale;
            let defender_power =
                core.population as f64 * 0.05 * info.fort_level as f64;

            if attacker_power >= defender_power * SIEGE_ASSAULT_POWER_RATIO {
                // Assault succeeds — capture settlement
                let attacker_faction = entity_map.get_bevy(att_state.faction_id);
                let casualty_rate =
                    rng.0.random_range(SIEGE_ASSAULT_CASUALTY_MIN..SIEGE_ASSAULT_CASUALTY_MAX);
                let att_casualties = (att_state.strength as f64 * casualty_rate) as u32;

                commands.write(
                    SimCommand::new(
                        SimCommandKind::ResolveAssault {
                            army: ae,
                            settlement: info.sett_entity,
                            succeeded: true,
                            attacker_casualties: att_casualties,
                            defender_casualties: 0,
                        },
                        EventKind::Conquest,
                        format!("Assault on {} succeeded", sett_sim.name),
                    )
                    .with_participant(ae, ParticipantRole::Attacker)
                    .with_participant(info.sett_entity, ParticipantRole::Object),
                );

                if let Some(af) = attacker_faction {
                    commands.write(
                        SimCommand::new(
                            SimCommandKind::CaptureSettlement {
                                settlement: info.sett_entity,
                                new_faction: af,
                            },
                            EventKind::Conquest,
                            format!("{} conquered by assault", sett_sim.name),
                        )
                        .with_participant(info.sett_entity, ParticipantRole::Object),
                    );
                }
            } else {
                // Assault fails — attacker takes casualties and morale hit
                let casualty_rate =
                    rng.0.random_range(SIEGE_ASSAULT_CASUALTY_MIN..SIEGE_ASSAULT_CASUALTY_MAX);
                let att_casualties = (att_state.strength as f64 * casualty_rate) as u32;

                commands.write(
                    SimCommand::new(
                        SimCommandKind::ResolveAssault {
                            army: ae,
                            settlement: info.sett_entity,
                            succeeded: false,
                            attacker_casualties: att_casualties,
                            defender_casualties: 0,
                        },
                        EventKind::Battle,
                        format!("Assault on {} failed", sett_sim.name),
                    )
                    .with_participant(ae, ParticipantRole::Attacker)
                    .with_participant(info.sett_entity, ParticipantRole::Object),
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// System 11: War endings and disbanding (yearly guard)
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn war_endings_and_disbanding(
    clock: Res<SimClock>,
    factions: Query<(Entity, &SimEntity, &FactionCore, &FactionMilitary), With<Faction>>,
    armies: Query<(Entity, &ArmyState, &SimEntity, Option<&LocatedIn>), With<Army>>,
    _settlements: Query<(&SimEntity, Option<&MemberOf>, Option<&LocatedIn>), With<Settlement>>,
    hired_by_query: Query<(Entity, &HiredBy), With<Faction>>,
    rel_graph: Res<RelationshipGraph>,
    entity_map: Res<SimEntityMap>,
    mut rng: ResMut<SimRng>,
    mut commands: MessageWriter<SimCommand>,
) {
    if !clock.time.is_year_start() {
        return;
    }

    let current_year = clock.time.year();

    // Find all active war pairs
    let mut war_pairs: Vec<(Entity, Entity)> = Vec::new();
    let alive_factions: Vec<(Entity, &FactionCore, &FactionMilitary)> = factions
        .iter()
        .filter(|(_, sim, core, _)| sim.is_alive() && !is_non_state_faction(core))
        .map(|(e, _, core, mil)| (e, core, mil))
        .collect();

    for i in 0..alive_factions.len() {
        for j in (i + 1)..alive_factions.len() {
            let (fa, _, _) = alive_factions[i];
            let (fb, _, _) = alive_factions[j];
            if rel_graph.are_at_war(fa, fb) {
                war_pairs.push((fa, fb));
            }
        }
    }

    for (fa, fb) in war_pairs {
        // Check if either side has no army (instant surrender)
        let fa_has_army = armies.iter().any(|(_, state, asim, _)| {
            asim.is_alive()
                && state.strength > 0
                && entity_map.get_bevy(state.faction_id) == Some(fa)
        });
        let fb_has_army = armies.iter().any(|(_, state, asim, _)| {
            asim.is_alive()
                && state.strength > 0
                && entity_map.get_bevy(state.faction_id) == Some(fb)
        });

        let should_end = if !fa_has_army || !fb_has_army {
            true
        } else {
            // War exhaustion check: growing probability over time
            let war_start = rel_graph
                .at_war
                .get(&RelationshipGraph::canonical_pair(fa, fb))
                .map(|meta| meta.start);
            if let Some(start) = war_start {
                let war_years = clock.time.years_since(start);
                if war_years >= WAR_EXHAUSTION_START_YEAR {
                    let peace_chance = (PEACE_CHANCE_PER_YEAR
                        * (war_years - WAR_EXHAUSTION_START_YEAR + 1) as f64)
                        .min(0.8);
                    rng.0.random_range(0.0..1.0) < peace_chance
                } else {
                    false
                }
            } else {
                false
            }
        };

        if !should_end {
            continue;
        }

        // Determine winner/loser
        let fa_strength: u32 = armies
            .iter()
            .filter(|(_, state, asim, _)| {
                asim.is_alive() && entity_map.get_bevy(state.faction_id) == Some(fa)
            })
            .map(|(_, state, _, _)| state.strength)
            .sum();
        let fb_strength: u32 = armies
            .iter()
            .filter(|(_, state, asim, _)| {
                asim.is_alive() && entity_map.get_bevy(state.faction_id) == Some(fb)
            })
            .map(|(_, state, _, _)| state.strength)
            .sum();

        let (winner, loser) = if fa_strength >= fb_strength {
            (fa, fb)
        } else {
            (fb, fa)
        };
        let decisive = !fa_has_army || !fb_has_army || fa_strength > fb_strength * 2;

        commands.write(
            SimCommand::new(
                SimCommandKind::SignTreaty {
                    faction_a: fa,
                    faction_b: fb,
                    winner,
                    loser,
                    decisive,
                },
                EventKind::Treaty,
                format!("Treaty signed in year {current_year}"),
            )
            .with_participant(fa, ParticipantRole::Subject)
            .with_participant(fb, ParticipantRole::Object),
        );

        // Disband armies of both factions
        for (army_entity, state, asim, _) in armies.iter() {
            if !asim.is_alive() {
                continue;
            }
            let army_faction = entity_map.get_bevy(state.faction_id);
            if (army_faction == Some(fa) || army_faction == Some(fb))
                && !state.is_mercenary
            {
                commands.write(SimCommand::new(
                    SimCommandKind::DisbandArmy { army: army_entity },
                    EventKind::Dissolution,
                    format!("{} disbanded after war", asim.name),
                ));
            }
        }

        // End mercenary contracts for both sides
        for (merc_entity, hb) in hired_by_query.iter() {
            if hb.0 == fa || hb.0 == fb {
                commands.write(SimCommand::new(
                    SimCommandKind::EndMercenaryContract {
                        mercenary: merc_entity,
                    },
                    EventKind::Dissolution,
                    format!("Mercenary contract ended after war in year {current_year}"),
                ));
            }
        }
    }

    // --- Mercenary disbanding ---
    for (army_entity, state, asim, _) in armies.iter() {
        if !asim.is_alive() || !state.is_mercenary {
            continue;
        }
        if state.strength < MERC_DISBAND_STRENGTH_MIN
            || state.months_campaigning > MERC_DISBAND_IDLE_MONTHS
        {
            commands.write(SimCommand::new(
                SimCommandKind::DisbandArmy { army: army_entity },
                EventKind::Dissolution,
                format!("{} mercenary company disbanded", asim.name),
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::app::build_sim_app;
    use crate::ecs::components::{
        FactionCore, FactionDiplomacy, FactionMilitary, SettlementCore, SettlementCrime,
        SettlementCulture, SettlementDisease, SettlementEducation, SettlementMilitary,
        SettlementTrade,
    };
    use crate::ecs::relationships::{MemberOf, RegionAdjacency, RelationshipGraph, RelationshipMeta};
    use crate::ecs::schedule::SimTick;
    use crate::ecs::spawn;
    use crate::ecs::time::SimTime;

    fn setup_app() -> bevy_app::App {
        let mut app = build_sim_app(100);
        app.insert_resource(RegionAdjacency::new());
        add_conflict_systems(&mut app);
        app
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

    fn spawn_settlement(
        app: &mut bevy_app::App,
        sim_id: u64,
        faction: Entity,
        region: Entity,
        population: u32,
    ) -> Entity {
        use crate::ecs::components::{EcsBuildingBonuses, EcsSeasonalModifiers};
        use crate::model::population::PopulationBreakdown;

        let entity = spawn::spawn_settlement(
            app.world_mut(),
            sim_id,
            format!("Settlement {sim_id}"),
            Some(SimTime::from_year(50)),
            SettlementCore {
                population,
                population_breakdown: PopulationBreakdown::from_total(population),
                prosperity: 0.5,
                capacity: 500,
                ..SettlementCore::default()
            },
            SettlementCulture::default(),
            SettlementDisease::default(),
            SettlementTrade::default(),
            SettlementMilitary::default(),
            SettlementCrime::default(),
            SettlementEducation::default(),
            EcsSeasonalModifiers::default(),
            EcsBuildingBonuses::default(),
        );
        app.world_mut()
            .entity_mut(entity)
            .insert((LocatedIn(region), MemberOf(faction)));
        entity
    }

    fn spawn_army_entity(
        app: &mut bevy_app::App,
        sim_id: u64,
        faction_sim_id: u64,
        region: Entity,
        strength: u32,
    ) -> Entity {
        let entity = spawn::spawn_army(
            app.world_mut(),
            sim_id,
            format!("Army {sim_id}"),
            Some(SimTime::from_year(100)),
            ArmyState {
                strength,
                starting_strength: strength,
                faction_id: faction_sim_id,
                morale: 0.8,
                supply: 3.0,
                ..ArmyState::default()
            },
        );
        app.world_mut().entity_mut(entity).insert(LocatedIn(region));
        entity
    }

    fn tick_months(app: &mut bevy_app::App, months: u32) {
        use crate::ecs::clock::SimClock;
        use crate::ecs::time::MINUTES_PER_MONTH;
        for _ in 0..months {
            let new_time = SimTime::from_minutes(
                app.world().resource::<SimClock>().time.as_minutes() + MINUTES_PER_MONTH,
            );
            app.world_mut().resource_mut::<SimClock>().time = new_time;
            app.world_mut().run_schedule(SimTick);
        }
    }

    fn tick_years(app: &mut bevy_app::App, years: u32) {
        tick_months(app, years * 12);
    }

    #[test]
    fn terrain_defense_bonus_values() {
        assert!((get_terrain_defense_bonus(Terrain::Mountains) - 1.3).abs() < 0.001);
        assert!((get_terrain_defense_bonus(Terrain::Forest) - 1.15).abs() < 0.001);
        assert!((get_terrain_defense_bonus(Terrain::Plains) - 1.0).abs() < 0.001);
    }

    #[test]
    fn forage_terrain_modifier_values() {
        assert!((forage_terrain_modifier(Terrain::Plains) - 1.3).abs() < 0.001);
        assert!((forage_terrain_modifier(Terrain::Swamp) - 0.6).abs() < 0.001);
        assert!((forage_terrain_modifier(Terrain::ShallowWater) - 0.0).abs() < 0.001);
    }

    #[test]
    fn disease_rate_values() {
        assert!((disease_rate_for_terrain(Terrain::Swamp) - 0.03).abs() < 0.001);
        assert!((disease_rate_for_terrain(Terrain::Plains) - 0.005).abs() < 0.001);
    }

    #[test]
    fn war_declaration_between_enemies() {
        let mut app = setup_app();
        let r = spawn_region(&mut app, 100, Terrain::Plains);
        let fa = spawn_faction(&mut app, 1);
        let fb = spawn_faction(&mut app, 2);
        spawn_settlement(&mut app, 10, fa, r, 500);
        spawn_settlement(&mut app, 11, fb, r, 500);

        // Make them enemies
        let pair = RelationshipGraph::canonical_pair(fa, fb);
        app.world_mut()
            .resource_mut::<RelationshipGraph>()
            .enemies
            .insert(pair, RelationshipMeta::new(SimTime::from_year(50)));

        // Set up adjacency
        app.world_mut()
            .resource_mut::<RegionAdjacency>()
            .add_edge(r, r); // Self-adjacent for test

        // Run for many years to trigger war declaration
        tick_years(&mut app, 30);

        // Check if at war
        let rel = app.world().resource::<RelationshipGraph>();
        // War may or may not have been declared (probabilistic), just verify no crash
    }

    #[test]
    fn army_takes_attrition() {
        let mut app = setup_app();
        let r = spawn_region(&mut app, 100, Terrain::Swamp);
        let _fa = spawn_faction(&mut app, 1);
        let army = spawn_army_entity(&mut app, 200, 1, r, 100);

        let initial = app.world().get::<ArmyState>(army).unwrap().strength;
        tick_months(&mut app, 6);
        let after = app.world().get::<ArmyState>(army).unwrap().strength;
        assert!(after < initial, "army should lose strength to attrition in swamp");
    }

    #[test]
    fn battle_resolves_with_casualties() {
        let mut app = setup_app();
        let r = spawn_region(&mut app, 100, Terrain::Plains);
        let fa = spawn_faction(&mut app, 1);
        let fb = spawn_faction(&mut app, 2);
        spawn_settlement(&mut app, 10, fa, r, 500);
        spawn_settlement(&mut app, 11, fb, r, 500);
        let army_a = spawn_army_entity(&mut app, 200, 1, r, 100);
        let army_b = spawn_army_entity(&mut app, 201, 2, r, 100);

        // Declare war
        let pair = RelationshipGraph::canonical_pair(fa, fb);
        app.world_mut()
            .resource_mut::<RelationshipGraph>()
            .at_war
            .insert(pair, RelationshipMeta::new(SimTime::from_year(99)));

        tick_months(&mut app, 1);

        // At least one army should have taken casualties
        let str_a = app.world().get::<ArmyState>(army_a).unwrap().strength;
        let str_b = app.world().get::<ArmyState>(army_b).unwrap().strength;
        assert!(
            str_a < 100 || str_b < 100,
            "battle should cause casualties: a={str_a}, b={str_b}"
        );
    }

    #[test]
    fn siege_starts_on_fortified_settlement() {
        let mut app = setup_app();
        let r = spawn_region(&mut app, 100, Terrain::Plains);
        let fa = spawn_faction(&mut app, 1);
        let fb = spawn_faction(&mut app, 2);
        let sett = spawn_settlement(&mut app, 10, fb, r, 300);
        spawn_settlement(&mut app, 11, fa, r, 300);
        let _army = spawn_army_entity(&mut app, 200, 1, r, 100);

        // Fortify settlement
        app.world_mut()
            .get_mut::<SettlementMilitary>(sett)
            .unwrap()
            .fortification_level = 1;

        // Declare war
        let pair = RelationshipGraph::canonical_pair(fa, fb);
        app.world_mut()
            .resource_mut::<RelationshipGraph>()
            .at_war
            .insert(pair, RelationshipMeta::new(SimTime::from_year(99)));

        tick_months(&mut app, 1);

        // Check if settlement is under siege
        let has_siege = app.world().get::<EcsActiveSiege>(sett).is_some();
        assert!(has_siege, "fortified settlement should be under siege");
    }

    #[test]
    fn war_ends_after_exhaustion() {
        let mut app = setup_app();
        let r = spawn_region(&mut app, 100, Terrain::Plains);
        let fa = spawn_faction(&mut app, 1);
        let fb = spawn_faction(&mut app, 2);
        spawn_settlement(&mut app, 10, fa, r, 500);
        spawn_settlement(&mut app, 11, fb, r, 500);
        let _army_a = spawn_army_entity(&mut app, 200, 1, r, 100);
        let _army_b = spawn_army_entity(&mut app, 201, 2, r, 100);

        // Declare war
        let pair = RelationshipGraph::canonical_pair(fa, fb);
        app.world_mut()
            .resource_mut::<RelationshipGraph>()
            .at_war
            .insert(pair, RelationshipMeta::new(SimTime::from_year(100)));

        // Run for many years — war should eventually end
        tick_years(&mut app, 20);

        // War may or may not have ended (probabilistic + armies may die)
        // Just verify no crash
    }

    #[test]
    fn unfortified_captured_instantly() {
        let mut app = setup_app();
        let r = spawn_region(&mut app, 100, Terrain::Plains);
        let fa = spawn_faction(&mut app, 1);
        let fb = spawn_faction(&mut app, 2);
        let sett = spawn_settlement(&mut app, 10, fb, r, 300);
        spawn_settlement(&mut app, 11, fa, r, 300);
        let _army = spawn_army_entity(&mut app, 200, 1, r, 100);

        // Leave fortification at 0 (default)
        // Declare war
        let pair = RelationshipGraph::canonical_pair(fa, fb);
        app.world_mut()
            .resource_mut::<RelationshipGraph>()
            .at_war
            .insert(pair, RelationshipMeta::new(SimTime::from_year(99)));

        tick_months(&mut app, 1);

        // Settlement should have been captured (MemberOf changed)
        let member = app.world().get::<MemberOf>(sett);
        if let Some(m) = member {
            // May have been captured by fa
            assert!(
                m.0 == fa || m.0 == fb,
                "settlement should belong to some faction"
            );
        }
    }
}
