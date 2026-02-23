use super::context::TickContext;
use super::signal::{Signal, SignalKind};
use super::system::{SimSystem, TickFrequency};
use crate::model::action::{ActionKind, ActionResult, PlayerAction};
use crate::model::{EntityKind, EventKind, ParticipantRole, RelationshipKind, World};

pub struct PlayerActionSystem;

impl SimSystem for PlayerActionSystem {
    fn name(&self) -> &str {
        "player_actions"
    }

    fn frequency(&self) -> TickFrequency {
        TickFrequency::Yearly
    }

    fn tick(&mut self, ctx: &mut TickContext) {
        let actions: Vec<PlayerAction> = std::mem::take(&mut ctx.world.pending_actions);

        for action in actions {
            let result = match action.kind {
                ActionKind::Assassinate { target_id } => {
                    process_assassinate(ctx, action.player_id, target_id)
                }
                ActionKind::SupportFaction { faction_id } => {
                    process_support_faction(ctx, action.player_id, faction_id)
                }
                ActionKind::UndermineFaction { faction_id } => {
                    process_undermine_faction(ctx, action.player_id, faction_id)
                }
                ActionKind::BrokerAlliance {
                    faction_a,
                    faction_b,
                } => process_broker_alliance(ctx, action.player_id, faction_a, faction_b),
            };
            ctx.world.action_results.push(result);
        }
    }
}

