use history_gen::model::action::{Action, ActionKind, ActionOutcome, ActionSource};
use history_gen::model::{EntityKind, EventKind, ParticipantRole, RelationshipKind};
use history_gen::sim::{
    ActionSystem, DemographicsSystem, PoliticsSystem, SimConfig, SimSystem, run,
};
use history_gen::worldgen::{self, config::WorldGenConfig};

fn make_world_with_player(seed: u64) -> (history_gen::model::World, u64) {
    let config = WorldGenConfig {
        seed,
        ..WorldGenConfig::default()
    };
    let mut world = worldgen::generate_world(&config);

    // Run 1 year to establish rulers and factions
    let mut systems: Vec<Box<dyn SimSystem>> = vec![
        Box::new(ActionSystem),
        Box::new(DemographicsSystem),
        Box::new(PoliticsSystem),
    ];
    run(&mut world, &mut systems, SimConfig::new(1, 1, seed));

    // Create a player entity
    let time = world.current_time;
    let ev = world.add_event(EventKind::Birth, time, "Player born".to_string());
    let player_id = world.add_entity(
        EntityKind::Person,
        "Dorian Blackthorn".to_string(),
        Some(time),
        ev,
    );
    world.set_property(
        player_id,
        "is_player".to_string(),
        serde_json::json!(true),
        ev,
    );

    // Join the first living faction
    let faction_id = world
        .entities
        .values()
        .find(|e| e.kind == EntityKind::Faction && e.end.is_none())
        .map(|e| e.id)
        .expect("need at least one faction");
    world.add_relationship(player_id, faction_id, RelationshipKind::MemberOf, time, ev);

    (world, player_id)
}

/// Find a faction that has a ruler. Returns (faction_id, ruler_id).
fn find_ruled_faction(world: &history_gen::model::World) -> Option<(u64, u64)> {
    for entity in world.entities.values() {
        if entity.kind != EntityKind::Person || entity.end.is_some() {
            continue;
        }
        for rel in &entity.relationships {
            if rel.kind == RelationshipKind::RulerOf && rel.end.is_none() {
                let faction = world.entities.get(&rel.target_entity_id)?;
                if faction.kind == EntityKind::Faction && faction.end.is_none() {
                    return Some((rel.target_entity_id, entity.id));
                }
            }
        }
    }
    None
}

#[test]
fn assassination_triggers_succession() {
    let (mut world, player_id) = make_world_with_player(42);

    let (faction_id, ruler_id) =
        find_ruled_faction(&world).expect("should have a ruled faction after 1 year");

    // Queue assassination of the ruler
    world.queue_action(Action {
        actor_id: player_id,
        source: ActionSource::Player,
        kind: ActionKind::Assassinate {
            target_id: ruler_id,
        },
    });

    // Run 3 years with all systems (Actions runs first, then demographics, then politics)
    let mut systems: Vec<Box<dyn SimSystem>> = vec![
        Box::new(ActionSystem),
        Box::new(DemographicsSystem),
        Box::new(PoliticsSystem),
    ];
    run(&mut world, &mut systems, SimConfig::new(2, 3, 42));

    // Verify ruler is dead
    assert!(
        world.entities[&ruler_id].end.is_some(),
        "assassinated ruler should be dead"
    );

    // Verify assassination event exists
    let assassination = world
        .events
        .values()
        .find(|e| e.kind == EventKind::Custom("assassination".to_string()))
        .expect("should have assassination event");

    // Verify player is Instigator
    assert!(world.event_participants.iter().any(|p| {
        p.event_id == assassination.id
            && p.entity_id == player_id
            && p.role == ParticipantRole::Instigator
    }));

    // Verify death event caused by assassination
    let death = world
        .events
        .values()
        .find(|e| e.kind == EventKind::Death && e.caused_by == Some(assassination.id))
        .expect("should have death event caused by assassination");
    let _ = death;

    // Verify succession occurred — faction should have a new ruler (or succession event exists)
    let succession_exists = world.events.values().any(|e| {
        e.kind == EventKind::Succession
            && world
                .event_participants
                .iter()
                .any(|p| p.event_id == e.id && p.entity_id == faction_id)
    });

    // The faction either got a new ruler or a succession event was created
    let has_new_ruler = world.entities.values().any(|e| {
        e.kind == EntityKind::Person
            && e.end.is_none()
            && e.id != ruler_id
            && e.relationships.iter().any(|r| {
                r.kind == RelationshipKind::RulerOf
                    && r.target_entity_id == faction_id
                    && r.end.is_none()
            })
    });

    assert!(
        succession_exists || has_new_ruler,
        "faction should have succession event or new ruler after assassination"
    );

    // Verify action result is success
    assert!(
        world
            .action_results
            .iter()
            .any(|r| matches!(r.outcome, ActionOutcome::Success { .. })),
        "assassination should produce a success result"
    );
}

#[test]
fn undermining_destabilizes_faction() {
    let (mut world, player_id) = make_world_with_player(99);

    let faction_id = world
        .entities
        .values()
        .find(|e| e.kind == EntityKind::Faction && e.end.is_none())
        .map(|e| e.id)
        .expect("need a faction");

    // Record starting stability
    let starting_stability = world.entities[&faction_id]
        .properties
        .get("stability")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.5);

    // Queue undermine actions across multiple years
    for _ in 0..5 {
        world.queue_action(Action {
            actor_id: player_id,
            source: ActionSource::Player,
            kind: ActionKind::UndermineFaction { faction_id },
        });
    }

    // Run 1 year — all 5 actions will process in the first tick
    let mut systems: Vec<Box<dyn SimSystem>> = vec![
        Box::new(ActionSystem),
        Box::new(DemographicsSystem),
        Box::new(PoliticsSystem),
    ];
    run(&mut world, &mut systems, SimConfig::new(2, 1, 99));

    // Check faction still exists (may have been ended if stability dropped to 0)
    if let Some(faction) = world.entities.get(&faction_id) {
        if faction.end.is_none() {
            let final_stability = faction
                .properties
                .get("stability")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.5);
            assert!(
                final_stability < starting_stability,
                "stability should drop: started at {starting_stability}, ended at {final_stability}"
            );
        }
        // If faction ended, that's also a valid outcome of heavy undermining
    }

    // All undermine actions should have succeeded
    let undermine_events: Vec<_> = world
        .events
        .values()
        .filter(|e| e.kind == EventKind::Custom("faction_undermine".to_string()))
        .collect();
    assert_eq!(undermine_events.len(), 5, "should have 5 undermine events");
}
