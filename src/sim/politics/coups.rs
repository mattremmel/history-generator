use rand::Rng;
use rand::RngCore;

use crate::model::action::ActionKind;
use crate::model::traits::{Trait, has_trait};
use crate::model::{EventKind, ParticipantRole, RelationshipKind, Role, SimTimestamp, World};
use crate::sim::context::TickContext;
use crate::sim::helpers;
use crate::sim::signal::{Signal, SignalKind};

use crate::sim::helpers::entity_name;

use super::{MemberInfo, collect_faction_members};

// --- Coups ---
const COUP_STABILITY_THRESHOLD: f64 = 0.55;
const COUP_BASE_ATTEMPT_CHANCE: f64 = 0.08;
const COUP_UNHAPPINESS_LOW_FACTOR: f64 = 0.3;
const COUP_UNHAPPINESS_HIGH_FACTOR: f64 = 0.7;
const COUP_LEADER_PRESTIGE_ATTEMPT_RESISTANCE: f64 = 0.3;
const COUP_MILITARY_NORMALIZATION: f64 = 200.0;
const COUP_RESISTANCE_BASE: f64 = 0.2;
const COUP_RESISTANCE_HAPPINESS_LOW: f64 = 0.3;
const COUP_RESISTANCE_HAPPINESS_HIGH: f64 = 0.7;
const COUP_LEADER_PRESTIGE_SUCCESS_RESISTANCE: f64 = 0.2;
const COUP_POWER_BASE: f64 = 0.2;
const COUP_POWER_INSTABILITY_WEIGHT: f64 = 0.3;
const COUP_NOISE_RANGE: f64 = 0.1;
const COUP_SUCCESS_MIN: f64 = 0.1;
const COUP_SUCCESS_MAX: f64 = 0.9;
const COUP_POST_UNHAPPINESS_BONUS_WEIGHT: f64 = 0.25;
const COUP_POST_ILLEGITIMACY_BONUS_WEIGHT: f64 = 0.1;
const COUP_POST_STABILITY_BASE: f64 = 0.35;
const COUP_POST_STABILITY_MIN: f64 = 0.2;
const COUP_POST_STABILITY_MAX: f64 = 0.65;
const COUP_LIBERATION_HAPPINESS_THRESHOLD: f64 = 0.35;
const COUP_LIBERATION_LEGITIMACY_BASE: f64 = 0.4;
const COUP_LIBERATION_LEGITIMACY_HAPPINESS_WEIGHT: f64 = 0.3;
const COUP_POWER_GRAB_LEGITIMACY_BASE: f64 = 0.15;
const COUP_POWER_GRAB_LEGITIMACY_HAPPINESS_WEIGHT: f64 = 0.15;
const COUP_HAPPINESS_HIT_BASE: f64 = -0.05;
const COUP_HAPPINESS_HIT_SCALED: f64 = -0.1;
const FAILED_COUP_STABILITY_HIT: f64 = -0.05;
const FAILED_COUP_LEGITIMACY_BOOST: f64 = 0.1;
const FAILED_COUP_EXECUTION_CHANCE: f64 = 0.5;

pub(super) fn check_coups(ctx: &mut TickContext, time: SimTimestamp, current_year: u32) {
    use crate::model::EntityKind;

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
            if stability >= COUP_STABILITY_THRESHOLD {
                return None;
            }
            let leader_id = helpers::faction_leader(ctx.world, e.id)?;
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
        let leader_prestige = ctx
            .world
            .entities
            .get(&target.current_leader_id)
            .and_then(|e| e.data.as_person())
            .map(|pd| pd.prestige)
            .unwrap_or(0.0);
        let attempt_chance = COUP_BASE_ATTEMPT_CHANCE
            * instability
            * (COUP_UNHAPPINESS_LOW_FACTOR + COUP_UNHAPPINESS_HIGH_FACTOR * unhappiness_factor)
            * (1.0 - leader_prestige * COUP_LEADER_PRESTIGE_ATTEMPT_RESISTANCE);
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
            &[Role::Warrior, Role::Elder],
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
        let military = (able_bodied as f64 / COUP_MILITARY_NORMALIZATION).clamp(0.0, 1.0);
        let resistance = COUP_RESISTANCE_BASE
            + military
                * target.legitimacy
                * (COUP_RESISTANCE_HAPPINESS_LOW
                    + COUP_RESISTANCE_HAPPINESS_HIGH * target.happiness)
            + leader_prestige * COUP_LEADER_PRESTIGE_SUCCESS_RESISTANCE;
        let noise: f64 = ctx.rng.random_range(-COUP_NOISE_RANGE..COUP_NOISE_RANGE);
        let coup_power =
            (COUP_POWER_BASE + COUP_POWER_INSTABILITY_WEIGHT * instability + noise).max(0.0);
        let success_chance =
            (coup_power / (coup_power + resistance)).clamp(COUP_SUCCESS_MIN, COUP_SUCCESS_MAX);

        // Collect names before mutation
        let instigator_name = entity_name(ctx.world, instigator_id);
        let leader_name = entity_name(ctx.world, target.current_leader_id);
        let faction_name = entity_name(ctx.world, target.faction_id);

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
                RelationshipKind::LeaderOf,
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
            let unhappiness_bonus = COUP_POST_UNHAPPINESS_BONUS_WEIGHT * (1.0 - target.happiness);
            let illegitimacy_bonus =
                COUP_POST_ILLEGITIMACY_BONUS_WEIGHT * (1.0 - target.legitimacy);
            let post_coup_stability =
                (COUP_POST_STABILITY_BASE + unhappiness_bonus + illegitimacy_bonus)
                    .clamp(COUP_POST_STABILITY_MIN, COUP_POST_STABILITY_MAX);

            // New legitimacy
            let new_legitimacy = if target.happiness < COUP_LIBERATION_HAPPINESS_THRESHOLD {
                // Liberation: people were miserable
                COUP_LIBERATION_LEGITIMACY_BASE
                    + COUP_LIBERATION_LEGITIMACY_HAPPINESS_WEIGHT * (1.0 - target.happiness)
            } else {
                // Power grab
                COUP_POWER_GRAB_LEGITIMACY_BASE
                    + COUP_POWER_GRAB_LEGITIMACY_HAPPINESS_WEIGHT * (1.0 - target.happiness)
            }
            .clamp(0.0, 1.0);

            // Happiness hit
            let happiness_hit =
                COUP_HAPPINESS_HIT_BASE + COUP_HAPPINESS_HIT_SCALED * target.happiness;
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
            let new_stability = (target.stability + FAILED_COUP_STABILITY_HIT).clamp(0.0, 1.0);

            // Legitimacy boost for surviving leader
            let new_legitimacy = (target.legitimacy + FAILED_COUP_LEGITIMACY_BOOST).clamp(0.0, 1.0);

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
            if ctx.rng.random_bool(FAILED_COUP_EXECUTION_CHANCE) {
                let death_ev = ctx.world.add_caused_event(
                    EventKind::Death,
                    time,
                    format!("{instigator_name} was executed in year {current_year}"),
                    ev,
                );
                ctx.world
                    .add_event_participant(death_ev, instigator_id, ParticipantRole::Subject);

                // End relationships
                helpers::end_all_person_relationships(ctx.world, instigator_id, time, death_ev);

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

fn select_weighted_member_with_traits(
    candidates: &[&MemberInfo],
    preferred_roles: &[Role],
    world: &World,
    rng: &mut dyn RngCore,
) -> u64 {
    let weights: Vec<u32> = candidates
        .iter()
        .map(|m| {
            let mut w: u32 = if preferred_roles.contains(&m.role) {
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
