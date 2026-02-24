mod coups;
mod diplomacy;

use rand::Rng;
use rand::RngCore;

use super::context::TickContext;
use super::extra_keys as K;
use super::faction_names::generate_unique_faction_name;
use super::signal::{Signal, SignalKind};
use super::system::{SimSystem, TickFrequency};
use crate::model::traits::{Trait, has_trait};
use crate::model::{
    EntityData, EntityKind, EventKind, FactionData, ParticipantRole, RelationshipKind, Role,
    SiegeOutcome, SimTimestamp, World,
};
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
const STABILITY_MIN_TARGET: f64 = 0.15;
const STABILITY_MAX_TARGET: f64 = 0.95;
const STABILITY_NOISE_RANGE: f64 = 0.05;
const STABILITY_DRIFT_RATE: f64 = 0.12;
const STABILITY_LEADERLESS_PRESSURE: f64 = 0.04;

// --- Succession ---
const SUCCESSION_STABILITY_HIT: f64 = -0.12;
const SUCCESSION_PRESTIGE_SOFTENING: f64 = 0.5;

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
                    apply_happiness_delta(
                        ctx.world,
                        *attacker_id,
                        WAR_STARTED_HAPPINESS_HIT,
                        signal.event_id,
                    );
                    apply_happiness_delta(
                        ctx.world,
                        *defender_id,
                        WAR_STARTED_HAPPINESS_HIT,
                        signal.event_id,
                    );
                }
                SignalKind::WarEnded {
                    winner_id,
                    loser_id,
                    decisive,
                    ..
                } => {
                    if *decisive {
                        apply_happiness_delta(
                            ctx.world,
                            *winner_id,
                            WAR_WON_DECISIVE_HAPPINESS,
                            signal.event_id,
                        );
                        apply_stability_delta(
                            ctx.world,
                            *winner_id,
                            WAR_WON_DECISIVE_STABILITY,
                            signal.event_id,
                        );
                        apply_happiness_delta(
                            ctx.world,
                            *loser_id,
                            WAR_LOST_DECISIVE_HAPPINESS,
                            signal.event_id,
                        );
                        apply_stability_delta(
                            ctx.world,
                            *loser_id,
                            WAR_LOST_DECISIVE_STABILITY,
                            signal.event_id,
                        );
                    } else {
                        apply_happiness_delta(
                            ctx.world,
                            *winner_id,
                            WAR_WON_INDECISIVE_HAPPINESS,
                            signal.event_id,
                        );
                        apply_stability_delta(
                            ctx.world,
                            *winner_id,
                            WAR_WON_INDECISIVE_STABILITY,
                            signal.event_id,
                        );
                        apply_happiness_delta(
                            ctx.world,
                            *loser_id,
                            WAR_LOST_INDECISIVE_HAPPINESS,
                            signal.event_id,
                        );
                        apply_stability_delta(
                            ctx.world,
                            *loser_id,
                            WAR_LOST_INDECISIVE_STABILITY,
                            signal.event_id,
                        );
                    }
                }
                SignalKind::SettlementCaptured { old_faction_id, .. } => {
                    apply_stability_delta(
                        ctx.world,
                        *old_faction_id,
                        SETTLEMENT_CAPTURED_STABILITY,
                        signal.event_id,
                    );
                }
                SignalKind::RefugeesArrived {
                    settlement_id,
                    count,
                    ..
                } => {
                    // Large refugee influx (>20% of destination pop) reduces faction happiness
                    let dest_pop = ctx
                        .world
                        .entities
                        .get(settlement_id)
                        .and_then(|e| e.data.as_settlement())
                        .map(|s| s.population)
                        .unwrap_or(0);
                    if dest_pop > 0 && (*count as f64 / dest_pop as f64) > REFUGEE_THRESHOLD_RATIO {
                        // Find the faction this settlement belongs to
                        if let Some(faction_id) =
                            ctx.world.entities.get(settlement_id).and_then(|e| {
                                e.relationships
                                    .iter()
                                    .find(|r| {
                                        r.kind == RelationshipKind::MemberOf && r.end.is_none()
                                    })
                                    .map(|r| r.target_entity_id)
                            })
                        {
                            apply_happiness_delta(
                                ctx.world,
                                faction_id,
                                REFUGEE_HAPPINESS_HIT,
                                signal.event_id,
                            );
                        }
                    }
                }
                SignalKind::CulturalRebellion { faction_id, .. } => {
                    apply_stability_delta(
                        ctx.world,
                        *faction_id,
                        CULTURAL_REBELLION_STABILITY,
                        signal.event_id,
                    );
                    apply_happiness_delta(
                        ctx.world,
                        *faction_id,
                        CULTURAL_REBELLION_HAPPINESS,
                        signal.event_id,
                    );
                }
                SignalKind::PlagueStarted { settlement_id, .. } => {
                    // Plague destabilizes the faction that owns this settlement
                    if let Some(faction_id) = ctx.world.entities.get(settlement_id).and_then(|e| {
                        e.relationships
                            .iter()
                            .find(|r| r.kind == RelationshipKind::MemberOf && r.end.is_none())
                            .map(|r| r.target_entity_id)
                    }) {
                        apply_stability_delta(
                            ctx.world,
                            faction_id,
                            PLAGUE_STABILITY_HIT,
                            signal.event_id,
                        );
                        apply_happiness_delta(
                            ctx.world,
                            faction_id,
                            PLAGUE_HAPPINESS_HIT,
                            signal.event_id,
                        );
                    }
                }
                SignalKind::SiegeStarted {
                    defender_faction_id,
                    ..
                } => {
                    apply_happiness_delta(
                        ctx.world,
                        *defender_faction_id,
                        SIEGE_STARTED_HAPPINESS,
                        signal.event_id,
                    );
                    apply_stability_delta(
                        ctx.world,
                        *defender_faction_id,
                        SIEGE_STARTED_STABILITY,
                        signal.event_id,
                    );
                }
                SignalKind::SiegeEnded {
                    defender_faction_id,
                    outcome,
                    ..
                } => {
                    if *outcome == SiegeOutcome::Lifted {
                        apply_happiness_delta(
                            ctx.world,
                            *defender_faction_id,
                            SIEGE_LIFTED_HAPPINESS,
                            signal.event_id,
                        );
                    }
                }
                SignalKind::LeaderVacancy {
                    faction_id,
                    previous_leader_id,
                } => {
                    // Verify this is actually a faction (not a settlement from legacy signals)
                    let is_faction = ctx
                        .world
                        .entities
                        .get(faction_id)
                        .is_some_and(|e| e.kind == EntityKind::Faction && e.end.is_none());
                    if !is_faction {
                        continue;
                    }

                    // Skip if a leader was already assigned this tick (e.g. by fill_leader_vacancies)
                    if has_leader(ctx.world, *faction_id) {
                        continue;
                    }

                    let gov_type = get_government_type(ctx.world, *faction_id);
                    let faction_name = helpers::entity_name(ctx.world, *faction_id);
                    let members = collect_faction_members(ctx.world, *faction_id);
                    if let Some(leader_id) = select_leader(
                        &members,
                        &gov_type,
                        ctx.world,
                        ctx.rng,
                        Some(*previous_leader_id),
                    ) {
                        let leader_name = helpers::entity_name(ctx.world, leader_id);
                        let ev = ctx.world.add_caused_event(
                            EventKind::Succession,
                            time,
                            format!("{leader_name} succeeded to leadership of {faction_name} in year {current_year}"),
                            signal.event_id,
                        );
                        ctx.world
                            .add_event_participant(ev, leader_id, ParticipantRole::Subject);
                        ctx.world
                            .add_event_participant(ev, *faction_id, ParticipantRole::Object);
                        ctx.world.add_relationship(
                            leader_id,
                            *faction_id,
                            RelationshipKind::LeaderOf,
                            time,
                            ev,
                        );

                        // Succession causes a stability hit
                        apply_succession_stability_hit(ctx.world, *faction_id, ev);
                    }
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
                    // Disaster reduces happiness and stability of the owning faction
                    if let Some(faction_id) = ctx.world.entities.get(settlement_id).and_then(|e| {
                        e.relationships
                            .iter()
                            .find(|r| r.kind == RelationshipKind::MemberOf && r.end.is_none())
                            .map(|r| r.target_entity_id)
                    }) {
                        let happiness_hit =
                            DISASTER_HAPPINESS_BASE - severity * DISASTER_HAPPINESS_SEVERITY_WEIGHT;
                        apply_happiness_delta(
                            ctx.world,
                            faction_id,
                            happiness_hit,
                            signal.event_id,
                        );
                        apply_stability_delta(
                            ctx.world,
                            faction_id,
                            DISASTER_STABILITY_HIT,
                            signal.event_id,
                        );
                    }
                }
                SignalKind::DisasterEnded { settlement_id, .. } => {
                    // Relief: small happiness recovery
                    if let Some(faction_id) = ctx.world.entities.get(settlement_id).and_then(|e| {
                        e.relationships
                            .iter()
                            .find(|r| r.kind == RelationshipKind::MemberOf && r.end.is_none())
                            .map(|r| r.target_entity_id)
                    }) {
                        apply_happiness_delta(
                            ctx.world,
                            faction_id,
                            DISASTER_ENDED_HAPPINESS_RECOVERY,
                            signal.event_id,
                        );
                    }
                }
                _ => {}
            }
        }
    }
}

