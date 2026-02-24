use rand::Rng;

use super::context::TickContext;
use super::extra_keys as K;
use super::signal::{Signal, SignalKind};
use super::system::{SimSystem, TickFrequency};
use crate::model::action::{Action, ActionKind, ActionOutcome, ActionResult, ActionSource};
use crate::model::{EntityKind, EventKind, ParticipantRole, RelationshipKind, World};
use crate::sim::helpers;

// --- Support faction ---
const SUPPORT_STABILITY_BOOST: f64 = 0.08;
const SUPPORT_HAPPINESS_BOOST: f64 = 0.06;

// --- Undermine faction ---
const UNDERMINE_STABILITY_PENALTY: f64 = 0.10;
const UNDERMINE_HAPPINESS_PENALTY: f64 = 0.08;
const UNDERMINE_LEGITIMACY_PENALTY: f64 = 0.06;

// --- Coup attempt ---
const COUP_ABLE_BODIED_DIVISOR: u32 = 4;
const COUP_MILITARY_NORMALIZATION: f64 = 200.0;
const COUP_RESISTANCE_BASE: f64 = 0.2;
const COUP_RESISTANCE_HAPPINESS_WEIGHT: f64 = 0.3;
const COUP_RESISTANCE_HAPPINESS_COMPLEMENT: f64 = 0.7;
const COUP_NOISE_RANGE: f64 = 0.1;
const COUP_POWER_BASE: f64 = 0.2;
const COUP_POWER_INSTABILITY_FACTOR: f64 = 0.3;
const COUP_SUCCESS_MIN: f64 = 0.1;
const COUP_SUCCESS_MAX: f64 = 0.9;
const COUP_STABILITY_MULTIPLIER: f64 = 0.6;
const COUP_LEGITIMACY_MULTIPLIER: f64 = 0.5;
const COUP_LEGITIMACY_BASE: f64 = 0.1;
const COUP_FAILED_EXECUTION_CHANCE: f64 = 0.5;

// --- Defection ---
const DEFECT_STABILITY_PENALTY: f64 = 0.05;

// --- Seek office (election) ---
const ELECTION_BASE_CHANCE: f64 = 0.3;
const ELECTION_CHARISMATIC_BONUS: f64 = 0.2;
const ELECTION_INSTABILITY_BONUS: f64 = 0.1;
const ELECTION_INSTABILITY_THRESHOLD: f64 = 0.5;

pub struct ActionSystem;

impl SimSystem for ActionSystem {
    fn name(&self) -> &str {
        "actions"
    }

    fn frequency(&self) -> TickFrequency {
        TickFrequency::Yearly
    }

    fn tick(&mut self, ctx: &mut TickContext) {
        let actions: Vec<Action> = std::mem::take(&mut ctx.world.pending_actions);

        for action in actions {
            let source = action.source.clone();
            let outcome = match action.kind {
                ActionKind::Assassinate { target_id } => {
                    process_assassinate(ctx, action.actor_id, &action.source, target_id)
                }
                ActionKind::SupportFaction { faction_id } => {
                    process_support_faction(ctx, action.actor_id, &action.source, faction_id)
                }
                ActionKind::UndermineFaction { faction_id } => {
                    process_undermine_faction(ctx, action.actor_id, &action.source, faction_id)
                }
                ActionKind::BrokerAlliance {
                    faction_a,
                    faction_b,
                } => process_broker_alliance(
                    ctx,
                    action.actor_id,
                    &action.source,
                    faction_a,
                    faction_b,
                ),
                ActionKind::DeclareWar { target_faction_id } => {
                    process_declare_war(ctx, action.actor_id, &action.source, target_faction_id)
                }
                ActionKind::AttemptCoup { faction_id } => {
                    process_attempt_coup(ctx, action.actor_id, &action.source, faction_id)
                }
                ActionKind::Defect {
                    from_faction,
                    to_faction,
                } => process_defect(
                    ctx,
                    action.actor_id,
                    &action.source,
                    from_faction,
                    to_faction,
                ),
                ActionKind::SeekOffice { faction_id } => {
                    process_seek_office(ctx, action.actor_id, &action.source, faction_id)
                }
            };
            ctx.world.action_results.push(ActionResult {
                actor_id: action.actor_id,
                source,
                outcome,
            });
        }
    }
}

fn store_source_on_event(world: &mut World, event_id: u64, source: &ActionSource) {
    if let Some(event) = world.events.get_mut(&event_id) {
        event.data = serde_json::to_value(source).unwrap();
    }
}

fn process_assassinate(
    ctx: &mut TickContext,
    actor_id: u64,
    source: &ActionSource,
    target_id: u64,
) -> ActionOutcome {
    let time = ctx.world.current_time;
    let year = time.year();

    // Validate target exists and is a living person
    let target_valid = ctx
        .world
        .entities
        .get(&target_id)
        .is_some_and(|e| e.kind == EntityKind::Person && e.end.is_none());
    if !target_valid {
        return ActionOutcome::Failed {
            reason: format!("target {target_id} does not exist or is not a living person"),
        };
    }

    let actor_name = helpers::entity_name(ctx.world, actor_id);
    let target_name = helpers::entity_name(ctx.world, target_id);

    // Create assassination event
    let assassination_ev = ctx.world.add_event(
        EventKind::Custom("assassination".to_string()),
        time,
        format!("{actor_name} assassinated {target_name} in year {year}"),
    );
    store_source_on_event(ctx.world, assassination_ev, source);
    ctx.world
        .add_event_participant(assassination_ev, actor_id, ParticipantRole::Instigator);
    ctx.world
        .add_event_participant(assassination_ev, target_id, ParticipantRole::Object);

    // Create caused Death event
    let death_ev = ctx.world.add_caused_event(
        EventKind::Death,
        time,
        format!("{target_name} was killed in year {year}"),
        assassination_ev,
    );
    ctx.world
        .add_event_participant(death_ev, target_id, ParticipantRole::Subject);

    // Check if target was a leader before ending relationships
    let leader_of_faction: Option<u64> = ctx
        .world
        .entities
        .get(&target_id)
        .and_then(|e| e.active_rel(RelationshipKind::LeaderOf));

    // End all active relationships
    helpers::end_all_person_relationships(ctx.world, target_id, time, death_ev);

    // End the target entity
    ctx.world.end_entity(target_id, time, death_ev);

    // Emit EntityDied signal
    ctx.signals.push(Signal {
        event_id: death_ev,
        kind: SignalKind::EntityDied {
            entity_id: target_id,
        },
    });

    // If target was a leader, emit LeaderVacancy signal
    if let Some(faction_id) = leader_of_faction {
        ctx.signals.push(Signal {
            event_id: death_ev,
            kind: SignalKind::LeaderVacancy {
                faction_id,
                previous_leader_id: target_id,
            },
        });
    }

    ActionOutcome::Success {
        event_id: assassination_ev,
    }
}

