use bevy_ecs::entity::Entity;
use bevy_ecs::message::Messages;
use bevy_ecs::world::World;

use crate::ecs::clock::SimClock;
use crate::ecs::events::SimReactiveEvent;
use crate::ecs::relationships::RelationshipGraph;
use crate::ecs::resources::event_log::EcsEvent;
use crate::ecs::resources::{EcsIdGenerator, EventLog, SimEntityMap};
use crate::ecs::time::SimTime;
use crate::model::effect::{EventEffect, StateChange};
use crate::model::event::EventParticipant;

use super::{SimCommand, SimCommandKind};
use super::apply_demographics;
use super::apply_lifecycle;
use super::apply_military;
use super::apply_relationship;
use super::apply_set_field;

/// Context passed to all `apply_*` sub-functions, providing mutable access
/// to the resources they need without requiring direct World access.
pub(crate) struct ApplyCtx {
    pub event_log: EventLog,
    pub id_gen: EcsIdGenerator,
    pub entity_map: SimEntityMap,
    pub rel_graph: RelationshipGraph,
    pub clock_time: SimTime,
    pub reactive_events: Vec<SimReactiveEvent>,
}

impl ApplyCtx {
    /// Record an Event entry in the log for a non-bookkeeping command.
    /// Returns the event_id (0 for bookkeeping commands that skip recording).
    pub(crate) fn record_event(&mut self, cmd: &SimCommand) -> u64 {
        if cmd.is_bookkeeping() {
            return 0;
        }

        let event_id = self.id_gen.0.next_id();

        self.event_log.events.push(EcsEvent {
            id: event_id,
            kind: cmd.event_kind.clone(),
            timestamp: self.clock_time,
            description: cmd.description.clone(),
            caused_by: cmd.caused_by,
            data: cmd.event_data.clone(),
        });

        for (entity, role) in &cmd.participants {
            if let Some(sim_id) = self.entity_map.get_sim(*entity) {
                self.event_log.participants.push(EventParticipant {
                    event_id,
                    entity_id: sim_id,
                    role: role.clone(),
                });
            }
        }

        event_id
    }

    /// Record a state-change effect against an entity.
    pub(crate) fn record_effect(&mut self, event_id: u64, entity: Entity, change: StateChange) {
        let entity_id = self.entity_map.get_sim(entity).unwrap_or(0);
        self.event_log.effects.push(EventEffect {
            event_id,
            entity_id,
            effect: change,
        });
    }

    /// Queue a reactive event for emission after all commands are processed.
    pub(crate) fn emit(&mut self, event: SimReactiveEvent) {
        self.reactive_events.push(event);
    }
}

