use rand::Rng;

use super::context::TickContext;
use super::culture_names::{
    generate_culture_person_name_with_surname, generate_unique_culture_person_name,
};
use super::names::{
    extract_surname, generate_person_name_with_surname, generate_unique_person_name,
};
use super::population::PopulationBreakdown;
use super::signal::{Signal, SignalKind};
use super::system::{SimSystem, TickFrequency};
use crate::model::traits::generate_traits;
use crate::model::{
    EntityData, EntityKind, EventKind, ParticipantRole, PersonData, RelationshipKind, SimTimestamp,
};
use crate::worldgen::terrain::{Terrain, TerrainTag};

pub struct DemographicsSystem;

impl SimSystem for DemographicsSystem {
    fn name(&self) -> &str {
        "demographics"
    }

    fn frequency(&self) -> TickFrequency {
        TickFrequency::Yearly
    }

    fn tick(&mut self, ctx: &mut TickContext) {
        let current_year = ctx.world.current_time.year();
        let time = ctx.world.current_time;

        // Create a shared year-tick event for population updates
        let year_event = ctx.world.add_event(
            EventKind::Custom("year_tick".to_string()),
            time,
            format!("Year {current_year} demographics tick"),
        );

        // --- Collect region terrain data for carrying capacity ---
        let region_capacities: Vec<(u64, u32)> = ctx
            .world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Region)
            .filter_map(|e| {
                let region = e.data.as_region()?;
                let terrain = Terrain::try_from(region.terrain.clone()).unwrap_or(Terrain::Plains);
                let tags: Vec<TerrainTag> = region
                    .terrain_tags
                    .iter()
                    .filter_map(|s| TerrainTag::try_from(s.clone()).ok())
                    .collect();
                let profile = crate::worldgen::terrain::TerrainProfile::new(terrain, tags);
                let capacity = profile.effective_population_range().1 * 5;
                Some((e.id, capacity))
            })
            .collect();

        // --- Collect settlement data ---
        let settlements: Vec<SettlementInfo> = ctx
            .world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
            .filter_map(|e| {
                let settlement = e.data.as_settlement()?;
                let breakdown = settlement.population_breakdown.clone();

                let region_id = e
                    .relationships
                    .iter()
                    .find(|r| r.kind == RelationshipKind::LocatedIn && r.end.is_none())
                    .map(|r| r.target_entity_id);

                let base_capacity = region_capacities
                    .iter()
                    .find(|(id, _)| Some(*id) == region_id)
                    .map(|(_, cap)| *cap)
                    .unwrap_or(500);

                // Building bonuses from BuildingSystem
                let capacity_bonus = e
                    .extra
                    .get("building_capacity_bonus")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                // Granary food buffer acts as extra effective capacity (reduces starvation)
                let food_buffer = e
                    .extra
                    .get("building_food_buffer")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let food_buffer_capacity = (food_buffer * 50.0) as u32; // Each unit of buffer supports ~50 people
                let capacity = base_capacity + capacity_bonus as u32 + food_buffer_capacity;

                Some(SettlementInfo {
                    id: e.id,
                    breakdown,
                    capacity,
                })
            })
            .collect();

        // Store effective capacity as an extra for other systems (economy, etc.)
        for s in &settlements {
            ctx.world.set_extra(
                s.id,
                "capacity".to_string(),
                serde_json::json!(s.capacity),
                year_event,
            );
        }

        // --- 3a: Population growth (bracket-based) ---
        struct PopUpdate {
            settlement_id: u64,
            old_pop: u32,
            new_breakdown: PopulationBreakdown,
            abandon: bool,
        }

