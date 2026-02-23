use history_gen::model::EntityData;
use history_gen::model::{EntityKind, EventKind, RelationshipKind, World};
use history_gen::sim::{
    DemographicsSystem, EconomySystem, PoliticsSystem, SimConfig, SimSystem, run,
};
use history_gen::worldgen::{self, config::WorldGenConfig};

fn generate_and_run(seed: u64, num_years: u32) -> World {
    let config = WorldGenConfig {
        seed,
        ..WorldGenConfig::default()
    };
    let mut world = worldgen::generate_world(&config);
    let mut systems: Vec<Box<dyn SimSystem>> = vec![
        Box::new(DemographicsSystem),
        Box::new(EconomySystem),
        Box::new(PoliticsSystem),
    ];
    run(&mut world, &mut systems, SimConfig::new(1, num_years, seed));
    world
}

#[test]
fn parent_child_relationships_exist() {
    let world = generate_and_run(42, 100);

    let parent_rels = world
        .collect_relationships()
        .filter(|r| r.kind == RelationshipKind::Parent)
        .count();
    let child_rels = world
        .collect_relationships()
        .filter(|r| r.kind == RelationshipKind::Child)
        .count();

    assert!(
        parent_rels > 0,
        "expected Parent relationships after 100 years"
    );
    assert!(
        child_rels > 0,
        "expected Child relationships after 100 years"
    );
    // Parent and Child should be symmetric
    assert_eq!(
        parent_rels, child_rels,
        "Parent count ({parent_rels}) should equal Child count ({child_rels})"
    );
}

#[test]
fn marriages_occur() {
    let world = generate_and_run(42, 200);

    let union_events = world
        .events
        .values()
        .filter(|e| e.kind == EventKind::Union)
        .count();
    let spouse_rels = world
        .collect_relationships()
        .filter(|r| r.kind == RelationshipKind::Spouse)
        .count();

    assert!(union_events > 0, "expected Union events after 200 years");
    assert!(
        spouse_rels > 0,
        "expected Spouse relationships after 200 years"
    );
    // Spouse relationships should be even (bidirectional)
    assert_eq!(
        spouse_rels % 2,
        0,
        "Spouse relationships should be bidirectional (got {spouse_rels})"
    );
}

#[test]
fn surname_dynasties_visible() {
    let world = generate_and_run(42, 300);

    // Collect surnames of living persons
    let mut surnames: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for e in world.entities.values() {
        if e.kind == EntityKind::Person && e.end.is_none() {
            let base = e.name.split(" the ").next().unwrap_or(&e.name);
            if let Some((_, surname)) = base.rsplit_once(' ') {
                *surnames.entry(surname.to_string()).or_default() += 1;
            }
        }
    }

    let shared_surnames = surnames.values().filter(|&&count| count >= 2).count();
    assert!(
        shared_surnames > 0,
        "expected at least one surname shared by 2+ living persons after 300 years"
    );
}

#[test]
fn hereditary_succession_follows_bloodline() {
    // Run 500 years across multiple seeds to find at least one bloodline succession
    let mut found_bloodline_succession = false;

    for seed in [42, 99, 123, 777] {
        let world = generate_and_run(seed, 500);

        // Check succession events: find cases where the new leader is a child of the previous
        for ev in world.events.values() {
            if ev.kind != EventKind::Succession {
                continue;
            }
            // Find the subject (new leader) from event participants
            let new_leader_id = world
                .event_participants
                .iter()
                .find(|p| {
                    p.event_id == ev.id && p.role == history_gen::model::ParticipantRole::Subject
                })
                .map(|p| p.entity_id);
            let Some(new_leader) = new_leader_id else {
                continue;
            };
            // Check if this new leader has a Child relationship to anyone who was a previous leader
            if let Some(entity) = world.entities.get(&new_leader) {
                for r in &entity.relationships {
                    if r.kind == RelationshipKind::Child {
                        // r.target_entity_id is a parent — check if that parent was a leader
                        if let Some(parent) = world.entities.get(&r.target_entity_id) {
                            let was_leader = parent
                                .relationships
                                .iter()
                                .any(|pr| pr.kind == RelationshipKind::LeaderOf);
                            if was_leader {
                                found_bloodline_succession = true;
                            }
                        }
                    }
                }
            }
        }
        if found_bloodline_succession {
            break;
        }
    }

    assert!(
        found_bloodline_succession,
        "expected at least one bloodline succession across 4 seeds x 500 years"
    );
}

#[test]
fn cross_faction_marriages_create_alliances() {
    // In 500 years across multiple seeds, at least one cross-faction marriage should create
    // an Ally relationship or set marriage_alliance_year
    let mut found = false;

    for seed in [42, 99, 123, 777] {
        let world = generate_and_run(seed, 500);

        // Check for marriage_alliance_year property on any faction
        for e in world.entities.values() {
            if e.kind == EntityKind::Faction && e.extra.contains_key("marriage_alliance_year") {
                found = true;
                break;
            }
        }

        // Also check for Union events that mention "forging ties"
        if !found {
            for ev in world.events.values() {
                if ev.kind == EventKind::Union && ev.description.contains("forging ties") {
                    found = true;
                    break;
                }
            }
        }

        if found {
            break;
        }
    }

    assert!(
        found,
        "expected at least one cross-faction marriage alliance across 4 seeds x 500 years"
    );
}

#[test]
fn thousand_year_family_simulation() {
    let world = generate_and_run(42, 1000);

    // Family relationships exist
    let parent_count = world
        .collect_relationships()
        .filter(|r| r.kind == RelationshipKind::Parent)
        .count();
    assert!(
        parent_count > 10,
        "expected many Parent rels, got {parent_count}"
    );

    // Marriages occurred
    let union_count = world
        .events
        .values()
        .filter(|e| e.kind == EventKind::Union)
        .count();
    assert!(union_count > 0, "expected Union events in 1000 years");

    // Dynasties visible (shared surnames)
    let mut surnames: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for e in world.entities.values() {
        if e.kind == EntityKind::Person {
            let base = e.name.split(" the ").next().unwrap_or(&e.name);
            if let Some((_, surname)) = base.rsplit_once(' ') {
                *surnames.entry(surname.to_string()).or_default() += 1;
            }
        }
    }
    let dynasty_names = surnames.values().filter(|&&count| count >= 3).count();
    assert!(
        dynasty_names > 0,
        "expected at least one surname used 3+ times in 1000 years"
    );

    // Succession events happened
    let successions = world
        .events
        .values()
        .filter(|e| e.kind == EventKind::Succession)
        .count();
    assert!(successions > 0, "expected succession events");

    // Living persons exist
    let living = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Person && e.end.is_none())
        .count();
    assert!(living > 0, "expected living persons after 1000 years");
}

#[test]
fn determinism_preserved_with_family() {
    let world1 = generate_and_run(99, 200);
    let world2 = generate_and_run(99, 200);

    let count1 = world1.entities.len();
    let count2 = world2.entities.len();
    assert_eq!(
        count1, count2,
        "same seed should produce same entity count: {count1} vs {count2}"
    );

    let event_count1 = world1.events.len();
    let event_count2 = world2.events.len();
    assert_eq!(
        event_count1, event_count2,
        "same seed should produce same event count: {event_count1} vs {event_count2}"
    );

    // Also check relationship counts
    let rel_count1 = world1.collect_relationships().count();
    let rel_count2 = world2.collect_relationships().count();
    assert_eq!(
        rel_count1, rel_count2,
        "same seed should produce same relationship count: {rel_count1} vs {rel_count2}"
    );
}
