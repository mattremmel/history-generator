mod fortifications;
pub(crate) mod trade;

use std::collections::BTreeMap;

use super::context::TickContext;
use super::signal::{Signal, SignalKind};
use super::system::{SimSystem, TickFrequency};
use crate::model::entity_data::ResourceType;
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
const CRIME_PROSPERITY_PENALTY: f64 = 0.1;

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
        let is_year_start = time.is_year_start();

        let tick_event = ctx.world.add_event(
            EventKind::Custom("economy_tick".to_string()),
            time,
            format!("Economic activity in Y{} M{}", current_year, time.month()),
        );

        // Monthly operations — run every month, scaled by seasonal modifiers
        update_production(ctx);
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
                SignalKind::BanditRaid { settlement_id, .. } => {
                    // Reduce prosperity on raided settlement
                    if let Some(entity) = ctx.world.entities.get_mut(settlement_id)
                        && let Some(sd) = entity.data.as_settlement_mut()
                    {
                        let old = sd.prosperity;
                        sd.prosperity = (sd.prosperity - 0.05).max(0.0);
                        ctx.world.record_change(
                            *settlement_id,
                            signal.event_id,
                            "prosperity",
                            serde_json::json!(old),
                            serde_json::json!((old - 0.05).max(0.0)),
                        );
                    }
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
    resources: Vec<ResourceType>,
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
            if deposit.resource_type.as_str() == resource_type {
                Some(deposit.quality)
            } else {
                None
            }
        })
        .next()
        .unwrap_or(DEFAULT_RESOURCE_QUALITY)
}

