//! Reputation system — migrated from `src/sim/reputation.rs`.
//!
//! Three chained yearly systems (Update phase):
//! 1. `update_person_prestige` — drift prestige toward trait-weighted target
//! 2. `update_faction_prestige` — drift prestige toward territory/economy target
//! 3. `update_settlement_prestige` — drift prestige toward population/infrastructure target
//!
//! One reaction system (Reactions phase):
//! 4. `handle_reputation_events` — 23+ signal types → immediate prestige deltas + tier updates

use bevy_app::{App, Plugin};
use bevy_ecs::entity::Entity;
use bevy_ecs::message::{MessageReader, MessageWriter};
use bevy_ecs::query::With;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Query, Res, ResMut};
use rand::Rng;

use crate::ecs::clock::SimClock;
use crate::ecs::commands::{SimCommand, SimCommandKind};
use crate::ecs::components::{
    Building, Faction, FactionCore, Person, PersonCore, PersonReputation, Settlement,
    SettlementCore, SettlementMilitary, SettlementTrade, SimEntity,
};
use crate::ecs::conditions::yearly;
use crate::ecs::events::SimReactiveEvent;
use crate::ecs::relationships::{LeaderOf, LocatedIn, MemberOf, MemberOfSources};
use crate::ecs::resources::ReputationRng;
use crate::ecs::schedule::{DomainSet, SimPhase, SimTick};
use crate::model::entity_data::Role;
use crate::model::traits::Trait;

// ---------------------------------------------------------------------------
// Prestige tier thresholds
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

// ---------------------------------------------------------------------------
// Signal response deltas — conquest and siege
// ---------------------------------------------------------------------------
const CAPTURE_NEW_FACTION_DELTA: f64 = 0.03;
const CAPTURE_OLD_FACTION_DELTA: f64 = -0.05;

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
const DISASTER_STRUCK_SETTLEMENT_DELTA: f64 = -0.05;
const DISASTER_STRUCK_FACTION_DELTA: f64 = -0.03;
const DISASTER_ENDED_SETTLEMENT_DELTA: f64 = 0.02;
const BANDIT_RAID_FACTION_DELTA: f64 = -0.03;
const BETRAYAL_FACTION_PRESTIGE_DELTA: f64 = -0.10;
const BETRAYAL_VICTIM_SYMPATHY_DELTA: f64 = 0.03;
const CRISIS_FACTION_PRESTIGE_HIT: f64 = -0.05;

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
const SETTLEMENT_TARGET_MAX: f64 = 0.85;

// ---------------------------------------------------------------------------
// Settlement drift parameters
// ---------------------------------------------------------------------------
const SETTLEMENT_DRIFT_RATE: f64 = 0.08;
const SETTLEMENT_NOISE_RANGE: f64 = 0.01;

// ---------------------------------------------------------------------------
// Plugin registration
// ---------------------------------------------------------------------------

pub struct ReputationPlugin;

impl Plugin for ReputationPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            SimTick,
            (
                update_person_prestige,
                update_faction_prestige,
                update_settlement_prestige,
            )
                .chain()
                .run_if(yearly)
                .in_set(DomainSet::Reputation),
        );
        app.add_systems(
            SimTick,
            handle_reputation_events.in_set(SimPhase::Reactions),
        );
    }
}

// ---------------------------------------------------------------------------
// System 1: Update person prestige (yearly)
// ---------------------------------------------------------------------------

fn prestige_tier(prestige: f64) -> u8 {
    if prestige >= TIER_LEGENDARY {
        4
    } else if prestige >= TIER_ILLUSTRIOUS {
        3
    } else if prestige >= TIER_RENOWNED {
        2
    } else if prestige >= TIER_NOTABLE {
        1
    } else {
        0
    }
}

fn apply_prestige_delta(prestige: &mut f64, delta: f64) {
    *prestige = (*prestige + delta).clamp(0.0, 1.0);
}

