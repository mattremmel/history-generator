use history_gen::PopulationBreakdown;
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
fn thousand_year_demographics() {
    let world = generate_and_run(42, 1000);

    // Person entities were created
    let total_persons = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Person)
        .count();
    assert!(
        total_persons > 100,
        "expected many person entities, got {total_persons}"
    );

    // Some are alive, some are dead
    let living = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Person && e.end.is_none())
        .count();
    let dead = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Person && e.end.is_some())
        .count();
    assert!(living > 0, "expected some living persons");
    assert!(dead > 0, "expected some dead persons");

    // Most settlements survived with population >= 10
    let living_settlements = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
        .count();
    assert!(
        living_settlements > 0,
        "expected some surviving settlements"
    );

    // Events include births and deaths
    let birth_events = world
        .events
        .values()
        .filter(|e| e.kind == EventKind::Birth)
        .count();
    let death_events = world
        .events
        .values()
        .filter(|e| e.kind == EventKind::Death)
        .count();
    assert!(birth_events > 0, "expected birth events");
    assert!(death_events > 0, "expected death events");

    // Succession events exist (leadership changes)
    let succession_events = world
        .events
        .values()
        .filter(|e| e.kind == EventKind::Succession)
        .count();
    assert!(succession_events > 0, "expected succession events");

    // Living persons have relationships
    for person in world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Person && e.end.is_none())
    {
        let has_located_in = person
            .relationships
            .iter()
            .any(|r| r.kind == RelationshipKind::LocatedIn && r.end.is_none());
        assert!(
            has_located_in,
            "living person {} should have LocatedIn relationship",
            person.name
        );
    }

    // Some factions have rulers
    let rulers = world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Person
                && e.end.is_none()
                && e.relationships
                    .iter()
                    .any(|r| r.kind == RelationshipKind::RulerOf && r.end.is_none())
        })
        .count();
    assert!(rulers > 0, "expected some rulers");

    // Every living settlement has a population_breakdown property
    for settlement in world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
    {
        let bd_value = settlement
            .properties
            .get("population_breakdown")
            .unwrap_or_else(|| {
                panic!(
                    "settlement {} should have population_breakdown",
                    settlement.name
                )
            });
        let bd: PopulationBreakdown =
            serde_json::from_value(bd_value.clone()).unwrap_or_else(|e| {
                panic!(
                    "population_breakdown for {} should deserialize: {e}",
                    settlement.name
                )
            });

        // breakdown.total() matches population property
        let pop = settlement
            .properties
            .get("population")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        assert_eq!(
            bd.total(),
            pop,
            "breakdown total ({}) should match population property ({}) for {}",
            bd.total(),
            pop,
            settlement.name
        );

        // After 1000 years, brackets should have shifted from initial distribution —
        // specifically, elder+ brackets should have nonzero population
        let elderly = bd.bracket_total(4) + bd.bracket_total(5) + bd.bracket_total(6);
        assert!(
            elderly > 0 || pop < 50,
            "settlement {} with pop {} should have some elderly after 1000 years",
            settlement.name,
            pop
        );

        // Settlement should have prosperity
        assert!(
            settlement.has_property("prosperity"),
            "settlement {} missing prosperity",
            settlement.name
        );
        let prosperity = settlement.properties["prosperity"].as_f64().unwrap();
        assert!(
            (0.0..=1.0).contains(&prosperity),
            "settlement {} prosperity {} out of range",
            settlement.name,
            prosperity
        );
    }
}

#[test]
fn determinism_same_seed() {
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
}

#[test]
fn flush_checkpoints_written() {
    let seed = 77u64;
    let config = WorldGenConfig {
        seed,
        ..WorldGenConfig::default()
    };
    let mut world = worldgen::generate_world(&config);
    let mut systems: Vec<Box<dyn SimSystem>> =
        vec![Box::new(DemographicsSystem), Box::new(PoliticsSystem)];

    let tmp_dir = std::env::temp_dir().join(format!("history_gen_test_{}", seed));
    let _ = std::fs::remove_dir_all(&tmp_dir);

    run(
        &mut world,
        &mut systems,
        SimConfig {
            start_year: 1,
            num_years: 100,
            seed,
            flush_interval: Some(50),
            output_dir: Some(tmp_dir.clone()),
        },
    );

    // Should have checkpoint at year 50 and year 100 (final)
    assert!(
        tmp_dir.join("year_000050").exists(),
        "expected checkpoint at year 50"
    );
    assert!(
        tmp_dir.join("year_000100").exists(),
        "expected checkpoint at year 100 (final)"
    );

    // Checkpoint should contain JSONL files
    let checkpoint = tmp_dir.join("year_000100");
    assert!(checkpoint.join("entities.jsonl").exists());
    assert!(checkpoint.join("events.jsonl").exists());
    assert!(checkpoint.join("relationships.jsonl").exists());

    // Cleanup
    let _ = std::fs::remove_dir_all(&tmp_dir);
}
