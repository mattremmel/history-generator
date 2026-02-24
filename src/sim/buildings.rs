use rand::Rng;

use super::context::TickContext;
use super::signal::{Signal, SignalKind};
use super::system::{SimSystem, TickFrequency};
use crate::model::{
    BuildingData, BuildingType, EntityData, EntityKind, EventKind, ParticipantRole,
    RelationshipKind, SimTimestamp,
};
use crate::sim::helpers;

// --- Building costs & prerequisites ---

const BUILDING_SPECS: &[(BuildingType, u32, f64)] = &[
    (BuildingType::Granary, 100, 15.0),
    (BuildingType::Market, 200, 25.0),
    (BuildingType::Workshop, 300, 30.0),
    (BuildingType::Temple, 400, 40.0),
    (BuildingType::Library, 500, 50.0),
    (BuildingType::Aqueduct, 800, 80.0),
];

pub struct BuildingSystem;

impl SimSystem for BuildingSystem {
    fn name(&self) -> &str {
        "buildings"
    }

    fn frequency(&self) -> TickFrequency {
        TickFrequency::Yearly
    }

    fn tick(&mut self, ctx: &mut TickContext) {
        let time = ctx.world.current_time;
        let current_year = time.year();

        let year_event = ctx.world.add_event(
            EventKind::Custom("buildings_tick".to_string()),
            time,
            format!("Building activity in year {current_year}"),
        );

        compute_building_bonuses(ctx, year_event);
        decay_buildings(ctx, time, current_year, year_event);
        construct_buildings(ctx, time, current_year, year_event);
        upgrade_buildings(ctx, time, current_year, year_event);
    }