#[allow(clippy::type_complexity)]
fn update_person_prestige(
    mut rng: ResMut<ReputationRng>,
    clock: Res<SimClock>,
    mut persons: Query<
        (
            Entity,
            &SimEntity,
            &mut PersonReputation,
            &PersonCore,
            Option<&LeaderOf>,
        ),
        With<Person>,
    >,
    factions: Query<&MemberOfSources, With<Faction>>,
    mut commands: MessageWriter<SimCommand>,
) {
    let rng = &mut rng.0;

    for (entity, sim, mut rep, core, leader_of) in persons.iter_mut() {
        if !sim.is_alive() || core.traits.is_empty() {
            continue;
        }

        // Compute target
        let mut target = PERSON_BASE_TARGET;

        // Leadership bonus
        if let Some(leader) = leader_of {
            target += PERSON_LEADERSHIP_BONUS;

            // Count settlements in the faction
            if let Ok(members) = factions.get(leader.0) {
                let settlement_count = members.len();
                if settlement_count >= PERSON_LARGE_TERRITORY_THRESHOLD {
                    target += PERSON_LARGE_TERRITORY_BONUS;
                }
                if settlement_count >= PERSON_MAJOR_TERRITORY_THRESHOLD {
                    target += PERSON_MAJOR_TERRITORY_BONUS;
                }
            }
        }

        // Role bonuses
        match core.role {
            Role::Warrior => target += PERSON_WARRIOR_BONUS,
            Role::Elder => target += PERSON_ELDER_BONUS,
            Role::Scholar => target += PERSON_SCHOLAR_BONUS,
            _ => {}
        }

        // Longevity bonus
        let age = clock.time.years_since(core.born);
        if age > PERSON_LONGEVITY_AGE {
            let years_over = (age - PERSON_LONGEVITY_AGE) as f64;
            target += PERSON_LONGEVITY_BONUS * (years_over / PERSON_LONGEVITY_SCALE_YEARS).min(1.0);
        }

        target = target.min(PERSON_TARGET_MAX);

        // Trait-based drift rate
        let mut rate = PERSON_BASE_DRIFT_RATE;
        for t in &core.traits {
            match t {
                Trait::Ambitious => rate *= TRAIT_AMBITIOUS_MULT,
                Trait::Charismatic => rate *= TRAIT_CHARISMATIC_MULT,
                Trait::Content => rate *= TRAIT_CONTENT_MULT,
                Trait::Reclusive => rate *= TRAIT_RECLUSIVE_MULT,
                _ => {}
            }
        }

        // Drift
        let noise = rng.random_range(-PERSON_NOISE_RANGE..PERSON_NOISE_RANGE);
        let old = rep.prestige;
        rep.prestige = (rep.prestige + (target - rep.prestige) * rate + noise).clamp(0.0, 1.0);

        // Check tier change
        let old_tier = rep.prestige_tier;
        let new_tier = prestige_tier(rep.prestige);
        if new_tier != old_tier {
            rep.prestige_tier = new_tier;
            commands.write(SimCommand::bookkeeping(SimCommandKind::SetField {
                entity,
                field: "prestige_tier".to_string(),
                old_value: serde_json::json!(old_tier),
                new_value: serde_json::json!(new_tier),
            }));
        }

        // Bookkeeping for prestige drift
        if (rep.prestige - old).abs() > f64::EPSILON {
            commands.write(SimCommand::bookkeeping(SimCommandKind::SetField {
                entity,
                field: "prestige".to_string(),
                old_value: serde_json::json!(old),
                new_value: serde_json::json!(rep.prestige),
            }));
        }
    }
}