// --- 4a: Fill leader vacancies ---

fn fill_leader_vacancies(ctx: &mut TickContext, time: SimTimestamp, current_year: u32) {
    // Collect faction info
    struct FactionInfo {
        id: u64,
        government_type: String,
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
                .map(|f| f.government_type.as_str())
                .unwrap_or("chieftain")
                .to_string(),
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
        let previous_leader_id = find_previous_leader(ctx.world, faction.id, &members);

        if let Some(leader_id) = select_leader(
            &members,
            &faction.government_type,
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
            let has_enemies = e
                .relationships
                .iter()
                .any(|r| r.kind == RelationshipKind::Enemy && r.end.is_none());
            let has_allies = e
                .relationships
                .iter()
                .any(|r| r.kind == RelationshipKind::Ally && r.end.is_none());
            HappinessInfo {
                faction_id: e.id,
                old_happiness,
                stability,
                has_leader: false, // filled below
                has_enemies,
                has_allies,
                avg_prosperity: DEFAULT_PROSPERITY, // filled below
                avg_cultural_tension: 0.0,          // filled below
            }
        })
        .collect();

    // Compute leader presence and avg prosperity per faction
    let factions: Vec<HappinessInfo> = factions
        .into_iter()
        .map(|mut f| {
            f.has_leader = has_leader(ctx.world, f.faction_id);

            // Compute average prosperity and cultural tension of faction's settlements
            let mut prosperity_sum = 0.0;
            let mut tension_sum = 0.0;
            let mut settlement_count = 0u32;
            for e in ctx.world.entities.values() {
                if e.kind == EntityKind::Settlement
                    && e.end.is_none()
                    && e.relationships.iter().any(|r| {
                        r.kind == RelationshipKind::MemberOf
                            && r.target_entity_id == f.faction_id
                            && r.end.is_none()
                    })
                {
                    if let Some(sd) = e.data.as_settlement() {
                        prosperity_sum += sd.prosperity;
                        tension_sum += sd.cultural_tension;
                    } else {
                        prosperity_sum += DEFAULT_PROSPERITY;
                    }
                    settlement_count += 1;
                }
            }
            f.avg_prosperity = if settlement_count > 0 {
                prosperity_sum / settlement_count as f64
            } else {
                DEFAULT_PROSPERITY
            };
            f.avg_cultural_tension = if settlement_count > 0 {
                tension_sum / settlement_count as f64
            } else {
                0.0
            };
            f
        })
        .collect();

    // Compute total building happiness bonus per faction (from temples)
    let mut faction_building_happiness: std::collections::HashMap<u64, f64> =
        std::collections::HashMap::new();
    for e in ctx.world.entities.values() {
        if e.kind == EntityKind::Settlement
            && e.end.is_none()
            && let Some(faction_id) = e
                .relationships
                .iter()
                .find(|r| r.kind == RelationshipKind::MemberOf && r.end.is_none())
                .map(|r| r.target_entity_id)
        {
            let bonus = e.extra_f64_or(K::BUILDING_HAPPINESS_BONUS, 0.0);
            *faction_building_happiness.entry(faction_id).or_default() += bonus;
        }
    }

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
    }

    let factions: Vec<FactionStability> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        .map(|e| {
            let fd = e.data.as_faction();
            FactionStability {
                id: e.id,
                old_stability: fd.map(|f| f.stability).unwrap_or(STABILITY_DEFAULT),
                happiness: fd.map(|f| f.happiness).unwrap_or(STABILITY_DEFAULT),
                legitimacy: fd.map(|f| f.legitimacy).unwrap_or(STABILITY_DEFAULT),
                has_leader: false,         // filled below
                avg_cultural_tension: 0.0, // filled below
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
                    && e.relationships.iter().any(|r| {
                        r.kind == RelationshipKind::MemberOf
                            && r.target_entity_id == f.id
                            && r.end.is_none()
                    })
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
        let target = (base_target + leader_adj + tension_adj)
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

fn check_faction_splits(ctx: &mut TickContext, time: SimTimestamp, current_year: u32) {
    // Collect faction sentiment data for split checks
    struct FactionSentiment {
        stability: f64,
        happiness: f64,
        government_type: String,
        prestige: f64,
    }

    let faction_sentiments: std::collections::BTreeMap<u64, FactionSentiment> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        .map(|e| {
            let fd = e.data.as_faction();
            (
                e.id,
                FactionSentiment {
                    stability: fd.map(|f| f.stability).unwrap_or(STABILITY_DEFAULT),
                    happiness: fd.map(|f| f.happiness).unwrap_or(STABILITY_DEFAULT),
                    government_type: fd
                        .map(|f| f.government_type.clone())
                        .unwrap_or_else(|| "chieftain".to_string()),
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
    struct SplitPlan {
        settlement_id: u64,
        old_faction_id: u64,
        old_happiness: f64,
        old_gov_type: String,
        parent_prestige: f64,
    }

    let gov_types = ["hereditary", "elective", "chieftain"];

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
                old_gov_type: sentiment.government_type.clone(),
                parent_prestige: sentiment.prestige,
            });
            // Decrease count so we don't split a faction down to 0 settlements
            if let Some(c) = faction_settlement_count.get_mut(&sf.faction_id) {
                *c = c.saturating_sub(1);
            }
        }
    }

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
            split.old_gov_type.clone()
        } else {
            gov_types[ctx.rng.random_range(0..gov_types.len())].to_string()
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
                    && e.relationships.iter().any(|r| {
                        r.kind == RelationshipKind::LocatedIn
                            && r.target_entity_id == split.settlement_id
                            && r.end.is_none()
                    })
                    && e.relationships.iter().any(|r| {
                        r.kind == RelationshipKind::MemberOf
                            && r.target_entity_id == split.old_faction_id
                            && r.end.is_none()
                    })
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
    }

    // --- Faction dissolution: end factions with 0 settlements ---
    let empty_factions: Vec<u64> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        .filter(|e| {
            !ctx.world.entities.values().any(|s| {
                s.kind == EntityKind::Settlement
                    && s.end.is_none()
                    && s.relationships.iter().any(|r| {
                        r.kind == RelationshipKind::MemberOf
                            && r.target_entity_id == e.id
                            && r.end.is_none()
                    })
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
                && e.relationships.iter().any(|r| {
                    r.kind == RelationshipKind::MemberOf
                        && r.target_entity_id == faction_id
                        && r.end.is_none()
                })
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
    government_type: &str,
    world: &World,
    rng: &mut dyn RngCore,
    previous_leader_id: Option<u64>,
) -> Option<u64> {
    if members.is_empty() {
        return None;
    }

    match government_type {
        "hereditary" => {
            // Try bloodline succession if we have a previous leader
            if let Some(prev_id) = previous_leader_id {
                let member_ids: std::collections::HashSet<u64> =
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
        "elective" => {
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
        _ => {
            // Chieftain: warrior preferred, else oldest
            let warriors: Vec<&MemberInfo> =
                members.iter().filter(|m| m.role == Role::Warrior).collect();
            if !warriors.is_empty() {
                // Oldest warrior
                warriors.iter().min_by_key(|m| m.birth_year).map(|m| m.id)
            } else {
                members.iter().min_by_key(|m| m.birth_year).map(|m| m.id)
            }
        }
    }
}

fn has_leader(world: &World, faction_id: u64) -> bool {
    world.entities.values().any(|e| {
        e.kind == EntityKind::Person
            && e.end.is_none()
            && e.relationships.iter().any(|r| {
                r.kind == RelationshipKind::LeaderOf
                    && r.target_entity_id == faction_id
                    && r.end.is_none()
            })
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

pub(super) fn apply_stability_delta(world: &mut World, faction_id: u64, delta: f64, event_id: u64) {
    let (old, new) = {
        let Some(entity) = world.entities.get_mut(&faction_id) else {
            return;
        };
        let Some(fd) = entity.data.as_faction_mut() else {
            return;
        };
        let old = fd.stability;
        fd.stability = (old + delta).clamp(0.0, 1.0);
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
fn find_previous_leader(world: &World, faction_id: u64, _members: &[MemberInfo]) -> Option<u64> {
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

fn get_government_type(world: &World, faction_id: u64) -> String {
    world
        .entities
        .get(&faction_id)
        .and_then(|e| e.data.as_faction())
        .map(|f| f.government_type.clone())
        .unwrap_or_else(|| "chieftain".to_string())
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
        let mut world = worldgen::generate_world(&config);
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
                .expect(&format!("faction {} should have FactionData", faction.name));
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
        let leader = select_leader(&members, "hereditary", &world, &mut rng, Some(parent));
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
        let leader = select_leader(&members, "hereditary", &world, &mut rng, Some(old_leader));
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
        let leader = select_leader(&members, "hereditary", &world, &mut rng, Some(old_leader));
        assert_eq!(
            leader,
            Some(older),
            "oldest member should be fallback when no relatives"
        );
    }
}
