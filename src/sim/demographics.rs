use rand::{Rng, RngCore};

use super::context::TickContext;
use super::culture_names::{
    generate_culture_person_name_with_surname, generate_unique_culture_person_name,
};
use super::names::{
    extract_surname, generate_person_name_with_surname, generate_unique_person_name,
};
use super::signal::{Signal, SignalKind};
use super::system::{SimSystem, TickFrequency};
use crate::model::population::PopulationBreakdown;
use crate::model::traits::generate_traits;
use crate::model::{
    EntityData, EntityKind, EventKind, NamingStyle, ParticipantRole, PersonData, RelationshipKind,
    Role, Sex, SimTimestamp, World,
};
use crate::sim::helpers;

// --- Carrying capacity ---

/// Multiplier applied to a region's terrain population range upper bound.
const REGION_CAPACITY_MULTIPLIER: u32 = 5;

/// Default carrying capacity when a settlement has no region.
const DEFAULT_CAPACITY: u32 = 500;

/// Population supported per unit of food buffer (granary bonus).
const FOOD_BUFFER_POP_PER_UNIT: f64 = 50.0;

/// Extra carrying capacity for coastal settlements with a port.
const PORT_FISHING_CAPACITY: u32 = 200;

/// Extra carrying capacity for coastal settlements without a port.
const COASTAL_FISHING_CAPACITY: u32 = 50;

// --- Population thresholds ---

/// Settlements with population below this are abandoned.
const ABANDONMENT_THRESHOLD: u32 = 10;

/// Fractional change in population that triggers a PopulationChanged signal.
const SIGNIFICANT_POP_CHANGE_FRACTION: f64 = 0.10;

// --- Notable generation ---

/// Divisor for the target-notable sqrt formula: sqrt(pop / DIVISOR).
const NOTABLE_POP_DIVISOR: f64 = 5.0;

/// Minimum number of notable persons per settlement.
const MIN_TARGET_NOTABLES: u32 = 3;

/// Maximum number of notable persons per settlement.
const MAX_TARGET_NOTABLES: u32 = 25;

/// Maximum notable births per settlement per year.
const MAX_NOTABLE_BIRTHS_PER_YEAR: u32 = 2;

// --- Role weights for newborn notables ---

const ROLES: [Role; 6] = [
    Role::Common,
    Role::Artisan,
    Role::Warrior,
    Role::Merchant,
    Role::Scholar,
    Role::Elder,
];
const ROLE_WEIGHTS: [u32; 6] = [30, 20, 20, 15, 10, 5];

// --- Mortality rates by age bracket ---

const MORTALITY_INFANT: f64 = 0.03;
const MORTALITY_CHILD: f64 = 0.005;
const MORTALITY_YOUNG_ADULT: f64 = 0.008;
const MORTALITY_MIDDLE_AGE: f64 = 0.015;
const MORTALITY_ELDER: f64 = 0.04;
const MORTALITY_AGED: f64 = 0.10;
const MORTALITY_ANCIENT: f64 = 0.25;
const MORTALITY_CENTENARIAN: f64 = 1.0;

// --- Age thresholds ---

/// Minimum age to be considered an adult (for marriage and parenthood).
const ADULT_AGE: u32 = 16;

// --- Marriage parameters ---

/// Probability per settlement per year that an intra-settlement marriage occurs.
const INTRA_SETTLEMENT_MARRIAGE_CHANCE: f64 = 0.15;

/// Probability per tick that a cross-faction marriage is attempted.
const CROSS_FACTION_MARRIAGE_CHANCE: f64 = 0.05;

/// Probability that a cross-faction marriage creates a new alliance.
const CROSS_FACTION_ALLIANCE_CHANCE: f64 = 0.5;

/// Years a widowed person must wait before remarrying.
const WIDOWED_REMARRIAGE_COOLDOWN: u32 = 3;

pub struct DemographicsSystem;

impl SimSystem for DemographicsSystem {
    fn name(&self) -> &str {
        "demographics"
    }

    fn frequency(&self) -> TickFrequency {
        TickFrequency::Yearly
    }