fn process_support_faction(
    ctx: &mut TickContext,
    actor_id: u64,
    source: &ActionSource,
    faction_id: u64,
) -> ActionOutcome {
    let time = ctx.world.current_time;
    let year = time.year();

    let faction_valid = ctx
        .world
        .entities
        .get(&faction_id)
        .is_some_and(|e| e.kind == EntityKind::Faction && e.end.is_none());
    if !faction_valid {
        return ActionOutcome::Failed {
            reason: format!("faction {faction_id} does not exist or is not a living faction"),
        };
    }

    let actor_name = helpers::entity_name(ctx.world, actor_id);
    let faction_name = helpers::entity_name(ctx.world, faction_id);

    let ev = ctx.world.add_event(
        EventKind::Custom("faction_support".to_string()),
        time,
        format!("{actor_name} bolstered {faction_name} in year {year}"),
    );
    store_source_on_event(ctx.world, ev, source);
    ctx.world
        .add_event_participant(ev, actor_id, ParticipantRole::Instigator);
    ctx.world
        .add_event_participant(ev, faction_id, ParticipantRole::Object);

    // Apply boosts
    let (old_stab, new_stab, old_hap, new_hap) = {
        let entity = ctx.world.entities.get_mut(&faction_id).unwrap();
        let fd = entity.data.as_faction_mut().unwrap();
        let old_stab = fd.stability;
        let old_hap = fd.happiness;
        fd.stability = (old_stab + SUPPORT_STABILITY_BOOST).clamp(0.0, 1.0);
        fd.happiness = (old_hap + SUPPORT_HAPPINESS_BOOST).clamp(0.0, 1.0);
        (old_stab, fd.stability, old_hap, fd.happiness)
    };
    ctx.world.record_change(
        faction_id,
        ev,
        "stability",
        serde_json::json!(old_stab),
        serde_json::json!(new_stab),
    );
    ctx.world.record_change(
        faction_id,
        ev,
        "happiness",
        serde_json::json!(old_hap),
        serde_json::json!(new_hap),
    );

    ActionOutcome::Success { event_id: ev }
}

fn process_undermine_faction(
    ctx: &mut TickContext,
    actor_id: u64,
    source: &ActionSource,
    faction_id: u64,
) -> ActionOutcome {
    let time = ctx.world.current_time;
    let year = time.year();

    let faction_valid = ctx
        .world
        .entities
        .get(&faction_id)
        .is_some_and(|e| e.kind == EntityKind::Faction && e.end.is_none());
    if !faction_valid {
        return ActionOutcome::Failed {
            reason: format!("faction {faction_id} does not exist or is not a living faction"),
        };
    }

    let actor_name = helpers::entity_name(ctx.world, actor_id);
    let faction_name = helpers::entity_name(ctx.world, faction_id);

    let ev = ctx.world.add_event(
        EventKind::Custom("faction_undermine".to_string()),
        time,
        format!("{actor_name} undermined {faction_name} in year {year}"),
    );
    store_source_on_event(ctx.world, ev, source);
    ctx.world
        .add_event_participant(ev, actor_id, ParticipantRole::Instigator);
    ctx.world
        .add_event_participant(ev, faction_id, ParticipantRole::Object);

    // Apply penalties
    let (old_stab, new_stab, old_hap, new_hap, old_leg, new_leg) = {
        let entity = ctx.world.entities.get_mut(&faction_id).unwrap();
        let fd = entity.data.as_faction_mut().unwrap();
        let old_stab = fd.stability;
        let old_hap = fd.happiness;
        let old_leg = fd.legitimacy;
        fd.stability = (old_stab - UNDERMINE_STABILITY_PENALTY).clamp(0.0, 1.0);
        fd.happiness = (old_hap - UNDERMINE_HAPPINESS_PENALTY).clamp(0.0, 1.0);
        fd.legitimacy = (old_leg - UNDERMINE_LEGITIMACY_PENALTY).clamp(0.0, 1.0);
        (
            old_stab,
            fd.stability,
            old_hap,
            fd.happiness,
            old_leg,
            fd.legitimacy,
        )
    };
    ctx.world.record_change(
        faction_id,
        ev,
        "stability",
        serde_json::json!(old_stab),
        serde_json::json!(new_stab),
    );
    ctx.world.record_change(
        faction_id,
        ev,
        "happiness",
        serde_json::json!(old_hap),
        serde_json::json!(new_hap),
    );
    ctx.world.record_change(
        faction_id,
        ev,
        "legitimacy",
        serde_json::json!(old_leg),
        serde_json::json!(new_leg),
    );

    ActionOutcome::Success { event_id: ev }
}

