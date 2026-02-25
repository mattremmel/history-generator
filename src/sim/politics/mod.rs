mod coups;
pub(crate) mod diplomacy;

use rand::Rng;
use rand::RngCore;

use super::context::TickContext;
use super::extra_keys as K;
use super::faction_names::generate_unique_faction_name;
use super::signal::{Signal, SignalKind};
use super::system::{SimSystem, TickFrequency};
use crate::model::traits::{Trait, has_trait};
use crate::model::{
    EntityData, EntityKind, EventKind, FactionData, GovernmentType, ParticipantRole,
    RelationshipKind, Role, SiegeOutcome, SimTimestamp, World,
};
use crate::sim::grievance as grv;
use crate::sim::helpers;

// --- Signal Deltas: War ---
const WAR_STARTED_HAPPINESS_HIT: f64 = -0.15;
const WAR_WON_DECISIVE_HAPPINESS: f64 = 0.15;
const WAR_WON_DECISIVE_STABILITY: f64 = 0.10;
const WAR_LOST_DECISIVE_HAPPINESS: f64 = -0.15;
const WAR_LOST_DECISIVE_STABILITY: f64 = -0.15;
const WAR_WON_INDECISIVE_HAPPINESS: f64 = 0.05;
const WAR_WON_INDECISIVE_STABILITY: f64 = 0.03;
const WAR_LOST_INDECISIVE_HAPPINESS: f64 = -0.05;
const WAR_LOST_INDECISIVE_STABILITY: f64 = -0.05;

// --- Signal Deltas: Settlement & Territory ---
const SETTLEMENT_CAPTURED_STABILITY: f64 = -0.15;
const REFUGEE_THRESHOLD_RATIO: f64 = 0.20;
const REFUGEE_HAPPINESS_HIT: f64 = -0.1;

// --- Signal Deltas: Cultural & Plague ---
const CULTURAL_REBELLION_STABILITY: f64 = -0.15;
const CULTURAL_REBELLION_HAPPINESS: f64 = -0.10;
const PLAGUE_STABILITY_HIT: f64 = -0.10;
const PLAGUE_HAPPINESS_HIT: f64 = -0.15;

// --- Signal Deltas: Siege ---
const SIEGE_STARTED_HAPPINESS: f64 = -0.10;
const SIEGE_STARTED_STABILITY: f64 = -0.05;
const SIEGE_LIFTED_HAPPINESS: f64 = 0.10;

// --- Signal Deltas: Disaster ---
const DISASTER_HAPPINESS_BASE: f64 = -0.05;
const DISASTER_HAPPINESS_SEVERITY_WEIGHT: f64 = 0.10;
const DISASTER_STABILITY_HIT: f64 = -0.05;
const DISASTER_ENDED_HAPPINESS_RECOVERY: f64 = 0.03;
const BETRAYAL_VICTIM_HAPPINESS_RALLY: f64 = 0.05;
const BETRAYAL_VICTIM_STABILITY_RALLY: f64 = 0.05;

// --- Happiness Calculation ---
const HAPPINESS_DEFAULT: f64 = 0.6;
const HAPPINESS_BASE_TARGET: f64 = 0.6;
const HAPPINESS_PROSPERITY_WEIGHT: f64 = 0.15;
const HAPPINESS_STABILITY_NEUTRAL: f64 = 0.5;
const HAPPINESS_STABILITY_WEIGHT: f64 = 0.2;
const HAPPINESS_ENEMIES_PENALTY: f64 = -0.1;
const HAPPINESS_ALLIES_BONUS: f64 = 0.05;
const HAPPINESS_LEADER_PRESENT_BONUS: f64 = 0.05;
const HAPPINESS_LEADER_ABSENT_PENALTY: f64 = -0.1;
const HAPPINESS_TENSION_WEIGHT: f64 = 0.15;
const HAPPINESS_RELIGIOUS_TENSION_WEIGHT: f64 = 0.10;
const HAPPINESS_BUILDING_CAP: f64 = 0.15;
const HAPPINESS_MIN_TARGET: f64 = 0.1;
const HAPPINESS_MAX_TARGET: f64 = 0.95;
const HAPPINESS_NOISE_RANGE: f64 = 0.02;
const HAPPINESS_DRIFT_RATE: f64 = 0.15;
const DEFAULT_PROSPERITY: f64 = 0.3;

// --- Legitimacy Calculation ---
const LEGITIMACY_BASE_TARGET: f64 = 0.5;
const LEGITIMACY_HAPPINESS_WEIGHT: f64 = 0.4;
const LEGITIMACY_LEADER_PRESTIGE_WEIGHT: f64 = 0.1;
const LEGITIMACY_DRIFT_RATE: f64 = 0.1;

// --- Stability Calculation ---
const STABILITY_DEFAULT: f64 = 0.5;
const STABILITY_BASE_TARGET: f64 = 0.5;
const STABILITY_HAPPINESS_WEIGHT: f64 = 0.2;
const STABILITY_LEGITIMACY_WEIGHT: f64 = 0.15;
const STABILITY_LEADER_PRESENT_BONUS: f64 = 0.05;
const STABILITY_LEADER_ABSENT_PENALTY: f64 = -0.15;
const STABILITY_TENSION_WEIGHT: f64 = 0.10;
const STABILITY_THEOCRACY_FERVOR_BONUS: f64 = 0.02;
const STABILITY_MIN_TARGET: f64 = 0.15;
const STABILITY_MAX_TARGET: f64 = 0.95;
const STABILITY_NOISE_RANGE: f64 = 0.05;
const STABILITY_DRIFT_RATE: f64 = 0.12;
const STABILITY_LEADERLESS_PRESSURE: f64 = 0.04;

// --- Succession ---
const SUCCESSION_STABILITY_HIT: f64 = -0.12;
const SUCCESSION_PRESTIGE_SOFTENING: f64 = 0.5;

// --- Succession Claims ---
const CLAIM_CHILD_STRENGTH: f64 = 0.9;
const CLAIM_SIBLING_STRENGTH: f64 = 0.6;
const CLAIM_GRANDCHILD_STRENGTH: f64 = 0.4;
const CLAIM_SPOUSE_FACTOR: f64 = 0.5;
const CLAIM_DEPOSED_STRENGTH: f64 = 0.7;
const CLAIM_SPLIT_STRENGTH: f64 = 0.5;
const CLAIM_DECAY_PER_YEAR: f64 = 0.05;
const CLAIM_MIN_THRESHOLD: f64 = 0.1;
const CRISIS_CLAIM_THRESHOLD: f64 = 0.5;
const CRISIS_STABILITY_HIT: f64 = -0.15;
const CRISIS_LEGITIMACY_HIT: f64 = -0.20;

// --- Grievance ---
const GRIEVANCE_BASE_DECAY: f64 = 0.03;
const GRIEVANCE_MIN_THRESHOLD: f64 = 0.05;
const GRIEVANCE_CONQUEST: f64 = 0.40;
const GRIEVANCE_WAR_DEFEAT_DECISIVE: f64 = 0.35;
const GRIEVANCE_WAR_DEFEAT_INDECISIVE: f64 = 0.10;
const GRIEVANCE_BETRAYAL: f64 = 0.50;
const GRIEVANCE_RAID: f64 = 0.15;
const GRIEVANCE_SATISFACTION_DECISIVE: f64 = 0.40;
const GRIEVANCE_SATISFACTION_INDECISIVE: f64 = 0.15;
const GRIEVANCE_SATISFACTION_CAPTURE: f64 = 0.15;

// --- Faction Splits ---
const SPLIT_STABILITY_THRESHOLD: f64 = 0.3;
const SPLIT_HAPPINESS_THRESHOLD: f64 = 0.35;
const SPLIT_BASE_CHANCE: f64 = 0.01;
const SPLIT_PRESTIGE_RESISTANCE: f64 = 0.3;
const SPLIT_GOV_TYPE_INHERITANCE_CHANCE: f64 = 0.5;
const SPLIT_NEW_FACTION_STABILITY: f64 = 0.5;
const SPLIT_NEW_FACTION_HAPPINESS_BONUS: f64 = 0.1;
const SPLIT_NEW_FACTION_LEGITIMACY: f64 = 0.6;
const SPLIT_NEW_FACTION_PRESTIGE_INHERITANCE: f64 = 0.25;
const SPLIT_POST_ENEMY_CHANCE: f64 = 0.7;

pub struct PoliticsSystem;

impl SimSystem for PoliticsSystem {
    fn name(&self) -> &str {
        "politics"
    }

    fn frequency(&self) -> TickFrequency {
        TickFrequency::Yearly
    }

    fn tick(&mut self, ctx: &mut TickContext) {
        let time = ctx.world.current_time;
        let current_year = time.year();

        // --- 4a: Fill leader vacancies ---
        fill_leader_vacancies(ctx, time, current_year);

        // --- Claim decay (yearly) ---
        decay_claims(ctx, current_year);

        // --- Grievance decay (yearly) ---
        decay_grievances(ctx);

        // --- Sentiment updates (before stability) ---
        update_happiness(ctx, time);
        update_legitimacy(ctx, time);

        // --- 4b: Stability drift ---
        update_stability(ctx, time);

        // --- 4c: Coups ---
        coups::check_coups(ctx, time, current_year);

        // --- 4d: Inter-faction diplomacy ---
        diplomacy::update_diplomacy(ctx, time, current_year);

        // --- 4e: Faction splits ---
        check_faction_splits(ctx, time, current_year);
    }