    fn tick(&mut self, ctx: &mut TickContext) {
        let time = ctx.world.current_time;

        // Create a shared year-tick event for population updates
        let year_event = ctx.world.add_event(
            EventKind::Custom("year_tick".to_string()),
            time,
            format!("Year {} demographics tick", time.year()),
        );

        let settlements = compute_capacity(ctx);
        grow_population(ctx, &settlements, time, year_event);
        process_mortality(ctx, time);
        process_births(ctx, time);
        process_marriages(ctx, time);
    }
}

// --- Helper structs ---

struct SettlementInfo {
    id: u64,
    breakdown: PopulationBreakdown,
    capacity: u32,
}

struct PopUpdate {
    settlement_id: u64,
    old_pop: u32,
    new_breakdown: PopulationBreakdown,
    abandon: bool,
}

struct PersonInfo {
    id: u64,
    born: SimTimestamp,
    settlement_id: Option<u64>,
    is_leader: bool,
}

struct DeathInfo {
    person_id: u64,
    settlement_id: Option<u64>,
    is_leader: bool,
}

struct SettlementBirthInfo {
    id: u64,
    population: u32,
}

struct LivingPersonInfo {
    id: u64,
    settlement_id: Option<u64>,
    sex: Sex,
    born: SimTimestamp,
    spouse_id: Option<u64>,
}

struct BirthPlan {
    settlement_id: u64,
    count: u32,
}

// --- Tick sub-functions ---

/// Compute carrying capacity for each living settlement based on region terrain,
/// building bonuses, and seasonal modifiers. Stores capacity as an extra on each
/// settlement for use by other systems.
fn compute_capacity(ctx: &mut TickContext) -> Vec<SettlementInfo> {
    // Collect region terrain data for carrying capacity
    let region_capacities: Vec<(u64, u32)> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Region)
        .filter_map(|e| {
            let region = e.data.as_region()?;
            let profile = crate::worldgen::terrain::TerrainProfile::new(
                region.terrain,
                region.terrain_tags.clone(),
            );
            let capacity = profile.effective_population_range().1 * REGION_CAPACITY_MULTIPLIER;
            Some((e.id, capacity))
        })
        .collect();

    // Collect settlement data
    let settlements: Vec<SettlementInfo> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
        .filter_map(|e| {
            let settlement = e.data.as_settlement()?;
            let breakdown = settlement.population_breakdown.clone();

            let region_id = e.active_rel(RelationshipKind::LocatedIn);

            let base_capacity = region_capacities
                .iter()
                .find(|(id, _)| Some(*id) == region_id)
                .map(|(_, cap)| *cap)
                .unwrap_or(DEFAULT_CAPACITY);

            // Building bonuses from BuildingSystem
            let sd = e.data.as_settlement();
            let capacity_bonus = sd.map(|s| s.building_bonuses.capacity).unwrap_or(0.0);
            // Granary food buffer acts as extra effective capacity (reduces starvation)
            let food_buffer = sd.map(|s| s.building_bonuses.food_buffer).unwrap_or(0.0);
            let food_buffer_capacity = (food_buffer * FOOD_BUFFER_POP_PER_UNIT) as u32;

            // Fishing capacity: coastal settlements get extra pop capacity
            let is_coastal = sd.is_some_and(|s| s.is_coastal);
            let has_port = sd.is_some_and(|s| s.building_bonuses.port_trade > 0.0);
            let fishing_cap = match (is_coastal, has_port) {
                (true, true) => PORT_FISHING_CAPACITY,
                (true, false) => COASTAL_FISHING_CAPACITY,
                _ => 0,
            };

            // Seasonal food modifier reduces effective capacity in winter/droughts
            let season_food_annual = sd.map(|s| s.seasonal.food_annual).unwrap_or(1.0);
            let raw_capacity =
                base_capacity + capacity_bonus as u32 + food_buffer_capacity + fishing_cap;
            let capacity = (raw_capacity as f64 * season_food_annual) as u32;

            Some(SettlementInfo {
                id: e.id,
                breakdown,
                capacity,
            })
        })
        .collect();

    // Store effective capacity on settlement struct field for other systems (economy, etc.)
    for s in &settlements {
        ctx.world.settlement_mut(s.id).capacity = s.capacity;
    }

    settlements
}

