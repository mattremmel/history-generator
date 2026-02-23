use std::collections::{HashMap, VecDeque};

use rand::Rng;

use super::context::TickContext;
use super::signal::{Signal, SignalKind};
use super::system::{SimSystem, TickFrequency};
use crate::model::{EntityKind, EventKind, ParticipantRole, RelationshipKind, SimTimestamp, World};

const MAX_TRADE_HOPS: usize = 6;
const MAX_ROUTES_PER_SETTLEMENT: usize = 3;
const TRADE_ROUTE_FORMATION_CHANCE: f64 = 0.15;
const TAX_RATE: f64 = 0.15;
const ARMY_MAINTENANCE_PER_STRENGTH: f64 = 0.5;
const SETTLEMENT_UPKEEP: f64 = 2.0;

pub struct EconomySystem;

impl SimSystem for EconomySystem {
    fn name(&self) -> &str {
        "economy"
    }

    fn frequency(&self) -> TickFrequency {
        TickFrequency::Yearly
    }

    fn tick(&mut self, ctx: &mut TickContext) {
        let time = ctx.world.current_time;
        let current_year = time.year();

        let year_event = ctx.world.add_event(
            EventKind::Custom("economy_tick".to_string()),
            time,
            format!("Economic activity in year {current_year}"),
        );

        update_production(ctx, year_event);
        manage_trade_routes(ctx, time, current_year, year_event);
        calculate_trade_flows(ctx, year_event);
        update_treasuries(ctx, time, year_event);
        update_economic_prosperity(ctx, year_event);
        check_trade_diplomacy(ctx, time, current_year, year_event);
        check_economic_tensions(ctx, year_event);
    }