fn process_assassinate(ctx: &mut TickContext, player_id: u64, target_id: u64) -> ActionResult {
    let time = ctx.world.current_time;
    let year = time.year();

    // Validate target exists and is a living person
    let target_valid = ctx
        .world
        .entities
        .get(&target_id)
        .is_some_and(|e| e.kind == EntityKind::Person && e.end.is_none());
    if !target_valid {
        return ActionResult::Failed {
            reason: format!("target {target_id} does not exist or is not a living person"),
        };
    }

    let player_name = get_entity_name(ctx.world, player_id);
    let target_name = get_entity_name(ctx.world, target_id);

    // Create assassination event
    let assassination_ev = ctx.world.add_event(
        EventKind::Custom("player_assassination".to_string()),
        time,
        format!("{player_name} assassinated {target_name} in year {year}"),
    );
    ctx.world
        .add_event_participant(assassination_ev, player_id, ParticipantRole::Instigator);
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

    // Check if target was a ruler before ending relationships
    let ruler_of_faction: Option<u64> = ctx.world.entities.get(&target_id).and_then(|e| {
        e.relationships
            .iter()
            .find(|r| r.kind == RelationshipKind::RulerOf && r.end.is_none())
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

    // If target was a ruler, emit RulerVacancy signal
    if let Some(faction_id) = ruler_of_faction {
        ctx.signals.push(Signal {
            event_id: death_ev,
            kind: SignalKind::RulerVacancy {
                faction_id,
                previous_ruler_id: target_id,
            },
        });
    }

    ActionResult::Success {
        event_id: assassination_ev,
    }
}

fn process_support_faction(ctx: &mut TickContext, player_id: u64, faction_id: u64) -> ActionResult {
    let time = ctx.world.current_time;
    let year = time.year();

    let faction_valid = ctx
        .world
        .entities
        .get(&faction_id)
        .is_some_and(|e| e.kind == EntityKind::Faction && e.end.is_none());
    if !faction_valid {
        return ActionResult::Failed {
            reason: format!("faction {faction_id} does not exist or is not a living faction"),
        };
    }

    let player_name = get_entity_name(ctx.world, player_id);
    let faction_name = get_entity_name(ctx.world, faction_id);

    let ev = ctx.world.add_event(
        EventKind::Custom("player_support".to_string()),
        time,
        format!("{player_name} bolstered {faction_name} in year {year}"),
    );
    ctx.world
        .add_event_participant(ev, player_id, ParticipantRole::Instigator);
    ctx.world
        .add_event_participant(ev, faction_id, ParticipantRole::Object);

    // Apply boosts
    let stability = get_f64_property(ctx.world, faction_id, "stability", 0.5);
    let happiness = get_f64_property(ctx.world, faction_id, "happiness", 0.5);

    ctx.world.set_property(
        faction_id,
        "stability".to_string(),
        serde_json::json!((stability + 0.08).clamp(0.0, 1.0)),
        ev,
    );
    ctx.world.set_property(
        faction_id,
        "happiness".to_string(),
        serde_json::json!((happiness + 0.06).clamp(0.0, 1.0)),
        ev,
    );

    ActionResult::Success { event_id: ev }
}

fn process_undermine_faction(
    ctx: &mut TickContext,
    player_id: u64,
    faction_id: u64,
) -> ActionResult {
    let time = ctx.world.current_time;
    let year = time.year();

    let faction_valid = ctx
        .world
        .entities
        .get(&faction_id)
        .is_some_and(|e| e.kind == EntityKind::Faction && e.end.is_none());
    if !faction_valid {
        return ActionResult::Failed {
            reason: format!("faction {faction_id} does not exist or is not a living faction"),
        };
    }

    let player_name = get_entity_name(ctx.world, player_id);
    let faction_name = get_entity_name(ctx.world, faction_id);

    let ev = ctx.world.add_event(
        EventKind::Custom("player_undermine".to_string()),
        time,
        format!("{player_name} undermined {faction_name} in year {year}"),
    );
    ctx.world
        .add_event_participant(ev, player_id, ParticipantRole::Instigator);
    ctx.world
        .add_event_participant(ev, faction_id, ParticipantRole::Object);

    // Apply penalties
    let stability = get_f64_property(ctx.world, faction_id, "stability", 0.5);
    let happiness = get_f64_property(ctx.world, faction_id, "happiness", 0.5);
    let legitimacy = get_f64_property(ctx.world, faction_id, "legitimacy", 0.5);

    ctx.world.set_property(
        faction_id,
        "stability".to_string(),
        serde_json::json!((stability - 0.10).clamp(0.0, 1.0)),
        ev,
    );
    ctx.world.set_property(
        faction_id,
        "happiness".to_string(),
        serde_json::json!((happiness - 0.08).clamp(0.0, 1.0)),
        ev,
    );
    ctx.world.set_property(
        faction_id,
        "legitimacy".to_string(),
        serde_json::json!((legitimacy - 0.06).clamp(0.0, 1.0)),
        ev,
    );

    ActionResult::Success { event_id: ev }
}

fn process_broker_alliance(
    ctx: &mut TickContext,
    player_id: u64,
    faction_a: u64,
    faction_b: u64,
) -> ActionResult {
    let time = ctx.world.current_time;
    let year = time.year();

    if faction_a == faction_b {
        return ActionResult::Failed {
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
        return ActionResult::Failed {
            reason: "one or both factions do not exist or are not alive".to_string(),
        };
    }

    // Check for existing relationships between the two
    let has_existing = has_active_rel_between(ctx.world, faction_a, faction_b);
    if has_existing {
        // Determine if allied or enemies for better error message
        if has_active_rel_of_kind(ctx.world, faction_a, faction_b, &RelationshipKind::Ally) {
            return ActionResult::Failed {
                reason: "factions are already allied".to_string(),
            };
        }
        if has_active_rel_of_kind(ctx.world, faction_a, faction_b, &RelationshipKind::Enemy) {
            return ActionResult::Failed {
                reason: "factions are currently enemies".to_string(),
            };
        }
        return ActionResult::Failed {
            reason: "factions already have an active diplomatic relationship".to_string(),
        };
    }

    let player_name = get_entity_name(ctx.world, player_id);
    let name_a = get_entity_name(ctx.world, faction_a);
    let name_b = get_entity_name(ctx.world, faction_b);

    let ev = ctx.world.add_event(
        EventKind::Custom("player_broker_alliance".to_string()),
        time,
        format!("{player_name} brokered an alliance between {name_a} and {name_b} in year {year}"),
    );
    ctx.world
        .add_event_participant(ev, player_id, ParticipantRole::Instigator);
    ctx.world
        .add_event_participant(ev, faction_a, ParticipantRole::Subject);
    ctx.world
        .add_event_participant(ev, faction_b, ParticipantRole::Object);

    // Add bidirectional Ally relationships
    ctx.world
        .add_relationship(faction_a, faction_b, RelationshipKind::Ally, time, ev);
    ctx.world
        .add_relationship(faction_b, faction_a, RelationshipKind::Ally, time, ev);

    ActionResult::Success { event_id: ev }
}

// --- Helpers ---

fn get_entity_name(world: &World, entity_id: u64) -> String {
    world
        .entities
        .get(&entity_id)
        .map(|e| e.name.clone())
        .unwrap_or_else(|| format!("entity {entity_id}"))
}

fn get_f64_property(world: &World, entity_id: u64, key: &str, default: f64) -> f64 {
    world
        .entities
        .get(&entity_id)
        .and_then(|e| e.properties.get(key))
        .and_then(|v| v.as_f64())
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
                && matches!(r.kind, RelationshipKind::Ally | RelationshipKind::Enemy)
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
    use crate::model::action::{ActionKind, PlayerAction};
    use crate::model::{SimTimestamp, World};
    use crate::sim::context::TickContext;
    use crate::sim::signal::Signal;
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    fn ts(year: u32) -> SimTimestamp {
        SimTimestamp::from_year(year)
    }

    /// Create a minimal world with a player Person entity.
    /// Returns (world, player_id).
    fn setup_world_with_player() -> (World, u64) {
        let mut world = World::new();
        world.current_time = ts(100);
        let ev = world.add_event(EventKind::Birth, ts(80), "Player born".to_string());
        let player_id =
            world.add_entity(EntityKind::Person, "Dorian".to_string(), Some(ts(80)), ev);
        world.set_property(
            player_id,
            "is_player".to_string(),
            serde_json::json!(true),
            ev,
        );
        (world, player_id)
    }

    /// Add a living Person target to the world. Returns target_id.
    fn add_person(world: &mut World, name: &str) -> u64 {
        let ev = world.add_event(EventKind::Birth, ts(70), format!("{name} born"));
        world.add_entity(EntityKind::Person, name.to_string(), Some(ts(70)), ev)
    }

    /// Add a living Faction to the world. Returns faction_id.
    fn add_faction(world: &mut World, name: &str) -> u64 {
        let ev = world.add_event(EventKind::FactionFormed, ts(50), format!("{name} formed"));
        let fid = world.add_entity(EntityKind::Faction, name.to_string(), Some(ts(50)), ev);
        world.set_property(fid, "stability".to_string(), serde_json::json!(0.5), ev);
        world.set_property(fid, "happiness".to_string(), serde_json::json!(0.5), ev);
        world.set_property(fid, "legitimacy".to_string(), serde_json::json!(0.5), ev);
        fid
    }

    fn tick_system(world: &mut World) -> Vec<Signal> {
        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals = Vec::new();
        let mut system = PlayerActionSystem;
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
        let (mut world, player_id) = setup_world_with_player();
        let target_id = add_person(&mut world, "Victim");

        world.queue_action(PlayerAction {
            player_id,
            kind: ActionKind::Assassinate { target_id },
        });

        let signals = tick_system(&mut world);

        // Target should be dead
        assert!(world.entities[&target_id].end.is_some());

        // Should have assassination and death events
        let assassination = world
            .events
            .values()
            .find(|e| e.kind == EventKind::Custom("player_assassination".to_string()))
            .expect("should have assassination event");
        let death = world
            .events
            .values()
            .find(|e| e.kind == EventKind::Death && e.caused_by == Some(assassination.id))
            .expect("should have caused death event");

        // Player is Instigator on assassination
        assert!(world.event_participants.iter().any(|p| {
            p.event_id == assassination.id
                && p.entity_id == player_id
                && p.role == ParticipantRole::Instigator
        }));

        // EntityDied signal emitted
        assert!(signals.iter().any(
            |s| matches!(s.kind, SignalKind::EntityDied { entity_id } if entity_id == target_id)
        ));

        // Result is success
        assert!(matches!(
            &world.action_results[0],
            ActionResult::Success { event_id } if *event_id == assassination.id
        ));

        let _ = death; // used above in find
    }

    #[test]
    fn assassinate_ruler_emits_vacancy() {
        let (mut world, player_id) = setup_world_with_player();
        let target_id = add_person(&mut world, "King");
        let faction_id = add_faction(&mut world, "The Kingdom");

        // Make target the ruler
        let ev = world.add_event(EventKind::Succession, ts(90), "Crowned".to_string());
        world.add_relationship(target_id, faction_id, RelationshipKind::RulerOf, ts(90), ev);

        world.queue_action(PlayerAction {
            player_id,
            kind: ActionKind::Assassinate { target_id },
        });

        let signals = tick_system(&mut world);

        // RulerVacancy signal emitted
        assert!(signals.iter().any(|s| matches!(
            &s.kind,
            SignalKind::RulerVacancy {
                faction_id: fid,
                previous_ruler_id: rid,
            } if *fid == faction_id && *rid == target_id
        )));

        // RulerOf relationship should be ended
        let ruler_rel = world.entities[&target_id]
            .relationships
            .iter()
            .find(|r| r.kind == RelationshipKind::RulerOf && r.target_entity_id == faction_id)
            .expect("should still have the relationship record");
        assert!(ruler_rel.end.is_some());
    }

    #[test]
    fn assassinate_invalid_target_fails() {
        let (mut world, player_id) = setup_world_with_player();

        world.queue_action(PlayerAction {
            player_id,
            kind: ActionKind::Assassinate { target_id: 99999 },
        });

        tick_system(&mut world);

        assert!(matches!(
            &world.action_results[0],
            ActionResult::Failed { reason } if reason.contains("does not exist")
        ));
    }

    #[test]
    fn assassinate_dead_target_fails() {
        let (mut world, player_id) = setup_world_with_player();
        let target_id = add_person(&mut world, "DeadGuy");

        // Kill target first
        let ev = world.add_event(EventKind::Death, ts(95), "Already dead".to_string());
        world.end_entity(target_id, ts(95), ev);

        world.queue_action(PlayerAction {
            player_id,
            kind: ActionKind::Assassinate { target_id },
        });

        tick_system(&mut world);

        assert!(matches!(
            &world.action_results[0],
            ActionResult::Failed { reason } if reason.contains("not a living person")
        ));
    }

    #[test]
    fn support_faction_boosts_properties() {
        let (mut world, player_id) = setup_world_with_player();
        let faction_id = add_faction(&mut world, "The Alliance");

        world.queue_action(PlayerAction {
            player_id,
            kind: ActionKind::SupportFaction { faction_id },
        });

        tick_system(&mut world);

        let faction = &world.entities[&faction_id];
        let stability = faction.properties["stability"].as_f64().unwrap();
        let happiness = faction.properties["happiness"].as_f64().unwrap();

        assert!(
            (stability - 0.58).abs() < 1e-9,
            "expected stability ~0.58, got {stability}"
        );
        assert!(
            (happiness - 0.56).abs() < 1e-9,
            "expected happiness ~0.56, got {happiness}"
        );

        assert!(matches!(
            &world.action_results[0],
            ActionResult::Success { .. }
        ));
    }

    #[test]
    fn undermine_faction_damages_properties() {
        let (mut world, player_id) = setup_world_with_player();
        let faction_id = add_faction(&mut world, "The Empire");

        world.queue_action(PlayerAction {
            player_id,
            kind: ActionKind::UndermineFaction { faction_id },
        });

        tick_system(&mut world);

        let faction = &world.entities[&faction_id];
        let stability = faction.properties["stability"].as_f64().unwrap();
        let happiness = faction.properties["happiness"].as_f64().unwrap();
        let legitimacy = faction.properties["legitimacy"].as_f64().unwrap();

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
        let (mut world, player_id) = setup_world_with_player();
        let fa = add_faction(&mut world, "Faction A");
        let fb = add_faction(&mut world, "Faction B");

        world.queue_action(PlayerAction {
            player_id,
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
                .any(|e| e.kind == EventKind::Custom("player_broker_alliance".to_string()))
        );

        assert!(matches!(
            &world.action_results[0],
            ActionResult::Success { .. }
        ));
    }

    #[test]
    fn broker_alliance_fails_if_enemies() {
        let (mut world, player_id) = setup_world_with_player();
        let fa = add_faction(&mut world, "Faction A");
        let fb = add_faction(&mut world, "Faction B");

        // Make them enemies
        let ev = world.add_event(
            EventKind::Custom("rivalry".to_string()),
            ts(90),
            "Became rivals".to_string(),
        );
        world.add_relationship(fa, fb, RelationshipKind::Enemy, ts(90), ev);

        world.queue_action(PlayerAction {
            player_id,
            kind: ActionKind::BrokerAlliance {
                faction_a: fa,
                faction_b: fb,
            },
        });

        tick_system(&mut world);

        assert!(matches!(
            &world.action_results[0],
            ActionResult::Failed { reason } if reason.contains("enemies")
        ));
    }

    #[test]
    fn broker_alliance_fails_if_already_allied() {
        let (mut world, player_id) = setup_world_with_player();
        let fa = add_faction(&mut world, "Faction A");
        let fb = add_faction(&mut world, "Faction B");

        // Make them allies
        let ev = world.add_event(EventKind::Treaty, ts(90), "Allied".to_string());
        world.add_relationship(fa, fb, RelationshipKind::Ally, ts(90), ev);

        world.queue_action(PlayerAction {
            player_id,
            kind: ActionKind::BrokerAlliance {
                faction_a: fa,
                faction_b: fb,
            },
        });

        tick_system(&mut world);

        assert!(matches!(
            &world.action_results[0],
            ActionResult::Failed { reason } if reason.contains("already allied")
        ));
    }

    #[test]
    fn actions_cleared_after_tick() {
        let (mut world, player_id) = setup_world_with_player();
        let fa = add_faction(&mut world, "Faction A");
        let fb = add_faction(&mut world, "Faction B");

        world.queue_action(PlayerAction {
            player_id,
            kind: ActionKind::SupportFaction { faction_id: fa },
        });
        world.queue_action(PlayerAction {
            player_id,
            kind: ActionKind::SupportFaction { faction_id: fb },
        });

        assert_eq!(world.pending_actions.len(), 2);
        tick_system(&mut world);
        assert!(world.pending_actions.is_empty());
        assert_eq!(world.action_results.len(), 2);
    }

    #[test]
    fn causal_chain_traceable() {
        let (mut world, player_id) = setup_world_with_player();
        let target_id = add_person(&mut world, "Victim");

        world.queue_action(PlayerAction {
            player_id,
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
        assert_eq!(
            cause.kind,
            EventKind::Custom("player_assassination".to_string())
        );

        // Verify player is Instigator on the root cause
        assert!(world.event_participants.iter().any(|p| {
            p.event_id == cause.id
                && p.entity_id == player_id
                && p.role == ParticipantRole::Instigator
        }));
    }
}