/// Apply bracket-based population growth to each settlement. Abandons settlements
/// that fall below the minimum population threshold.
fn grow_population(
    ctx: &mut TickContext,
    settlements: &[SettlementInfo],
    time: SimTimestamp,
    year_event: u64,
) {
    let mut pop_updates: Vec<PopUpdate> = Vec::new();
    for s in settlements {
        let capacity = s.capacity;

        let old_pop = s.breakdown.total();
        let mut breakdown = s.breakdown.clone();
        breakdown.tick_year(capacity, ctx.rng);
        let new_pop = breakdown.total();

        pop_updates.push(PopUpdate {
            settlement_id: s.id,
            old_pop,
            new_breakdown: breakdown,
            abandon: new_pop < ABANDONMENT_THRESHOLD,
        });
    }

    // Apply population updates
    for update in &pop_updates {
        if update.abandon {
            let ev = ctx.world.add_event(
                EventKind::Abandoned,
                time,
                "Settlement abandoned due to population collapse".to_string(),
            );
            ctx.world
                .add_event_participant(ev, update.settlement_id, ParticipantRole::Subject);
            ctx.world.end_entity(update.settlement_id, time, ev);
        } else {
            let new_pop = update.new_breakdown.total();
            // Mutate typed fields on SettlementData
            {
                let entity = ctx.world.entities.get_mut(&update.settlement_id).unwrap();
                let settlement = entity.data.as_settlement_mut().unwrap();
                settlement.population = new_pop;
                settlement.population_breakdown = update.new_breakdown.clone();
            }
            ctx.world.record_change(
                update.settlement_id,
                year_event,
                "population",
                serde_json::json!(update.old_pop),
                serde_json::json!(new_pop),
            );

            // Emit signal for significant changes (>10%)
            if update.old_pop > 0 {
                let change_pct =
                    (new_pop as f64 - update.old_pop as f64).abs() / update.old_pop as f64;
                if change_pct > SIGNIFICANT_POP_CHANGE_FRACTION {
                    ctx.signals.push(Signal {
                        event_id: year_event,
                        kind: SignalKind::PopulationChanged {
                            settlement_id: update.settlement_id,
                            old: update.old_pop,
                            new: new_pop,
                        },
                    });
                }
            }
        }
    }
}

/// Roll mortality checks for all living persons and apply deaths. Handles leader
/// vacancy signals, spouse widowing, and relationship cleanup.
fn process_mortality(ctx: &mut TickContext, time: SimTimestamp) {
    let persons: Vec<PersonInfo> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Person && e.end.is_none())
        .filter_map(|e| {
            let person = e.data.as_person()?;
            let settlement_id = e.active_rel(RelationshipKind::LocatedIn);
            let is_leader = e.active_rel(RelationshipKind::LeaderOf).is_some();
            Some(PersonInfo {
                id: e.id,
                born: person.born,
                settlement_id,
                is_leader,
            })
        })
        .collect();

    let mut deaths: Vec<DeathInfo> = Vec::new();
    for person in &persons {
        let age = time.years_since(person.born);
        let mortality = mortality_rate(age);
        let roll: f64 = ctx.rng.random_range(0.0..1.0);
        if roll < mortality {
            deaths.push(DeathInfo {
                person_id: person.id,
                settlement_id: person.settlement_id,
                is_leader: person.is_leader,
            });
        }
    }

    // Apply deaths
    for death in &deaths {
        let person_name = ctx
            .world
            .entities
            .get(&death.person_id)
            .map(|e| e.name.clone())
            .unwrap_or_else(|| format!("entity {}", death.person_id));
        let ev = ctx.world.add_event(
            EventKind::Death,
            time,
            format!("{person_name} died in year {}", time.year()),
        );
        ctx.world
            .add_event_participant(ev, death.person_id, ParticipantRole::Subject);
        if let Some(sid) = death.settlement_id {
            ctx.world
                .add_event_participant(ev, sid, ParticipantRole::Location);
        }

        // Collect spouse IDs before ending relationships
        let spouse_ids: Vec<u64> = ctx
            .world
            .entities
            .get(&death.person_id)
            .map(|e| e.active_rels(RelationshipKind::Spouse).collect())
            .unwrap_or_default();

        // End LocatedIn, MemberOf, and Spouse relationships on the dying person
        end_person_relationships(ctx.world, death.person_id, time, ev);

        // End the reverse Spouse relationship on surviving spouses and set widowed_year
        for spouse_id in &spouse_ids {
            // End reverse Spouse rel
            if ctx
                .world
                .entities
                .get(spouse_id)
                .is_some_and(|e| e.has_active_rel(RelationshipKind::Spouse, death.person_id))
            {
                ctx.world.end_relationship(
                    *spouse_id,
                    death.person_id,
                    RelationshipKind::Spouse,
                    time,
                    ev,
                );
            }
            // Set widowed_at for remarriage cooldown
            ctx.world.person_mut(*spouse_id).widowed_at = Some(time);
        }

        // If leader, end LeaderOf and emit vacancy signal
        if death.is_leader
            && let Some(leader_target) = find_leader_target(ctx.world, death.person_id)
        {
            ctx.world.end_relationship(
                death.person_id,
                leader_target,
                RelationshipKind::LeaderOf,
                time,
                ev,
            );
            ctx.signals.push(Signal {
                event_id: ev,
                kind: SignalKind::LeaderVacancy {
                    faction_id: leader_target,
                    previous_leader_id: death.person_id,
                },
            });
        }

        ctx.world.end_entity(death.person_id, time, ev);

        ctx.signals.push(Signal {
            event_id: ev,
            kind: SignalKind::EntityDied {
                entity_id: death.person_id,
            },
        });
    }
}