fn process_broker_alliance(
    ctx: &mut TickContext,
    actor_id: u64,
    source: &ActionSource,
    faction_a: u64,
    faction_b: u64,
) -> ActionOutcome {
    let time = ctx.world.current_time;
    let year = time.year();

    if faction_a == faction_b {
        return ActionOutcome::Failed {
            reason: "cannot broker alliance between a faction and itself".to_string(),
        };
    }

    // Validate both factions exist and are alive
    let a_valid = ctx
        .world
        .entities
        .get(&faction_a)
        .is_some_and(|e| e.kind == EntityKind::Faction && e.end.is_none());
    let b_valid = ctx
        .world
        .entities
        .get(&faction_b)
        .is_some_and(|e| e.kind == EntityKind::Faction && e.end.is_none());
    if !a_valid || !b_valid {
        return ActionOutcome::Failed {
            reason: "one or both factions do not exist or are not alive".to_string(),
        };
    }

    // Check for existing relationships between the two
    let has_existing = has_active_rel_between(ctx.world, faction_a, faction_b);
    if has_existing {
        // Determine if allied or enemies for better error message
        if helpers::has_active_rel_of_kind(ctx.world, faction_a, faction_b, RelationshipKind::Ally)
        {
            return ActionOutcome::Failed {
                reason: "factions are already allied".to_string(),
            };
        }
        if helpers::has_active_rel_of_kind(ctx.world, faction_a, faction_b, RelationshipKind::Enemy)
        {
            return ActionOutcome::Failed {
                reason: "factions are currently enemies".to_string(),
            };
        }
        return ActionOutcome::Failed {
            reason: "factions already have an active diplomatic relationship".to_string(),
        };
    }

    let actor_name = helpers::entity_name(ctx.world, actor_id);
    let name_a = helpers::entity_name(ctx.world, faction_a);
    let name_b = helpers::entity_name(ctx.world, faction_b);

    let ev = ctx.world.add_event(
        EventKind::Custom("broker_alliance".to_string()),
        time,
        format!("{actor_name} brokered an alliance between {name_a} and {name_b} in year {year}"),
    );
    store_source_on_event(ctx.world, ev, source);
    ctx.world
        .add_event_participant(ev, actor_id, ParticipantRole::Instigator);
    ctx.world
        .add_event_participant(ev, faction_a, ParticipantRole::Subject);
    ctx.world
        .add_event_participant(ev, faction_b, ParticipantRole::Object);

    // Add bidirectional Ally relationships
    ctx.world
        .add_relationship(faction_a, faction_b, RelationshipKind::Ally, time, ev);
    ctx.world
        .add_relationship(faction_b, faction_a, RelationshipKind::Ally, time, ev);

    ActionOutcome::Success { event_id: ev }
}

fn process_declare_war(
    ctx: &mut TickContext,
    actor_id: u64,
    source: &ActionSource,
    target_faction_id: u64,
) -> ActionOutcome {
    let time = ctx.world.current_time;
    let year = time.year();

    // Validate target is a living faction
    let target_valid = ctx
        .world
        .entities
        .get(&target_faction_id)
        .is_some_and(|e| e.kind == EntityKind::Faction && e.end.is_none());
    if !target_valid {
        return ActionOutcome::Failed {
            reason: format!(
                "faction {target_faction_id} does not exist or is not a living faction"
            ),
        };
    }

    // Find actor's faction
    let Some(actor_faction) = find_actor_faction(ctx.world, actor_id) else {
        return ActionOutcome::Failed {
            reason: "actor does not belong to any faction".to_string(),
        };
    };

    if actor_faction == target_faction_id {
        return ActionOutcome::Failed {
            reason: "cannot declare war on own faction".to_string(),
        };
    }

    // Check not already at war
    if helpers::has_active_rel_of_kind(
        ctx.world,
        actor_faction,
        target_faction_id,
        RelationshipKind::AtWar,
    ) {
        return ActionOutcome::Failed {
            reason: "factions are already at war".to_string(),
        };
    }

    let attacker_name = helpers::entity_name(ctx.world, actor_faction);
    let defender_name = helpers::entity_name(ctx.world, target_faction_id);
    let actor_name = helpers::entity_name(ctx.world, actor_id);

    let ev = ctx.world.add_event(
        EventKind::WarDeclared,
        time,
        format!("{actor_name} of {attacker_name} declared war on {defender_name} in year {year}"),
    );
    store_source_on_event(ctx.world, ev, source);
    ctx.world
        .add_event_participant(ev, actor_id, ParticipantRole::Instigator);
    ctx.world
        .add_event_participant(ev, actor_faction, ParticipantRole::Attacker);
    ctx.world
        .add_event_participant(ev, target_faction_id, ParticipantRole::Defender);

    // Add bidirectional AtWar relationships
    ctx.world.add_relationship(
        actor_faction,
        target_faction_id,
        RelationshipKind::AtWar,
        time,
        ev,
    );
    ctx.world.add_relationship(
        target_faction_id,
        actor_faction,
        RelationshipKind::AtWar,
        time,
        ev,
    );

    // Set war_start_year on both factions
    ctx.world.set_extra(
        actor_faction,
        K::WAR_START_YEAR,
        serde_json::json!(year),
        ev,
    );
    ctx.world.set_extra(
        target_faction_id,
        K::WAR_START_YEAR,
        serde_json::json!(year),
        ev,
    );

    // End any active Ally relationship between them
    helpers::end_ally_relationship(ctx.world, actor_faction, target_faction_id, time, ev);

    ctx.signals.push(Signal {
        event_id: ev,
        kind: SignalKind::WarStarted {
            attacker_id: actor_faction,
            defender_id: target_faction_id,
        },
    });

    ActionOutcome::Success { event_id: ev }
}

