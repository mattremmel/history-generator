mod fortifications;
mod trade;

use std::collections::HashMap;

use super::context::TickContext;
use super::extra_keys as K;
use super::signal::{Signal, SignalKind};
use super::system::{SimSystem, TickFrequency};
use crate::model::{EntityKind, EventKind, ParticipantRole, RelationshipKind, SimTimestamp, World};
use crate::sim::helpers;

const TAX_RATE: f64 = 0.15;
const ARMY_MAINTENANCE_PER_STRENGTH: f64 = 0.5;
const SETTLEMENT_UPKEEP: f64 = 2.0;

// Production parameters
const POP_FACTOR_DIVISOR: f64 = 100.0;
const POP_FACTOR_MIN: f64 = 0.1;
const CONSUMPTION_DIVISOR: f64 = 200.0;
const QUALITY_BASELINE: f64 = 0.5;
const MONTHS_PER_YEAR: f64 = 12.0;

// Default resource quality when no deposit exists
const DEFAULT_RESOURCE_QUALITY: f64 = 0.5;

// Prosperity parameters
const DEFAULT_CAPACITY: u64 = 500;
const PER_CAPITA_POP_DIVISOR: f64 = 100.0;
const PER_CAPITA_PROSPERITY_DIVISOR: f64 = 10.0;
const PRESTIGE_PROSPERITY_FACTOR: f64 = 0.05;
const PROSPERITY_CONVERGENCE_RATE: f64 = 0.2;
const OVERCROWDING_THRESHOLD: f64 = 0.8;
const OVERCROWDING_PENALTY_FACTOR: f64 = 0.3;
const PROSPERITY_FLOOR: f64 = 0.05;
const PROSPERITY_CEILING: f64 = 0.95;

// Economic tension parameters
const RESOURCE_SCARCITY_MOTIVATION: f64 = 0.3;
const WEALTH_INEQUALITY_RATIO: f64 = 3.0;
const WEALTH_INEQUALITY_MOTIVATION: f64 = 0.2;

pub struct EconomySystem;

impl SimSystem for EconomySystem {
    fn name(&self) -> &str {
        "economy"
    }

    fn frequency(&self) -> TickFrequency {
        TickFrequency::Monthly
    }

    fn tick(&mut self, ctx: &mut TickContext) {
        let time = ctx.world.current_time;
        let current_year = time.year();
        let is_year_start = time.day() == 1;

        let tick_event = ctx.world.add_event(
            EventKind::Custom("economy_tick".to_string()),
            time,
            format!("Economic activity in Y{} M{}", current_year, time.month()),
        );

        // Monthly operations — run every month, scaled by seasonal modifiers
        update_production(ctx, tick_event);
        trade::calculate_trade_flows(ctx, tick_event);
        update_treasuries(ctx, time, tick_event);
        update_economic_prosperity(ctx, tick_event);

        // Yearly operations — run only at year start (month 1)
        if is_year_start {
            trade::manage_trade_routes(ctx, time, current_year, tick_event);
            fortifications::update_fortifications(ctx, time, current_year, tick_event);
            trade::check_trade_diplomacy(ctx, time, current_year, tick_event);
            check_economic_tensions(ctx, tick_event);
        }
    }

