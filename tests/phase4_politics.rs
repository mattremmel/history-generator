use history_gen::model::EventKind;
use history_gen::testutil;

#[test]
fn determinism_same_seed() {
    let world1 = testutil::generate_and_run(99, 50, testutil::core_systems());
    let world2 = testutil::generate_and_run(99, 50, testutil::core_systems());

    testutil::assert_deterministic(&world1, &world2);
}

#[test]
fn succession_claims_and_crises_occur_over_500_years() {
    // Run all systems for 500 years across multiple seeds to find
    // succession crises (which require Hereditary factions + leader death +
    // blood relatives in other factions).
    let mut total_crises = 0;
    let mut total_succession_wars = 0;

    for seed in 0u64..5 {
        let world = testutil::generate_and_run(seed, 500, testutil::all_systems());

        let crises = world
            .events
            .values()
            .filter(|e| e.kind == EventKind::SuccessionCrisis)
            .count();
        total_crises += crises;

        // Succession wars are WarDeclared events that mention "pressed their claim"
        let claim_wars = world
            .events
            .values()
            .filter(|e| {
                e.kind == EventKind::WarDeclared && e.description.contains("pressed their claim")
            })
            .count();
        total_succession_wars += claim_wars;
    }

    // We expect at least some succession crises across 5 seeds × 500 years
    // If none occur, the system isn't wired up correctly
    assert!(
        total_crises > 0 || total_succession_wars > 0,
        "expected at least one succession crisis or claim war across 5 × 500-year runs \
         (got {total_crises} crises, {total_succession_wars} claim wars)"
    );
}