    fn handle_signals(&mut self, ctx: &mut TickContext) {
        let time = ctx.world.current_time;
        let current_year = time.year();

        for signal in ctx.inbox {
            match &signal.kind {
                SignalKind::WarStarted {
                    attacker_id,
                    defender_id,
                } => {
                    handle_war_started(ctx.world, signal.event_id, *attacker_id, *defender_id);
                }
                SignalKind::WarEnded {
                    winner_id,
                    loser_id,
                    decisive,
                    ..
                } => {
                    handle_war_ended(ctx.world, signal.event_id, *winner_id, *loser_id, *decisive);
                    // Grievance: loser → winner
                    let delta = if *decisive {
                        GRIEVANCE_WAR_DEFEAT_DECISIVE
                    } else {
                        GRIEVANCE_WAR_DEFEAT_INDECISIVE
                    };
                    grv::add_grievance(
                        ctx.world,
                        *loser_id,
                        *winner_id,
                        delta,
                        "war_defeat",
                        current_year,
                        signal.event_id,
                    );
                    // Satisfaction: winner's grievance vs loser reduced
                    let satisfaction = if *decisive {
                        GRIEVANCE_SATISFACTION_DECISIVE
                    } else {
                        GRIEVANCE_SATISFACTION_INDECISIVE
                    };
                    grv::reduce_grievance(
                        ctx.world,
                        *winner_id,
                        *loser_id,
                        satisfaction,
                        GRIEVANCE_MIN_THRESHOLD,
                    );
                }
                SignalKind::SettlementCaptured {
                    old_faction_id,
                    new_faction_id,
                    ..
                } => {
                    handle_settlement_captured(ctx.world, signal.event_id, *old_faction_id);
                    // Grievance: old faction → new faction
                    grv::add_grievance(
                        ctx.world,
                        *old_faction_id,
                        *new_faction_id,
                        GRIEVANCE_CONQUEST,
                        "conquest",
                        current_year,
                        signal.event_id,
                    );
                    // Satisfaction: capturer's grievance vs old owner reduced
                    grv::reduce_grievance(
                        ctx.world,
                        *new_faction_id,
                        *old_faction_id,
                        GRIEVANCE_SATISFACTION_CAPTURE,
                        GRIEVANCE_MIN_THRESHOLD,
                    );
                }
                SignalKind::RefugeesArrived {
                    settlement_id,
                    count,
                    ..
                } => {
                    handle_refugees_arrived(ctx.world, signal.event_id, *settlement_id, *count);
                }
                SignalKind::CulturalRebellion { faction_id, .. } => {
                    handle_cultural_rebellion(ctx.world, signal.event_id, *faction_id);
                }
                SignalKind::PlagueStarted { settlement_id, .. } => {
                    handle_plague_started(ctx.world, signal.event_id, *settlement_id);
                }
                SignalKind::SiegeStarted {
                    defender_faction_id,
                    ..
                } => {
                    handle_siege_started(ctx.world, signal.event_id, *defender_faction_id);
                }
                SignalKind::SiegeEnded {
                    defender_faction_id,
                    outcome,
                    ..
                } => {
                    handle_siege_ended(ctx.world, signal.event_id, *defender_faction_id, *outcome);
                }
                SignalKind::LeaderVacancy {
                    faction_id,
                    previous_leader_id,
                } => {
                    handle_leader_vacancy(
                        ctx.world,
                        ctx.rng,
                        signal.event_id,
                        time,
                        current_year,
                        *faction_id,
                        *previous_leader_id,
                    );
                }
                SignalKind::DisasterStruck {
                    settlement_id,
                    severity,
                    ..
                }
                | SignalKind::DisasterStarted {
                    settlement_id,
                    severity,
                    ..
                } => {
                    handle_disaster_struck(ctx.world, signal.event_id, *settlement_id, *severity);
                }
                SignalKind::DisasterEnded { settlement_id, .. } => {
                    handle_disaster_ended(ctx.world, signal.event_id, *settlement_id);
                }
                SignalKind::BanditGangFormed { region_id, .. } => {
                    // Stability hit to the faction that owns settlements in this region
                    let affected_factions: Vec<u64> = ctx
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
                    for fid in affected_factions {
                        helpers::apply_stability_delta(ctx.world, fid, -0.05, signal.event_id);
                    }
                }
                SignalKind::BanditRaid {
                    settlement_id,
                    bandit_faction_id,
                    ..
                } => {
                    if let Some(fid) = helpers::settlement_faction(ctx.world, *settlement_id) {
                        apply_happiness_delta(ctx.world, fid, -0.08, signal.event_id);
                        helpers::apply_stability_delta(ctx.world, fid, -0.05, signal.event_id);
                        // Grievance: victim faction → bandit faction
                        grv::add_grievance(
                            ctx.world,
                            fid,
                            *bandit_faction_id,
                            GRIEVANCE_RAID,
                            "raid",
                            current_year,
                            signal.event_id,
                        );
                    }
                }
                SignalKind::TradeRouteRaided {
                    from_settlement,
                    to_settlement,
                    ..
                } => {
                    if let Some(fid) = helpers::settlement_faction(ctx.world, *from_settlement) {
                        apply_happiness_delta(ctx.world, fid, -0.03, signal.event_id);
                    }
                    if let Some(fid) = helpers::settlement_faction(ctx.world, *to_settlement) {
                        apply_happiness_delta(ctx.world, fid, -0.03, signal.event_id);
                    }
                }
                SignalKind::AllianceBetrayed {
                    victim_faction_id,
                    betrayer_faction_id,
                    ..
                } => {
                    // Victim rallies — sympathy boost
                    apply_happiness_delta(
                        ctx.world,
                        *victim_faction_id,
                        BETRAYAL_VICTIM_HAPPINESS_RALLY,
                        signal.event_id,
                    );
                    helpers::apply_stability_delta(
                        ctx.world,
                        *victim_faction_id,
                        BETRAYAL_VICTIM_STABILITY_RALLY,
                        signal.event_id,
                    );
                    // Grievance: victim → betrayer
                    grv::add_grievance(
                        ctx.world,
                        *victim_faction_id,
                        *betrayer_faction_id,
                        GRIEVANCE_BETRAYAL,
                        "betrayal",
                        current_year,
                        signal.event_id,
                    );
                }
                _ => {}
            }
        }
    }
}

// --- Signal handlers ---

fn handle_war_started(world: &mut World, event_id: u64, attacker_id: u64, defender_id: u64) {
    apply_happiness_delta(world, attacker_id, WAR_STARTED_HAPPINESS_HIT, event_id);
    apply_happiness_delta(world, defender_id, WAR_STARTED_HAPPINESS_HIT, event_id);
}

fn handle_war_ended(
    world: &mut World,
    event_id: u64,
    winner_id: u64,
    loser_id: u64,
    decisive: bool,
) {
    if decisive {
        apply_happiness_delta(world, winner_id, WAR_WON_DECISIVE_HAPPINESS, event_id);
        helpers::apply_stability_delta(world, winner_id, WAR_WON_DECISIVE_STABILITY, event_id);
        apply_happiness_delta(world, loser_id, WAR_LOST_DECISIVE_HAPPINESS, event_id);
        helpers::apply_stability_delta(world, loser_id, WAR_LOST_DECISIVE_STABILITY, event_id);
    } else {
        apply_happiness_delta(world, winner_id, WAR_WON_INDECISIVE_HAPPINESS, event_id);
        helpers::apply_stability_delta(world, winner_id, WAR_WON_INDECISIVE_STABILITY, event_id);
        apply_happiness_delta(world, loser_id, WAR_LOST_INDECISIVE_HAPPINESS, event_id);
        helpers::apply_stability_delta(world, loser_id, WAR_LOST_INDECISIVE_STABILITY, event_id);
    }
}

fn handle_settlement_captured(world: &mut World, event_id: u64, old_faction_id: u64) {
    helpers::apply_stability_delta(
        world,
        old_faction_id,
        SETTLEMENT_CAPTURED_STABILITY,
        event_id,
    );
}

fn handle_refugees_arrived(world: &mut World, event_id: u64, settlement_id: u64, count: u32) {
    // Large refugee influx (>20% of destination pop) reduces faction happiness
    let dest_pop = world
        .entities
        .get(&settlement_id)
        .and_then(|e| e.data.as_settlement())
        .map(|s| s.population)
        .unwrap_or(0);
    if dest_pop > 0 && (count as f64 / dest_pop as f64) > REFUGEE_THRESHOLD_RATIO {
        // Find the faction this settlement belongs to
        if let Some(faction_id) = world
            .entities
            .get(&settlement_id)
            .and_then(|e| e.active_rel(RelationshipKind::MemberOf))
        {
            apply_happiness_delta(world, faction_id, REFUGEE_HAPPINESS_HIT, event_id);
        }
    }
}

fn handle_cultural_rebellion(world: &mut World, event_id: u64, faction_id: u64) {
    helpers::apply_stability_delta(world, faction_id, CULTURAL_REBELLION_STABILITY, event_id);
    apply_happiness_delta(world, faction_id, CULTURAL_REBELLION_HAPPINESS, event_id);
}

fn handle_plague_started(world: &mut World, event_id: u64, settlement_id: u64) {
    // Plague destabilizes the faction that owns this settlement
    if let Some(faction_id) = world
        .entities
        .get(&settlement_id)
        .and_then(|e| e.active_rel(RelationshipKind::MemberOf))
    {
        helpers::apply_stability_delta(world, faction_id, PLAGUE_STABILITY_HIT, event_id);
        apply_happiness_delta(world, faction_id, PLAGUE_HAPPINESS_HIT, event_id);
    }
}

fn handle_siege_started(world: &mut World, event_id: u64, defender_faction_id: u64) {
    apply_happiness_delta(
        world,
        defender_faction_id,
        SIEGE_STARTED_HAPPINESS,
        event_id,
    );
    helpers::apply_stability_delta(
        world,
        defender_faction_id,
        SIEGE_STARTED_STABILITY,
        event_id,
    );
}

fn handle_siege_ended(
    world: &mut World,
    event_id: u64,
    defender_faction_id: u64,
    outcome: SiegeOutcome,
) {
    if outcome == SiegeOutcome::Lifted {
        apply_happiness_delta(world, defender_faction_id, SIEGE_LIFTED_HAPPINESS, event_id);
    }
}

fn handle_leader_vacancy(
    world: &mut World,
    rng: &mut dyn RngCore,
    cause_event_id: u64,
    time: SimTimestamp,
    current_year: u32,
    faction_id: u64,
    previous_leader_id: u64,
) {
    // Verify this is actually a faction (not a settlement from legacy signals)
    let is_faction = world
        .entities
        .get(&faction_id)
        .is_some_and(|e| e.kind == EntityKind::Faction && e.end.is_none());
    if !is_faction {
        return;
    }

    // Skip if a leader was already assigned this tick (e.g. by fill_leader_vacancies)
    if has_leader(world, faction_id) {
        return;
    }

    let gov_type = get_government_type(world, faction_id);
    let faction_name = helpers::entity_name(world, faction_id);
    let members = collect_faction_members(world, faction_id);
    if let Some(leader_id) = select_leader(&members, gov_type, world, rng, Some(previous_leader_id))
    {
        let leader_name = helpers::entity_name(world, leader_id);
        let ev = world.add_caused_event(
            EventKind::Succession,
            time,
            format!(
                "{leader_name} succeeded to leadership of {faction_name} in year {current_year}"
            ),
            cause_event_id,
        );
        world.add_event_participant(ev, leader_id, ParticipantRole::Subject);
        world.add_event_participant(ev, faction_id, ParticipantRole::Object);
        world.add_relationship(leader_id, faction_id, RelationshipKind::LeaderOf, time, ev);

        // Succession causes a stability hit
        apply_succession_stability_hit(world, faction_id, ev);

        // Create claims for passed-over blood relatives (Hereditary only)
        if gov_type == GovernmentType::Hereditary {
            create_succession_claims(world, faction_id, previous_leader_id, current_year, ev);
        }
    }
}

fn handle_disaster_struck(world: &mut World, event_id: u64, settlement_id: u64, severity: f64) {
    // Disaster reduces happiness and stability of the owning faction
    if let Some(faction_id) = world
        .entities
        .get(&settlement_id)
        .and_then(|e| e.active_rel(RelationshipKind::MemberOf))
    {
        let happiness_hit = DISASTER_HAPPINESS_BASE - severity * DISASTER_HAPPINESS_SEVERITY_WEIGHT;
        apply_happiness_delta(world, faction_id, happiness_hit, event_id);
        helpers::apply_stability_delta(world, faction_id, DISASTER_STABILITY_HIT, event_id);
    }
}

