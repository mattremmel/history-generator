//! Demographics system — migrated from `src/sim/demographics.rs`.
//!
//! Five chained yearly systems (Update phase):
//! 1. `compute_carrying_capacity` — terrain + bonuses → SettlementCore.capacity
//! 2. `grow_population` — bracket growth, abandonment
//! 3. `process_mortality` — age-bracket death rolls on living Persons
//! 4. `process_births` — notable person generation per settlement
//! 5. `process_marriages` — intra-settlement + cross-faction marriages

use std::collections::BTreeMap;

use bevy_app::App;
use bevy_ecs::entity::Entity;
use bevy_ecs::message::MessageWriter;
use bevy_ecs::query::With;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Query, Res, ResMut};
use rand::Rng;

use crate::ecs::clock::SimClock;
use crate::ecs::commands::{SimCommand, SimCommandKind};
use crate::ecs::components::{
    CultureState, EcsBuildingBonuses, EcsSeasonalModifiers, Faction, Person, PersonCore,
    RegionState, Settlement, SettlementCore, SettlementEducation, SettlementTrade, SimEntity,
};
use crate::ecs::conditions::yearly;
use crate::ecs::relationships::{LocatedIn, MemberOf, RelationshipGraph};
use crate::ecs::resources::{SimEntityMap, SimRng};
use crate::ecs::schedule::{SimPhase, SimTick};
use crate::model::Sex;
use crate::model::entity_data::Role;
use crate::model::event::{EventKind, ParticipantRole};
use crate::model::traits::generate_traits;
use crate::sim::culture_names::generate_culture_person_name;
use crate::sim::names::{EPITHETS, extract_surname, generate_person_name};
use crate::worldgen::terrain::TerrainProfile;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const REGION_CAPACITY_MULTIPLIER: u32 = 5;
const DEFAULT_CAPACITY: u32 = 500;
const FOOD_BUFFER_POP_PER_UNIT: f64 = 50.0;
const PORT_FISHING_CAPACITY: u32 = 200;
const COASTAL_FISHING_CAPACITY: u32 = 50;

const ABANDONMENT_THRESHOLD: u32 = 10;

const NOTABLE_POP_DIVISOR: f64 = 5.0;
const MIN_TARGET_NOTABLES: u32 = 3;
const MAX_TARGET_NOTABLES: u32 = 25;
const MAX_NOTABLE_BIRTHS_PER_YEAR: u32 = 2;

const ROLES: [Role; 6] = [
    Role::Common,
    Role::Artisan,
    Role::Warrior,
    Role::Merchant,
    Role::Scholar,
    Role::Elder,
];
const ROLE_WEIGHTS: [u32; 6] = [30, 20, 20, 15, 10, 5];

const MORTALITY_INFANT: f64 = 0.03;
const MORTALITY_CHILD: f64 = 0.005;
const MORTALITY_YOUNG_ADULT: f64 = 0.008;
const MORTALITY_MIDDLE_AGE: f64 = 0.015;
const MORTALITY_ELDER: f64 = 0.04;
const MORTALITY_AGED: f64 = 0.10;
const MORTALITY_ANCIENT: f64 = 0.25;
const MORTALITY_CENTENARIAN: f64 = 1.0;

const ADULT_AGE: u32 = 16;

const INTRA_SETTLEMENT_MARRIAGE_CHANCE: f64 = 0.15;
const CROSS_FACTION_MARRIAGE_CHANCE: f64 = 0.05;
const CROSS_FACTION_ALLIANCE_CHANCE: f64 = 0.5;
const WIDOWED_REMARRIAGE_COOLDOWN: u32 = 3;

// ---------------------------------------------------------------------------
// Shared types
// ---------------------------------------------------------------------------

struct PersonInfo {
    entity: Entity,
    sex: Sex,
    born: crate::ecs::time::SimTime,
    settlement: Option<Entity>,
    _is_married: bool,
    spouse: Option<Entity>,
}

// ---------------------------------------------------------------------------
// Plugin registration
// ---------------------------------------------------------------------------

pub fn add_demographics_systems(app: &mut App) {
    app.add_systems(
        SimTick,
        (
            compute_carrying_capacity,
            grow_population,
            process_mortality,
            process_births,
            process_marriages,
        )
            .chain()
            .run_if(yearly)
            .in_set(SimPhase::Update),
    );
}

