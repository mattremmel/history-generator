use history_gen::model::{EntityKind, EventKind, RelationshipKind, World};
use history_gen::sim::{
    ActionSystem, ConflictSystem, DemographicsSystem, PoliticsSystem, SimConfig, SimSystem, run,
};
use history_gen::worldgen::{self, config::WorldGenConfig};

fn generate_and_run(seed: u64, num_years: u32) -> World {
    let config = WorldGenConfig {
        seed,
        ..WorldGenConfig::default()
    };
    let mut world = worldgen::generate_world(&config);
    let mut systems: Vec<Box<dyn SimSystem>> = vec![
        Box::new(ActionSystem),
        Box::new(DemographicsSystem),
        Box::new(ConflictSystem),
        Box::new(PoliticsSystem),
    ];
    run(&mut world, &mut systems, SimConfig::new(1, num_years, seed));
    world
}

/// Helper to run without ConflictSystem for comparison tests
fn generate_and_run_no_conflicts(seed: u64, num_years: u32) -> World {
    let config = WorldGenConfig {
        seed,
        ..WorldGenConfig::default()
    };
    let mut world = worldgen::generate_world(&config);
    let mut systems: Vec<Box<dyn SimSystem>> = vec![
        Box::new(ActionSystem),
        Box::new(DemographicsSystem),
        Box::new(PoliticsSystem),
    ];
    run(&mut world, &mut systems, SimConfig::new(1, num_years, seed));
    world
}

#[test]
fn thousand_year_conflicts() {
    let mut total_wars = 0;
    let mut total_battles = 0;
    let mut total_treaties = 0;

    for seed in [42, 99, 123, 777] {
        let world = generate_and_run(seed, 1000);

        total_wars += world
            .events
            .values()
            .filter(|e| e.kind == EventKind::WarDeclared)
            .count();
        total_battles += world
            .events
            .values()
            .filter(|e| e.kind == EventKind::Battle)
            .count();
        total_treaties += world
            .events
            .values()
            .filter(|e| e.kind == EventKind::Treaty)
            .count();

        // All living factions have valid stability/happiness
        for faction in world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        {
            let stability = faction
                .properties
                .get("stability")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.5);
            assert!(
                (0.0..=1.0).contains(&stability),
                "faction {} stability {} out of range",
                faction.name,
                stability
            );
            let happiness = faction
                .properties
                .get("happiness")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.5);
            assert!(
                (0.0..=1.0).contains(&happiness),
                "faction {} happiness {} out of range",
                faction.name,
                happiness
            );
        }

        // Living settlements belong to exactly one faction
        for settlement in world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
        {
            let faction_memberships: Vec<_> = settlement
                .relationships
                .iter()
                .filter(|r| {
                    r.kind == RelationshipKind::MemberOf
                        && r.end.is_none()
                        && world
                            .entities
                            .get(&r.target_entity_id)
                            .is_some_and(|t| t.kind == EntityKind::Faction)
                })
                .collect();
            assert_eq!(
                faction_memberships.len(),
                1,
                "settlement {} should belong to exactly 1 faction, got {}",
                settlement.name,
                faction_memberships.len()
            );
        }
    }

    assert!(
        total_wars > 0,
        "expected at least one WarDeclared event across 4 seeds x 1000 years"
    );
    assert!(
        total_battles > 0,
        "expected at least one Battle event across 4 seeds x 1000 years"
    );
    assert!(
        total_treaties > 0,
        "expected at least one Treaty event across 4 seeds x 1000 years"
    );
}

#[test]
fn war_produces_casualties() {
    let mut found_battle_deaths = false;
    for seed in [42, 99, 123, 777, 1, 2, 3, 4] {
        let world = generate_and_run(seed, 1000);

        // Look for Death events caused by Battle events
        let battle_event_ids: Vec<u64> = world
            .events
            .values()
            .filter(|e| e.kind == EventKind::Battle)
            .map(|e| e.id)
            .collect();

        for ev in world.events.values() {
            if ev.kind == EventKind::Death {
                if let Some(caused_by) = ev.caused_by {
                    if battle_event_ids.contains(&caused_by) {
                        found_battle_deaths = true;
                        break;
                    }
                }
            }
        }
        if found_battle_deaths {
            break;
        }
    }

    assert!(
        found_battle_deaths,
        "expected Death events caused by Battle events across 8 seeds x 1000 years"
    );
}