        let mut pop_updates: Vec<PopUpdate> = Vec::new();
        for s in &settlements {
            let capacity = s.capacity;

            let old_pop = s.breakdown.total();
            let mut breakdown = s.breakdown.clone();
            breakdown.tick_year(capacity, ctx.rng);
            let new_pop = breakdown.total();

            if new_pop < 10 {
                pop_updates.push(PopUpdate {
                    settlement_id: s.id,
                    old_pop,
                    new_breakdown: breakdown,
                    abandon: true,
                });
            } else {
                pop_updates.push(PopUpdate {
                    settlement_id: s.id,
                    old_pop,
                    new_breakdown: breakdown,
                    abandon: false,
                });
            }
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
                    if change_pct > 0.10 {
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

        // --- 3b: NPC mortality ---
        let persons: Vec<PersonInfo> = ctx
            .world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Person && e.end.is_none())
            .filter_map(|e| {
                let person = e.data.as_person()?;
                let settlement_id = e
                    .relationships
                    .iter()
                    .find(|r| r.kind == RelationshipKind::LocatedIn && r.end.is_none())
                    .map(|r| r.target_entity_id);
                let is_leader = e
                    .relationships
                    .iter()
                    .any(|r| r.kind == RelationshipKind::LeaderOf && r.end.is_none());
                Some(PersonInfo {
                    id: e.id,
                    birth_year: person.birth_year,
                    settlement_id,
                    is_leader,
                })
            })
            .collect();

        let mut deaths: Vec<DeathInfo> = Vec::new();
        for person in &persons {
            let age = current_year.saturating_sub(person.birth_year);
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
                format!("{person_name} died in year {current_year}"),
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
                .map(|e| {
                    e.relationships
                        .iter()
                        .filter(|r| r.kind == RelationshipKind::Spouse && r.end.is_none())
                        .map(|r| r.target_entity_id)
                        .collect()
                })
                .unwrap_or_default();

            // End LocatedIn, MemberOf, and Spouse relationships on the dying person
            end_person_relationships(ctx.world, death.person_id, time, ev);

            // End the reverse Spouse relationship on surviving spouses and set widowed_year
            for spouse_id in &spouse_ids {
                // End reverse Spouse rel
                if ctx.world.entities.get(spouse_id).is_some_and(|e| {
                    e.relationships.iter().any(|r| {
                        r.kind == RelationshipKind::Spouse
                            && r.target_entity_id == death.person_id
                            && r.end.is_none()
                    })
                }) {
                    ctx.world.end_relationship(
                        *spouse_id,
                        death.person_id,
                        &RelationshipKind::Spouse,
                        time,
                        ev,
                    );
                }
                // Set widowed_year for remarriage cooldown
                ctx.world.set_extra(
                    *spouse_id,
                    "widowed_year".to_string(),
                    serde_json::json!(current_year),
                    ev,
                );
            }

            // If leader, end LeaderOf and emit vacancy signal
            if death.is_leader
                && let Some(leader_target) = find_leader_target(ctx.world, death.person_id)
            {
                ctx.world.end_relationship(
                    death.person_id,
                    leader_target,
                    &RelationshipKind::LeaderOf,
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

        // --- 3c: NPC births ---
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
                let settlement_id = e
                    .relationships
                    .iter()
                    .find(|r| r.kind == RelationshipKind::LocatedIn && r.end.is_none())
                    .map(|r| r.target_entity_id);
                let spouse_id = e
                    .relationships
                    .iter()
                    .find(|r| r.kind == RelationshipKind::Spouse && r.end.is_none())
                    .map(|r| r.target_entity_id);
                Some(LivingPersonInfo {
                    id: e.id,
                    settlement_id,
                    sex: person.sex.clone(),
                    birth_year: person.birth_year,
                    spouse_id,
                })
            })
            .collect();

        struct BirthPlan {
            settlement_id: u64,
            count: u32,
        }

        let mut birth_plans: Vec<BirthPlan> = Vec::new();
        for s in &living_settlements {
            let target_notables = ((s.population as f64 / 5.0).sqrt().round() as u32).clamp(3, 25);
            let current_notables = living_persons
                .iter()
                .filter(|p| p.settlement_id == Some(s.id))
                .count() as u32;
            if current_notables < target_notables {
                let births = (target_notables - current_notables).min(2);
                birth_plans.push(BirthPlan {
                    settlement_id: s.id,
                    count: births,
                });
            }
        }

        // Apply births
        let roles = [
            "common", "artisan", "warrior", "merchant", "scholar", "elder",
        ];
        let weights = [30u32, 20, 20, 15, 10, 5];
        let weight_total: u32 = weights.iter().sum();

        for plan in &birth_plans {
            for _ in 0..plan.count {
                // Find parents for surname inheritance and relationships
                let (father_id, mother_id) =
                    find_parents(&living_persons, plan.settlement_id, current_year, ctx.rng);

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
                let name = if let Some(parent_id) = father_id.or(mother_id) {
                    let parent_name = ctx
                        .world
                        .entities
                        .get(&parent_id)
                        .map(|e| e.name.as_str())
                        .unwrap_or("");
                    if let Some(surname) = extract_surname(parent_name) {
                        if let Some(ref style) = naming_style {
                            generate_culture_person_name_with_surname(
                                ctx.world, style, ctx.rng, surname,
                            )
                        } else {
                            generate_person_name_with_surname(ctx.world, ctx.rng, surname)
                        }
                    } else if let Some(ref style) = naming_style {
                        generate_unique_culture_person_name(ctx.world, style, ctx.rng)
                    } else {
                        generate_unique_person_name(ctx.world, ctx.rng)
                    }
                } else if let Some(ref style) = naming_style {
                    generate_unique_culture_person_name(ctx.world, style, ctx.rng)
                } else {
                    generate_unique_person_name(ctx.world, ctx.rng)
                };

                // Weighted role selection
                let roll = ctx.rng.random_range(0..weight_total);
                let mut cumulative = 0;
                let mut selected_role = roles[0];
                for (i, &w) in weights.iter().enumerate() {
                    cumulative += w;
                    if roll < cumulative {
                        selected_role = roles[i];
                        break;
                    }
                }

                // Random sex
                let sex = if ctx.rng.random_bool(0.5) {
                    "male"
                } else {
                    "female"
                };

                // Generate personality traits
                let traits = generate_traits(selected_role, ctx.rng);

                let ev = ctx.world.add_event(
                    EventKind::Birth,
                    time,
                    format!("{name} born in year {current_year}"),
                );

                let person_id = ctx.world.add_entity(
                    EntityKind::Person,
                    name,
                    Some(time),
                    EntityData::Person(PersonData {
                        birth_year: current_year,
                        sex: sex.to_string(),
                        role: selected_role.to_string(),
                        traits,
                        last_action_year: 0,
                        culture_id: settlement_culture_id,
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
                if let Some(faction_id) = find_settlement_faction(ctx.world, plan.settlement_id) {
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

        // --- 3d: Marriages ---
        process_marriages(ctx, time, current_year);
    }
}

// --- Helper structs ---

struct SettlementInfo {
    id: u64,
    breakdown: PopulationBreakdown,
    capacity: u32,
}

struct PersonInfo {
    id: u64,
    birth_year: u32,
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
    sex: String,
    birth_year: u32,
    spouse_id: Option<u64>,
}

// --- Helper functions ---

fn mortality_rate(age: u32) -> f64 {
    match age {
        0..=5 => 0.03,
        6..=15 => 0.005,
        16..=40 => 0.008,
        41..=60 => 0.015,
        61..=75 => 0.04,
        76..=90 => 0.10,
        91..=99 => 0.25,
        _ => 1.0,
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
        world.end_relationship(person_id, target_id, &kind, time, event_id);
    }
}

struct MarriageCandidate {
    id: u64,
    sex: String,
    faction_id: Option<u64>,
    settlement_id: u64,
}

fn process_marriages(ctx: &mut TickContext, time: SimTimestamp, current_year: u32) {
    // Collect unmarried adults grouped by settlement (BTreeMap for deterministic order)
    let mut by_settlement: std::collections::BTreeMap<u64, Vec<MarriageCandidate>> =
        std::collections::BTreeMap::new();

    for e in ctx.world.entities.values() {
        if e.kind != EntityKind::Person || e.end.is_some() {
            continue;
        }
        let Some(person) = e.data.as_person() else {
            continue;
        };
        if current_year.saturating_sub(person.birth_year) < 16 {
            continue;
        }
        // Skip if already married
        let is_married = e
            .relationships
            .iter()
            .any(|r| r.kind == RelationshipKind::Spouse && r.end.is_none());
        if is_married {
            continue;
        }
        // Skip if recently widowed (3-year cooldown)
        if let Some(widowed_year) = e.extra.get("widowed_year").and_then(|v| v.as_u64())
            && current_year.saturating_sub(widowed_year as u32) < 3
        {
            continue;
        }
        let settlement_id = e
            .relationships
            .iter()
            .find(|r| r.kind == RelationshipKind::LocatedIn && r.end.is_none())
            .map(|r| r.target_entity_id);
        let Some(sid) = settlement_id else {
            continue;
        };
        let sex = person.sex.clone();
        let faction_id = e
            .relationships
            .iter()
            .find(|r| {
                r.kind == RelationshipKind::MemberOf
                    && r.end.is_none()
                    && ctx
                        .world
                        .entities
                        .get(&r.target_entity_id)
                        .is_some_and(|t| t.kind == EntityKind::Faction)
            })
            .map(|r| r.target_entity_id);

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

    // Intra-settlement marriages: 15% chance per settlement, max 1 per year
    struct MarriagePlan {
        spouse_a: u64,
        spouse_b: u64,
        settlement_id: u64,
        cross_faction: bool,
        faction_a: Option<u64>,
        faction_b: Option<u64>,
    }
    let mut marriages: Vec<MarriagePlan> = Vec::new();

    for (sid, candidates) in &by_settlement {
        let males: Vec<&MarriageCandidate> =
            candidates.iter().filter(|c| c.sex == "male").collect();
        let females: Vec<&MarriageCandidate> =
            candidates.iter().filter(|c| c.sex == "female").collect();
        if males.is_empty() || females.is_empty() {
            continue;
        }
        if ctx.rng.random_range(0.0..1.0) < 0.15 {
            let groom = males[ctx.rng.random_range(0..males.len())];
            let bride = females[ctx.rng.random_range(0..females.len())];
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

    // Cross-faction marriage: 5% chance per tick
    if ctx.rng.random_range(0.0..1.0) < 0.05 {
        // Collect all factions
        let faction_ids: Vec<u64> = ctx
            .world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
            .map(|e| e.id)
            .collect();

        if faction_ids.len() >= 2 {
            // Pick two random non-enemy, non-at-war factions
            let idx_a = ctx.rng.random_range(0..faction_ids.len());
            let mut idx_b = ctx.rng.random_range(0..faction_ids.len() - 1);
            if idx_b >= idx_a {
                idx_b += 1;
            }
            let fa = faction_ids[idx_a];
            let fb = faction_ids[idx_b];

            // Check they're not enemies or at war
            let hostile = ctx.world.entities.get(&fa).is_some_and(|e| {
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
                    let a = cand_a[ctx.rng.random_range(0..cand_a.len())];
                    let b = cand_b[ctx.rng.random_range(0..cand_b.len())];
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

    // Apply marriages
    for marriage in &marriages {
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
            format!("{name_a} and {name_b} married in year {current_year}")
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
            let already_allies = ctx.world.entities.get(&fa).is_some_and(|e| {
                e.relationships.iter().any(|r| {
                    r.end.is_none() && r.target_entity_id == fb && r.kind == RelationshipKind::Ally
                })
            });

            if already_allies {
                // Strengthen existing alliance with marriage_alliance_year
                ctx.world.set_extra(
                    fa,
                    "marriage_alliance_year".to_string(),
                    serde_json::json!(current_year),
                    ev,
                );
                ctx.world.set_extra(
                    fb,
                    "marriage_alliance_year".to_string(),
                    serde_json::json!(current_year),
                    ev,
                );
            } else if ctx.rng.random_bool(0.5) {
                // 50% chance to create new alliance
                ctx.world
                    .add_relationship(fa, fb, RelationshipKind::Ally, time, ev);
                ctx.world.set_extra(
                    fa,
                    "marriage_alliance_year".to_string(),
                    serde_json::json!(current_year),
                    ev,
                );
                ctx.world.set_extra(
                    fb,
                    "marriage_alliance_year".to_string(),
                    serde_json::json!(current_year),
                    ev,
                );
            }
        }
    }
}

fn find_parents(
    living: &[LivingPersonInfo],
    settlement_id: u64,
    current_year: u32,
    rng: &mut dyn rand::RngCore,
) -> (Option<u64>, Option<u64>) {
    let adults: Vec<&LivingPersonInfo> = living
        .iter()
        .filter(|p| {
            p.settlement_id == Some(settlement_id)
                && current_year.saturating_sub(p.birth_year) >= 16
        })
        .collect();

    let males: Vec<&LivingPersonInfo> =
        adults.iter().filter(|p| p.sex == "male").copied().collect();
    let females: Vec<&LivingPersonInfo> = adults
        .iter()
        .filter(|p| p.sex == "female")
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

fn find_settlement_faction(world: &crate::model::World, settlement_id: u64) -> Option<u64> {
    world.entities.get(&settlement_id).and_then(|e| {
        e.relationships
            .iter()
            .find(|r| {
                r.kind == RelationshipKind::MemberOf
                    && r.end.is_none()
                    && world
                        .entities
                        .get(&r.target_entity_id)
                        .is_some_and(|t| t.kind == EntityKind::Faction)
            })
            .map(|r| r.target_entity_id)
    })
}

fn find_leader_target(world: &crate::model::World, person_id: u64) -> Option<u64> {
    world.entities.get(&person_id).and_then(|e| {
        e.relationships
            .iter()
            .find(|r| r.kind == RelationshipKind::LeaderOf && r.end.is_none())
            .map(|r| r.target_entity_id)
    })
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
                sex: "male".to_string(),
                birth_year: 10,
                spouse_id: Some(2),
            },
            LivingPersonInfo {
                id: 2,
                settlement_id: Some(100),
                sex: "female".to_string(),
                birth_year: 12,
                spouse_id: Some(1),
            },
            LivingPersonInfo {
                id: 3,
                settlement_id: Some(100),
                sex: "male".to_string(),
                birth_year: 15,
                spouse_id: None,
            },
        ];

        let mut rng = SmallRng::seed_from_u64(42);
        let (father, mother) = find_parents(&living, 100, 50, &mut rng);
        assert_eq!(father, Some(1), "married male should be picked as father");
        assert_eq!(mother, Some(2), "married female should be picked as mother");
    }

    #[test]
    fn find_parents_returns_none_for_empty_settlement() {
        use rand::SeedableRng;
        use rand::rngs::SmallRng;

        let living: Vec<LivingPersonInfo> = vec![];
        let mut rng = SmallRng::seed_from_u64(42);
        let (father, mother) = find_parents(&living, 100, 50, &mut rng);
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
                sex: "male".to_string(),
                birth_year: 40,
                spouse_id: None,
            },
            LivingPersonInfo {
                id: 2,
                settlement_id: Some(100),
                sex: "female".to_string(),
                birth_year: 42,
                spouse_id: None,
            },
        ];

        let mut rng = SmallRng::seed_from_u64(42);
        let (father, mother) = find_parents(&living, 100, 50, &mut rng);
        assert_eq!(father, None, "children should not be parents");
        assert_eq!(mother, None, "children should not be parents");
    }
}