/// Generate notable person entities in settlements that are below their target
/// notable count. Handles parent selection, name generation, role assignment,
/// and relationship wiring.
fn process_births(ctx: &mut TickContext, time: SimTimestamp) {
    // Re-collect living settlements (some may have been abandoned)
    let living_settlements: Vec<SettlementBirthInfo> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
        .filter_map(|e| {
            let settlement = e.data.as_settlement()?;
            Some(SettlementBirthInfo {
                id: e.id,
                population: settlement.population,
            })
        })
        .collect();

    // Count living notables per settlement (with info for parent selection)
    let living_persons: Vec<LivingPersonInfo> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Person && e.end.is_none())
        .filter_map(|e| {
            let person = e.data.as_person()?;
            let settlement_id = e.active_rel(RelationshipKind::LocatedIn);
            let spouse_id = e.active_rel(RelationshipKind::Spouse);
            Some(LivingPersonInfo {
                id: e.id,
                settlement_id,
                sex: person.sex,
                born: person.born,
                spouse_id,
            })
        })
        .collect();

    let mut birth_plans: Vec<BirthPlan> = Vec::new();
    for s in &living_settlements {
        let target_notables = ((s.population as f64 / NOTABLE_POP_DIVISOR).sqrt().round() as u32)
            .clamp(MIN_TARGET_NOTABLES, MAX_TARGET_NOTABLES);
        let current_notables = living_persons
            .iter()
            .filter(|p| p.settlement_id == Some(s.id))
            .count() as u32;
        if current_notables < target_notables {
            let births = (target_notables - current_notables).min(MAX_NOTABLE_BIRTHS_PER_YEAR);
            birth_plans.push(BirthPlan {
                settlement_id: s.id,
                count: births,
            });
        }
    }

    // Apply births
    for plan in &birth_plans {
        for _ in 0..plan.count {
            // Find parents for surname inheritance and relationships
            let (father_id, mother_id) =
                find_parents(&living_persons, plan.settlement_id, time, ctx.rng);

            // Look up settlement's dominant culture and naming style
            let (settlement_culture_id, naming_style) = ctx
                .world
                .entities
                .get(&plan.settlement_id)
                .and_then(|e| e.data.as_settlement())
                .and_then(|sd| sd.dominant_culture)
                .and_then(|cid| {
                    ctx.world
                        .entities
                        .get(&cid)
                        .and_then(|e| e.data.as_culture())
                        .map(|cd| (cid, cd.naming_style.clone()))
                })
                .unzip();

            // Generate name — inherit surname from father (or mother) if possible
            let name = generate_person_name(
                ctx.world,
                father_id,
                mother_id,
                naming_style.as_ref(),
                ctx.rng,
            );

            // Weighted role selection (Scholar weight boosted by settlement literacy)
            let literacy = helpers::settlement_literacy(ctx.world, plan.settlement_id);
            let scholar_boost = (literacy * 10.0) as u32;
            let mut adjusted_weights = ROLE_WEIGHTS;
            adjusted_weights[4] += scholar_boost; // index 4 = Scholar
            let adj_weight_total: u32 = adjusted_weights.iter().sum();
            let roll = ctx.rng.random_range(0..adj_weight_total);
            let mut cumulative = 0;
            let mut selected_role = ROLES[0].clone();
            for (i, &w) in adjusted_weights.iter().enumerate() {
                cumulative += w;
                if roll < cumulative {
                    selected_role = ROLES[i].clone();
                    break;
                }
            }

            // Random sex
            let sex = if ctx.rng.random_bool(0.5) {
                Sex::Male
            } else {
                Sex::Female
            };

            // Generate personality traits
            let traits = generate_traits(&selected_role, ctx.rng);

            let ev = ctx.world.add_event(
                EventKind::Birth,
                time,
                format!("{name} born in year {}", time.year()),
            );

            let person_id = ctx.world.add_entity(
                EntityKind::Person,
                name,
                Some(time),
                EntityData::Person(PersonData {
                    born: time,
                    sex,
                    role: selected_role,
                    traits,
                    last_action: SimTimestamp::default(),
                    culture_id: settlement_culture_id,
                    prestige: 0.0,
                    grievances: std::collections::BTreeMap::new(),
                    secrets: std::collections::BTreeMap::new(),
                    claims: std::collections::BTreeMap::new(),
                    widowed_at: None,
                    prestige_tier: 0,
                    loyalty: std::collections::BTreeMap::new(),
                    education: 0.0,
                }),
                ev,
            );

            ctx.world
                .add_event_participant(ev, person_id, ParticipantRole::Subject);
            ctx.world
                .add_event_participant(ev, plan.settlement_id, ParticipantRole::Location);

            // Wire parent-child relationships
            if let Some(fid) = father_id {
                ctx.world
                    .add_relationship(fid, person_id, RelationshipKind::Parent, time, ev);
                ctx.world
                    .add_relationship(person_id, fid, RelationshipKind::Child, time, ev);
                ctx.world
                    .add_event_participant(ev, fid, ParticipantRole::Parent);
            }
            if let Some(mid) = mother_id {
                ctx.world
                    .add_relationship(mid, person_id, RelationshipKind::Parent, time, ev);
                ctx.world
                    .add_relationship(person_id, mid, RelationshipKind::Child, time, ev);
                ctx.world
                    .add_event_participant(ev, mid, ParticipantRole::Parent);
            }

            // Relationships
            ctx.world.add_relationship(
                person_id,
                plan.settlement_id,
                RelationshipKind::LocatedIn,
                time,
                ev,
            );
            ctx.world.add_relationship(
                person_id,
                plan.settlement_id,
                RelationshipKind::MemberOf,
                time,
                ev,
            );

            // Also join the settlement's faction
            if let Some(faction_id) = helpers::settlement_faction(ctx.world, plan.settlement_id) {
                ctx.world.add_relationship(
                    person_id,
                    faction_id,
                    RelationshipKind::MemberOf,
                    time,
                    ev,
                );
            }
        }
    }
}