// ---------------------------------------------------------------------------
// System 1: Compute carrying capacity
// ---------------------------------------------------------------------------

#[allow(clippy::type_complexity)]
fn compute_carrying_capacity(
    mut settlements: Query<
        (
            &SimEntity,
            &mut SettlementCore,
            &SettlementTrade,
            &EcsBuildingBonuses,
            &EcsSeasonalModifiers,
            Option<&LocatedIn>,
        ),
        With<Settlement>,
    >,
    regions: Query<&RegionState>,
) {
    for (sim, mut core, trade, bonuses, seasonal, located_in) in settlements.iter_mut() {
        if !sim.is_alive() {
            continue;
        }

        let base_capacity = located_in
            .and_then(|loc| regions.get(loc.0).ok())
            .map(|region| {
                let profile = TerrainProfile::new(region.terrain, region.terrain_tags.clone());
                profile.effective_population_range().1 * REGION_CAPACITY_MULTIPLIER
            })
            .unwrap_or(DEFAULT_CAPACITY);

        let capacity_bonus = bonuses.capacity as u32;
        let food_buffer_capacity = (bonuses.food_buffer * FOOD_BUFFER_POP_PER_UNIT) as u32;

        let has_port = bonuses.port_trade > 0.0;
        let fishing_cap = match (trade.is_coastal, has_port) {
            (true, true) => PORT_FISHING_CAPACITY,
            (true, false) => COASTAL_FISHING_CAPACITY,
            _ => 0,
        };

        let raw_capacity = base_capacity + capacity_bonus + food_buffer_capacity + fishing_cap;
        core.capacity = (raw_capacity as f64 * seasonal.food_annual) as u32;
    }
}

// ---------------------------------------------------------------------------
// System 2: Grow population
// ---------------------------------------------------------------------------

fn grow_population(
    clock: Res<SimClock>,
    mut rng: ResMut<SimRng>,
    mut settlements: Query<(Entity, &SimEntity, &mut SettlementCore), With<Settlement>>,
    mut commands: MessageWriter<SimCommand>,
) {
    let current_year = clock.time.year();
    let rng = &mut rng.0;

    // Collect settlement data immutably first to determine actions
    struct SettlementSnapshot {
        entity: Entity,
        capacity: u32,
        is_alive: bool,
    }

    let snapshots: Vec<SettlementSnapshot> = settlements
        .iter()
        .map(|(entity, sim, core)| SettlementSnapshot {
            entity,
            capacity: core.capacity.max(1),
            is_alive: sim.is_alive(),
        })
        .collect();

    // Now apply growth with mutable access
    let mut abandons: Vec<Entity> = Vec::new();

    for snap in &snapshots {
        if !snap.is_alive {
            continue;
        }
        if let Ok((_, _, mut core)) = settlements.get_mut(snap.entity) {
            core.population_breakdown.tick_year(snap.capacity, rng);
            let new_pop = core.population_breakdown.total();
            core.population = new_pop;
            if new_pop < ABANDONMENT_THRESHOLD {
                abandons.push(snap.entity);
            }
        }
    }

    for entity in abandons {
        commands.write(
            SimCommand::new(
                SimCommandKind::EndEntity { entity },
                EventKind::Abandoned,
                format!("Settlement abandoned due to population collapse in year {current_year}"),
            )
            .with_participant(entity, ParticipantRole::Subject),
        );
    }
}

// ---------------------------------------------------------------------------
// System 3: Process mortality
// ---------------------------------------------------------------------------

