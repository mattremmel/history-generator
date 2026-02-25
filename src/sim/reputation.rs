use rand::Rng;

use super::context::TickContext;
use super::extra_keys as K;
use super::signal::{Signal, SignalKind};
use super::system::{SimSystem, TickFrequency};
use crate::model::traits::Trait;
use crate::model::{
    EntityKind, EventKind, RelationshipKind, Role, SecretMotivation, SiegeOutcome, SimTimestamp,
};
use crate::sim::helpers;

// ---------------------------------------------------------------------------
// Prestige tier thresholds (0=Obscure, 1=Notable, 2=Renowned, 3=Illustrious, 4=Legendary)
// ---------------------------------------------------------------------------
const TIER_LEGENDARY: f64 = 0.8;
const TIER_ILLUSTRIOUS: f64 = 0.6;
const TIER_RENOWNED: f64 = 0.4;
const TIER_NOTABLE: f64 = 0.2;

// ---------------------------------------------------------------------------
// Signal response deltas — war
// ---------------------------------------------------------------------------
const WAR_DECISIVE_WINNER_FACTION_DELTA: f64 = 0.15;
const WAR_DECISIVE_LOSER_FACTION_DELTA: f64 = -0.15;
const WAR_DECISIVE_WINNER_LEADER_DELTA: f64 = 0.10;
const WAR_DECISIVE_LOSER_LEADER_DELTA: f64 = -0.05;
const WAR_MINOR_WINNER_FACTION_DELTA: f64 = 0.05;
const WAR_MINOR_LOSER_FACTION_DELTA: f64 = -0.05;

// ---------------------------------------------------------------------------
// Signal response deltas — conquest and siege
// ---------------------------------------------------------------------------
const CAPTURE_NEW_FACTION_DELTA: f64 = 0.03;
const CAPTURE_OLD_FACTION_DELTA: f64 = -0.05;
const SIEGE_CONQUERED_ATTACKER_DELTA: f64 = 0.05;
const SIEGE_LIFTED_DEFENDER_DELTA: f64 = 0.05;

// ---------------------------------------------------------------------------
// Signal response deltas — buildings, trade, plague, politics
// ---------------------------------------------------------------------------
const BUILDING_CONSTRUCTED_SETTLEMENT_DELTA: f64 = 0.02;
const BUILDING_CONSTRUCTED_FACTION_DELTA: f64 = 0.01;
const BUILDING_UPGRADED_SETTLEMENT_DELTA: f64 = 0.03;
const BUILDING_UPGRADED_FACTION_DELTA: f64 = 0.01;
const TRADE_ROUTE_SETTLEMENT_DELTA: f64 = 0.01;
const TRADE_ROUTE_FACTION_DELTA: f64 = 0.005;
const PLAGUE_ENDED_SETTLEMENT_DELTA: f64 = 0.02;
const FACTION_SPLIT_DELTA: f64 = -0.10;
const CULTURAL_REBELLION_DELTA: f64 = -0.05;
const TREASURY_DEPLETED_DELTA: f64 = -0.05;
const LEADER_DIED_FACTION_DELTA: f64 = -0.03;

// ---------------------------------------------------------------------------
// Signal response deltas — disasters and knowledge
// ---------------------------------------------------------------------------
const DISASTER_STRUCK_SETTLEMENT_BASE: f64 = -0.05;
const DISASTER_FACTION_SEVERITY_THRESHOLD: f64 = 0.5;
const DISASTER_STRUCK_FACTION_DELTA: f64 = -0.03;
const DISASTER_ENDED_SETTLEMENT_DELTA: f64 = 0.02;
const KNOWLEDGE_CREATED_SETTLEMENT_BASE: f64 = 0.01;

// ---------------------------------------------------------------------------
// Signal response deltas — religion
// ---------------------------------------------------------------------------
const SCHISM_PARENT_FACTION_DELTA: f64 = -0.03;
const SCHISM_SETTLEMENT_DELTA: f64 = 0.02;
const PROPHECY_SETTLEMENT_DELTA: f64 = 0.02;
const PROPHECY_PROPHET_DELTA: f64 = 0.05;
const RELIGION_FOUNDED_FOUNDER_DELTA: f64 = 0.03;
const BETRAYAL_FACTION_PRESTIGE_DELTA: f64 = -0.10;
const BETRAYAL_LEADER_PRESTIGE_DELTA: f64 = -0.08;
const BETRAYAL_VICTIM_SYMPATHY_DELTA: f64 = 0.03;
const CRISIS_FACTION_PRESTIGE_HIT: f64 = -0.05;
const CRISIS_LEADER_PRESTIGE_HIT: f64 = -0.03;

// ---------------------------------------------------------------------------
// Person prestige target computation
// ---------------------------------------------------------------------------
const PERSON_BASE_TARGET: f64 = 0.05;
const PERSON_LEADERSHIP_BONUS: f64 = 0.15;
const PERSON_LARGE_TERRITORY_THRESHOLD: usize = 3;
const PERSON_LARGE_TERRITORY_BONUS: f64 = 0.10;
const PERSON_MAJOR_TERRITORY_THRESHOLD: usize = 6;
const PERSON_MAJOR_TERRITORY_BONUS: f64 = 0.10;
const PERSON_WARRIOR_BONUS: f64 = 0.05;
const PERSON_ELDER_BONUS: f64 = 0.04;
const PERSON_SCHOLAR_BONUS: f64 = 0.03;
const PERSON_LONGEVITY_AGE: u32 = 50;
const PERSON_LONGEVITY_BONUS: f64 = 0.02;
const PERSON_LONGEVITY_SCALE_YEARS: f64 = 30.0;
const PERSON_TARGET_MAX: f64 = 0.85;

// ---------------------------------------------------------------------------
// Person trait convergence rate multipliers
// ---------------------------------------------------------------------------
const TRAIT_AMBITIOUS_MULT: f64 = 1.3;
const TRAIT_CHARISMATIC_MULT: f64 = 1.2;
const TRAIT_CONTENT_MULT: f64 = 0.7;
const TRAIT_RECLUSIVE_MULT: f64 = 0.5;

// ---------------------------------------------------------------------------
// Person drift parameters
// ---------------------------------------------------------------------------
const PERSON_BASE_DRIFT_RATE: f64 = 0.10;
const PERSON_NOISE_RANGE: f64 = 0.01;

// ---------------------------------------------------------------------------
// Faction prestige target computation
// ---------------------------------------------------------------------------
const FACTION_BASE_TARGET: f64 = 0.10;
const FACTION_TERRITORY_PER_SETTLEMENT: f64 = 0.05;
const FACTION_TERRITORY_CAP: f64 = 0.30;
const FACTION_PROSPERITY_WEIGHT: f64 = 0.15;
const FACTION_TRADE_PER_ROUTE: f64 = 0.02;
const FACTION_TRADE_CAP: f64 = 0.10;
const FACTION_BUILDING_PER_BUILDING: f64 = 0.01;
const FACTION_BUILDING_CAP: f64 = 0.10;
const FACTION_STABILITY_WEIGHT: f64 = 0.05;
const FACTION_LEGITIMACY_WEIGHT: f64 = 0.05;
const FACTION_LEADER_PRESTIGE_WEIGHT: f64 = 0.10;
const FACTION_TARGET_MAX: f64 = 0.90;