fn process_attempt_coup(
    ctx: &mut TickContext,
    actor_id: u64,
    source: &ActionSource,
    faction_id: u64,
) -> ActionOutcome {
    let time = ctx.world.current_time;
    let year = time.year();

    // Validate faction exists and is alive
    let faction_valid = ctx
        .world
        .entities
        .get(&faction_id)
        .is_some_and(|e| e.kind == EntityKind::Faction && e.end.is_none());
    if !faction_valid {
        return ActionOutcome::Failed {
            reason: format!("faction {faction_id} does not exist or is not a living faction"),
        };
    }

    // Find current leader
    let current_leader_id = helpers::faction_leader(ctx.world, faction_id);
    let Some(leader_id) = current_leader_id else {
        return ActionOutcome::Failed {
            reason: "faction has no leader to overthrow".to_string(),
        };
    };

    if actor_id == leader_id {
        return ActionOutcome::Failed {
            reason: "cannot coup yourself".to_string(),
        };
    }

    // Compute success chance based on faction instability
    let stability = get_faction_field(ctx.world, faction_id, "stability", 0.5);
    let happiness = get_faction_field(ctx.world, faction_id, "happiness", 0.5);
    let legitimacy = get_faction_field(ctx.world, faction_id, "legitimacy", 0.5);
    let instability = 1.0 - stability;

    // Military strength from faction settlements
    let mut able_bodied = 0u32;
    for e in ctx.world.entities.values() {
        if e.kind == EntityKind::Settlement
            && e.end.is_none()
            && e.has_active_rel(RelationshipKind::MemberOf, faction_id)
        {
            let pop = e.data.as_settlement().map(|s| s.population).unwrap_or(0);
            able_bodied += pop / COUP_ABLE_BODIED_DIVISOR;
        }
    }
    let military = (able_bodied as f64 / COUP_MILITARY_NORMALIZATION).clamp(0.0, 1.0);
    let resistance = COUP_RESISTANCE_BASE
        + military
            * legitimacy
            * (COUP_RESISTANCE_HAPPINESS_WEIGHT + COUP_RESISTANCE_HAPPINESS_COMPLEMENT * happiness);
    let noise: f64 = ctx.rng.random_range(-COUP_NOISE_RANGE..COUP_NOISE_RANGE);
    let coup_power =
        (COUP_POWER_BASE + COUP_POWER_INSTABILITY_FACTOR * instability + noise).max(0.0);
    let success_chance =
        (coup_power / (coup_power + resistance)).clamp(COUP_SUCCESS_MIN, COUP_SUCCESS_MAX);

    let actor_name = helpers::entity_name(ctx.world, actor_id);
    let leader_name = helpers::entity_name(ctx.world, leader_id);
    let faction_name = helpers::entity_name(ctx.world, faction_id);

    if ctx.rng.random_range(0.0..1.0) < success_chance {
        // Successful coup
        let ev = ctx.world.add_event(
            EventKind::Coup,
            time,
            format!("{actor_name} overthrew {leader_name} of {faction_name} in year {year}"),
        );
        store_source_on_event(ctx.world, ev, source);
        ctx.world
            .add_event_participant(ev, actor_id, ParticipantRole::Instigator);
        ctx.world
            .add_event_participant(ev, leader_id, ParticipantRole::Subject);
        ctx.world
            .add_event_participant(ev, faction_id, ParticipantRole::Object);

        // End old leader's LeaderOf
        ctx.world
            .end_relationship(leader_id, faction_id, RelationshipKind::LeaderOf, time, ev);

        // New leader takes over
        ctx.world
            .add_relationship(actor_id, faction_id, RelationshipKind::LeaderOf, time, ev);

        // Post-coup stability hit
        let new_stability = (stability * COUP_STABILITY_MULTIPLIER).clamp(0.0, 1.0);
        let new_legitimacy =
            (legitimacy * COUP_LEGITIMACY_MULTIPLIER + COUP_LEGITIMACY_BASE).clamp(0.0, 1.0);
        {
            let entity = ctx.world.entities.get_mut(&faction_id).unwrap();
            let fd = entity.data.as_faction_mut().unwrap();
            fd.stability = new_stability;
            fd.legitimacy = new_legitimacy;
        } // entity borrow dropped
        ctx.world.record_change(
            faction_id,
            ev,
            "stability",
            serde_json::json!(stability),
            serde_json::json!(new_stability),
        );
        ctx.world.record_change(
            faction_id,
            ev,
            "legitimacy",
            serde_json::json!(legitimacy),
            serde_json::json!(new_legitimacy),
        );

        ActionOutcome::Success { event_id: ev }
    } else {
        // Failed coup — 50% chance instigator is executed
        let ev = ctx.world.add_event(
            EventKind::Custom("failed_coup".to_string()),
            time,
            format!(
                "{actor_name} failed to overthrow {leader_name} of {faction_name} in year {year}"
            ),
        );
        store_source_on_event(ctx.world, ev, source);
        ctx.world
            .add_event_participant(ev, actor_id, ParticipantRole::Instigator);
        ctx.world
            .add_event_participant(ev, leader_id, ParticipantRole::Subject);
        ctx.world
            .add_event_participant(ev, faction_id, ParticipantRole::Object);

        if ctx.rng.random_bool(COUP_FAILED_EXECUTION_CHANCE) {
            // Instigator executed
            let death_ev = ctx.world.add_caused_event(
                EventKind::Death,
                time,
                format!("{actor_name} was executed after a failed coup in year {year}"),
                ev,
            );
            ctx.world
                .add_event_participant(death_ev, actor_id, ParticipantRole::Subject);
            helpers::end_all_person_relationships(ctx.world, actor_id, time, death_ev);
            ctx.world.end_entity(actor_id, time, death_ev);
            ctx.signals.push(Signal {
                event_id: death_ev,
                kind: SignalKind::EntityDied {
                    entity_id: actor_id,
                },
            });
        }

        ActionOutcome::Failed {
            reason: "coup attempt failed".to_string(),
        }
    }
}

fn process_defect(
    ctx: &mut TickContext,
    actor_id: u64,
    source: &ActionSource,
    from_faction: u64,
    to_faction: u64,
) -> ActionOutcome {
    let time = ctx.world.current_time;
    let year = time.year();

    // Validate both factions alive
    let from_valid = ctx
        .world
        .entities
        .get(&from_faction)
        .is_some_and(|e| e.kind == EntityKind::Faction && e.end.is_none());
    let to_valid = ctx
        .world
        .entities
        .get(&to_faction)
        .is_some_and(|e| e.kind == EntityKind::Faction && e.end.is_none());
    if !from_valid || !to_valid {
        return ActionOutcome::Failed {
            reason: "one or both factions do not exist or are not alive".to_string(),
        };
    }

    // Validate NPC is member of from_faction
    let is_member = ctx
        .world
        .entities
        .get(&actor_id)
        .is_some_and(|e| e.has_active_rel(RelationshipKind::MemberOf, from_faction));
    if !is_member {
        return ActionOutcome::Failed {
            reason: "actor is not a member of the source faction".to_string(),
        };
    }

    // Leaders can't defect
    let is_leader = ctx
        .world
        .entities
        .get(&actor_id)
        .is_some_and(|e| e.has_active_rel(RelationshipKind::LeaderOf, from_faction));
    if is_leader {
        return ActionOutcome::Failed {
            reason: "leaders cannot defect".to_string(),
        };
    }

    let actor_name = helpers::entity_name(ctx.world, actor_id);
    let from_name = helpers::entity_name(ctx.world, from_faction);
    let to_name = helpers::entity_name(ctx.world, to_faction);

    let ev = ctx.world.add_event(
        EventKind::Custom("defection".to_string()),
        time,
        format!("{actor_name} defected from {from_name} to {to_name} in year {year}"),
    );
    store_source_on_event(ctx.world, ev, source);
    ctx.world
        .add_event_participant(ev, actor_id, ParticipantRole::Instigator);
    ctx.world
        .add_event_participant(ev, from_faction, ParticipantRole::Origin);
    ctx.world
        .add_event_participant(ev, to_faction, ParticipantRole::Destination);

    // End MemberOf with old faction
    ctx.world
        .end_relationship(actor_id, from_faction, RelationshipKind::MemberOf, time, ev);

    // Start MemberOf with new faction
    ctx.world
        .add_relationship(actor_id, to_faction, RelationshipKind::MemberOf, time, ev);

    // Update LocatedIn if new faction has a settlement
    let new_faction_settlement: Option<u64> = ctx
        .world
        .entities
        .values()
        .find(|e| {
            e.kind == EntityKind::Settlement
                && e.end.is_none()
                && e.has_active_rel(RelationshipKind::MemberOf, to_faction)
        })
        .map(|e| e.id);

    if let Some(settlement_id) = new_faction_settlement {
        // End old LocatedIn if any
        let old_location: Option<u64> = ctx
            .world
            .entities
            .get(&actor_id)
            .and_then(|e| e.active_rel(RelationshipKind::LocatedIn));
        if let Some(old_loc) = old_location {
            ctx.world
                .end_relationship(actor_id, old_loc, RelationshipKind::LocatedIn, time, ev);
        }
        ctx.world.add_relationship(
            actor_id,
            settlement_id,
            RelationshipKind::LocatedIn,
            time,
            ev,
        );
    }

    // Apply stability hit to old faction
    let (old_stab, new_stab) = {
        let entity = ctx.world.entities.get_mut(&from_faction).unwrap();
        let fd = entity.data.as_faction_mut().unwrap();
        let old_stab = fd.stability;
        fd.stability = (old_stab - DEFECT_STABILITY_PENALTY).clamp(0.0, 1.0);
        (old_stab, fd.stability)
    };
    ctx.world.record_change(
        from_faction,
        ev,
        "stability",
        serde_json::json!(old_stab),
        serde_json::json!(new_stab),
    );

    ActionOutcome::Success { event_id: ev }
}