// --- Helper functions ---

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

/// Generate a person name, inheriting surname from a parent when possible
/// and using a culture-specific naming style when available.
fn generate_person_name(
    world: &World,
    father_id: Option<u64>,
    mother_id: Option<u64>,
    naming_style: Option<&NamingStyle>,
    rng: &mut dyn RngCore,
) -> String {
    // Try to inherit surname from father (preferred) or mother
    let surname = father_id.or(mother_id).and_then(|parent_id| {
        let parent_name = world
            .entities
            .get(&parent_id)
            .map(|e| e.name.as_str())
            .unwrap_or("");
        extract_surname(parent_name)
    });

    match (surname, naming_style) {
        (Some(surname), Some(style)) => {
            generate_culture_person_name_with_surname(world, style, rng, surname)
        }
        (Some(surname), None) => generate_person_name_with_surname(world, rng, surname),
        (None, Some(style)) => generate_unique_culture_person_name(world, style, rng),
        (None, None) => generate_unique_person_name(world, rng),
    }
}

fn end_person_relationships(
    world: &mut crate::model::World,
    person_id: u64,
    time: SimTimestamp,
    event_id: u64,
) {
    // Collect relationship targets before mutating
    // End LocatedIn, MemberOf, and Spouse — but NOT Parent/Child (permanent genealogical facts)
    let rels: Vec<(u64, RelationshipKind)> = world
        .entities
        .get(&person_id)
        .map(|e| {
            e.relationships
                .iter()
                .filter(|r| {
                    r.end.is_none()
                        && matches!(
                            r.kind,
                            RelationshipKind::LocatedIn
                                | RelationshipKind::MemberOf
                                | RelationshipKind::Spouse
                        )
                })
                .map(|r| (r.target_entity_id, r.kind.clone()))
                .collect()
        })
        .unwrap_or_default();

    for (target_id, kind) in rels {
        world.end_relationship(person_id, target_id, kind, time, event_id);
    }
}

