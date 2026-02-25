use rand::Rng;

use super::context::TickContext;
use super::extra_keys as K;
use super::signal::{Signal, SignalKind};
use super::system::{SimSystem, TickFrequency};
use crate::model::entity_data::{
    ArmyData, EntityData, FactionData, GovernmentType, SettlementData,
};
use crate::model::population::PopulationBreakdown;
use crate::model::traits::Trait;
use crate::model::{EntityKind, EventKind, ParticipantRole, RelationshipKind, Role, SimTimestamp};
use crate::sim::helpers;

// ---------------------------------------------------------------------------
// Crime rate computation
// ---------------------------------------------------------------------------
const CRIME_POVERTY_WEIGHT: f64 = 0.3;
const CRIME_OVERCROWDING_WEIGHT: f64 = 0.2;
const CRIME_INSTABILITY_WEIGHT: f64 = 0.2;
const CRIME_BANDIT_THREAT_WEIGHT: f64 = 0.2;
const CRIME_GUARD_REDUCTION: f64 = 0.5;
const CRIME_CONVERGENCE_RATE: f64 = 0.3;

// ---------------------------------------------------------------------------
// Guard strength computation
// ---------------------------------------------------------------------------
const GUARD_COST_PER_SETTLEMENT: f64 = 2.0;
const GUARD_BASE_STRENGTH: f64 = 0.1;
const GUARD_TREASURY_FACTOR: f64 = 0.3;
const GUARD_FORTIFICATION_BONUS: f64 = 0.1;

// ---------------------------------------------------------------------------
// Bandit formation
// ---------------------------------------------------------------------------
const BANDIT_FORMATION_CRIME_THRESHOLD: f64 = 0.5;
const BANDIT_FORMATION_CHANCE: f64 = 0.08;
const BANDIT_MIN_STRENGTH: u32 = 15;
const BANDIT_MAX_STRENGTH: u32 = 30;

// ---------------------------------------------------------------------------
// Trade route raiding
// ---------------------------------------------------------------------------
const RAID_TRADE_BASE_CHANCE: f64 = 0.15;
const RAID_TRADE_STRENGTH_SCALE: f64 = 30.0;
const RAID_TRADE_MAX_CHANCE: f64 = 0.3;
const RAID_TRADE_INCOME_LOSS_FRACTION: f64 = 0.4;
const RAID_TRADE_SEVER_STRENGTH: u32 = 50;

// ---------------------------------------------------------------------------
// Settlement raiding
// ---------------------------------------------------------------------------
const RAID_SETTLEMENT_BASE_CHANCE: f64 = 0.10;
const RAID_SETTLEMENT_STRENGTH_SCALE: f64 = 30.0;
const RAID_SETTLEMENT_GUARD_THRESHOLD: f64 = 0.3;
const RAID_SETTLEMENT_POP_LOSS_MIN: f64 = 0.01;
const RAID_SETTLEMENT_POP_LOSS_MAX: f64 = 0.03;
const RAID_SETTLEMENT_TREASURY_THEFT: f64 = 5.0;
const RAID_SETTLEMENT_TREASURY_FRACTION: f64 = 0.1;

// ---------------------------------------------------------------------------
// Bandit lifecycle
// ---------------------------------------------------------------------------
const BANDIT_GROWTH_CHANCE: f64 = 0.15;
const BANDIT_GROWTH_MIN: u32 = 5;
const BANDIT_GROWTH_MAX: u32 = 10;
const BANDIT_MAX_ARMY_STRENGTH: u32 = 80;
const BANDIT_DISBAND_CHANCE: f64 = 0.10;
const BANDIT_THREAT_PER_STRENGTH: f64 = 1.0 / 80.0; // strength 80 → threat 1.0

// ---------------------------------------------------------------------------
// Signal deltas
// ---------------------------------------------------------------------------
const CRIME_SPIKE_CONQUEST: f64 = 0.15;
const CRIME_SPIKE_WAR_LOSS: f64 = 0.10;
const CRIME_SPIKE_PLAGUE: f64 = 0.08;
const CRIME_SPIKE_PLAGUE_DEATH_THRESHOLD: u32 = 50;
const CRIME_SPIKE_DISASTER: f64 = 0.05;

// ---------------------------------------------------------------------------
// Bandit name generation
// ---------------------------------------------------------------------------
const BANDIT_PREFIXES: &[&str] = &[
    "Shadow", "Black", "Blood", "Iron", "Red", "Gray", "Dark", "Storm", "Bone", "Ash",
];
const BANDIT_SUFFIXES: &[&str] = &[
    "Fangs",
    "Blades",
    "Brotherhood",
    "Wolves",
    "Marauders",
    "Reavers",
    "Claws",
    "Raiders",
    "Daggers",
    "Serpents",
];

pub struct CrimeSystem;

impl SimSystem for CrimeSystem {
    fn name(&self) -> &str {
        "crime"
    }

    fn frequency(&self) -> TickFrequency {
        TickFrequency::Yearly
    }

    fn tick(&mut self, ctx: &mut TickContext) {
        let time = ctx.world.current_time;
        let current_year = time.year();

        let tick_event = ctx.world.add_event(
            EventKind::Custom("crime_tick".to_string()),
            time,
            format!("Crime activity in year {current_year}"),
        );

        update_crime_rates(ctx, tick_event);
        update_guard_strength(ctx, tick_event);
        form_bandit_gangs(ctx, time, current_year, tick_event);
        raid_trade_routes(ctx, time, current_year, tick_event);
        raid_settlements(ctx, time, current_year, tick_event);
        update_bandit_lifecycle(ctx, time, current_year, tick_event);
    }