// ---------------------------------------------------------------------------
// System 2: Update faction prestige (yearly)
// ---------------------------------------------------------------------------

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn update_faction_prestige(
    mut rng: ResMut<ReputationRng>,
    mut factions: Query<(Entity, &SimEntity, &mut FactionCore, &MemberOfSources), With<Faction>>,
    settlements: Query<(&SettlementCore, &SettlementTrade), (With<Settlement>, With<MemberOf>)>,
    persons: Query<(&PersonReputation, &LeaderOf), With<Person>>,
    settlement_entities: Query<(Entity, &MemberOf), With<Settlement>>,
    building_locs: Query<(&SimEntity, &LocatedIn), With<Building>>,
    mut commands: MessageWriter<SimCommand>,
) {
    let rng = &mut rng.0;

    for (entity, sim, mut core, _members) in factions.iter_mut() {
        if !sim.is_alive() {
            continue;
        }

        // Count settlements in this faction
        let faction_settlements: Vec<Entity> = settlement_entities
            .iter()
            .filter(|(_, m)| m.0 == entity)
            .map(|(e, _)| e)
            .collect();
        let settlement_count = faction_settlements.len();

        // Territory contribution
        let territory_bonus =
            (settlement_count as f64 * FACTION_TERRITORY_PER_SETTLEMENT).min(FACTION_TERRITORY_CAP);

        // Average prosperity and trade route count
        let (total_prosperity, total_routes) =
            faction_settlements.iter().fold((0.0, 0usize), |acc, &se| {
                if let Ok((sc, st)) = settlements.get(se) {
                    (acc.0 + sc.prosperity, acc.1 + st.trade_routes.len())
                } else {
                    acc
                }
            });
        let avg_prosperity = if settlement_count > 0 {
            total_prosperity / settlement_count as f64
        } else {
            0.3
        };
        let trade_bonus = (total_routes as f64 * FACTION_TRADE_PER_ROUTE).min(FACTION_TRADE_CAP);

        // Count buildings in faction settlements
        let building_count = building_locs
            .iter()
            .filter(|(bsim, loc)| bsim.is_alive() && faction_settlements.contains(&loc.0))
            .count();
        let building_bonus =
            (building_count as f64 * FACTION_BUILDING_PER_BUILDING).min(FACTION_BUILDING_CAP);

        // Leader prestige
        let leader_prestige = persons
            .iter()
            .find(|(_, lo)| lo.0 == entity)
            .map(|(rep, _)| rep.prestige)
            .unwrap_or(0.0);

        let mut target = FACTION_BASE_TARGET
            + territory_bonus
            + avg_prosperity * FACTION_PROSPERITY_WEIGHT
            + trade_bonus
            + building_bonus
            + core.stability * FACTION_STABILITY_WEIGHT
            + core.legitimacy * FACTION_LEGITIMACY_WEIGHT
            + leader_prestige * FACTION_LEADER_PRESTIGE_WEIGHT;

        target = target.min(FACTION_TARGET_MAX);

        let noise = rng.random_range(-FACTION_NOISE_RANGE..FACTION_NOISE_RANGE);
        let old = core.prestige;
        core.prestige =
            (core.prestige + (target - core.prestige) * FACTION_DRIFT_RATE + noise).clamp(0.0, 1.0);

        let old_tier = core.prestige_tier;
        let new_tier = prestige_tier(core.prestige);
        if new_tier != old_tier {
            core.prestige_tier = new_tier;
            commands.write(SimCommand::bookkeeping(SimCommandKind::SetField {
                entity,
                field: "prestige_tier".to_string(),
                old_value: serde_json::json!(old_tier),
                new_value: serde_json::json!(new_tier),
            }));
        }

        if (core.prestige - old).abs() > f64::EPSILON {
            commands.write(SimCommand::bookkeeping(SimCommandKind::SetField {
                entity,
                field: "prestige".to_string(),
                old_value: serde_json::json!(old),
                new_value: serde_json::json!(core.prestige),
            }));
        }
    }
}

// ---------------------------------------------------------------------------
// System 3: Update settlement prestige (yearly)
// ---------------------------------------------------------------------------

