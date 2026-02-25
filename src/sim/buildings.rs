use rand::Rng;

use super::context::TickContext;
use super::extra_keys as K;
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

// ---------------------------------------------------------------------------
// Bonus multipliers — per building type
// ---------------------------------------------------------------------------

/// Bonus multiplier per level in effective_bonus: condition * (1 + LEVEL_SCALING * level).
const LEVEL_SCALING: f64 = 0.5;

/// Production bonus per Mine (scaled by effective_bonus).
const MINE_BONUS: f64 = 0.30;
/// Trade bonus per Port (scaled by effective_bonus).
const PORT_TRADE_BONUS: f64 = 0.20;
/// Trade bonus per Market (scaled by effective_bonus).
const MARKET_BONUS: f64 = 0.25;
/// Happiness bonus per Temple (scaled by effective_bonus).
const TEMPLE_HAPPINESS_BONUS: f64 = 0.05;
/// Knowledge preservation bonus per Temple (scaled by effective_bonus).
const TEMPLE_KNOWLEDGE_BONUS: f64 = 0.10;
/// Religion drift bonus per Temple (scaled by effective_bonus).
const TEMPLE_RELIGION_BONUS: f64 = 0.02;
/// Production bonus per Workshop (scaled by effective_bonus).
const WORKSHOP_BONUS: f64 = 0.20;
/// Carrying capacity bonus per Aqueduct (scaled by effective_bonus).
const AQUEDUCT_CAPACITY_BONUS: f64 = 100.0;
/// Happiness bonus per Library (scaled by effective_bonus).
const LIBRARY_HAPPINESS_BONUS: f64 = 0.02;
/// Knowledge preservation bonus per Library (scaled by effective_bonus).
const LIBRARY_BONUS: f64 = 0.15;

// ---------------------------------------------------------------------------
// Decay rates — condition loss per year
// ---------------------------------------------------------------------------

/// Annual condition loss for buildings in normal settlements.
const NORMAL_DECAY_RATE: f64 = 0.01;
/// Annual condition loss for buildings in settlements under siege.
const SIEGE_DECAY_RATE: f64 = 0.05;
/// Annual condition loss for buildings in abandoned settlements.
const ABANDONED_DECAY_RATE: f64 = 0.10;

// ---------------------------------------------------------------------------
// Construction parameters
// ---------------------------------------------------------------------------

/// Population divisor for max building count: max(1, pop / POP_PER_BUILDING_SLOT).
const POP_PER_BUILDING_SLOT: u32 = 200;
/// Base probability of constructing a building (prosperity adds to this).
const CONSTRUCTION_CHANCE_BASE: f64 = 0.3;
/// Prosperity scaling factor added to construction chance.
const CONSTRUCTION_CHANCE_PROSPERITY_FACTOR: f64 = 0.3;
/// Minimum construction-friendly months needed to allow building.
const MIN_CONSTRUCTION_MONTHS: u32 = 4;
/// Population-to-capacity ratio required before an Aqueduct can be built.
const AQUEDUCT_CAPACITY_RATIO_THRESHOLD: f64 = 0.8;

// ---------------------------------------------------------------------------
// Upgrade parameters
// ---------------------------------------------------------------------------

/// Maximum building level (0-indexed: levels 0, 1, 2).
const MAX_BUILDING_LEVEL: u8 = 2;
/// Minimum settlement prosperity to be eligible for upgrades.
const UPGRADE_MIN_PROSPERITY: f64 = 0.6;
/// Population threshold for upgrade from level 0 to level 1.
const UPGRADE_POP_THRESHOLD_1: u32 = 200;
/// Population threshold for upgrade from level 1 to level 2.
const UPGRADE_POP_THRESHOLD_2: u32 = 500;
/// Cost multiplier for upgrade from level 0 to level 1.
const UPGRADE_COST_MULTIPLIER_1: f64 = 1.5;
/// Cost multiplier for upgrade from level 1 to level 2.
const UPGRADE_COST_MULTIPLIER_2: f64 = 3.0;
/// Default base cost for buildings not listed in BUILDING_SPECS.
const UPGRADE_DEFAULT_BASE_COST: f64 = 20.0;
/// Annual probability that an eligible building is upgraded.
const UPGRADE_PROBABILITY: f64 = 0.2;