fn process_mortality(
    clock: Res<SimClock>,
    mut rng: ResMut<SimRng>,
    persons: Query<(Entity, &SimEntity, &PersonCore), With<Person>>,
    rel_graph: Res<RelationshipGraph>,
    mut commands: MessageWriter<SimCommand>,
) {
    let rng = &mut rng.0;

    let mut deaths: Vec<(Entity, String)> = Vec::new();

    for (entity, sim, core) in persons.iter() {
        if !sim.is_alive() {
            continue;
        }
        let age = clock.time.years_since(core.born);
        let mortality = mortality_rate(age);
        let roll: f64 = rng.random_range(0.0..1.0);
        if roll < mortality {
            deaths.push((entity, sim.name.clone()));
        }
    }

    for (entity, name) in deaths {
        commands.write(
            SimCommand::new(
                SimCommandKind::PersonDied { person: entity },
                EventKind::Death,
                format!("{name} died in year {}", clock.time.year()),
            )
            .with_participant(entity, ParticipantRole::Subject),
        );
    }

    // Set widowed_at for surviving spouses (direct writes)
    // This is handled in the applicator via PersonDied → EntityDied reactive event chain
    let _ = rel_graph; // used for spouse tracking in future
}

// ---------------------------------------------------------------------------
// System 4: Process births
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn process_births(
    clock: Res<SimClock>,
    mut rng: ResMut<SimRng>,
    settlements: Query<
        (
            Entity,
            &SimEntity,
            &SettlementCore,
            &SettlementEducation,
            Option<&MemberOf>,
        ),
        With<Settlement>,
    >,
    persons: Query<(Entity, &SimEntity, &PersonCore, Option<&LocatedIn>), With<Person>>,
    _cultures: Query<&CultureState>,
    _entity_map: Res<SimEntityMap>,
    rel_graph: Res<RelationshipGraph>,
    mut commands: MessageWriter<SimCommand>,
) {
    let rng = &mut rng.0;

    // Collect living person names for uniqueness checking
    let living_names: BTreeMap<String, ()> = persons
        .iter()
        .filter(|(_, sim, _, _)| sim.is_alive())
        .map(|(_, sim, _, _)| (sim.name.clone(), ()))
        .collect();

    // Collect per-settlement person info for parent selection

    let person_infos: Vec<PersonInfo> = persons
        .iter()
        .filter(|(_, sim, _, _)| sim.is_alive())
        .map(|(entity, _, core, loc)| {
            let settlement = loc.map(|l| l.0);
            // Check if married via RelationshipGraph
            let spouse = rel_graph
                .spouses
                .iter()
                .find(|((a, b), meta)| meta.is_active() && (*a == entity || *b == entity))
                .map(|((a, b), _)| if *a == entity { *b } else { *a });
            PersonInfo {
                entity,
                sex: core.sex,
                born: core.born,
                settlement,
                _is_married: spouse.is_some(),
                spouse,
            }
        })
        .collect();

    struct BirthPlan {
        settlement_entity: Entity,
        faction_entity: Entity,
        count: u32,
    }

    let mut birth_plans: Vec<BirthPlan> = Vec::new();

    for (sett_entity, sett_sim, sett_core, _, member_of) in settlements.iter() {
        if !sett_sim.is_alive() {
            continue;
        }
        let faction_entity = match member_of {
            Some(m) => m.0,
            None => continue,
        };

        let target_notables = ((sett_core.population as f64 / NOTABLE_POP_DIVISOR)
            .sqrt()
            .round() as u32)
            .clamp(MIN_TARGET_NOTABLES, MAX_TARGET_NOTABLES);
        let current_notables = person_infos
            .iter()
            .filter(|p| p.settlement == Some(sett_entity))
            .count() as u32;

        if current_notables < target_notables {
            let births = (target_notables - current_notables).min(MAX_NOTABLE_BIRTHS_PER_YEAR);
            birth_plans.push(BirthPlan {
                settlement_entity: sett_entity,
                faction_entity,
                count: births,
            });
        }
    }

    // Track generated names to avoid duplicates within this batch
    let mut used_names: BTreeMap<String, ()> = living_names;

    for plan in &birth_plans {
        // Get settlement culture info for naming
        let naming_style: Option<crate::model::NamingStyle> = None; // simplified — uses generic names

        // Get settlement literacy for scholar boost
        let literacy = settlements
            .get(plan.settlement_entity)
            .ok()
            .map(|(_, _, _, edu, _)| edu.literacy_rate)
            .unwrap_or(0.0);

        for _ in 0..plan.count {
            // Find parents
            let (father, mother) =
                find_parents(&person_infos, plan.settlement_entity, clock.time, rng);

            // Generate name with surname inheritance
            let name = generate_ecs_person_name(
                father,
                mother,
                &persons,
                naming_style.as_ref(),
                &used_names,
                rng,
            );
            used_names.insert(name.clone(), ());

            // Weighted role selection (Scholar weight boosted by literacy)
            let scholar_boost = (literacy * 10.0) as u32;
            let mut adjusted_weights = ROLE_WEIGHTS;
            adjusted_weights[4] += scholar_boost;
            let adj_weight_total: u32 = adjusted_weights.iter().sum();
            let roll = rng.random_range(0..adj_weight_total);
            let mut cumulative = 0;
            let mut selected_role = ROLES[0].clone();
            for (i, &w) in adjusted_weights.iter().enumerate() {
                cumulative += w;
                if roll < cumulative {
                    selected_role = ROLES[i].clone();
                    break;
                }
            }

            let sex = if rng.random_bool(0.5) {
                Sex::Male
            } else {
                Sex::Female
            };

            let traits = generate_traits(&selected_role, rng);

            // Look up settlement's dominant culture ID
            let culture_id: Option<u64> = None; // simplified for now

            commands.write(
                SimCommand::new(
                    SimCommandKind::PersonBorn {
                        name: name.clone(),
                        faction: plan.faction_entity,
                        settlement: plan.settlement_entity,
                        sex,
                        role: selected_role,
                        traits,
                        culture_id,
                        father,
                        mother,
                    },
                    EventKind::Birth,
                    format!("{name} born in year {}", clock.time.year()),
                )
                .with_participant(plan.settlement_entity, ParticipantRole::Location),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// System 5: Process marriages
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn process_marriages(
    clock: Res<SimClock>,
    mut rng: ResMut<SimRng>,
    persons: Query<
        (
            Entity,
            &SimEntity,
            &PersonCore,
            Option<&LocatedIn>,
            Option<&MemberOf>,
        ),
        With<Person>,
    >,
    factions: Query<(Entity, &SimEntity), With<Faction>>,
    rel_graph: Res<RelationshipGraph>,
    mut commands: MessageWriter<SimCommand>,
) {
    let rng = &mut rng.0;

    struct MarriageCandidate {
        entity: Entity,
        sex: Sex,
        faction: Option<Entity>,
        _settlement: Entity,
    }

    // Collect unmarried adults grouped by settlement
    let mut by_settlement: BTreeMap<Entity, Vec<MarriageCandidate>> = BTreeMap::new();

    for (entity, sim, core, loc, member_of) in persons.iter() {
        if !sim.is_alive() {
            continue;
        }
        if clock.time.years_since(core.born) < ADULT_AGE {
            continue;
        }
        // Skip if already married
        let is_married = rel_graph
            .spouses
            .iter()
            .any(|((a, b), meta)| meta.is_active() && (*a == entity || *b == entity));
        if is_married {
            continue;
        }
        // Skip if recently widowed
        if let Some(widowed_at) = core.widowed_at
            && clock.time.years_since(widowed_at) < WIDOWED_REMARRIAGE_COOLDOWN
        {
            continue;
        }
        let Some(loc) = loc else { continue };
        let faction = member_of.map(|m| m.0);

        by_settlement
            .entry(loc.0)
            .or_default()
            .push(MarriageCandidate {
                entity,
                sex: core.sex,
                faction,
                _settlement: loc.0,
            });
    }

    struct MarriagePlan {
        spouse_a: Entity,
        spouse_b: Entity,
        cross_faction: bool,
        faction_a: Option<Entity>,
        faction_b: Option<Entity>,
    }

    let mut marriages: Vec<MarriagePlan> = Vec::new();

    // Intra-settlement marriages
    for candidates in by_settlement.values() {
        let males: Vec<&MarriageCandidate> =
            candidates.iter().filter(|c| c.sex == Sex::Male).collect();
        let females: Vec<&MarriageCandidate> =
            candidates.iter().filter(|c| c.sex == Sex::Female).collect();
        if males.is_empty() || females.is_empty() {
            continue;
        }
        if rng.random_range(0.0..1.0) < INTRA_SETTLEMENT_MARRIAGE_CHANCE {
            let groom = males[rng.random_range(0..males.len())];
            let bride = females[rng.random_range(0..females.len())];
            marriages.push(MarriagePlan {
                spouse_a: groom.entity,
                spouse_b: bride.entity,
                cross_faction: false,
                faction_a: groom.faction,
                faction_b: bride.faction,
            });
        }
    }

    // Cross-faction marriage
    if rng.random_range(0.0..1.0) < CROSS_FACTION_MARRIAGE_CHANCE {
        let living_factions: Vec<Entity> = factions
            .iter()
            .filter(|(_, sim)| sim.is_alive())
            .map(|(e, _)| e)
            .collect();

        if living_factions.len() >= 2 {
            let idx_a = rng.random_range(0..living_factions.len());
            let mut idx_b = rng.random_range(0..living_factions.len() - 1);
            if idx_b >= idx_a {
                idx_b += 1;
            }
            let fa = living_factions[idx_a];
            let fb = living_factions[idx_b];

            // Check not at war or enemies
            if !rel_graph.are_at_war(fa, fb) && !rel_graph.are_enemies(fa, fb) {
                let all_candidates: Vec<&MarriageCandidate> =
                    by_settlement.values().flat_map(|v| v.iter()).collect();
                let cand_a: Vec<&&MarriageCandidate> = all_candidates
                    .iter()
                    .filter(|c| c.faction == Some(fa))
                    .collect();
                let cand_b: Vec<&&MarriageCandidate> = all_candidates
                    .iter()
                    .filter(|c| c.faction == Some(fb))
                    .collect();

                if !cand_a.is_empty() && !cand_b.is_empty() {
                    let a = cand_a[rng.random_range(0..cand_a.len())];
                    let b = cand_b[rng.random_range(0..cand_b.len())];
                    if a.entity != b.entity && a.sex != b.sex {
                        marriages.push(MarriagePlan {
                            spouse_a: a.entity,
                            spouse_b: b.entity,
                            cross_faction: true,
                            faction_a: Some(fa),
                            faction_b: Some(fb),
                        });
                    }
                }
            }
        }
    }

    // Emit commands
    for marriage in &marriages {
        let name_a = persons
            .get(marriage.spouse_a)
            .map(|(_, sim, _, _, _)| sim.name.clone())
            .unwrap_or_default();
        let name_b = persons
            .get(marriage.spouse_b)
            .map(|(_, sim, _, _, _)| sim.name.clone())
            .unwrap_or_default();

        let desc = format!(
            "{name_a} and {name_b} married in year {}",
            clock.time.year()
        );

        commands.write(
            SimCommand::new(
                SimCommandKind::Marriage {
                    person_a: marriage.spouse_a,
                    person_b: marriage.spouse_b,
                },
                EventKind::Union,
                desc,
            )
            .with_participant(marriage.spouse_a, ParticipantRole::Subject)
            .with_participant(marriage.spouse_b, ParticipantRole::Object),
        );

        // Cross-faction diplomatic marriage → potential alliance
        if marriage.cross_faction
            && let (Some(fa), Some(fb)) = (marriage.faction_a, marriage.faction_b)
            && !rel_graph.are_allies(fa, fb)
            && rng.random_bool(CROSS_FACTION_ALLIANCE_CHANCE)
        {
            commands.write(
                SimCommand::new(
                    SimCommandKind::FormAlliance {
                        faction_a: fa,
                        faction_b: fb,
                    },
                    EventKind::Alliance,
                    format!(
                        "Alliance formed through marriage in year {}",
                        clock.time.year()
                    ),
                )
                .with_participant(fa, ParticipantRole::Subject)
                .with_participant(fb, ParticipantRole::Object),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

fn mortality_rate(age: u32) -> f64 {
    match age {
        0..=5 => MORTALITY_INFANT,
        6..=15 => MORTALITY_CHILD,
        16..=40 => MORTALITY_YOUNG_ADULT,
        41..=60 => MORTALITY_MIDDLE_AGE,
        61..=75 => MORTALITY_ELDER,
        76..=90 => MORTALITY_AGED,
        91..=99 => MORTALITY_ANCIENT,
        _ => MORTALITY_CENTENARIAN,
    }
}

fn find_parents(
    person_infos: &[PersonInfo],
    settlement: Entity,
    current_time: crate::ecs::time::SimTime,
    rng: &mut dyn rand::RngCore,
) -> (Option<Entity>, Option<Entity>) {
    let adults: Vec<&PersonInfo> = person_infos
        .iter()
        .filter(|p| {
            p.settlement == Some(settlement) && current_time.years_since(p.born) >= ADULT_AGE
        })
        .collect();

    let males: Vec<&PersonInfo> = adults
        .iter()
        .filter(|p| p.sex == Sex::Male)
        .copied()
        .collect();
    let females: Vec<&PersonInfo> = adults
        .iter()
        .filter(|p| p.sex == Sex::Female)
        .copied()
        .collect();

    // Try to find a married couple
    let mut couples: Vec<(Entity, Entity)> = Vec::new();
    for m in &males {
        if let Some(spouse) = m.spouse
            && females.iter().any(|f| f.entity == spouse)
        {
            couples.push((m.entity, spouse));
        }
    }

    if !couples.is_empty() {
        let idx = rng.random_range(0..couples.len());
        return (Some(couples[idx].0), Some(couples[idx].1));
    }

    // Fallback: random male + random female
    let father = if !males.is_empty() {
        Some(males[rng.random_range(0..males.len())].entity)
    } else {
        None
    };
    let mother = if !females.is_empty() {
        Some(females[rng.random_range(0..females.len())].entity)
    } else {
        None
    };

    (father, mother)
}

/// ECS-compatible name generation. Inherits surname from parent when possible,
/// checks uniqueness against collected living names.
fn generate_ecs_person_name(
    father: Option<Entity>,
    mother: Option<Entity>,
    persons: &Query<(Entity, &SimEntity, &PersonCore, Option<&LocatedIn>), With<Person>>,
    naming_style: Option<&crate::model::NamingStyle>,
    used_names: &BTreeMap<String, ()>,
    rng: &mut dyn rand::RngCore,
) -> String {
    // Try to inherit surname from father or mother
    let surname = father.or(mother).and_then(|parent_entity| {
        persons
            .get(parent_entity)
            .ok()
            .and_then(|(_, sim, _, _)| extract_surname(&sim.name))
            .map(|s| s.to_string())
    });

    match (surname, naming_style) {
        (Some(ref surname), Some(style)) => {
            generate_unique_with_surname_ecs(style, surname, used_names, rng)
        }
        (Some(ref surname), None) => {
            generate_unique_with_surname_generic_ecs(surname, used_names, rng)
        }
        (None, Some(style)) => generate_unique_culture_ecs(style, used_names, rng),
        (None, None) => generate_unique_generic_ecs(used_names, rng),
    }
}

fn generate_unique_generic_ecs(
    used_names: &BTreeMap<String, ()>,
    rng: &mut dyn rand::RngCore,
) -> String {
    for _ in 0..5 {
        let name = generate_person_name(rng);
        if !used_names.contains_key(&name) {
            return name;
        }
    }
    let base = generate_person_name(rng);
    let epithet = EPITHETS[rng.random_range(0..EPITHETS.len())];
    format!("{base} the {epithet}")
}

fn generate_unique_culture_ecs(
    style: &crate::model::NamingStyle,
    used_names: &BTreeMap<String, ()>,
    rng: &mut dyn rand::RngCore,
) -> String {
    for _ in 0..5 {
        let name = generate_culture_person_name(style, rng);
        if !used_names.contains_key(&name) {
            return name;
        }
    }
    let base = generate_culture_person_name(style, rng);
    let epithet = EPITHETS[rng.random_range(0..EPITHETS.len())];
    format!("{base} the {epithet}")
}

fn generate_unique_with_surname_generic_ecs(
    surname: &str,
    used_names: &BTreeMap<String, ()>,
    rng: &mut dyn rand::RngCore,
) -> String {
    use crate::sim::names::{FIRST_PREFIXES, FIRST_SUFFIXES};
    for _ in 0..5 {
        let prefix = FIRST_PREFIXES[rng.random_range(0..FIRST_PREFIXES.len())];
        let suffix = FIRST_SUFFIXES[rng.random_range(0..FIRST_SUFFIXES.len())];
        let name = format!("{prefix}{suffix} {surname}");
        if !used_names.contains_key(&name) {
            return name;
        }
    }
    let prefix = FIRST_PREFIXES[rng.random_range(0..FIRST_PREFIXES.len())];
    let suffix = FIRST_SUFFIXES[rng.random_range(0..FIRST_SUFFIXES.len())];
    let epithet = EPITHETS[rng.random_range(0..EPITHETS.len())];
    format!("{prefix}{suffix} {surname} the {epithet}")
}

fn generate_unique_with_surname_ecs(
    style: &crate::model::NamingStyle,
    surname: &str,
    used_names: &BTreeMap<String, ()>,
    rng: &mut dyn rand::RngCore,
) -> String {
    // Generate culture-specific first name + given surname
    // We need the culture tables, which are private in culture_names.
    // Fall back to generating a full culture name and replacing the surname portion.
    for _ in 0..5 {
        let full = generate_culture_person_name(style, rng);
        // Replace the generated surname with the inherited one
        let name = if let Some(idx) = full.rfind(' ') {
            format!("{} {surname}", &full[..idx])
        } else {
            format!("{full} {surname}")
        };
        if !used_names.contains_key(&name) {
            return name;
        }
    }
    let full = generate_culture_person_name(style, rng);
    let first = full.split(' ').next().unwrap_or(&full);
    let epithet = EPITHETS[rng.random_range(0..EPITHETS.len())];
    format!("{first} {surname} the {epithet}")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::app::build_sim_app_seeded;
    use crate::ecs::components::*;
    use crate::ecs::relationships::RegionAdjacency;
    use crate::ecs::resources::SimEntityMap;
    use crate::ecs::test_helpers::tick_years;
    use crate::ecs::time::SimTime;
    use crate::model::Terrain;
    use crate::model::population::PopulationBreakdown;

    fn setup_app() -> App {
        let mut app = build_sim_app_seeded(100, 42);
        // Advance ID generator past the IDs we use in tests (1000+)
        // so PersonBorn commands don't collide with test entity IDs.
        let mut id_gen = app
            .world_mut()
            .resource_mut::<crate::ecs::resources::EcsIdGenerator>();
        id_gen.0 = crate::id::IdGenerator::starting_from(2000);
        add_demographics_systems(&mut app);
        app
    }

    fn spawn_region(app: &mut App, sim_id: u64, terrain: Terrain) -> Entity {
        let entity = app
            .world_mut()
            .spawn((
                SimEntity {
                    id: sim_id,
                    name: "Region".to_string(),
                    origin: Some(SimTime::from_year(100)),
                    end: None,
                },
                Region,
                RegionState {
                    terrain,
                    ..RegionState::default()
                },
            ))
            .id();
        app.world_mut()
            .resource_mut::<SimEntityMap>()
            .insert(sim_id, entity);
        entity
    }

    fn spawn_faction(app: &mut App, sim_id: u64, treasury: f64) -> Entity {
        let entity = app
            .world_mut()
            .spawn((
                SimEntity {
                    id: sim_id,
                    name: "Kingdom".to_string(),
                    origin: Some(SimTime::from_year(100)),
                    end: None,
                },
                Faction,
                FactionCore {
                    treasury,
                    ..FactionCore::default()
                },
                FactionDiplomacy::default(),
                FactionMilitary::default(),
            ))
            .id();
        app.world_mut()
            .resource_mut::<SimEntityMap>()
            .insert(sim_id, entity);
        entity
    }

    fn spawn_settlement(
        app: &mut App,
        sim_id: u64,
        faction: Entity,
        region: Entity,
        population: u32,
    ) -> Entity {
        let entity = app
            .world_mut()
            .spawn((
                SimEntity {
                    id: sim_id,
                    name: "Town".to_string(),
                    origin: Some(SimTime::from_year(100)),
                    end: None,
                },
                Settlement,
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
            ))
            .id();
        app.world_mut()
            .entity_mut(entity)
            .insert((LocatedIn(region), MemberOf(faction)));
        app.world_mut()
            .resource_mut::<SimEntityMap>()
            .insert(sim_id, entity);
        entity
    }

    fn spawn_person(
        app: &mut App,
        sim_id: u64,
        name: &str,
        born_year: u32,
        sex: Sex,
        settlement: Entity,
        faction: Entity,
    ) -> Entity {
        let entity = app
            .world_mut()
            .spawn((
                SimEntity {
                    id: sim_id,
                    name: name.to_string(),
                    origin: Some(SimTime::from_year(born_year)),
                    end: None,
                },
                Person,
                PersonCore {
                    born: SimTime::from_year(born_year),
                    sex,
                    role: Role::Common,
                    traits: vec![],
                    last_action: SimTime::default(),
                    culture_id: None,
                    widowed_at: None,
                },
                PersonReputation::default(),
                PersonSocial::default(),
                PersonEducation::default(),
            ))
            .id();
        app.world_mut()
            .entity_mut(entity)
            .insert((LocatedIn(settlement), MemberOf(faction)));
        app.world_mut()
            .resource_mut::<SimEntityMap>()
            .insert(sim_id, entity);
        entity
    }

    #[test]
    fn capacity_computed_from_terrain() {
        let mut app = setup_app();
        let region = spawn_region(&mut app, 1001, Terrain::Plains);
        let faction = spawn_faction(&mut app, 1002, 100.0);
        let sett = spawn_settlement(&mut app, 1003, faction, region, 200);

        tick_years(&mut app, 1);

        let core = app.world().get::<SettlementCore>(sett).unwrap();
        // Plains upper bound = 800, * 5 = 4000
        assert!(
            core.capacity > 0,
            "capacity should be computed, got {}",
            core.capacity
        );
        assert!(
            core.capacity >= 2000,
            "plains should have high capacity, got {}",
            core.capacity
        );
    }

    #[test]
    fn population_grows_under_capacity() {
        let mut app = setup_app();
        let region = spawn_region(&mut app, 1001, Terrain::Plains);
        let faction = spawn_faction(&mut app, 1002, 100.0);
        let sett = spawn_settlement(&mut app, 1003, faction, region, 200);

        let pop_before = app.world().get::<SettlementCore>(sett).unwrap().population;

        tick_years(&mut app, 20);

        let pop_after = app.world().get::<SettlementCore>(sett).unwrap().population;

        // Population should have changed (grown or had births/deaths)
        assert_ne!(
            pop_before, pop_after,
            "population should change over 20 years"
        );
    }

    #[test]
    fn mortality_kills_old_persons() {
        let mut app = setup_app();
        let region = spawn_region(&mut app, 1001, Terrain::Plains);
        let faction = spawn_faction(&mut app, 1002, 100.0);
        let sett = spawn_settlement(&mut app, 1003, faction, region, 200);

        // Born in year 5 → age 95 at year 100 → high mortality
        let elder = spawn_person(&mut app, 1010, "Elder Ashford", 5, Sex::Male, sett, faction);

        tick_years(&mut app, 10);

        let sim = app.world().get::<SimEntity>(elder).unwrap();
        assert!(sim.end.is_some(), "95+ year old should likely be dead");
    }

    #[test]
    fn births_create_new_persons() {
        let mut app = setup_app();
        let region = spawn_region(&mut app, 1001, Terrain::Plains);
        let faction = spawn_faction(&mut app, 1002, 100.0);
        let sett = spawn_settlement(&mut app, 1003, faction, region, 500);

        let persons_before: usize = app
            .world_mut()
            .query_filtered::<&SimEntity, With<Person>>()
            .iter(app.world())
            .filter(|s| s.is_alive())
            .count();

        tick_years(&mut app, 5);

        let persons_after: usize = app
            .world_mut()
            .query_filtered::<&SimEntity, With<Person>>()
            .iter(app.world())
            .filter(|s| s.is_alive())
            .count();

        assert!(
            persons_after > persons_before,
            "should have born new persons (before={persons_before}, after={persons_after})"
        );
    }

    #[test]
    fn mortality_rate_increases_with_age() {
        let rates: Vec<f64> = [10, 30, 50, 70, 85, 95, 100]
            .iter()
            .map(|&age| mortality_rate(age))
            .collect();
        for window in rates.windows(2) {
            assert!(
                window[0] <= window[1],
                "mortality should increase: {} <= {}",
                window[0],
                window[1]
            );
        }
    }
}