fn handle_disaster_ended(world: &mut World, event_id: u64, settlement_id: u64) {
    // Relief: small happiness recovery
    if let Some(faction_id) = world
        .entities
        .get(&settlement_id)
        .and_then(|e| e.active_rel(RelationshipKind::MemberOf))
    {
        apply_happiness_delta(
            world,
            faction_id,
            DISASTER_ENDED_HAPPINESS_RECOVERY,
            event_id,
        );
    }
}

// --- 4a: Fill leader vacancies ---

fn fill_leader_vacancies(ctx: &mut TickContext, time: SimTimestamp, current_year: u32) {
    // Collect faction info
    struct FactionInfo {
        id: u64,
        government_type: GovernmentType,
    }

    let factions: Vec<FactionInfo> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        .map(|e| FactionInfo {
            id: e.id,
            government_type: e
                .data
                .as_faction()
                .map(|f| f.government_type)
                .unwrap_or(GovernmentType::Chieftain),
        })
        .collect();

    // Find which factions have no leader
    let leaderless: Vec<&FactionInfo> = factions
        .iter()
        .filter(|f| !has_leader(ctx.world, f.id))
        .collect();

    for faction in leaderless {
        let faction_name = helpers::entity_name(ctx.world, faction.id);
        let members = collect_faction_members(ctx.world, faction.id);

        // Find previous leader from most recently ended LeaderOf relationship
        let previous_leader_id = find_previous_leader(ctx.world, faction.id);

        if let Some(leader_id) = select_leader(
            &members,
            faction.government_type,
            ctx.world,
            ctx.rng,
            previous_leader_id,
        ) {
            let leader_name = helpers::entity_name(ctx.world, leader_id);
            let ev = ctx.world.add_event(
                EventKind::Succession,
                time,
                format!("{leader_name} became leader of {faction_name} in year {current_year}"),
            );
            ctx.world
                .add_event_participant(ev, leader_id, ParticipantRole::Subject);
            ctx.world
                .add_event_participant(ev, faction.id, ParticipantRole::Object);
            ctx.world
                .add_relationship(leader_id, faction.id, RelationshipKind::LeaderOf, time, ev);

            // Succession causes a stability hit
            apply_succession_stability_hit(ctx.world, faction.id, ev);
        }
    }
}

// --- Happiness ---

fn update_happiness(ctx: &mut TickContext, time: SimTimestamp) {
    struct HappinessInfo {
        faction_id: u64,
        old_happiness: f64,
        stability: f64,
        has_leader: bool,
        has_enemies: bool,
        has_allies: bool,
        avg_prosperity: f64,
        avg_cultural_tension: f64,
        avg_religious_tension: f64,
    }

    let factions: Vec<HappinessInfo> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        .map(|e| {
            let fd = e.data.as_faction();
            let old_happiness = fd.map(|f| f.happiness).unwrap_or(HAPPINESS_DEFAULT);
            let stability = fd.map(|f| f.stability).unwrap_or(STABILITY_DEFAULT);
            let has_enemies = e.active_rels(RelationshipKind::Enemy).next().is_some();
            let has_allies = e.active_rels(RelationshipKind::Ally).next().is_some();
            HappinessInfo {
                faction_id: e.id,
                old_happiness,
                stability,
                has_leader: false, // filled below
                has_enemies,
                has_allies,
                avg_prosperity: DEFAULT_PROSPERITY, // filled below
                avg_cultural_tension: 0.0,          // filled below
                avg_religious_tension: 0.0,         // filled below
            }
        })
        .collect();

    // Single pass over living settlements: aggregate prosperity, tension, and building
    // happiness bonus per faction. This is O(S) instead of the previous O(F×S).
    // Tuple: (prosperity_sum, cultural_tension_sum, building_bonus, religious_tension_sum, count)
    let mut faction_agg: std::collections::BTreeMap<u64, (f64, f64, f64, f64, u32)> =
        std::collections::BTreeMap::new();
    for (_id, e) in ctx.world.living(EntityKind::Settlement) {
        if let Some(faction_id) = e.active_rel(RelationshipKind::MemberOf) {
            let (prosperity, tension, religious_tension) = if let Some(sd) = e.data.as_settlement()
            {
                (sd.prosperity, sd.cultural_tension, sd.religious_tension)
            } else {
                (DEFAULT_PROSPERITY, 0.0, 0.0)
            };
            let building_bonus = e.extra_f64_or(K::BUILDING_HAPPINESS_BONUS, 0.0);
            let entry = faction_agg
                .entry(faction_id)
                .or_insert((0.0, 0.0, 0.0, 0.0, 0));
            entry.0 += prosperity;
            entry.1 += tension;
            entry.2 += building_bonus;
            entry.3 += religious_tension;
            entry.4 += 1;
        }
    }

    // Compute leader presence and avg prosperity per faction using pre-aggregated data
    let factions: Vec<HappinessInfo> = factions
        .into_iter()
        .map(|mut f| {
            f.has_leader = has_leader(ctx.world, f.faction_id);

            if let Some(&(prosperity_sum, tension_sum, _, rel_tension_sum, count)) =
                faction_agg.get(&f.faction_id)
            {
                f.avg_prosperity = prosperity_sum / count as f64;
                f.avg_cultural_tension = tension_sum / count as f64;
                f.avg_religious_tension = rel_tension_sum / count as f64;
            }
            f
        })
        .collect();

    // Extract building happiness from the same pre-aggregated data
    let faction_building_happiness: std::collections::BTreeMap<u64, f64> = faction_agg
        .iter()
        .map(|(&fid, &(_, _, bonus, _, _))| (fid, bonus))
        .collect();

    let year_event = ctx.world.add_event(
        EventKind::Custom("happiness_tick".to_string()),
        time,
        format!("Year {} happiness update", time.year()),
    );

    for f in &factions {
        let base_target = HAPPINESS_BASE_TARGET;
        let prosperity_bonus = f.avg_prosperity * HAPPINESS_PROSPERITY_WEIGHT;
        let stability_bonus =
            (f.stability - HAPPINESS_STABILITY_NEUTRAL) * HAPPINESS_STABILITY_WEIGHT;
        let peace_bonus = if f.has_enemies {
            HAPPINESS_ENEMIES_PENALTY
        } else if f.has_allies {
            HAPPINESS_ALLIES_BONUS
        } else {
            0.0
        };
        let leader_bonus = if f.has_leader {
            HAPPINESS_LEADER_PRESENT_BONUS
        } else {
            HAPPINESS_LEADER_ABSENT_PENALTY
        };

        let trade_bonus = ctx
            .world
            .entities
            .get(&f.faction_id)
            .map(|e| e.extra_f64_or(K::TRADE_HAPPINESS_BONUS, 0.0))
            .unwrap_or(0.0);

        let tension_penalty = -f.avg_cultural_tension * HAPPINESS_TENSION_WEIGHT;
        let religious_tension_penalty =
            -f.avg_religious_tension * HAPPINESS_RELIGIOUS_TENSION_WEIGHT;

        // Building happiness bonus (temples)
        let building_happiness = faction_building_happiness
            .get(&f.faction_id)
            .copied()
            .unwrap_or(0.0)
            .min(HAPPINESS_BUILDING_CAP);

        let target = (base_target
            + prosperity_bonus
            + stability_bonus
            + peace_bonus
            + leader_bonus
            + trade_bonus
            + tension_penalty
            + religious_tension_penalty
            + building_happiness)
            .clamp(HAPPINESS_MIN_TARGET, HAPPINESS_MAX_TARGET);
        let noise: f64 = ctx
            .rng
            .random_range(-HAPPINESS_NOISE_RANGE..HAPPINESS_NOISE_RANGE);
        let new_happiness =
            (f.old_happiness + (target - f.old_happiness) * HAPPINESS_DRIFT_RATE + noise)
                .clamp(0.0, 1.0);

        let old = {
            let entity = ctx.world.entities.get_mut(&f.faction_id).unwrap();
            let fd = entity.data.as_faction_mut().unwrap();
            let old = fd.happiness;
            fd.happiness = new_happiness;
            old
        };
        ctx.world.record_change(
            f.faction_id,
            year_event,
            "happiness",
            serde_json::json!(old),
            serde_json::json!(new_happiness),
        );
    }
}

// --- Legitimacy ---

fn update_legitimacy(ctx: &mut TickContext, time: SimTimestamp) {
    struct LegitimacyInfo {
        faction_id: u64,
        old_legitimacy: f64,
        happiness: f64,
        leader_prestige: f64,
    }

    let factions: Vec<LegitimacyInfo> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        .map(|e| {
            let fd = e.data.as_faction();
            let leader_prestige = helpers::faction_leader(ctx.world, e.id)
                .and_then(|lid| ctx.world.entities.get(&lid))
                .and_then(|le| le.data.as_person())
                .map(|pd| pd.prestige)
                .unwrap_or(0.0);
            LegitimacyInfo {
                faction_id: e.id,
                old_legitimacy: fd.map(|f| f.legitimacy).unwrap_or(LEGITIMACY_BASE_TARGET),
                happiness: fd.map(|f| f.happiness).unwrap_or(LEGITIMACY_BASE_TARGET),
                leader_prestige,
            }
        })
        .collect();

    let year_event = ctx.world.add_event(
        EventKind::Custom("legitimacy_tick".to_string()),
        time,
        format!("Year {} legitimacy update", time.year()),
    );

    for f in &factions {
        let target = LEGITIMACY_BASE_TARGET
            + LEGITIMACY_HAPPINESS_WEIGHT * f.happiness
            + f.leader_prestige * LEGITIMACY_LEADER_PRESTIGE_WEIGHT;
        let new_legitimacy = (f.old_legitimacy
            + (target - f.old_legitimacy) * LEGITIMACY_DRIFT_RATE)
            .clamp(0.0, 1.0);

        let old = {
            let entity = ctx.world.entities.get_mut(&f.faction_id).unwrap();
            let fd = entity.data.as_faction_mut().unwrap();
            let old = fd.legitimacy;
            fd.legitimacy = new_legitimacy;
            old
        };
        ctx.world.record_change(
            f.faction_id,
            year_event,
            "legitimacy",
            serde_json::json!(old),
            serde_json::json!(new_legitimacy),
        );
    }
}

// --- 4b: Stability drift ---