struct MarriageCandidate {
    id: u64,
    sex: Sex,
    faction_id: Option<u64>,
    settlement_id: u64,
}

struct MarriagePlan {
    spouse_a: u64,
    spouse_b: u64,
    settlement_id: u64,
    cross_faction: bool,
    faction_a: Option<u64>,
    faction_b: Option<u64>,
}

fn process_marriages(ctx: &mut TickContext, time: SimTimestamp) {
    let by_settlement = collect_marriage_candidates(ctx.world, time);
    let marriages = plan_marriages(&by_settlement, ctx.world, ctx.rng);
    apply_marriages(&marriages, ctx, time);
}

/// Collect unmarried adults grouped by settlement (BTreeMap for deterministic order).
fn collect_marriage_candidates(
    world: &World,
    time: SimTimestamp,
) -> std::collections::BTreeMap<u64, Vec<MarriageCandidate>> {
    let mut by_settlement: std::collections::BTreeMap<u64, Vec<MarriageCandidate>> =
        std::collections::BTreeMap::new();

    for e in world.entities.values() {
        if e.kind != EntityKind::Person || e.end.is_some() {
            continue;
        }
        let Some(person) = e.data.as_person() else {
            continue;
        };
        if time.years_since(person.born) < ADULT_AGE {
            continue;
        }
        // Skip if already married
        if e.active_rel(RelationshipKind::Spouse).is_some() {
            continue;
        }
        // Skip if recently widowed (cooldown)
        if let Some(widowed_at) = person.widowed_at
            && time.years_since(widowed_at) < WIDOWED_REMARRIAGE_COOLDOWN
        {
            continue;
        }
        let Some(sid) = e.active_rel(RelationshipKind::LocatedIn) else {
            continue;
        };
        let sex = person.sex;
        let faction_id = e.active_rels(RelationshipKind::MemberOf).find(|&id| {
            world
                .entities
                .get(&id)
                .is_some_and(|t| t.kind == EntityKind::Faction)
        });

        by_settlement
            .entry(sid)
            .or_default()
            .push(MarriageCandidate {
                id: e.id,
                sex,
                faction_id,
                settlement_id: sid,
            });
    }

    by_settlement
}

/// Plan intra-settlement and cross-faction marriages from candidates.
fn plan_marriages(
    by_settlement: &std::collections::BTreeMap<u64, Vec<MarriageCandidate>>,
    world: &World,
    rng: &mut dyn RngCore,
) -> Vec<MarriagePlan> {
    let mut marriages: Vec<MarriagePlan> = Vec::new();

    // Intra-settlement marriages: 15% chance per settlement, max 1 per year
    for (sid, candidates) in by_settlement {
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
                spouse_a: groom.id,
                spouse_b: bride.id,
                settlement_id: *sid,
                cross_faction: false,
                faction_a: groom.faction_id,
                faction_b: bride.faction_id,
            });
        }
    }

    // Cross-faction marriage
    if rng.random_range(0.0..1.0) < CROSS_FACTION_MARRIAGE_CHANCE {
        // Collect all factions
        let faction_ids: Vec<u64> = world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
            .map(|e| e.id)
            .collect();

        if faction_ids.len() >= 2 {
            // Pick two random non-enemy, non-at-war factions
            let idx_a = rng.random_range(0..faction_ids.len());
            let mut idx_b = rng.random_range(0..faction_ids.len() - 1);
            if idx_b >= idx_a {
                idx_b += 1;
            }
            let fa = faction_ids[idx_a];
            let fb = faction_ids[idx_b];

            // Check they're not enemies or at war
            let hostile = world.entities.get(&fa).is_some_and(|e| {
                e.relationships.iter().any(|r| {
                    r.end.is_none()
                        && r.target_entity_id == fb
                        && matches!(r.kind, RelationshipKind::Enemy | RelationshipKind::AtWar)
                })
            });

            if !hostile {
                // Find an unmarried adult from each faction (from all candidates)
                let all_candidates: Vec<&MarriageCandidate> =
                    by_settlement.values().flat_map(|v| v.iter()).collect();
                let cand_a: Vec<&&MarriageCandidate> = all_candidates
                    .iter()
                    .filter(|c| c.faction_id == Some(fa))
                    .collect();
                let cand_b: Vec<&&MarriageCandidate> = all_candidates
                    .iter()
                    .filter(|c| c.faction_id == Some(fb))
                    .collect();

                if !cand_a.is_empty() && !cand_b.is_empty() {
                    let a = cand_a[rng.random_range(0..cand_a.len())];
                    let b = cand_b[rng.random_range(0..cand_b.len())];
                    // Ensure they're different people and different sexes
                    if a.id != b.id && a.sex != b.sex {
                        marriages.push(MarriagePlan {
                            spouse_a: a.id,
                            spouse_b: b.id,
                            settlement_id: a.settlement_id,
                            cross_faction: true,
                            faction_a: Some(fa),
                            faction_b: Some(fb),
                        });
                    }
                }
            }
        }
    }

    marriages
}