#[test]
fn territory_changes_hands() {
    let mut found_conquest = false;
    for seed in [42, 99, 123, 777] {
        let world = generate_and_run(seed, 1000);

        let conquest_events: Vec<_> = world
            .events
            .values()
            .filter(|e| e.kind == EventKind::Conquest)
            .collect();

        if !conquest_events.is_empty() {
            found_conquest = true;

            // For each conquest, verify the settlement's current faction matches conqueror
            for conquest in &conquest_events {
                // Find the settlement (Object participant) and attacker (Attacker participant)
                let settlement_id = world
                    .event_participants
                    .iter()
                    .find(|p| p.event_id == conquest.id && p.role == ParticipantRole::Object)
                    .map(|p| p.entity_id);
                let attacker_id = world
                    .event_participants
                    .iter()
                    .find(|p| p.event_id == conquest.id && p.role == ParticipantRole::Attacker)
                    .map(|p| p.entity_id);

                if let (Some(sid), Some(aid)) = (settlement_id, attacker_id) {
                    // If settlement is still alive, verify it has the right faction
                    // (it may have been conquered again or abandoned later)
                    if let Some(settlement) = world.entities.get(&sid) {
                        if settlement.end.is_none() {
                            // Settlement was conquered by this attacker at this point
                            // but may have changed hands since — just verify it belongs to *some* faction
                            let has_faction = settlement.relationships.iter().any(|r| {
                                r.kind == RelationshipKind::MemberOf
                                    && r.end.is_none()
                                    && world
                                        .entities
                                        .get(&r.target_entity_id)
                                        .is_some_and(|t| t.kind == EntityKind::Faction)
                            });
                            assert!(
                                has_faction,
                                "conquered settlement {} should belong to a faction",
                                settlement.name
                            );
                        }
                    }
                    let _ = aid; // used for lookup
                }
            }
            break;
        }
    }

    assert!(
        found_conquest,
        "expected at least one Conquest event across 4 seeds x 1000 years"
    );
}

use history_gen::model::event::ParticipantRole;

#[test]
fn army_entities_created_and_disbanded() {
    let mut found_army_mustered = false;
    for seed in [42, 99, 123] {
        let world = generate_and_run(seed, 500);

        let army_mustered_count = world
            .events
            .values()
            .filter(|e| e.kind == EventKind::Custom("army_mustered".to_string()))
            .count();

        if army_mustered_count > 0 {
            found_army_mustered = true;

            // Most Army entities should eventually be ended
            let total_armies = world
                .entities
                .values()
                .filter(|e| e.kind == EntityKind::Army)
                .count();
            let ended_armies = world
                .entities
                .values()
                .filter(|e| e.kind == EntityKind::Army && e.end.is_some())
                .count();

            // At least some armies should be ended (wars resolve)
            if total_armies > 1 {
                assert!(
                    ended_armies > 0,
                    "expected some armies to be disbanded after 500 years (total: {total_armies})"
                );
            }
            break;
        }
    }

    assert!(
        found_army_mustered,
        "expected army_mustered events across 3 seeds x 500 years"
    );
}

#[test]
fn determinism_with_conflicts() {
    let world1 = generate_and_run(42, 200);
    let world2 = generate_and_run(42, 200);

    let entity_count1 = world1.entities.len();
    let entity_count2 = world2.entities.len();
    assert_eq!(
        entity_count1, entity_count2,
        "same seed should produce same entity count: {entity_count1} vs {entity_count2}"
    );

    let event_count1 = world1.events.len();
    let event_count2 = world2.events.len();
    assert_eq!(
        event_count1, event_count2,
        "same seed should produce same event count: {event_count1} vs {event_count2}"
    );
}

#[test]
fn war_reduces_population() {
    // Compare population with and without conflict system across multiple seeds
    let mut found_reduction = false;
    for seed in [42, 99, 123] {
        let world_with = generate_and_run(seed, 500);
        let world_without = generate_and_run_no_conflicts(seed, 500);

        let pop_with: u64 = world_with
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
            .filter_map(|e| e.properties.get("population")?.as_u64())
            .sum();

        let pop_without: u64 = world_without
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
            .filter_map(|e| e.properties.get("population")?.as_u64())
            .sum();

        // If wars happened, population should be lower with conflict system
        let wars_happened = world_with
            .events
            .values()
            .any(|e| e.kind == EventKind::WarDeclared);

        if wars_happened && pop_with < pop_without {
            found_reduction = true;
            break;
        }
    }

    assert!(
        found_reduction,
        "expected wars to reduce total population in at least one seed"
    );
}