// ---------------------------------------------------------------------------
// Faction drift parameters
// ---------------------------------------------------------------------------
const FACTION_DRIFT_RATE: f64 = 0.12;
const FACTION_NOISE_RANGE: f64 = 0.02;

// ---------------------------------------------------------------------------
// Settlement prestige target computation
// ---------------------------------------------------------------------------
const SETTLEMENT_BASE_TARGET: f64 = 0.05;
const SETTLEMENT_POP_TIER1: u32 = 100;
const SETTLEMENT_POP_TIER1_BONUS: f64 = 0.05;
const SETTLEMENT_POP_TIER2: u32 = 500;
const SETTLEMENT_POP_TIER2_BONUS: f64 = 0.10;
const SETTLEMENT_POP_TIER3: u32 = 1000;
const SETTLEMENT_POP_TIER3_BONUS: f64 = 0.10;
const SETTLEMENT_POP_TIER4: u32 = 2000;
const SETTLEMENT_POP_TIER4_BONUS: f64 = 0.05;
const SETTLEMENT_PROSPERITY_WEIGHT: f64 = 0.10;
const SETTLEMENT_BUILDING_PER_BUILDING: f64 = 0.03;
const SETTLEMENT_BUILDING_CAP: f64 = 0.15;
const SETTLEMENT_FORTIFICATION_PER_LEVEL: f64 = 0.02;
const SETTLEMENT_TRADE_PER_ROUTE: f64 = 0.03;
const SETTLEMENT_TRADE_CAP: f64 = 0.10;
const SETTLEMENT_WRITTEN_LARGE: usize = 30;
const SETTLEMENT_WRITTEN_LARGE_BONUS: f64 = 0.05;
const SETTLEMENT_WRITTEN_MEDIUM: usize = 15;
const SETTLEMENT_WRITTEN_MEDIUM_BONUS: f64 = 0.03;
const SETTLEMENT_WRITTEN_SMALL: usize = 5;
const SETTLEMENT_WRITTEN_SMALL_BONUS: f64 = 0.02;
const SETTLEMENT_SIEGE_PENALTY: f64 = -0.10;
const SETTLEMENT_TARGET_MAX: f64 = 0.85;

// ---------------------------------------------------------------------------
// Settlement drift parameters
// ---------------------------------------------------------------------------
const SETTLEMENT_DRIFT_RATE: f64 = 0.08;
const SETTLEMENT_NOISE_RANGE: f64 = 0.01;

// ---------------------------------------------------------------------------
// Query helper defaults
// ---------------------------------------------------------------------------
const DEFAULT_AVG_PROSPERITY: f64 = 0.3;

pub struct ReputationSystem;

impl SimSystem for ReputationSystem {
    fn name(&self) -> &str {
        "reputation"
    }

    fn frequency(&self) -> TickFrequency {
        TickFrequency::Yearly
    }

    fn tick(&mut self, ctx: &mut TickContext) {
        let time = ctx.world.current_time;
        let year_event = ctx.world.add_event(
            EventKind::Custom("reputation_tick".to_string()),
            time,
            format!("Reputation update in year {}", time.year()),
        );

        update_person_prestige(ctx, time, year_event);
        update_faction_prestige(ctx, time, year_event);
        update_settlement_prestige(ctx, time, year_event);
    }

