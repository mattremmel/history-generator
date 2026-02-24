use history_gen::model::World;
use history_gen::sim::{DemographicsSystem, EconomySystem, PoliticsSystem};
use history_gen::testutil;

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