#[allow(clippy::type_complexity)]
fn update_settlement_prestige(
    mut rng: ResMut<ReputationRng>,
    mut settlements: Query<
        (
            Entity,
            &SimEntity,
            &mut SettlementCore,
            &SettlementTrade,
            &SettlementMilitary,
        ),
        With<Settlement>,
    >,
    buildings: Query<(&SimEntity, &LocatedIn), With<Building>>,
    mut commands: MessageWriter<SimCommand>,
) {
    let rng = &mut rng.0;

    for (entity, sim, mut core, trade, military) in settlements.iter_mut() {
        if !sim.is_alive() {
            continue;
        }

        let mut target = SETTLEMENT_BASE_TARGET;

        // Population tiers
        if core.population >= SETTLEMENT_POP_TIER1 {
            target += SETTLEMENT_POP_TIER1_BONUS;
        }
        if core.population >= SETTLEMENT_POP_TIER2 {
            target += SETTLEMENT_POP_TIER2_BONUS;
        }
        if core.population >= SETTLEMENT_POP_TIER3 {
            target += SETTLEMENT_POP_TIER3_BONUS;
        }
        if core.population >= SETTLEMENT_POP_TIER4 {
            target += SETTLEMENT_POP_TIER4_BONUS;
        }

        // Prosperity
        target += core.prosperity * SETTLEMENT_PROSPERITY_WEIGHT;

        // Building count
        let building_count = buildings
            .iter()
            .filter(|(bsim, loc)| bsim.is_alive() && loc.0 == entity)
            .count();
        target +=
            (building_count as f64 * SETTLEMENT_BUILDING_PER_BUILDING).min(SETTLEMENT_BUILDING_CAP);

        // Fortifications
        target += military.fortification_level as f64 * SETTLEMENT_FORTIFICATION_PER_LEVEL;

        // Trade routes
        let route_count = trade.trade_routes.len();
        target += (route_count as f64 * SETTLEMENT_TRADE_PER_ROUTE).min(SETTLEMENT_TRADE_CAP);

        target = target.min(SETTLEMENT_TARGET_MAX);

        let noise = rng.random_range(-SETTLEMENT_NOISE_RANGE..SETTLEMENT_NOISE_RANGE);
        let old = core.prestige;
        core.prestige = (core.prestige + (target - core.prestige) * SETTLEMENT_DRIFT_RATE + noise)
            .clamp(0.0, 1.0);

        let old_tier = core.prestige_tier;
        let new_tier = prestige_tier(core.prestige);
        if new_tier != old_tier {
            core.prestige_tier = new_tier;
            commands.write(SimCommand::bookkeeping(SimCommandKind::SetField {
                entity,
                field: "prestige_tier".to_string(),
                old_value: serde_json::json!(old_tier),
                new_value: serde_json::json!(new_tier),
            }));
        }

        if (core.prestige - old).abs() > f64::EPSILON {
            commands.write(SimCommand::bookkeeping(SimCommandKind::SetField {
                entity,
                field: "prestige".to_string(),
                old_value: serde_json::json!(old),
                new_value: serde_json::json!(core.prestige),
            }));
        }
    }
}