/// Exclusive system that drains all pending `SimCommand` messages, applies
/// state changes, records audit trail, and emits `SimReactiveEvent` messages.
///
/// Runs in `SimPhase::PostUpdate`.
pub fn apply_sim_commands(world: &mut World) {
    // Drain all pending commands
    let commands: Vec<SimCommand> = {
        let Some(mut messages) = world.get_resource_mut::<Messages<SimCommand>>() else {
            return;
        };
        messages.drain().collect()
    };

    if commands.is_empty() {
        return;
    }

    // Extract resources into ApplyCtx
    let clock_time = world.resource::<SimClock>().time;
    let event_log = world.remove_resource::<EventLog>().unwrap();
    let id_gen = world.remove_resource::<EcsIdGenerator>().unwrap();
    let entity_map = world.remove_resource::<SimEntityMap>().unwrap();
    let rel_graph = world.remove_resource::<RelationshipGraph>().unwrap();

    let mut ctx = ApplyCtx {
        event_log,
        id_gen,
        entity_map,
        rel_graph,
        clock_time,
        reactive_events: Vec::new(),
    };

    // Process each command
    for cmd in &commands {
        let event_id = ctx.record_event(cmd);

        match &cmd.kind {
            // Entity Lifecycle
            SimCommandKind::EndEntity { entity } => {
                apply_lifecycle::apply_end_entity(&mut ctx, world, event_id, *entity);
            }
            SimCommandKind::RenameEntity { entity, new_name } => {
                apply_lifecycle::apply_rename_entity(&mut ctx, world, event_id, *entity, new_name);
            }

            // Relationships
            SimCommandKind::AddRelationship {
                source,
                target,
                kind,
            } => {
                apply_relationship::apply_add_relationship(
                    &mut ctx, world, event_id, *source, *target, kind,
                );
            }
            SimCommandKind::EndRelationship {
                source,
                target,
                kind,
            } => {
                apply_relationship::apply_end_relationship(
                    &mut ctx, world, event_id, *source, *target, kind,
                );
            }

            // Demographics
            SimCommandKind::PersonDied { person } => {
                apply_demographics::apply_person_died(&mut ctx, world, event_id, *person);
            }
            SimCommandKind::PersonBorn {
                name,
                faction,
                settlement,
            } => {
                apply_demographics::apply_person_born(
                    &mut ctx, world, event_id, name, *faction, *settlement,
                );
            }

            // Military
            SimCommandKind::DeclareWar { attacker, defender } => {
                apply_military::apply_declare_war(&mut ctx, event_id, *attacker, *defender);
            }
            SimCommandKind::CaptureSettlement {
                settlement,
                new_faction,
            } => {
                apply_military::apply_capture_settlement(
                    &mut ctx, world, event_id, *settlement, *new_faction,
                );
            }

            // Generic
            SimCommandKind::SetField {
                entity,
                field,
                old_value,
                new_value,
            } => {
                apply_set_field::apply_set_field(
                    &mut ctx, event_id, *entity, field, old_value, new_value,
                );
            }

            // Unimplemented variants — warn but don't panic
            _ => {
                tracing::warn!("Unimplemented SimCommandKind: {:?}", cmd.kind);
            }
        }
    }

    // Write reactive events
    let reactive_events = std::mem::take(&mut ctx.reactive_events);
    if let Some(mut messages) = world.get_resource_mut::<Messages<SimReactiveEvent>>() {
        messages.write_batch(reactive_events);
    }

    // Put resources back
    world.insert_resource(ctx.event_log);
    world.insert_resource(ctx.id_gen);
    world.insert_resource(ctx.entity_map);
    world.insert_resource(ctx.rel_graph);
}

#[cfg(test)]
mod tests {
    use crate::ecs::app::build_sim_app;
    use crate::ecs::commands::{SimCommand, SimCommandKind};
    use crate::ecs::components::common::SimEntity;
    use crate::ecs::components::{
        Faction, FactionCore, Person, PersonCore, PersonEducation, PersonReputation, PersonSocial,
        Settlement, SettlementCore,
    };
    use crate::ecs::events::SimReactiveEvent;
    use crate::ecs::relationships::{LeaderOf, MemberOf, RelationshipGraph};
    use crate::ecs::resources::{EventLog, SimEntityMap};
    use crate::ecs::schedule::SimTick;
    use crate::ecs::time::SimTime;
    use crate::model::effect::StateChange;
    use crate::model::event::{EventKind, ParticipantRole};
    use crate::model::relationship::RelationshipKind;

    use bevy_ecs::message::Messages;
    use bevy_ecs::world::World;

    use super::*;

    /// Helper: spawn a minimal person entity with sim_id registration.
    fn spawn_test_person(world: &mut World, sim_id: u64, name: &str) -> Entity {
        let entity = world
            .spawn((
                SimEntity {
                    id: sim_id,
                    name: name.to_string(),
                    origin: Some(SimTime::from_year(100)),
                    end: None,
                },
                Person,
                PersonCore::default(),
                PersonReputation::default(),
                PersonSocial::default(),
                PersonEducation::default(),
            ))
            .id();
        world.resource_mut::<SimEntityMap>().insert(sim_id, entity);
        entity
    }