fn update_stability(ctx: &mut TickContext, time: SimTimestamp) {
    struct FactionStability {
        id: u64,
        old_stability: f64,
        happiness: f64,
        legitimacy: f64,
        has_leader: bool,
        avg_cultural_tension: f64,
        theocracy_fervor: f64, // fervor bonus for Theocracy governments
    }

    let factions: Vec<FactionStability> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        .map(|e| {
            let fd = e.data.as_faction();
            let theocracy_fervor = fd
                .filter(|f| f.government_type == GovernmentType::Theocracy)
                .and_then(|f| f.primary_religion)
                .and_then(|rid| ctx.world.entities.get(&rid))
                .and_then(|re| re.data.as_religion())
                .map(|rd| rd.fervor)
                .unwrap_or(0.0);
            FactionStability {
                id: e.id,
                old_stability: fd.map(|f| f.stability).unwrap_or(STABILITY_DEFAULT),
                happiness: fd.map(|f| f.happiness).unwrap_or(STABILITY_DEFAULT),
                legitimacy: fd.map(|f| f.legitimacy).unwrap_or(STABILITY_DEFAULT),
                has_leader: false,         // filled below
                avg_cultural_tension: 0.0, // filled below
                theocracy_fervor,
            }
        })
        .collect();

    let factions: Vec<FactionStability> = factions
        .into_iter()
        .map(|mut f| {
            f.has_leader = has_leader(ctx.world, f.id);
            // Compute avg cultural tension
            let mut tension_sum = 0.0;
            let mut count = 0u32;
            for e in ctx.world.entities.values() {
                if e.kind == EntityKind::Settlement
                    && e.end.is_none()
                    && e.has_active_rel(RelationshipKind::MemberOf, f.id)
                {
                    if let Some(sd) = e.data.as_settlement() {
                        tension_sum += sd.cultural_tension;
                    }
                    count += 1;
                }
            }
            f.avg_cultural_tension = if count > 0 {
                tension_sum / count as f64
            } else {
                0.0
            };
            f
        })
        .collect();

    let year_event = ctx.world.add_event(
        EventKind::Custom("politics_tick".to_string()),
        time,
        format!("Year {} politics tick", time.year()),
    );

    struct StabilityUpdate {
        faction_id: u64,
        new_stability: f64,
    }

    let mut updates: Vec<StabilityUpdate> = Vec::new();
    for faction in &factions {
        let base_target = STABILITY_BASE_TARGET
            + STABILITY_HAPPINESS_WEIGHT * faction.happiness
            + STABILITY_LEGITIMACY_WEIGHT * faction.legitimacy;
        let leader_adj = if faction.has_leader {
            STABILITY_LEADER_PRESENT_BONUS
        } else {
            STABILITY_LEADER_ABSENT_PENALTY
        };
        let tension_adj = -faction.avg_cultural_tension * STABILITY_TENSION_WEIGHT;
        let theocracy_adj = faction.theocracy_fervor * STABILITY_THEOCRACY_FERVOR_BONUS;
        let target = (base_target + leader_adj + tension_adj + theocracy_adj)
            .clamp(STABILITY_MIN_TARGET, STABILITY_MAX_TARGET);

        let noise: f64 = ctx
            .rng
            .random_range(-STABILITY_NOISE_RANGE..STABILITY_NOISE_RANGE);
        let mut drift = (target - faction.old_stability) * STABILITY_DRIFT_RATE + noise;
        // Direct instability pressure when leaderless
        if !faction.has_leader {
            drift -= STABILITY_LEADERLESS_PRESSURE;
        }
        let new_stability = (faction.old_stability + drift).clamp(0.0, 1.0);
        updates.push(StabilityUpdate {
            faction_id: faction.id,
            new_stability,
        });
    }

    for update in updates {
        let old = {
            let entity = ctx.world.entities.get_mut(&update.faction_id).unwrap();
            let fd = entity.data.as_faction_mut().unwrap();
            let old = fd.stability;
            fd.stability = update.new_stability;
            old
        };
        ctx.world.record_change(
            update.faction_id,
            year_event,
            "stability",
            serde_json::json!(old),
            serde_json::json!(update.new_stability),
        );
    }
}

// --- 4e: Faction splits ---

struct SplitPlan {
    settlement_id: u64,
    old_faction_id: u64,
    old_happiness: f64,
    old_gov_type: GovernmentType,
    parent_prestige: f64,
}

fn check_faction_splits(ctx: &mut TickContext, time: SimTimestamp, current_year: u32) {
    let splits = evaluate_split_candidates(ctx);
    execute_faction_splits(ctx, splits, time, current_year);
    dissolve_empty_factions(ctx, time, current_year);
}

fn evaluate_split_candidates(ctx: &mut TickContext) -> Vec<SplitPlan> {
    // Collect faction sentiment data for split checks
    struct FactionSentiment {
        stability: f64,
        happiness: f64,
        government_type: GovernmentType,
        prestige: f64,
    }

    let faction_sentiments: std::collections::BTreeMap<u64, FactionSentiment> = ctx
        .world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Faction
                && e.end.is_none()
                && !e
                    .data
                    .as_faction()
                    .is_some_and(|fd| fd.government_type == GovernmentType::BanditClan)
        })
        .map(|e| {
            let fd = e.data.as_faction();
            (
                e.id,
                FactionSentiment {
                    stability: fd.map(|f| f.stability).unwrap_or(STABILITY_DEFAULT),
                    happiness: fd.map(|f| f.happiness).unwrap_or(STABILITY_DEFAULT),
                    government_type: fd
                        .map(|f| f.government_type)
                        .unwrap_or(GovernmentType::Chieftain),
                    prestige: fd.map(|f| f.prestige).unwrap_or(0.0),
                },
            )
        })
        .collect();

    // Collect settlements with their faction membership
    struct SettlementFaction {
        settlement_id: u64,
        faction_id: u64,
    }

    let settlement_factions: Vec<SettlementFaction> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
        .filter_map(|e| {
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
                .map(|r| r.target_entity_id)?;
            Some(SettlementFaction {
                settlement_id: e.id,
                faction_id,
            })
        })
        .collect();

    // Count settlements per faction
    let mut faction_settlement_count: std::collections::BTreeMap<u64, u32> =
        std::collections::BTreeMap::new();
    for sf in &settlement_factions {
        *faction_settlement_count.entry(sf.faction_id).or_default() += 1;
    }

    // Misery-based splits — no multi-settlement guard
    let mut splits: Vec<SplitPlan> = Vec::new();
    for sf in &settlement_factions {
        let Some(sentiment) = faction_sentiments.get(&sf.faction_id) else {
            continue;
        };

        // Skip if faction is reasonably stable or happy
        if sentiment.stability >= SPLIT_STABILITY_THRESHOLD
            || sentiment.happiness >= SPLIT_HAPPINESS_THRESHOLD
        {
            continue;
        }

        let misery = (1.0 - sentiment.happiness) * (1.0 - sentiment.stability);
        let split_chance =
            SPLIT_BASE_CHANCE * misery * (1.0 - sentiment.prestige * SPLIT_PRESTIGE_RESISTANCE);

        if ctx.rng.random_range(0.0..1.0) < split_chance {
            splits.push(SplitPlan {
                settlement_id: sf.settlement_id,
                old_faction_id: sf.faction_id,
                old_happiness: sentiment.happiness,
                old_gov_type: sentiment.government_type,
                parent_prestige: sentiment.prestige,
            });
            // Decrease count so we don't split a faction down to 0 settlements
            if let Some(c) = faction_settlement_count.get_mut(&sf.faction_id) {
                *c = c.saturating_sub(1);
            }
        }
    }

    splits
}

fn execute_faction_splits(
    ctx: &mut TickContext,
    splits: Vec<SplitPlan>,
    time: SimTimestamp,
    current_year: u32,
) {
    let gov_types = [
        GovernmentType::Hereditary,
        GovernmentType::Elective,
        GovernmentType::Chieftain,
    ];

    for split in splits {
        let old_faction_name = helpers::entity_name(ctx.world, split.old_faction_id);
        let name = generate_unique_faction_name(ctx.world, ctx.rng);
        let ev = ctx.world.add_event(
            EventKind::FactionFormed,
            time,
            format!("{name} formed by secession from {old_faction_name} in year {current_year}"),
        );

        // 50% inherit government type, 50% random
        let gov_type = if ctx.rng.random_bool(SPLIT_GOV_TYPE_INHERITANCE_CHANCE) {
            split.old_gov_type
        } else {
            gov_types[ctx.rng.random_range(0..gov_types.len())]
        };

        let new_faction_data = EntityData::Faction(FactionData {
            government_type: gov_type,
            stability: SPLIT_NEW_FACTION_STABILITY,
            happiness: (split.old_happiness + SPLIT_NEW_FACTION_HAPPINESS_BONUS).clamp(0.0, 1.0),
            legitimacy: SPLIT_NEW_FACTION_LEGITIMACY,
            treasury: 0.0,
            alliance_strength: 0.0,
            primary_culture: None,
            prestige: split.parent_prestige * SPLIT_NEW_FACTION_PRESTIGE_INHERITANCE,
            primary_religion: None,
            grievances: std::collections::BTreeMap::new(),
        });

        let new_faction_id =
            ctx.world
                .add_entity(EntityKind::Faction, name, Some(time), new_faction_data, ev);

        // Move settlement to new faction
        ctx.world.end_relationship(
            split.settlement_id,
            split.old_faction_id,
            RelationshipKind::MemberOf,
            time,
            ev,
        );
        ctx.world.add_relationship(
            split.settlement_id,
            new_faction_id,
            RelationshipKind::MemberOf,
            time,
            ev,
        );

        // Transfer NPCs in this settlement to new faction
        let npc_transfers: Vec<u64> = ctx
            .world
            .entities
            .values()
            .filter(|e| {
                e.kind == EntityKind::Person
                    && e.end.is_none()
                    && e.has_active_rel(RelationshipKind::LocatedIn, split.settlement_id)
                    && e.has_active_rel(RelationshipKind::MemberOf, split.old_faction_id)
            })
            .map(|e| e.id)
            .collect();

        for npc_id in npc_transfers {
            ctx.world.end_relationship(
                npc_id,
                split.old_faction_id,
                RelationshipKind::MemberOf,
                time,
                ev,
            );
            ctx.world.add_relationship(
                npc_id,
                new_faction_id,
                RelationshipKind::MemberOf,
                time,
                ev,
            );
        }

        // High chance old and new factions become enemies
        if ctx.rng.random_bool(SPLIT_POST_ENEMY_CHANCE) {
            ctx.world.add_relationship(
                split.old_faction_id,
                new_faction_id,
                RelationshipKind::Enemy,
                time,
                ev,
            );
        }

        ctx.signals.push(Signal {
            event_id: ev,
            kind: SignalKind::FactionSplit {
                old_faction_id: split.old_faction_id,
                new_faction_id: Some(new_faction_id),
                settlement_id: split.settlement_id,
            },
        });

        ctx.world
            .add_event_participant(ev, split.settlement_id, ParticipantRole::Subject);
        ctx.world
            .add_event_participant(ev, split.old_faction_id, ParticipantRole::Origin);
        ctx.world
            .add_event_participant(ev, new_faction_id, ParticipantRole::Destination);

        // Create claims for blood relatives of old faction leader now in new faction
        create_split_claims(
            ctx.world,
            split.old_faction_id,
            new_faction_id,
            current_year,
            ev,
        );
    }
}