    fn handle_signals(&mut self, ctx: &mut TickContext) {
        let time = ctx.world.current_time;
        let signal_event = ctx.world.add_event(
            EventKind::Custom("crime_signal".to_string()),
            time,
            format!("Crime signal processing in year {}", time.year()),
        );

        for signal in ctx.inbox {
            match &signal.kind {
                SignalKind::SettlementCaptured { settlement_id, .. } => {
                    apply_crime_spike(
                        ctx.world,
                        *settlement_id,
                        CRIME_SPIKE_CONQUEST,
                        signal_event,
                    );
                }
                SignalKind::WarEnded {
                    loser_id, decisive, ..
                } => {
                    if *decisive {
                        let settlements = helpers::faction_settlements(ctx.world, *loser_id);
                        for sid in settlements {
                            apply_crime_spike(ctx.world, sid, CRIME_SPIKE_WAR_LOSS, signal_event);
                        }
                    }
                }
                SignalKind::PlagueEnded {
                    settlement_id,
                    deaths,
                    ..
                } => {
                    if *deaths > CRIME_SPIKE_PLAGUE_DEATH_THRESHOLD {
                        apply_crime_spike(
                            ctx.world,
                            *settlement_id,
                            CRIME_SPIKE_PLAGUE,
                            signal_event,
                        );
                    }
                }
                SignalKind::DisasterStruck {
                    settlement_id,
                    severity,
                    ..
                } => {
                    apply_crime_spike(
                        ctx.world,
                        *settlement_id,
                        CRIME_SPIKE_DISASTER * severity,
                        signal_event,
                    );
                }
                _ => {}
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Phase 1: Crime rate computation
// ---------------------------------------------------------------------------

fn update_crime_rates(ctx: &mut TickContext, tick_event: u64) {
    struct CrimeUpdate {
        id: u64,
        new_crime: f64,
    }

    let default_capacity: u64 = 500;

    let updates: Vec<CrimeUpdate> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
        .filter_map(|e| {
            let sd = e.data.as_settlement()?;

            // Skip bandit hideouts
            let faction_id = e.active_rel(RelationshipKind::MemberOf)?;
            if is_bandit_faction(ctx.world, faction_id) {
                return None;
            }

            let prosperity = sd.prosperity;
            let capacity = e.extra_u64_or(K::CAPACITY, default_capacity) as f64;
            let overcrowding = if capacity > 0.0 {
                (sd.population as f64 / capacity - 0.8).max(0.0) / 0.2
            } else {
                0.0
            }
            .min(1.0);

            let stability = helpers::faction_stability(ctx.world, faction_id);

            let target = ((1.0 - prosperity) * CRIME_POVERTY_WEIGHT
                + overcrowding * CRIME_OVERCROWDING_WEIGHT
                + (1.0 - stability) * CRIME_INSTABILITY_WEIGHT
                + sd.bandit_threat * CRIME_BANDIT_THREAT_WEIGHT
                - sd.guard_strength * CRIME_GUARD_REDUCTION)
                .clamp(0.0, 1.0);

            let new_crime =
                (sd.crime_rate + (target - sd.crime_rate) * CRIME_CONVERGENCE_RATE).clamp(0.0, 1.0);

            Some(CrimeUpdate {
                id: e.id,
                new_crime,
            })
        })
        .collect();

    for u in updates {
        let old = ctx.world.settlement(u.id).crime_rate;
        ctx.world.settlement_mut(u.id).crime_rate = u.new_crime;
        ctx.world.record_change(
            u.id,
            tick_event,
            "crime_rate",
            serde_json::json!(old),
            serde_json::json!(u.new_crime),
        );
    }
}

// ---------------------------------------------------------------------------
// Phase 2: Guard strength
// ---------------------------------------------------------------------------

fn update_guard_strength(ctx: &mut TickContext, tick_event: u64) {
    struct GuardUpdate {
        settlement_id: u64,
        faction_id: u64,
        new_strength: f64,
    }

    let updates: Vec<GuardUpdate> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
        .filter_map(|e| {
            let sd = e.data.as_settlement()?;
            let faction_id = e.active_rel(RelationshipKind::MemberOf)?;

            // BanditClan factions don't patrol
            if is_bandit_faction(ctx.world, faction_id) {
                return None;
            }

            let fd = ctx.world.entities.get(&faction_id)?.data.as_faction()?;

            let can_afford = fd.treasury >= GUARD_COST_PER_SETTLEMENT;
            let treasury_factor = if can_afford {
                (fd.treasury / 50.0).min(1.0)
            } else {
                0.0
            };

            let strength = (GUARD_BASE_STRENGTH
                + treasury_factor * GUARD_TREASURY_FACTOR
                + sd.fortification_level as f64 * GUARD_FORTIFICATION_BONUS)
                .clamp(0.0, 1.0);

            Some(GuardUpdate {
                settlement_id: e.id,
                faction_id,
                new_strength: strength,
            })
        })
        .collect();

    // Deduct costs and apply strengths
    let mut faction_costs: std::collections::BTreeMap<u64, f64> = std::collections::BTreeMap::new();
    for u in &updates {
        *faction_costs.entry(u.faction_id).or_default() += GUARD_COST_PER_SETTLEMENT;
    }

    // Deduct guard costs from faction treasuries
    for (&fid, &cost) in &faction_costs {
        if let Some(entity) = ctx.world.entities.get_mut(&fid)
            && let Some(fd) = entity.data.as_faction_mut()
            && fd.treasury >= cost
        {
            let old = fd.treasury;
            fd.treasury -= cost;
            ctx.world.record_change(
                fid,
                tick_event,
                "treasury",
                serde_json::json!(old),
                serde_json::json!(old - cost),
            );
        }
    }

    for u in updates {
        let old = ctx.world.settlement(u.settlement_id).guard_strength;
        ctx.world.settlement_mut(u.settlement_id).guard_strength = u.new_strength;
        ctx.world.record_change(
            u.settlement_id,
            tick_event,
            "guard_strength",
            serde_json::json!(old),
            serde_json::json!(u.new_strength),
        );
    }
}

// ---------------------------------------------------------------------------
// Phase 3: Bandit gang formation
// ---------------------------------------------------------------------------

fn form_bandit_gangs(
    ctx: &mut TickContext,
    time: SimTimestamp,
    current_year: u32,
    _tick_event: u64,
) {
    struct FormationCandidate {
        settlement_id: u64,
        region_id: u64,
    }

    let candidates: Vec<FormationCandidate> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
        .filter_map(|e| {
            let sd = e.data.as_settlement()?;
            if sd.crime_rate <= BANDIT_FORMATION_CRIME_THRESHOLD {
                return None;
            }
            let region_id = e.active_rel(RelationshipKind::LocatedIn)?;

            // Check no existing bandit faction in this region
            if has_bandit_faction_in_region(ctx.world, region_id) {
                return None;
            }

            Some(FormationCandidate {
                settlement_id: e.id,
                region_id,
            })
        })
        .collect();

    for c in candidates {
        if ctx.rng.random_range(0.0..1.0) >= BANDIT_FORMATION_CHANCE {
            continue;
        }

        let strength = ctx
            .rng
            .random_range(BANDIT_MIN_STRENGTH..=BANDIT_MAX_STRENGTH);

        let gang_name = generate_bandit_name(ctx.rng);

        let ev = ctx.world.add_event(
            EventKind::Custom("bandit_gang_formed".to_string()),
            time,
            format!("The {gang_name} formed near region in year {current_year}"),
        );

        // Create faction
        let faction_id = ctx.world.add_entity(
            EntityKind::Faction,
            gang_name.clone(),
            Some(time),
            EntityData::Faction(FactionData {
                government_type: GovernmentType::BanditClan,
                stability: 0.5,
                happiness: 0.5,
                legitimacy: 0.0,
                treasury: 0.0,
                alliance_strength: 0.0,
                primary_culture: None,
                prestige: 0.0,
            }),
            ev,
        );

        // Create hideout settlement (pop 0)
        let hideout_id = ctx.world.add_entity(
            EntityKind::Settlement,
            format!("{gang_name} Hideout"),
            Some(time),
            EntityData::Settlement(SettlementData {
                population: 0,
                population_breakdown: PopulationBreakdown::empty(),
                x: 0.0,
                y: 0.0,
                resources: Vec::new(),
                prosperity: 0.0,
                treasury: 0.0,
                dominant_culture: None,
                culture_makeup: std::collections::BTreeMap::new(),
                cultural_tension: 0.0,
                active_disease: None,
                plague_immunity: 0.0,
                fortification_level: 0,
                active_siege: None,
                prestige: 0.0,
                active_disaster: None,
                crime_rate: 0.0,
                guard_strength: 0.0,
                bandit_threat: 0.0,
            }),
            ev,
        );
        ctx.world
            .add_relationship(hideout_id, faction_id, RelationshipKind::MemberOf, time, ev);
        ctx.world.add_relationship(
            hideout_id,
            c.region_id,
            RelationshipKind::LocatedIn,
            time,
            ev,
        );

        // Create army
        let army_id = ctx.world.add_entity(
            EntityKind::Army,
            format!("{gang_name} Warband"),
            Some(time),
            EntityData::Army(ArmyData {
                strength,
                morale: 0.8,
                supply: 3.0,
            }),
            ev,
        );
        ctx.world
            .add_relationship(army_id, faction_id, RelationshipKind::MemberOf, time, ev);
        ctx.world
            .add_relationship(army_id, c.region_id, RelationshipKind::LocatedIn, time, ev);
        ctx.world
            .set_extra(army_id, K::FACTION_ID, serde_json::json!(faction_id), ev);
        ctx.world.set_extra(
            army_id,
            K::HOME_REGION_ID,
            serde_json::json!(c.region_id),
            ev,
        );

        // Create leader
        let leader_name = crate::sim::names::generate_unique_person_name(ctx.world, ctx.rng);
        let leader_id = ctx.world.add_entity(
            EntityKind::Person,
            leader_name,
            Some(time),
            EntityData::Person(crate::model::entity_data::PersonData {
                birth_year: current_year.saturating_sub(ctx.rng.random_range(20..40)),
                sex: crate::model::entity_data::Sex::Male,
                role: Role::Warrior,
                traits: vec![Trait::Aggressive],
                last_action_year: 0,
                culture_id: None,
                prestige: 0.0,
            }),
            ev,
        );
        ctx.world
            .add_relationship(leader_id, faction_id, RelationshipKind::MemberOf, time, ev);
        ctx.world
            .add_relationship(leader_id, faction_id, RelationshipKind::LeaderOf, time, ev);
        ctx.world
            .add_relationship(leader_id, hideout_id, RelationshipKind::LocatedIn, time, ev);

        ctx.world
            .add_event_participant(ev, faction_id, ParticipantRole::Subject);
        ctx.world
            .add_event_participant(ev, c.settlement_id, ParticipantRole::Location);

        ctx.signals.push(Signal {
            event_id: ev,
            kind: SignalKind::BanditGangFormed {
                faction_id,
                region_id: c.region_id,
            },
        });
    }
}

// ---------------------------------------------------------------------------
// Phase 4: Trade route raiding
// ---------------------------------------------------------------------------

fn raid_trade_routes(
    ctx: &mut TickContext,
    time: SimTimestamp,
    current_year: u32,
    tick_event: u64,
) {
    // Collect bandit armies and their regions
    struct BanditArmy {
        faction_id: u64,
        region_id: u64,
        strength: u32,
    }

    let bandits: Vec<BanditArmy> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Army && e.end.is_none())
        .filter_map(|e| {
            let faction_id = e.active_rel(RelationshipKind::MemberOf)?;
            if !is_bandit_faction(ctx.world, faction_id) {
                return None;
            }
            let region_id = e.active_rel(RelationshipKind::LocatedIn)?;
            let strength = e.data.as_army()?.strength;
            Some(BanditArmy {
                faction_id,
                region_id,
                strength,
            })
        })
        .collect();

    // For each bandit, find trade routes passing through their region
    struct RaidTarget {
        bandit_faction: u64,
        from_settlement: u64,
        to_settlement: u64,
        bandit_strength: u32,
    }

    let mut targets: Vec<RaidTarget> = Vec::new();

    for bandit in &bandits {
        // Find settlements with trade routes whose path includes bandit region
        let settlements: Vec<u64> = ctx
            .world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
            .map(|e| e.id)
            .collect();

        for &sid in &settlements {
            let trade_routes: Vec<u64> = ctx
                .world
                .entities
                .get(&sid)
                .map(|e| e.active_rels(RelationshipKind::TradeRoute).collect())
                .unwrap_or_default();

            for target_sid in trade_routes {
                // Check if route path passes through bandit region
                // Trade routes use the path extra — check both endpoints' regions
                // A simple heuristic: if either endpoint is in the bandit region, the route is vulnerable
                let from_region = ctx
                    .world
                    .entities
                    .get(&sid)
                    .and_then(|e| e.active_rel(RelationshipKind::LocatedIn));
                let to_region = ctx
                    .world
                    .entities
                    .get(&target_sid)
                    .and_then(|e| e.active_rel(RelationshipKind::LocatedIn));

                let passes_through = from_region == Some(bandit.region_id)
                    || to_region == Some(bandit.region_id)
                    || helpers::adjacent_regions(ctx.world, bandit.region_id)
                        .iter()
                        .any(|&r| from_region == Some(r) || to_region == Some(r));

                if passes_through {
                    targets.push(RaidTarget {
                        bandit_faction: bandit.faction_id,
                        from_settlement: sid,
                        to_settlement: target_sid,
                        bandit_strength: bandit.strength,
                    });
                }
            }
        }
    }

    // Deduplicate (same route can be found from both endpoints)
    targets.sort_by(|a, b| {
        let key_a = (
            a.bandit_faction,
            a.from_settlement.min(a.to_settlement),
            a.from_settlement.max(a.to_settlement),
        );
        let key_b = (
            b.bandit_faction,
            b.from_settlement.min(b.to_settlement),
            b.from_settlement.max(b.to_settlement),
        );
        key_a.cmp(&key_b)
    });
    targets.dedup_by(|a, b| {
        a.bandit_faction == b.bandit_faction
            && a.from_settlement.min(a.to_settlement) == b.from_settlement.min(b.to_settlement)
            && a.from_settlement.max(a.to_settlement) == b.from_settlement.max(b.to_settlement)
    });

    for target in targets {
        let raid_chance = (RAID_TRADE_BASE_CHANCE
            * (target.bandit_strength as f64 / RAID_TRADE_STRENGTH_SCALE))
            .min(RAID_TRADE_MAX_CHANCE);

        if ctx.rng.random_range(0.0..1.0) >= raid_chance {
            continue;
        }

        // Calculate income lost
        let trade_income = ctx
            .world
            .entities
            .get(&target.from_settlement)
            .and_then(|e| e.extra.get("trade_income"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        let income_lost = trade_income * RAID_TRADE_INCOME_LOSS_FRACTION;

        // Transfer income to bandit treasury
        if income_lost > 0.0
            && let Some(entity) = ctx.world.entities.get_mut(&target.bandit_faction)
            && let Some(fd) = entity.data.as_faction_mut()
        {
            fd.treasury += income_lost;
        }

        // Strong bandits sever the route entirely
        if target.bandit_strength > RAID_TRADE_SEVER_STRENGTH {
            // Sever in both directions
            let has_route = ctx
                .world
                .entities
                .get(&target.from_settlement)
                .is_some_and(|e| {
                    e.has_active_rel(RelationshipKind::TradeRoute, target.to_settlement)
                });
            if has_route {
                ctx.world.end_relationship(
                    target.from_settlement,
                    target.to_settlement,
                    RelationshipKind::TradeRoute,
                    time,
                    tick_event,
                );
            }
            let has_reverse = ctx
                .world
                .entities
                .get(&target.to_settlement)
                .is_some_and(|e| {
                    e.has_active_rel(RelationshipKind::TradeRoute, target.from_settlement)
                });
            if has_reverse {
                ctx.world.end_relationship(
                    target.to_settlement,
                    target.from_settlement,
                    RelationshipKind::TradeRoute,
                    time,
                    tick_event,
                );
            }
        }

        let ev = ctx.world.add_event(
            EventKind::Custom("trade_route_raided".to_string()),
            time,
            format!(
                "Bandits raided trade route in year {current_year}, stealing {income_lost:.1} income"
            ),
        );
        ctx.world
            .add_event_participant(ev, target.bandit_faction, ParticipantRole::Subject);
        ctx.world
            .add_event_participant(ev, target.from_settlement, ParticipantRole::Object);

        ctx.signals.push(Signal {
            event_id: ev,
            kind: SignalKind::TradeRouteRaided {
                bandit_faction_id: target.bandit_faction,
                from_settlement: target.from_settlement,
                to_settlement: target.to_settlement,
                income_lost,
            },
        });
    }
}

// ---------------------------------------------------------------------------
// Phase 5: Settlement raiding
// ---------------------------------------------------------------------------

fn raid_settlements(
    ctx: &mut TickContext,
    time: SimTimestamp,
    current_year: u32,
    _tick_event: u64,
) {
    struct BanditArmy {
        faction_id: u64,
        region_id: u64,
        strength: u32,
    }

    let bandits: Vec<BanditArmy> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Army && e.end.is_none())
        .filter_map(|e| {
            let faction_id = e.active_rel(RelationshipKind::MemberOf)?;
            if !is_bandit_faction(ctx.world, faction_id) {
                return None;
            }
            let region_id = e.active_rel(RelationshipKind::LocatedIn)?;
            let strength = e.data.as_army()?.strength;
            Some(BanditArmy {
                faction_id,
                region_id,
                strength,
            })
        })
        .collect();

    struct RaidResult {
        bandit_faction: u64,
        settlement_id: u64,
        pop_lost: u32,
        treasury_stolen: f64,
    }

    let mut raids: Vec<RaidResult> = Vec::new();

    for bandit in &bandits {
        // Target settlements in same or adjacent regions
        let mut candidate_regions = vec![bandit.region_id];
        candidate_regions.extend(helpers::adjacent_regions(ctx.world, bandit.region_id));

        for &region_id in &candidate_regions {
            let settlements_in_region: Vec<u64> = ctx
                .world
                .entities
                .values()
                .filter(|e| {
                    e.kind == EntityKind::Settlement
                        && e.end.is_none()
                        && e.has_active_rel(RelationshipKind::LocatedIn, region_id)
                })
                .filter_map(|e| {
                    let sd = e.data.as_settlement()?;
                    // Skip bandit hideouts
                    let fid = e.active_rel(RelationshipKind::MemberOf)?;
                    if is_bandit_faction(ctx.world, fid) {
                        return None;
                    }
                    // Only poorly defended settlements
                    if sd.guard_strength >= RAID_SETTLEMENT_GUARD_THRESHOLD {
                        return None;
                    }
                    // Check no army present in region
                    let army_present = ctx.world.entities.values().any(|a| {
                        a.kind == EntityKind::Army
                            && a.end.is_none()
                            && a.has_active_rel(RelationshipKind::LocatedIn, region_id)
                            && a.active_rel(RelationshipKind::MemberOf)
                                .is_some_and(|f| !is_bandit_faction(ctx.world, f))
                    });
                    if army_present {
                        return None;
                    }
                    Some(e.id)
                })
                .collect();

            for sid in settlements_in_region {
                let raid_chance = RAID_SETTLEMENT_BASE_CHANCE
                    * (bandit.strength as f64 / RAID_SETTLEMENT_STRENGTH_SCALE);
                if ctx.rng.random_range(0.0..1.0) >= raid_chance {
                    continue;
                }

                let sd = ctx.world.settlement(sid);
                let pop_loss_frac = ctx
                    .rng
                    .random_range(RAID_SETTLEMENT_POP_LOSS_MIN..RAID_SETTLEMENT_POP_LOSS_MAX);
                let pop_lost = (sd.population as f64 * pop_loss_frac).ceil() as u32;

                let faction_id = ctx
                    .world
                    .entities
                    .get(&sid)
                    .and_then(|e| e.active_rel(RelationshipKind::MemberOf));

                let treasury_stolen = if let Some(fid) = faction_id {
                    let ft = ctx
                        .world
                        .entities
                        .get(&fid)
                        .and_then(|e| e.data.as_faction())
                        .map(|f| f.treasury)
                        .unwrap_or(0.0);
                    (ft * RAID_SETTLEMENT_TREASURY_FRACTION).min(RAID_SETTLEMENT_TREASURY_THEFT)
                } else {
                    0.0
                };

                raids.push(RaidResult {
                    bandit_faction: bandit.faction_id,
                    settlement_id: sid,
                    pop_lost,
                    treasury_stolen,
                });
            }
        }
    }

    for raid in raids {
        // Apply population loss
        let old_pop = ctx.world.settlement(raid.settlement_id).population;
        if raid.pop_lost > 0 && old_pop > raid.pop_lost {
            let sd = ctx.world.settlement_mut(raid.settlement_id);
            sd.population_breakdown
                .scale_to(old_pop.saturating_sub(raid.pop_lost));
            sd.sync_population();
        }

        // Transfer treasury
        if raid.treasury_stolen > 0.0 {
            // Deduct from victim faction
            let victim_faction = ctx
                .world
                .entities
                .get(&raid.settlement_id)
                .and_then(|e| e.active_rel(RelationshipKind::MemberOf));

            if let Some(fid) = victim_faction
                && let Some(entity) = ctx.world.entities.get_mut(&fid)
                && let Some(fd) = entity.data.as_faction_mut()
            {
                fd.treasury = (fd.treasury - raid.treasury_stolen).max(0.0);
            }

            // Add to bandit treasury
            if let Some(entity) = ctx.world.entities.get_mut(&raid.bandit_faction)
                && let Some(fd) = entity.data.as_faction_mut()
            {
                fd.treasury += raid.treasury_stolen;
            }
        }

        let ev = ctx.world.add_event(
            EventKind::Custom("bandit_raid".to_string()),
            time,
            format!(
                "Bandit raid in year {current_year}: {} killed, {:.1} treasury stolen",
                raid.pop_lost, raid.treasury_stolen
            ),
        );
        ctx.world
            .add_event_participant(ev, raid.bandit_faction, ParticipantRole::Subject);
        ctx.world
            .add_event_participant(ev, raid.settlement_id, ParticipantRole::Object);

        ctx.signals.push(Signal {
            event_id: ev,
            kind: SignalKind::BanditRaid {
                bandit_faction_id: raid.bandit_faction,
                settlement_id: raid.settlement_id,
                population_lost: raid.pop_lost,
                treasury_stolen: raid.treasury_stolen,
            },
        });
    }
}

// ---------------------------------------------------------------------------
// Phase 6: Bandit lifecycle (growth, disband, threat propagation)
// ---------------------------------------------------------------------------

fn update_bandit_lifecycle(
    ctx: &mut TickContext,
    time: SimTimestamp,
    _current_year: u32,
    tick_event: u64,
) {
    struct BanditInfo {
        faction_id: u64,
        army_id: u64,
        region_id: u64,
        strength: u32,
    }

    let bandits: Vec<BanditInfo> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Army && e.end.is_none())
        .filter_map(|e| {
            let faction_id = e.active_rel(RelationshipKind::MemberOf)?;
            if !is_bandit_faction(ctx.world, faction_id) {
                return None;
            }
            let region_id = e.active_rel(RelationshipKind::LocatedIn)?;
            let strength = e.data.as_army()?.strength;
            Some(BanditInfo {
                faction_id,
                army_id: e.id,
                region_id,
                strength,
            })
        })
        .collect();

    // Growth: 15% chance/year to gain 5-10 strength
    for b in &bandits {
        if b.strength >= BANDIT_MAX_ARMY_STRENGTH {
            continue;
        }
        if ctx.rng.random_range(0.0..1.0) < BANDIT_GROWTH_CHANCE {
            let growth = ctx.rng.random_range(BANDIT_GROWTH_MIN..=BANDIT_GROWTH_MAX);
            let new_strength = (b.strength + growth).min(BANDIT_MAX_ARMY_STRENGTH);
            if let Some(entity) = ctx.world.entities.get_mut(&b.army_id)
                && let Some(ad) = entity.data.as_army_mut()
            {
                ad.strength = new_strength;
            }
        }
    }

    // Disband: 10% chance/year if all nearby settlements are well-defended
    let mut to_disband: Vec<u64> = Vec::new();
    for b in &bandits {
        let mut regions = vec![b.region_id];
        regions.extend(helpers::adjacent_regions(ctx.world, b.region_id));

        let any_viable_target = regions.iter().any(|&rid| {
            ctx.world.entities.values().any(|e| {
                e.kind == EntityKind::Settlement
                    && e.end.is_none()
                    && e.has_active_rel(RelationshipKind::LocatedIn, rid)
                    && e.data
                        .as_settlement()
                        .is_some_and(|sd| sd.guard_strength < RAID_SETTLEMENT_GUARD_THRESHOLD)
                    && e.active_rel(RelationshipKind::MemberOf)
                        .is_some_and(|f| !is_bandit_faction(ctx.world, f))
            })
        });

        if !any_viable_target && ctx.rng.random_range(0.0..1.0) < BANDIT_DISBAND_CHANCE {
            to_disband.push(b.faction_id);
        }
    }

    for faction_id in to_disband {
        disband_bandit_faction(ctx.world, faction_id, time, tick_event);
    }

    // Update bandit_threat on nearby settlements
    // First, reset all bandit_threat to 0
    let settlement_ids: Vec<u64> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
        .map(|e| e.id)
        .collect();

    for &sid in &settlement_ids {
        ctx.world.settlement_mut(sid).bandit_threat = 0.0;
    }

    // Re-collect living bandits (some may have been disbanded)
    let living_bandits: Vec<(u64, u32)> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Army && e.end.is_none())
        .filter_map(|e| {
            let faction_id = e.active_rel(RelationshipKind::MemberOf)?;
            if !is_bandit_faction(ctx.world, faction_id) {
                return None;
            }
            let region_id = e.active_rel(RelationshipKind::LocatedIn)?;
            let strength = e.data.as_army()?.strength;
            Some((region_id, strength))
        })
        .collect();

    for (region_id, strength) in &living_bandits {
        let threat = (*strength as f64 * BANDIT_THREAT_PER_STRENGTH).min(1.0);
        let mut affected_regions = vec![*region_id];
        affected_regions.extend(helpers::adjacent_regions(ctx.world, *region_id));

        for &rid in &affected_regions {
            for &sid in &settlement_ids {
                let in_region = ctx
                    .world
                    .entities
                    .get(&sid)
                    .is_some_and(|e| e.has_active_rel(RelationshipKind::LocatedIn, rid));
                if in_region {
                    let sd = ctx.world.settlement_mut(sid);
                    sd.bandit_threat = (sd.bandit_threat + threat).min(1.0);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn is_bandit_faction(world: &crate::model::World, faction_id: u64) -> bool {
    world
        .entities
        .get(&faction_id)
        .and_then(|e| e.data.as_faction())
        .is_some_and(|fd| fd.government_type == GovernmentType::BanditClan)
}

fn has_bandit_faction_in_region(world: &crate::model::World, region_id: u64) -> bool {
    world.entities.values().any(|e| {
        e.kind == EntityKind::Army
            && e.end.is_none()
            && e.has_active_rel(RelationshipKind::LocatedIn, region_id)
            && e.active_rel(RelationshipKind::MemberOf)
                .is_some_and(|f| is_bandit_faction(world, f))
    })
}

fn generate_bandit_name(rng: &mut dyn rand::RngCore) -> String {
    let prefix = BANDIT_PREFIXES[rng.random_range(0..BANDIT_PREFIXES.len())];
    let suffix = BANDIT_SUFFIXES[rng.random_range(0..BANDIT_SUFFIXES.len())];
    format!("The {prefix} {suffix}")
}

fn apply_crime_spike(
    world: &mut crate::model::World,
    settlement_id: u64,
    spike: f64,
    event_id: u64,
) {
    let Some(entity) = world.entities.get_mut(&settlement_id) else {
        return;
    };
    let Some(sd) = entity.data.as_settlement_mut() else {
        return;
    };
    let old = sd.crime_rate;
    sd.crime_rate = (sd.crime_rate + spike).clamp(0.0, 1.0);
    world.record_change(
        settlement_id,
        event_id,
        "crime_rate",
        serde_json::json!(old),
        serde_json::json!((old + spike).clamp(0.0, 1.0)),
    );
}

fn disband_bandit_faction(
    world: &mut crate::model::World,
    faction_id: u64,
    time: SimTimestamp,
    event_id: u64,
) {
    // End all entities belonging to this faction
    let member_ids: Vec<u64> = world
        .entities
        .values()
        .filter(|e| e.end.is_none() && e.has_active_rel(RelationshipKind::MemberOf, faction_id))
        .map(|e| e.id)
        .collect();

    for id in member_ids {
        world.end_entity(id, time, event_id);
    }
    world.end_entity(faction_id, time, event_id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Scenario;
    use crate::testutil;

    #[test]
    fn scenario_crime_rate_increases_with_poverty() {
        let mut s = Scenario::at_year(100);
        let region = s.add_region("Plains");
        let faction = s.faction("Kingdom").stability(0.3).treasury(5.0).id();
        let settlement = s
            .settlement("Poor Town", faction, region)
            .population(500)
            .prosperity(0.1)
            .id();
        let mut world = s.build();

        testutil::tick_system(&mut world, &mut CrimeSystem, 100, 42);

        let crime = world.settlement(settlement).crime_rate;
        assert!(
            crime > 0.05,
            "crime should increase with poverty, got {crime}"
        );
    }

    #[test]
    fn scenario_crime_rate_low_in_prosperous_settlement() {
        let mut s = Scenario::at_year(100);
        let region = s.add_region("Plains");
        let faction = s.faction("Kingdom").stability(0.9).treasury(100.0).id();
        let settlement = s
            .settlement("Rich Town", faction, region)
            .population(300)
            .prosperity(0.9)
            .id();
        let mut world = s.build();

        testutil::tick_system(&mut world, &mut CrimeSystem, 100, 42);

        let crime = world.settlement(settlement).crime_rate;
        assert!(
            crime < 0.1,
            "crime should be low in prosperous settlement, got {crime}"
        );
    }

    #[test]
    fn scenario_guard_strength_set_with_treasury() {
        let mut s = Scenario::at_year(100);
        let region = s.add_region("Plains");
        let faction = s.faction("Kingdom").treasury(50.0).id();
        let settlement = s
            .settlement("Town", faction, region)
            .population(300)
            .fortification_level(2)
            .id();
        let mut world = s.build();

        testutil::tick_system(&mut world, &mut CrimeSystem, 100, 42);

        let guard = world.settlement(settlement).guard_strength;
        assert!(
            guard > 0.2,
            "guard strength should be set with treasury, got {guard}"
        );
    }

    #[test]
    fn scenario_bandit_formation_at_high_crime() {
        let mut s = Scenario::at_year(100);
        let region = s.add_region("Wilds");
        let faction = s.faction("Weak Kingdom").stability(0.2).treasury(1.0).id();
        s.settlement("Lawless Town", faction, region)
            .population(500)
            .prosperity(0.1)
            .with(|sd| sd.crime_rate = 0.8);
        let mut world = s.build();

        // Run many ticks to increase chance of formation
        let mut formed = false;
        for seed in 0..50 {
            let signals = testutil::tick_system(&mut world, &mut CrimeSystem, 100, seed);
            if testutil::has_signal(&signals, |sk| {
                matches!(sk, SignalKind::BanditGangFormed { .. })
            }) {
                formed = true;
                break;
            }
            // Reset crime rate for next attempt (since update_crime_rates runs each tick)
            world
                .settlement_mut(
                    *world
                        .entities
                        .values()
                        .find(|e| e.name == "Lawless Town")
                        .unwrap()
                        .relationships
                        .iter()
                        .find(|_| true) // just need the entity id
                        .map(|_| {
                            &world
                                .entities
                                .values()
                                .find(|e| e.name == "Lawless Town")
                                .unwrap()
                                .id
                        })
                        .unwrap(),
                )
                .crime_rate = 0.8;
        }
        assert!(formed, "bandit gang should form at high crime rate");
    }

    #[test]
    fn scenario_bandit_raid_reduces_population() {
        // Create a world with a bandit faction already in place
        let mut s = Scenario::at_year(100);
        let region = s.add_region("Plains");

        // Victim faction
        let victim_faction = s.faction("Villagers").treasury(20.0).id();
        let settlement = s
            .settlement("Village", victim_faction, region)
            .population(500)
            .prosperity(0.3)
            .with(|sd| sd.guard_strength = 0.0)
            .id();

        // Bandit faction
        let bandit_faction = s
            .faction("Bandits")
            .government_type(GovernmentType::BanditClan)
            .id();
        let hideout = s
            .settlement("Hideout", bandit_faction, region)
            .population(0)
            .id();
        s.add_army("Warband", bandit_faction, region, 40);
        let mut world = s.build();

        // Run multiple ticks looking for a raid
        let mut raided = false;
        for seed in 0..50 {
            let signals = testutil::tick_system(&mut world, &mut CrimeSystem, 100, seed);
            if testutil::has_signal(&signals, |sk| matches!(sk, SignalKind::BanditRaid { .. })) {
                raided = true;
                break;
            }
        }
        assert!(raided, "bandits should raid poorly defended settlement");
    }

    #[test]
    fn scenario_bandit_disband_when_no_targets() {
        let mut s = Scenario::at_year(100);
        let region = s.add_region("Plains");

        // Well-defended settlement
        let faction = s.faction("Strong Kingdom").treasury(100.0).id();
        s.settlement("Fort", faction, region)
            .population(500)
            .prosperity(0.8)
            .with(|sd| sd.guard_strength = 0.9)
            .fortification_level(3);

        // Bandit faction
        let bandit_faction = s
            .faction("Bandits")
            .government_type(GovernmentType::BanditClan)
            .id();
        s.settlement("Hideout", bandit_faction, region)
            .population(0);
        s.add_army("Warband", bandit_faction, region, 20);
        let mut world = s.build();

        // Run many ticks — bandits should eventually disband
        let mut disbanded = false;
        for seed in 0..100 {
            testutil::tick_system(&mut world, &mut CrimeSystem, 100 + seed as u32, seed);
            let bandit_alive = world
                .entities
                .get(&bandit_faction)
                .is_some_and(|e| e.is_alive());
            if !bandit_alive {
                disbanded = true;
                break;
            }
        }
        assert!(disbanded, "bandits should disband when no viable targets");
    }

    #[test]
    fn scenario_crime_spike_on_conquest() {
        let mut s = Scenario::at_year(100);
        let region = s.add_region("Plains");
        let faction = s.faction("Victims").id();
        let settlement = s
            .settlement("Conquered Town", faction, region)
            .population(500)
            .prosperity(0.5)
            .id();
        let mut world = s.build();

        let inbox = vec![Signal {
            event_id: 0,
            kind: SignalKind::SettlementCaptured {
                settlement_id: settlement,
                old_faction_id: faction,
                new_faction_id: 999,
            },
        }];

        // Need a valid event for the signal handler
        world.add_event(
            EventKind::Custom("test".to_string()),
            world.current_time,
            "test".to_string(),
        );

        testutil::deliver_signals(&mut world, &mut CrimeSystem, &inbox, 42);

        let crime = world.settlement(settlement).crime_rate;
        assert!(
            crime >= CRIME_SPIKE_CONQUEST - 0.001,
            "crime should spike after conquest, got {crime}"
        );
    }

    #[test]
    fn scenario_trade_route_raided() {
        let mut s = Scenario::at_year(100);
        let region = s.add_region("Plains");

        // Two settlements with a trade route
        let faction = s.faction("Traders").treasury(50.0).id();
        let town_a = s
            .settlement("Town A", faction, region)
            .population(300)
            .prosperity(0.6)
            .id();
        let town_b = s
            .settlement("Town B", faction, region)
            .population(300)
            .prosperity(0.6)
            .id();

        // Bandit faction in same region
        let bandit_faction = s
            .faction("Bandits")
            .government_type(GovernmentType::BanditClan)
            .id();
        s.settlement("Hideout", bandit_faction, region)
            .population(0);
        s.add_army("Warband", bandit_faction, region, 40);

        let mut world = s.build();

        // Add trade route
        let route_event = world.add_event(
            EventKind::Custom("test_route".to_string()),
            world.current_time,
            "test".to_string(),
        );
        world.add_relationship(
            town_a,
            town_b,
            RelationshipKind::TradeRoute,
            world.current_time,
            route_event,
        );
        world.add_relationship(
            town_b,
            town_a,
            RelationshipKind::TradeRoute,
            world.current_time,
            route_event,
        );
        world.set_extra(town_a, "trade_income", serde_json::json!(10.0), route_event);

        // Run multiple ticks
        let mut raided = false;
        for seed in 0..50 {
            let signals = testutil::tick_system(&mut world, &mut CrimeSystem, 100, seed);
            if testutil::has_signal(&signals, |sk| {
                matches!(sk, SignalKind::TradeRouteRaided { .. })
            }) {
                raided = true;
                break;
            }
        }
        assert!(raided, "trade routes should be raided by bandits");
    }

    #[test]
    fn scenario_500_year_deterministic_with_crime() {
        let world1 = testutil::generate_and_run(42, 500, testutil::all_systems());
        let world2 = testutil::generate_and_run(42, 500, testutil::all_systems());
        testutil::assert_deterministic(&world1, &world2);

        // Verify crime system produced observable effects
        let settlements_with_crime = world1
            .entities
            .values()
            .filter(|e| {
                e.kind == crate::model::EntityKind::Settlement
                    && e.end.is_none()
                    && e.data.as_settlement().is_some_and(|sd| sd.crime_rate > 0.0)
            })
            .count();
        assert!(
            settlements_with_crime > 0,
            "some settlements should have non-zero crime after 500 years"
        );
    }
}
