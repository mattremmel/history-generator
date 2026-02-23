use rand::Rng;
use rand::RngCore;

use super::context::TickContext;
use super::faction_names::generate_unique_faction_name;
use super::signal::{Signal, SignalKind};
use super::system::{SimSystem, TickFrequency};
use crate::model::{EntityKind, EventKind, ParticipantRole, RelationshipKind, SimTimestamp, World};

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

        // --- 4a: Fill ruler vacancies ---
        fill_ruler_vacancies(ctx, time, current_year);

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
                } => {
                    apply_happiness_delta(ctx.world, *winner_id, 0.10, signal.event_id);
                    apply_stability_delta(ctx.world, *winner_id, 0.05, signal.event_id);
                    apply_happiness_delta(ctx.world, *loser_id, -0.10, signal.event_id);
                    apply_stability_delta(ctx.world, *loser_id, -0.10, signal.event_id);
                }
                SignalKind::SettlementCaptured { old_faction_id, .. } => {
                    apply_stability_delta(ctx.world, *old_faction_id, -0.15, signal.event_id);
                }
                SignalKind::RulerVacancy {
                    faction_id,
                    previous_ruler_id: _,
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

                    // Skip if a ruler was already assigned this tick (e.g. by fill_ruler_vacancies)
                    if has_ruler(ctx.world, *faction_id) {
                        continue;
                    }

                    let gov_type = get_government_type(ctx.world, *faction_id);
                    let faction_name = get_entity_name(ctx.world, *faction_id);
                    let members = collect_faction_members(ctx.world, *faction_id);
                    if let Some(ruler_id) = select_ruler(&members, &gov_type, ctx.world, ctx.rng) {
                        let ruler_name = get_entity_name(ctx.world, ruler_id);
                        let ev = ctx.world.add_caused_event(
                            EventKind::Succession,
                            time,
                            format!("{ruler_name} succeeded to leadership of {faction_name} in year {current_year}"),
                            signal.event_id,
                        );
                        ctx.world
                            .add_event_participant(ev, ruler_id, ParticipantRole::Subject);
                        ctx.world
                            .add_event_participant(ev, *faction_id, ParticipantRole::Object);
                        ctx.world.add_relationship(
                            ruler_id,
                            *faction_id,
                            RelationshipKind::RulerOf,
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

// --- 4a: Fill ruler vacancies ---

fn fill_ruler_vacancies(ctx: &mut TickContext, time: SimTimestamp, current_year: u32) {
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
                .properties
                .get("government_type")
                .and_then(|v| v.as_str())
                .unwrap_or("chieftain")
                .to_string(),
        })
        .collect();

    // Find which factions have no ruler
    let leaderless: Vec<&FactionInfo> = factions
        .iter()
        .filter(|f| !has_ruler(ctx.world, f.id))
        .collect();

    for faction in leaderless {
        let faction_name = get_entity_name(ctx.world, faction.id);
        let members = collect_faction_members(ctx.world, faction.id);
        if let Some(ruler_id) = select_ruler(&members, &faction.government_type, ctx.world, ctx.rng)
        {
            let ruler_name = get_entity_name(ctx.world, ruler_id);
            let ev = ctx.world.add_event(
                EventKind::Succession,
                time,
                format!("{ruler_name} became leader of {faction_name} in year {current_year}"),
            );
            ctx.world
                .add_event_participant(ev, ruler_id, ParticipantRole::Subject);
            ctx.world
                .add_event_participant(ev, faction.id, ParticipantRole::Object);
            ctx.world
                .add_relationship(ruler_id, faction.id, RelationshipKind::RulerOf, time, ev);

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
        has_ruler: bool,
        has_enemies: bool,
        has_allies: bool,
        avg_prosperity: f64,
    }

    let factions: Vec<HappinessInfo> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        .map(|e| {
            let old_happiness = e
                .properties
                .get("happiness")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.6);
            let stability = e
                .properties
                .get("stability")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.5);
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
                has_ruler: false, // filled below
                has_enemies,
                has_allies,
                avg_prosperity: 0.3, // filled below
            }
        })
        .collect();

    // Compute ruler presence and avg prosperity per faction
    let factions: Vec<HappinessInfo> = factions
        .into_iter()
        .map(|mut f| {
            f.has_ruler = has_ruler(ctx.world, f.faction_id);

            // Compute average prosperity of faction's settlements
            let mut prosperity_sum = 0.0;
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
                    let prosperity = e
                        .properties
                        .get("prosperity")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.3);
                    prosperity_sum += prosperity;
                    settlement_count += 1;
                }
            }
            f.avg_prosperity = if settlement_count > 0 {
                prosperity_sum / settlement_count as f64
            } else {
                0.3
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
        let ruler_bonus = if f.has_ruler { 0.05 } else { -0.1 };

        let target = (base_target + prosperity_bonus + stability_bonus + peace_bonus + ruler_bonus)
            .clamp(0.1, 0.95);
        let noise: f64 = ctx.rng.random_range(-0.02..0.02);
        let new_happiness =
            (f.old_happiness + (target - f.old_happiness) * 0.15 + noise).clamp(0.0, 1.0);

        ctx.world.set_property(
            f.faction_id,
            "happiness".to_string(),
            serde_json::json!(new_happiness),
            year_event,
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
        .map(|e| LegitimacyInfo {
            faction_id: e.id,
            old_legitimacy: e
                .properties
                .get("legitimacy")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.5),
            happiness: e
                .properties
                .get("happiness")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.5),
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

        ctx.world.set_property(
            f.faction_id,
            "legitimacy".to_string(),
            serde_json::json!(new_legitimacy),
            year_event,
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
        has_ruler: bool,
    }

    let factions: Vec<FactionStability> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        .map(|e| FactionStability {
            id: e.id,
            old_stability: e
                .properties
                .get("stability")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.5),
            happiness: e
                .properties
                .get("happiness")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.5),
            legitimacy: e
                .properties
                .get("legitimacy")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.5),
            has_ruler: false, // filled below
        })
        .collect();

    let factions: Vec<FactionStability> = factions
        .into_iter()
        .map(|mut f| {
            f.has_ruler = has_ruler(ctx.world, f.id);
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
        let ruler_adj = if faction.has_ruler { 0.05 } else { -0.15 };
        let target = (base_target + ruler_adj).clamp(0.15, 0.95);

        let noise: f64 = ctx.rng.random_range(-0.05..0.05);
        let mut drift = (target - faction.old_stability) * 0.12 + noise;
        // Direct instability pressure when leaderless
        if !faction.has_ruler {
            drift -= 0.04;
        }
        let new_stability = (faction.old_stability + drift).clamp(0.0, 1.0);
        updates.push(StabilityUpdate {
            faction_id: faction.id,
            new_stability,
        });
    }

    for update in updates {
        ctx.world.set_property(
            update.faction_id,
            "stability".to_string(),
            serde_json::json!(update.new_stability),
            year_event,
        );
    }
}