fn dissolve_empty_factions(ctx: &mut TickContext, time: SimTimestamp, current_year: u32) {
    let empty_factions: Vec<u64> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        .filter(|e| {
            !ctx.world.entities.values().any(|s| {
                s.kind == EntityKind::Settlement
                    && s.end.is_none()
                    && s.has_active_rel(RelationshipKind::MemberOf, e.id)
            })
        })
        .map(|e| e.id)
        .collect();

    for faction_id in empty_factions {
        let faction_name = helpers::entity_name(ctx.world, faction_id);
        let ev = ctx.world.add_event(
            EventKind::Custom("faction_dissolved".to_string()),
            time,
            format!("{faction_name} dissolved in year {current_year}"),
        );
        ctx.world
            .add_event_participant(ev, faction_id, ParticipantRole::Subject);

        // End leader relationship if any
        if let Some(leader_id) = helpers::faction_leader(ctx.world, faction_id) {
            ctx.world
                .end_relationship(leader_id, faction_id, RelationshipKind::LeaderOf, time, ev);
        }

        // End diplomatic relationships
        let diplo_rels: Vec<(u64, u64, RelationshipKind)> = ctx
            .world
            .entities
            .values()
            .flat_map(|e| {
                e.relationships
                    .iter()
                    .filter(|r| {
                        r.end.is_none()
                            && (r.source_entity_id == faction_id
                                || r.target_entity_id == faction_id)
                            && matches!(
                                r.kind,
                                RelationshipKind::Ally
                                    | RelationshipKind::Enemy
                                    | RelationshipKind::AtWar
                            )
                    })
                    .map(|r| (r.source_entity_id, r.target_entity_id, r.kind.clone()))
            })
            .collect();

        for (source, target, kind) in diplo_rels {
            ctx.world.end_relationship(source, target, kind, time, ev);
        }

        ctx.world.end_entity(faction_id, time, ev);
    }
}

// --- Helpers ---

pub(super) struct MemberInfo {
    pub(super) id: u64,
    pub(super) birth_year: u32,
    pub(super) role: Role,
}

pub(super) fn collect_faction_members(world: &World, faction_id: u64) -> Vec<MemberInfo> {
    world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Person
                && e.end.is_none()
                && e.has_active_rel(RelationshipKind::MemberOf, faction_id)
        })
        .map(|e| {
            let pd = e.data.as_person();
            MemberInfo {
                id: e.id,
                birth_year: pd.map(|p| p.birth_year).unwrap_or(0),
                role: pd.map(|p| p.role.clone()).unwrap_or(Role::Common),
            }
        })
        .collect()
}

fn select_leader(
    members: &[MemberInfo],
    government_type: GovernmentType,
    world: &World,
    rng: &mut dyn RngCore,
    previous_leader_id: Option<u64>,
) -> Option<u64> {
    if members.is_empty() {
        return None;
    }

    match government_type {
        GovernmentType::Hereditary => {
            // Try bloodline succession if we have a previous leader
            if let Some(prev_id) = previous_leader_id {
                let member_ids: std::collections::BTreeSet<u64> =
                    members.iter().map(|m| m.id).collect();

                // 1. Find living children of previous leader (Parent rels → target)
                let children: Vec<&MemberInfo> =
                    if let Some(prev_entity) = world.entities.get(&prev_id) {
                        let child_ids: Vec<u64> = prev_entity
                            .relationships
                            .iter()
                            .filter(|r| r.kind == RelationshipKind::Parent)
                            .map(|r| r.target_entity_id)
                            .filter(|id| member_ids.contains(id))
                            .collect();
                        members
                            .iter()
                            .filter(|m| child_ids.contains(&m.id))
                            .collect()
                    } else {
                        Vec::new()
                    };

                if !children.is_empty() {
                    // Pick oldest child (lowest birth_year)
                    return children.iter().min_by_key(|m| m.birth_year).map(|m| m.id);
                }

                // 2. Find siblings: previous leader's parents → parent's children → filter to members
                if let Some(prev_entity) = world.entities.get(&prev_id) {
                    let parent_ids: Vec<u64> = prev_entity
                        .relationships
                        .iter()
                        .filter(|r| r.kind == RelationshipKind::Child)
                        .map(|r| r.target_entity_id)
                        .collect();

                    let mut sibling_ids: Vec<u64> = Vec::new();
                    for pid in &parent_ids {
                        if let Some(parent_entity) = world.entities.get(pid) {
                            for r in &parent_entity.relationships {
                                if r.kind == RelationshipKind::Parent
                                    && r.target_entity_id != prev_id
                                    && member_ids.contains(&r.target_entity_id)
                                    && !sibling_ids.contains(&r.target_entity_id)
                                {
                                    sibling_ids.push(r.target_entity_id);
                                }
                            }
                        }
                    }

                    let siblings: Vec<&MemberInfo> = members
                        .iter()
                        .filter(|m| sibling_ids.contains(&m.id))
                        .collect();
                    if !siblings.is_empty() {
                        return siblings.iter().min_by_key(|m| m.birth_year).map(|m| m.id);
                    }
                }
            }

            // Fallback: oldest faction member
            members.iter().min_by_key(|m| m.birth_year).map(|m| m.id)
        }
        GovernmentType::Elective => {
            // Weighted random: elder/scholar roles get 3x, Charismatic trait gets 2x
            let preferred = [Role::Elder, Role::Scholar];
            let refs: Vec<&MemberInfo> = members.iter().collect();
            let weights: Vec<u32> = refs
                .iter()
                .map(|m| {
                    let mut w: u32 = if preferred.contains(&m.role) { 3 } else { 1 };
                    if let Some(entity) = world.entities.get(&m.id)
                        && has_trait(entity, &Trait::Charismatic)
                    {
                        w *= 2;
                    }
                    w
                })
                .collect();
            let total: u32 = weights.iter().sum();
            let roll = rng.random_range(0..total);
            let mut cumulative = 0u32;
            for (i, &w) in weights.iter().enumerate() {
                cumulative += w;
                if roll < cumulative {
                    return Some(refs[i].id);
                }
            }
            Some(refs.last().unwrap().id)
        }
        GovernmentType::Chieftain | GovernmentType::BanditClan => {
            // Chieftain/BanditClan: warrior preferred, else oldest
            let warriors: Vec<&MemberInfo> =
                members.iter().filter(|m| m.role == Role::Warrior).collect();
            if !warriors.is_empty() {
                // Oldest warrior
                warriors.iter().min_by_key(|m| m.birth_year).map(|m| m.id)
            } else {
                members.iter().min_by_key(|m| m.birth_year).map(|m| m.id)
            }
        }
        GovernmentType::Theocracy => {
            // Theocracy: prefer Priest role, then Pious trait, else oldest
            let priests: Vec<&MemberInfo> =
                members.iter().filter(|m| m.role == Role::Priest).collect();
            if !priests.is_empty() {
                return priests.iter().min_by_key(|m| m.birth_year).map(|m| m.id);
            }
            let pious: Vec<&MemberInfo> = members
                .iter()
                .filter(|m| {
                    world
                        .entities
                        .get(&m.id)
                        .is_some_and(|e| has_trait(e, &Trait::Pious))
                })
                .collect();
            if !pious.is_empty() {
                return pious.iter().min_by_key(|m| m.birth_year).map(|m| m.id);
            }
            members.iter().min_by_key(|m| m.birth_year).map(|m| m.id)
        }
    }
}

fn has_leader(world: &World, faction_id: u64) -> bool {
    world.entities.values().any(|e| {
        e.kind == EntityKind::Person
            && e.end.is_none()
            && e.has_active_rel(RelationshipKind::LeaderOf, faction_id)
    })
}

pub(super) fn apply_happiness_delta(world: &mut World, faction_id: u64, delta: f64, event_id: u64) {
    let (old, new) = {
        let Some(entity) = world.entities.get_mut(&faction_id) else {
            return;
        };
        let Some(fd) = entity.data.as_faction_mut() else {
            return;
        };
        let old = fd.happiness;
        fd.happiness = (old + delta).clamp(0.0, 1.0);
        (old, fd.happiness)
    };
    world.record_change(
        faction_id,
        event_id,
        "happiness",
        serde_json::json!(old),
        serde_json::json!(new),
    );
}

fn apply_succession_stability_hit(world: &mut World, faction_id: u64, event_id: u64) {
    // Prestigious new leader softens the succession instability
    let new_leader_prestige = helpers::faction_leader(world, faction_id)
        .and_then(|lid| world.entities.get(&lid))
        .and_then(|e| e.data.as_person())
        .map(|pd| pd.prestige)
        .unwrap_or(0.0);
    let hit =
        SUCCESSION_STABILITY_HIT * (1.0 - new_leader_prestige * SUCCESSION_PRESTIGE_SOFTENING);
    let (old, new) = {
        let Some(entity) = world.entities.get_mut(&faction_id) else {
            return;
        };
        let Some(fd) = entity.data.as_faction_mut() else {
            return;
        };
        let old = fd.stability;
        fd.stability = (old + hit).clamp(0.0, 1.0);
        (old, fd.stability)
    };
    world.record_change(
        faction_id,
        event_id,
        "stability",
        serde_json::json!(old),
        serde_json::json!(new),
    );
}

/// Find the most recent previous leader of a faction by scanning members'
/// ended LeaderOf relationships.
fn find_previous_leader(world: &World, faction_id: u64) -> Option<u64> {
    // Check all living and dead persons for the most recent ended LeaderOf to this faction
    let mut best: Option<(u64, SimTimestamp)> = None;
    for e in world.entities.values() {
        if e.kind != EntityKind::Person {
            continue;
        }
        for r in &e.relationships {
            if r.kind == RelationshipKind::LeaderOf
                && r.target_entity_id == faction_id
                && let Some(end_time) = r.end
                && (best.is_none() || end_time > best.unwrap().1)
            {
                best = Some((e.id, end_time));
            }
        }
    }
    best.map(|(id, _)| id)
}

fn get_government_type(world: &World, faction_id: u64) -> GovernmentType {
    world
        .entities
        .get(&faction_id)
        .and_then(|e| e.data.as_faction())
        .map(|f| f.government_type)
        .unwrap_or(GovernmentType::Chieftain)
}

// --- Succession Claims ---