    /// Helper: spawn a minimal faction entity.
    fn spawn_test_faction(world: &mut World, sim_id: u64, name: &str) -> Entity {
        let entity = world
            .spawn((
                SimEntity {
                    id: sim_id,
                    name: name.to_string(),
                    origin: Some(SimTime::from_year(100)),
                    end: None,
                },
                Faction,
                FactionCore::default(),
            ))
            .id();
        world.resource_mut::<SimEntityMap>().insert(sim_id, entity);
        entity
    }

    /// Helper: spawn a minimal settlement entity.
    fn spawn_test_settlement(world: &mut World, sim_id: u64, name: &str) -> Entity {
        let entity = world
            .spawn((
                SimEntity {
                    id: sim_id,
                    name: name.to_string(),
                    origin: Some(SimTime::from_year(100)),
                    end: None,
                },
                Settlement,
                SettlementCore::default(),
            ))
            .id();
        world.resource_mut::<SimEntityMap>().insert(sim_id, entity);
        entity
    }

    fn write_command(world: &mut World, cmd: SimCommand) {
        world.resource_mut::<Messages<SimCommand>>().write(cmd);
    }

    fn tick(app: &mut bevy_app::App) {
        app.world_mut().run_schedule(SimTick);
    }

    #[test]
    fn end_entity_marks_ended() {
        let mut app = build_sim_app(100);
        let person = spawn_test_person(app.world_mut(), 1, "Aldric");

        let cmd = SimCommand::new(
            SimCommandKind::EndEntity { entity: person },
            EventKind::Death,
            "Aldric died",
        )
        .with_participant(person, ParticipantRole::Subject);

        write_command(app.world_mut(), cmd);
        tick(&mut app);

        // Verify entity is ended
        let sim_entity = app.world().get::<SimEntity>(person).unwrap();
        assert!(sim_entity.end.is_some());

        // Verify EventLog has the event
        let log = app.world().resource::<EventLog>();
        assert_eq!(log.events.len(), 1);
        assert_eq!(log.events[0].kind, EventKind::Death);
        assert_eq!(log.events[0].description, "Aldric died");

        // Verify EntityEnded effect
        let has_ended_effect = log
            .effects
            .iter()
            .any(|e| e.entity_id == 1 && matches!(e.effect, StateChange::EntityEnded));
        assert!(has_ended_effect, "expected EntityEnded effect");

        // Verify reactive event emitted
        let reactive = app.world().resource::<Messages<SimReactiveEvent>>();
        assert!(!reactive.is_empty());
    }

    #[test]
    fn duplicate_end_entity_is_noop() {
        let mut app = build_sim_app(100);
        let person = spawn_test_person(app.world_mut(), 1, "Aldric");

        // First end
        let cmd = SimCommand::new(
            SimCommandKind::EndEntity { entity: person },
            EventKind::Death,
            "Aldric died",
        );
        write_command(app.world_mut(), cmd);
        tick(&mut app);

        assert_eq!(app.world().resource::<EventLog>().events.len(), 1);

        // Second end (idempotent — no new effect)
        let cmd2 = SimCommand::new(
            SimCommandKind::EndEntity { entity: person },
            EventKind::Death,
            "Aldric died again",
        );
        write_command(app.world_mut(), cmd2);
        tick(&mut app);

        // 2 events recorded (both commands), but only 1 EntityEnded effect
        let log = app.world().resource::<EventLog>();
        let ended_effects: Vec<_> = log
            .effects
            .iter()
            .filter(|e| matches!(e.effect, StateChange::EntityEnded))
            .collect();
        assert_eq!(ended_effects.len(), 1);
    }

