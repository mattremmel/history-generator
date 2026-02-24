use history_gen::model::World;
use history_gen::sim::{
    DemographicsSystem, EconomySystem, PoliticsSystem, SimConfig, SimSystem, run,
};
use history_gen::testutil;
use history_gen::worldgen::{self, config::WorldGenConfig};

fn generate_and_run(seed: u64, num_years: u32) -> World {
    testutil::generate_and_run(
        seed,
        num_years,
        vec![
            Box::new(DemographicsSystem),
            Box::new(EconomySystem),
            Box::new(PoliticsSystem),
        ],
    )
}

#[test]
fn determinism_same_seed() {
    let world1 = generate_and_run(99, 50);
    let world2 = generate_and_run(99, 50);

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
    let mut systems: Vec<Box<dyn SimSystem>> = vec![
        Box::new(DemographicsSystem),
        Box::new(EconomySystem),
        Box::new(PoliticsSystem),
    ];

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