    fn handle_signals(&mut self, ctx: &mut TickContext) {
        let time = ctx.world.current_time;
        let year_event = ctx.world.add_event(
            EventKind::Custom("reputation_signal".to_string()),
            time,
            format!("Reputation signal processing in year {}", time.year()),
        );

        for signal in ctx.inbox {
            match &signal.kind {
                SignalKind::WarEnded {
                    winner_id,
                    loser_id,
                    decisive,
                    ..
                } => {
                    if *decisive {
                        apply_prestige_delta(
                            ctx.world,
                            *winner_id,
                            WAR_DECISIVE_WINNER_FACTION_DELTA,
                            year_event,
                        );
                        apply_prestige_delta(
                            ctx.world,
                            *loser_id,
                            WAR_DECISIVE_LOSER_FACTION_DELTA,
                            year_event,
                        );
                        // Boost/penalize faction leaders
                        if let Some(leader_id) = helpers::faction_leader(ctx.world, *winner_id) {
                            apply_prestige_delta(
                                ctx.world,
                                leader_id,
                                WAR_DECISIVE_WINNER_LEADER_DELTA,
                                year_event,
                            );
                        }
                        if let Some(leader_id) = helpers::faction_leader(ctx.world, *loser_id) {
                            apply_prestige_delta(
                                ctx.world,
                                leader_id,
                                WAR_DECISIVE_LOSER_LEADER_DELTA,
                                year_event,
                            );
                        }
                    } else {
                        apply_prestige_delta(
                            ctx.world,
                            *winner_id,
                            WAR_MINOR_WINNER_FACTION_DELTA,
                            year_event,
                        );
                        apply_prestige_delta(
                            ctx.world,
                            *loser_id,
                            WAR_MINOR_LOSER_FACTION_DELTA,
                            year_event,
                        );
                    }
                }
                SignalKind::SettlementCaptured {
                    new_faction_id,
                    old_faction_id,
                    ..
                } => {
                    apply_prestige_delta(
                        ctx.world,
                        *new_faction_id,
                        CAPTURE_NEW_FACTION_DELTA,
                        year_event,
                    );
                    apply_prestige_delta(
                        ctx.world,
                        *old_faction_id,
                        CAPTURE_OLD_FACTION_DELTA,
                        year_event,
                    );
                }
                SignalKind::SiegeEnded {
                    attacker_faction_id,
                    defender_faction_id,
                    outcome,
                    ..
                } => {
                    if *outcome == SiegeOutcome::Conquered {
                        apply_prestige_delta(
                            ctx.world,
                            *attacker_faction_id,
                            SIEGE_CONQUERED_ATTACKER_DELTA,
                            year_event,
                        );
                    } else if *outcome == SiegeOutcome::Lifted {
                        apply_prestige_delta(
                            ctx.world,
                            *defender_faction_id,
                            SIEGE_LIFTED_DEFENDER_DELTA,
                            year_event,
                        );
                    }
                }
                SignalKind::BuildingConstructed { settlement_id, .. } => {
                    apply_prestige_delta(
                        ctx.world,
                        *settlement_id,
                        BUILDING_CONSTRUCTED_SETTLEMENT_DELTA,
                        year_event,
                    );
                    if let Some(fid) = helpers::settlement_faction(ctx.world, *settlement_id) {
                        apply_prestige_delta(
                            ctx.world,
                            fid,
                            BUILDING_CONSTRUCTED_FACTION_DELTA,
                            year_event,
                        );
                    }
                }
                SignalKind::BuildingUpgraded { settlement_id, .. } => {
                    apply_prestige_delta(
                        ctx.world,
                        *settlement_id,
                        BUILDING_UPGRADED_SETTLEMENT_DELTA,
                        year_event,
                    );
                    if let Some(fid) = helpers::settlement_faction(ctx.world, *settlement_id) {
                        apply_prestige_delta(
                            ctx.world,
                            fid,
                            BUILDING_UPGRADED_FACTION_DELTA,
                            year_event,
                        );
                    }
                }
                SignalKind::TradeRouteEstablished {
                    from_settlement,
                    to_settlement,
                    from_faction,
                    to_faction,
                    ..
                } => {
                    apply_prestige_delta(
                        ctx.world,
                        *from_settlement,
                        TRADE_ROUTE_SETTLEMENT_DELTA,
                        year_event,
                    );
                    apply_prestige_delta(
                        ctx.world,
                        *to_settlement,
                        TRADE_ROUTE_SETTLEMENT_DELTA,
                        year_event,
                    );
                    apply_prestige_delta(
                        ctx.world,
                        *from_faction,
                        TRADE_ROUTE_FACTION_DELTA,
                        year_event,
                    );
                    apply_prestige_delta(
                        ctx.world,
                        *to_faction,
                        TRADE_ROUTE_FACTION_DELTA,
                        year_event,
                    );
                }
                SignalKind::PlagueEnded { settlement_id, .. } => {
                    apply_prestige_delta(
                        ctx.world,
                        *settlement_id,
                        PLAGUE_ENDED_SETTLEMENT_DELTA,
                        year_event,
                    );
                }
                SignalKind::FactionSplit { old_faction_id, .. } => {
                    apply_prestige_delta(
                        ctx.world,
                        *old_faction_id,
                        FACTION_SPLIT_DELTA,
                        year_event,
                    );
                }
                SignalKind::CulturalRebellion { faction_id, .. } => {
                    apply_prestige_delta(
                        ctx.world,
                        *faction_id,
                        CULTURAL_REBELLION_DELTA,
                        year_event,
                    );
                }
                SignalKind::TreasuryDepleted { faction_id } => {
                    apply_prestige_delta(
                        ctx.world,
                        *faction_id,
                        TREASURY_DEPLETED_DELTA,
                        year_event,
                    );
                }
                SignalKind::EntityDied { entity_id } => {
                    // If a leader died, penalize their faction
                    let faction_ids: Vec<u64> = ctx
                        .world
                        .entities
                        .get(entity_id)
                        .filter(|e| e.kind == EntityKind::Person)
                        .map(|e| {
                            e.active_rels(RelationshipKind::LeaderOf)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    for fid in faction_ids {
                        apply_prestige_delta(ctx.world, fid, LEADER_DIED_FACTION_DELTA, year_event);
                    }
                }
                SignalKind::DisasterStruck {
                    settlement_id,
                    severity,
                    ..
                } => {
                    // Disaster reduces settlement prestige based on severity
                    apply_prestige_delta(
                        ctx.world,
                        *settlement_id,
                        DISASTER_STRUCK_SETTLEMENT_BASE * severity,
                        year_event,
                    );
                    // Large disasters also affect the owning faction
                    if *severity > DISASTER_FACTION_SEVERITY_THRESHOLD
                        && let Some(faction_id) =
                            helpers::settlement_faction(ctx.world, *settlement_id)
                    {
                        apply_prestige_delta(
                            ctx.world,
                            faction_id,
                            DISASTER_STRUCK_FACTION_DELTA,
                            year_event,
                        );
                    }
                }
                SignalKind::DisasterEnded { settlement_id, .. } => {
                    // Surviving a disaster shows resilience
                    apply_prestige_delta(
                        ctx.world,
                        *settlement_id,
                        DISASTER_ENDED_SETTLEMENT_DELTA,
                        year_event,
                    );
                }
                SignalKind::KnowledgeCreated {
                    settlement_id,
                    significance,
                    ..
                } => {
                    // Knowledge creation gives small prestige to origin settlement
                    apply_prestige_delta(
                        ctx.world,
                        *settlement_id,
                        KNOWLEDGE_CREATED_SETTLEMENT_BASE * significance,
                        year_event,
                    );
                }
                SignalKind::BanditGangFormed { region_id, .. } => {
                    // Prestige hit to faction owning region
                    let affected: Vec<u64> = ctx
                        .world
                        .entities
                        .values()
                        .filter(|e| {
                            e.kind == EntityKind::Settlement
                                && e.end.is_none()
                                && e.has_active_rel(RelationshipKind::LocatedIn, *region_id)
                        })
                        .filter_map(|e| e.active_rel(RelationshipKind::MemberOf))
                        .collect();
                    for fid in affected {
                        apply_prestige_delta(ctx.world, fid, -0.05, year_event);
                    }
                }
                SignalKind::BanditRaid { settlement_id, .. } => {
                    if let Some(fid) = helpers::settlement_faction(ctx.world, *settlement_id) {
                        apply_prestige_delta(ctx.world, fid, -0.03, year_event);
                    }
                }
                SignalKind::ItemTierPromoted {
                    item_id, new_tier, ..
                } if *new_tier >= 2 => {
                    // High-tier items boost their holder's prestige
                    let delta = if *new_tier >= 3 { 0.08 } else { 0.03 };
                    if let Some(holder_id) = ctx
                        .world
                        .entities
                        .get(item_id)
                        .and_then(|e| e.active_rel(RelationshipKind::HeldBy))
                    {
                        apply_prestige_delta(ctx.world, holder_id, delta, year_event);
                    }
                }
                SignalKind::ReligionSchism { settlement_id, .. } => {
                    apply_prestige_delta(
                        ctx.world,
                        *settlement_id,
                        SCHISM_SETTLEMENT_DELTA,
                        year_event,
                    );
                    if let Some(fid) = helpers::settlement_faction(ctx.world, *settlement_id) {
                        apply_prestige_delta(
                            ctx.world,
                            fid,
                            SCHISM_PARENT_FACTION_DELTA,
                            year_event,
                        );
                    }
                }
                SignalKind::ProphecyDeclared {
                    settlement_id,
                    prophet_id,
                    ..
                } => {
                    apply_prestige_delta(
                        ctx.world,
                        *settlement_id,
                        PROPHECY_SETTLEMENT_DELTA,
                        year_event,
                    );
                    if let Some(pid) = prophet_id {
                        apply_prestige_delta(ctx.world, *pid, PROPHECY_PROPHET_DELTA, year_event);
                    }
                }
                SignalKind::ReligionFounded {
                    founder_id: Some(fid),
                    ..
                } => {
                    apply_prestige_delta(
                        ctx.world,
                        *fid,
                        RELIGION_FOUNDED_FOUNDER_DELTA,
                        year_event,
                    );
                }
                SignalKind::AllianceBetrayed {
                    betrayer_faction_id,
                    victim_faction_id,
                    betrayer_leader_id,
                } => {
                    apply_prestige_delta(
                        ctx.world,
                        *betrayer_faction_id,
                        BETRAYAL_FACTION_PRESTIGE_DELTA,
                        year_event,
                    );
                    apply_prestige_delta(
                        ctx.world,
                        *betrayer_leader_id,
                        BETRAYAL_LEADER_PRESTIGE_DELTA,
                        year_event,
                    );
                    apply_prestige_delta(
                        ctx.world,
                        *victim_faction_id,
                        BETRAYAL_VICTIM_SYMPATHY_DELTA,
                        year_event,
                    );
                }
                SignalKind::SuccessionCrisis {
                    faction_id,
                    new_leader_id,
                    ..
                } => {
                    apply_prestige_delta(
                        ctx.world,
                        *faction_id,
                        CRISIS_FACTION_PRESTIGE_HIT,
                        year_event,
                    );
                    apply_prestige_delta(
                        ctx.world,
                        *new_leader_id,
                        CRISIS_LEADER_PRESTIGE_HIT,
                        year_event,
                    );
                }
                SignalKind::SecretRevealed {
                    keeper_id,
                    motivation,
                    sensitivity,
                    ..
                } => {
                    let faction_delta = match motivation {
                        SecretMotivation::Shameful => -0.08 * sensitivity,
                        SecretMotivation::Strategic => -0.03 * sensitivity,
                        SecretMotivation::Sacred => -0.02 * sensitivity,
                        SecretMotivation::Dangerous => 0.0,
                    };
                    if faction_delta != 0.0 {
                        apply_prestige_delta(ctx.world, *keeper_id, faction_delta, year_event);
                    }
                    // Also hit the keeper's leader
                    if let SecretMotivation::Shameful = motivation
                        && let Some(leader_id) = helpers::faction_leader(ctx.world, *keeper_id)
                    {
                        apply_prestige_delta(
                            ctx.world,
                            leader_id,
                            -0.05 * sensitivity,
                            year_event,
                        );
                    }
                }
                _ => {}
            }
        }

        // Check for tier changes and emit threshold signals
        emit_threshold_signals(ctx, year_event);
    }
}

// ---------------------------------------------------------------------------
// Prestige tiers
// ---------------------------------------------------------------------------

fn prestige_tier(prestige: f64) -> u8 {
    match prestige {
        p if p >= TIER_LEGENDARY => 4,
        p if p >= TIER_ILLUSTRIOUS => 3,
        p if p >= TIER_RENOWNED => 2,
        p if p >= TIER_NOTABLE => 1,
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// Person prestige convergence
// ---------------------------------------------------------------------------

fn update_person_prestige(ctx: &mut TickContext, time: SimTimestamp, year_event: u64) {
    struct PersonInfo {
        id: u64,
        old_prestige: f64,
        target: f64,
        convergence_rate: f64,
    }

    let current_year = time.year();

    // Collect person info
    let persons: Vec<PersonInfo> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Person && e.end.is_none())
        .filter_map(|e| {
            let pd = e.data.as_person()?;
            // Only track prestige for notable NPCs (those with traits)
            if pd.traits.is_empty() {
                return None;
            }

            let mut base_target = PERSON_BASE_TARGET;

            // Leadership bonus
            let leader_faction = e.active_rel(RelationshipKind::LeaderOf);
            if let Some(faction_id) = leader_faction {
                base_target += PERSON_LEADERSHIP_BONUS;
                // Count settlements belonging to their faction
                let settlement_count = helpers::faction_settlements(ctx.world, faction_id).len();
                if settlement_count >= PERSON_LARGE_TERRITORY_THRESHOLD {
                    base_target += PERSON_LARGE_TERRITORY_BONUS;
                }
                if settlement_count >= PERSON_MAJOR_TERRITORY_THRESHOLD {
                    base_target += PERSON_MAJOR_TERRITORY_BONUS;
                }
            }

            // Role bonus
            match pd.role {
                Role::Warrior => base_target += PERSON_WARRIOR_BONUS,
                Role::Elder => base_target += PERSON_ELDER_BONUS,
                Role::Scholar => base_target += PERSON_SCHOLAR_BONUS,
                _ => {}
            }

            // Longevity bonus
            if current_year > pd.born.year() {
                let age = current_year - pd.born.year();
                if age >= PERSON_LONGEVITY_AGE {
                    base_target += PERSON_LONGEVITY_BONUS
                        * ((age - PERSON_LONGEVITY_AGE) as f64 / PERSON_LONGEVITY_SCALE_YEARS)
                            .min(1.0);
                }
            }

            let target = base_target.clamp(0.0, PERSON_TARGET_MAX);

            // Trait-based convergence rate modifier
            let mut trait_mult = 1.0;
            for t in &pd.traits {
                match t {
                    Trait::Ambitious => trait_mult *= TRAIT_AMBITIOUS_MULT,
                    Trait::Charismatic => trait_mult *= TRAIT_CHARISMATIC_MULT,
                    Trait::Content => trait_mult *= TRAIT_CONTENT_MULT,
                    Trait::Reclusive => trait_mult *= TRAIT_RECLUSIVE_MULT,
                    _ => {}
                }
            }

            Some(PersonInfo {
                id: e.id,
                old_prestige: pd.prestige,
                target,
                convergence_rate: PERSON_BASE_DRIFT_RATE * trait_mult,
            })
        })
        .collect();

    // Apply
    for p in persons {
        let noise = ctx
            .rng
            .random_range(-PERSON_NOISE_RANGE..PERSON_NOISE_RANGE);
        let new_prestige =
            (p.old_prestige + (p.target - p.old_prestige) * p.convergence_rate + noise)
                .clamp(0.0, 1.0);

        if let Some(entity) = ctx.world.entities.get_mut(&p.id)
            && let Some(pd) = entity.data.as_person_mut()
        {
            pd.prestige = new_prestige;
        }
        ctx.world.record_change(
            p.id,
            year_event,
            "prestige",
            serde_json::json!(p.old_prestige),
            serde_json::json!(new_prestige),
        );
    }
}

// ---------------------------------------------------------------------------
// Faction prestige convergence
// ---------------------------------------------------------------------------

fn update_faction_prestige(ctx: &mut TickContext, _time: SimTimestamp, year_event: u64) {
    struct FactionInfo {
        id: u64,
        old_prestige: f64,
        target: f64,
    }

    let factions: Vec<FactionInfo> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        .filter_map(|e| {
            let fd = e.data.as_faction()?;
            let faction_id = e.id;

            let mut base_target = FACTION_BASE_TARGET;

            // Territory size
            let settlement_count = helpers::faction_settlements(ctx.world, faction_id).len();
            base_target += (settlement_count as f64 * FACTION_TERRITORY_PER_SETTLEMENT)
                .min(FACTION_TERRITORY_CAP);

            // Average settlement prosperity
            let avg_prosperity = avg_faction_prosperity(ctx.world, faction_id);
            base_target += avg_prosperity * FACTION_PROSPERITY_WEIGHT;

            // Trade routes
            let trade_count = count_faction_trade_routes(ctx.world, faction_id);
            base_target += (trade_count as f64 * FACTION_TRADE_PER_ROUTE).min(FACTION_TRADE_CAP);

            // Infrastructure (buildings)
            let building_count = count_faction_buildings(ctx.world, faction_id);
            base_target +=
                (building_count as f64 * FACTION_BUILDING_PER_BUILDING).min(FACTION_BUILDING_CAP);

            // Governance
            base_target +=
                fd.stability * FACTION_STABILITY_WEIGHT + fd.legitimacy * FACTION_LEGITIMACY_WEIGHT;

            // Leader prestige contribution
            if let Some(leader_prestige) = get_leader_prestige(ctx.world, faction_id) {
                base_target += leader_prestige * FACTION_LEADER_PRESTIGE_WEIGHT;
            }

            let target = base_target.clamp(0.0, FACTION_TARGET_MAX);

            Some(FactionInfo {
                id: faction_id,
                old_prestige: fd.prestige,
                target,
            })
        })
        .collect();

    // Apply
    for f in factions {
        let noise = ctx
            .rng
            .random_range(-FACTION_NOISE_RANGE..FACTION_NOISE_RANGE);
        let new_prestige =
            (f.old_prestige + (f.target - f.old_prestige) * FACTION_DRIFT_RATE + noise)
                .clamp(0.0, 1.0);

        if let Some(entity) = ctx.world.entities.get_mut(&f.id)
            && let Some(fd) = entity.data.as_faction_mut()
        {
            fd.prestige = new_prestige;
        }
        ctx.world.record_change(
            f.id,
            year_event,
            "prestige",
            serde_json::json!(f.old_prestige),
            serde_json::json!(new_prestige),
        );
    }
}

// ---------------------------------------------------------------------------
// Settlement prestige convergence
// ---------------------------------------------------------------------------

fn update_settlement_prestige(ctx: &mut TickContext, _time: SimTimestamp, year_event: u64) {
    struct SettlementInfo {
        id: u64,
        old_prestige: f64,
        target: f64,
    }

    let settlements: Vec<SettlementInfo> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
        .filter_map(|e| {
            let sd = e.data.as_settlement()?;
            let settlement_id = e.id;

            let mut base_target = SETTLEMENT_BASE_TARGET;

            // Population milestones
            let pop = sd.population;
            if pop >= SETTLEMENT_POP_TIER1 {
                base_target += SETTLEMENT_POP_TIER1_BONUS;
            }
            if pop >= SETTLEMENT_POP_TIER2 {
                base_target += SETTLEMENT_POP_TIER2_BONUS;
            }
            if pop >= SETTLEMENT_POP_TIER3 {
                base_target += SETTLEMENT_POP_TIER3_BONUS;
            }
            if pop >= SETTLEMENT_POP_TIER4 {
                base_target += SETTLEMENT_POP_TIER4_BONUS;
            }

            // Prosperity
            base_target += sd.prosperity * SETTLEMENT_PROSPERITY_WEIGHT;

            // Buildings
            let building_count = helpers::settlement_building_count(ctx.world, settlement_id);
            base_target += (building_count as f64 * SETTLEMENT_BUILDING_PER_BUILDING)
                .min(SETTLEMENT_BUILDING_CAP);

            // Fortifications
            base_target += sd.fortification_level as f64 * SETTLEMENT_FORTIFICATION_PER_LEVEL;

            // Trade routes
            let trade_count = count_settlement_trade_routes(e);
            base_target +=
                (trade_count as f64 * SETTLEMENT_TRADE_PER_ROUTE).min(SETTLEMENT_TRADE_CAP);

            // Written manifestations (knowledge/library prestige)
            let written_count = count_settlement_written_manifestations(ctx.world, settlement_id);
            if written_count > SETTLEMENT_WRITTEN_LARGE {
                base_target += SETTLEMENT_WRITTEN_LARGE_BONUS;
            } else if written_count > SETTLEMENT_WRITTEN_MEDIUM {
                base_target += SETTLEMENT_WRITTEN_MEDIUM_BONUS;
            } else if written_count > SETTLEMENT_WRITTEN_SMALL {
                base_target += SETTLEMENT_WRITTEN_SMALL_BONUS;
            }

            // Siege penalty
            if sd.active_siege.is_some() {
                base_target += SETTLEMENT_SIEGE_PENALTY;
            }

            let target = base_target.clamp(0.0, SETTLEMENT_TARGET_MAX);

            Some(SettlementInfo {
                id: settlement_id,
                old_prestige: sd.prestige,
                target,
            })
        })
        .collect();

    // Apply
    for s in settlements {
        let noise = ctx
            .rng
            .random_range(-SETTLEMENT_NOISE_RANGE..SETTLEMENT_NOISE_RANGE);
        let new_prestige =
            (s.old_prestige + (s.target - s.old_prestige) * SETTLEMENT_DRIFT_RATE + noise)
                .clamp(0.0, 1.0);

        if let Some(entity) = ctx.world.entities.get_mut(&s.id)
            && let Some(sd) = entity.data.as_settlement_mut()
        {
            sd.prestige = new_prestige;
        }
        ctx.world.record_change(
            s.id,
            year_event,
            "prestige",
            serde_json::json!(s.old_prestige),
            serde_json::json!(new_prestige),
        );
    }
}

// ---------------------------------------------------------------------------
// Delta helpers
// ---------------------------------------------------------------------------

fn apply_prestige_delta(
    world: &mut crate::model::World,
    entity_id: u64,
    delta: f64,
    event_id: u64,
) {
    let (old, new) = {
        let Some(entity) = world.entities.get_mut(&entity_id) else {
            return;
        };
        let prestige_ref = match &mut entity.data {
            crate::model::EntityData::Person(d) => &mut d.prestige,
            crate::model::EntityData::Faction(d) => &mut d.prestige,
            crate::model::EntityData::Settlement(d) => &mut d.prestige,
            _ => return,
        };
        let old = *prestige_ref;
        *prestige_ref = (*prestige_ref + delta).clamp(0.0, 1.0);
        (old, *prestige_ref)
    };
    world.record_change(
        entity_id,
        event_id,
        "prestige",
        serde_json::json!(old),
        serde_json::json!(new),
    );
}

// ---------------------------------------------------------------------------
// Threshold signal emission
// ---------------------------------------------------------------------------

fn emit_threshold_signals(ctx: &mut TickContext, event_id: u64) {
    // We need to track old tiers — store them before tick in a pre-pass.
    // Since handle_signals runs after tick, the prestige values have already
    // been updated by both tick() and signal deltas. We check extras for
    // the previous tier, stored at end of each cycle.
    for e in ctx.world.entities.values() {
        if e.end.is_some() {
            continue;
        }

        let current_prestige = match e.kind {
            EntityKind::Person => e.data.as_person().map(|p| p.prestige),
            EntityKind::Faction => e.data.as_faction().map(|f| f.prestige),
            EntityKind::Settlement => e.data.as_settlement().map(|s| s.prestige),
            _ => None,
        };

        if let Some(prestige) = current_prestige {
            let new_tier = prestige_tier(prestige);
            let old_tier = e.extra_u64(K::PRESTIGE_TIER).map(|v| v as u8).unwrap_or(0);

            if new_tier != old_tier {
                ctx.signals.push(Signal {
                    event_id,
                    kind: SignalKind::PrestigeThresholdCrossed {
                        entity_id: e.id,
                        old_tier,
                        new_tier,
                    },
                });
            }
        }
    }

    // Update stored tiers for next cycle
    let tier_updates: Vec<(u64, u8)> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.end.is_none())
        .filter_map(|e| {
            let prestige = match e.kind {
                EntityKind::Person => e.data.as_person().map(|p| p.prestige),
                EntityKind::Faction => e.data.as_faction().map(|f| f.prestige),
                EntityKind::Settlement => e.data.as_settlement().map(|s| s.prestige),
                _ => None,
            }?;
            Some((e.id, prestige_tier(prestige)))
        })
        .collect();

    for (id, tier) in tier_updates {
        ctx.world
            .set_extra(id, K::PRESTIGE_TIER, serde_json::json!(tier), event_id);
    }
}

// ---------------------------------------------------------------------------
// Query helpers
// ---------------------------------------------------------------------------

/// Average prosperity of a faction's settlements.
fn avg_faction_prosperity(world: &crate::model::World, faction_id: u64) -> f64 {
    let mut sum = 0.0;
    let mut count = 0u32;
    for e in world.entities.values() {
        if e.kind == EntityKind::Settlement
            && e.end.is_none()
            && e.has_active_rel(RelationshipKind::MemberOf, faction_id)
        {
            if let Some(sd) = e.data.as_settlement() {
                sum += sd.prosperity;
            }
            count += 1;
        }
    }
    if count > 0 {
        sum / count as f64
    } else {
        DEFAULT_AVG_PROSPERITY
    }
}

/// Count trade routes across all faction settlements.
fn count_faction_trade_routes(world: &crate::model::World, faction_id: u64) -> usize {
    let mut count = 0;
    for e in world.entities.values() {
        if e.kind == EntityKind::Settlement
            && e.end.is_none()
            && e.has_active_rel(RelationshipKind::MemberOf, faction_id)
        {
            count += e.active_rels(RelationshipKind::TradeRoute).count();
        }
    }
    count
}

/// Count buildings belonging to a faction's settlements.
fn count_faction_buildings(world: &crate::model::World, faction_id: u64) -> usize {
    let settlement_ids: std::collections::BTreeSet<u64> = world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Settlement
                && e.end.is_none()
                && e.has_active_rel(RelationshipKind::MemberOf, faction_id)
        })
        .map(|e| e.id)
        .collect();