    #[test]
    fn rename_entity() {
        let mut app = build_sim_app(100);
        let settlement = spawn_test_settlement(app.world_mut(), 1, "Ironhold");

        let cmd = SimCommand::new(
            SimCommandKind::RenameEntity {
                entity: settlement,
                new_name: "Ironhaven".to_string(),
            },
            EventKind::Renamed,
            "Ironhold renamed to Ironhaven",
        );
        write_command(app.world_mut(), cmd);
        tick(&mut app);

        let sim_entity = app.world().get::<SimEntity>(settlement).unwrap();
        assert_eq!(sim_entity.name, "Ironhaven");

        let log = app.world().resource::<EventLog>();
        let has_name_effect = log.effects.iter().any(|e| {
            matches!(
                &e.effect,
                StateChange::NameChanged { old, new } if old == "Ironhold" && new == "Ironhaven"
            )
        });
        assert!(has_name_effect, "expected NameChanged effect");
    }

    #[test]
    fn declare_war_creates_relationship() {
        let mut app = build_sim_app(100);
        let faction_a = spawn_test_faction(app.world_mut(), 1, "Kingdom A");
        let faction_b = spawn_test_faction(app.world_mut(), 2, "Kingdom B");

        let cmd = SimCommand::new(
            SimCommandKind::DeclareWar {
                attacker: faction_a,
                defender: faction_b,
            },
            EventKind::WarDeclared,
            "Kingdom A declares war on Kingdom B",
        )
        .with_participant(faction_a, ParticipantRole::Attacker)
        .with_participant(faction_b, ParticipantRole::Defender);

        write_command(app.world_mut(), cmd);
        tick(&mut app);

        let rel_graph = app.world().resource::<RelationshipGraph>();
        assert!(rel_graph.are_at_war(faction_a, faction_b));

        let reactive = app.world().resource::<Messages<SimReactiveEvent>>();
        assert!(!reactive.is_empty());
    }

    #[test]
    fn add_structural_relationship() {
        let mut app = build_sim_app(100);
        let person = spawn_test_person(app.world_mut(), 1, "Aldric");
        let faction = spawn_test_faction(app.world_mut(), 2, "Kingdom");

        let cmd = SimCommand::new(
            SimCommandKind::AddRelationship {
                source: person,
                target: faction,
                kind: RelationshipKind::MemberOf,
            },
            EventKind::Joined,
            "Aldric joins the Kingdom",
        );
        write_command(app.world_mut(), cmd);
        tick(&mut app);

        let member_of = app.world().get::<MemberOf>(person);
        assert!(member_of.is_some());
        assert_eq!(member_of.unwrap().0, faction);
    }

    #[test]
    fn set_field_bookkeeping() {
        let mut app = build_sim_app(100);
        let person = spawn_test_person(app.world_mut(), 1, "Aldric");

        let cmd = SimCommand::bookkeeping(SimCommandKind::SetField {
            entity: person,
            field: "prestige".to_string(),
            old_value: serde_json::json!(10.0),
            new_value: serde_json::json!(15.0),
        });
        write_command(app.world_mut(), cmd);
        tick(&mut app);

        // No event in the log
        let log = app.world().resource::<EventLog>();
        assert!(log.events.is_empty());

        // But there is a PropertyChanged effect
        let has_prop_effect = log.effects.iter().any(|e| {
            matches!(&e.effect, StateChange::PropertyChanged { field, .. } if field == "prestige")
        });
        assert!(has_prop_effect, "expected PropertyChanged effect");
    }