fn update_production(ctx: &mut TickContext) {
    let settlements = gather_settlements(ctx.world);

    struct ProdUpdate {
        id: u64,
        production: BTreeMap<ResourceType, f64>,
        surplus: BTreeMap<ResourceType, f64>,
    }

    let mut updates: Vec<ProdUpdate> = Vec::new();

    for s in &settlements {
        let mut production = BTreeMap::new();
        let mut surplus = BTreeMap::new();

        let pop_factor = (s.population as f64 / POP_FACTOR_DIVISOR)
            .sqrt()
            .max(POP_FACTOR_MIN);
        let consumption_per_resource = s.population as f64 / CONSUMPTION_DIVISOR / MONTHS_PER_YEAR;

        // Read building bonuses (set by BuildingSystem before Economy ticks)
        let sd = ctx.world.settlement(s.id);
        let mine_bonus = sd.building_bonuses.mine;
        let workshop_bonus = sd.building_bonuses.workshop;

        // Read seasonal food modifier (set by EnvironmentSystem)
        let season_food_mod = sd.seasonal.food;

        for resource in &s.resources {
            let resource_str = resource.as_str();
            let quality = get_resource_quality(ctx.world, s.region_id, resource_str);
            let mut output = pop_factor * (QUALITY_BASELINE + quality);

            // Apply building bonuses
            if helpers::is_mining_resource(resource) {
                output *= 1.0 + mine_bonus;
            }
            if !helpers::is_food_resource(resource) {
                output *= 1.0 + workshop_bonus;
            }

            // Apply seasonal modifier to food resources
            if helpers::is_food_resource(resource) {
                output *= season_food_mod;
            }

            // Scale to monthly (production is computed each month)
            output /= MONTHS_PER_YEAR;

            production.insert(resource.clone(), output);

            let surplus_val = output - consumption_per_resource;
            surplus.insert(resource.clone(), surplus_val);
        }

        updates.push(ProdUpdate {
            id: s.id,
            production,
            surplus,
        });
    }

    for u in updates {
        let sd = ctx.world.settlement_mut(u.id);
        sd.production = u.production;
        sd.surplus = u.surplus;
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

                // Production value from settlement struct field
                let production_value: f64 = e
                    .data
                    .as_settlement()
                    .map(|sd| {
                        sd.production
                            .iter()
                            .map(|(res, &val)| val * resource_base_value(res.as_str()))
                            .sum()
                    })
                    .unwrap_or(0.0);

                let trade_income = e
                    .data
                    .as_settlement()
                    .map(|sd| sd.trade_income)
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

    // Collect tribute obligations from faction struct fields: (payer_id, payee_id, amount, years_remaining)
    let obligations: Vec<(u64, u64, f64, u32)> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        .flat_map(|e| {
            e.data
                .as_faction()
                .map(|fd| {
                    fd.tributes
                        .iter()
                        .filter(|(_, trib)| trib.years_remaining > 0)
                        .map(|(&payee_id, trib)| (e.id, payee_id, trib.amount, trib.years_remaining))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
        .collect();

    for (payer_id, payee_id, amount, years_remaining) in obligations {
        let payer_treasury = ctx
            .world
            .entities
            .get(&payer_id)
            .and_then(|e| e.data.as_faction())
            .map(|f| f.treasury)
            .unwrap_or(0.0);

        let transfer = amount.min(payer_treasury);

        // Deduct from payer
        if transfer > 0.0 {
            let (old_payer, new_payer) = {
                let entity = ctx.world.entities.get_mut(&payer_id).unwrap();
                let fd = entity.data.as_faction_mut().unwrap();
                let old = fd.treasury;
                fd.treasury = (old - transfer).max(0.0);
                (old, fd.treasury)
            };
            ctx.world.record_change(
                payer_id,
                year_event,
                "treasury",
                serde_json::json!(old_payer),
                serde_json::json!(new_payer),
            );
            // Add to payee
            let payee_change = if let Some(entity) = ctx.world.entities.get_mut(&payee_id)
                && let Some(fd) = entity.data.as_faction_mut()
            {
                let old = fd.treasury;
                fd.treasury += transfer;
                Some((old, fd.treasury))
            } else {
                None
            };
            if let Some((old_payee, new_payee)) = payee_change {
                ctx.world.record_change(
                    payee_id,
                    year_event,
                    "treasury",
                    serde_json::json!(old_payee),
                    serde_json::json!(new_payee),
                );
            }
        }

        let new_years = years_remaining - 1;

        if new_years == 0 {
            // Tribute ended — remove from struct field
            ctx.world.faction_mut(payer_id).tributes.remove(&payee_id);

            // End tribute_to relationship
            if let Some(entity) = ctx.world.entities.get_mut(&payer_id) {
                let kind = RelationshipKind::Custom("tribute_to".to_string());
                for r in &mut entity.relationships {
                    if r.target_entity_id == payee_id && r.kind == kind && r.end.is_none() {
                        r.end = Some(time);
                    }
                }
            }

            let payer_name = ctx
                .world
                .entities
                .get(&payer_id)
                .map(|e| e.name.clone())
                .unwrap_or_default();
            let payee_name = ctx
                .world
                .entities
                .get(&payee_id)
                .map(|e| e.name.clone())
                .unwrap_or_default();
            let ev = ctx.world.add_event(
                EventKind::TributeEnded,
                time,
                format!(
                    "{payer_name} completed tribute obligations to {payee_name} in year {}",
                    time.year()
                ),
            );
            ctx.world
                .add_event_participant(ev, payer_id, ParticipantRole::Subject);
            ctx.world
                .add_event_participant(ev, payee_id, ParticipantRole::Object);
        } else {
            // Decrement years_remaining in place
            if let Some(trib) = ctx.world.faction_mut(payer_id).tributes.get_mut(&payee_id) {
                trib.years_remaining = new_years;
            }

            // If payer can't pay (treasury at 0), create defaulted event
            if payer_treasury <= 0.0 {
                let payer_name = ctx
                    .world
                    .entities
                    .get(&payer_id)
                    .map(|e| e.name.clone())
                    .unwrap_or_default();
                let payee_name = ctx
                    .world
                    .entities
                    .get(&payee_id)
                    .map(|e| e.name.clone())
                    .unwrap_or_default();
                let _ev = ctx.world.add_event(
                    EventKind::TributeDefaulted,
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

        // Capacity from settlement struct field (default to DEFAULT_CAPACITY if 0)
        let capacity = if settlement.capacity == 0 {
            DEFAULT_CAPACITY as f64
        } else {
            settlement.capacity as f64
        };

        // Production value from settlement struct field
        let production_value: f64 = settlement
            .production
            .iter()
            .map(|(res, &val)| val * resource_base_value(res.as_str()))
            .sum();

        let trade_income = settlement.trade_income;

        let settlement_prestige = settlement.prestige;
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

        // Crime penalty
        new_prosperity -= settlement.crime_rate * CRIME_PROSPERITY_PENALTY / MONTHS_PER_YEAR;

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

fn check_economic_tensions(ctx: &mut TickContext, _year_event: u64) {
    let strategic_resources = [
        ResourceType::Iron,
        ResourceType::Copper,
        ResourceType::Horses,
        ResourceType::Timber,
    ];

    let factions: Vec<u64> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        .map(|e| e.id)
        .collect();

    // Collect resources available to each faction
    let mut faction_resources: BTreeMap<u64, std::collections::BTreeSet<ResourceType>> =
        BTreeMap::new();
    let mut faction_treasury_per_settlement: BTreeMap<u64, f64> = BTreeMap::new();

    for &fid in &factions {
        let mut resources = std::collections::BTreeSet::new();
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
        let mut adjacent_factions: std::collections::BTreeSet<u64> =
            std::collections::BTreeSet::new();
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
            for res in &strategic_resources {
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
        ctx.world.faction_mut(u.faction_id).economic_motivation = u.motivation;
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

    // -----------------------------------------------------------------------
    // Signal handler tests (deliver_signals, zero ticks)
    // -----------------------------------------------------------------------

    use crate::model::{EventKind, RelationshipKind};
    use crate::scenario::Scenario;
    use crate::testutil::{assert_approx, deliver_signals, has_relationship};

    fn test_event(world: &mut crate::model::World) -> u64 {
        world.add_event(
            EventKind::Custom("test".to_string()),
            world.current_time,
            "test signal".to_string(),
        )
    }

    #[test]
    fn scenario_war_severs_faction_trade_routes() {
        let mut s = Scenario::at_year(100);
        let ra = s.add_region("RA");
        let rb = s.add_region("RB");
        let fa = s.add_faction("FactionA");
        let fb = s.add_faction("FactionB");
        let sa = s.settlement("SA", fa, ra).population(200).id();
        let sb = s.settlement("SB", fb, rb).population(200).id();
        s.make_trade_route(sa, sb);
        let mut world = s.build();
        let ev = test_event(&mut world);

        assert!(has_relationship(
            &world,
            sa,
            &RelationshipKind::TradeRoute,
            sb
        ));

        let inbox = vec![Signal {
            event_id: ev,
            kind: SignalKind::WarStarted {
                attacker_id: fa,
                defender_id: fb,
            },
        }];
        deliver_signals(&mut world, &mut EconomySystem, &inbox, 42);

        assert!(
            !has_relationship(&world, sa, &RelationshipKind::TradeRoute, sb),
            "trade route should be severed after war"
        );
    }

    #[test]
    fn scenario_settlement_captured_severs_trade() {
        let mut s = Scenario::at_year(100);
        let r = s.add_region("R");
        let fa = s.add_faction("Old");
        let fb = s.add_faction("New");
        let sa = s.settlement("SA", fa, r).population(200).id();
        let sb = s.settlement("SB", fa, r).population(200).id();
        s.settlement("SC", fb, r).population(200).id();
        s.make_trade_route(sa, sb);
        let mut world = s.build();
        let ev = test_event(&mut world);

        assert!(has_relationship(
            &world,
            sa,
            &RelationshipKind::TradeRoute,
            sb
        ));

        let inbox = vec![Signal {
            event_id: ev,
            kind: SignalKind::SettlementCaptured {
                settlement_id: sa,
                old_faction_id: fa,
                new_faction_id: fb,
            },
        }];
        deliver_signals(&mut world, &mut EconomySystem, &inbox, 42);

        assert!(
            !has_relationship(&world, sa, &RelationshipKind::TradeRoute, sb),
            "trade route should be severed after capture"
        );
    }

    #[test]
    fn scenario_plague_severs_trade() {
        let mut s = Scenario::at_year(100);
        let r = s.add_region("R");
        let f = s.add_faction("F");
        let sa = s.settlement("SA", f, r).population(200).id();
        let sb = s.settlement("SB", f, r).population(200).id();
        s.make_trade_route(sa, sb);
        let mut world = s.build();
        let ev = test_event(&mut world);

        assert!(has_relationship(
            &world,
            sa,
            &RelationshipKind::TradeRoute,
            sb
        ));

        let inbox = vec![Signal {
            event_id: ev,
            kind: SignalKind::PlagueStarted {
                settlement_id: sa,
                disease_id: 999,
            },
        }];
        deliver_signals(&mut world, &mut EconomySystem, &inbox, 42);

        assert!(
            !has_relationship(&world, sa, &RelationshipKind::TradeRoute, sb),
            "trade route should be severed after plague"
        );
    }

    #[test]
    fn scenario_siege_severs_trade() {
        let mut s = Scenario::at_year(100);
        let r = s.add_region("R");
        let defender = s.add_faction("Defender");
        let attacker = s.add_faction("Attacker");
        let sa = s.settlement("SA", defender, r).population(200).id();
        let sb = s.settlement("SB", defender, r).population(200).id();
        s.settlement("SC", attacker, r).population(200).id();
        s.make_trade_route(sa, sb);
        let mut world = s.build();
        let ev = test_event(&mut world);

        assert!(has_relationship(
            &world,
            sa,
            &RelationshipKind::TradeRoute,
            sb
        ));

        let inbox = vec![Signal {
            event_id: ev,
            kind: SignalKind::SiegeStarted {
                settlement_id: sa,
                attacker_faction_id: attacker,
                defender_faction_id: defender,
            },
        }];
        deliver_signals(&mut world, &mut EconomySystem, &inbox, 42);

        assert!(
            !has_relationship(&world, sa, &RelationshipKind::TradeRoute, sb),
            "trade route should be severed during siege"
        );
    }

    #[test]
    fn scenario_bandit_raid_reduces_prosperity() {
        let mut s = Scenario::at_year(100);
        let r = s.add_region("R");
        let f = s.add_faction("F");
        let sett = s
            .settlement("Town", f, r)
            .population(300)
            .prosperity(0.6)
            .id();
        let mut world = s.build();
        let ev = test_event(&mut world);

        let inbox = vec![Signal {
            event_id: ev,
            kind: SignalKind::BanditRaid {
                bandit_faction_id: 999,
                settlement_id: sett,
                population_lost: 10,
                treasury_stolen: 5.0,
            },
        }];
        deliver_signals(&mut world, &mut EconomySystem, &inbox, 42);

        assert_approx(
            world.settlement(sett).prosperity,
            0.55,
            0.001,
            "prosperity after raid",
        );
    }

    #[test]
    fn scenario_tribute_records_payer_treasury_change() {
        let mut s = Scenario::at_year(100);
        let r = s.add_region("R");
        let payer = s.faction("Payer").treasury(100.0).id();
        let payee = s.faction("Payee").treasury(50.0).id();
        s.settlement("PayerTown", payer, r).population(200).id();
        s.settlement("PayeeTown", payee, r).population(200).id();
        s.add_tribute(payer, payee, 10.0, 3);

        let world = s.run(&mut [Box::new(EconomySystem)], 1, 42);

        crate::testutil::assert_property_changed(&world, payer, "treasury");
    }

    #[test]
    fn scenario_tribute_records_payee_treasury_change() {
        let mut s = Scenario::at_year(100);
        let r = s.add_region("R");
        let payer = s.faction("Payer").treasury(100.0).id();
        let payee = s.faction("Payee").treasury(50.0).id();
        s.settlement("PayerTown", payer, r).population(200).id();
        s.settlement("PayeeTown", payee, r).population(200).id();
        s.add_tribute(payer, payee, 10.0, 3);

        let world = s.run(&mut [Box::new(EconomySystem)], 1, 42);

        crate::testutil::assert_property_changed(&world, payee, "treasury");
    }
}