    fn handle_signals(&mut self, ctx: &mut TickContext) {
        let time = ctx.world.current_time;
        for signal in ctx.inbox {
            match &signal.kind {
                SignalKind::WarStarted {
                    attacker_id,
                    defender_id,
                } => {
                    trade::sever_faction_trade_routes(
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
                    trade::sever_settlement_trade_routes(
                        ctx,
                        *settlement_id,
                        *old_faction_id,
                        time,
                        signal.event_id,
                    );
                }
                SignalKind::PlagueStarted { settlement_id, .. }
                | SignalKind::SiegeStarted { settlement_id, .. }
                | SignalKind::DisasterStruck { settlement_id, .. }
                | SignalKind::DisasterStarted { settlement_id, .. } => {
                    // Quarantine/siege/disaster: sever trade routes to/from affected settlement
                    trade::sever_settlement_trade_routes(
                        ctx,
                        *settlement_id,
                        0, // faction_id unused by this function
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
// Resource classification helpers
// ---------------------------------------------------------------------------

fn is_food_resource(resource: &str) -> bool {
    matches!(
        resource,
        "grain" | "cattle" | "sheep" | "fish" | "game" | "freshwater"
    )
}

const MINING_RESOURCES: &[&str] = &[
    "iron", "stone", "copper", "gold", "gems", "obsidian", "sulfur", "clay", "ore",
];

fn is_mining_resource(resource: &str) -> bool {
    MINING_RESOURCES.contains(&resource)
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
            let region_id = e.active_rel(RelationshipKind::LocatedIn)?;
            let faction_id = e.active_rel(RelationshipKind::MemberOf)?;
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
        .filter(|e| e.has_active_rel(RelationshipKind::LocatedIn, region_id))
        .filter_map(|e| {
            let deposit = e.data.as_resource_deposit()?;
            if deposit.resource_type == resource_type {
                Some(deposit.quality)
            } else {
                None
            }
        })
        .next()
        .unwrap_or(DEFAULT_RESOURCE_QUALITY)
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

        let pop_factor = (s.population as f64 / POP_FACTOR_DIVISOR)
            .sqrt()
            .max(POP_FACTOR_MIN);
        let consumption_per_resource = s.population as f64 / CONSUMPTION_DIVISOR / MONTHS_PER_YEAR;

        // Read building bonuses (set by BuildingSystem before Economy ticks)
        let entity = ctx.world.entities.get(&s.id);
        let mine_bonus = entity
            .map(|e| e.extra_f64_or(K::BUILDING_MINE_BONUS, 0.0))
            .unwrap_or(0.0);
        let workshop_bonus = entity
            .map(|e| e.extra_f64_or(K::BUILDING_WORKSHOP_BONUS, 0.0))
            .unwrap_or(0.0);

        // Read seasonal food modifier (set by EnvironmentSystem)
        let season_food_mod = entity
            .map(|e| e.extra_f64_or(K::SEASON_FOOD_MODIFIER, 1.0))
            .unwrap_or(1.0);

        for resource in &s.resources {
            let quality = get_resource_quality(ctx.world, s.region_id, resource);
            let mut output = pop_factor * (QUALITY_BASELINE + quality);

            // Apply building bonuses
            if is_mining_resource(resource) {
                output *= 1.0 + mine_bonus;
            }
            if !is_food_resource(resource) {
                output *= 1.0 + workshop_bonus;
            }

            // Apply seasonal modifier to food resources
            if is_food_resource(resource) {
                output *= season_food_mod;
            }

            // Scale to monthly (production is computed each month)
            output /= MONTHS_PER_YEAR;

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
            .set_extra(u.id, K::PRODUCTION, u.production, year_event);
        ctx.world.set_extra(u.id, K::SURPLUS, u.surplus, year_event);
    }
}

// ---------------------------------------------------------------------------
// Phase D: Treasuries
// ---------------------------------------------------------------------------

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
                && e.has_active_rel(RelationshipKind::MemberOf, fid)
            {
                settlement_count += 1;

                // Production value (dynamic/extra property)
                let production_value: f64 = e
                    .extra
                    .get(K::PRODUCTION)
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
                && e.has_active_rel(RelationshipKind::MemberOf, fid)
            {
                let strength = e.data.as_army().map(|a| a.strength).unwrap_or(0) as f64;
                army_expense += strength * ARMY_MAINTENANCE_PER_STRENGTH;
            }
        }

        // Scale expenses to monthly (constants are annual rates)
        let expenses =
            (army_expense + settlement_count as f64 * SETTLEMENT_UPKEEP) / MONTHS_PER_YEAR;

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

    // --- Tribute collection pass ---
    collect_tributes(ctx, year_event);
}

fn collect_tributes(ctx: &mut TickContext, year_event: u64) {
    let time = ctx.world.current_time;

    // Collect tribute obligations: (payer_id, payee_id, amount, years_remaining, treaty_event_id)
    struct TributeObligation {
        payer_id: u64,
        payee_id: u64,
        amount: f64,
        years_remaining: u32,
    }

    let mut obligations: Vec<TributeObligation> = Vec::new();

    let faction_ids: Vec<u64> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        .map(|e| e.id)
        .collect();

    for &fid in &faction_ids {
        let tribute_keys: Vec<(String, u64, f64, u32)> = ctx
            .world
            .entities
            .get(&fid)
            .map(|e| {
                e.extra
                    .iter()
                    .filter_map(|(k, v)| {
                        if !k.starts_with("tribute_") {
                            return None;
                        }
                        let payee_id: u64 = k.strip_prefix("tribute_")?.parse().ok()?;
                        let amount = v.get("amount")?.as_f64()?;
                        let years = v.get("years_remaining")?.as_u64()? as u32;
                        if years == 0 {
                            return None;
                        }
                        Some((k.clone(), payee_id, amount, years))
                    })
                    .collect()
            })
            .unwrap_or_default();

        for (_key, payee_id, amount, years) in tribute_keys {
            obligations.push(TributeObligation {
                payer_id: fid,
                payee_id,
                amount,
                years_remaining: years,
            });
        }
    }

    for ob in obligations {
        let payer_treasury = ctx
            .world
            .entities
            .get(&ob.payer_id)
            .and_then(|e| e.data.as_faction())
            .map(|f| f.treasury)
            .unwrap_or(0.0);

        let transfer = ob.amount.min(payer_treasury);

        // Deduct from payer
        if transfer > 0.0 {
            {
                let entity = ctx.world.entities.get_mut(&ob.payer_id).unwrap();
                let fd = entity.data.as_faction_mut().unwrap();
                fd.treasury = (fd.treasury - transfer).max(0.0);
            }
            // Add to payee
            if let Some(entity) = ctx.world.entities.get_mut(&ob.payee_id)
                && let Some(fd) = entity.data.as_faction_mut()
            {
                fd.treasury += transfer;
            }
        }

        let new_years = ob.years_remaining - 1;
        let tribute_key = format!("tribute_{}", ob.payee_id);

        if new_years == 0 {
            // Tribute ended — clean up
            ctx.world.set_extra(
                ob.payer_id,
                &tribute_key,
                serde_json::Value::Null,
                year_event,
            );

            // End tribute_to relationship
            if let Some(entity) = ctx.world.entities.get_mut(&ob.payer_id) {
                let kind = RelationshipKind::Custom("tribute_to".to_string());
                for r in &mut entity.relationships {
                    if r.target_entity_id == ob.payee_id && r.kind == kind && r.end.is_none() {
                        r.end = Some(time);
                    }
                }
            }

            let payer_name = ctx
                .world
                .entities
                .get(&ob.payer_id)
                .map(|e| e.name.clone())
                .unwrap_or_default();
            let payee_name = ctx
                .world
                .entities
                .get(&ob.payee_id)
                .map(|e| e.name.clone())
                .unwrap_or_default();
            let ev = ctx.world.add_event(
                EventKind::Custom("tribute_ended".to_string()),
                time,
                format!(
                    "{payer_name} completed tribute obligations to {payee_name} in year {}",
                    time.year()
                ),
            );
            ctx.world
                .add_event_participant(ev, ob.payer_id, ParticipantRole::Subject);
            ctx.world
                .add_event_participant(ev, ob.payee_id, ParticipantRole::Object);
        } else {
            // Decrement years_remaining
            ctx.world.set_extra(
                ob.payer_id,
                &tribute_key,
                serde_json::json!({
                    "amount": ob.amount,
                    "years_remaining": new_years,
                }),
                year_event,
            );

            // If payer can't pay (treasury at 0), create defaulted event
            if payer_treasury <= 0.0 {
                let payer_name = ctx
                    .world
                    .entities
                    .get(&ob.payer_id)
                    .map(|e| e.name.clone())
                    .unwrap_or_default();
                let payee_name = ctx
                    .world
                    .entities
                    .get(&ob.payee_id)
                    .map(|e| e.name.clone())
                    .unwrap_or_default();
                let _ev = ctx.world.add_event(
                    EventKind::Custom("tribute_defaulted".to_string()),
                    time,
                    format!(
                        "{payer_name} defaulted on tribute to {payee_name} in year {}",
                        time.year()
                    ),
                );
            }
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
        let capacity = entity.extra_u64_or(K::CAPACITY, DEFAULT_CAPACITY) as f64;

        // Production value (dynamic/extra property)
        let production_value: f64 = entity
            .extra
            .get(K::PRODUCTION)
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

        let settlement_prestige = entity
            .data
            .as_settlement()
            .map(|sd| sd.prestige)
            .unwrap_or(0.0);
        let economic_output = production_value + trade_income;
        // Scale: a settlement producing ~5 value per 100 people is baseline (0.5 prosperity)
        let per_capita = economic_output / (population.max(1.0) / PER_CAPITA_POP_DIVISOR);
        let raw_prosperity = (per_capita / PER_CAPITA_PROSPERITY_DIVISOR
            + settlement_prestige * PRESTIGE_PROSPERITY_FACTOR)
            .clamp(0.0, 1.0);

        // Smooth convergence (monthly rate = yearly rate / 12)
        let mut new_prosperity = old_prosperity
            + (raw_prosperity - old_prosperity) * (PROSPERITY_CONVERGENCE_RATE / MONTHS_PER_YEAR);

        // Overcrowding penalty
        let capacity_ratio = population / capacity.max(1.0);
        if capacity_ratio > OVERCROWDING_THRESHOLD {
            new_prosperity -= (capacity_ratio - OVERCROWDING_THRESHOLD)
                * OVERCROWDING_PENALTY_FACTOR
                / MONTHS_PER_YEAR;
        }

        new_prosperity = new_prosperity.clamp(PROSPERITY_FLOOR, PROSPERITY_CEILING);

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
// Phase F: Economic Tensions
// ---------------------------------------------------------------------------

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
                && e.has_active_rel(RelationshipKind::MemberOf, fid)
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
                    && e.has_active_rel(RelationshipKind::MemberOf, fid)
            })
            .filter_map(|e| e.active_rel(RelationshipKind::LocatedIn))
            .collect();

        // Find adjacent factions
        let mut adjacent_factions: std::collections::HashSet<u64> =
            std::collections::HashSet::new();
        for &region in &my_regions {
            for adj_region in helpers::adjacent_regions(ctx.world, region) {
                for e in ctx.world.entities.values() {
                    if e.kind == EntityKind::Settlement
                        && e.end.is_none()
                        && e.has_active_rel(RelationshipKind::LocatedIn, adj_region)
                        && let Some(adj_faction) = e.active_rel(RelationshipKind::MemberOf)
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
                    motivation += RESOURCE_SCARCITY_MOTIVATION;
                }
            }

            // Wealth inequality: they are much richer
            let their_wealth = faction_treasury_per_settlement
                .get(&adj_fid)
                .copied()
                .unwrap_or(0.0);
            if their_wealth > 0.0
                && my_wealth > 0.0
                && their_wealth / my_wealth > WEALTH_INEQUALITY_RATIO
            {
                motivation += WEALTH_INEQUALITY_MOTIVATION;
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
            K::ECONOMIC_WAR_MOTIVATION,
            serde_json::json!(u.motivation),
            year_event,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