// ---------------------------------------------------------------------------
// Conquest damage
// ---------------------------------------------------------------------------

/// Minimum damage applied to each building during conquest.
const CONQUEST_MIN_DAMAGE: f64 = 0.2;
/// Maximum damage applied to each building during conquest.
const CONQUEST_MAX_DAMAGE: f64 = 0.5;

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
                damage_buildings_from_conquest(
                    ctx,
                    *settlement_id,
                    CONQUEST_MIN_DAMAGE,
                    CONQUEST_MAX_DAMAGE,
                    time,
                    signal.event_id,
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Bonus computation
// ---------------------------------------------------------------------------

/// Effective bonus = condition * (1 + 0.5 * level)
fn effective_bonus(condition: f64, level: u8) -> f64 {
    condition * (1.0 + LEVEL_SCALING * level as f64)
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
        let Some(sid) = e.active_rel(RelationshipKind::LocatedIn) else {
            continue;
        };
        settlement_buildings
            .entry(sid)
            .or_default()
            .push(BuildingInfo {
                building_type: bd.building_type,
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
        let mut temple_religion_bonus = 0.0;

        if let Some(buildings) = buildings {
            for b in buildings {
                let eff = effective_bonus(b.condition, b.level);
                match b.building_type {
                    BuildingType::Mine => mine_bonus += MINE_BONUS * eff,
                    BuildingType::Port => {
                        port_trade_bonus += PORT_TRADE_BONUS * eff;
                        port_range_bonus += 1.0; // +1 hop per port (flat)
                    }
                    BuildingType::Market => market_bonus += MARKET_BONUS * eff,
                    BuildingType::Granary => food_buffer += 1.0 * eff,
                    BuildingType::Temple => {
                        happiness_bonus += TEMPLE_HAPPINESS_BONUS * eff;
                        temple_knowledge_bonus += TEMPLE_KNOWLEDGE_BONUS * eff;
                        temple_religion_bonus += TEMPLE_RELIGION_BONUS * eff;
                    }
                    BuildingType::Workshop => workshop_bonus += WORKSHOP_BONUS * eff,
                    BuildingType::Aqueduct => capacity_bonus += AQUEDUCT_CAPACITY_BONUS * eff,
                    BuildingType::Library => {
                        happiness_bonus += LIBRARY_HAPPINESS_BONUS * eff;
                        library_bonus += LIBRARY_BONUS * eff;
                    }
                }
            }
        }

        ctx.world
            .set_extra_f64(sid, K::BUILDING_MINE_BONUS, mine_bonus, year_event);
        ctx.world
            .set_extra_f64(sid, K::BUILDING_WORKSHOP_BONUS, workshop_bonus, year_event);
        ctx.world
            .set_extra_f64(sid, K::BUILDING_MARKET_BONUS, market_bonus, year_event);
        ctx.world.set_extra_f64(
            sid,
            K::BUILDING_PORT_TRADE_BONUS,
            port_trade_bonus,
            year_event,
        );
        ctx.world.set_extra_f64(
            sid,
            K::BUILDING_PORT_RANGE_BONUS,
            port_range_bonus,
            year_event,
        );
        ctx.world.set_extra_f64(
            sid,
            K::BUILDING_HAPPINESS_BONUS,
            happiness_bonus,
            year_event,
        );
        ctx.world
            .set_extra_f64(sid, K::BUILDING_CAPACITY_BONUS, capacity_bonus, year_event);
        ctx.world
            .set_extra_f64(sid, K::BUILDING_FOOD_BUFFER, food_buffer, year_event);
        ctx.world
            .set_extra_f64(sid, K::BUILDING_LIBRARY_BONUS, library_bonus, year_event);
        ctx.world.set_extra_f64(
            sid,
            K::BUILDING_TEMPLE_KNOWLEDGE_BONUS,
            temple_knowledge_bonus,
            year_event,
        );
        ctx.world.set_extra_f64(
            sid,
            K::BUILDING_TEMPLE_RELIGION_BONUS,
            temple_religion_bonus,
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
    let siege_settlements: std::collections::BTreeSet<u64> = ctx
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

    let abandoned_settlements: std::collections::BTreeSet<u64> = ctx
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
        let settlement_id = e.active_rel(RelationshipKind::LocatedIn).unwrap_or(0);

        let decay_rate = if abandoned_settlements.contains(&settlement_id) {
            ABANDONED_DECAY_RATE
        } else if siege_settlements.contains(&settlement_id) {
            SIEGE_DECAY_RATE
        } else {
            NORMAL_DECAY_RATE
        };

        let new_condition = (bd.condition - decay_rate).max(0.0);
        let destroy = new_condition <= 0.0;

        updates.push(DecayUpdate {
            building_id: e.id,
            settlement_id,
            building_type: bd.building_type,
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
                    building_type: u.building_type,
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

fn settlement_has_building_type(
    world: &crate::model::World,
    settlement_id: u64,
    bt: &BuildingType,
) -> bool {
    world.entities.values().any(|e| {
        e.kind == EntityKind::Building
            && e.end.is_none()
            && e.data.as_building().is_some_and(|b| &b.building_type == bt)
            && e.has_active_rel(RelationshipKind::LocatedIn, settlement_id)
    })
}

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

struct BuildPlan {
    settlement_id: u64,
    settlement_name: String,
    faction_id: u64,
    building_type: BuildingType,
    cost: f64,
    x: f64,
    y: f64,
}

fn collect_construction_candidates(world: &crate::model::World) -> Vec<ConstructionCandidate> {
    world
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
            let construction_months = e.extra_u64_or(K::SEASON_CONSTRUCTION_MONTHS, 12) as u32;
            if construction_months < MIN_CONSTRUCTION_MONTHS {
                return None;
            }
            let faction_id = e.active_rel(RelationshipKind::MemberOf)?;

            // Skip BanditClan settlements
            if world
                .entities
                .get(&faction_id)
                .and_then(|f| f.data.as_faction())
                .is_some_and(|fd| fd.government_type == crate::model::GovernmentType::BanditClan)
            {
                return None;
            }

            let has_trade_routes = e.active_rels(RelationshipKind::TradeRoute).next().is_some();

            let has_non_food = sd.resources.iter().any(|r| !helpers::is_food_resource(r));

            let capacity = e.extra_u64_or(K::CAPACITY, 500);

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
        .collect()
}

fn plan_construction(
    candidates: &[ConstructionCandidate],
    ctx: &mut TickContext,
) -> Vec<BuildPlan> {
    let mut plans: Vec<BuildPlan> = Vec::new();

    for c in candidates {
        // Capacity limit: max(1, pop / POP_PER_BUILDING_SLOT)
        let max_buildings = (c.population / POP_PER_BUILDING_SLOT).max(1) as usize;
        let current_count = helpers::settlement_building_count(ctx.world, c.settlement_id);
        if current_count >= max_buildings {
            continue;
        }

        // Probability check: 0.3 + 0.3 * prosperity, scaled by construction season
        let construction_months = ctx
            .world
            .entities
            .get(&c.settlement_id)
            .map(|e| e.extra_u64_or(K::SEASON_CONSTRUCTION_MONTHS, 12))
            .unwrap_or(12) as f64;
        let season_scale = construction_months / 12.0;
        let build_chance = (CONSTRUCTION_CHANCE_BASE
            + CONSTRUCTION_CHANCE_PROSPERITY_FACTOR * c.prosperity)
            * season_scale;
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
                    // Pop must exceed threshold fraction of base capacity
                    if (c.population as f64)
                        <= c.capacity as f64 * AQUEDUCT_CAPACITY_RATIO_THRESHOLD
                    {
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
                building_type: *bt,
                cost,
                x: sx,
                y: sy,
            });
            break; // One building per settlement per year
        }
    }

    plans
}

fn apply_construction(
    plans: Vec<BuildPlan>,
    ctx: &mut TickContext,
    time: SimTimestamp,
    current_year: u32,
    year_event: u64,
) {
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
                building_type: plan.building_type,
                output_resource: None,
                x: plan.x,
                y: plan.y,
                condition: 1.0,
                level: 0,
                constructed: time,
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
                building_type: plan.building_type,
            },
        });
    }
}

fn construct_buildings(
    ctx: &mut TickContext,
    time: SimTimestamp,
    current_year: u32,
    year_event: u64,
) {
    let candidates = collect_construction_candidates(ctx.world);
    let plans = plan_construction(&candidates, ctx);
    apply_construction(plans, ctx, time, current_year, year_event);
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

        if prosperity <= UPGRADE_MIN_PROSPERITY {
            continue;
        }

        let faction_id = match settlement.and_then(|e| e.active_rel(RelationshipKind::MemberOf)) {
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
                    && e.has_active_rel(RelationshipKind::LocatedIn, sid)
            })
            .filter_map(|e| {
                let bd = e.data.as_building()?;
                if bd.level >= MAX_BUILDING_LEVEL {
                    return None;
                }
                Some((e.id, bd.building_type, bd.level))
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
            building_type: *bt,
            settlement_id: sid,
            faction_id,
            level: *level,
            population,
        });
    }

    for c in candidates {
        // Pop thresholds and cost scaling per level
        let (pop_threshold, cost_multiplier) = match c.level {
            0 => (UPGRADE_POP_THRESHOLD_1, UPGRADE_COST_MULTIPLIER_1),
            1 => (UPGRADE_POP_THRESHOLD_2, UPGRADE_COST_MULTIPLIER_2),
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
            .unwrap_or(UPGRADE_DEFAULT_BASE_COST);
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
        if ctx.rng.random_range(0.0..1.0) >= UPGRADE_PROBABILITY {
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
                building_type: c.building_type,
                new_level,
            },
        });
    }
}

