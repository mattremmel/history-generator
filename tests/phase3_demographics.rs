use history_gen::sim::{
    DemographicsSystem, EconomySystem, PoliticsSystem, SimConfig, SimSystem, run,
};
use history_gen::testutil;
use history_gen::worldgen::{self, config::WorldGenConfig};

#[test]
fn determinism_same_seed() {
    let world1 = testutil::generate_and_run(99, 50, testutil::core_systems());
    let world2 = testutil::generate_and_run(99, 50, testutil::core_systems());

    testutil::assert_deterministic(&world1, &world2);
}

#[test]
fn flush_checkpoints_written() {
    let seed = 77u64;
    let config = WorldGenConfig {
        seed,
        ..WorldGenConfig::default()
    };
    let mut world = worldgen::generate_world(config);
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