    #[test]
    fn person_died_ends_relationships() {
        let mut app = build_sim_app(100);
        let faction = spawn_test_faction(app.world_mut(), 1, "Kingdom");
        let person = spawn_test_person(app.world_mut(), 2, "Aldric");

        // Set up person as leader and member of faction
        app.world_mut()
            .entity_mut(person)
            .insert((MemberOf(faction), LeaderOf(faction)));

        let cmd = SimCommand::new(
            SimCommandKind::PersonDied { person },
            EventKind::Death,
            "Aldric died",
        );
        write_command(app.world_mut(), cmd);
        tick(&mut app);

        // Verify ended
        let sim_entity = app.world().get::<SimEntity>(person).unwrap();
        assert!(sim_entity.end.is_some());

        // Verify MemberOf removed
        assert!(app.world().get::<MemberOf>(person).is_none());

        // Verify LeaderOf removed
        assert!(app.world().get::<LeaderOf>(person).is_none());

        // Verify LeaderVacancy reactive event
        let reactive = app.world().resource::<Messages<SimReactiveEvent>>();
        assert!(!reactive.is_empty());
    }

    #[test]
    fn capture_settlement_changes_faction() {
        let mut app = build_sim_app(100);
        let old_faction = spawn_test_faction(app.world_mut(), 1, "Kingdom A");
        let new_faction = spawn_test_faction(app.world_mut(), 2, "Kingdom B");
        let settlement = spawn_test_settlement(app.world_mut(), 3, "Ironhold");

        app.world_mut()
            .entity_mut(settlement)
            .insert(MemberOf(old_faction));

        let cmd = SimCommand::new(
            SimCommandKind::CaptureSettlement {
                settlement,
                new_faction,
            },
            EventKind::Conquest,
            "Ironhold captured by Kingdom B",
        );
        write_command(app.world_mut(), cmd);
        tick(&mut app);

        let member_of = app.world().get::<MemberOf>(settlement).unwrap();
        assert_eq!(member_of.0, new_faction);

        let reactive = app.world().resource::<Messages<SimReactiveEvent>>();
        assert!(!reactive.is_empty());
    }

    #[test]
    fn causal_chain_preserved() {
        let mut app = build_sim_app(100);
        let faction_a = spawn_test_faction(app.world_mut(), 1, "Kingdom A");
        let faction_b = spawn_test_faction(app.world_mut(), 2, "Kingdom B");

        // First: declare war
        let cmd = SimCommand::new(
            SimCommandKind::DeclareWar {
                attacker: faction_a,
                defender: faction_b,
            },
            EventKind::WarDeclared,
            "War declared",
        );
        write_command(app.world_mut(), cmd);
        tick(&mut app);

        let war_event_id = app.world().resource::<EventLog>().events[0].id;

        // Second: bookkeeping caused by the war
        let cmd2 = SimCommand::bookkeeping(SimCommandKind::SetField {
            entity: faction_a,
            field: "at_war".to_string(),
            old_value: serde_json::json!(false),
            new_value: serde_json::json!(true),
        })
        .caused_by(war_event_id);
        write_command(app.world_mut(), cmd2);
        tick(&mut app);

        let log = app.world().resource::<EventLog>();
        // Only the war event (bookkeeping skips event creation)
        assert_eq!(log.events.len(), 1);
        assert_eq!(log.events[0].id, war_event_id);
    }

    #[test]
    fn messages_cleared_between_ticks() {
        let mut app = build_sim_app(100);
        let faction_a = spawn_test_faction(app.world_mut(), 1, "Kingdom A");
        let faction_b = spawn_test_faction(app.world_mut(), 2, "Kingdom B");

        // Tick 1: emit a command
        let cmd = SimCommand::new(
            SimCommandKind::DeclareWar {
                attacker: faction_a,
                defender: faction_b,
            },
            EventKind::WarDeclared,
            "War declared",
        );
        write_command(app.world_mut(), cmd);
        tick(&mut app);

        assert!(
            !app.world()
                .resource::<Messages<SimReactiveEvent>>()
                .is_empty()
        );

        // Tick 2: no commands — message_update_system rotates buffers
        tick(&mut app);

        // Tick 3: old messages fully cleared from double-buffer
        tick(&mut app);

        let reactive = app.world().resource::<Messages<SimReactiveEvent>>();
        assert!(
            reactive.is_empty(),
            "stale reactive events should be cleared"
        );
    }
}