fn process_seek_office(
    ctx: &mut TickContext,
    actor_id: u64,
    source: &ActionSource,
    faction_id: u64,
) -> ActionOutcome {
    let time = ctx.world.current_time;
    let year = time.year();

    // Validate faction is alive
    let faction_valid = ctx
        .world
        .entities
        .get(&faction_id)
        .is_some_and(|e| e.kind == EntityKind::Faction && e.end.is_none());
    if !faction_valid {
        return ActionOutcome::Failed {
            reason: format!("faction {faction_id} does not exist or is not a living faction"),
        };
    }

    // Validate NPC is a member
    let is_member = ctx
        .world
        .entities
        .get(&actor_id)
        .is_some_and(|e| e.has_active_rel(RelationshipKind::MemberOf, faction_id));
    if !is_member {
        return ActionOutcome::Failed {
            reason: "actor is not a member of the faction".to_string(),
        };
    }

    // Check if already leader
    let is_already_leader = ctx
        .world
        .entities
        .get(&actor_id)
        .is_some_and(|e| e.has_active_rel(RelationshipKind::LeaderOf, faction_id));
    if is_already_leader {
        return ActionOutcome::Failed {
            reason: "actor is already the leader".to_string(),
        };
    }

    let current_leader_id = helpers::faction_leader(ctx.world, faction_id);
    let actor_name = helpers::entity_name(ctx.world, actor_id);
    let faction_name = helpers::entity_name(ctx.world, faction_id);

    if current_leader_id.is_none() {
        // Leaderless faction → auto-succeed
        let ev = ctx.world.add_event(
            EventKind::Succession,
            time,
            format!("{actor_name} claimed leadership of {faction_name} in year {year}"),
        );
        store_source_on_event(ctx.world, ev, source);
        ctx.world
            .add_event_participant(ev, actor_id, ParticipantRole::Instigator);
        ctx.world
            .add_event_participant(ev, faction_id, ParticipantRole::Object);

        ctx.world
            .add_relationship(actor_id, faction_id, RelationshipKind::LeaderOf, time, ev);

        return ActionOutcome::Success { event_id: ev };
    }

    // Faction has leader — check government type
    let gov_type_owned = ctx
        .world
        .entities
        .get(&faction_id)
        .and_then(|e| e.data.as_faction())
        .map(|f| f.government_type.clone())
        .unwrap_or_else(|| "chieftain".to_string());
    let gov_type = gov_type_owned.as_str();

    if gov_type != "elective" {
        return ActionOutcome::Failed {
            reason: "faction government is not elective".to_string(),
        };
    }

    // Elective faction: probabilistic success
    // 30% base chance, +20% if Charismatic, +10% per stability below 0.5
    let stability = get_faction_field(ctx.world, faction_id, "stability", 0.5);
    let mut success_chance = ELECTION_BASE_CHANCE;

    // Check if actor has Charismatic trait
    let has_charismatic = ctx.world.entities.get(&actor_id).is_some_and(|e| {
        crate::model::traits::has_trait(e, &crate::model::traits::Trait::Charismatic)
    });
    if has_charismatic {
        success_chance += ELECTION_CHARISMATIC_BONUS;
    }

    // Instability bonus
    if stability < ELECTION_INSTABILITY_THRESHOLD {
        success_chance += ELECTION_INSTABILITY_BONUS
            * ((ELECTION_INSTABILITY_THRESHOLD - stability) / ELECTION_INSTABILITY_THRESHOLD);
    }

    let leader_id = current_leader_id.unwrap();
    let leader_name = helpers::entity_name(ctx.world, leader_id);

    if ctx.rng.random_range(0.0..1.0) < success_chance {
        // Success: replace leader
        let ev = ctx.world.add_event(
            EventKind::Succession,
            time,
            format!(
                "{actor_name} was elected to lead {faction_name}, replacing {leader_name} in year {year}"
            ),
        );
        store_source_on_event(ctx.world, ev, source);
        ctx.world
            .add_event_participant(ev, actor_id, ParticipantRole::Instigator);
        ctx.world
            .add_event_participant(ev, leader_id, ParticipantRole::Subject);
        ctx.world
            .add_event_participant(ev, faction_id, ParticipantRole::Object);

        // End old leader's LeaderOf
        ctx.world
            .end_relationship(leader_id, faction_id, RelationshipKind::LeaderOf, time, ev);

        // New leader takes over
        ctx.world
            .add_relationship(actor_id, faction_id, RelationshipKind::LeaderOf, time, ev);

        ActionOutcome::Success { event_id: ev }
    } else {
        // Failed election — no death risk
        let ev = ctx.world.add_event(
            EventKind::Custom("failed_election".to_string()),
            time,
            format!("{actor_name} failed to win election in {faction_name} in year {year}"),
        );
        store_source_on_event(ctx.world, ev, source);
        ctx.world
            .add_event_participant(ev, actor_id, ParticipantRole::Instigator);
        ctx.world
            .add_event_participant(ev, faction_id, ParticipantRole::Object);

        ActionOutcome::Failed {
            reason: "election attempt failed".to_string(),
        }
    }
}