/// Apply planned marriages: create events, relationships, and diplomacy.
fn apply_marriages(marriages: &[MarriagePlan], ctx: &mut TickContext, time: SimTimestamp) {
    for marriage in marriages {
        let name_a = ctx
            .world
            .entities
            .get(&marriage.spouse_a)
            .map(|e| e.name.clone())
            .unwrap_or_default();
        let name_b = ctx
            .world
            .entities
            .get(&marriage.spouse_b)
            .map(|e| e.name.clone())
            .unwrap_or_default();

        let desc = if marriage.cross_faction {
            let fa_name = marriage
                .faction_a
                .and_then(|id| ctx.world.entities.get(&id))
                .map(|e| e.name.clone())
                .unwrap_or_default();
            let fb_name = marriage
                .faction_b
                .and_then(|id| ctx.world.entities.get(&id))
                .map(|e| e.name.clone())
                .unwrap_or_default();
            format!("{name_a} and {name_b} married, forging ties between {fa_name} and {fb_name}")
        } else {
            format!("{name_a} and {name_b} married in year {}", time.year())
        };

        let ev = ctx.world.add_event(EventKind::Union, time, desc);
        ctx.world
            .add_event_participant(ev, marriage.spouse_a, ParticipantRole::Subject);
        ctx.world
            .add_event_participant(ev, marriage.spouse_b, ParticipantRole::Object);
        ctx.world
            .add_event_participant(ev, marriage.settlement_id, ParticipantRole::Location);

        // Bidirectional Spouse relationships
        ctx.world.add_relationship(
            marriage.spouse_a,
            marriage.spouse_b,
            RelationshipKind::Spouse,
            time,
            ev,
        );
        ctx.world.add_relationship(
            marriage.spouse_b,
            marriage.spouse_a,
            RelationshipKind::Spouse,
            time,
            ev,
        );

        // Cross-faction marriage diplomacy
        if marriage.cross_faction
            && let (Some(fa), Some(fb)) = (marriage.faction_a, marriage.faction_b)
        {
            // Check if already allies
            let already_allies = ctx
                .world
                .entities
                .get(&fa)
                .is_some_and(|e| e.has_active_rel(RelationshipKind::Ally, fb));

            if already_allies {
                // Strengthen existing alliance with pair-specific marriage year
                ctx.world
                    .faction_mut(fa)
                    .marriage_alliances
                    .insert(fb, time.year());
                ctx.world
                    .faction_mut(fb)
                    .marriage_alliances
                    .insert(fa, time.year());
            } else if ctx.rng.random_bool(CROSS_FACTION_ALLIANCE_CHANCE) {
                ctx.world
                    .add_relationship(fa, fb, RelationshipKind::Ally, time, ev);
                ctx.world
                    .faction_mut(fa)
                    .marriage_alliances
                    .insert(fb, time.year());
                ctx.world
                    .faction_mut(fb)
                    .marriage_alliances
                    .insert(fa, time.year());
            }
        }
    }
}

