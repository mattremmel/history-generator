use history_gen::model::action::{Action, ActionKind, ActionOutcome, ActionSource};
use history_gen::model::{EntityKind, EventKind, ParticipantRole, RelationshipKind};
use history_gen::scenario::Scenario;
use history_gen::sim::{
    ActionSystem, ConflictSystem, DemographicsSystem, EconomySystem, PoliticsSystem, SimConfig,
    SimSystem, run,
};

#[test]
fn scenario_assassination_triggers_succession() {
    let mut s = Scenario::at_year(100);
    let setup = s.add_settlement_standalone_with(
        "Capital",
        |fd| {
            fd.stability = 0.7;
            fd.happiness = 0.6;
            fd.legitimacy = 0.7;
        },
        |sd| {
            sd.population = 500;
        },
    );
    let faction = setup.faction;

    let leader = s.add_person_with("Old King", faction, |pd| {
        pd.birth_year = 60;
        pd.role = "warrior".to_string();
    });
    s.make_leader(leader, faction);

    let player = s.add_person_with("Dorian Blackthorn", faction, |pd| {
        pd.birth_year = 70;
    });
    s.set_extra(player, "is_player", serde_json::json!(true));

    // Add some other persons so succession can happen
    let _noble = s.add_person_with("Noble Heir", faction, |pd| {
        pd.birth_year = 75;
        pd.role = "warrior".to_string();
    });
    let mut world = s.build();

    // Queue assassination of the leader
    world.queue_action(Action {
        actor_id: player,
        source: ActionSource::Player,
        kind: ActionKind::Assassinate { target_id: leader },
    });

    let mut systems: Vec<Box<dyn SimSystem>> = vec![
        Box::new(ActionSystem),
        Box::new(DemographicsSystem),
        Box::new(EconomySystem),
        Box::new(PoliticsSystem),
    ];
    run(&mut world, &mut systems, SimConfig::new(100, 3, 42));

    // Verify leader is dead
    assert!(
        world.entities[&leader].end.is_some(),
        "assassinated leader should be dead"
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
            && p.entity_id == player
            && p.role == ParticipantRole::Instigator
    }));

    // Verify death event caused by assassination
    assert!(
        world
            .events
            .values()
            .any(|e| e.kind == EventKind::Death && e.caused_by == Some(assassination.id)),
        "should have death event caused by assassination"
    );

    // Verify succession occurred
    let succession_exists = world.events.values().any(|e| {
        e.kind == EventKind::Succession
            && world
                .event_participants
                .iter()
                .any(|p| p.event_id == e.id && p.entity_id == faction)
    });
    let has_new_leader = world.entities.values().any(|e| {
        e.kind == EntityKind::Person
            && e.end.is_none()
            && e.id != leader
            && e.relationships.iter().any(|r| {
                r.kind == RelationshipKind::LeaderOf
                    && r.target_entity_id == faction
                    && r.end.is_none()
            })
    });

    assert!(
        succession_exists || has_new_leader,
        "faction should have succession event or new leader after assassination"
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
fn scenario_undermining_destabilizes_faction() {
    let mut s = Scenario::at_year(100);
    let setup = s.add_settlement_standalone_with(
        "Capital",
        |fd| {
            fd.stability = 0.7;
            fd.happiness = 0.6;
            fd.legitimacy = 0.7;
        },
        |sd| {
            sd.population = 500;
        },
    );
    let faction = setup.faction;

    let leader = s.add_person("King", faction);
    s.make_leader(leader, faction);

    let player = s.add_person_with("Dorian Blackthorn", faction, |pd| {
        pd.birth_year = 70;
    });
    s.set_extra(player, "is_player", serde_json::json!(true));
    let mut world = s.build();

    let starting_stability = world.entities[&faction]
        .data
        .as_faction()
        .unwrap()
        .stability;

    // Queue 5 undermine actions
    for _ in 0..5 {
        world.queue_action(Action {
            actor_id: player,
            source: ActionSource::Player,
            kind: ActionKind::UndermineFaction {
                faction_id: faction,
            },
        });
    }

    let mut systems: Vec<Box<dyn SimSystem>> = vec![
        Box::new(ActionSystem),
        Box::new(DemographicsSystem),
        Box::new(EconomySystem),
        Box::new(PoliticsSystem),
    ];
    run(&mut world, &mut systems, SimConfig::new(100, 1, 42));

    // Check faction still exists
    if let Some(faction_entity) = world.entities.get(&faction) {
        if faction_entity.end.is_none() {
            let final_stability = faction_entity
                .data
                .as_faction()
                .map(|f| f.stability)
                .unwrap_or(0.5);
            assert!(
                final_stability < starting_stability,
                "stability should drop: started at {starting_stability}, ended at {final_stability}"
            );
        }
        // If faction ended, that's also a valid outcome of heavy undermining
    }

    let undermine_events: Vec<_> = world
        .events
        .values()
        .filter(|e| e.kind == EventKind::Custom("faction_undermine".to_string()))
        .collect();
    assert_eq!(undermine_events.len(), 5, "should have 5 undermine events");
}

#[test]
fn scenario_declare_war_action() {
    let mut s = Scenario::at_year(100);
    let pk = s.add_kingdom_with(
        "Player Kingdom",
        |fd| {
            fd.stability = 0.8;
            fd.happiness = 0.5;
        },
        |sd| sd.population = 500,
        |_| {},
    );
    let tk = s.add_rival_kingdom_with(
        "Target Kingdom",
        pk.region,
        |fd| {
            fd.stability = 0.5;
            fd.happiness = 0.5;
        },
        |sd| sd.population = 500,
        |_| {},
    );
    let player_faction = pk.faction;
    let target_faction = tk.faction;
    let player = s.add_player_in_with("Dorian Blackthorn", player_faction, |pd| {
        pd.birth_year = 70;
    });
    let mut world = s.build();

    // Queue DeclareWar action
    world.queue_action(Action {
        actor_id: player,
        source: ActionSource::Player,
        kind: ActionKind::DeclareWar {
            target_faction_id: target_faction,
        },
    });

    let mut systems: Vec<Box<dyn SimSystem>> = vec![
        Box::new(ActionSystem),
        Box::new(DemographicsSystem),
        Box::new(EconomySystem),
        Box::new(ConflictSystem),
        Box::new(PoliticsSystem),
    ];
    run(&mut world, &mut systems, SimConfig::new(100, 1, 42));

    // Verify WarDeclared event exists
    let war_declared = world
        .events
        .values()
        .find(|e| e.kind == EventKind::WarDeclared)
        .expect("should have WarDeclared event");

    // Verify player is Instigator
    assert!(world.event_participants.iter().any(|p| {
        p.event_id == war_declared.id
            && p.entity_id == player
            && p.role == ParticipantRole::Instigator
    }));

    // Verify AtWar relationship was created
    let has_at_war = world.entities.get(&player_faction).is_some_and(|e| {
        e.relationships
            .iter()
            .any(|r| r.kind == RelationshipKind::AtWar && r.target_entity_id == target_faction)
    });
    assert!(
        has_at_war,
        "should have AtWar relationship from player's faction to target"
    );

    // Verify action result is success
    assert!(
        world
            .action_results
            .iter()
            .any(|r| matches!(r.outcome, ActionOutcome::Success { .. })),
        "DeclareWar action should produce a success result"
    );
}