// ---------------------------------------------------------------------------
// Cross-system: siege/conquest damage
// ---------------------------------------------------------------------------

/// Damage all buildings in a settlement from conquest (random damage in range).
fn damage_buildings_from_conquest(
    ctx: &mut TickContext,
    settlement_id: u64,
    min_damage: f64,
    max_damage: f64,
    time: SimTimestamp,
    caused_by: u64,
) {
    let rng = &mut *ctx.rng;
    helpers::damage_buildings(
        ctx.world,
        ctx.signals,
        settlement_id,
        time,
        caused_by,
        |old_condition| (old_condition - rng.random_range(min_damage..max_damage)).max(0.0),
        |_| true,
        "conquest",
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::entity_data::{ActiveSiege, ResourceType};
    use crate::scenario::Scenario;
    use crate::sim::context::TickContext;
    use crate::testutil::{self, assert_approx, extra_f64};
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
        let _ = s.faction_mut(setup.faction).treasury(500.0);
        let _ = s
            .settlement_mut(setup.settlement)
            .population(500)
            .prosperity(0.7)
            .resources(vec![ResourceType::Iron, ResourceType::Grain]);
        let sett = setup.settlement;
        s.add_building(BuildingType::Mine, sett);
        let mut world = s.build();

        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals = Vec::new();
        let (mut ctx, year_event) = make_ctx(&mut world, &mut rng, &mut signals);
        compute_building_bonuses(&mut ctx, year_event);

        assert_approx(
            extra_f64(ctx.world, sett, K::BUILDING_MINE_BONUS),
            0.30,
            0.01,
            "mine bonus",
        );
    }

    #[test]
    fn scenario_bonus_scales_with_level() {
        let mut s = Scenario::at_year(100);
        let setup = s.add_settlement_standalone("Town");
        let _ = s.faction_mut(setup.faction).treasury(500.0);
        let _ = s
            .settlement_mut(setup.settlement)
            .population(500)
            .prosperity(0.7);
        let sett = setup.settlement;
        s.add_building_with(BuildingType::Temple, sett, |bd| bd.level = 2);
        let mut world = s.build();

        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals = Vec::new();
        let (mut ctx, year_event) = make_ctx(&mut world, &mut rng, &mut signals);
        compute_building_bonuses(&mut ctx, year_event);

        // 0.05 * 1.0 * (1 + 0.5 * 2) = 0.05 * 2.0 = 0.10
        assert_approx(
            extra_f64(ctx.world, sett, K::BUILDING_HAPPINESS_BONUS),
            0.10,
            0.01,
            "level 2 temple happiness",
        );
    }

    #[test]
    fn scenario_decay_reduces_condition() {
        let mut s = Scenario::at_year(100);
        let setup = s.add_settlement_standalone("Town");
        let _ = s.faction_mut(setup.faction).treasury(500.0);
        let _ = s
            .settlement_mut(setup.settlement)
            .population(500)
            .prosperity(0.7);
        let sett = setup.settlement;
        let bid = s.add_building_with(BuildingType::Market, sett, |bd| bd.condition = 0.5);
        let mut world = s.build();

        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals = Vec::new();
        let (mut ctx, year_event) = make_ctx(&mut world, &mut rng, &mut signals);
        decay_buildings(&mut ctx, SimTimestamp::from_year(100), 100, year_event);

        let cond = ctx.world.building(bid).condition;
        assert_approx(cond, 0.49, 0.01, "condition after 0.01 decay");
    }

    #[test]
    fn scenario_decay_destroys_at_zero() {
        let mut s = Scenario::at_year(100);
        let setup = s.add_settlement_standalone("Town");
        let _ = s.faction_mut(setup.faction).treasury(500.0);
        let _ = s
            .settlement_mut(setup.settlement)
            .population(500)
            .prosperity(0.7);
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
        let _ = s.faction_mut(setup.faction).treasury(500.0);
        let _ = s
            .settlement_mut(setup.settlement)
            .population(500)
            .prosperity(0.7)
            .resources(vec![ResourceType::Iron, ResourceType::Grain]);
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
        let _ = s.faction_mut(setup.faction).treasury(500.0);
        let _ = s
            .settlement_mut(setup.settlement)
            .population(500)
            .prosperity(0.9)
            .resources(vec![ResourceType::Iron, ResourceType::Grain]);
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
            let treasury = world.faction(faction).treasury;
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
        let _ = s.faction_mut(setup.faction).treasury(500.0);
        let _ = s
            .settlement_mut(setup.settlement)
            .population(500)
            .prosperity(0.9)
            .resources(vec![ResourceType::Iron, ResourceType::Grain]);
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
                started: SimTimestamp::from_year_month(99, 1),
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
            world.faction(faction).treasury,
            500.0,
            0.01,
            "treasury unchanged",
        );
    }

    #[test]
    fn scenario_capacity_limit_respected() {
        let mut s = Scenario::at_year(100);
        let setup = s.add_settlement_standalone("Town");
        let _ = s.faction_mut(setup.faction).treasury(500.0);
        let _ = s
            .settlement_mut(setup.settlement)
            .population(100)
            .prosperity(0.9)
            .resources(vec![ResourceType::Iron, ResourceType::Grain]);
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

    #[test]
    fn scenario_settlement_captured_damages_buildings() {
        let mut s = Scenario::at_year(100);
        let setup = s.add_settlement_standalone("Town");
        let sett = setup.settlement;
        s.add_building(BuildingType::Market, sett);
        let mut world = s.build();

        // Find the building entity
        let building_id = world
            .entities
            .values()
            .find(|e| e.kind == EntityKind::Building && e.is_alive())
            .expect("building should exist")
            .id;

        // Verify condition starts at 1.0
        assert_approx(
            world.building(building_id).condition,
            1.0,
            0.001,
            "initial condition",
        );

        let ev = world.add_event(
            EventKind::Custom("test".to_string()),
            world.current_time,
            "test".to_string(),
        );
        let inbox = vec![Signal {
            event_id: ev,
            kind: SignalKind::SettlementCaptured {
                settlement_id: sett,
                old_faction_id: setup.faction,
                new_faction_id: 999,
            },
        }];
        testutil::deliver_signals(&mut world, &mut BuildingSystem, &inbox, 42);

        assert!(
            world.building(building_id).condition < 1.0,
            "building condition should decrease after conquest, got {}",
            world.building(building_id).condition,
        );
    }
}