fn find_parents(
    living: &[LivingPersonInfo],
    settlement_id: u64,
    time: SimTimestamp,
    rng: &mut dyn rand::RngCore,
) -> (Option<u64>, Option<u64>) {
    let adults: Vec<&LivingPersonInfo> = living
        .iter()
        .filter(|p| p.settlement_id == Some(settlement_id) && time.years_since(p.born) >= ADULT_AGE)
        .collect();

    let males: Vec<&LivingPersonInfo> = adults
        .iter()
        .filter(|p| p.sex == Sex::Male)
        .copied()
        .collect();
    let females: Vec<&LivingPersonInfo> = adults
        .iter()
        .filter(|p| p.sex == Sex::Female)
        .copied()
        .collect();

    // Try to find a married couple in this settlement
    let mut couples: Vec<(u64, u64)> = Vec::new();
    for m in &males {
        if let Some(spouse_id) = m.spouse_id
            && females.iter().any(|f| f.id == spouse_id)
        {
            couples.push((m.id, spouse_id));
        }
    }

    if !couples.is_empty() {
        let idx = rng.random_range(0..couples.len());
        return (Some(couples[idx].0), Some(couples[idx].1));
    }

    // Fallback: pick a random male and random female
    let father = if !males.is_empty() {
        Some(males[rng.random_range(0..males.len())].id)
    } else {
        None
    };
    let mother = if !females.is_empty() {
        Some(females[rng.random_range(0..females.len())].id)
    } else {
        None
    };

    (father, mother)
}

fn find_leader_target(world: &crate::model::World, person_id: u64) -> Option<u64> {
    world
        .entities
        .get(&person_id)
        .and_then(|e| e.active_rel(RelationshipKind::LeaderOf))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mortality_rate_increases_after_childhood() {
        // After childhood (age 6+), mortality increases with age
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
        // Infant mortality is higher than childhood
        assert!(mortality_rate(0) > mortality_rate(10));
    }

    #[test]
    fn mortality_100_is_certain() {
        assert_eq!(mortality_rate(100), 1.0);
        assert_eq!(mortality_rate(150), 1.0);
    }

    #[test]
    fn find_parents_returns_married_couple() {
        use rand::SeedableRng;
        use rand::rngs::SmallRng;

        let living = vec![
            LivingPersonInfo {
                id: 1,
                settlement_id: Some(100),
                sex: Sex::Male,
                born: SimTimestamp::from_year(10),
                spouse_id: Some(2),
            },
            LivingPersonInfo {
                id: 2,
                settlement_id: Some(100),
                sex: Sex::Female,
                born: SimTimestamp::from_year(12),
                spouse_id: Some(1),
            },
            LivingPersonInfo {
                id: 3,
                settlement_id: Some(100),
                sex: Sex::Male,
                born: SimTimestamp::from_year(15),
                spouse_id: None,
            },
        ];

        let mut rng = SmallRng::seed_from_u64(42);
        let (father, mother) = find_parents(&living, 100, SimTimestamp::from_year(50), &mut rng);
        assert_eq!(father, Some(1), "married male should be picked as father");
        assert_eq!(mother, Some(2), "married female should be picked as mother");
    }

    #[test]
    fn find_parents_returns_none_for_empty_settlement() {
        use rand::SeedableRng;
        use rand::rngs::SmallRng;

        let living: Vec<LivingPersonInfo> = vec![];
        let mut rng = SmallRng::seed_from_u64(42);
        let (father, mother) = find_parents(&living, 100, SimTimestamp::from_year(50), &mut rng);
        assert_eq!(father, None);
        assert_eq!(mother, None);
    }

    #[test]
    fn find_parents_skips_children() {
        use rand::SeedableRng;
        use rand::rngs::SmallRng;

        // Only children (age < 16) in the settlement
        let living = vec![
            LivingPersonInfo {
                id: 1,
                settlement_id: Some(100),
                sex: Sex::Male,
                born: SimTimestamp::from_year(40),
                spouse_id: None,
            },
            LivingPersonInfo {
                id: 2,
                settlement_id: Some(100),
                sex: Sex::Female,
                born: SimTimestamp::from_year(42),
                spouse_id: None,
            },
        ];

        let mut rng = SmallRng::seed_from_u64(42);
        let (father, mother) = find_parents(&living, 100, SimTimestamp::from_year(50), &mut rng);
        assert_eq!(father, None, "children should not be parents");
        assert_eq!(mother, None, "children should not be parents");
    }
}