#[test]
fn armies_have_location() {
    for seed in [42, 99, 123] {
        let world = generate_and_run(seed, 200);

        // Every living army should have a LocatedIn relationship to a Region
        for army in world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Army && e.end.is_none())
        {
            let has_location = army.relationships.iter().any(|r| {
                r.kind == RelationshipKind::LocatedIn
                    && r.end.is_none()
                    && world
                        .entities
                        .get(&r.target_entity_id)
                        .is_some_and(|t| t.kind == EntityKind::Region)
            });
            assert!(
                has_location,
                "living army {} should have a LocatedIn relationship to a Region",
                army.name
            );
        }
    }
}

#[test]
fn armies_travel_between_regions() {
    let mut found_moved = false;
    for seed in [42, 99, 123, 777] {
        let world = generate_and_run(seed, 500);

        let moved_count = world
            .events
            .values()
            .filter(|e| e.kind == EventKind::Custom("army_moved".to_string()))
            .count();

        if moved_count > 0 {
            found_moved = true;
            break;
        }
    }

    assert!(
        found_moved,
        "expected army_moved events across 4 seeds x 500 years"
    );
}

#[test]
fn army_attrition_occurs() {
    let mut found_attrition = false;
    for seed in [19, 51, 62, 68, 72, 78, 42, 99, 123, 777] {
        let world = generate_and_run(seed, 1000);

        let attrition_count = world
            .events
            .values()
            .filter(|e| e.kind == EventKind::Custom("army_attrition".to_string()))
            .count();

        if attrition_count > 0 {
            found_attrition = true;
            break;
        }
    }

    assert!(
        found_attrition,
        "expected army_attrition events across 10 seeds x 1000 years"
    );
}

#[test]
fn army_supply_depletes() {
    let mut found_depleted = false;
    for seed in [42, 99, 123, 777, 1, 2, 3, 4, 5, 6, 7, 8] {
        let world = generate_and_run(seed, 1000);

        // Check if any army had supply < starting supply or morale < 1.0
        for army in world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Army)
        {
            let supply = army
                .properties
                .get("supply")
                .and_then(|v| v.as_f64())
                .unwrap_or(3.0);
            if supply < 2.99 {
                found_depleted = true;
                break;
            }
        }
        if found_depleted {
            break;
        }
    }

    assert!(
        found_depleted,
        "expected armies to have depleted supply at some point"
    );
}

#[test]
fn battles_happen_at_army_location() {
    let mut found_battle_with_location = false;
    for seed in [42, 99, 123, 777] {
        let world = generate_and_run(seed, 500);

        for ev in world.events.values() {
            if ev.kind == EventKind::Battle {
                // Battle should have a Location participant
                let has_location = world
                    .event_participants
                    .iter()
                    .any(|p| p.event_id == ev.id && p.role == ParticipantRole::Location);
                if has_location {
                    found_battle_with_location = true;
                    break;
                }
            }
        }
        if found_battle_with_location {
            break;
        }
    }

    assert!(
        found_battle_with_location,
        "expected Battle events to have a Location participant"
    );
}

#[test]
fn army_retreat_occurs() {
    let mut found_retreat = false;
    for seed in [42, 99, 123, 777, 1, 2, 3, 4, 5, 6, 7, 8] {
        let world = generate_and_run(seed, 1000);

        let retreat_count = world
            .events
            .values()
            .filter(|e| e.kind == EventKind::Custom("army_retreated".to_string()))
            .count();

        if retreat_count > 0 {
            found_retreat = true;
            break;
        }
    }

    assert!(
        found_retreat,
        "expected army_retreated events across 12 seeds x 1000 years"
    );
}

#[test]
fn long_campaigns_cause_starvation() {
    let mut found_long_campaign = false;
    for seed in [19, 51, 62, 68, 72, 78, 42, 99, 123, 777] {
        let world = generate_and_run(seed, 1000);

        // Look for armies that campaigned long enough that supply dropped significantly
        for army in world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Army)
        {
            let supply = army
                .properties
                .get("supply")
                .and_then(|v| v.as_f64())
                .unwrap_or(3.0);
            let months = army
                .properties
                .get("months_campaigning")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            // Either supply depleted or campaigned for multiple months
            if supply < 1.0 || (months > 6 && supply < 2.0) {
                found_long_campaign = true;
                break;
            }
        }
        if found_long_campaign {
            break;
        }
    }

    assert!(
        found_long_campaign,
        "expected long campaigns to deplete supply across 10 seeds x 1000 years"
    );
}