fn find_actor_faction(world: &World, actor_id: u64) -> Option<u64> {
    world.entities.get(&actor_id).and_then(|e| {
        e.active_rels(RelationshipKind::MemberOf).find(|&target| {
            world
                .entities
                .get(&target)
                .is_some_and(|t| t.kind == EntityKind::Faction)
        })
    })
}

// --- Helpers ---

fn get_faction_field(world: &World, faction_id: u64, field: &str, default: f64) -> f64 {
    world
        .entities
        .get(&faction_id)
        .and_then(|e| e.data.as_faction())
        .map(|f| match field {
            "stability" => f.stability,
            "happiness" => f.happiness,
            "legitimacy" => f.legitimacy,
            _ => default,
        })
        .unwrap_or(default)
}

fn has_active_rel_between(world: &World, a: u64, b: u64) -> bool {
    has_active_rel_directed(world, a, b) || has_active_rel_directed(world, b, a)
}

fn has_active_rel_directed(world: &World, source: u64, target: u64) -> bool {
    world.entities.get(&source).is_some_and(|e| {
        e.relationships.iter().any(|r| {
            r.target_entity_id == target
                && r.end.is_none()
                && matches!(
                    r.kind,
                    RelationshipKind::Ally | RelationshipKind::Enemy | RelationshipKind::AtWar
                )
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::action::{Action, ActionKind, ActionOutcome, ActionSource};
    use crate::scenario::Scenario;
    use crate::testutil;

    fn tick(world: &mut World) -> Vec<crate::sim::signal::Signal> {
        testutil::tick_system(world, &mut ActionSystem, 100, 42)
    }

    /// Faction with matching initial values for property-checking tests.
    fn add_test_faction(s: &mut Scenario, name: &str) -> u64 {
        s.faction(name)
            .stability(0.5)
            .happiness(0.5)
            .legitimacy(0.5)
            .treasury(0.0)
            .id()
    }

    #[test]
    fn scenario_assassinate_kills_target() {
        let mut s = Scenario::at_year(100);
        let actor_id = s.add_person_standalone("Dorian");
        s.make_player(actor_id);
        let target_id = s.add_person_standalone("Victim");
        let mut world = s.build();

        world.queue_action(Action {
            actor_id,
            source: ActionSource::Player,
            kind: ActionKind::Assassinate { target_id },
        });

        let signals = tick(&mut world);

        assert!(world.entities[&target_id].end.is_some());

        let assassination = world
            .events
            .values()
            .find(|e| e.kind == EventKind::Custom("assassination".to_string()))
            .expect("should have assassination event");
        let death = world
            .events
            .values()
            .find(|e| e.kind == EventKind::Death && e.caused_by == Some(assassination.id))
            .expect("should have caused death event");

        assert!(world.event_participants.iter().any(|p| {
            p.event_id == assassination.id
                && p.entity_id == actor_id
                && p.role == ParticipantRole::Instigator
        }));

        assert!(signals.iter().any(
            |s| matches!(s.kind, SignalKind::EntityDied { entity_id } if entity_id == target_id)
        ));

        let result = &world.action_results[0];
        assert!(matches!(
            &result.outcome,
            ActionOutcome::Success { event_id } if *event_id == assassination.id
        ));

        let _ = death;
    }

    #[test]
    fn scenario_assassinate_leader_emits_vacancy() {
        let mut s = Scenario::at_year(100);
        let actor_id = s.add_person_standalone("Dorian");
        s.make_player(actor_id);
        let target_id = s.add_person_standalone("King");
        let faction_id = s.add_faction("The Kingdom");
        s.make_leader(target_id, faction_id);
        let mut world = s.build();

        world.queue_action(Action {
            actor_id,
            source: ActionSource::Player,
            kind: ActionKind::Assassinate { target_id },
        });

        let signals = tick(&mut world);

        assert!(signals.iter().any(|s| matches!(
            &s.kind,
            SignalKind::LeaderVacancy {
                faction_id: fid,
                previous_leader_id: rid,
            } if *fid == faction_id && *rid == target_id
        )));

        let leader_rel = world.entities[&target_id]
            .relationships
            .iter()
            .find(|r| r.kind == RelationshipKind::LeaderOf && r.target_entity_id == faction_id)
            .expect("should still have the relationship record");
        assert!(leader_rel.end.is_some());
    }

    #[test]
    fn scenario_assassinate_invalid_target_fails() {
        let (mut world, actor_id) = testutil::action_scenario();

        world.queue_action(Action {
            actor_id,
            source: ActionSource::Player,
            kind: ActionKind::Assassinate { target_id: 99999 },
        });

        tick(&mut world);

        assert!(matches!(
            &world.action_results[0].outcome,
            ActionOutcome::Failed { reason } if reason.contains("does not exist")
        ));
    }

    #[test]
    fn scenario_assassinate_dead_target_fails() {
        let mut s = Scenario::at_year(100);
        let actor_id = s.add_person_standalone("Dorian");
        s.make_player(actor_id);
        let target_id = s.add_person_standalone("DeadGuy");
        s.end_entity(target_id);
        let mut world = s.build();

        world.queue_action(Action {
            actor_id,
            source: ActionSource::Player,
            kind: ActionKind::Assassinate { target_id },
        });

        tick(&mut world);

        assert!(matches!(
            &world.action_results[0].outcome,
            ActionOutcome::Failed { reason } if reason.contains("not a living person")
        ));
    }

    #[test]
    fn scenario_support_faction_boosts_properties() {
        let mut s = Scenario::at_year(100);
        let actor_id = s.add_person_standalone("Dorian");
        s.make_player(actor_id);
        let faction_id = add_test_faction(&mut s, "The Alliance");
        let mut world = s.build();

        world.queue_action(Action {
            actor_id,
            source: ActionSource::Player,
            kind: ActionKind::SupportFaction { faction_id },
        });

        tick(&mut world);

        let fd = testutil::get_faction(&world, faction_id);
        assert!(
            (fd.stability - 0.58).abs() < 1e-9,
            "expected stability ~0.58, got {}",
            fd.stability
        );
        assert!(
            (fd.happiness - 0.56).abs() < 1e-9,
            "expected happiness ~0.56, got {}",
            fd.happiness
        );

        assert!(matches!(
            &world.action_results[0].outcome,
            ActionOutcome::Success { .. }
        ));
    }

    #[test]
    fn scenario_undermine_faction_damages_properties() {
        let mut s = Scenario::at_year(100);
        let actor_id = s.add_person_standalone("Dorian");
        s.make_player(actor_id);
        let faction_id = add_test_faction(&mut s, "The Empire");
        let mut world = s.build();

        world.queue_action(Action {
            actor_id,
            source: ActionSource::Player,
            kind: ActionKind::UndermineFaction { faction_id },
        });

        tick(&mut world);

        let fd = testutil::get_faction(&world, faction_id);
        assert!(
            (fd.stability - 0.40).abs() < 1e-9,
            "expected stability ~0.40, got {}",
            fd.stability
        );
        assert!(
            (fd.happiness - 0.42).abs() < 1e-9,
            "expected happiness ~0.42, got {}",
            fd.happiness
        );
        assert!(
            (fd.legitimacy - 0.44).abs() < 1e-9,
            "expected legitimacy ~0.44, got {}",
            fd.legitimacy
        );
    }

    #[test]
    fn scenario_broker_alliance_creates_relationship() {
        let mut s = Scenario::at_year(100);
        let actor_id = s.add_person_standalone("Dorian");
        s.make_player(actor_id);
        let fa = s.add_faction("Faction A");
        let fb = s.add_faction("Faction B");
        let mut world = s.build();

        world.queue_action(Action {
            actor_id,
            source: ActionSource::Player,
            kind: ActionKind::BrokerAlliance {
                faction_a: fa,
                faction_b: fb,
            },
        });

        tick(&mut world);

        let a_allies: Vec<_> = world.entities[&fa]
            .active_rels(RelationshipKind::Ally)
            .collect();
        let b_allies: Vec<_> = world.entities[&fb]
            .active_rels(RelationshipKind::Ally)
            .collect();

        assert_eq!(a_allies.len(), 1);
        assert_eq!(a_allies[0], fb);
        assert_eq!(b_allies.len(), 1);
        assert_eq!(b_allies[0], fa);

        assert!(
            world
                .events
                .values()
                .any(|e| e.kind == EventKind::Custom("broker_alliance".to_string()))
        );

        assert!(matches!(
            &world.action_results[0].outcome,
            ActionOutcome::Success { .. }
        ));
    }

    #[test]
    fn scenario_broker_alliance_fails_if_enemies() {
        let mut s = Scenario::at_year(100);
        let actor_id = s.add_person_standalone("Dorian");
        s.make_player(actor_id);
        let fa = s.add_faction("Faction A");
        let fb = s.add_faction("Faction B");
        s.make_enemies(fa, fb);
        let mut world = s.build();

        world.queue_action(Action {
            actor_id,
            source: ActionSource::Player,
            kind: ActionKind::BrokerAlliance {
                faction_a: fa,
                faction_b: fb,
            },
        });

        tick(&mut world);

        assert!(matches!(
            &world.action_results[0].outcome,
            ActionOutcome::Failed { reason } if reason.contains("enemies")
        ));
    }

    #[test]
    fn scenario_broker_alliance_fails_if_already_allied() {
        let mut s = Scenario::at_year(100);
        let actor_id = s.add_person_standalone("Dorian");
        s.make_player(actor_id);
        let fa = s.add_faction("Faction A");
        let fb = s.add_faction("Faction B");
        s.make_allies(fa, fb);
        let mut world = s.build();

        world.queue_action(Action {
            actor_id,
            source: ActionSource::Player,
            kind: ActionKind::BrokerAlliance {
                faction_a: fa,
                faction_b: fb,
            },
        });

        tick(&mut world);

        assert!(matches!(
            &world.action_results[0].outcome,
            ActionOutcome::Failed { reason } if reason.contains("already allied")
        ));
    }

    #[test]
    fn scenario_actions_cleared_after_tick() {
        let mut s = Scenario::at_year(100);
        let actor_id = s.add_person_standalone("Dorian");
        s.make_player(actor_id);
        let fa = s.add_faction("Faction A");
        let fb = s.add_faction("Faction B");
        let mut world = s.build();

        world.queue_action(Action {
            actor_id,
            source: ActionSource::Player,
            kind: ActionKind::SupportFaction { faction_id: fa },
        });
        world.queue_action(Action {
            actor_id,
            source: ActionSource::Player,
            kind: ActionKind::SupportFaction { faction_id: fb },
        });

        assert_eq!(world.pending_actions.len(), 2);
        tick(&mut world);
        assert!(world.pending_actions.is_empty());
        assert_eq!(world.action_results.len(), 2);
    }

    #[test]
    fn scenario_causal_chain_traceable() {
        let mut s = Scenario::at_year(100);
        let actor_id = s.add_person_standalone("Dorian");
        s.make_player(actor_id);
        let target_id = s.add_person_standalone("Victim");
        let mut world = s.build();

        world.queue_action(Action {
            actor_id,
            source: ActionSource::Player,
            kind: ActionKind::Assassinate { target_id },
        });

        tick(&mut world);

        let death = world
            .events
            .values()
            .find(|e| e.kind == EventKind::Death && e.description.contains("was killed"))
            .expect("should have death event");

        let cause_id = death.caused_by.expect("death should have caused_by");
        let cause = &world.events[&cause_id];
        assert_eq!(cause.kind, EventKind::Custom("assassination".to_string()));

        assert!(world.event_participants.iter().any(|p| {
            p.event_id == cause.id
                && p.entity_id == actor_id
                && p.role == ParticipantRole::Instigator
        }));
    }

    #[test]
    fn scenario_action_result_includes_source() {
        let mut s = Scenario::at_year(100);
        let actor_id = s.add_person_standalone("Dorian");
        s.make_player(actor_id);
        let faction_id = s.add_faction("TestFaction");
        let mut world = s.build();

        world.queue_action(Action {
            actor_id,
            source: ActionSource::Player,
            kind: ActionKind::SupportFaction { faction_id },
        });

        tick(&mut world);

        let result = &world.action_results[0];
        assert_eq!(result.actor_id, actor_id);
        assert!(matches!(result.source, ActionSource::Player));
        assert!(matches!(result.outcome, ActionOutcome::Success { .. }));
    }

    #[test]
    fn scenario_action_result_includes_order_source() {
        let mut s = Scenario::at_year(100);
        let actor_id = s.add_person_standalone("Dorian");
        s.make_player(actor_id);
        let faction_id = s.add_faction("TestFaction");
        let commander_id = s.add_person_standalone("Commander");
        let mut world = s.build();

        world.queue_action(Action {
            actor_id,
            source: ActionSource::Order {
                ordered_by: commander_id,
            },
            kind: ActionKind::SupportFaction { faction_id },
        });

        tick(&mut world);

        let result = &world.action_results[0];
        assert_eq!(result.actor_id, actor_id);
        assert!(
            matches!(result.source, ActionSource::Order { ordered_by } if ordered_by == commander_id)
        );
    }

    #[test]
    fn scenario_event_data_contains_source() {
        let mut s = Scenario::at_year(100);
        let actor_id = s.add_person_standalone("Dorian");
        s.make_player(actor_id);
        let faction_id = s.add_faction("TestFaction");
        let mut world = s.build();

        world.queue_action(Action {
            actor_id,
            source: ActionSource::Player,
            kind: ActionKind::SupportFaction { faction_id },
        });

        tick(&mut world);

        let ev = world
            .events
            .values()
            .find(|e| e.kind == EventKind::Custom("faction_support".to_string()))
            .expect("should have faction_support event");

        assert_eq!(ev.data, serde_json::json!("player"));
    }

    #[test]
    fn scenario_defect_moves_npc_to_new_faction() {
        let mut s = Scenario::at_year(100);
        let from_faction = add_test_faction(&mut s, "Old Faction");
        let to_faction = s.add_faction("New Faction");
        let actor_id = s.add_person("Dorian", from_faction);
        s.make_player(actor_id);
        let mut world = s.build();

        world.queue_action(Action {
            actor_id,
            source: ActionSource::Autonomous,
            kind: ActionKind::Defect {
                from_faction,
                to_faction,
            },
        });

        tick(&mut world);

        assert!(matches!(
            &world.action_results[0].outcome,
            ActionOutcome::Success { .. }
        ));

        let old_member = world.entities[&actor_id]
            .relationships
            .iter()
            .find(|r| r.kind == RelationshipKind::MemberOf && r.target_entity_id == from_faction)
            .expect("should still have old membership record");
        assert!(old_member.end.is_some(), "old membership should be ended");

        assert!(
            world.entities[&actor_id].has_active_rel(RelationshipKind::MemberOf, to_faction),
            "should have new faction membership"
        );

        assert!(
            world
                .events
                .values()
                .any(|e| e.kind == EventKind::Custom("defection".to_string()))
        );

        let stability = testutil::get_faction(&world, from_faction).stability;
        assert!(
            (stability - 0.45).abs() < 1e-9,
            "old faction stability should drop: got {stability}"
        );
    }

    #[test]
    fn scenario_defect_as_leader_fails() {
        let mut s = Scenario::at_year(100);
        let from_faction = s.add_faction("Old Faction");
        let to_faction = s.add_faction("New Faction");
        let actor_id = s.add_person("Dorian", from_faction);
        s.make_player(actor_id);
        s.make_leader(actor_id, from_faction);
        let mut world = s.build();

        world.queue_action(Action {
            actor_id,
            source: ActionSource::Autonomous,
            kind: ActionKind::Defect {
                from_faction,
                to_faction,
            },
        });

        tick(&mut world);

        assert!(matches!(
            &world.action_results[0].outcome,
            ActionOutcome::Failed { reason } if reason.contains("leaders cannot defect")
        ));
    }

    #[test]
    fn scenario_seek_office_leaderless_auto_succeeds() {
        let mut s = Scenario::at_year(100);
        let faction_id = s.add_faction("Leaderless Faction");
        let actor_id = s.add_person("Dorian", faction_id);
        s.make_player(actor_id);
        let mut world = s.build();

        world.queue_action(Action {
            actor_id,
            source: ActionSource::Autonomous,
            kind: ActionKind::SeekOffice { faction_id },
        });

        tick(&mut world);

        assert!(matches!(
            &world.action_results[0].outcome,
            ActionOutcome::Success { .. }
        ));

        assert!(
            world.entities[&actor_id].has_active_rel(RelationshipKind::LeaderOf, faction_id),
            "actor should be leader after seeking office in leaderless faction"
        );

        let succession = world
            .events
            .values()
            .find(|e| e.kind == EventKind::Succession && e.description.contains("claimed"));
        assert!(succession.is_some(), "should have succession event");
    }

    #[test]
    fn scenario_seek_office_elective_probabilistic() {
        let mut s = Scenario::at_year(100);
        let faction_id = s.faction("Republic").government_type("elective").id();
        let actor_id = s.add_person("Dorian", faction_id);
        s.make_player(actor_id);
        let leader_id = s.add_person("Incumbent", faction_id);
        s.make_leader(leader_id, faction_id);
        let mut world = s.build();

        world.queue_action(Action {
            actor_id,
            source: ActionSource::Autonomous,
            kind: ActionKind::SeekOffice { faction_id },
        });

        tick(&mut world);

        let result = &world.action_results[0];
        match &result.outcome {
            ActionOutcome::Success { .. } => {
                assert!(
                    world.entities[&actor_id]
                        .has_active_rel(RelationshipKind::LeaderOf, faction_id),
                    "on success, actor should be leader"
                );
            }
            ActionOutcome::Failed { reason } => {
                assert!(
                    reason.contains("election attempt failed"),
                    "failure reason should be about election: {reason}"
                );
                assert!(
                    !world.entities[&actor_id]
                        .has_active_rel(RelationshipKind::LeaderOf, faction_id),
                    "on failure, actor should not be leader"
                );
            }
        }
    }
}