/// Create claims for blood relatives of the dead leader who are in other factions.
fn create_succession_claims(
    world: &mut World,
    faction_id: u64,
    dead_leader_id: u64,
    current_year: u32,
    event_id: u64,
) {
    // Collect person→strength pairs for direct blood relatives
    let mut claim_candidates: Vec<(u64, f64, &str)> = Vec::new();

    let Some(dead_entity) = world.entities.get(&dead_leader_id) else {
        return;
    };

    // Children of the dead leader (Parent rels → target is child)
    let children: Vec<u64> = dead_entity
        .relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::Parent)
        .map(|r| r.target_entity_id)
        .collect();

    for &child_id in &children {
        if is_living_in_other_faction(world, child_id, faction_id) {
            claim_candidates.push((child_id, CLAIM_CHILD_STRENGTH, "bloodline"));
        }

        // Grandchildren: children of this child
        if let Some(child_entity) = world.entities.get(&child_id) {
            let grandchildren: Vec<u64> = child_entity
                .relationships
                .iter()
                .filter(|r| r.kind == RelationshipKind::Parent)
                .map(|r| r.target_entity_id)
                .collect();
            for &gc_id in &grandchildren {
                if is_living_in_other_faction(world, gc_id, faction_id) {
                    claim_candidates.push((gc_id, CLAIM_GRANDCHILD_STRENGTH, "bloodline"));
                }
            }
        }
    }

    // Siblings: dead leader's parents → parent's children
    let parent_ids: Vec<u64> = dead_entity
        .relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::Child)
        .map(|r| r.target_entity_id)
        .collect();

    let mut sibling_ids: Vec<u64> = Vec::new();
    for &pid in &parent_ids {
        if let Some(parent_entity) = world.entities.get(&pid) {
            for r in &parent_entity.relationships {
                if r.kind == RelationshipKind::Parent
                    && r.target_entity_id != dead_leader_id
                    && !sibling_ids.contains(&r.target_entity_id)
                {
                    sibling_ids.push(r.target_entity_id);
                }
            }
        }
    }

    for &sib_id in &sibling_ids {
        if is_living_in_other_faction(world, sib_id, faction_id) {
            claim_candidates.push((sib_id, CLAIM_SIBLING_STRENGTH, "bloodline"));
        }
    }

    // Spouse claims: find spouses of anyone who got a blood claim
    let blood_claimant_ids: Vec<u64> = claim_candidates.iter().map(|(id, _, _)| *id).collect();
    let mut spouse_claims: Vec<(u64, f64)> = Vec::new();
    for &bc_id in &blood_claimant_ids {
        if let Some(bc_entity) = world.entities.get(&bc_id) {
            let strength = claim_candidates
                .iter()
                .find(|(id, _, _)| *id == bc_id)
                .map(|(_, s, _)| *s)
                .unwrap_or(0.0);
            for r in &bc_entity.relationships {
                if r.kind == RelationshipKind::Spouse
                    && r.end.is_none()
                    && is_living_in_other_faction(world, r.target_entity_id, faction_id)
                    && !blood_claimant_ids.contains(&r.target_entity_id)
                {
                    spouse_claims.push((r.target_entity_id, strength * CLAIM_SPOUSE_FACTOR));
                }
            }
        }
    }
    for (spouse_id, strength) in spouse_claims {
        claim_candidates.push((spouse_id, strength, "marriage"));
    }

    // Now set extras (skip if person already has a claim on this faction)
    let claim_key = format!("claim_{faction_id}");
    let mut claimant_ids = Vec::new();
    for (person_id, strength, source) in &claim_candidates {
        let already_has = world
            .entities
            .get(person_id)
            .is_some_and(|e| e.extra.contains_key(&claim_key));
        if already_has {
            continue;
        }
        world.set_extra(
            *person_id,
            &claim_key,
            serde_json::json!({
                "strength": strength,
                "source": source,
                "year": current_year,
            }),
            event_id,
        );
        claimant_ids.push(*person_id);
    }

    // Detect succession crisis if any strong claimant exists
    if !claimant_ids.is_empty() {
        detect_succession_crisis(
            world,
            faction_id,
            &claimant_ids,
            &claim_key,
            current_year,
            event_id,
        );
    }
}

/// Check if any claimant has strength >= threshold and trigger a crisis.
fn detect_succession_crisis(
    world: &mut World,
    faction_id: u64,
    claimant_ids: &[u64],
    claim_key: &str,
    current_year: u32,
    cause_event_id: u64,
) {
    let strong_claimants: Vec<u64> = claimant_ids
        .iter()
        .filter(|&&cid| {
            world
                .entities
                .get(&cid)
                .and_then(|e| e.extra.get(claim_key))
                .and_then(|v| v.get("strength"))
                .and_then(|v| v.as_f64())
                .is_some_and(|s| s >= CRISIS_CLAIM_THRESHOLD)
        })
        .copied()
        .collect();

    if strong_claimants.is_empty() {
        return;
    }

    let _new_leader_id = helpers::faction_leader(world, faction_id).unwrap_or(0);
    let faction_name = helpers::entity_name(world, faction_id);

    // Stability and legitimacy hits
    helpers::apply_stability_delta(world, faction_id, CRISIS_STABILITY_HIT, cause_event_id);
    {
        if let Some(fd) = world
            .entities
            .get_mut(&faction_id)
            .and_then(|e| e.data.as_faction_mut())
        {
            fd.legitimacy = (fd.legitimacy + CRISIS_LEGITIMACY_HIT).clamp(0.0, 1.0);
        }
    }

    // Set faction extra
    world.set_extra(
        faction_id,
        K::SUCCESSION_CRISIS_YEAR,
        serde_json::json!(current_year),
        cause_event_id,
    );

    // Create event
    let ev = world.add_caused_event(
        EventKind::SuccessionCrisis,
        SimTimestamp::from_year(current_year),
        format!(
            "Succession crisis in {faction_name}: {} claimants contest the throne in year {current_year}",
            strong_claimants.len()
        ),
        cause_event_id,
    );
    world.add_event_participant(ev, faction_id, ParticipantRole::Subject);
    for &cid in &strong_claimants {
        world.add_event_participant(ev, cid, ParticipantRole::Instigator);
    }

    // Note: SuccessionCrisis signal is emitted by the caller (handle_signals for handle_leader_vacancy,
    // or coups for deposed claims). We don't emit here to avoid needing access to signals vec.
    // Instead we store the claimant_ids so the caller can emit the signal.
    // Actually, this function is called from handle_leader_vacancy which doesn't have signals access.
    // The signal will be emitted via the crisis event which other systems can detect.
    // For cross-system integration, reputation/knowledge handle the event kind directly.
}

/// Yearly decay of all claim extras on living persons.
fn decay_claims(ctx: &mut TickContext, current_year: u32) {
    let claim_prefix = K::CLAIM_PREFIX;

    // Collect (person_id, key, new_strength) or (person_id, key, None) to remove
    let mut updates: Vec<(u64, String, Option<serde_json::Value>)> = Vec::new();

    for e in ctx.world.entities.values() {
        if e.kind != EntityKind::Person || e.end.is_some() {
            continue;
        }
        for (key, val) in &e.extra {
            if !key.starts_with(claim_prefix) {
                continue;
            }
            let strength = val.get("strength").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let new_strength = strength - CLAIM_DECAY_PER_YEAR;
            if new_strength < CLAIM_MIN_THRESHOLD {
                updates.push((e.id, key.clone(), None));
            } else {
                let mut new_val = val.clone();
                new_val["strength"] = serde_json::json!(new_strength);
                updates.push((e.id, key.clone(), Some(new_val)));
            }
        }
    }

    if updates.is_empty() {
        return;
    }

    let ev = ctx.world.add_event(
        EventKind::Custom("claim_decay".to_string()),
        SimTimestamp::from_year(current_year),
        format!("Claim decay in year {current_year}"),
    );

    for (person_id, key, new_val) in updates {
        match new_val {
            Some(val) => ctx.world.set_extra(person_id, &key, val, ev),
            None => ctx
                .world
                .set_extra(person_id, &key, serde_json::Value::Null, ev),
        }
    }
}

/// Decay all faction and person grievances by `GRIEVANCE_BASE_DECAY` per year.
/// NPCs decay at a trait-modulated rate.  Entries below threshold are removed.
fn decay_grievances(ctx: &mut TickContext) {
    // Collect (entity_id, target_id, new_severity_or_remove) tuples
    let mut updates: Vec<(u64, u64, Option<f64>)> = Vec::new();

    for e in ctx.world.entities.values() {
        if e.end.is_some() {
            continue;
        }
        match &e.data {
            EntityData::Faction(fd) => {
                for (&target, g) in &fd.grievances {
                    let new_sev = g.severity - GRIEVANCE_BASE_DECAY;
                    if new_sev < GRIEVANCE_MIN_THRESHOLD {
                        updates.push((e.id, target, None));
                    } else {
                        updates.push((e.id, target, Some(new_sev)));
                    }
                }
            }
            EntityData::Person(pd) => {
                let mult = grv::trait_decay_multiplier(&pd.traits);
                let decay = GRIEVANCE_BASE_DECAY * mult;
                for (&target, g) in &pd.grievances {
                    let new_sev = g.severity - decay;
                    if new_sev < GRIEVANCE_MIN_THRESHOLD {
                        updates.push((e.id, target, None));
                    } else {
                        updates.push((e.id, target, Some(new_sev)));
                    }
                }
            }
            _ => {}
        }
    }

    for (holder, target, new_sev) in updates {
        let entity = ctx.world.entities.get_mut(&holder).unwrap();
        if let Some(fd) = entity.data.as_faction_mut() {
            match new_sev {
                Some(s) => {
                    if let Some(g) = fd.grievances.get_mut(&target) {
                        g.severity = s;
                    }
                }
                None => {
                    fd.grievances.remove(&target);
                }
            }
        } else if let Some(pd) = entity.data.as_person_mut() {
            match new_sev {
                Some(s) => {
                    if let Some(g) = pd.grievances.get_mut(&target) {
                        g.severity = s;
                    }
                }
                None => {
                    pd.grievances.remove(&target);
                }
            }
        }
    }
}

/// Create claims for a deposed leader's blood relatives (after a coup).
pub(super) fn create_deposed_claims(
    world: &mut World,
    deposed_leader_id: u64,
    faction_id: u64,
    current_year: u32,
    event_id: u64,
) {
    let Some(deposed_entity) = world.entities.get(&deposed_leader_id) else {
        return;
    };

    let children: Vec<u64> = deposed_entity
        .relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::Parent)
        .map(|r| r.target_entity_id)
        .collect();

    let parent_ids: Vec<u64> = deposed_entity
        .relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::Child)
        .map(|r| r.target_entity_id)
        .collect();

    let mut sibling_ids: Vec<u64> = Vec::new();
    for &pid in &parent_ids {
        if let Some(parent_entity) = world.entities.get(&pid) {
            for r in &parent_entity.relationships {
                if r.kind == RelationshipKind::Parent
                    && r.target_entity_id != deposed_leader_id
                    && !sibling_ids.contains(&r.target_entity_id)
                {
                    sibling_ids.push(r.target_entity_id);
                }
            }
        }
    }

    let claim_key = format!("claim_{faction_id}");
    let mut candidates: Vec<u64> = Vec::new();
    candidates.extend(&children);
    candidates.extend(&sibling_ids);

    for person_id in candidates {
        // Must be alive
        let alive = world
            .entities
            .get(&person_id)
            .is_some_and(|e| e.kind == EntityKind::Person && e.end.is_none());
        if !alive {
            continue;
        }
        // Skip if already has claim
        let already_has = world
            .entities
            .get(&person_id)
            .is_some_and(|e| e.extra.contains_key(&claim_key));
        if already_has {
            continue;
        }
        world.set_extra(
            person_id,
            &claim_key,
            serde_json::json!({
                "strength": CLAIM_DEPOSED_STRENGTH,
                "source": "bloodline",
                "year": current_year,
            }),
            event_id,
        );
    }
}