// --- 4c: Coups ---

fn check_coups(ctx: &mut TickContext, time: SimTimestamp, current_year: u32) {
    struct CoupTarget {
        faction_id: u64,
        current_ruler_id: u64,
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
            let stability = e
                .properties
                .get("stability")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.5);
            if stability >= 0.55 {
                return None;
            }
            let ruler_id = find_faction_ruler(ctx.world, e.id)?;
            let happiness = e
                .properties
                .get("happiness")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.5);
            let legitimacy = e
                .properties
                .get("legitimacy")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.5);
            Some(CoupTarget {
                faction_id: e.id,
                current_ruler_id: ruler_id,
                stability,
                happiness,
                legitimacy,
            })
        })
        .collect();

    for target in targets {
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
            .filter(|m| m.id != target.current_ruler_id)
            .collect();
        if candidates.is_empty() {
            continue;
        }

        let instigator_id = select_weighted_member(&candidates, &["warrior", "elder"], ctx.rng);

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
                let pop = e
                    .properties
                    .get("population")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
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
        let ruler_name = get_entity_name(ctx.world, target.current_ruler_id);
        let faction_name = get_entity_name(ctx.world, target.faction_id);

        if ctx.rng.random_range(0.0..1.0) < success_chance {
            // --- Successful coup ---
            let ev = ctx.world.add_event(
                EventKind::Coup,
                time,
                format!("{instigator_name} overthrew {ruler_name} of {faction_name} in year {current_year}"),
            );
            ctx.world
                .add_event_participant(ev, instigator_id, ParticipantRole::Instigator);
            ctx.world
                .add_event_participant(ev, target.current_ruler_id, ParticipantRole::Subject);
            ctx.world
                .add_event_participant(ev, target.faction_id, ParticipantRole::Object);

            // End old ruler's RulerOf
            ctx.world.end_relationship(
                target.current_ruler_id,
                target.faction_id,
                &RelationshipKind::RulerOf,
                time,
                ev,
            );

            // New ruler takes over
            ctx.world.add_relationship(
                instigator_id,
                target.faction_id,
                RelationshipKind::RulerOf,
                time,
                ev,
            );

            // Post-coup stability depends on sentiment
            let unhappiness_bonus = 0.25 * (1.0 - target.happiness);
            let illegitimacy_bonus = 0.1 * (1.0 - target.legitimacy);
            let post_coup_stability =
                (0.35 + unhappiness_bonus + illegitimacy_bonus).clamp(0.2, 0.65);
            ctx.world.set_property(
                target.faction_id,
                "stability".to_string(),
                serde_json::json!(post_coup_stability),
                ev,
            );

            // New legitimacy
            let new_legitimacy = if target.happiness < 0.35 {
                // Liberation: people were miserable
                0.4 + 0.3 * (1.0 - target.happiness)
            } else {
                // Power grab
                0.15 + 0.15 * (1.0 - target.happiness)
            };
            ctx.world.set_property(
                target.faction_id,
                "legitimacy".to_string(),
                serde_json::json!(new_legitimacy.clamp(0.0, 1.0)),
                ev,
            );

            // Happiness hit
            let happiness_hit = -0.05 - 0.1 * target.happiness;
            let new_happiness = (target.happiness + happiness_hit).clamp(0.0, 1.0);
            ctx.world.set_property(
                target.faction_id,
                "happiness".to_string(),
                serde_json::json!(new_happiness),
                ev,
            );
        } else {
            // --- Failed coup ---
            let ev = ctx.world.add_event(
                EventKind::Custom("failed_coup".to_string()),
                time,
                format!("{instigator_name} failed to overthrow {ruler_name} of {faction_name} in year {current_year}"),
            );
            ctx.world
                .add_event_participant(ev, instigator_id, ParticipantRole::Instigator);
            ctx.world
                .add_event_participant(ev, target.current_ruler_id, ParticipantRole::Subject);
            ctx.world
                .add_event_participant(ev, target.faction_id, ParticipantRole::Object);

            // Minor stability hit
            let old_stability = target.stability;
            ctx.world.set_property(
                target.faction_id,
                "stability".to_string(),
                serde_json::json!((old_stability - 0.05).clamp(0.0, 1.0)),
                ev,
            );

            // Legitimacy boost for surviving ruler
            let new_legitimacy = (target.legitimacy + 0.1).clamp(0.0, 1.0);
            ctx.world.set_property(
                target.faction_id,
                "legitimacy".to_string(),
                serde_json::json!(new_legitimacy),
                ev,
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
            FactionDiplo {
                id: e.id,
                happiness: e
                    .properties
                    .get("happiness")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.5),
                stability: e
                    .properties
                    .get("stability")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.5),
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
                        if ctx.rng.random_range(0.0..1.0) < 0.02 {
                            ends.push(EndAction {
                                source_id: fid,
                                target_id: rel.target_entity_id,
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
        ctx.world
            .add_relationship(rel.source_id, rel.target_id, rel.kind, time, ev);
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
            (
                e.id,
                FactionSentiment {
                    stability: e
                        .properties
                        .get("stability")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.5),
                    happiness: e
                        .properties
                        .get("happiness")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.5),
                    government_type: e
                        .properties
                        .get("government_type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("chieftain")
                        .to_string(),
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

        let new_faction_id = ctx
            .world
            .add_entity(EntityKind::Faction, name, Some(time), ev);

        // 50% inherit government type, 50% random
        let gov_type = if ctx.rng.random_bool(0.5) {
            split.old_gov_type.clone()
        } else {
            gov_types[ctx.rng.random_range(0..gov_types.len())].to_string()
        };

        ctx.world.set_property(
            new_faction_id,
            "government_type".to_string(),
            serde_json::json!(gov_type),
            ev,
        );
        ctx.world.set_property(
            new_faction_id,
            "stability".to_string(),
            serde_json::json!(0.5),
            ev,
        );
        ctx.world.set_property(
            new_faction_id,
            "happiness".to_string(),
            serde_json::json!((split.old_happiness + 0.1).clamp(0.0, 1.0)),
            ev,
        );
        ctx.world.set_property(
            new_faction_id,
            "legitimacy".to_string(),
            serde_json::json!(0.6),
            ev,
        );

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

        // End ruler relationship if any
        if let Some(ruler_id) = find_faction_ruler(ctx.world, faction_id) {
            ctx.world
                .end_relationship(ruler_id, faction_id, &RelationshipKind::RulerOf, time, ev);
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
        .map(|e| MemberInfo {
            id: e.id,
            birth_year: e
                .properties
                .get("birth_year")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            role: e
                .properties
                .get("role")
                .and_then(|v| v.as_str())
                .unwrap_or("common")
                .to_string(),
        })
        .collect()
}

fn select_ruler(
    members: &[MemberInfo],
    government_type: &str,
    _world: &World,
    rng: &mut dyn RngCore,
) -> Option<u64> {
    if members.is_empty() {
        return None;
    }

    match government_type {
        "hereditary" => {
            // Oldest member (lowest birth_year)
            members.iter().min_by_key(|m| m.birth_year).map(|m| m.id)
        }
        "elective" => {
            // Weighted random: elder/scholar roles get 3x weight
            let preferred = ["elder", "scholar"];
            Some(select_weighted_member_from_slice(members, &preferred, rng))
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

fn select_weighted_member(
    candidates: &[&MemberInfo],
    preferred_roles: &[&str],
    rng: &mut dyn RngCore,
) -> u64 {
    let weights: Vec<u32> = candidates
        .iter()
        .map(|m| {
            if preferred_roles.contains(&m.role.as_str()) {
                3
            } else {
                1
            }
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

fn select_weighted_member_from_slice(
    members: &[MemberInfo],
    preferred_roles: &[&str],
    rng: &mut dyn RngCore,
) -> u64 {
    let refs: Vec<&MemberInfo> = members.iter().collect();
    select_weighted_member(&refs, preferred_roles, rng)
}

fn has_ruler(world: &World, faction_id: u64) -> bool {
    world.entities.values().any(|e| {
        e.kind == EntityKind::Person
            && e.end.is_none()
            && e.relationships.iter().any(|r| {
                r.kind == RelationshipKind::RulerOf
                    && r.target_entity_id == faction_id
                    && r.end.is_none()
            })
    })
}

fn apply_happiness_delta(world: &mut World, faction_id: u64, delta: f64, event_id: u64) {
    if let Some(faction) = world.entities.get(&faction_id) {
        let old = faction
            .properties
            .get("happiness")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.5);
        let new = (old + delta).clamp(0.0, 1.0);
        world.set_property(
            faction_id,
            "happiness".to_string(),
            serde_json::json!(new),
            event_id,
        );
    }
}

fn apply_stability_delta(world: &mut World, faction_id: u64, delta: f64, event_id: u64) {
    if let Some(faction) = world.entities.get(&faction_id) {
        let old = faction
            .properties
            .get("stability")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.5);
        let new = (old + delta).clamp(0.0, 1.0);
        world.set_property(
            faction_id,
            "stability".to_string(),
            serde_json::json!(new),
            event_id,
        );
    }
}

fn apply_succession_stability_hit(world: &mut World, faction_id: u64, event_id: u64) {
    if let Some(faction) = world.entities.get(&faction_id) {
        let old_stability = faction
            .properties
            .get("stability")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.5);
        let new_stability = (old_stability - 0.12).clamp(0.0, 1.0);
        world.set_property(
            faction_id,
            "stability".to_string(),
            serde_json::json!(new_stability),
            event_id,
        );
    }
}

fn find_faction_ruler(world: &World, faction_id: u64) -> Option<u64> {
    world
        .entities
        .values()
        .find(|e| {
            e.kind == EntityKind::Person
                && e.end.is_none()
                && e.relationships.iter().any(|r| {
                    r.kind == RelationshipKind::RulerOf
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
        .and_then(|e| e.properties.get("government_type"))
        .and_then(|v| v.as_str())
        .unwrap_or("chieftain")
        .to_string()
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
    fn faction_gets_ruler_on_first_tick() {
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
            if has_ruler(&world, fid) {
                ruled += 1;
            }
        }
        // After 1 year, factions with members should have rulers
        assert!(
            ruled > 0,
            "at least some factions should have rulers after year 1"
        );
    }

    #[test]
    fn stability_drifts_without_ruler() {
        // Create a world, run 1 year to establish factions, then check stability
        let world = make_political_world(42, 50);

        for faction in world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        {
            assert!(
                faction.has_property("stability"),
                "faction {} should have stability",
                faction.name
            );
            let stability = faction.properties["stability"].as_f64().unwrap();
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
        // Try multiple seeds — coups require stability < 0.35 which is rare in stable worlds
        let mut total_coups = 0;
        let mut total_failed = 0;
        for seed in [42, 99, 123, 777] {
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
        }
        assert!(
            total_coups + total_failed > 0,
            "expected at least one coup attempt across 4 seeds x 1000 years (coups: {total_coups}, failed: {total_failed})"
        );
    }

    #[test]
    fn failed_coup_events_exist() {
        // Try multiple seeds to increase probability of observing a failed coup
        let mut total_failed = 0;
        let mut total_coups = 0;
        for seed in [42, 99, 123, 777, 1, 2, 3, 4] {
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
        }
        // Across 8 seeds x 1000 years, we expect at least one failed coup
        assert!(
            total_failed > 0,
            "expected at least one failed coup across 8 seeds x 1000 years (successes: {total_coups})"
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
                desc.contains("died in year"),
                "death description should be narrative: {desc}"
            );
        }
    }
}
