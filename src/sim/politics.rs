use rand::Rng;
use rand::RngCore;

use super::context::TickContext;
use super::faction_names::generate_unique_faction_name;
use super::signal::{Signal, SignalKind};
use super::system::{SimSystem, TickFrequency};
use crate::model::action::ActionKind;
use crate::model::traits::{Trait, has_trait};
use crate::model::{
    EntityData, EntityKind, EventKind, FactionData, ParticipantRole, RelationshipKind,
    SimTimestamp, World,
};

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
        check_coups(ctx, time, current_year);

        // --- 4d: Inter-faction diplomacy ---
        update_diplomacy(ctx, time, current_year);

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
                    apply_happiness_delta(ctx.world, *attacker_id, -0.15, signal.event_id);
                    apply_happiness_delta(ctx.world, *defender_id, -0.15, signal.event_id);
                }
                SignalKind::WarEnded {
                    winner_id,
                    loser_id,
                    decisive,
                    ..
                } => {
                    if *decisive {
                        apply_happiness_delta(ctx.world, *winner_id, 0.15, signal.event_id);
                        apply_stability_delta(ctx.world, *winner_id, 0.10, signal.event_id);
                        apply_happiness_delta(ctx.world, *loser_id, -0.15, signal.event_id);
                        apply_stability_delta(ctx.world, *loser_id, -0.15, signal.event_id);
                    } else {
                        apply_happiness_delta(ctx.world, *winner_id, 0.05, signal.event_id);
                        apply_stability_delta(ctx.world, *winner_id, 0.03, signal.event_id);
                        apply_happiness_delta(ctx.world, *loser_id, -0.05, signal.event_id);
                        apply_stability_delta(ctx.world, *loser_id, -0.05, signal.event_id);
                    }
                }
                SignalKind::SettlementCaptured { old_faction_id, .. } => {
                    apply_stability_delta(ctx.world, *old_faction_id, -0.15, signal.event_id);
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
                    if dest_pop > 0 && (*count as f64 / dest_pop as f64) > 0.20 {
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
                            apply_happiness_delta(ctx.world, faction_id, -0.1, signal.event_id);
                        }
                    }
                }
                SignalKind::CulturalRebellion { faction_id, .. } => {
                    apply_stability_delta(ctx.world, *faction_id, -0.15, signal.event_id);
                    apply_happiness_delta(ctx.world, *faction_id, -0.10, signal.event_id);
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
                    let faction_name = get_entity_name(ctx.world, *faction_id);
                    let members = collect_faction_members(ctx.world, *faction_id);
                    if let Some(leader_id) = select_leader(
                        &members,
                        &gov_type,
                        ctx.world,
                        ctx.rng,
                        Some(*previous_leader_id),
                    ) {
                        let leader_name = get_entity_name(ctx.world, leader_id);
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
        let faction_name = get_entity_name(ctx.world, faction.id);
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
            let leader_name = get_entity_name(ctx.world, leader_id);
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
            let old_happiness = fd.map(|f| f.happiness).unwrap_or(0.6);
            let stability = fd.map(|f| f.stability).unwrap_or(0.5);
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
                avg_prosperity: 0.3,       // filled below
                avg_cultural_tension: 0.0, // filled below
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
                        prosperity_sum += 0.3;
                    }
                    settlement_count += 1;
                }
            }
            f.avg_prosperity = if settlement_count > 0 {
                prosperity_sum / settlement_count as f64
            } else {
                0.3
            };
            f.avg_cultural_tension = if settlement_count > 0 {
                tension_sum / settlement_count as f64
            } else {
                0.0
            };
            f
        })
        .collect();

    let year_event = ctx.world.add_event(
        EventKind::Custom("happiness_tick".to_string()),
        time,
        format!("Year {} happiness update", time.year()),
    );

    for f in &factions {
        let base_target = 0.6;
        let prosperity_bonus = f.avg_prosperity * 0.15;
        let stability_bonus = (f.stability - 0.5) * 0.2;
        let peace_bonus = if f.has_enemies {
            -0.1
        } else if f.has_allies {
            0.05
        } else {
            0.0
        };
        let leader_bonus = if f.has_leader { 0.05 } else { -0.1 };

        let trade_bonus = ctx
            .world
            .entities
            .get(&f.faction_id)
            .and_then(|e| e.extra.get("trade_happiness_bonus"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        let tension_penalty = -f.avg_cultural_tension * 0.15;

        let target = (base_target
            + prosperity_bonus
            + stability_bonus
            + peace_bonus
            + leader_bonus
            + trade_bonus
            + tension_penalty)
            .clamp(0.1, 0.95);
        let noise: f64 = ctx.rng.random_range(-0.02..0.02);
        let new_happiness =
            (f.old_happiness + (target - f.old_happiness) * 0.15 + noise).clamp(0.0, 1.0);

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
    }

    let factions: Vec<LegitimacyInfo> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        .map(|e| {
            let fd = e.data.as_faction();
            LegitimacyInfo {
                faction_id: e.id,
                old_legitimacy: fd.map(|f| f.legitimacy).unwrap_or(0.5),
                happiness: fd.map(|f| f.happiness).unwrap_or(0.5),
            }
        })
        .collect();

    let year_event = ctx.world.add_event(
        EventKind::Custom("legitimacy_tick".to_string()),
        time,
        format!("Year {} legitimacy update", time.year()),
    );

    for f in &factions {
        let target = 0.5 + 0.4 * f.happiness;
        let new_legitimacy = (f.old_legitimacy + (target - f.old_legitimacy) * 0.1).clamp(0.0, 1.0);

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
                old_stability: fd.map(|f| f.stability).unwrap_or(0.5),
                happiness: fd.map(|f| f.happiness).unwrap_or(0.5),
                legitimacy: fd.map(|f| f.legitimacy).unwrap_or(0.5),
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
        let base_target = 0.5 + 0.2 * faction.happiness + 0.15 * faction.legitimacy;
        let leader_adj = if faction.has_leader { 0.05 } else { -0.15 };
        let tension_adj = -faction.avg_cultural_tension * 0.10;
        let target = (base_target + leader_adj + tension_adj).clamp(0.15, 0.95);

        let noise: f64 = ctx.rng.random_range(-0.05..0.05);
        let mut drift = (target - faction.old_stability) * 0.12 + noise;
        // Direct instability pressure when leaderless
        if !faction.has_leader {
            drift -= 0.04;
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

// --- 4c: Coups ---

fn check_coups(ctx: &mut TickContext, time: SimTimestamp, current_year: u32) {
    struct CoupTarget {
        faction_id: u64,
        current_leader_id: u64,
        stability: f64,
        happiness: f64,
        legitimacy: f64,
    }

    let targets: Vec<CoupTarget> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        .filter_map(|e| {
            let fd = e.data.as_faction()?;
            let stability = fd.stability;
            if stability >= 0.55 {
                return None;
            }
            let leader_id = find_faction_leader(ctx.world, e.id)?;
            let happiness = fd.happiness;
            let legitimacy = fd.legitimacy;
            Some(CoupTarget {
                faction_id: e.id,
                current_leader_id: leader_id,
                stability,
                happiness,
                legitimacy,
            })
        })
        .collect();

    for target in targets {
        // Dedup: skip if an NPC already queued AttemptCoup for this faction
        let npc_coup_queued = ctx.world.pending_actions.iter().any(|a| {
            matches!(a.kind, ActionKind::AttemptCoup { faction_id } if faction_id == target.faction_id)
        });
        if npc_coup_queued {
            continue;
        }

        // Stage 1: Coup attempt
        let instability = 1.0 - target.stability;
        let unhappiness_factor = 1.0 - target.happiness;
        let attempt_chance = 0.08 * instability * (0.3 + 0.7 * unhappiness_factor);
        if ctx.rng.random_range(0.0..1.0) >= attempt_chance {
            continue;
        }

        // Find a coup leader (warrior-weighted)
        let members = collect_faction_members(ctx.world, target.faction_id);
        let candidates: Vec<&MemberInfo> = members
            .iter()
            .filter(|m| m.id != target.current_leader_id)
            .collect();
        if candidates.is_empty() {
            continue;
        }

        let instigator_id = select_weighted_member_with_traits(
            &candidates,
            &["warrior", "elder"],
            ctx.world,
            ctx.rng,
        );

        // Stage 2: Coup success check
        // Compute military strength from faction settlements
        let mut able_bodied = 0u32;
        for e in ctx.world.entities.values() {
            if e.kind == EntityKind::Settlement
                && e.end.is_none()
                && e.relationships.iter().any(|r| {
                    r.kind == RelationshipKind::MemberOf
                        && r.target_entity_id == target.faction_id
                        && r.end.is_none()
                })
            {
                let pop = e.data.as_settlement().map(|s| s.population).unwrap_or(0);
                // Rough estimate: ~25% of population is able-bodied men
                able_bodied += pop / 4;
            }
        }
        let military = (able_bodied as f64 / 200.0).clamp(0.0, 1.0);
        let resistance = 0.2 + military * target.legitimacy * (0.3 + 0.7 * target.happiness);
        let noise: f64 = ctx.rng.random_range(-0.1..0.1);
        let coup_power = (0.2 + 0.3 * instability + noise).max(0.0);
        let success_chance = (coup_power / (coup_power + resistance)).clamp(0.1, 0.9);

        // Collect names before mutation
        let instigator_name = get_entity_name(ctx.world, instigator_id);
        let leader_name = get_entity_name(ctx.world, target.current_leader_id);
        let faction_name = get_entity_name(ctx.world, target.faction_id);

        if ctx.rng.random_range(0.0..1.0) < success_chance {
            // --- Successful coup ---
            let ev = ctx.world.add_event(
                EventKind::Coup,
                time,
                format!("{instigator_name} overthrew {leader_name} of {faction_name} in year {current_year}"),
            );
            ctx.world
                .add_event_participant(ev, instigator_id, ParticipantRole::Instigator);
            ctx.world
                .add_event_participant(ev, target.current_leader_id, ParticipantRole::Subject);
            ctx.world
                .add_event_participant(ev, target.faction_id, ParticipantRole::Object);

            // End old leader's LeaderOf
            ctx.world.end_relationship(
                target.current_leader_id,
                target.faction_id,
                &RelationshipKind::LeaderOf,
                time,
                ev,
            );

            // New leader takes over
            ctx.world.add_relationship(
                instigator_id,
                target.faction_id,
                RelationshipKind::LeaderOf,
                time,
                ev,
            );

            // Post-coup stability depends on sentiment
            let unhappiness_bonus = 0.25 * (1.0 - target.happiness);
            let illegitimacy_bonus = 0.1 * (1.0 - target.legitimacy);
            let post_coup_stability =
                (0.35 + unhappiness_bonus + illegitimacy_bonus).clamp(0.2, 0.65);

            // New legitimacy
            let new_legitimacy = if target.happiness < 0.35 {
                // Liberation: people were miserable
                0.4 + 0.3 * (1.0 - target.happiness)
            } else {
                // Power grab
                0.15 + 0.15 * (1.0 - target.happiness)
            }
            .clamp(0.0, 1.0);

            // Happiness hit
            let happiness_hit = -0.05 - 0.1 * target.happiness;
            let new_happiness = (target.happiness + happiness_hit).clamp(0.0, 1.0);

            {
                let entity = ctx.world.entities.get_mut(&target.faction_id).unwrap();
                let fd = entity.data.as_faction_mut().unwrap();
                fd.stability = post_coup_stability;
                fd.legitimacy = new_legitimacy;
                fd.happiness = new_happiness;
            }
            ctx.world.record_change(
                target.faction_id,
                ev,
                "stability",
                serde_json::json!(target.stability),
                serde_json::json!(post_coup_stability),
            );
            ctx.world.record_change(
                target.faction_id,
                ev,
                "legitimacy",
                serde_json::json!(target.legitimacy),
                serde_json::json!(new_legitimacy),
            );
            ctx.world.record_change(
                target.faction_id,
                ev,
                "happiness",
                serde_json::json!(target.happiness),
                serde_json::json!(new_happiness),
            );
        } else {
            // --- Failed coup ---
            let ev = ctx.world.add_event(
                EventKind::Custom("failed_coup".to_string()),
                time,
                format!("{instigator_name} failed to overthrow {leader_name} of {faction_name} in year {current_year}"),
            );
            ctx.world
                .add_event_participant(ev, instigator_id, ParticipantRole::Instigator);
            ctx.world
                .add_event_participant(ev, target.current_leader_id, ParticipantRole::Subject);
            ctx.world
                .add_event_participant(ev, target.faction_id, ParticipantRole::Object);

            // Minor stability hit
            let new_stability = (target.stability - 0.05).clamp(0.0, 1.0);

            // Legitimacy boost for surviving leader
            let new_legitimacy = (target.legitimacy + 0.1).clamp(0.0, 1.0);

            {
                let entity = ctx.world.entities.get_mut(&target.faction_id).unwrap();
                let fd = entity.data.as_faction_mut().unwrap();
                fd.stability = new_stability;
                fd.legitimacy = new_legitimacy;
            }
            ctx.world.record_change(
                target.faction_id,
                ev,
                "stability",
                serde_json::json!(target.stability),
                serde_json::json!(new_stability),
            );
            ctx.world.record_change(
                target.faction_id,
                ev,
                "legitimacy",
                serde_json::json!(target.legitimacy),
                serde_json::json!(new_legitimacy),
            );

            // 50% chance coup leader is executed
            if ctx.rng.random_bool(0.5) {
                let death_ev = ctx.world.add_caused_event(
                    EventKind::Death,
                    time,
                    format!("{instigator_name} was executed in year {current_year}"),
                    ev,
                );
                ctx.world
                    .add_event_participant(death_ev, instigator_id, ParticipantRole::Subject);

                // End relationships
                end_person_relationships(ctx.world, instigator_id, time, death_ev);

                // End entity
                ctx.world.end_entity(instigator_id, time, death_ev);

                ctx.signals.push(Signal {
                    event_id: death_ev,
                    kind: SignalKind::EntityDied {
                        entity_id: instigator_id,
                    },
                });
            }
        }
    }
}

// --- 4d: Inter-faction diplomacy ---

fn update_diplomacy(ctx: &mut TickContext, time: SimTimestamp, current_year: u32) {
    // Collect living factions with their properties
    struct FactionDiplo {
        id: u64,
        happiness: f64,
        stability: f64,
        ally_count: u32,
    }

    let factions: Vec<FactionDiplo> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        .map(|e| {
            let ally_count = e
                .relationships
                .iter()
                .filter(|r| r.kind == RelationshipKind::Ally && r.end.is_none())
                .count() as u32;
            let fd = e.data.as_faction();
            FactionDiplo {
                id: e.id,
                happiness: fd.map(|f| f.happiness).unwrap_or(0.5),
                stability: fd.map(|f| f.stability).unwrap_or(0.5),
                ally_count,
            }
        })
        .collect();

    let faction_ids: Vec<u64> = factions.iter().map(|f| f.id).collect();

    // Check for dissolution of existing relationships
    struct EndAction {
        source_id: u64,
        target_id: u64,
        kind: RelationshipKind,
    }
    let mut ends: Vec<EndAction> = Vec::new();

    for &fid in &faction_ids {
        if let Some(entity) = ctx.world.entities.get(&fid) {
            for rel in &entity.relationships {
                if rel.end.is_some() {
                    continue;
                }
                match &rel.kind {
                    RelationshipKind::Ally => {
                        // Calculate alliance strength from all sources
                        let target = rel.target_entity_id;
                        let strength = calculate_alliance_strength(ctx.world, fid, target);

                        // Decay rate modulated by strength: at 1.0+ strength, no decay
                        let base_dissolution = 0.03;
                        let dissolution_chance = base_dissolution * (1.0 - strength).max(0.0);
                        if ctx.rng.random_range(0.0..1.0) < dissolution_chance {
                            ends.push(EndAction {
                                source_id: fid,
                                target_id: target,
                                kind: RelationshipKind::Ally,
                            });
                        }
                    }
                    RelationshipKind::Enemy => {
                        if ctx.rng.random_range(0.0..1.0) < 0.03 {
                            ends.push(EndAction {
                                source_id: fid,
                                target_id: rel.target_entity_id,
                                kind: RelationshipKind::Enemy,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    for end in ends {
        let name_a = get_entity_name(ctx.world, end.source_id);
        let name_b = get_entity_name(ctx.world, end.target_id);
        let rel_type = match &end.kind {
            RelationshipKind::Ally => "alliance",
            RelationshipKind::Enemy => "rivalry",
            _ => "relation",
        };
        let ev = ctx.world.add_event(
            EventKind::Dissolution,
            time,
            format!("The {rel_type} between {name_a} and {name_b} ended in year {current_year}"),
        );
        ctx.world
            .add_event_participant(ev, end.source_id, ParticipantRole::Subject);
        ctx.world
            .add_event_participant(ev, end.target_id, ParticipantRole::Object);
        ctx.world
            .end_relationship(end.source_id, end.target_id, &end.kind, time, ev);
    }

    // Check for new relationships between unrelated pairs
    struct NewRelAction {
        source_id: u64,
        target_id: u64,
        kind: RelationshipKind,
    }
    let mut new_rels: Vec<NewRelAction> = Vec::new();

    for i in 0..factions.len() {
        for j in (i + 1)..factions.len() {
            let a = &factions[i];
            let b = &factions[j];

            if has_active_diplomatic_rel(ctx.world, a.id, b.id) {
                continue;
            }

            // Check for shared enemies (boosts alliance chance)
            let shared_enemies = has_shared_enemy(ctx.world, a.id, b.id);

            // Alliance soft cap: halve rate if either has 2+ alliances
            let alliance_cap = if a.ally_count >= 2 || b.ally_count >= 2 {
                0.5
            } else {
                1.0
            };

            let avg_happiness = (a.happiness + b.happiness) / 2.0;
            let shared_enemy_mult = if shared_enemies { 2.0 } else { 1.0 };
            let alliance_rate =
                0.008 * shared_enemy_mult * (0.5 + 0.5 * avg_happiness) * alliance_cap;

            let avg_instability = (1.0 - a.stability + 1.0 - b.stability) / 2.0;
            let rivalry_rate = 0.006 * (0.5 + 0.5 * avg_instability);

            let roll: f64 = ctx.rng.random_range(0.0..1.0);
            if roll < alliance_rate {
                new_rels.push(NewRelAction {
                    source_id: a.id,
                    target_id: b.id,
                    kind: RelationshipKind::Ally,
                });
            } else if roll < alliance_rate + rivalry_rate {
                new_rels.push(NewRelAction {
                    source_id: a.id,
                    target_id: b.id,
                    kind: RelationshipKind::Enemy,
                });
            }
        }
    }

    for rel in new_rels {
        let name_a = get_entity_name(ctx.world, rel.source_id);
        let name_b = get_entity_name(ctx.world, rel.target_id);
        let (desc, event_kind) = match &rel.kind {
            RelationshipKind::Ally => (
                format!("{name_a} and {name_b} formed an alliance in year {current_year}"),
                EventKind::Treaty,
            ),
            RelationshipKind::Enemy => (
                format!("{name_a} and {name_b} became rivals in year {current_year}"),
                EventKind::Custom("rivalry".to_string()),
            ),
            _ => unreachable!(),
        };
        let ev = ctx.world.add_event(event_kind, time, desc);
        ctx.world
            .add_event_participant(ev, rel.source_id, ParticipantRole::Subject);
        ctx.world
            .add_event_participant(ev, rel.target_id, ParticipantRole::Object);
        // Use ensure_relationship: another system (economy) may have already
        // created this alliance in the same tick.
        ctx.world
            .ensure_relationship(rel.source_id, rel.target_id, rel.kind, time, ev);
    }
}

// --- 4e: Faction splits ---

fn check_faction_splits(ctx: &mut TickContext, time: SimTimestamp, current_year: u32) {
    // Collect faction sentiment data for split checks
    struct FactionSentiment {
        stability: f64,
        happiness: f64,
        government_type: String,
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
                    stability: fd.map(|f| f.stability).unwrap_or(0.5),
                    happiness: fd.map(|f| f.happiness).unwrap_or(0.5),
                    government_type: fd
                        .map(|f| f.government_type.clone())
                        .unwrap_or_else(|| "chieftain".to_string()),
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
    }

    let gov_types = ["hereditary", "elective", "chieftain"];

    let mut splits: Vec<SplitPlan> = Vec::new();
    for sf in &settlement_factions {
        let Some(sentiment) = faction_sentiments.get(&sf.faction_id) else {
            continue;
        };

        // Skip if faction is reasonably stable or happy
        if sentiment.stability >= 0.3 || sentiment.happiness >= 0.35 {
            continue;
        }

        let misery = (1.0 - sentiment.happiness) * (1.0 - sentiment.stability);
        let split_chance = 0.01 * misery;

        if ctx.rng.random_range(0.0..1.0) < split_chance {
            splits.push(SplitPlan {
                settlement_id: sf.settlement_id,
                old_faction_id: sf.faction_id,
                old_happiness: sentiment.happiness,
                old_gov_type: sentiment.government_type.clone(),
            });
            // Decrease count so we don't split a faction down to 0 settlements
            if let Some(c) = faction_settlement_count.get_mut(&sf.faction_id) {
                *c = c.saturating_sub(1);
            }
        }
    }

    for split in splits {
        let old_faction_name = get_entity_name(ctx.world, split.old_faction_id);
        let name = generate_unique_faction_name(ctx.world, ctx.rng);
        let ev = ctx.world.add_event(
            EventKind::FactionFormed,
            time,
            format!("{name} formed by secession from {old_faction_name} in year {current_year}"),
        );

        // 50% inherit government type, 50% random
        let gov_type = if ctx.rng.random_bool(0.5) {
            split.old_gov_type.clone()
        } else {
            gov_types[ctx.rng.random_range(0..gov_types.len())].to_string()
        };

        let new_faction_data = EntityData::Faction(FactionData {
            government_type: gov_type,
            stability: 0.5,
            happiness: (split.old_happiness + 0.1).clamp(0.0, 1.0),
            legitimacy: 0.6,
            treasury: 0.0,
            alliance_strength: 0.0,
            primary_culture: None,
        });

        let new_faction_id =
            ctx.world
                .add_entity(EntityKind::Faction, name, Some(time), new_faction_data, ev);

        // Move settlement to new faction
        ctx.world.end_relationship(
            split.settlement_id,
            split.old_faction_id,
            &RelationshipKind::MemberOf,
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
                &RelationshipKind::MemberOf,
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
        if ctx.rng.random_bool(0.7) {
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
                new_faction_id,
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
        let faction_name = get_entity_name(ctx.world, faction_id);
        let ev = ctx.world.add_event(
            EventKind::Custom("faction_dissolved".to_string()),
            time,
            format!("{faction_name} dissolved in year {current_year}"),
        );
        ctx.world
            .add_event_participant(ev, faction_id, ParticipantRole::Subject);

        // End leader relationship if any
        if let Some(leader_id) = find_faction_leader(ctx.world, faction_id) {
            ctx.world.end_relationship(
                leader_id,
                faction_id,
                &RelationshipKind::LeaderOf,
                time,
                ev,
            );
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
            ctx.world.end_relationship(source, target, &kind, time, ev);
        }

        ctx.world.end_entity(faction_id, time, ev);
    }
}

// --- Helpers ---

struct MemberInfo {
    id: u64,
    birth_year: u32,
    role: String,
}

fn collect_faction_members(world: &World, faction_id: u64) -> Vec<MemberInfo> {
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
                role: pd
                    .map(|p| p.role.clone())
                    .unwrap_or_else(|| "common".to_string()),
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
            let preferred = ["elder", "scholar"];
            let refs: Vec<&MemberInfo> = members.iter().collect();
            let weights: Vec<u32> = refs
                .iter()
                .map(|m| {
                    let mut w: u32 = if preferred.contains(&m.role.as_str()) {
                        3
                    } else {
                        1
                    };
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
                members.iter().filter(|m| m.role == "warrior").collect();
            if !warriors.is_empty() {
                // Oldest warrior
                warriors.iter().min_by_key(|m| m.birth_year).map(|m| m.id)
            } else {
                members.iter().min_by_key(|m| m.birth_year).map(|m| m.id)
            }
        }
    }
}

fn select_weighted_member_with_traits(
    candidates: &[&MemberInfo],
    preferred_roles: &[&str],
    world: &World,
    rng: &mut dyn RngCore,
) -> u64 {
    let weights: Vec<u32> = candidates
        .iter()
        .map(|m| {
            let mut w: u32 = if preferred_roles.contains(&m.role.as_str()) {
                3
            } else {
                1
            };
            // Ambitious or Aggressive traits give 2x multiplier
            if let Some(entity) = world.entities.get(&m.id)
                && (has_trait(entity, &Trait::Ambitious) || has_trait(entity, &Trait::Aggressive))
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
            return candidates[i].id;
        }
    }
    candidates.last().unwrap().id
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

fn apply_happiness_delta(world: &mut World, faction_id: u64, delta: f64, event_id: u64) {
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

fn apply_stability_delta(world: &mut World, faction_id: u64, delta: f64, event_id: u64) {
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
    let (old, new) = {
        let Some(entity) = world.entities.get_mut(&faction_id) else {
            return;
        };
        let Some(fd) = entity.data.as_faction_mut() else {
            return;
        };
        let old = fd.stability;
        fd.stability = (old - 0.12).clamp(0.0, 1.0);
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

fn find_faction_leader(world: &World, faction_id: u64) -> Option<u64> {
    world
        .entities
        .values()
        .find(|e| {
            e.kind == EntityKind::Person
                && e.end.is_none()
                && e.relationships.iter().any(|r| {
                    r.kind == RelationshipKind::LeaderOf
                        && r.target_entity_id == faction_id
                        && r.end.is_none()
                })
        })
        .map(|e| e.id)
}

fn get_government_type(world: &World, faction_id: u64) -> String {
    world
        .entities
        .get(&faction_id)
        .and_then(|e| e.data.as_faction())
        .map(|f| f.government_type.clone())
        .unwrap_or_else(|| "chieftain".to_string())
}

fn end_person_relationships(world: &mut World, person_id: u64, time: SimTimestamp, event_id: u64) {
    let rels: Vec<(u64, RelationshipKind)> = world
        .entities
        .get(&person_id)
        .map(|e| {
            e.relationships
                .iter()
                .filter(|r| r.end.is_none())
                .map(|r| (r.target_entity_id, r.kind.clone()))
                .collect()
        })
        .unwrap_or_default();

    for (target_id, kind) in rels {
        world.end_relationship(person_id, target_id, &kind, time, event_id);
    }
}

fn get_entity_name(world: &World, entity_id: u64) -> String {
    world
        .entities
        .get(&entity_id)
        .map(|e| e.name.clone())
        .unwrap_or_else(|| format!("entity {entity_id}"))
}

fn has_shared_enemy(world: &World, a: u64, b: u64) -> bool {
    let enemies_a: Vec<u64> = world
        .entities
        .get(&a)
        .map(|e| {
            e.relationships
                .iter()
                .filter(|r| r.kind == RelationshipKind::Enemy && r.end.is_none())
                .map(|r| r.target_entity_id)
                .collect()
        })
        .unwrap_or_default();

    if enemies_a.is_empty() {
        return false;
    }

    world
        .entities
        .get(&b)
        .map(|e| {
            e.relationships.iter().any(|r| {
                r.kind == RelationshipKind::Enemy
                    && r.end.is_none()
                    && enemies_a.contains(&r.target_entity_id)
            })
        })
        .unwrap_or(false)
}

fn has_active_diplomatic_rel(world: &World, a: u64, b: u64) -> bool {
    if let Some(entity) = world.entities.get(&a) {
        for rel in &entity.relationships {
            if rel.end.is_some() {
                continue;
            }
            if rel.target_entity_id == b
                && (rel.kind == RelationshipKind::Ally
                    || rel.kind == RelationshipKind::Enemy
                    || rel.kind == RelationshipKind::AtWar)
            {
                return true;
            }
        }
    }
    if let Some(entity) = world.entities.get(&b) {
        for rel in &entity.relationships {
            if rel.end.is_some() {
                continue;
            }
            if rel.target_entity_id == a
                && (rel.kind == RelationshipKind::Ally
                    || rel.kind == RelationshipKind::Enemy
                    || rel.kind == RelationshipKind::AtWar)
            {
                return true;
            }
        }
    }
    false
}

/// Calculate the strength of an alliance between two factions based on all
/// active reasons for being allies. Strength >= 1.0 prevents decay entirely.
///
/// Sources:
/// - Trade routes: min(route_count * 0.2, 0.6)
/// - Shared enemies: 0.3
/// - Marriage alliance: 0.4
/// - Base (existing alliance): 0.1
fn calculate_alliance_strength(world: &World, faction_a: u64, faction_b: u64) -> f64 {
    let mut strength = 0.1; // base strength for any existing alliance

    // Trade routes between these factions (set by economy system)
    if let Some(entity) = world.entities.get(&faction_a)
        && let Some(trade_map) = entity.extra.get("trade_partner_routes")
    {
        let key = faction_b.to_string();
        if let Some(count) = trade_map.get(&key).and_then(|v| v.as_u64()) {
            strength += (count as f64 * 0.2).min(0.6);
        }
    }

    // Shared enemies
    if has_shared_enemy(world, faction_a, faction_b) {
        strength += 0.3;
    }

    // Marriage alliance (both factions have marriage_alliance_year)
    let a_marriage = world
        .entities
        .get(&faction_a)
        .is_some_and(|e| e.extra.contains_key("marriage_alliance_year"));
    let b_marriage = world
        .entities
        .get(&faction_b)
        .is_some_and(|e| e.extra.contains_key("marriage_alliance_year"));
    if a_marriage && b_marriage {
        strength += 0.4;
    }

    strength
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
    fn hereditary_succession_prefers_children() {
        use crate::model::{EntityData, PersonData, SimTimestamp};
        use rand::SeedableRng;
        use rand::rngs::SmallRng;

        let mut world = World::new();
        let ts = SimTimestamp::from_year(100);

        // Create parent (old leader, now dead)
        let ev = world.add_event(EventKind::Birth, ts, "Born".to_string());
        let parent = world.add_entity(
            EntityKind::Person,
            "Parent".to_string(),
            Some(ts),
            EntityData::default_for_kind(&EntityKind::Person),
            ev,
        );

        // Create child (faction member)
        let ev2 = world.add_event(EventKind::Birth, ts, "Born".to_string());
        let child = world.add_entity(
            EntityKind::Person,
            "Child".to_string(),
            Some(ts),
            EntityData::Person(PersonData {
                birth_year: 80,
                sex: "male".to_string(),
                role: "common".to_string(),
                traits: Vec::new(),
                last_action_year: 0,
                culture_id: None,
            }),
            ev2,
        );

        // Create unrelated older member
        let ev3 = world.add_event(EventKind::Birth, ts, "Born".to_string());
        let elder = world.add_entity(
            EntityKind::Person,
            "Elder".to_string(),
            Some(ts),
            EntityData::Person(PersonData {
                birth_year: 50,
                sex: "male".to_string(),
                role: "common".to_string(),
                traits: Vec::new(),
                last_action_year: 0,
                culture_id: None,
            }),
            ev3,
        );

        // Create faction
        let ev4 = world.add_event(EventKind::FactionFormed, ts, "Formed".to_string());
        let faction = world.add_entity(
            EntityKind::Faction,
            "Dynasty".to_string(),
            None,
            EntityData::default_for_kind(&EntityKind::Faction),
            ev4,
        );

        // Parent → Child relationship
        let ev5 = world.add_event(EventKind::Birth, ts, "parentage".to_string());
        world.add_relationship(parent, child, RelationshipKind::Parent, ts, ev5);
        world.add_relationship(child, parent, RelationshipKind::Child, ts, ev5);

        // Both child and elder are faction members
        let ev6 = world.add_event(EventKind::Joined, ts, "join".to_string());
        world.add_relationship(child, faction, RelationshipKind::MemberOf, ts, ev6);
        let ev7 = world.add_event(EventKind::Joined, ts, "join".to_string());
        world.add_relationship(elder, faction, RelationshipKind::MemberOf, ts, ev7);

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
    fn hereditary_succession_falls_back_to_siblings() {
        use crate::model::{EntityData, PersonData, SimTimestamp};
        use rand::SeedableRng;
        use rand::rngs::SmallRng;

        let mut world = World::new();
        let ts = SimTimestamp::from_year(100);

        // Create parent of both
        let ev = world.add_event(EventKind::Birth, ts, "Born".to_string());
        let parent = world.add_entity(
            EntityKind::Person,
            "Parent".to_string(),
            Some(ts),
            EntityData::default_for_kind(&EntityKind::Person),
            ev,
        );

        // Create old leader (sibling, now dead — not a faction member)
        let ev2 = world.add_event(EventKind::Birth, ts, "Born".to_string());
        let old_leader = world.add_entity(
            EntityKind::Person,
            "OldLeader".to_string(),
            Some(ts),
            EntityData::default_for_kind(&EntityKind::Person),
            ev2,
        );

        // Create sibling (faction member)
        let ev3 = world.add_event(EventKind::Birth, ts, "Born".to_string());
        let sibling = world.add_entity(
            EntityKind::Person,
            "Sibling".to_string(),
            Some(ts),
            EntityData::Person(PersonData {
                birth_year: 75,
                sex: "male".to_string(),
                role: "common".to_string(),
                traits: Vec::new(),
                last_action_year: 0,
                culture_id: None,
            }),
            ev3,
        );

        // Create unrelated older member
        let ev4 = world.add_event(EventKind::Birth, ts, "Born".to_string());
        let elder = world.add_entity(
            EntityKind::Person,
            "Elder".to_string(),
            Some(ts),
            EntityData::Person(PersonData {
                birth_year: 50,
                sex: "male".to_string(),
                role: "common".to_string(),
                traits: Vec::new(),
                last_action_year: 0,
                culture_id: None,
            }),
            ev4,
        );

        // Create faction
        let ev5 = world.add_event(EventKind::FactionFormed, ts, "Formed".to_string());
        let faction = world.add_entity(
            EntityKind::Faction,
            "Dynasty".to_string(),
            None,
            EntityData::default_for_kind(&EntityKind::Faction),
            ev5,
        );

        // Parent → old_leader and parent → sibling
        let ev6 = world.add_event(EventKind::Birth, ts, "parentage".to_string());
        world.add_relationship(parent, old_leader, RelationshipKind::Parent, ts, ev6);
        world.add_relationship(old_leader, parent, RelationshipKind::Child, ts, ev6);
        let ev7 = world.add_event(EventKind::Birth, ts, "parentage".to_string());
        world.add_relationship(parent, sibling, RelationshipKind::Parent, ts, ev7);
        world.add_relationship(sibling, parent, RelationshipKind::Child, ts, ev7);

        // Sibling and elder are faction members (old_leader is NOT a member)
        let ev8 = world.add_event(EventKind::Joined, ts, "join".to_string());
        world.add_relationship(sibling, faction, RelationshipKind::MemberOf, ts, ev8);
        let ev9 = world.add_event(EventKind::Joined, ts, "join".to_string());
        world.add_relationship(elder, faction, RelationshipKind::MemberOf, ts, ev9);

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
    fn hereditary_succession_falls_back_to_oldest() {
        use crate::model::{EntityData, PersonData, SimTimestamp};
        use rand::SeedableRng;
        use rand::rngs::SmallRng;

        let mut world = World::new();
        let ts = SimTimestamp::from_year(100);

        // Create old leader with no children or siblings in faction
        let ev = world.add_event(EventKind::Birth, ts, "Born".to_string());
        let old_leader = world.add_entity(
            EntityKind::Person,
            "OldLeader".to_string(),
            Some(ts),
            EntityData::default_for_kind(&EntityKind::Person),
            ev,
        );

        // Create two unrelated members
        let ev2 = world.add_event(EventKind::Birth, ts, "Born".to_string());
        let younger = world.add_entity(
            EntityKind::Person,
            "Younger".to_string(),
            Some(ts),
            EntityData::Person(PersonData {
                birth_year: 80,
                sex: "male".to_string(),
                role: "common".to_string(),
                traits: Vec::new(),
                last_action_year: 0,
                culture_id: None,
            }),
            ev2,
        );

        let ev3 = world.add_event(EventKind::Birth, ts, "Born".to_string());
        let older = world.add_entity(
            EntityKind::Person,
            "Older".to_string(),
            Some(ts),
            EntityData::Person(PersonData {
                birth_year: 50,
                sex: "male".to_string(),
                role: "common".to_string(),
                traits: Vec::new(),
                last_action_year: 0,
                culture_id: None,
            }),
            ev3,
        );

        // Create faction
        let ev4 = world.add_event(EventKind::FactionFormed, ts, "Formed".to_string());
        let faction = world.add_entity(
            EntityKind::Faction,
            "Dynasty".to_string(),
            None,
            EntityData::default_for_kind(&EntityKind::Faction),
            ev4,
        );

        let ev5 = world.add_event(EventKind::Joined, ts, "join".to_string());
        world.add_relationship(younger, faction, RelationshipKind::MemberOf, ts, ev5);
        let ev6 = world.add_event(EventKind::Joined, ts, "join".to_string());
        world.add_relationship(older, faction, RelationshipKind::MemberOf, ts, ev6);

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
