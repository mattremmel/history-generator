use history_gen::testutil;

#[test]
fn determinism_same_seed() {
    let world1 = testutil::generate_and_run(99, 50, testutil::core_systems());
    let world2 = testutil::generate_and_run(99, 50, testutil::core_systems());

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