// ---------------------------------------------------------------------------
// Reaction system: Handle reputation events
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn handle_reputation_events(
    mut events: MessageReader<SimReactiveEvent>,
    mut persons: Query<&mut PersonReputation, With<Person>>,
    mut factions: Query<&mut FactionCore, With<Faction>>,
    mut settlements: Query<&mut SettlementCore, With<Settlement>>,
    settlement_membership: Query<(&MemberOf,), With<Settlement>>,
    person_leaders: Query<(Entity, &LeaderOf), With<Person>>,
    building_locs: Query<&LocatedIn, With<Building>>,
) {
    for event in events.read() {
        match event {
            SimReactiveEvent::WarEnded { winner, loser, .. } => {
                // Decisive: winner gets positive, loser negative
                if let Ok(mut core_w) = factions.get_mut(*winner) {
                    apply_prestige_delta(&mut core_w.prestige, WAR_DECISIVE_WINNER_FACTION_DELTA);
                    core_w.prestige_tier = prestige_tier(core_w.prestige);
                }
                if let Ok(mut core_l) = factions.get_mut(*loser) {
                    apply_prestige_delta(&mut core_l.prestige, WAR_DECISIVE_LOSER_FACTION_DELTA);
                    core_l.prestige_tier = prestige_tier(core_l.prestige);
                }
                // Leader prestige
                for (pe, lo) in person_leaders.iter() {
                    if lo.0 == *winner
                        && let Ok(mut rep) = persons.get_mut(pe)
                    {
                        apply_prestige_delta(&mut rep.prestige, WAR_DECISIVE_WINNER_LEADER_DELTA);
                        rep.prestige_tier = prestige_tier(rep.prestige);
                    }
                    if lo.0 == *loser
                        && let Ok(mut rep) = persons.get_mut(pe)
                    {
                        apply_prestige_delta(&mut rep.prestige, WAR_DECISIVE_LOSER_LEADER_DELTA);
                        rep.prestige_tier = prestige_tier(rep.prestige);
                    }
                }
            }

            SimReactiveEvent::SettlementCaptured {
                old_faction,
                new_faction,
                ..
            } => {
                if let Ok(mut core) = factions.get_mut(*new_faction) {
                    apply_prestige_delta(&mut core.prestige, CAPTURE_NEW_FACTION_DELTA);
                    core.prestige_tier = prestige_tier(core.prestige);
                }
                if let Some(old) = old_faction
                    && let Ok(mut core) = factions.get_mut(*old)
                {
                    apply_prestige_delta(&mut core.prestige, CAPTURE_OLD_FACTION_DELTA);
                    core.prestige_tier = prestige_tier(core.prestige);
                }
            }

            SimReactiveEvent::BuildingConstructed { settlement, .. } => {
                if let Ok(mut score) = settlements.get_mut(*settlement) {
                    apply_prestige_delta(
                        &mut score.prestige,
                        BUILDING_CONSTRUCTED_SETTLEMENT_DELTA,
                    );
                    score.prestige_tier = prestige_tier(score.prestige);
                }
                // Faction bonus
                if let Ok((member,)) = settlement_membership.get(*settlement)
                    && let Ok(mut fcore) = factions.get_mut(member.0)
                {
                    apply_prestige_delta(&mut fcore.prestige, BUILDING_CONSTRUCTED_FACTION_DELTA);
                    fcore.prestige_tier = prestige_tier(fcore.prestige);
                }
            }

            SimReactiveEvent::BuildingUpgraded { building, .. } => {
                // Find settlement containing this building
                if let Ok(loc) = building_locs.get(*building) {
                    if let Ok(mut score) = settlements.get_mut(loc.0) {
                        apply_prestige_delta(
                            &mut score.prestige,
                            BUILDING_UPGRADED_SETTLEMENT_DELTA,
                        );
                        score.prestige_tier = prestige_tier(score.prestige);
                    }
                    if let Ok((member,)) = settlement_membership.get(loc.0)
                        && let Ok(mut fcore) = factions.get_mut(member.0)
                    {
                        apply_prestige_delta(&mut fcore.prestige, BUILDING_UPGRADED_FACTION_DELTA);
                        fcore.prestige_tier = prestige_tier(fcore.prestige);
                    }
                }
            }

            SimReactiveEvent::TradeRouteEstablished {
                settlement_a,
                settlement_b,
                ..
            } => {
                for &sett in &[*settlement_a, *settlement_b] {
                    if let Ok(mut score) = settlements.get_mut(sett) {
                        apply_prestige_delta(&mut score.prestige, TRADE_ROUTE_SETTLEMENT_DELTA);
                        score.prestige_tier = prestige_tier(score.prestige);
                    }
                    if let Ok((member,)) = settlement_membership.get(sett)
                        && let Ok(mut fcore) = factions.get_mut(member.0)
                    {
                        apply_prestige_delta(&mut fcore.prestige, TRADE_ROUTE_FACTION_DELTA);
                        fcore.prestige_tier = prestige_tier(fcore.prestige);
                    }
                }
            }

            SimReactiveEvent::PlagueEnded { settlement, .. } => {
                if let Ok(mut score) = settlements.get_mut(*settlement) {
                    apply_prestige_delta(&mut score.prestige, PLAGUE_ENDED_SETTLEMENT_DELTA);
                    score.prestige_tier = prestige_tier(score.prestige);
                }
            }

            SimReactiveEvent::FactionSplit { parent_faction, .. } => {
                if let Ok(mut core) = factions.get_mut(*parent_faction) {
                    apply_prestige_delta(&mut core.prestige, FACTION_SPLIT_DELTA);
                    core.prestige_tier = prestige_tier(core.prestige);
                }
            }

            SimReactiveEvent::CulturalRebellion { settlement, .. } => {
                if let Ok((member,)) = settlement_membership.get(*settlement)
                    && let Ok(mut fcore) = factions.get_mut(member.0)
                {
                    apply_prestige_delta(&mut fcore.prestige, CULTURAL_REBELLION_DELTA);
                    fcore.prestige_tier = prestige_tier(fcore.prestige);
                }
            }

            SimReactiveEvent::TreasuryDepleted { faction, .. } => {
                if let Ok(mut core) = factions.get_mut(*faction) {
                    apply_prestige_delta(&mut core.prestige, TREASURY_DEPLETED_DELTA);
                    core.prestige_tier = prestige_tier(core.prestige);
                }
            }

            SimReactiveEvent::DisasterStruck { settlement, .. } => {
                if let Ok(mut score) = settlements.get_mut(*settlement) {
                    apply_prestige_delta(&mut score.prestige, DISASTER_STRUCK_SETTLEMENT_DELTA);
                    score.prestige_tier = prestige_tier(score.prestige);
                }
                // Faction prestige hit
                if let Ok((member,)) = settlement_membership.get(*settlement)
                    && let Ok(mut fcore) = factions.get_mut(member.0)
                {
                    apply_prestige_delta(&mut fcore.prestige, DISASTER_STRUCK_FACTION_DELTA);
                    fcore.prestige_tier = prestige_tier(fcore.prestige);
                }
            }

            SimReactiveEvent::DisasterEnded { settlement, .. } => {
                // Surviving a disaster shows resilience
                if let Ok(mut score) = settlements.get_mut(*settlement) {
                    apply_prestige_delta(&mut score.prestige, DISASTER_ENDED_SETTLEMENT_DELTA);
                    score.prestige_tier = prestige_tier(score.prestige);
                }
            }

            SimReactiveEvent::ReligionSchism { .. } => {
                // Parent faction prestige hit handled by culture system
            }

            SimReactiveEvent::ProphecyDeclared { .. } => {
                // Prophet and settlement prestige handled inline
            }

            SimReactiveEvent::ReligionFounded { .. } => {
                // Founder prestige already handled by applicator
            }

            SimReactiveEvent::AllianceBetrayed {
                betrayer, betrayed, ..
            } => {
                if let Ok(mut core) = factions.get_mut(*betrayer) {
                    apply_prestige_delta(&mut core.prestige, BETRAYAL_FACTION_PRESTIGE_DELTA);
                    core.prestige_tier = prestige_tier(core.prestige);
                }
                if let Ok(mut core) = factions.get_mut(*betrayed) {
                    apply_prestige_delta(&mut core.prestige, BETRAYAL_VICTIM_SYMPATHY_DELTA);
                    core.prestige_tier = prestige_tier(core.prestige);
                }
            }

            SimReactiveEvent::SuccessionCrisis { faction, .. } => {
                if let Ok(mut core) = factions.get_mut(*faction) {
                    apply_prestige_delta(&mut core.prestige, CRISIS_FACTION_PRESTIGE_HIT);
                    core.prestige_tier = prestige_tier(core.prestige);
                }
            }

            SimReactiveEvent::BanditGangFormed { .. } => {
                // Region → factions owning region; simplified
            }

            SimReactiveEvent::BanditRaid { settlement, .. } => {
                if let Ok((member,)) = settlement_membership.get(*settlement)
                    && let Ok(mut fcore) = factions.get_mut(member.0)
                {
                    apply_prestige_delta(&mut fcore.prestige, BANDIT_RAID_FACTION_DELTA);
                    fcore.prestige_tier = prestige_tier(fcore.prestige);
                }
            }

            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::app::build_sim_app_seeded;
    use crate::ecs::components::*;
    use crate::ecs::relationships::MemberOf;
    use crate::ecs::resources::SimEntityMap;
    use crate::ecs::test_helpers::tick_years;
    use crate::ecs::time::SimTime;

    fn setup_app() -> App {
        let mut app = build_sim_app_seeded(100, 42);
        app.add_plugins(ReputationPlugin);
        app
    }

    fn spawn_faction(app: &mut App, sim_id: u64) -> Entity {
        let entity = app
            .world_mut()
            .spawn((
                SimEntity {
                    id: sim_id,
                    name: "Kingdom".to_string(),
                    origin: Some(SimTime::from_year(100)),
                    end: None,
                },
                Faction,
                FactionCore {
                    stability: 0.5,
                    legitimacy: 0.5,
                    treasury: 50.0,
                    ..FactionCore::default()
                },
                FactionDiplomacy::default(),
                FactionMilitary::default(),
            ))
            .id();
        app.world_mut()
            .resource_mut::<SimEntityMap>()
            .insert(sim_id, entity);
        entity
    }

    fn spawn_settlement(app: &mut App, sim_id: u64, faction: Entity, population: u32) -> Entity {
        let entity = app
            .world_mut()
            .spawn((
                SimEntity {
                    id: sim_id,
                    name: "Town".to_string(),
                    origin: Some(SimTime::from_year(100)),
                    end: None,
                },
                Settlement,
                SettlementCore {
                    population,
                    prosperity: 0.5,
                    ..SettlementCore::default()
                },
                SettlementCulture::default(),
                SettlementDisease::default(),
                SettlementTrade::default(),
                SettlementMilitary::default(),
                SettlementCrime::default(),
                SettlementEducation::default(),
                EcsSeasonalModifiers::default(),
                EcsBuildingBonuses::default(),
            ))
            .id();
        app.world_mut().entity_mut(entity).insert(MemberOf(faction));
        app.world_mut()
            .resource_mut::<SimEntityMap>()
            .insert(sim_id, entity);
        entity
    }

    fn spawn_person_with_traits(
        app: &mut App,
        sim_id: u64,
        faction: Entity,
        traits: Vec<Trait>,
    ) -> Entity {
        let entity = app
            .world_mut()
            .spawn((
                SimEntity {
                    id: sim_id,
                    name: "Person".to_string(),
                    origin: Some(SimTime::from_year(100)),
                    end: None,
                },
                Person,
                PersonCore {
                    born: SimTime::from_year(70),
                    role: Role::Warrior,
                    traits,
                    ..PersonCore::default()
                },
                PersonReputation::default(),
                PersonSocial::default(),
                PersonEducation::default(),
            ))
            .id();
        app.world_mut().entity_mut(entity).insert(MemberOf(faction));
        app.world_mut()
            .resource_mut::<SimEntityMap>()
            .insert(sim_id, entity);
        entity
    }

    #[test]
    fn person_prestige_converges_upward_for_leader() {
        let mut app = setup_app();
        let faction = spawn_faction(&mut app, 1);
        let _sett = spawn_settlement(&mut app, 2, faction, 500);
        let leader = spawn_person_with_traits(&mut app, 3, faction, vec![Trait::Ambitious]);
        app.world_mut().entity_mut(leader).insert(LeaderOf(faction));

        tick_years(&mut app, 5);

        let rep = app.world().get::<PersonReputation>(leader).unwrap();
        assert!(
            rep.prestige > 0.0,
            "leader prestige should increase, got {}",
            rep.prestige
        );
    }

    #[test]
    fn faction_prestige_reflects_territory() {
        let mut app = setup_app();
        let faction = spawn_faction(&mut app, 1);
        spawn_settlement(&mut app, 2, faction, 500);
        spawn_settlement(&mut app, 3, faction, 500);
        spawn_settlement(&mut app, 4, faction, 500);

        tick_years(&mut app, 5);

        let core = app.world().get::<FactionCore>(faction).unwrap();
        assert!(
            core.prestige > 0.0,
            "faction prestige should increase with territory, got {}",
            core.prestige
        );
    }

    #[test]
    fn settlement_prestige_gets_population_tier_bonus() {
        let mut app = setup_app();
        let faction = spawn_faction(&mut app, 1);
        let small = spawn_settlement(&mut app, 2, faction, 50);
        let large = spawn_settlement(&mut app, 3, faction, 1500);

        tick_years(&mut app, 10);

        let small_prestige = app.world().get::<SettlementCore>(small).unwrap().prestige;
        let large_prestige = app.world().get::<SettlementCore>(large).unwrap().prestige;
        assert!(
            large_prestige > small_prestige,
            "large settlement should have higher prestige: large={large_prestige}, small={small_prestige}"
        );
    }

    #[test]
    fn war_victory_boosts_winner_prestige() {
        let mut app = setup_app();
        let winner = spawn_faction(&mut app, 1);
        let loser = spawn_faction(&mut app, 2);

        // Inject WarEnded reactive event
        let war_event = SimReactiveEvent::WarEnded {
            event_id: 1,
            winner,
            loser,
            decisive: true,
        };
        app.world_mut()
            .resource_mut::<bevy_ecs::message::Messages<SimReactiveEvent>>()
            .write(war_event);

        tick_years(&mut app, 1);

        let winner_prestige = app.world().get::<FactionCore>(winner).unwrap().prestige;
        let loser_prestige = app.world().get::<FactionCore>(loser).unwrap().prestige;
        assert!(
            winner_prestige > loser_prestige,
            "winner={winner_prestige} should beat loser={loser_prestige}"
        );
    }

    #[test]
    fn building_construction_boosts_settlement_prestige() {
        let mut app = setup_app();
        let faction = spawn_faction(&mut app, 1);
        let sett = spawn_settlement(&mut app, 2, faction, 300);

        let initial = app.world().get::<SettlementCore>(sett).unwrap().prestige;

        // Inject BuildingConstructed event
        let build_event = SimReactiveEvent::BuildingConstructed {
            event_id: 1,
            building: sett, // dummy, just needs to be an entity
            settlement: sett,
        };
        app.world_mut()
            .resource_mut::<bevy_ecs::message::Messages<SimReactiveEvent>>()
            .write(build_event);

        tick_years(&mut app, 1);

        let after = app.world().get::<SettlementCore>(sett).unwrap().prestige;
        assert!(
            after > initial,
            "building should boost prestige: before={initial}, after={after}"
        );
    }
}