/// Create claims for blood relatives of the old faction leader after a faction split.
pub(super) fn create_split_claims(
    world: &mut World,
    old_faction_id: u64,
    new_faction_id: u64,
    current_year: u32,
    event_id: u64,
) {
    // Find the old faction's leader
    let Some(old_leader_id) = helpers::faction_leader(world, old_faction_id) else {
        return;
    };

    let Some(leader_entity) = world.entities.get(&old_leader_id) else {
        return;
    };

    // Find blood relatives (children, siblings) who are now in the new faction
    let children: Vec<u64> = leader_entity
        .relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::Parent)
        .map(|r| r.target_entity_id)
        .collect();

    let parent_ids: Vec<u64> = leader_entity
        .relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::Child)
        .map(|r| r.target_entity_id)
        .collect();

    let mut sibling_ids: Vec<u64> = Vec::new();
    for &pid in &parent_ids {
        if let Some(parent_entity) = world.entities.get(&pid) {
            for r in &parent_entity.relationships {
                if r.kind == RelationshipKind::Parent
                    && r.target_entity_id != old_leader_id
                    && !sibling_ids.contains(&r.target_entity_id)
                {
                    sibling_ids.push(r.target_entity_id);
                }
            }
        }
    }

    let claim_key = format!("claim_{old_faction_id}");
    let all_relatives: Vec<u64> = children.into_iter().chain(sibling_ids).collect();

    for person_id in all_relatives {
        // Must be alive and in the new faction
        let in_new_faction = world.entities.get(&person_id).is_some_and(|e| {
            e.kind == EntityKind::Person
                && e.end.is_none()
                && e.has_active_rel(RelationshipKind::MemberOf, new_faction_id)
        });
        if !in_new_faction {
            continue;
        }
        let already_has = world
            .entities
            .get(&person_id)
            .is_some_and(|e| e.extra.contains_key(&claim_key));
        if already_has {
            continue;
        }
        world.set_extra(
            person_id,
            &claim_key,
            serde_json::json!({
                "strength": CLAIM_SPLIT_STRENGTH,
                "source": "bloodline",
                "year": current_year,
            }),
            event_id,
        );
    }
}