    world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Building
                && e.end.is_none()
                && e.active_rels(RelationshipKind::LocatedIn)
                    .any(|t| settlement_ids.contains(&t))
        })
        .count()
}

/// Count active trade routes on a settlement entity.
fn count_settlement_trade_routes(entity: &crate::model::Entity) -> usize {
    entity.active_rels(RelationshipKind::TradeRoute).count()
}

/// Get prestige of a faction's leader.
fn get_leader_prestige(world: &crate::model::World, faction_id: u64) -> Option<f64> {
    let leader_id = helpers::faction_leader(world, faction_id)?;
    let leader = world.entities.get(&leader_id)?;
    leader.data.as_person().map(|p| p.prestige)
}

/// Count written manifestations (books, scrolls) held by a settlement.
fn count_settlement_written_manifestations(
    world: &crate::model::World,
    settlement_id: u64,
) -> usize {
    world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Manifestation
                && e.end.is_none()
                && e.data.as_manifestation().is_some_and(|md| {
                    matches!(
                        md.medium,
                        crate::model::Medium::WrittenBook
                            | crate::model::Medium::Scroll
                            | crate::model::Medium::EncodedCipher
                    )
                })
                && e.has_active_rel(RelationshipKind::HeldBy, settlement_id)
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Scenario;
    use crate::testutil::{
        PoliticalSetup, assert_approx, deliver_signals, has_signal, political_scenario, tick_system,
    };
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    #[test]
    fn prestige_tier_thresholds() {
        assert_eq!(prestige_tier(0.0), 0);
        assert_eq!(prestige_tier(0.19), 0);
        assert_eq!(prestige_tier(0.2), 1);
        assert_eq!(prestige_tier(0.39), 1);
        assert_eq!(prestige_tier(0.4), 2);
        assert_eq!(prestige_tier(0.59), 2);
        assert_eq!(prestige_tier(0.6), 3);
        assert_eq!(prestige_tier(0.79), 3);
        assert_eq!(prestige_tier(0.8), 4);
        assert_eq!(prestige_tier(1.0), 4);
    }

    #[test]
    fn scenario_leader_prestige_converges_upward() {
        let PoliticalSetup {
            mut world, leader, ..
        } = political_scenario();

        for year in 100..120 {
            tick_system(&mut world, &mut ReputationSystem, year, 42);
        }

        let prestige = world.person(leader).prestige;
        assert!(
            prestige > 0.15,
            "leader prestige should rise, got {prestige}"
        );
    }

    #[test]
    fn scenario_non_leader_prestige_stays_low() {
        let mut s = Scenario::at_year(100);
        let setup = s.add_settlement_standalone("Town");
        let faction = setup.faction;
        let commoner = s
            .person("Commoner", faction)
            .role(Role::Common)
            .traits(vec![Trait::Content])
            .id();
        let mut world = s.build();

        let year_event = world.add_event(
            EventKind::Custom("test".to_string()),
            world.current_time,
            "test".to_string(),
        );
        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals = Vec::new();
        let mut ctx = TickContext {
            world: &mut world,
            rng: &mut rng,
            signals: &mut signals,
            inbox: &[],
        };

        for _ in 0..20 {
            let time = ctx.world.current_time;
            update_person_prestige(&mut ctx, time, year_event);
        }

        let prestige = ctx.world.person(commoner).prestige;
        assert!(
            prestige < 0.15,
            "commoner prestige should stay low, got {prestige}"
        );
    }

    #[test]
    fn scenario_faction_prestige_scales_with_territory() {
        let mut s = Scenario::at_year(100);
        let region = s.add_region("Plains");
        let small_faction = s.add_faction("Small");
        s.add_settlement("Town", small_faction, region);

        let large_faction = s.add_faction("Large");
        for i in 0..5 {
            let r = s.add_region(&format!("Region{i}"));
            s.add_settlement(&format!("City{i}"), large_faction, r);
        }
        let mut world = s.build();

        let year_event = world.add_event(
            EventKind::Custom("test".to_string()),
            world.current_time,
            "test".to_string(),
        );
        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals = Vec::new();
        let mut ctx = TickContext {
            world: &mut world,
            rng: &mut rng,
            signals: &mut signals,
            inbox: &[],
        };

        for _ in 0..30 {
            let time = ctx.world.current_time;
            update_faction_prestige(&mut ctx, time, year_event);
        }

        let small_p = ctx.world.faction(small_faction).prestige;
        let large_p = ctx.world.faction(large_faction).prestige;
        assert!(
            large_p > small_p,
            "larger faction should have more prestige: large={large_p} small={small_p}"
        );
    }

    #[test]
    fn scenario_settlement_prestige_scales_with_population() {
        let mut s = Scenario::at_year(100);
        let region = s.add_region("Plains");
        let faction = s.add_faction("Kingdom");
        let village = s.settlement("Village", faction, region).population(50).id();
        let city = s
            .settlement("City", faction, region)
            .population(1500)
            .prosperity(0.7)
            .fortification_level(2)
            .id();
        let mut world = s.build();

        let year_event = world.add_event(
            EventKind::Custom("test".to_string()),
            world.current_time,
            "test".to_string(),
        );
        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals = Vec::new();
        let mut ctx = TickContext {
            world: &mut world,
            rng: &mut rng,
            signals: &mut signals,
            inbox: &[],
        };

        for _ in 0..30 {
            let time = ctx.world.current_time;
            update_settlement_prestige(&mut ctx, time, year_event);
        }

        let village_p = ctx.world.settlement(village).prestige;
        let city_p = ctx.world.settlement(city).prestige;
        assert!(
            city_p > village_p,
            "city should have more prestige: city={city_p} village={village_p}"
        );
    }

    #[test]
    fn scenario_war_victory_boosts_faction_prestige() {
        let mut s = Scenario::at_year(100);
        let region = s.add_region("Plains");
        let winner = s.faction("Winners").prestige(0.3).id();
        s.add_settlement("Capital", winner, region);
        s.add_settlement("Town", winner, region);
        let loser = s.faction("Losers").prestige(0.3).id();
        s.add_settlement("Outpost", loser, region);
        s.add_settlement("Village", loser, region);
        let mut world = s.build();

        let inbox = vec![Signal {
            event_id: 0,
            kind: SignalKind::WarEnded {
                winner_id: winner,
                loser_id: loser,
                decisive: true,
                reparations: 0.0,
                tribute_years: 0,
            },
        }];

        deliver_signals(&mut world, &mut ReputationSystem, &inbox, 42);

        assert_approx(world.faction(winner).prestige, 0.45, 0.001, "winner +0.15");
        assert_approx(world.faction(loser).prestige, 0.15, 0.001, "loser -0.15");
    }

    #[test]
    fn scenario_threshold_signal_emitted_on_tier_change() {
        let mut s = Scenario::at_year(100);
        let region = s.add_region("Plains");
        let faction = s.faction("Kingdom").prestige(0.19).id();
        s.add_settlement("Capital", faction, region);
        s.add_settlement("Town", faction, region);
        let mut world = s.build();

        let year_event = world.add_event(
            EventKind::Custom("test".to_string()),
            world.current_time,
            "test".to_string(),
        );

        apply_prestige_delta(&mut world, faction, 0.05, year_event);

        let prestige = world.faction(faction).prestige;
        assert!(prestige >= 0.2, "prestige should cross 0.2, got {prestige}");

        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals_out = vec![];
        let mut ctx = TickContext {
            world: &mut world,
            rng: &mut rng,
            signals: &mut signals_out,
            inbox: &[],
        };

        emit_threshold_signals(&mut ctx, year_event);

        assert!(has_signal(&signals_out, |sk| matches!(
            sk,
            SignalKind::PrestigeThresholdCrossed {
                entity_id,
                old_tier: 0,
                new_tier: 1,
            } if *entity_id == faction
        )));
    }

    #[test]
    fn scenario_prestige_stays_bounded_after_extreme_signals() {
        let mut s = Scenario::at_year(100);
        let region = s.add_region("Plains");
        let winner = s.faction("Winners").prestige(0.95).id();
        s.add_settlement("Capital", winner, region);
        let loser = s.faction("Losers").prestige(0.05).id();
        s.add_settlement("Outpost", loser, region);
        let mut world = s.build();

        let inbox = vec![Signal {
            event_id: 0,
            kind: SignalKind::WarEnded {
                winner_id: winner,
                loser_id: loser,
                decisive: true,
                reparations: 100.0,
                tribute_years: 5,
            },
        }];

        deliver_signals(&mut world, &mut ReputationSystem, &inbox, 42);

        let winner_prestige = world.faction(winner).prestige;
        let loser_prestige = world.faction(loser).prestige;
        assert!(
            winner_prestige <= 1.0,
            "winner prestige should be clamped to 1.0, got {winner_prestige}"
        );
        assert!(
            loser_prestige >= 0.0,
            "loser prestige should be clamped to 0.0, got {loser_prestige}"
        );
    }

    // -----------------------------------------------------------------------
    // Signal handler tests (deliver_signals, zero ticks)
    // -----------------------------------------------------------------------

    #[test]
    fn scenario_conquest_prestige_shift() {
        let mut s = Scenario::at_year(100);
        let r = s.add_region("R");
        let new_f = s.faction("Conqueror").prestige(0.5).id();
        let old_f = s.faction("Defender").prestige(0.5).id();
        s.add_settlement("S1", new_f, r);
        s.add_settlement("S2", old_f, r);
        let mut world = s.build();

        let inbox = vec![Signal {
            event_id: 0,
            kind: SignalKind::SettlementCaptured {
                settlement_id: 999,
                old_faction_id: old_f,
                new_faction_id: new_f,
            },
        }];
        deliver_signals(&mut world, &mut ReputationSystem, &inbox, 42);

        assert_approx(
            world.faction(new_f).prestige,
            0.5 + CAPTURE_NEW_FACTION_DELTA,
            0.001,
            "conqueror prestige",
        );
        assert_approx(
            world.faction(old_f).prestige,
            0.5 + CAPTURE_OLD_FACTION_DELTA,
            0.001,
            "defender prestige",
        );
    }

    #[test]
    fn scenario_siege_conquered_prestige() {
        let mut s = Scenario::at_year(100);
        let r = s.add_region("R");
        let attacker = s.faction("Attacker").prestige(0.4).id();
        let defender = s.add_faction("Defender");
        s.add_settlement("S1", attacker, r);
        s.add_settlement("S2", defender, r);
        let mut world = s.build();

        let inbox = vec![Signal {
            event_id: 0,
            kind: SignalKind::SiegeEnded {
                settlement_id: 999,
                attacker_faction_id: attacker,
                defender_faction_id: defender,
                outcome: SiegeOutcome::Conquered,
            },
        }];
        deliver_signals(&mut world, &mut ReputationSystem, &inbox, 42);

        assert_approx(
            world.faction(attacker).prestige,
            0.4 + SIEGE_CONQUERED_ATTACKER_DELTA,
            0.001,
            "attacker prestige after siege",
        );
    }

    #[test]
    fn scenario_building_constructed_prestige() {
        let mut s = Scenario::at_year(100);
        let r = s.add_region("R");
        let f = s.faction("Kingdom").prestige(0.3).id();
        let sett = s
            .settlement("Town", f, r)
            .population(300)
            .prestige(0.2)
            .id();
        let mut world = s.build();

        let inbox = vec![Signal {
            event_id: 0,
            kind: SignalKind::BuildingConstructed {
                building_id: 999,
                settlement_id: sett,
                building_type: crate::model::entity_data::BuildingType::Market,
            },
        }];
        deliver_signals(&mut world, &mut ReputationSystem, &inbox, 42);

        assert_approx(
            world.settlement(sett).prestige,
            0.2 + BUILDING_CONSTRUCTED_SETTLEMENT_DELTA,
            0.001,
            "settlement prestige",
        );
        assert_approx(
            world.faction(f).prestige,
            0.3 + BUILDING_CONSTRUCTED_FACTION_DELTA,
            0.001,
            "faction prestige",
        );
    }

    #[test]
    fn scenario_trade_route_prestige() {
        let mut s = Scenario::at_year(100);
        let r = s.add_region("R");
        let fa = s.faction("FA").prestige(0.3).id();
        let fb = s.faction("FB").prestige(0.3).id();
        let sa = s.settlement("SA", fa, r).population(200).prestige(0.2).id();
        let sb = s.settlement("SB", fb, r).population(200).prestige(0.2).id();
        let mut world = s.build();

        let inbox = vec![Signal {
            event_id: 0,
            kind: SignalKind::TradeRouteEstablished {
                from_settlement: sa,
                to_settlement: sb,
                from_faction: fa,
                to_faction: fb,
            },
        }];
        deliver_signals(&mut world, &mut ReputationSystem, &inbox, 42);

        assert_approx(
            world.settlement(sa).prestige,
            0.2 + TRADE_ROUTE_SETTLEMENT_DELTA,
            0.001,
            "from settlement prestige",
        );
        assert_approx(
            world.settlement(sb).prestige,
            0.2 + TRADE_ROUTE_SETTLEMENT_DELTA,
            0.001,
            "to settlement prestige",
        );
        assert_approx(
            world.faction(fa).prestige,
            0.3 + TRADE_ROUTE_FACTION_DELTA,
            0.001,
            "from faction prestige",
        );
        assert_approx(
            world.faction(fb).prestige,
            0.3 + TRADE_ROUTE_FACTION_DELTA,
            0.001,
            "to faction prestige",
        );
    }

    #[test]
    fn scenario_faction_split_prestige_loss() {
        let mut s = Scenario::at_year(100);
        let r = s.add_region("R");
        let old_f = s.faction("OldFaction").prestige(0.6).id();
        s.add_settlement("S1", old_f, r);
        let mut world = s.build();

        let inbox = vec![Signal {
            event_id: 0,
            kind: SignalKind::FactionSplit {
                old_faction_id: old_f,
                new_faction_id: Some(999),
                settlement_id: 998,
            },
        }];
        deliver_signals(&mut world, &mut ReputationSystem, &inbox, 42);

        assert_approx(
            world.faction(old_f).prestige,
            0.6 + FACTION_SPLIT_DELTA,
            0.001,
            "faction split prestige loss",
        );
    }

    #[test]
    fn scenario_treasury_depleted_prestige() {
        let mut s = Scenario::at_year(100);
        let r = s.add_region("R");
        let f = s.faction("BrokeFaction").prestige(0.4).id();
        s.add_settlement("S1", f, r);
        let mut world = s.build();

        let inbox = vec![Signal {
            event_id: 0,
            kind: SignalKind::TreasuryDepleted { faction_id: f },
        }];
        deliver_signals(&mut world, &mut ReputationSystem, &inbox, 42);

        assert_approx(
            world.faction(f).prestige,
            0.4 + TREASURY_DEPLETED_DELTA,
            0.001,
            "treasury depleted prestige",
        );
    }

    #[test]
    fn scenario_betrayal_prestige_shift() {
        let mut s = Scenario::at_year(100);
        let r = s.add_region("R");
        let betrayer = s.faction("Betrayer").prestige(0.5).id();
        let victim = s.faction("Victim").prestige(0.3).id();
        s.add_settlement("S1", betrayer, r);
        s.add_settlement("S2", victim, r);
        let leader = s.person("Leader", betrayer).prestige(0.5).id();
        let mut world = s.build();

        let inbox = vec![Signal {
            event_id: 0,
            kind: SignalKind::AllianceBetrayed {
                betrayer_faction_id: betrayer,
                victim_faction_id: victim,
                betrayer_leader_id: leader,
            },
        }];
        deliver_signals(&mut world, &mut ReputationSystem, &inbox, 42);

        assert_approx(
            world.faction(betrayer).prestige,
            0.5 + BETRAYAL_FACTION_PRESTIGE_DELTA,
            0.001,
            "betrayer faction prestige",
        );
        assert_approx(
            world.person(leader).prestige,
            0.5 + BETRAYAL_LEADER_PRESTIGE_DELTA,
            0.001,
            "betrayer leader prestige",
        );
        assert_approx(
            world.faction(victim).prestige,
            0.3 + BETRAYAL_VICTIM_SYMPATHY_DELTA,
            0.001,
            "victim sympathy prestige",
        );
    }

    #[test]
    fn scenario_disaster_prestige_loss() {
        let mut s = Scenario::at_year(100);
        let r = s.add_region("R");
        let f = s.add_faction("Kingdom");
        let sett = s
            .settlement("Town", f, r)
            .population(300)
            .prestige(0.5)
            .id();
        let mut world = s.build();

        let severity = 0.8;
        let inbox = vec![Signal {
            event_id: 0,
            kind: SignalKind::DisasterStruck {
                settlement_id: sett,
                region_id: r,
                disaster_type: crate::model::entity_data::DisasterType::Earthquake,
                severity,
            },
        }];
        deliver_signals(&mut world, &mut ReputationSystem, &inbox, 42);

        let expected = 0.5 + DISASTER_STRUCK_SETTLEMENT_BASE * severity;
        assert_approx(
            world.settlement(sett).prestige,
            expected,
            0.001,
            "settlement prestige after disaster",
        );
    }

    #[test]
    fn scenario_building_upgraded_prestige() {
        let mut s = Scenario::at_year(100);
        let r = s.add_region("R");
        let f = s.faction("Kingdom").prestige(0.3).id();
        let sett = s
            .settlement("Town", f, r)
            .population(300)
            .prestige(0.2)
            .id();
        let building = s.add_building(crate::model::entity_data::BuildingType::Market, sett);
        let mut world = s.build();

        let event_id = world.add_event(
            EventKind::Custom("test".to_string()),
            world.current_time,
            "test".to_string(),
        );
        let inbox = vec![Signal {
            event_id,
            kind: SignalKind::BuildingUpgraded {
                building_id: building,
                settlement_id: sett,
                building_type: crate::model::entity_data::BuildingType::Market,
                new_level: 1,
            },
        }];
        deliver_signals(&mut world, &mut ReputationSystem, &inbox, 42);

        assert_approx(
            world.settlement(sett).prestige,
            0.2 + BUILDING_UPGRADED_SETTLEMENT_DELTA,
            0.001,
            "settlement prestige after building upgrade",
        );
        assert_approx(
            world.faction(f).prestige,
            0.3 + BUILDING_UPGRADED_FACTION_DELTA,
            0.001,
            "faction prestige after building upgrade",
        );
    }

    #[test]
    fn scenario_plague_ended_prestige_recovery() {
        let mut s = Scenario::at_year(100);
        let r = s.add_region("R");
        let f = s.add_faction("Kingdom");
        let sett = s
            .settlement("Town", f, r)
            .population(300)
            .prestige(0.1)
            .id();
        let mut world = s.build();

        let event_id = world.add_event(
            EventKind::Custom("test".to_string()),
            world.current_time,
            "test".to_string(),
        );
        let inbox = vec![Signal {
            event_id,
            kind: SignalKind::PlagueEnded {
                settlement_id: sett,
                disease_id: 999,
                deaths: 10,
            },
        }];
        deliver_signals(&mut world, &mut ReputationSystem, &inbox, 42);

        assert_approx(
            world.settlement(sett).prestige,
            0.1 + PLAGUE_ENDED_SETTLEMENT_DELTA,
            0.001,
            "settlement prestige after plague ended",
        );
    }

    #[test]
    fn scenario_entity_died_leader_prestige_hit() {
        let mut s = Scenario::at_year(100);
        let r = s.add_region("R");
        let f = s.faction("Kingdom").prestige(0.5).id();
        s.add_settlement("Capital", f, r);
        let leader = s
            .person("King", f)
            .role(Role::Warrior)
            .traits(vec![Trait::Ambitious])
            .prestige(0.4)
            .id();
        s.make_leader(leader, f);
        let mut world = s.build();

        // End the leader entity (simulating death) — relationships remain active
        let death_event = world.add_event(
            EventKind::Death,
            world.current_time,
            "Leader died".to_string(),
        );
        world.end_entity(leader, world.current_time, death_event);

        let inbox = vec![Signal {
            event_id: death_event,
            kind: SignalKind::EntityDied { entity_id: leader },
        }];
        deliver_signals(&mut world, &mut ReputationSystem, &inbox, 42);

        assert_approx(
            world.faction(f).prestige,
            0.5 + LEADER_DIED_FACTION_DELTA,
            0.001,
            "faction prestige after leader died",
        );
    }
}
