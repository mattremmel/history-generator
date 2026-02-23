use rand::Rng;

use super::context::TickContext;
use super::names::generate_unique_person_name;
use super::population::PopulationBreakdown;
use super::signal::{Signal, SignalKind};
use super::system::{SimSystem, TickFrequency};
use crate::model::{EntityKind, EventKind, ParticipantRole, RelationshipKind, SimTimestamp};
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
            .map(|e| {
                let terrain_str = e
                    .properties
                    .get("terrain")
                    .and_then(|v| v.as_str())
                    .unwrap_or("plains")
                    .to_string();
                let terrain = Terrain::try_from(terrain_str).unwrap_or(Terrain::Plains);
                let tags: Vec<TerrainTag> = e
                    .properties
                    .get("terrain_tags")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| {
                                v.as_str()
                                    .and_then(|s| TerrainTag::try_from(s.to_string()).ok())
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let profile = crate::worldgen::terrain::TerrainProfile::new(terrain, tags);
                let capacity = profile.effective_population_range().1 * 5;
                (e.id, capacity)
            })
            .collect();

        // --- Collect settlement data ---
        let settlements: Vec<SettlementInfo> = ctx
            .world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
            .map(|e| {
                let population = e
                    .properties
                    .get("population")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;

                let breakdown = e
                    .properties
                    .get("population_breakdown")
                    .and_then(|v| serde_json::from_value::<PopulationBreakdown>(v.clone()).ok())
                    .unwrap_or_else(|| PopulationBreakdown::from_total(population));

                let region_id = e
                    .relationships
                    .iter()
                    .find(|r| r.kind == RelationshipKind::LocatedIn && r.end.is_none())
                    .map(|r| r.target_entity_id);

                let capacity = region_capacities
                    .iter()
                    .find(|(id, _)| Some(*id) == region_id)
                    .map(|(_, cap)| *cap)
                    .unwrap_or(500);

                SettlementInfo {
                    id: e.id,
                    breakdown,
                    capacity,
                }
            })
            .collect();

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
                ctx.world.set_property(
                    update.settlement_id,
                    "population".to_string(),
                    serde_json::json!(new_pop),
                    year_event,
                );
                ctx.world.set_property(
                    update.settlement_id,
                    "population_breakdown".to_string(),
                    serde_json::to_value(&update.new_breakdown).unwrap(),
                    year_event,
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

        // --- Prosperity updates ---
        update_prosperity(ctx, &settlements, year_event);

        // --- 3b: NPC mortality ---
        let persons: Vec<PersonInfo> = ctx
            .world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Person && e.end.is_none())
            .map(|e| {
                let birth_year = e
                    .properties
                    .get("birth_year")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                let settlement_id = e
                    .relationships
                    .iter()
                    .find(|r| r.kind == RelationshipKind::LocatedIn && r.end.is_none())
                    .map(|r| r.target_entity_id);
                let is_ruler = e
                    .relationships
                    .iter()
                    .any(|r| r.kind == RelationshipKind::RulerOf && r.end.is_none());
                PersonInfo {
                    id: e.id,
                    birth_year,
                    settlement_id,
                    is_ruler,
                }
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
                    is_ruler: person.is_ruler,
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

            // End LocatedIn and MemberOf relationships
            end_person_relationships(ctx.world, death.person_id, time, ev);

            // If ruler, end RulerOf and emit vacancy signal
            if death.is_ruler
                && let Some(ruler_target) = find_ruler_target(ctx.world, death.person_id)
            {
                ctx.world.end_relationship(
                    death.person_id,
                    ruler_target,
                    &RelationshipKind::RulerOf,
                    time,
                    ev,
                );
                ctx.signals.push(Signal {
                    event_id: ev,
                    kind: SignalKind::RulerVacancy {
                        faction_id: ruler_target,
                        previous_ruler_id: death.person_id,
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
            .map(|e| {
                let population = e
                    .properties
                    .get("population")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                SettlementBirthInfo {
                    id: e.id,
                    population,
                }
            })
            .collect();

        // Count living notables per settlement
        let living_persons: Vec<(u64, Option<u64>)> = ctx
            .world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Person && e.end.is_none())
            .map(|e| {
                let sid = e
                    .relationships
                    .iter()
                    .find(|r| r.kind == RelationshipKind::LocatedIn && r.end.is_none())
                    .map(|r| r.target_entity_id);
                (e.id, sid)
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
                .filter(|(_, sid)| *sid == Some(s.id))
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
                let name = generate_unique_person_name(ctx.world, ctx.rng);
                let ev = ctx.world.add_event(
                    EventKind::Birth,
                    time,
                    format!("{name} born in year {current_year}"),
                );

                let person_id = ctx
                    .world
                    .add_entity(EntityKind::Person, name, Some(time), ev);

                ctx.world
                    .add_event_participant(ev, person_id, ParticipantRole::Subject);
                ctx.world
                    .add_event_participant(ev, plan.settlement_id, ParticipantRole::Location);

                ctx.world.set_property(
                    person_id,
                    "birth_year".to_string(),
                    serde_json::json!(current_year),
                    ev,
                );
                ctx.world.set_property(
                    person_id,
                    "settlement_id".to_string(),
                    serde_json::json!(plan.settlement_id),
                    ev,
                );

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
                ctx.world.set_property(
                    person_id,
                    "role".to_string(),
                    serde_json::json!(selected_role),
                    ev,
                );

                // Random sex
                let sex = if ctx.rng.random_bool(0.5) {
                    "male"
                } else {
                    "female"
                };
                ctx.world
                    .set_property(person_id, "sex".to_string(), serde_json::json!(sex), ev);

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
    is_ruler: bool,
}

struct DeathInfo {
    person_id: u64,
    settlement_id: Option<u64>,
    is_ruler: bool,
}

struct SettlementBirthInfo {
    id: u64,
    population: u32,
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
                            RelationshipKind::LocatedIn | RelationshipKind::MemberOf
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

fn update_prosperity(ctx: &mut TickContext, settlements: &[SettlementInfo], year_event: u64) {
    struct ProsperityUpdate {
        settlement_id: u64,
        new_prosperity: f64,
    }

    let mut updates: Vec<ProsperityUpdate> = Vec::new();
    for s in settlements {
        let old_prosperity = ctx
            .world
            .entities
            .get(&s.id)
            .and_then(|e| e.properties.get("prosperity"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.5);

        let population = s.breakdown.total();
        let capacity = s.capacity;

        let resource_count = ctx
            .world
            .entities
            .get(&s.id)
            .and_then(|e| e.properties.get("resources"))
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);

        let resource_bonus = (resource_count as f64 * 0.1).min(0.3);
        let capacity_ratio = if capacity > 0 {
            population as f64 / capacity as f64
        } else {
            1.0
        };
        let overcrowding = if capacity_ratio > 0.8 {
            (capacity_ratio - 0.8) * 0.5
        } else {
            0.0
        };
        let target = (0.5 + resource_bonus - overcrowding).clamp(0.1, 0.9);
        let noise: f64 = ctx.rng.random_range(-0.02..0.02);
        let new_prosperity =
            (old_prosperity + (target - old_prosperity) * 0.1 + noise).clamp(0.0, 1.0);

        updates.push(ProsperityUpdate {
            settlement_id: s.id,
            new_prosperity,
        });
    }

    for update in updates {
        ctx.world.set_property(
            update.settlement_id,
            "prosperity".to_string(),
            serde_json::json!(update.new_prosperity),
            year_event,
        );
    }
}

fn find_ruler_target(world: &crate::model::World, person_id: u64) -> Option<u64> {
    world.entities.get(&person_id).and_then(|e| {
        e.relationships
            .iter()
            .find(|r| r.kind == RelationshipKind::RulerOf && r.end.is_none())
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
}