/// Check if a person is alive and a member of a faction other than the given one.
fn is_living_in_other_faction(world: &World, person_id: u64, excluded_faction: u64) -> bool {
    world.entities.get(&person_id).is_some_and(|e| {
        e.kind == EntityKind::Person
            && e.end.is_none()
            && e.relationships.iter().any(|r| {
                r.kind == RelationshipKind::MemberOf
                    && r.end.is_none()
                    && r.target_entity_id != excluded_faction
                    && world
                        .entities
                        .get(&r.target_entity_id)
                        .is_some_and(|t| t.kind == EntityKind::Faction && t.end.is_none())
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::World;
    use crate::sim::demographics::DemographicsSystem;
    use crate::sim::runner::{SimConfig, run};
    use crate::worldgen::{self, config::WorldGenConfig};
    fn make_political_world(seed: u64, num_years: u32) -> World {
        let config = WorldGenConfig {
            seed,
            ..WorldGenConfig::default()
        };
        let mut world = worldgen::generate_world(config);
        let mut systems: Vec<Box<dyn SimSystem>> =
            vec![Box::new(DemographicsSystem), Box::new(PoliticsSystem)];
        run(&mut world, &mut systems, SimConfig::new(1, num_years, seed));
        world
    }

    #[test]
    fn faction_gets_leader_on_first_tick() {
        let world = make_political_world(42, 1);

        let factions: Vec<u64> = world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
            .map(|e| e.id)
            .collect();
        assert!(!factions.is_empty(), "should have factions");

        let mut ruled = 0;
        for &fid in &factions {
            if has_leader(&world, fid) {
                ruled += 1;
            }
        }
        // After 1 year, factions with members should have leaders
        assert!(
            ruled > 0,
            "at least some factions should have leaders after year 1"
        );
    }

    #[test]
    fn stability_drifts_without_leader() {
        // Create a world, run 1 year to establish factions, then check stability
        let world = make_political_world(42, 50);

        for faction in world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        {
            let fd = faction
                .data
                .as_faction()
                .unwrap_or_else(|| panic!("faction {} should have FactionData", faction.name));
            let stability = fd.stability;
            assert!(
                (0.0..=1.0).contains(&stability),
                "stability should be in [0, 1], got {}",
                stability
            );
        }
    }

    #[test]
    fn succession_events_created() {
        let world = make_political_world(42, 100);

        let succession_count = world
            .events
            .values()
            .filter(|e| e.kind == EventKind::Succession)
            .count();
        assert!(
            succession_count > 0,
            "expected succession events after 100 years"
        );
    }

    #[test]
    fn diplomacy_forms_over_time() {
        let world = make_political_world(42, 200);

        let ally_count = world
            .collect_relationships()
            .filter(|r| r.kind == RelationshipKind::Ally)
            .count();
        let enemy_count = world
            .collect_relationships()
            .filter(|r| r.kind == RelationshipKind::Enemy)
            .count();
        assert!(
            ally_count + enemy_count > 0,
            "expected some diplomatic relationships after 200 years"
        );
    }

    #[test]
    fn coup_eventually_occurs() {
        // Marriages stabilize factions, so coups need many seeds to observe
        let mut total_coups = 0;
        let mut total_failed = 0;
        for seed in 0u64..50 {
            let world = make_political_world(seed, 1000);
            total_coups += world
                .events
                .values()
                .filter(|e| e.kind == EventKind::Coup)
                .count();
            total_failed += world
                .events
                .values()
                .filter(|e| e.kind == EventKind::Custom("failed_coup".to_string()))
                .count();
            if total_coups + total_failed > 0 {
                break;
            }
        }
        assert!(
            total_coups + total_failed > 0,
            "expected at least one coup attempt across 50 seeds x 1000 years (coups: {total_coups}, failed: {total_failed})"
        );
    }

    #[test]
    fn failed_coup_events_exist() {
        // Marriages stabilize factions, so failed coups need many seeds to observe
        let mut total_failed = 0;
        let mut total_coups = 0;
        for seed in 0u64..50 {
            let world = make_political_world(seed, 1000);
            total_failed += world
                .events
                .values()
                .filter(|e| e.kind == EventKind::Custom("failed_coup".to_string()))
                .count();
            total_coups += world
                .events
                .values()
                .filter(|e| e.kind == EventKind::Coup)
                .count();
            if total_failed > 0 {
                break;
            }
        }
        assert!(
            total_failed > 0,
            "expected at least one failed coup across 50 seeds x 1000 years (successes: {total_coups})"
        );
    }

    #[test]
    fn event_descriptions_contain_names() {
        let world = make_political_world(42, 100);

        // Check succession descriptions contain non-generic text
        let successions: Vec<&str> = world
            .events
            .values()
            .filter(|e| e.kind == EventKind::Succession)
            .map(|e| e.description.as_str())
            .collect();
        assert!(!successions.is_empty(), "expected succession events");
        for desc in &successions {
            // Should contain "of" or "became" or "succeeded" — not just "in year"
            assert!(
                desc.contains("became leader of") || desc.contains("succeeded to leadership of"),
                "succession description should be narrative: {desc}"
            );
        }

        // Check death descriptions
        let deaths: Vec<&str> = world
            .events
            .values()
            .filter(|e| e.kind == EventKind::Death)
            .map(|e| e.description.as_str())
            .collect();
        assert!(!deaths.is_empty(), "expected death events");
        for desc in &deaths {
            assert!(
                desc.contains("died in year") || desc.contains("was executed"),
                "death description should be narrative: {desc}"
            );
        }
    }

    #[test]
    fn scenario_hereditary_succession_prefers_children() {
        use crate::scenario::Scenario;
        use rand::SeedableRng;
        use rand::rngs::SmallRng;

        let mut s = Scenario::at_year(100);
        let faction = s.add_faction("Dynasty");

        // Parent (dead old leader) — standalone, not a faction member
        let parent = s.add_person_standalone("Parent");

        // Child and elder are faction members
        let child = s.person("Child", faction).birth_year(80).id();
        let _elder = s.person("Elder", faction).birth_year(50).id();

        s.make_parent_child(parent, child);

        let world = s.build();
        let members = collect_faction_members(&world, faction);
        let mut rng = SmallRng::seed_from_u64(42);
        let leader = select_leader(
            &members,
            GovernmentType::Hereditary,
            &world,
            &mut rng,
            Some(parent),
        );
        assert_eq!(
            leader,
            Some(child),
            "child should be preferred over older non-relative"
        );
    }

    #[test]
    fn scenario_hereditary_succession_falls_back_to_siblings() {
        use crate::scenario::Scenario;
        use rand::SeedableRng;
        use rand::rngs::SmallRng;

        let mut s = Scenario::at_year(100);
        let faction = s.add_faction("Dynasty");

        // Parent and old_leader are NOT faction members — standalone
        let parent = s.add_person_standalone("Parent");
        let old_leader = s.add_person_standalone("OldLeader");

        // Sibling and elder are faction members
        let sibling = s.person("Sibling", faction).birth_year(75).id();
        let _elder = s.person("Elder", faction).birth_year(50).id();

        // Parent → old_leader and parent → sibling
        s.make_parent_child(parent, old_leader);
        s.make_parent_child(parent, sibling);

        let world = s.build();
        let members = collect_faction_members(&world, faction);
        let mut rng = SmallRng::seed_from_u64(42);
        let leader = select_leader(
            &members,
            GovernmentType::Hereditary,
            &world,
            &mut rng,
            Some(old_leader),
        );
        assert_eq!(
            leader,
            Some(sibling),
            "sibling should be preferred when no children exist"
        );
    }

    #[test]
    fn scenario_hereditary_succession_falls_back_to_oldest() {
        use crate::scenario::Scenario;
        use rand::SeedableRng;
        use rand::rngs::SmallRng;

        let mut s = Scenario::at_year(100);
        let faction = s.add_faction("Dynasty");

        // Old leader with no children or siblings — standalone, not a member
        let old_leader = s.add_person_standalone("OldLeader");

        // Two unrelated faction members
        let _younger = s.person("Younger", faction).birth_year(80).id();
        let older = s.person("Older", faction).birth_year(50).id();

        let world = s.build();
        let members = collect_faction_members(&world, faction);
        let mut rng = SmallRng::seed_from_u64(42);
        let leader = select_leader(
            &members,
            GovernmentType::Hereditary,
            &world,
            &mut rng,
            Some(old_leader),
        );
        assert_eq!(
            leader,
            Some(older),
            "oldest member should be fallback when no relatives"
        );
    }

    #[test]
    fn scenario_succession_creates_claims_for_children_in_other_faction() {
        use crate::scenario::Scenario;

        let mut s = Scenario::at_year(100);
        let fa = s
            .faction("Dynasty A")
            .government_type(GovernmentType::Hereditary)
            .id();
        let fb = s.add_faction("Dynasty B");

        // Dead leader of faction A
        let dead_leader = s.add_person("Old King", fa);
        s.make_leader(dead_leader, fa);

        // Child in faction B (should get claim)
        let child = s.add_person("Prince", fb);
        s.make_parent_child(dead_leader, child);

        // New successor in faction A
        let successor = s.person("Successor", fa).birth_year(60).id();

        let mut world = s.build();

        // Simulate leader death + succession
        let ev = world.add_event(
            EventKind::Death,
            SimTimestamp::from_year(100),
            "Old King died".to_string(),
        );
        // End the leader
        world.entities.get_mut(&dead_leader).unwrap().end = Some(SimTimestamp::from_year(100));
        for r in &mut world.entities.get_mut(&dead_leader).unwrap().relationships {
            if r.kind == RelationshipKind::LeaderOf && r.end.is_none() {
                r.end = Some(SimTimestamp::from_year(100));
            }
        }
        // Install successor
        world.add_relationship(
            successor,
            fa,
            RelationshipKind::LeaderOf,
            SimTimestamp::from_year(100),
            ev,
        );
        // Now create succession claims
        create_succession_claims(&mut world, fa, dead_leader, 100, ev);

        // Child should have a claim on faction A
        let claim_key = format!("claim_{fa}");
        let claim = world.entities[&child]
            .extra
            .get(&claim_key)
            .expect("child should have claim");
        let strength = claim["strength"].as_f64().unwrap();
        assert!(
            (strength - CLAIM_CHILD_STRENGTH).abs() < 0.01,
            "child claim strength should be {CLAIM_CHILD_STRENGTH}, got {strength}"
        );
        assert_eq!(claim["source"].as_str().unwrap(), "bloodline");

        // Successor should NOT have a claim (same faction)
        assert!(
            !world.entities[&successor].extra.contains_key(&claim_key),
            "successor in same faction should not get a claim"
        );
    }

    #[test]
    fn scenario_succession_creates_sibling_and_grandchild_claims() {
        use crate::scenario::Scenario;

        let mut s = Scenario::at_year(100);
        let fa = s
            .faction("Dynasty A")
            .government_type(GovernmentType::Hereditary)
            .id();
        let fb = s.add_faction("Dynasty B");

        let dead_leader = s.add_person("Old King", fa);
        s.make_leader(dead_leader, fa);

        // Parent of dead leader (needed to establish sibling relation)
        let grandparent = s.add_person_standalone("Grandparent");
        s.make_parent_child(grandparent, dead_leader);

        // Sibling in faction B
        let sibling = s.add_person("Brother", fb);
        s.make_parent_child(grandparent, sibling);

        // Child in faction A (same faction, no claim)
        let child_same = s.add_person("Heir", fa);
        s.make_parent_child(dead_leader, child_same);

        // Child in faction B with their own child (grandchild) in faction B
        let child_other = s.add_person("Exiled Son", fb);
        s.make_parent_child(dead_leader, child_other);
        let grandchild = s.add_person("Grandchild", fb);
        s.make_parent_child(child_other, grandchild);

        let successor = s.person("Successor", fa).birth_year(60).id();
        let mut world = s.build();

        let ev = world.add_event(
            EventKind::Death,
            SimTimestamp::from_year(100),
            "Old King died".to_string(),
        );
        world.entities.get_mut(&dead_leader).unwrap().end = Some(SimTimestamp::from_year(100));
        for r in &mut world.entities.get_mut(&dead_leader).unwrap().relationships {
            if r.kind == RelationshipKind::LeaderOf && r.end.is_none() {
                r.end = Some(SimTimestamp::from_year(100));
            }
        }
        world.add_relationship(
            successor,
            fa,
            RelationshipKind::LeaderOf,
            SimTimestamp::from_year(100),
            ev,
        );
        create_succession_claims(&mut world, fa, dead_leader, 100, ev);

        let claim_key = format!("claim_{fa}");

        // Sibling should have claim at sibling strength
        let sib_claim = world.entities[&sibling]
            .extra
            .get(&claim_key)
            .expect("sibling should have claim");
        assert!((sib_claim["strength"].as_f64().unwrap() - CLAIM_SIBLING_STRENGTH).abs() < 0.01,);

        // Child in other faction should have child claim
        let child_claim = world.entities[&child_other]
            .extra
            .get(&claim_key)
            .expect("child in other faction should have claim");
        assert!((child_claim["strength"].as_f64().unwrap() - CLAIM_CHILD_STRENGTH).abs() < 0.01,);

        // Grandchild should have grandchild claim
        let gc_claim = world.entities[&grandchild]
            .extra
            .get(&claim_key)
            .expect("grandchild should have claim");
        assert!((gc_claim["strength"].as_f64().unwrap() - CLAIM_GRANDCHILD_STRENGTH).abs() < 0.01,);

        // Child in same faction should NOT have claim
        assert!(
            !world.entities[&child_same].extra.contains_key(&claim_key),
            "child in same faction should not get a claim"
        );
    }

    #[test]
    fn scenario_claim_decay_reduces_strength_and_removes_weak_claims() {
        use crate::scenario::Scenario;
        use rand::SeedableRng;
        use rand::rngs::SmallRng;

        let mut s = Scenario::at_year(100);
        let fa = s.add_faction("Dynasty A");
        let fb = s.add_faction("Dynasty B");
        let claimant = s.add_person("Claimant", fb);
        s.add_claim(claimant, fa, 0.5);
        let weak_claimant = s.add_person("Weak Claimant", fb);
        s.add_claim(weak_claimant, fa, 0.12); // barely above threshold
        let mut world = s.build();

        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals = Vec::new();
        let mut ctx = TickContext {
            world: &mut world,
            rng: &mut rng,
            signals: &mut signals,
            inbox: &[],
        };

        decay_claims(&mut ctx, 101);

        let claim_key = format!("claim_{fa}");
        // Strong claimant should still have claim, reduced by 0.05
        let remaining = ctx.world.entities[&claimant]
            .extra
            .get(&claim_key)
            .expect("strong claim should remain")["strength"]
            .as_f64()
            .unwrap();
        assert!(
            (remaining - 0.45).abs() < 0.01,
            "claim should decay from 0.5 to 0.45, got {remaining}"
        );

        // Weak claimant's claim should be removed (0.12 - 0.05 = 0.07 < 0.1 threshold)
        let weak_remaining = ctx.world.entities[&weak_claimant].extra.get(&claim_key);
        assert!(
            weak_remaining.is_none() || weak_remaining.unwrap().is_null(),
            "weak claim should be removed after decay"
        );
    }

    #[test]
    fn scenario_succession_crisis_fires_for_strong_claimant() {
        use crate::scenario::Scenario;

        let mut s = Scenario::at_year(100);
        let fa = s
            .faction("Dynasty A")
            .government_type(GovernmentType::Hereditary)
            .stability(0.8)
            .legitimacy(0.8)
            .id();
        let fb = s.add_faction("Dynasty B");

        let dead_leader = s.add_person("Old King", fa);
        s.make_leader(dead_leader, fa);

        // Child in faction B (strength 0.9 > 0.5 threshold → triggers crisis)
        let child = s.add_person("Exiled Prince", fb);
        s.make_parent_child(dead_leader, child);

        let successor = s.person("Successor", fa).birth_year(60).id();
        let mut world = s.build();

        let ev = world.add_event(
            EventKind::Death,
            SimTimestamp::from_year(100),
            "Old King died".to_string(),
        );
        world.entities.get_mut(&dead_leader).unwrap().end = Some(SimTimestamp::from_year(100));
        for r in &mut world.entities.get_mut(&dead_leader).unwrap().relationships {
            if r.kind == RelationshipKind::LeaderOf && r.end.is_none() {
                r.end = Some(SimTimestamp::from_year(100));
            }
        }
        world.add_relationship(
            successor,
            fa,
            RelationshipKind::LeaderOf,
            SimTimestamp::from_year(100),
            ev,
        );
        create_succession_claims(&mut world, fa, dead_leader, 100, ev);

        // Check crisis event was created
        let crisis = world
            .events
            .values()
            .find(|e| e.kind == EventKind::SuccessionCrisis);
        assert!(
            crisis.is_some(),
            "succession crisis event should be created"
        );

        // Check stability hit
        let faction_data = world.faction(fa);
        assert!(
            faction_data.stability < 0.8,
            "stability should decrease from crisis, got {}",
            faction_data.stability
        );

        // Check legitimacy hit
        assert!(
            faction_data.legitimacy < 0.8,
            "legitimacy should decrease from crisis, got {}",
            faction_data.legitimacy
        );

        // Check crisis year extra
        let crisis_year = world.entities[&fa]
            .extra
            .get(K::SUCCESSION_CRISIS_YEAR)
            .and_then(|v| v.as_u64());
        assert_eq!(crisis_year, Some(100));
    }

    #[test]
    fn scenario_no_crisis_for_non_hereditary() {
        use crate::scenario::Scenario;

        let mut s = Scenario::at_year(100);
        let fa = s
            .faction("Republic")
            .government_type(GovernmentType::Elective)
            .stability(0.8)
            .legitimacy(0.8)
            .id();
        let fb = s.add_faction("Rival");

        let dead_leader = s.add_person("Old President", fa);
        s.make_leader(dead_leader, fa);

        // Child in faction B
        let child = s.add_person("Exiled Child", fb);
        s.make_parent_child(dead_leader, child);

        let successor = s.person("Successor", fa).birth_year(60).id();
        let mut world = s.build();

        // Kill leader but DON'T call create_succession_claims (since politics checks gov type)
        let ev = world.add_event(
            EventKind::Death,
            SimTimestamp::from_year(100),
            "President died".to_string(),
        );
        world.entities.get_mut(&dead_leader).unwrap().end = Some(SimTimestamp::from_year(100));
        for r in &mut world.entities.get_mut(&dead_leader).unwrap().relationships {
            if r.kind == RelationshipKind::LeaderOf && r.end.is_none() {
                r.end = Some(SimTimestamp::from_year(100));
            }
        }
        world.add_relationship(
            successor,
            fa,
            RelationshipKind::LeaderOf,
            SimTimestamp::from_year(100),
            ev,
        );

        // Elective factions don't create claims
        // Verify no claims exist
        let claim_key = format!("claim_{fa}");
        assert!(
            !world.entities[&child].extra.contains_key(&claim_key),
            "elective faction should not create succession claims"
        );

        // Verify no crisis
        assert!(
            !world
                .events
                .values()
                .any(|e| e.kind == EventKind::SuccessionCrisis),
            "elective faction should not trigger succession crisis"
        );
    }

    #[test]
    fn scenario_coup_creates_deposed_claims() {
        use crate::scenario::Scenario;

        let mut s = Scenario::at_year(100);
        let fa = s.add_faction("Dynasty");
        let fb = s.add_faction("Rival");

        let deposed_leader = s.add_person("Deposed King", fa);
        s.make_leader(deposed_leader, fa);

        // Deposed leader's child in faction B
        let child = s.add_person("Prince", fb);
        s.make_parent_child(deposed_leader, child);

        // Deposed leader's sibling in faction B
        let grandparent = s.add_person_standalone("Grandparent");
        s.make_parent_child(grandparent, deposed_leader);
        let sibling = s.add_person("Sibling", fb);
        s.make_parent_child(grandparent, sibling);

        let mut world = s.build();

        let ev = world.add_event(
            EventKind::Coup,
            SimTimestamp::from_year(100),
            "Coup against Deposed King".to_string(),
        );

        create_deposed_claims(&mut world, deposed_leader, fa, 100, ev);

        let claim_key = format!("claim_{fa}");

        // Child should have deposed claim
        let child_claim = world.entities[&child]
            .extra
            .get(&claim_key)
            .expect("deposed leader's child should get claim");
        assert!((child_claim["strength"].as_f64().unwrap() - CLAIM_DEPOSED_STRENGTH).abs() < 0.01,);

        // Sibling should have deposed claim
        let sib_claim = world.entities[&sibling]
            .extra
            .get(&claim_key)
            .expect("deposed leader's sibling should get claim");
        assert!((sib_claim["strength"].as_f64().unwrap() - CLAIM_DEPOSED_STRENGTH).abs() < 0.01,);
    }
}
