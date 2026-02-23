use rand::Rng;

use super::context::TickContext;
use super::signal::{Signal, SignalKind};
use super::system::{SimSystem, TickFrequency};
use crate::model::action::{Action, ActionKind, ActionOutcome, ActionResult, ActionSource};
use crate::model::{EntityKind, EventKind, ParticipantRole, RelationshipKind, World};

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

    let actor_name = get_entity_name(ctx.world, actor_id);
    let target_name = get_entity_name(ctx.world, target_id);

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
    let leader_of_faction: Option<u64> = ctx.world.entities.get(&target_id).and_then(|e| {
        e.relationships
            .iter()
            .find(|r| r.kind == RelationshipKind::LeaderOf && r.end.is_none())
            .map(|r| r.target_entity_id)
    });

    // End all active relationships
    end_person_relationships(ctx.world, target_id, time, death_ev);

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

    let actor_name = get_entity_name(ctx.world, actor_id);
    let faction_name = get_entity_name(ctx.world, faction_id);

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
        fd.stability = (old_stab + 0.08).clamp(0.0, 1.0);
        fd.happiness = (old_hap + 0.06).clamp(0.0, 1.0);
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

    let actor_name = get_entity_name(ctx.world, actor_id);
    let faction_name = get_entity_name(ctx.world, faction_id);

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
        fd.stability = (old_stab - 0.10).clamp(0.0, 1.0);
        fd.happiness = (old_hap - 0.08).clamp(0.0, 1.0);
        fd.legitimacy = (old_leg - 0.06).clamp(0.0, 1.0);
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
        if has_active_rel_of_kind(ctx.world, faction_a, faction_b, &RelationshipKind::Ally) {
            return ActionOutcome::Failed {
                reason: "factions are already allied".to_string(),
            };
        }
        if has_active_rel_of_kind(ctx.world, faction_a, faction_b, &RelationshipKind::Enemy) {
            return ActionOutcome::Failed {
                reason: "factions are currently enemies".to_string(),
            };
        }
        return ActionOutcome::Failed {
            reason: "factions already have an active diplomatic relationship".to_string(),
        };
    }

    let actor_name = get_entity_name(ctx.world, actor_id);
    let name_a = get_entity_name(ctx.world, faction_a);
    let name_b = get_entity_name(ctx.world, faction_b);

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
    if has_active_rel_of_kind(
        ctx.world,
        actor_faction,
        target_faction_id,
        &RelationshipKind::AtWar,
    ) {
        return ActionOutcome::Failed {
            reason: "factions are already at war".to_string(),
        };
    }

    let attacker_name = get_entity_name(ctx.world, actor_faction);
    let defender_name = get_entity_name(ctx.world, target_faction_id);
    let actor_name = get_entity_name(ctx.world, actor_id);

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
        "war_start_year".to_string(),
        serde_json::json!(year),
        ev,
    );
    ctx.world.set_extra(
        target_faction_id,
        "war_start_year".to_string(),
        serde_json::json!(year),
        ev,
    );

    // End any active Ally relationship between them
    end_ally_between(ctx.world, actor_faction, target_faction_id, time, ev);

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
    let current_leader_id = find_faction_leader(ctx.world, faction_id);
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
            && e.relationships.iter().any(|r| {
                r.kind == RelationshipKind::MemberOf
                    && r.target_entity_id == faction_id
                    && r.end.is_none()
            })
        {
            let pop = e.data.as_settlement().map(|s| s.population).unwrap_or(0);
            able_bodied += pop / 4;
        }
    }
    let military = (able_bodied as f64 / 200.0).clamp(0.0, 1.0);
    let resistance = 0.2 + military * legitimacy * (0.3 + 0.7 * happiness);
    let noise: f64 = ctx.rng.random_range(-0.1..0.1);
    let coup_power = (0.2 + 0.3 * instability + noise).max(0.0);
    let success_chance = (coup_power / (coup_power + resistance)).clamp(0.1, 0.9);

    let actor_name = get_entity_name(ctx.world, actor_id);
    let leader_name = get_entity_name(ctx.world, leader_id);
    let faction_name = get_entity_name(ctx.world, faction_id);

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
            .end_relationship(leader_id, faction_id, &RelationshipKind::LeaderOf, time, ev);

        // New leader takes over
        ctx.world
            .add_relationship(actor_id, faction_id, RelationshipKind::LeaderOf, time, ev);

        // Post-coup stability hit
        let new_stability = (stability * 0.6).clamp(0.0, 1.0);
        let new_legitimacy = (legitimacy * 0.5 + 0.1).clamp(0.0, 1.0);
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

        if ctx.rng.random_bool(0.5) {
            // Instigator executed
            let death_ev = ctx.world.add_caused_event(
                EventKind::Death,
                time,
                format!("{actor_name} was executed after a failed coup in year {year}"),
                ev,
            );
            ctx.world
                .add_event_participant(death_ev, actor_id, ParticipantRole::Subject);
            end_person_relationships(ctx.world, actor_id, time, death_ev);
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
    let is_member = ctx.world.entities.get(&actor_id).is_some_and(|e| {
        e.relationships.iter().any(|r| {
            r.kind == RelationshipKind::MemberOf
                && r.target_entity_id == from_faction
                && r.end.is_none()
        })
    });
    if !is_member {
        return ActionOutcome::Failed {
            reason: "actor is not a member of the source faction".to_string(),
        };
    }

    // Leaders can't defect
    let is_leader = ctx.world.entities.get(&actor_id).is_some_and(|e| {
        e.relationships.iter().any(|r| {
            r.kind == RelationshipKind::LeaderOf
                && r.target_entity_id == from_faction
                && r.end.is_none()
        })
    });
    if is_leader {
        return ActionOutcome::Failed {
            reason: "leaders cannot defect".to_string(),
        };
    }

    let actor_name = get_entity_name(ctx.world, actor_id);
    let from_name = get_entity_name(ctx.world, from_faction);
    let to_name = get_entity_name(ctx.world, to_faction);

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
    ctx.world.end_relationship(
        actor_id,
        from_faction,
        &RelationshipKind::MemberOf,
        time,
        ev,
    );

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
                && e.relationships.iter().any(|r| {
                    r.kind == RelationshipKind::MemberOf
                        && r.target_entity_id == to_faction
                        && r.end.is_none()
                })
        })
        .map(|e| e.id);

    if let Some(settlement_id) = new_faction_settlement {
        // End old LocatedIn if any
        let old_location: Option<u64> = ctx.world.entities.get(&actor_id).and_then(|e| {
            e.relationships
                .iter()
                .find(|r| r.kind == RelationshipKind::LocatedIn && r.end.is_none())
                .map(|r| r.target_entity_id)
        });
        if let Some(old_loc) = old_location {
            ctx.world
                .end_relationship(actor_id, old_loc, &RelationshipKind::LocatedIn, time, ev);
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
        fd.stability = (old_stab - 0.05).clamp(0.0, 1.0);
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
    let is_member = ctx.world.entities.get(&actor_id).is_some_and(|e| {
        e.relationships.iter().any(|r| {
            r.kind == RelationshipKind::MemberOf
                && r.target_entity_id == faction_id
                && r.end.is_none()
        })
    });
    if !is_member {
        return ActionOutcome::Failed {
            reason: "actor is not a member of the faction".to_string(),
        };
    }

    // Check if already leader
    let is_already_leader = ctx.world.entities.get(&actor_id).is_some_and(|e| {
        e.relationships.iter().any(|r| {
            r.kind == RelationshipKind::LeaderOf
                && r.target_entity_id == faction_id
                && r.end.is_none()
        })
    });
    if is_already_leader {
        return ActionOutcome::Failed {
            reason: "actor is already the leader".to_string(),
        };
    }

    let current_leader_id = find_faction_leader(ctx.world, faction_id);
    let actor_name = get_entity_name(ctx.world, actor_id);
    let faction_name = get_entity_name(ctx.world, faction_id);

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
    let mut success_chance = 0.3;

    // Check if actor has Charismatic trait
    let has_charismatic = ctx.world.entities.get(&actor_id).is_some_and(|e| {
        crate::model::traits::has_trait(e, &crate::model::traits::Trait::Charismatic)
    });
    if has_charismatic {
        success_chance += 0.2;
    }

    // Instability bonus
    if stability < 0.5 {
        success_chance += 0.1 * ((0.5 - stability) / 0.5);
    }

    let leader_id = current_leader_id.unwrap();
    let leader_name = get_entity_name(ctx.world, leader_id);

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
            .end_relationship(leader_id, faction_id, &RelationshipKind::LeaderOf, time, ev);

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

fn find_faction_leader(world: &World, faction_id: u64) -> Option<u64> {
    world.entities.values().find_map(|e| {
        if e.kind == EntityKind::Person
            && e.end.is_none()
            && e.relationships.iter().any(|r| {
                r.kind == RelationshipKind::LeaderOf
                    && r.target_entity_id == faction_id
                    && r.end.is_none()
            })
        {
            Some(e.id)
        } else {
            None
        }
    })
}

fn find_actor_faction(world: &World, actor_id: u64) -> Option<u64> {
    world.entities.get(&actor_id).and_then(|e| {
        e.relationships
            .iter()
            .find(|r| {
                r.kind == RelationshipKind::MemberOf
                    && r.end.is_none()
                    && world
                        .entities
                        .get(&r.target_entity_id)
                        .is_some_and(|t| t.kind == EntityKind::Faction)
            })
            .map(|r| r.target_entity_id)
    })
}

fn end_ally_between(
    world: &mut World,
    a: u64,
    b: u64,
    time: crate::model::SimTimestamp,
    event_id: u64,
) {
    let has_a_to_b = world.entities.get(&a).is_some_and(|e| {
        e.relationships
            .iter()
            .any(|r| r.target_entity_id == b && r.kind == RelationshipKind::Ally && r.end.is_none())
    });
    if has_a_to_b {
        world.end_relationship(a, b, &RelationshipKind::Ally, time, event_id);
    }

    let has_b_to_a = world.entities.get(&b).is_some_and(|e| {
        e.relationships
            .iter()
            .any(|r| r.target_entity_id == a && r.kind == RelationshipKind::Ally && r.end.is_none())
    });
    if has_b_to_a {
        world.end_relationship(b, a, &RelationshipKind::Ally, time, event_id);
    }
}

// --- Helpers ---

fn get_entity_name(world: &World, entity_id: u64) -> String {
    world
        .entities
        .get(&entity_id)
        .map(|e| e.name.clone())
        .unwrap_or_else(|| format!("entity {entity_id}"))
}

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

fn end_person_relationships(
    world: &mut World,
    person_id: u64,
    time: crate::model::SimTimestamp,
    event_id: u64,
) {
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

fn has_active_rel_of_kind(world: &World, a: u64, b: u64, kind: &RelationshipKind) -> bool {
    let check = |source: u64, target: u64| -> bool {
        world.entities.get(&source).is_some_and(|e| {
            e.relationships
                .iter()
                .any(|r| r.target_entity_id == target && &r.kind == kind && r.end.is_none())
        })
    };
    check(a, b) || check(b, a)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::action::{Action, ActionKind, ActionOutcome, ActionSource};
    use crate::model::{SimTimestamp, World};
    use crate::sim::context::TickContext;
    use crate::sim::signal::Signal;
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    fn ts(year: u32) -> SimTimestamp {
        SimTimestamp::from_year(year)
    }

    /// Create a minimal world with an actor Person entity.
    /// Returns (world, actor_id).
    fn setup_world_with_actor() -> (World, u64) {
        use crate::model::EntityData;
        let mut world = World::new();
        world.current_time = ts(100);
        let ev = world.add_event(EventKind::Birth, ts(80), "Actor born".to_string());
        let actor_id = world.add_entity(
            EntityKind::Person,
            "Dorian".to_string(),
            Some(ts(80)),
            EntityData::default_for_kind(&EntityKind::Person),
            ev,
        );
        world.set_extra(
            actor_id,
            "is_player".to_string(),
            serde_json::json!(true),
            ev,
        );
        (world, actor_id)
    }

    /// Add a living Person target to the world. Returns target_id.
    fn add_person(world: &mut World, name: &str) -> u64 {
        use crate::model::EntityData;
        let ev = world.add_event(EventKind::Birth, ts(70), format!("{name} born"));
        world.add_entity(
            EntityKind::Person,
            name.to_string(),
            Some(ts(70)),
            EntityData::default_for_kind(&EntityKind::Person),
            ev,
        )
    }

    /// Add a living Faction to the world. Returns faction_id.
    fn add_faction(world: &mut World, name: &str) -> u64 {
        use crate::model::{EntityData, FactionData};
        let ev = world.add_event(EventKind::FactionFormed, ts(50), format!("{name} formed"));
        let fid = world.add_entity(
            EntityKind::Faction,
            name.to_string(),
            Some(ts(50)),
            EntityData::Faction(FactionData {
                government_type: "chieftain".to_string(),
                stability: 0.5,
                happiness: 0.5,
                legitimacy: 0.5,
                treasury: 0.0,
                alliance_strength: 0.0,
            }),
            ev,
        );
        fid
    }

    fn tick_system(world: &mut World) -> Vec<Signal> {
        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals = Vec::new();
        let mut system = ActionSystem;
        let mut ctx = TickContext {
            world,
            rng: &mut rng,
            signals: &mut signals,
            inbox: &[],
        };
        system.tick(&mut ctx);
        signals
    }

    #[test]
    fn assassinate_kills_target() {
        let (mut world, actor_id) = setup_world_with_actor();
        let target_id = add_person(&mut world, "Victim");

        world.queue_action(Action {
            actor_id,
            source: ActionSource::Player,
            kind: ActionKind::Assassinate { target_id },
        });

        let signals = tick_system(&mut world);

        // Target should be dead
        assert!(world.entities[&target_id].end.is_some());

        // Should have assassination and death events
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

        // Actor is Instigator on assassination
        assert!(world.event_participants.iter().any(|p| {
            p.event_id == assassination.id
                && p.entity_id == actor_id
                && p.role == ParticipantRole::Instigator
        }));

        // EntityDied signal emitted
        assert!(signals.iter().any(
            |s| matches!(s.kind, SignalKind::EntityDied { entity_id } if entity_id == target_id)
        ));

        // Result is success
        let result = &world.action_results[0];
        assert!(matches!(
            &result.outcome,
            ActionOutcome::Success { event_id } if *event_id == assassination.id
        ));

        let _ = death; // used above in find
    }

    #[test]
    fn assassinate_leader_emits_vacancy() {
        let (mut world, actor_id) = setup_world_with_actor();
        let target_id = add_person(&mut world, "King");
        let faction_id = add_faction(&mut world, "The Kingdom");

        // Make target the leader
        let ev = world.add_event(EventKind::Succession, ts(90), "Crowned".to_string());
        world.add_relationship(
            target_id,
            faction_id,
            RelationshipKind::LeaderOf,
            ts(90),
            ev,
        );

        world.queue_action(Action {
            actor_id,
            source: ActionSource::Player,
            kind: ActionKind::Assassinate { target_id },
        });

        let signals = tick_system(&mut world);

        // LeaderVacancy signal emitted
        assert!(signals.iter().any(|s| matches!(
            &s.kind,
            SignalKind::LeaderVacancy {
                faction_id: fid,
                previous_leader_id: rid,
            } if *fid == faction_id && *rid == target_id
        )));

        // LeaderOf relationship should be ended
        let leader_rel = world.entities[&target_id]
            .relationships
            .iter()
            .find(|r| r.kind == RelationshipKind::LeaderOf && r.target_entity_id == faction_id)
            .expect("should still have the relationship record");
        assert!(leader_rel.end.is_some());
    }

    #[test]
    fn assassinate_invalid_target_fails() {
        let (mut world, actor_id) = setup_world_with_actor();

        world.queue_action(Action {
            actor_id,
            source: ActionSource::Player,
            kind: ActionKind::Assassinate { target_id: 99999 },
        });

        tick_system(&mut world);

        assert!(matches!(
            &world.action_results[0].outcome,
            ActionOutcome::Failed { reason } if reason.contains("does not exist")
        ));
    }

    #[test]
    fn assassinate_dead_target_fails() {
        let (mut world, actor_id) = setup_world_with_actor();
        let target_id = add_person(&mut world, "DeadGuy");

        // Kill target first
        let ev = world.add_event(EventKind::Death, ts(95), "Already dead".to_string());
        world.end_entity(target_id, ts(95), ev);

        world.queue_action(Action {
            actor_id,
            source: ActionSource::Player,
            kind: ActionKind::Assassinate { target_id },
        });

        tick_system(&mut world);

        assert!(matches!(
            &world.action_results[0].outcome,
            ActionOutcome::Failed { reason } if reason.contains("not a living person")
        ));
    }

    #[test]
    fn support_faction_boosts_properties() {
        let (mut world, actor_id) = setup_world_with_actor();
        let faction_id = add_faction(&mut world, "The Alliance");

        world.queue_action(Action {
            actor_id,
            source: ActionSource::Player,
            kind: ActionKind::SupportFaction { faction_id },
        });

        tick_system(&mut world);

        let faction = &world.entities[&faction_id];
        let fd = faction.data.as_faction().unwrap();
        let stability = fd.stability;
        let happiness = fd.happiness;

        assert!(
            (stability - 0.58).abs() < 1e-9,
            "expected stability ~0.58, got {stability}"
        );
        assert!(
            (happiness - 0.56).abs() < 1e-9,
            "expected happiness ~0.56, got {happiness}"
        );

        assert!(matches!(
            &world.action_results[0].outcome,
            ActionOutcome::Success { .. }
        ));
    }

    #[test]
    fn undermine_faction_damages_properties() {
        let (mut world, actor_id) = setup_world_with_actor();
        let faction_id = add_faction(&mut world, "The Empire");

        world.queue_action(Action {
            actor_id,
            source: ActionSource::Player,
            kind: ActionKind::UndermineFaction { faction_id },
        });

        tick_system(&mut world);

        let faction = &world.entities[&faction_id];
        let fd = faction.data.as_faction().unwrap();
        let stability = fd.stability;
        let happiness = fd.happiness;
        let legitimacy = fd.legitimacy;

        assert!(
            (stability - 0.40).abs() < 1e-9,
            "expected stability ~0.40, got {stability}"
        );
        assert!(
            (happiness - 0.42).abs() < 1e-9,
            "expected happiness ~0.42, got {happiness}"
        );
        assert!(
            (legitimacy - 0.44).abs() < 1e-9,
            "expected legitimacy ~0.44, got {legitimacy}"
        );
    }

    #[test]
    fn broker_alliance_creates_relationship() {
        let (mut world, actor_id) = setup_world_with_actor();
        let fa = add_faction(&mut world, "Faction A");
        let fb = add_faction(&mut world, "Faction B");

        world.queue_action(Action {
            actor_id,
            source: ActionSource::Player,
            kind: ActionKind::BrokerAlliance {
                faction_a: fa,
                faction_b: fb,
            },
        });

        tick_system(&mut world);

        // Both should have Ally relationship to each other
        let a_allies: Vec<_> = world.entities[&fa]
            .relationships
            .iter()
            .filter(|r| r.kind == RelationshipKind::Ally && r.end.is_none())
            .collect();
        let b_allies: Vec<_> = world.entities[&fb]
            .relationships
            .iter()
            .filter(|r| r.kind == RelationshipKind::Ally && r.end.is_none())
            .collect();

        assert_eq!(a_allies.len(), 1);
        assert_eq!(a_allies[0].target_entity_id, fb);
        assert_eq!(b_allies.len(), 1);
        assert_eq!(b_allies[0].target_entity_id, fa);

        // Event created
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
    fn broker_alliance_fails_if_enemies() {
        let (mut world, actor_id) = setup_world_with_actor();
        let fa = add_faction(&mut world, "Faction A");
        let fb = add_faction(&mut world, "Faction B");

        // Make them enemies
        let ev = world.add_event(
            EventKind::Custom("rivalry".to_string()),
            ts(90),
            "Became rivals".to_string(),
        );
        world.add_relationship(fa, fb, RelationshipKind::Enemy, ts(90), ev);

        world.queue_action(Action {
            actor_id,
            source: ActionSource::Player,
            kind: ActionKind::BrokerAlliance {
                faction_a: fa,
                faction_b: fb,
            },
        });

        tick_system(&mut world);

        assert!(matches!(
            &world.action_results[0].outcome,
            ActionOutcome::Failed { reason } if reason.contains("enemies")
        ));
    }

    #[test]
    fn broker_alliance_fails_if_already_allied() {
        let (mut world, actor_id) = setup_world_with_actor();
        let fa = add_faction(&mut world, "Faction A");
        let fb = add_faction(&mut world, "Faction B");

        // Make them allies
        let ev = world.add_event(EventKind::Treaty, ts(90), "Allied".to_string());
        world.add_relationship(fa, fb, RelationshipKind::Ally, ts(90), ev);

        world.queue_action(Action {
            actor_id,
            source: ActionSource::Player,
            kind: ActionKind::BrokerAlliance {
                faction_a: fa,
                faction_b: fb,
            },
        });

        tick_system(&mut world);

        assert!(matches!(
            &world.action_results[0].outcome,
            ActionOutcome::Failed { reason } if reason.contains("already allied")
        ));
    }

    #[test]
    fn actions_cleared_after_tick() {
        let (mut world, actor_id) = setup_world_with_actor();
        let fa = add_faction(&mut world, "Faction A");
        let fb = add_faction(&mut world, "Faction B");

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
        tick_system(&mut world);
        assert!(world.pending_actions.is_empty());
        assert_eq!(world.action_results.len(), 2);
    }

    #[test]
    fn causal_chain_traceable() {
        let (mut world, actor_id) = setup_world_with_actor();
        let target_id = add_person(&mut world, "Victim");

        world.queue_action(Action {
            actor_id,
            source: ActionSource::Player,
            kind: ActionKind::Assassinate { target_id },
        });

        tick_system(&mut world);

        // Find the death event
        let death = world
            .events
            .values()
            .find(|e| e.kind == EventKind::Death && e.description.contains("was killed"))
            .expect("should have death event");

        // Walk the causal chain back
        let cause_id = death.caused_by.expect("death should have caused_by");
        let cause = &world.events[&cause_id];
        assert_eq!(cause.kind, EventKind::Custom("assassination".to_string()));

        // Actor is Instigator on the root cause
        assert!(world.event_participants.iter().any(|p| {
            p.event_id == cause.id
                && p.entity_id == actor_id
                && p.role == ParticipantRole::Instigator
        }));
    }

    #[test]
    fn action_result_includes_source() {
        let (mut world, actor_id) = setup_world_with_actor();
        let faction_id = add_faction(&mut world, "TestFaction");

        world.queue_action(Action {
            actor_id,
            source: ActionSource::Player,
            kind: ActionKind::SupportFaction { faction_id },
        });

        tick_system(&mut world);

        let result = &world.action_results[0];
        assert_eq!(result.actor_id, actor_id);
        assert!(matches!(result.source, ActionSource::Player));
        assert!(matches!(result.outcome, ActionOutcome::Success { .. }));
    }

    #[test]
    fn action_result_includes_order_source() {
        let (mut world, actor_id) = setup_world_with_actor();
        let faction_id = add_faction(&mut world, "TestFaction");
        let commander_id = add_person(&mut world, "Commander");

        world.queue_action(Action {
            actor_id,
            source: ActionSource::Order {
                ordered_by: commander_id,
            },
            kind: ActionKind::SupportFaction { faction_id },
        });

        tick_system(&mut world);

        let result = &world.action_results[0];
        assert_eq!(result.actor_id, actor_id);
        assert!(
            matches!(result.source, ActionSource::Order { ordered_by } if ordered_by == commander_id)
        );
    }

    #[test]
    fn event_data_contains_source() {
        let (mut world, actor_id) = setup_world_with_actor();
        let faction_id = add_faction(&mut world, "TestFaction");

        world.queue_action(Action {
            actor_id,
            source: ActionSource::Player,
            kind: ActionKind::SupportFaction { faction_id },
        });

        tick_system(&mut world);

        // Find the faction_support event
        let ev = world
            .events
            .values()
            .find(|e| e.kind == EventKind::Custom("faction_support".to_string()))
            .expect("should have faction_support event");

        assert_eq!(ev.data, serde_json::json!("player"));
    }

    #[test]
    fn defect_moves_npc_to_new_faction() {
        let (mut world, actor_id) = setup_world_with_actor();
        let from_faction = add_faction(&mut world, "Old Faction");
        let to_faction = add_faction(&mut world, "New Faction");

        // Make actor a member of from_faction
        let ev = world.add_event(EventKind::Joined, ts(90), "Joined".to_string());
        world.add_relationship(
            actor_id,
            from_faction,
            RelationshipKind::MemberOf,
            ts(90),
            ev,
        );

        world.queue_action(Action {
            actor_id,
            source: ActionSource::Autonomous,
            kind: ActionKind::Defect {
                from_faction,
                to_faction,
            },
        });

        tick_system(&mut world);

        assert!(matches!(
            &world.action_results[0].outcome,
            ActionOutcome::Success { .. }
        ));

        // Old MemberOf should be ended
        let old_member = world.entities[&actor_id]
            .relationships
            .iter()
            .find(|r| r.kind == RelationshipKind::MemberOf && r.target_entity_id == from_faction)
            .expect("should still have old membership record");
        assert!(old_member.end.is_some(), "old membership should be ended");

        // New MemberOf should be active
        let new_member = world.entities[&actor_id].relationships.iter().find(|r| {
            r.kind == RelationshipKind::MemberOf
                && r.target_entity_id == to_faction
                && r.end.is_none()
        });
        assert!(new_member.is_some(), "should have new faction membership");

        // Defection event should exist
        assert!(
            world
                .events
                .values()
                .any(|e| e.kind == EventKind::Custom("defection".to_string()))
        );

        // Stability hit on old faction
        let stability = world.entities[&from_faction]
            .data
            .as_faction()
            .unwrap()
            .stability;
        assert!(
            (stability - 0.45).abs() < 1e-9,
            "old faction stability should drop: got {stability}"
        );
    }

    #[test]
    fn defect_as_leader_fails() {
        let (mut world, actor_id) = setup_world_with_actor();
        let from_faction = add_faction(&mut world, "Old Faction");
        let to_faction = add_faction(&mut world, "New Faction");

        // Make actor a member and leader of from_faction
        let ev = world.add_event(EventKind::Joined, ts(90), "Joined".to_string());
        world.add_relationship(
            actor_id,
            from_faction,
            RelationshipKind::MemberOf,
            ts(90),
            ev,
        );
        let lev = world.add_event(EventKind::Succession, ts(90), "Led".to_string());
        world.add_relationship(
            actor_id,
            from_faction,
            RelationshipKind::LeaderOf,
            ts(90),
            lev,
        );

        world.queue_action(Action {
            actor_id,
            source: ActionSource::Autonomous,
            kind: ActionKind::Defect {
                from_faction,
                to_faction,
            },
        });

        tick_system(&mut world);

        assert!(matches!(
            &world.action_results[0].outcome,
            ActionOutcome::Failed { reason } if reason.contains("leaders cannot defect")
        ));
    }

    #[test]
    fn seek_office_leaderless_auto_succeeds() {
        let (mut world, actor_id) = setup_world_with_actor();
        let faction_id = add_faction(&mut world, "Leaderless Faction");

        // Make actor a member
        let ev = world.add_event(EventKind::Joined, ts(90), "Joined".to_string());
        world.add_relationship(actor_id, faction_id, RelationshipKind::MemberOf, ts(90), ev);

        world.queue_action(Action {
            actor_id,
            source: ActionSource::Autonomous,
            kind: ActionKind::SeekOffice { faction_id },
        });

        tick_system(&mut world);

        assert!(matches!(
            &world.action_results[0].outcome,
            ActionOutcome::Success { .. }
        ));

        // Actor should now be leader
        let is_leader = world.entities[&actor_id].relationships.iter().any(|r| {
            r.kind == RelationshipKind::LeaderOf
                && r.target_entity_id == faction_id
                && r.end.is_none()
        });
        assert!(
            is_leader,
            "actor should be leader after seeking office in leaderless faction"
        );

        // Succession event should exist
        let succession = world
            .events
            .values()
            .find(|e| e.kind == EventKind::Succession && e.description.contains("claimed"));
        assert!(succession.is_some(), "should have succession event");
    }

    #[test]
    fn seek_office_elective_probabilistic() {
        let (mut world, actor_id) = setup_world_with_actor();
        let faction_id = add_faction(&mut world, "Republic");

        // Set government type to elective
        world
            .entities
            .get_mut(&faction_id)
            .unwrap()
            .data
            .as_faction_mut()
            .unwrap()
            .government_type = "elective".to_string();

        // Make actor a member
        let ev = world.add_event(EventKind::Joined, ts(90), "Joined".to_string());
        world.add_relationship(actor_id, faction_id, RelationshipKind::MemberOf, ts(90), ev);

        // Add current leader
        let leader_id = add_person(&mut world, "Incumbent");
        let lev = world.add_event(EventKind::Joined, ts(80), "Joined".to_string());
        world.add_relationship(
            leader_id,
            faction_id,
            RelationshipKind::MemberOf,
            ts(80),
            lev,
        );
        let lev2 = world.add_event(EventKind::Succession, ts(80), "Elected".to_string());
        world.add_relationship(
            leader_id,
            faction_id,
            RelationshipKind::LeaderOf,
            ts(80),
            lev2,
        );

        world.queue_action(Action {
            actor_id,
            source: ActionSource::Autonomous,
            kind: ActionKind::SeekOffice { faction_id },
        });

        tick_system(&mut world);

        // With seed 42, it's probabilistic — just verify we get a valid result
        let result = &world.action_results[0];
        match &result.outcome {
            ActionOutcome::Success { .. } => {
                // Actor should be new leader
                let is_leader = world.entities[&actor_id].relationships.iter().any(|r| {
                    r.kind == RelationshipKind::LeaderOf
                        && r.target_entity_id == faction_id
                        && r.end.is_none()
                });
                assert!(is_leader, "on success, actor should be leader");
            }
            ActionOutcome::Failed { reason } => {
                assert!(
                    reason.contains("election attempt failed"),
                    "failure reason should be about election: {reason}"
                );
                // Actor should NOT be leader
                let is_leader = world.entities[&actor_id].relationships.iter().any(|r| {
                    r.kind == RelationshipKind::LeaderOf
                        && r.target_entity_id == faction_id
                        && r.end.is_none()
                });
                assert!(!is_leader, "on failure, actor should not be leader");
            }
        }
    }
}