    fn handle_signals(&mut self, ctx: &mut TickContext) {
        let time = ctx.world.current_time;
        for signal in ctx.inbox {
            // Conquest damages all settlement buildings
            if let SignalKind::SettlementCaptured { settlement_id, .. } = &signal.kind {
                damage_settlement_buildings(ctx, *settlement_id, 0.2, 0.5, time, signal.event_id);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Bonus computation
// ---------------------------------------------------------------------------

/// Effective bonus = condition * (1 + 0.5 * level)
fn effective_bonus(condition: f64, level: u8) -> f64 {
    condition * (1.0 + 0.5 * level as f64)
}

fn compute_building_bonuses(ctx: &mut TickContext, year_event: u64) {
    // Collect all living buildings grouped by settlement
    struct BuildingInfo {
        building_type: BuildingType,
        condition: f64,
        level: u8,
    }

    let mut settlement_buildings: std::collections::BTreeMap<u64, Vec<BuildingInfo>> =
        std::collections::BTreeMap::new();

    for e in ctx.world.entities.values() {
        if e.kind != EntityKind::Building || e.end.is_some() {
            continue;
        }
        let Some(bd) = e.data.as_building() else {
            continue;
        };
        // Find settlement via LocatedIn
        let settlement_id = e
            .relationships
            .iter()
            .find(|r| r.kind == RelationshipKind::LocatedIn && r.end.is_none())
            .map(|r| r.target_entity_id);
        let Some(sid) = settlement_id else {
            continue;
        };
        settlement_buildings
            .entry(sid)
            .or_default()
            .push(BuildingInfo {
                building_type: bd.building_type.clone(),
                condition: bd.condition,
                level: bd.level,
            });
    }

    // Collect all settlement IDs (including those with no buildings, to clear stale extras)
    let all_settlements: Vec<u64> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
        .map(|e| e.id)
        .collect();

    for sid in all_settlements {
        let buildings = settlement_buildings.get(&sid);

        let mut mine_bonus = 0.0;
        let mut workshop_bonus = 0.0;
        let mut market_bonus = 0.0;
        let mut port_trade_bonus = 0.0;
        let mut port_range_bonus = 0.0;
        let mut happiness_bonus = 0.0;
        let mut capacity_bonus = 0.0;
        let mut food_buffer = 0.0;
        let mut library_bonus = 0.0;
        let mut temple_knowledge_bonus = 0.0;

        if let Some(buildings) = buildings {
            for b in buildings {
                let eff = effective_bonus(b.condition, b.level);
                match b.building_type {
                    BuildingType::Mine => mine_bonus += 0.30 * eff,
                    BuildingType::Port => {
                        port_trade_bonus += 0.20 * eff;
                        port_range_bonus += 1.0; // +1 hop per port (flat)
                    }
                    BuildingType::Market => market_bonus += 0.25 * eff,
                    BuildingType::Granary => food_buffer += 1.0 * eff,
                    BuildingType::Temple => {
                        happiness_bonus += 0.05 * eff;
                        temple_knowledge_bonus += 0.10 * eff;
                    }
                    BuildingType::Workshop => workshop_bonus += 0.20 * eff,
                    BuildingType::Aqueduct => capacity_bonus += 100.0 * eff,
                    BuildingType::Library => {
                        happiness_bonus += 0.02 * eff;
                        library_bonus += 0.15 * eff;
                    }
                }
            }
        }

        ctx.world.set_extra(
            sid,
            "building_mine_bonus".to_string(),
            serde_json::json!(mine_bonus),
            year_event,
        );
        ctx.world.set_extra(
            sid,
            "building_workshop_bonus".to_string(),
            serde_json::json!(workshop_bonus),
            year_event,
        );
        ctx.world.set_extra(
            sid,
            "building_market_bonus".to_string(),
            serde_json::json!(market_bonus),
            year_event,
        );
        ctx.world.set_extra(
            sid,
            "building_port_trade_bonus".to_string(),
            serde_json::json!(port_trade_bonus),
            year_event,
        );
        ctx.world.set_extra(
            sid,
            "building_port_range_bonus".to_string(),
            serde_json::json!(port_range_bonus),
            year_event,
        );
        ctx.world.set_extra(
            sid,
            "building_happiness_bonus".to_string(),
            serde_json::json!(happiness_bonus),
            year_event,
        );
        ctx.world.set_extra(
            sid,
            "building_capacity_bonus".to_string(),
            serde_json::json!(capacity_bonus),
            year_event,
        );
        ctx.world.set_extra(
            sid,
            "building_food_buffer".to_string(),
            serde_json::json!(food_buffer),
            year_event,
        );
        ctx.world.set_extra(
            sid,
            "building_library_bonus".to_string(),
            serde_json::json!(library_bonus),
            year_event,
        );
        ctx.world.set_extra(
            sid,
            "building_temple_knowledge_bonus".to_string(),
            serde_json::json!(temple_knowledge_bonus),
            year_event,
        );
    }
}

// ---------------------------------------------------------------------------
// Building decay and destruction
// ---------------------------------------------------------------------------

fn decay_buildings(ctx: &mut TickContext, time: SimTimestamp, current_year: u32, year_event: u64) {
    struct DecayUpdate {
        building_id: u64,
        settlement_id: u64,
        building_type: BuildingType,
        old_condition: f64,
        new_condition: f64,
        destroy: bool,
    }

    let mut updates: Vec<DecayUpdate> = Vec::new();

    // Check which settlements are under siege or abandoned
    let siege_settlements: std::collections::HashSet<u64> = ctx
        .world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Settlement
                && e.end.is_none()
                && e.data
                    .as_settlement()
                    .is_some_and(|s| s.active_siege.is_some())
        })
        .map(|e| e.id)
        .collect();

    let abandoned_settlements: std::collections::HashSet<u64> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.end.is_some())
        .map(|e| e.id)
        .collect();

    for e in ctx.world.entities.values() {
        if e.kind != EntityKind::Building || e.end.is_some() {
            continue;
        }
        let Some(bd) = e.data.as_building() else {
            continue;
        };
        let settlement_id = e
            .relationships
            .iter()
            .find(|r| r.kind == RelationshipKind::LocatedIn && r.end.is_none())
            .map(|r| r.target_entity_id)
            .unwrap_or(0);

        let decay_rate = if abandoned_settlements.contains(&settlement_id) {
            0.10
        } else if siege_settlements.contains(&settlement_id) {
            0.05
        } else {
            0.01
        };

        let new_condition = (bd.condition - decay_rate).max(0.0);
        let destroy = new_condition <= 0.0;

        updates.push(DecayUpdate {
            building_id: e.id,
            settlement_id,
            building_type: bd.building_type.clone(),
            old_condition: bd.condition,
            new_condition,
            destroy,
        });
    }

    for u in updates {
        if u.destroy {
            let building_name = ctx
                .world
                .entities
                .get(&u.building_id)
                .map(|e| e.name.clone())
                .unwrap_or_default();
            let ev = ctx.world.add_caused_event(
                EventKind::Custom("building_destroyed".to_string()),
                time,
                format!("{building_name} crumbled to ruin in year {current_year}"),
                year_event,
            );
            ctx.world
                .add_event_participant(ev, u.building_id, ParticipantRole::Subject);
            ctx.world.end_entity(u.building_id, time, ev);

            ctx.signals.push(Signal {
                event_id: ev,
                kind: SignalKind::BuildingDestroyed {
                    building_id: u.building_id,
                    settlement_id: u.settlement_id,
                    building_type: u.building_type.clone(),
                    cause: "decay".to_string(),
                },
            });
        } else {
            let entity = ctx.world.entities.get_mut(&u.building_id).unwrap();
            let bd = entity.data.as_building_mut().unwrap();
            bd.condition = u.new_condition;

            ctx.world.record_change(
                u.building_id,
                year_event,
                "condition",
                serde_json::json!(u.old_condition),
                serde_json::json!(u.new_condition),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Construction logic
// ---------------------------------------------------------------------------

fn is_food_resource(resource: &str) -> bool {
    matches!(
        resource,
        "grain" | "cattle" | "sheep" | "fish" | "game" | "freshwater"
    )
}

fn settlement_has_building_type(
    world: &crate::model::World,
    settlement_id: u64,
    bt: &BuildingType,
) -> bool {
    world.entities.values().any(|e| {
        e.kind == EntityKind::Building
            && e.end.is_none()
            && e.data.as_building().is_some_and(|b| &b.building_type == bt)
            && e.relationships.iter().any(|r| {
                r.kind == RelationshipKind::LocatedIn
                    && r.target_entity_id == settlement_id
                    && r.end.is_none()
            })
    })
}

fn construct_buildings(
    ctx: &mut TickContext,
    time: SimTimestamp,
    current_year: u32,
    year_event: u64,
) {
    struct ConstructionCandidate {
        settlement_id: u64,
        settlement_name: String,
        faction_id: u64,
        population: u32,
        prosperity: f64,
        has_trade_routes: bool,
        has_non_food_resource: bool,
        capacity: u64,
    }

    let candidates: Vec<ConstructionCandidate> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
        .filter_map(|e| {
            let sd = e.data.as_settlement()?;
            // No building during siege or active disaster
            if sd.active_siege.is_some() || sd.active_disaster.is_some() {
                return None;
            }
            // Seasonal construction blocking: if fewer than 4 buildable months, skip
            let construction_months = e.extra_u64_or("season_construction_months", 12) as u32;
            if construction_months < 4 {
                return None;
            }
            let faction_id = e
                .relationships
                .iter()
                .find(|r| r.kind == RelationshipKind::MemberOf && r.end.is_none())
                .map(|r| r.target_entity_id)?;

            let has_trade_routes = e
                .relationships
                .iter()
                .any(|r| r.kind == RelationshipKind::TradeRoute && r.end.is_none());

            let has_non_food = sd.resources.iter().any(|r| !is_food_resource(r));

            let capacity = e.extra_u64_or("capacity", 500);

            Some(ConstructionCandidate {
                settlement_id: e.id,
                settlement_name: e.name.clone(),
                faction_id,
                population: sd.population,
                prosperity: sd.prosperity,
                has_trade_routes,
                has_non_food_resource: has_non_food,
                capacity,
            })
        })
        .collect();

    struct BuildPlan {
        settlement_id: u64,
        settlement_name: String,
        faction_id: u64,
        building_type: BuildingType,
        cost: f64,
        x: f64,
        y: f64,
    }

    let mut plans: Vec<BuildPlan> = Vec::new();

    for c in &candidates {
        // Capacity limit: max(1, pop / 200)
        let max_buildings = (c.population / 200).max(1) as usize;
        let current_count = helpers::settlement_building_count(ctx.world, c.settlement_id);
        if current_count >= max_buildings {
            continue;
        }

        // Probability check: 0.3 + 0.3 * prosperity, scaled by construction season
        let construction_months = ctx
            .world
            .entities
            .get(&c.settlement_id)
            .map(|e| e.extra_u64_or("season_construction_months", 12))
            .unwrap_or(12) as f64;
        let season_scale = construction_months / 12.0;
        let build_chance = (0.3 + 0.3 * c.prosperity) * season_scale;
        if ctx.rng.random_range(0.0..1.0) >= build_chance {
            continue;
        }

        // Get settlement coords for the building
        let (sx, sy) = ctx
            .world
            .entities
            .get(&c.settlement_id)
            .and_then(|e| e.data.as_settlement())
            .map(|s| (s.x, s.y))
            .unwrap_or((0.0, 0.0));

        // Priority order: Granary > Market > Workshop > Temple > Aqueduct
        for &(ref bt, min_pop, cost) in BUILDING_SPECS {
            if c.population < min_pop {
                continue;
            }
            // Skip if already has this building type
            if settlement_has_building_type(ctx.world, c.settlement_id, bt) {
                continue;
            }
            // Check prerequisites
            match bt {
                BuildingType::Market => {
                    if !c.has_trade_routes {
                        continue;
                    }
                }
                BuildingType::Workshop => {
                    if !c.has_non_food_resource {
                        continue;
                    }
                }
                BuildingType::Library => {
                    if !settlement_has_building_type(
                        ctx.world,
                        c.settlement_id,
                        &BuildingType::Temple,
                    ) {
                        continue;
                    }
                }
                BuildingType::Aqueduct => {
                    // Pop > 80% base capacity
                    if (c.population as f64) <= c.capacity as f64 * 0.8 {
                        continue;
                    }
                }
                _ => {}
            }

            // Check faction treasury
            let treasury = ctx
                .world
                .entities
                .get(&c.faction_id)
                .and_then(|e| e.data.as_faction())
                .map(|f| f.treasury)
                .unwrap_or(0.0);
            if treasury < cost {
                continue;
            }

            plans.push(BuildPlan {
                settlement_id: c.settlement_id,
                settlement_name: c.settlement_name.clone(),
                faction_id: c.faction_id,
                building_type: bt.clone(),
                cost,
                x: sx,
                y: sy,
            });
            break; // One building per settlement per year
        }
    }

    // Apply construction
    for plan in plans {
        // Deduct from faction treasury
        let old_treasury = {
            let entity = ctx.world.entities.get_mut(&plan.faction_id).unwrap();
            let fd = entity.data.as_faction_mut().unwrap();
            let old = fd.treasury;
            fd.treasury -= plan.cost;
            old
        };

        let building_name = format!(
            "{} {}",
            plan.settlement_name,
            capitalize_building_type(&plan.building_type)
        );

        let ev = ctx.world.add_caused_event(
            EventKind::Custom("building_constructed".to_string()),
            time,
            format!(
                "{} built in {} in year {current_year}",
                capitalize_building_type(&plan.building_type),
                plan.settlement_name
            ),
            year_event,
        );
        ctx.world
            .add_event_participant(ev, plan.settlement_id, ParticipantRole::Subject);

        let building_id = ctx.world.add_entity(
            EntityKind::Building,
            building_name,
            Some(time),
            EntityData::Building(BuildingData {
                building_type: plan.building_type.clone(),
                output_resource: None,
                x: plan.x,
                y: plan.y,
                condition: 1.0,
                level: 0,
                construction_year: current_year,
            }),
            ev,
        );

        // LocatedIn -> settlement
        ctx.world.add_relationship(
            building_id,
            plan.settlement_id,
            RelationshipKind::LocatedIn,
            time,
            ev,
        );

        ctx.world.record_change(
            plan.faction_id,
            ev,
            "treasury",
            serde_json::json!(old_treasury),
            serde_json::json!(old_treasury - plan.cost),
        );

        ctx.signals.push(Signal {
            event_id: ev,
            kind: SignalKind::BuildingConstructed {
                building_id,
                settlement_id: plan.settlement_id,
                building_type: plan.building_type.clone(),
            },
        });
    }
}

fn capitalize_building_type(bt: &BuildingType) -> &str {
    match bt {
        BuildingType::Mine => "Mine",
        BuildingType::Port => "Port",
        BuildingType::Market => "Market",
        BuildingType::Granary => "Granary",
        BuildingType::Temple => "Temple",
        BuildingType::Workshop => "Workshop",
        BuildingType::Aqueduct => "Aqueduct",
        BuildingType::Library => "Library",
    }
}

// ---------------------------------------------------------------------------
// Upgrades
// ---------------------------------------------------------------------------

fn upgrade_buildings(
    ctx: &mut TickContext,
    time: SimTimestamp,
    current_year: u32,
    year_event: u64,
) {
    struct UpgradeCandidate {
        building_id: u64,
        building_type: BuildingType,
        settlement_id: u64,
        faction_id: u64,
        level: u8,
        population: u32,
    }

    let mut candidates: Vec<UpgradeCandidate> = Vec::new();

    // Collect one upgradable building per settlement
    let settlement_ids: Vec<u64> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
        .map(|e| e.id)
        .collect();

    for &sid in &settlement_ids {
        let settlement = ctx.world.entities.get(&sid);
        let (population, prosperity) = settlement
            .and_then(|e| e.data.as_settlement())
            .map(|s| (s.population, s.prosperity))
            .unwrap_or((0, 0.0));

        if prosperity <= 0.6 {
            continue;
        }

        let faction_id = match settlement.and_then(|e| {
            e.relationships
                .iter()
                .find(|r| r.kind == RelationshipKind::MemberOf && r.end.is_none())
                .map(|r| r.target_entity_id)
        }) {
            Some(id) => id,
            None => continue,
        };

        // Find upgradable buildings in this settlement
        let upgradable: Vec<(u64, BuildingType, u8)> = ctx
            .world
            .entities
            .values()
            .filter(|e| {
                e.kind == EntityKind::Building
                    && e.end.is_none()
                    && e.relationships.iter().any(|r| {
                        r.kind == RelationshipKind::LocatedIn
                            && r.target_entity_id == sid
                            && r.end.is_none()
                    })
            })
            .filter_map(|e| {
                let bd = e.data.as_building()?;
                if bd.level >= 2 {
                    return None;
                }
                Some((e.id, bd.building_type.clone(), bd.level))
            })
            .collect();

        if upgradable.is_empty() {
            continue;
        }

        // Pick a random one
        let idx = ctx.rng.random_range(0..upgradable.len());
        let (bid, bt, level) = &upgradable[idx];
        candidates.push(UpgradeCandidate {
            building_id: *bid,
            building_type: bt.clone(),
            settlement_id: sid,
            faction_id,
            level: *level,
            population,
        });
    }

    for c in candidates {
        // Pop thresholds: level 0→1 requires 200, level 1→2 requires 500
        let (pop_threshold, cost_multiplier) = match c.level {
            0 => (200u32, 1.5),
            1 => (500u32, 3.0),
            _ => continue,
        };

        if c.population < pop_threshold {
            continue;
        }

        // Get base cost for this building type
        let base_cost = BUILDING_SPECS
            .iter()
            .find(|(bt, _, _)| *bt == c.building_type)
            .map(|(_, _, cost)| *cost)
            // Mine/Port aren't in BUILDING_SPECS (they're worldgen-only), use a default
            .unwrap_or(20.0);
        let upgrade_cost = base_cost * cost_multiplier;

        // Check faction treasury
        let treasury = ctx
            .world
            .entities
            .get(&c.faction_id)
            .and_then(|e| e.data.as_faction())
            .map(|f| f.treasury)
            .unwrap_or(0.0);
        if treasury < upgrade_cost {
            continue;
        }

        // Probability check
        if ctx.rng.random_range(0.0..1.0) >= 0.2 {
            continue;
        }

        let new_level = c.level + 1;

        // Deduct treasury
        {
            let entity = ctx.world.entities.get_mut(&c.faction_id).unwrap();
            let fd = entity.data.as_faction_mut().unwrap();
            fd.treasury -= upgrade_cost;
        }

        // Upgrade building
        {
            let entity = ctx.world.entities.get_mut(&c.building_id).unwrap();
            let bd = entity.data.as_building_mut().unwrap();
            bd.level = new_level;
            bd.condition = 1.0; // Restore condition on upgrade
        }

        let building_name = ctx
            .world
            .entities
            .get(&c.building_id)
            .map(|e| e.name.clone())
            .unwrap_or_default();
        let level_name = match new_level {
            1 => "improved",
            2 => "grand",
            _ => "upgraded",
        };

        let ev = ctx.world.add_caused_event(
            EventKind::Custom("building_upgraded".to_string()),
            time,
            format!("{building_name} upgraded to {level_name} in year {current_year}"),
            year_event,
        );
        ctx.world
            .add_event_participant(ev, c.building_id, ParticipantRole::Subject);
        ctx.world
            .add_event_participant(ev, c.settlement_id, ParticipantRole::Location);

        ctx.world.record_change(
            c.building_id,
            ev,
            "level",
            serde_json::json!(c.level),
            serde_json::json!(new_level),
        );

        ctx.signals.push(Signal {
            event_id: ev,
            kind: SignalKind::BuildingUpgraded {
                building_id: c.building_id,
                settlement_id: c.settlement_id,
                building_type: c.building_type.clone(),
                new_level,
            },
        });
    }
}

// ---------------------------------------------------------------------------
// Cross-system: siege/conquest damage
// ---------------------------------------------------------------------------

fn damage_settlement_buildings(
    ctx: &mut TickContext,
    settlement_id: u64,
    min_damage: f64,
    max_damage: f64,
    time: SimTimestamp,
    caused_by: u64,
) {
    let building_ids: Vec<u64> = ctx
        .world
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
        .map(|e| e.id)
        .collect();

    for bid in building_ids {
        let damage = ctx.rng.random_range(min_damage..max_damage);
        let old_condition = ctx
            .world
            .entities
            .get(&bid)
            .and_then(|e| e.data.as_building())
            .map(|b| b.condition)
            .unwrap_or(0.0);
        let new_condition = (old_condition - damage).max(0.0);

        if new_condition <= 0.0 {
            let building_name = ctx
                .world
                .entities
                .get(&bid)
                .map(|e| e.name.clone())
                .unwrap_or_default();
            let building_type = ctx
                .world
                .entities
                .get(&bid)
                .and_then(|e| e.data.as_building())
                .map(|b| b.building_type.clone());
            let Some(building_type) = building_type else {
                continue;
            };
            let ev = ctx.world.add_caused_event(
                EventKind::Custom("building_destroyed".to_string()),
                time,
                format!("{building_name} destroyed during conquest"),
                caused_by,
            );
            ctx.world
                .add_event_participant(ev, bid, ParticipantRole::Subject);
            ctx.world.end_entity(bid, time, ev);

            ctx.signals.push(Signal {
                event_id: ev,
                kind: SignalKind::BuildingDestroyed {
                    building_id: bid,
                    settlement_id,
                    building_type,
                    cause: "conquest".to_string(),
                },
            });
        } else {
            let entity = ctx.world.entities.get_mut(&bid).unwrap();
            let bd = entity.data.as_building_mut().unwrap();
            bd.condition = new_condition;

            ctx.world.record_change(
                bid,
                caused_by,
                "condition",
                serde_json::json!(old_condition),
                serde_json::json!(new_condition),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::entity_data::ActiveSiege;
    use crate::scenario::Scenario;
    use crate::sim::context::TickContext;
    use crate::testutil::{assert_approx, extra_f64, get_building, get_faction};
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    fn make_ctx<'a>(
        world: &'a mut crate::model::World,
        rng: &'a mut SmallRng,
        signals: &'a mut Vec<Signal>,
    ) -> (TickContext<'a>, u64) {
        let year_event = world.add_event(
            EventKind::Custom("test".to_string()),
            world.current_time,
            "test".to_string(),
        );
        let ctx = TickContext {
            world,
            rng,
            signals,
            inbox: &[],
        };
        (ctx, year_event)
    }

    #[test]
    fn scenario_mine_bonus() {
        let mut s = Scenario::at_year(100);
        let setup = s.add_settlement_standalone("TestTown");
        s.faction_mut(setup.faction).treasury(500.0);
        s.settlement_mut(setup.settlement).population(500).prosperity(0.7).resources(vec!["iron".to_string(), "grain".to_string()]);
        let sett = setup.settlement;
        s.add_building(BuildingType::Mine, sett);
        let mut world = s.build();

        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals = Vec::new();
        let (mut ctx, year_event) = make_ctx(&mut world, &mut rng, &mut signals);
        compute_building_bonuses(&mut ctx, year_event);

        assert_approx(
            extra_f64(ctx.world, sett, "building_mine_bonus"),
            0.30,
            0.01,
            "mine bonus",
        );
    }

    #[test]
    fn scenario_bonus_scales_with_level() {
        let mut s = Scenario::at_year(100);
        let setup = s.add_settlement_standalone("Town");
        s.faction_mut(setup.faction).treasury(500.0);
        s.settlement_mut(setup.settlement).population(500).prosperity(0.7);
        let sett = setup.settlement;
        s.add_building_with(BuildingType::Temple, sett, |bd| bd.level = 2);
        let mut world = s.build();

        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals = Vec::new();
        let (mut ctx, year_event) = make_ctx(&mut world, &mut rng, &mut signals);
        compute_building_bonuses(&mut ctx, year_event);

        // 0.05 * 1.0 * (1 + 0.5 * 2) = 0.05 * 2.0 = 0.10
        assert_approx(
            extra_f64(ctx.world, sett, "building_happiness_bonus"),
            0.10,
            0.01,
            "level 2 temple happiness",
        );
    }

    #[test]
    fn scenario_decay_reduces_condition() {
        let mut s = Scenario::at_year(100);
        let setup = s.add_settlement_standalone("Town");
        s.faction_mut(setup.faction).treasury(500.0);
        s.settlement_mut(setup.settlement).population(500).prosperity(0.7);
        let sett = setup.settlement;
        let bid = s.add_building_with(BuildingType::Market, sett, |bd| bd.condition = 0.5);
        let mut world = s.build();

        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals = Vec::new();
        let (mut ctx, year_event) = make_ctx(&mut world, &mut rng, &mut signals);
        decay_buildings(&mut ctx, SimTimestamp::from_year(100), 100, year_event);

        let cond = get_building(ctx.world, bid).condition;
        assert_approx(cond, 0.49, 0.01, "condition after 0.01 decay");
    }

    #[test]
    fn scenario_decay_destroys_at_zero() {
        let mut s = Scenario::at_year(100);
        let setup = s.add_settlement_standalone("Town");
        s.faction_mut(setup.faction).treasury(500.0);
        s.settlement_mut(setup.settlement).population(500).prosperity(0.7);
        let sett = setup.settlement;
        let bid = s.add_building_with(BuildingType::Granary, sett, |bd| bd.condition = 0.005);
        let mut world = s.build();

        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals = Vec::new();
        let (mut ctx, year_event) = make_ctx(&mut world, &mut rng, &mut signals);
        decay_buildings(&mut ctx, SimTimestamp::from_year(100), 100, year_event);

        let building = ctx.world.entities.get(&bid).unwrap();
        assert!(building.end.is_some(), "building should be destroyed");
        assert!(
            !signals.is_empty(),
            "should have emitted BuildingDestroyed signal"
        );
    }

    #[test]
    fn scenario_construction_creates_building() {
        let mut s = Scenario::at_year(100);
        let setup = s.add_settlement_standalone("Town");
        s.faction_mut(setup.faction).treasury(500.0);
        s.settlement_mut(setup.settlement).population(500).prosperity(0.7).resources(vec!["iron".to_string(), "grain".to_string()]);
        let sett = setup.settlement;
        // Add a second settlement for trade route
        let sett2 = s.add_settlement("Partner", setup.faction, setup.region);
        s.make_trade_route(sett, sett2);
        let mut world = s.build();

        let buildings_before = world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Building && e.end.is_none())
            .count();

        let mut rng = SmallRng::seed_from_u64(42);
        for _ in 0..20 {
            let mut signals = Vec::new();
            let (mut ctx, year_event) = make_ctx(&mut world, &mut rng, &mut signals);
            construct_buildings(&mut ctx, SimTimestamp::from_year(100), 100, year_event);
        }

        let buildings_after = world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Building && e.end.is_none())
            .count();
        assert!(
            buildings_after > buildings_before,
            "should have constructed at least one building"
        );
    }

    #[test]
    fn scenario_construction_deducts_treasury() {
        let mut s = Scenario::at_year(100);
        let setup = s.add_settlement_standalone("Town");
        s.faction_mut(setup.faction).treasury(500.0);
        s.settlement_mut(setup.settlement).population(500).prosperity(0.9).resources(vec!["iron".to_string(), "grain".to_string()]);
        let faction = setup.faction;
        let mut world = s.build();

        let mut rng = SmallRng::seed_from_u64(1);
        let mut built = false;
        for _ in 0..50 {
            let mut signals = Vec::new();
            let (mut ctx, year_event) = make_ctx(&mut world, &mut rng, &mut signals);
            construct_buildings(&mut ctx, SimTimestamp::from_year(100), 100, year_event);
            if !signals.is_empty() {
                built = true;
                break;
            }
        }

        if built {
            let treasury = get_faction(&world, faction).treasury;
            assert!(
                treasury < 500.0,
                "treasury should decrease after construction"
            );
        }
    }

    #[test]
    fn scenario_no_construction_under_siege() {
        let mut s = Scenario::at_year(100);
        let setup = s.add_settlement_standalone("Town");
        s.faction_mut(setup.faction).treasury(500.0);
        s.settlement_mut(setup.settlement).population(500).prosperity(0.9).resources(vec!["iron".to_string(), "grain".to_string()]);
        let sett = setup.settlement;
        let faction = setup.faction;
        let mut world = s.build();

        // Set siege on the settlement
        {
            let e = world.entities.get_mut(&sett).unwrap();
            let sd = e.data.as_settlement_mut().unwrap();
            sd.active_siege = Some(ActiveSiege {
                attacker_army_id: 999,
                attacker_faction_id: 888,
                started_year: 99,
                started_month: 1,
                months_elapsed: 3,
                civilian_deaths: 0,
            });
        }

        let mut rng = SmallRng::seed_from_u64(42);
        let mut any_built = false;
        for _ in 0..50 {
            let mut signals = Vec::new();
            let (mut ctx, year_event) = make_ctx(&mut world, &mut rng, &mut signals);
            construct_buildings(&mut ctx, SimTimestamp::from_year(100), 100, year_event);
            if !signals.is_empty() {
                any_built = true;
            }
        }

        assert!(!any_built, "no buildings should be constructed under siege");
        assert_approx(
            get_faction(&world, faction).treasury,
            500.0,
            0.01,
            "treasury unchanged",
        );
    }

    #[test]
    fn scenario_capacity_limit_respected() {
        let mut s = Scenario::at_year(100);
        let setup = s.add_settlement_standalone("Town");
        s.faction_mut(setup.faction).treasury(500.0);
        s.settlement_mut(setup.settlement).population(100).prosperity(0.9).resources(vec!["iron".to_string(), "grain".to_string()]);
        let sett = setup.settlement;
        // max buildings = max(1, 100/200) = 1; fill it
        s.add_building(BuildingType::Granary, sett);
        let mut world = s.build();

        let mut rng = SmallRng::seed_from_u64(42);
        let mut any_built = false;
        for _ in 0..50 {
            let mut signals = Vec::new();
            let (mut ctx, year_event) = make_ctx(&mut world, &mut rng, &mut signals);
            construct_buildings(&mut ctx, SimTimestamp::from_year(100), 100, year_event);
            if !signals.is_empty() {
                any_built = true;
            }
        }

        assert!(!any_built, "should not exceed building capacity limit");
    }
}