    fn handle_signals(&mut self, ctx: &mut TickContext) {
        let time = ctx.world.current_time;
        for signal in ctx.inbox {
            match &signal.kind {
                SignalKind::WarStarted {
                    attacker_id,
                    defender_id,
                } => {
                    sever_faction_trade_routes(
                        ctx,
                        *attacker_id,
                        *defender_id,
                        time,
                        signal.event_id,
                    );
                }
                SignalKind::SettlementCaptured {
                    settlement_id,
                    old_faction_id,
                    ..
                } => {
                    sever_settlement_trade_routes(
                        ctx,
                        *settlement_id,
                        *old_faction_id,
                        time,
                        signal.event_id,
                    );
                }
                _ => {}
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Resource values
// ---------------------------------------------------------------------------

fn resource_base_value(resource: &str) -> f64 {
    match resource {
        "grain" | "cattle" | "sheep" | "fish" => 1.0,
        "timber" | "stone" | "clay" => 1.5,
        "salt" | "herbs" => 2.0,
        "peat" | "freshwater" | "wool" | "furs" | "game" => 1.5,
        "iron" | "copper" => 3.0,
        "horses" => 4.0,
        "spices" | "dyes" | "pearls" | "ivory" => 6.0,
        "gold" | "gems" => 8.0,
        "obsidian" | "sulfur" | "glass" => 2.5,
        _ => 1.5,
    }
}

// ---------------------------------------------------------------------------
// Phase B: Resource Production
// ---------------------------------------------------------------------------

struct SettlementEcon {
    id: u64,
    region_id: u64,
    faction_id: u64,
    population: u32,
    resources: Vec<String>,
}

fn gather_settlements(world: &World) -> Vec<SettlementEcon> {
    world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
        .filter_map(|e| {
            let region_id = e
                .relationships
                .iter()
                .find(|r| r.kind == RelationshipKind::LocatedIn && r.end.is_none())
                .map(|r| r.target_entity_id)?;
            let faction_id = e
                .relationships
                .iter()
                .find(|r| r.kind == RelationshipKind::MemberOf && r.end.is_none())
                .map(|r| r.target_entity_id)?;
            let settlement = e.data.as_settlement()?;

            Some(SettlementEcon {
                id: e.id,
                region_id,
                faction_id,
                population: settlement.population,
                resources: settlement.resources.clone(),
            })
        })
        .collect()
}

fn get_resource_quality(world: &World, region_id: u64, resource_type: &str) -> f64 {
    world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::ResourceDeposit && e.end.is_none())
        .filter(|e| {
            e.relationships.iter().any(|r| {
                r.kind == RelationshipKind::LocatedIn
                    && r.target_entity_id == region_id
                    && r.end.is_none()
            })
        })
        .filter_map(|e| {
            let deposit = e.data.as_resource_deposit()?;
            if deposit.resource_type == resource_type {
                Some(deposit.quality)
            } else {
                None
            }
        })
        .next()
        .unwrap_or(0.5)
}

fn update_production(ctx: &mut TickContext, year_event: u64) {
    let settlements = gather_settlements(ctx.world);

    struct ProdUpdate {
        id: u64,
        production: serde_json::Value,
        surplus: serde_json::Value,
    }

    let mut updates: Vec<ProdUpdate> = Vec::new();

    for s in &settlements {
        let mut production = serde_json::Map::new();
        let mut surplus = serde_json::Map::new();

        let pop_factor = (s.population as f64 / 100.0).sqrt().max(0.1);
        let consumption_per_resource = s.population as f64 / 200.0;

        for resource in &s.resources {
            let quality = get_resource_quality(ctx.world, s.region_id, resource);
            let output = pop_factor * (0.5 + quality);
            production.insert(resource.clone(), serde_json::json!(output));

            let surplus_val = output - consumption_per_resource;
            surplus.insert(resource.clone(), serde_json::json!(surplus_val));
        }

        updates.push(ProdUpdate {
            id: s.id,
            production: serde_json::Value::Object(production),
            surplus: serde_json::Value::Object(surplus),
        });
    }

    for u in updates {
        ctx.world
            .set_extra(u.id, "production".to_string(), u.production, year_event);
        ctx.world
            .set_extra(u.id, "surplus".to_string(), u.surplus, year_event);
    }
}

// ---------------------------------------------------------------------------
// Phase C: Multi-hop trade route pathfinding
// ---------------------------------------------------------------------------

fn get_adjacent_regions(world: &World, region_id: u64) -> Vec<u64> {
    world
        .entities
        .get(&region_id)
        .map(|e| {
            e.relationships
                .iter()
                .filter(|r| r.kind == RelationshipKind::AdjacentTo && r.end.is_none())
                .map(|r| r.target_entity_id)
                .collect()
        })
        .unwrap_or_default()
}

fn get_settlement_faction(world: &World, settlement_id: u64) -> Option<u64> {
    world.entities.get(&settlement_id).and_then(|e| {
        e.relationships
            .iter()
            .find(|r| r.kind == RelationshipKind::MemberOf && r.end.is_none())
            .map(|r| r.target_entity_id)
    })
}

fn factions_at_war(world: &World, a: u64, b: u64) -> bool {
    world
        .entities
        .get(&a)
        .map(|e| {
            e.relationships.iter().any(|r| {
                r.kind == RelationshipKind::AtWar && r.target_entity_id == b && r.end.is_none()
            })
        })
        .unwrap_or(false)
}

fn region_has_hostile_settlement(world: &World, region_id: u64, hostile_factions: &[u64]) -> bool {
    world.entities.values().any(|e| {
        e.kind == EntityKind::Settlement
            && e.end.is_none()
            && e.relationships.iter().any(|r| {
                r.kind == RelationshipKind::LocatedIn
                    && r.target_entity_id == region_id
                    && r.end.is_none()
            })
            && e.relationships.iter().any(|r| {
                r.kind == RelationshipKind::MemberOf
                    && r.end.is_none()
                    && hostile_factions.contains(&r.target_entity_id)
            })
    })
}

/// BFS from source to target region, returning full path of region IDs
/// (excluding source, including target). Returns None if unreachable
/// within max_hops or if path is blocked by hostile territory.
fn find_trade_path(
    world: &World,
    source_region: u64,
    target_region: u64,
    max_hops: usize,
    hostile_factions: &[u64],
) -> Option<Vec<u64>> {
    if source_region == target_region {
        return Some(vec![]);
    }

    let mut parent: HashMap<u64, u64> = HashMap::new();
    parent.insert(source_region, source_region);
    let mut queue: VecDeque<(u64, usize)> = VecDeque::new();

    for adj in get_adjacent_regions(world, source_region) {
        if parent.contains_key(&adj) {
            continue;
        }
        if region_has_hostile_settlement(world, adj, hostile_factions) && adj != target_region {
            continue;
        }
        parent.insert(adj, source_region);
        queue.push_back((adj, 1));
    }

    while let Some((current, depth)) = queue.pop_front() {
        if current == target_region {
            let mut path = vec![current];
            let mut node = current;
            while parent[&node] != source_region {
                node = parent[&node];
                path.push(node);
            }
            path.reverse();
            return Some(path);
        }

        if depth >= max_hops {
            continue;
        }

        for adj in get_adjacent_regions(world, current) {
            if parent.contains_key(&adj) {
                continue;
            }
            if region_has_hostile_settlement(world, adj, hostile_factions) && adj != target_region {
                continue;
            }
            parent.insert(adj, current);
            queue.push_back((adj, depth + 1));
        }
    }

    None
}

/// Check if a region has a river flowing through it.
fn region_has_river(world: &World, region_id: u64) -> bool {
    world.entities.values().any(|e| {
        e.kind == EntityKind::River
            && e.end.is_none()
            && e.relationships.iter().any(|r| {
                r.kind == RelationshipKind::FlowsThrough
                    && r.target_entity_id == region_id
                    && r.end.is_none()
            })
    })
}

fn count_active_outgoing_routes(world: &World, settlement_id: u64) -> usize {
    world
        .entities
        .get(&settlement_id)
        .map(|e| {
            e.relationships
                .iter()
                .filter(|r| r.kind == RelationshipKind::TradeRoute && r.end.is_none())
                .count()
        })
        .unwrap_or(0)
}

fn manage_trade_routes(
    ctx: &mut TickContext,
    time: SimTimestamp,
    current_year: u32,
    _year_event: u64,
) {
    let settlements = gather_settlements(ctx.world);

    // Build surplus/deficit maps
    struct TradeCandidate {
        source_id: u64,
        target_id: u64,
        source_region: u64,
        target_region: u64,
        source_faction: u64,
        target_faction: u64,
        resource: String,
        value: f64,
    }

    // Collect surpluses and deficits
    let mut surplus_settlements: Vec<(u64, u64, u64, String, f64)> = Vec::new(); // (id, region, faction, resource, surplus)
    let mut deficit_settlements: Vec<(u64, u64, u64, String, f64)> = Vec::new();

    for s in &settlements {
        let surplus_map = ctx
            .world
            .entities
            .get(&s.id)
            .and_then(|e| e.extra.get("surplus"))
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();

        for (resource, val) in &surplus_map {
            let v = val.as_f64().unwrap_or(0.0);
            if v > 0.0 {
                surplus_settlements.push((s.id, s.region_id, s.faction_id, resource.clone(), v));
            } else if v < -0.1 {
                deficit_settlements.push((s.id, s.region_id, s.faction_id, resource.clone(), v));
            }
        }
    }

    // Build candidates: each surplus settlement tries to find a deficit settlement
    let mut candidates: Vec<TradeCandidate> = Vec::new();

    // Collect factions at war with each faction (for pathfinding)
    let faction_ids: Vec<u64> = settlements
        .iter()
        .map(|s| s.faction_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    for &(src_id, src_region, src_faction, ref resource, surplus_val) in &surplus_settlements {
        // Skip if already at route cap
        if count_active_outgoing_routes(ctx.world, src_id) >= MAX_ROUTES_PER_SETTLEMENT {
            continue;
        }

        // Check if already has a route for this resource
        let has_route_for_resource = ctx
            .world
            .entities
            .get(&src_id)
            .and_then(|e| e.extra.get("trade_routes"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .any(|r| r.get("resource").and_then(|v| v.as_str()) == Some(resource))
            })
            .unwrap_or(false);
        if has_route_for_resource {
            continue;
        }

        // Find hostile factions for pathfinding
        let hostile: Vec<u64> = faction_ids
            .iter()
            .filter(|&&fid| fid != src_faction && factions_at_war(ctx.world, src_faction, fid))
            .copied()
            .collect();

        for &(tgt_id, tgt_region, tgt_faction, ref def_resource, _deficit_val) in
            &deficit_settlements
        {
            if def_resource != resource {
                continue;
            }
            if src_id == tgt_id {
                continue;
            }
            // Don't trade with factions at war
            if factions_at_war(ctx.world, src_faction, tgt_faction) {
                continue;
            }
            // Check if a route already exists between these settlements
            let already_connected = ctx
                .world
                .entities
                .get(&src_id)
                .map(|e| {
                    e.relationships.iter().any(|r| {
                        r.kind == RelationshipKind::TradeRoute
                            && r.target_entity_id == tgt_id
                            && r.end.is_none()
                    })
                })
                .unwrap_or(false);
            if already_connected {
                continue;
            }

            // Pathfind
            if let Some(path) =
                find_trade_path(ctx.world, src_region, tgt_region, MAX_TRADE_HOPS, &hostile)
            {
                let distance = path.len();
                let value =
                    surplus_val * resource_base_value(resource) / (1.0 + 0.15 * distance as f64);

                candidates.push(TradeCandidate {
                    source_id: src_id,
                    target_id: tgt_id,
                    source_region: src_region,
                    target_region: tgt_region,
                    source_faction: src_faction,
                    target_faction: tgt_faction,
                    resource: resource.clone(),
                    value,
                });
            }
        }
    }

    // Sort by value descending
    candidates.sort_by(|a, b| {
        b.value
            .partial_cmp(&a.value)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Establish routes with probability check
    let mut routes_added: HashMap<u64, usize> = HashMap::new();

    for c in &candidates {
        let current_count = routes_added.get(&c.source_id).copied().unwrap_or(0)
            + count_active_outgoing_routes(ctx.world, c.source_id);
        if current_count >= MAX_ROUTES_PER_SETTLEMENT {
            continue;
        }

        if ctx.rng.random_range(0.0..1.0) >= TRADE_ROUTE_FORMATION_CHANCE {
            continue;
        }

        // Find the path again for storage
        let hostile: Vec<u64> = faction_ids
            .iter()
            .filter(|&&fid| {
                fid != c.source_faction && factions_at_war(ctx.world, c.source_faction, fid)
            })
            .copied()
            .collect();

        let path = match find_trade_path(
            ctx.world,
            c.source_region,
            c.target_region,
            MAX_TRADE_HOPS,
            &hostile,
        ) {
            Some(p) => p,
            None => continue,
        };

        let distance = path.len();

        // Add TradeRoute relationship
        let ev = ctx.world.add_event(
            EventKind::Custom("trade_route_established".to_string()),
            time,
            format!(
                "Trade route established for {} between settlements in year {current_year}",
                c.resource
            ),
        );
        ctx.world
            .add_event_participant(ev, c.source_id, ParticipantRole::Subject);
        ctx.world
            .add_event_participant(ev, c.target_id, ParticipantRole::Object);
        ctx.world.ensure_relationship(
            c.source_id,
            c.target_id,
            RelationshipKind::TradeRoute,
            time,
            ev,
        );

        // Store route metadata on the settlement
        let route_entry = serde_json::json!({
            "target": c.target_id,
            "path": path,
            "distance": distance,
            "resource": c.resource,
        });

        let mut routes: Vec<serde_json::Value> = ctx
            .world
            .entities
            .get(&c.source_id)
            .and_then(|e| e.extra.get("trade_routes"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        routes.push(route_entry);
        ctx.world.set_extra(
            c.source_id,
            "trade_routes".to_string(),
            serde_json::json!(routes),
            ev,
        );

        // Emit signal
        ctx.signals.push(Signal {
            event_id: ev,
            kind: SignalKind::TradeRouteEstablished {
                from_settlement: c.source_id,
                to_settlement: c.target_id,
                from_faction: c.source_faction,
                to_faction: c.target_faction,
            },
        });

        *routes_added.entry(c.source_id).or_insert(0) += 1;
    }
}

// ---------------------------------------------------------------------------
// Phase D: Trade Flows & Wealth
// ---------------------------------------------------------------------------

fn calculate_trade_flows(ctx: &mut TickContext, year_event: u64) {
    // For each settlement with trade routes, compute trade income
    struct TradeUpdate {
        settlement_id: u64,
        trade_income: f64,
    }

    let mut updates: Vec<TradeUpdate> = Vec::new();

    let settlement_ids: Vec<u64> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
        .map(|e| e.id)
        .collect();

    for &sid in &settlement_ids {
        let routes: Vec<serde_json::Value> = ctx
            .world
            .entities
            .get(&sid)
            .and_then(|e| e.extra.get("trade_routes"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut total_income = 0.0;

        for route in &routes {
            let resource = route
                .get("resource")
                .and_then(|v| v.as_str())
                .unwrap_or("grain");
            let distance = route.get("distance").and_then(|v| v.as_u64()).unwrap_or(1) as f64;
            let path: Vec<u64> = route
                .get("path")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_u64()).collect())
                .unwrap_or_default();

            // Get surplus at source
            let surplus = ctx
                .world
                .entities
                .get(&sid)
                .and_then(|e| e.extra.get("surplus"))
                .and_then(|v| v.get(resource))
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0)
                .max(0.0);

            if surplus <= 0.0 {
                continue;
            }

            // Get deficit at target
            let target_id = route.get("target").and_then(|v| v.as_u64()).unwrap_or(0);
            let target_deficit = ctx
                .world
                .entities
                .get(&target_id)
                .and_then(|e| e.extra.get("surplus"))
                .and_then(|v| v.get(resource))
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);

            // Only trade if target actually has a deficit
            let demand = if target_deficit < 0.0 {
                target_deficit.abs()
            } else {
                // Target no longer needs this — still some marginal value
                0.2
            };

            let volume = surplus.min(demand);
            let distance_decay = 1.0 / (1.0 + 0.15 * distance);

            // River bonus
            let river_bonus = if path.iter().any(|&rid| region_has_river(ctx.world, rid)) {
                1.3
            } else {
                1.0
            };

            let value = volume * resource_base_value(resource) * distance_decay * river_bonus;
            total_income += value;
        }

        if total_income > 0.0 {
            updates.push(TradeUpdate {
                settlement_id: sid,
                trade_income: total_income,
            });
        }
    }

    for u in updates {
        ctx.world.set_extra(
            u.settlement_id,
            "trade_income".to_string(),
            serde_json::json!(u.trade_income),
            year_event,
        );
    }
}

fn update_treasuries(ctx: &mut TickContext, _time: SimTimestamp, year_event: u64) {
    struct FactionFinance {
        id: u64,
        income: f64,
        expenses: f64,
        old_treasury: f64,
    }

    let factions: Vec<u64> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        .map(|e| e.id)
        .collect();

    let mut finances: Vec<FactionFinance> = Vec::new();

    for &fid in &factions {
        let old_treasury = ctx
            .world
            .entities
            .get(&fid)
            .and_then(|e| e.data.as_faction())
            .map(|f| f.treasury)
            .unwrap_or(0.0);

        // Income: taxes from settlements
        let mut income = 0.0;
        let mut settlement_count = 0u32;

        for e in ctx.world.entities.values() {
            if e.kind == EntityKind::Settlement
                && e.end.is_none()
                && e.relationships.iter().any(|r| {
                    r.kind == RelationshipKind::MemberOf
                        && r.target_entity_id == fid
                        && r.end.is_none()
                })
            {
                settlement_count += 1;

                // Production value (dynamic/extra property)
                let production_value: f64 = e
                    .extra
                    .get("production")
                    .and_then(|v| v.as_object())
                    .map(|obj| {
                        obj.iter()
                            .map(|(res, val)| {
                                val.as_f64().unwrap_or(0.0) * resource_base_value(res)
                            })
                            .sum()
                    })
                    .unwrap_or(0.0);

                let trade_income = e
                    .extra
                    .get("trade_income")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);

                income += (production_value + trade_income) * TAX_RATE;
            }
        }

        // Expenses: army maintenance
        let mut army_expense = 0.0;
        for e in ctx.world.entities.values() {
            if e.kind == EntityKind::Army
                && e.end.is_none()
                && e.relationships.iter().any(|r| {
                    r.kind == RelationshipKind::MemberOf
                        && r.target_entity_id == fid
                        && r.end.is_none()
                })
            {
                let strength = e.data.as_army().map(|a| a.strength).unwrap_or(0) as f64;
                army_expense += strength * ARMY_MAINTENANCE_PER_STRENGTH;
            }
        }

        let expenses = army_expense + settlement_count as f64 * SETTLEMENT_UPKEEP;

        finances.push(FactionFinance {
            id: fid,
            income,
            expenses,
            old_treasury,
        });
    }

    for f in finances {
        let new_treasury = (f.old_treasury + f.income - f.expenses).max(0.0);
        // Mutate typed field on FactionData
        {
            let entity = ctx.world.entities.get_mut(&f.id).unwrap();
            let faction = entity.data.as_faction_mut().unwrap();
            faction.treasury = new_treasury;
        }
        ctx.world.record_change(
            f.id,
            year_event,
            "treasury",
            serde_json::json!(f.old_treasury),
            serde_json::json!(new_treasury),
        );

        if new_treasury <= 0.0 && f.old_treasury > 0.0 {
            ctx.signals.push(Signal {
                event_id: year_event,
                kind: SignalKind::TreasuryDepleted { faction_id: f.id },
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Phase E: Prosperity
// ---------------------------------------------------------------------------

fn update_economic_prosperity(ctx: &mut TickContext, year_event: u64) {
    struct ProsperityUpdate {
        settlement_id: u64,
        new_prosperity: f64,
    }

    let mut updates: Vec<ProsperityUpdate> = Vec::new();

    let settlement_ids: Vec<u64> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
        .map(|e| e.id)
        .collect();

    for &sid in &settlement_ids {
        let entity = match ctx.world.entities.get(&sid) {
            Some(e) => e,
            None => continue,
        };

        let settlement = match entity.data.as_settlement() {
            Some(s) => s,
            None => continue,
        };

        let old_prosperity = settlement.prosperity;
        let population = settlement.population as f64;

        // capacity is a dynamic extra property (not on SettlementData)
        let capacity = entity
            .extra
            .get("capacity")
            .and_then(|v| v.as_u64())
            .unwrap_or(500) as f64;

        // Production value (dynamic/extra property)
        let production_value: f64 = entity
            .extra
            .get("production")
            .and_then(|v| v.as_object())
            .map(|obj| {
                obj.iter()
                    .map(|(res, val)| val.as_f64().unwrap_or(0.0) * resource_base_value(res))
                    .sum()
            })
            .unwrap_or(0.0);

        let trade_income = entity
            .extra
            .get("trade_income")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        let economic_output = production_value + trade_income;
        // Scale: a settlement producing ~5 value per 100 people is baseline (0.5 prosperity)
        let per_capita = economic_output / (population.max(1.0) / 100.0);
        let raw_prosperity = (per_capita / 10.0).clamp(0.0, 1.0);

        // Smooth convergence
        let mut new_prosperity = old_prosperity + (raw_prosperity - old_prosperity) * 0.2;

        // Overcrowding penalty
        let capacity_ratio = population / capacity.max(1.0);
        if capacity_ratio > 0.8 {
            new_prosperity -= (capacity_ratio - 0.8) * 0.3;
        }

        new_prosperity = new_prosperity.clamp(0.05, 0.95);

        updates.push(ProsperityUpdate {
            settlement_id: sid,
            new_prosperity,
        });
    }

    for u in updates {
        // Mutate typed field on SettlementData
        let old_prosperity = {
            let entity = ctx.world.entities.get_mut(&u.settlement_id).unwrap();
            let settlement = entity.data.as_settlement_mut().unwrap();
            let old = settlement.prosperity;
            settlement.prosperity = u.new_prosperity;
            old
        };
        ctx.world.record_change(
            u.settlement_id,
            year_event,
            "prosperity",
            serde_json::json!(old_prosperity),
            serde_json::json!(u.new_prosperity),
        );
    }
}

// ---------------------------------------------------------------------------
// Phase F: Diplomacy & War Integration
// ---------------------------------------------------------------------------

fn sever_faction_trade_routes(
    ctx: &mut TickContext,
    faction_a: u64,
    faction_b: u64,
    time: SimTimestamp,
    caused_by: u64,
) {
    // Find all trade routes between settlements of these two factions
    let mut to_sever: Vec<(u64, u64)> = Vec::new();

    for e in ctx.world.entities.values() {
        if e.kind != EntityKind::Settlement || e.end.is_some() {
            continue;
        }
        let my_faction = e
            .relationships
            .iter()
            .find(|r| r.kind == RelationshipKind::MemberOf && r.end.is_none())
            .map(|r| r.target_entity_id);

        let Some(my_faction) = my_faction else {
            continue;
        };

        if my_faction != faction_a && my_faction != faction_b {
            continue;
        }

        for r in &e.relationships {
            if r.kind == RelationshipKind::TradeRoute
                && r.end.is_none()
                && let Some(tf) = get_settlement_faction(ctx.world, r.target_entity_id)
                && ((my_faction == faction_a && tf == faction_b)
                    || (my_faction == faction_b && tf == faction_a))
            {
                to_sever.push((e.id, r.target_entity_id));
            }
        }
    }

    for (source, target) in to_sever {
        sever_route(ctx, source, target, time, caused_by);
    }
}

fn sever_settlement_trade_routes(
    ctx: &mut TickContext,
    settlement_id: u64,
    _old_faction_id: u64,
    time: SimTimestamp,
    caused_by: u64,
) {
    // Sever all routes from/to this settlement that were with the old faction's trade partners
    let mut to_sever: Vec<(u64, u64)> = Vec::new();

    if let Some(e) = ctx.world.entities.get(&settlement_id) {
        for r in &e.relationships {
            if r.kind == RelationshipKind::TradeRoute && r.end.is_none() {
                to_sever.push((settlement_id, r.target_entity_id));
            }
        }
    }

    // Also find incoming routes to this settlement
    for e in ctx.world.entities.values() {
        if e.kind == EntityKind::Settlement && e.end.is_none() && e.id != settlement_id {
            for r in &e.relationships {
                if r.kind == RelationshipKind::TradeRoute
                    && r.target_entity_id == settlement_id
                    && r.end.is_none()
                {
                    to_sever.push((e.id, settlement_id));
                }
            }
        }
    }

    for (source, target) in to_sever {
        sever_route(ctx, source, target, time, caused_by);
    }
}

fn sever_route(
    ctx: &mut TickContext,
    source: u64,
    target: u64,
    time: SimTimestamp,
    caused_by: u64,
) {
    // End the TradeRoute relationship
    if let Some(e) = ctx.world.entities.get_mut(&source) {
        for r in &mut e.relationships {
            if r.kind == RelationshipKind::TradeRoute
                && r.target_entity_id == target
                && r.end.is_none()
            {
                r.end = Some(time);
            }
        }
    }

    // Remove from trade_routes extra property
    if let Some(e) = ctx.world.entities.get(&source)
        && let Some(routes) = e.extra.get("trade_routes").and_then(|v| v.as_array())
    {
        let filtered: Vec<&serde_json::Value> = routes
            .iter()
            .filter(|r| r.get("target").and_then(|v| v.as_u64()) != Some(target))
            .collect();
        let new_routes = serde_json::json!(filtered);
        ctx.world
            .set_extra(source, "trade_routes".to_string(), new_routes, caused_by);
    }

    // Emit signal
    ctx.signals.push(Signal {
        event_id: caused_by,
        kind: SignalKind::TradeRouteSevered {
            from_settlement: source,
            to_settlement: target,
        },
    });
}

fn check_trade_diplomacy(
    ctx: &mut TickContext,
    time: SimTimestamp,
    current_year: u32,
    year_event: u64,
) {
    // Count cross-faction trade routes and compute trade happiness bonuses
    let factions: Vec<u64> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        .map(|e| e.id)
        .collect();

    // Count trade routes between each faction pair
    let mut faction_pair_routes: HashMap<(u64, u64), usize> = HashMap::new();
    let mut faction_trade_partner_count: HashMap<u64, usize> = HashMap::new();

    for e in ctx.world.entities.values() {
        if e.kind != EntityKind::Settlement || e.end.is_some() {
            continue;
        }
        let my_faction = e
            .relationships
            .iter()
            .find(|r| r.kind == RelationshipKind::MemberOf && r.end.is_none())
            .map(|r| r.target_entity_id);
        let Some(my_faction) = my_faction else {
            continue;
        };

        for r in &e.relationships {
            if r.kind == RelationshipKind::TradeRoute
                && r.end.is_none()
                && let Some(target_faction) = get_settlement_faction(ctx.world, r.target_entity_id)
                && target_faction != my_faction
            {
                let key = if my_faction < target_faction {
                    (my_faction, target_faction)
                } else {
                    (target_faction, my_faction)
                };
                *faction_pair_routes.entry(key).or_insert(0) += 1;
            }
        }
    }

    // Compute trade happiness bonus per faction
    for &fid in &factions {
        let mut partner_route_count = 0usize;
        for (&(a, b), &count) in &faction_pair_routes {
            if a == fid || b == fid {
                partner_route_count += count;
            }
        }

        let bonus = (partner_route_count as f64 * 0.01).min(0.05);
        ctx.world.set_extra(
            fid,
            "trade_happiness_bonus".to_string(),
            serde_json::json!(bonus),
            year_event,
        );

        *faction_trade_partner_count.entry(fid).or_insert(0) = partner_route_count;
    }

    // Store per-faction trade route counts with each partner for alliance strength calculation
    for &fid in &factions {
        let mut partner_routes: HashMap<String, usize> = HashMap::new();
        for (&(a, b), &count) in &faction_pair_routes {
            if count == 0 {
                continue;
            }
            if a == fid {
                partner_routes.insert(b.to_string(), count);
            } else if b == fid {
                partner_routes.insert(a.to_string(), count);
            }
        }
        ctx.world.set_extra(
            fid,
            "trade_partner_routes".to_string(),
            serde_json::json!(partner_routes),
            year_event,
        );
    }

    // Trade-to-alliance: factions with trade routes may form alliances
    for (&(fa, fb), &route_count) in &faction_pair_routes {
        if route_count == 0 {
            continue;
        }

        // Check if at war
        if factions_at_war(ctx.world, fa, fb) {
            continue;
        }

        // 3% chance per year if trading (low enough to not suppress all wars)
        if route_count >= 2 && ctx.rng.random_range(0.0..1.0) < 0.03 {
            let ev = ctx.world.add_event(
                EventKind::Custom("trade_alliance".to_string()),
                time,
                format!("Trade partnership led to alliance in year {current_year}"),
            );
            ctx.world
                .add_event_participant(ev, fa, ParticipantRole::Subject);
            ctx.world
                .add_event_participant(ev, fb, ParticipantRole::Subject);
            ctx.world
                .ensure_relationship(fa, fb, RelationshipKind::Ally, time, ev);
            ctx.world
                .ensure_relationship(fb, fa, RelationshipKind::Ally, time, ev);
        }
    }
}

fn check_economic_tensions(ctx: &mut TickContext, year_event: u64) {
    let strategic_resources = ["iron", "copper", "horses", "timber"];

    let factions: Vec<u64> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        .map(|e| e.id)
        .collect();

    // Collect resources available to each faction
    let mut faction_resources: HashMap<u64, std::collections::HashSet<String>> = HashMap::new();
    let mut faction_treasury_per_settlement: HashMap<u64, f64> = HashMap::new();

    for &fid in &factions {
        let mut resources = std::collections::HashSet::new();
        let mut settlement_count = 0u32;

        for e in ctx.world.entities.values() {
            if e.kind == EntityKind::Settlement
                && e.end.is_none()
                && e.relationships.iter().any(|r| {
                    r.kind == RelationshipKind::MemberOf
                        && r.target_entity_id == fid
                        && r.end.is_none()
                })
            {
                settlement_count += 1;
                if let Some(settlement) = e.data.as_settlement() {
                    for r in &settlement.resources {
                        resources.insert(r.clone());
                    }
                }
            }
        }

        let treasury = ctx
            .world
            .entities
            .get(&fid)
            .and_then(|e| e.data.as_faction())
            .map(|f| f.treasury)
            .unwrap_or(0.0);

        let per_settlement = if settlement_count > 0 {
            treasury / settlement_count as f64
        } else {
            0.0
        };

        faction_resources.insert(fid, resources);
        faction_treasury_per_settlement.insert(fid, per_settlement);
    }

    // Compute economic war motivation for each faction
    struct MotivationUpdate {
        faction_id: u64,
        motivation: f64,
    }

    let mut updates: Vec<MotivationUpdate> = Vec::new();

    for &fid in &factions {
        let my_resources = match faction_resources.get(&fid) {
            Some(r) => r,
            None => continue,
        };
        let my_wealth = faction_treasury_per_settlement
            .get(&fid)
            .copied()
            .unwrap_or(0.0);

        let mut motivation: f64 = 0.0;

        // Check adjacent factions for resource scarcity and wealth inequality
        // Get regions owned by this faction
        let my_regions: Vec<u64> = ctx
            .world
            .entities
            .values()
            .filter(|e| {
                e.kind == EntityKind::Settlement
                    && e.end.is_none()
                    && e.relationships.iter().any(|r| {
                        r.kind == RelationshipKind::MemberOf
                            && r.target_entity_id == fid
                            && r.end.is_none()
                    })
            })
            .filter_map(|e| {
                e.relationships
                    .iter()
                    .find(|r| r.kind == RelationshipKind::LocatedIn && r.end.is_none())
                    .map(|r| r.target_entity_id)
            })
            .collect();

        // Find adjacent factions
        let mut adjacent_factions: std::collections::HashSet<u64> =
            std::collections::HashSet::new();
        for &region in &my_regions {
            for adj_region in get_adjacent_regions(ctx.world, region) {
                for e in ctx.world.entities.values() {
                    if e.kind == EntityKind::Settlement
                        && e.end.is_none()
                        && e.relationships.iter().any(|r| {
                            r.kind == RelationshipKind::LocatedIn
                                && r.target_entity_id == adj_region
                                && r.end.is_none()
                        })
                        && let Some(adj_faction) = e
                            .relationships
                            .iter()
                            .find(|r| r.kind == RelationshipKind::MemberOf && r.end.is_none())
                            .map(|r| r.target_entity_id)
                        && adj_faction != fid
                    {
                        adjacent_factions.insert(adj_faction);
                    }
                }
            }
        }

        for &adj_fid in &adjacent_factions {
            let their_resources = match faction_resources.get(&adj_fid) {
                Some(r) => r,
                None => continue,
            };

            // Resource scarcity: they have strategic resources we lack
            for &res in &strategic_resources {
                if !my_resources.contains(res) && their_resources.contains(res) {
                    motivation += 0.3;
                }
            }

            // Wealth inequality: they are much richer
            let their_wealth = faction_treasury_per_settlement
                .get(&adj_fid)
                .copied()
                .unwrap_or(0.0);
            if their_wealth > 0.0 && my_wealth > 0.0 && their_wealth / my_wealth > 3.0 {
                motivation += 0.2;
            }
        }

        motivation = motivation.clamp(0.0, 1.0);
        updates.push(MotivationUpdate {
            faction_id: fid,
            motivation,
        });
    }

    for u in updates {
        ctx.world.set_extra(
            u.faction_id,
            "economic_war_motivation".to_string(),
            serde_json::json!(u.motivation),
            year_event,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{EntityData, FactionData, RegionData, SettlementData};
    use crate::sim::population::PopulationBreakdown;

    fn region_data() -> EntityData {
        EntityData::Region(RegionData {
            terrain: "plains".to_string(),
            terrain_tags: Vec::new(),
            x: 0.0,
            y: 0.0,
            resources: Vec::new(),
        })
    }

    fn faction_data() -> EntityData {
        EntityData::Faction(FactionData {
            government_type: "chieftain".to_string(),
            stability: 0.5,
            happiness: 0.5,
            legitimacy: 0.5,
            treasury: 0.0,
            alliance_strength: 0.0,
        })
    }

    fn settlement_data() -> EntityData {
        EntityData::Settlement(SettlementData {
            population: 0,
            population_breakdown: PopulationBreakdown::empty(),
            x: 0.0,
            y: 0.0,
            resources: Vec::new(),
            prosperity: 0.5,
            treasury: 0.0,
        })
    }

    #[test]
    fn resource_values_cover_all_types() {
        let resources = [
            "grain",
            "cattle",
            "sheep",
            "fish",
            "timber",
            "stone",
            "clay",
            "salt",
            "herbs",
            "iron",
            "copper",
            "horses",
            "spices",
            "dyes",
            "pearls",
            "ivory",
            "gold",
            "gems",
            "obsidian",
            "sulfur",
            "glass",
            "peat",
            "freshwater",
            "wool",
            "furs",
            "game",
        ];
        for r in resources {
            let v = resource_base_value(r);
            assert!(v > 0.0, "resource {r} has zero value");
        }
        // Unknown defaults to 1.5
        assert_eq!(resource_base_value("unknown_thing"), 1.5);
    }

    #[test]
    fn find_trade_path_direct_neighbor() {
        let mut world = World::new();
        let ev = world.add_event(
            EventKind::Custom("setup".to_string()),
            SimTimestamp::from_year(0),
            "setup".to_string(),
        );
        let r1 = world.add_entity(
            EntityKind::Region,
            "R1".to_string(),
            Some(SimTimestamp::from_year(0)),
            region_data(),
            ev,
        );
        let r2 = world.add_entity(
            EntityKind::Region,
            "R2".to_string(),
            Some(SimTimestamp::from_year(0)),
            region_data(),
            ev,
        );
        world.add_relationship(
            r1,
            r2,
            RelationshipKind::AdjacentTo,
            SimTimestamp::from_year(0),
            ev,
        );
        world.add_relationship(
            r2,
            r1,
            RelationshipKind::AdjacentTo,
            SimTimestamp::from_year(0),
            ev,
        );

        let path = find_trade_path(&world, r1, r2, 6, &[]);
        assert_eq!(path, Some(vec![r2]));
    }

    #[test]
    fn find_trade_path_multi_hop() {
        let mut world = World::new();
        let ev = world.add_event(
            EventKind::Custom("setup".to_string()),
            SimTimestamp::from_year(0),
            "setup".to_string(),
        );
        let r1 = world.add_entity(
            EntityKind::Region,
            "R1".to_string(),
            Some(SimTimestamp::from_year(0)),
            region_data(),
            ev,
        );
        let r2 = world.add_entity(
            EntityKind::Region,
            "R2".to_string(),
            Some(SimTimestamp::from_year(0)),
            region_data(),
            ev,
        );
        let r3 = world.add_entity(
            EntityKind::Region,
            "R3".to_string(),
            Some(SimTimestamp::from_year(0)),
            region_data(),
            ev,
        );
        let r4 = world.add_entity(
            EntityKind::Region,
            "R4".to_string(),
            Some(SimTimestamp::from_year(0)),
            region_data(),
            ev,
        );

        // Chain: R1 -- R2 -- R3 -- R4
        for (a, b) in [(r1, r2), (r2, r3), (r3, r4)] {
            world.add_relationship(
                a,
                b,
                RelationshipKind::AdjacentTo,
                SimTimestamp::from_year(0),
                ev,
            );
            world.add_relationship(
                b,
                a,
                RelationshipKind::AdjacentTo,
                SimTimestamp::from_year(0),
                ev,
            );
        }

        let path = find_trade_path(&world, r1, r4, 6, &[]);
        assert_eq!(path, Some(vec![r2, r3, r4]));
    }

    #[test]
    fn find_trade_path_respects_max_hops() {
        let mut world = World::new();
        let ev = world.add_event(
            EventKind::Custom("setup".to_string()),
            SimTimestamp::from_year(0),
            "setup".to_string(),
        );
        let r1 = world.add_entity(
            EntityKind::Region,
            "R1".to_string(),
            Some(SimTimestamp::from_year(0)),
            region_data(),
            ev,
        );
        let r2 = world.add_entity(
            EntityKind::Region,
            "R2".to_string(),
            Some(SimTimestamp::from_year(0)),
            region_data(),
            ev,
        );
        let r3 = world.add_entity(
            EntityKind::Region,
            "R3".to_string(),
            Some(SimTimestamp::from_year(0)),
            region_data(),
            ev,
        );
        let r4 = world.add_entity(
            EntityKind::Region,
            "R4".to_string(),
            Some(SimTimestamp::from_year(0)),
            region_data(),
            ev,
        );

        for (a, b) in [(r1, r2), (r2, r3), (r3, r4)] {
            world.add_relationship(
                a,
                b,
                RelationshipKind::AdjacentTo,
                SimTimestamp::from_year(0),
                ev,
            );
            world.add_relationship(
                b,
                a,
                RelationshipKind::AdjacentTo,
                SimTimestamp::from_year(0),
                ev,
            );
        }

        // Max 2 hops: can't reach R4 from R1 (need 3 hops)
        let path = find_trade_path(&world, r1, r4, 2, &[]);
        assert_eq!(path, None);

        // Max 3 hops: can reach R4
        let path = find_trade_path(&world, r1, r4, 3, &[]);
        assert!(path.is_some());
    }

    #[test]
    fn find_trade_path_blocks_hostile_regions() {
        let mut world = World::new();
        let ev = world.add_event(
            EventKind::Custom("setup".to_string()),
            SimTimestamp::from_year(0),
            "setup".to_string(),
        );
        let r1 = world.add_entity(
            EntityKind::Region,
            "R1".to_string(),
            Some(SimTimestamp::from_year(0)),
            region_data(),
            ev,
        );
        let r2 = world.add_entity(
            EntityKind::Region,
            "R2".to_string(),
            Some(SimTimestamp::from_year(0)),
            region_data(),
            ev,
        );
        let r3 = world.add_entity(
            EntityKind::Region,
            "R3".to_string(),
            Some(SimTimestamp::from_year(0)),
            region_data(),
            ev,
        );

        for (a, b) in [(r1, r2), (r2, r3)] {
            world.add_relationship(
                a,
                b,
                RelationshipKind::AdjacentTo,
                SimTimestamp::from_year(0),
                ev,
            );
            world.add_relationship(
                b,
                a,
                RelationshipKind::AdjacentTo,
                SimTimestamp::from_year(0),
                ev,
            );
        }

        // Place an enemy settlement in R2
        let enemy_faction = world.add_entity(
            EntityKind::Faction,
            "Enemy".to_string(),
            Some(SimTimestamp::from_year(0)),
            faction_data(),
            ev,
        );
        let enemy_settlement = world.add_entity(
            EntityKind::Settlement,
            "EnemyTown".to_string(),
            Some(SimTimestamp::from_year(0)),
            settlement_data(),
            ev,
        );
        world.add_relationship(
            enemy_settlement,
            r2,
            RelationshipKind::LocatedIn,
            SimTimestamp::from_year(0),
            ev,
        );
        world.add_relationship(
            enemy_settlement,
            enemy_faction,
            RelationshipKind::MemberOf,
            SimTimestamp::from_year(0),
            ev,
        );

        // Without hostile factions: path exists
        let path = find_trade_path(&world, r1, r3, 6, &[]);
        assert!(path.is_some());

        // With hostile factions: path blocked
        let path = find_trade_path(&world, r1, r3, 6, &[enemy_faction]);
        assert_eq!(path, None);
    }

    #[test]
    fn find_trade_path_same_region() {
        let mut world = World::new();
        let ev = world.add_event(
            EventKind::Custom("setup".to_string()),
            SimTimestamp::from_year(0),
            "setup".to_string(),
        );
        let r1 = world.add_entity(
            EntityKind::Region,
            "R1".to_string(),
            Some(SimTimestamp::from_year(0)),
            region_data(),
            ev,
        );

        let path = find_trade_path(&world, r1, r1, 6, &[]);
        assert_eq!(path, Some(vec![]));
    }
}
