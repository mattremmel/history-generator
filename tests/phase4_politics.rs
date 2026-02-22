use history_gen::model::{EntityKind, EventKind, RelationshipKind, World};
use history_gen::sim::{DemographicsSystem, PoliticsSystem, SimConfig, SimSystem, run};
use history_gen::worldgen::{self, config::WorldGenConfig};

fn generate_and_run(seed: u64, num_years: u32) -> World {
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
fn thousand_year_politics() {
    let world = generate_and_run(42, 1000);

    // Living factions exist
    let living_factions: Vec<u64> = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        .map(|e| e.id)
        .collect();
    assert!(
        !living_factions.is_empty(),
        "expected living factions after 1000 years"
    );

    // Some factions have rulers (Person with active RulerOf -> Faction)
    let ruled_factions: usize = living_factions
        .iter()
        .filter(|&&fid| {
            world.entities.values().any(|e| {
                e.kind == EntityKind::Person
                    && e.end.is_none()
                    && e.relationships.iter().any(|r| {
                        r.kind == RelationshipKind::RulerOf
                            && r.target_entity_id == fid
                            && r.end.is_none()
                    })
            })
        })
        .count();
    assert!(ruled_factions > 0, "expected some factions to have rulers");

    // Succession events occurred
    let succession_count = world
        .events
        .values()
        .filter(|e| e.kind == EventKind::Succession)
        .count();
    assert!(
        succession_count > 0,
        "expected succession events after 1000 years"
    );

    // FactionFormed events exist (at least from worldgen, possibly from splits)
    let formed_count = world
        .events
        .values()
        .filter(|e| e.kind == EventKind::FactionFormed)
        .count();
    assert!(formed_count > 0, "expected FactionFormed events");

    // Some inter-faction relationships exist (Ally or Enemy)
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
        "expected some diplomatic relationships after 1000 years"
    );

    // All living factions have stability and government_type properties
    for faction in world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
    {
        assert!(
            faction.has_property("stability"),
            "faction {} missing stability",
            faction.name
        );
        assert!(
            faction.has_property("government_type"),
            "faction {} missing government_type",
            faction.name
        );

        let stability = faction.properties["stability"].as_f64().unwrap();
        assert!(
            (0.0..=1.0).contains(&stability),
            "faction {} stability {} out of range",
            faction.name,
            stability
        );
    }

    // Every living settlement belongs to exactly one faction
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

    // Coup events occurred (probabilistic, but likely in 1000 years)
    let coup_count = world
        .events
        .values()
        .filter(|e| e.kind == EventKind::Coup)
        .count();
    assert!(coup_count > 0, "expected at least one coup in 1000 years");
}

#[test]
fn determinism_same_seed() {
    let world1 = generate_and_run(99, 200);
    let world2 = generate_and_run(99, 200);

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
