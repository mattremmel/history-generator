use history_gen::procgen;
use history_gen::procgen::ProcGenConfig;
use history_gen::scenario::Scenario;

/// Build a world with settlements, persons, and resources using Scenario builder
/// instead of running worldgen + simulation.
fn make_world_with_settlements() -> (history_gen::model::World, Vec<u64>) {
    // Start at year 1 so settlements have 99 years of "age" when queried at year 100
    let mut s = Scenario::new();
    let region = s.add_region_with("Plains", |rd| {
        rd.terrain = "plains".to_string();
    });
    let faction = s.add_faction("Kingdom");

    let s1 = s.add_settlement_with("Riverdale", faction, region, |sd| {
        sd.population = 300;
        sd.prosperity = 0.6;
    });
    let s2 = s.add_settlement_with("Hilltop", faction, region, |sd| {
        sd.population = 150;
        sd.prosperity = 0.4;
    });

    // Set resources on settlements (typed field, not extra)
    s.modify_settlement(s1, |sd| sd.resources = vec!["grain".into(), "iron".into()]);
    s.modify_settlement(s2, |sd| {
        sd.resources = vec!["timber".into(), "stone".into()]
    });

    let leader = s.add_person("King", faction);
    s.make_leader(leader, faction);

    (s.build(), vec![s1, s2])
}

#[test]
fn snapshot_from_world_produces_valid_snapshots() {
    let (world, settlements) = make_world_with_settlements();

    for &sid in &settlements {
        let snapshot = procgen::snapshot_from_world(&world, sid, 100)
            .expect("should produce snapshot for living settlement");

        assert_eq!(snapshot.settlement_id, sid);
        assert!(!snapshot.name.is_empty());
        assert!(snapshot.population.total() > 0);
        assert_eq!(snapshot.year, 100);
    }
}

#[test]
fn generate_details_produces_content() {
    let (world, settlements) = make_world_with_settlements();
    let config = ProcGenConfig::default();

    for &sid in &settlements {
        let snapshot = procgen::snapshot_from_world(&world, sid, 100).unwrap();
        let details = procgen::generate_settlement_details(&snapshot, &config);

        assert!(
            !details.inhabitants.is_empty(),
            "settlement {} should have inhabitants",
            snapshot.name
        );
        assert!(
            !details.artifacts.is_empty(),
            "settlement {} should have artifacts",
            snapshot.name
        );
        assert!(
            !details.writings.is_empty(),
            "settlement {} should have writings",
            snapshot.name
        );
    }
}

#[test]
fn deterministic_output() {
    let (world, settlements) = make_world_with_settlements();
    let config = ProcGenConfig::default();

    let sid = settlements[0];
    let snapshot = procgen::snapshot_from_world(&world, sid, 100).unwrap();

    let details1 = procgen::generate_settlement_details(&snapshot, &config);
    let details2 = procgen::generate_settlement_details(&snapshot, &config);

    assert_eq!(details1.inhabitants.len(), details2.inhabitants.len());
    assert_eq!(details1.artifacts.len(), details2.artifacts.len());
    assert_eq!(details1.writings.len(), details2.writings.len());

    for (a, b) in details1.inhabitants.iter().zip(details2.inhabitants.iter()) {
        assert_eq!(a.name, b.name);
        assert_eq!(a.age, b.age);
        assert_eq!(a.occupation, b.occupation);
    }

    for (a, b) in details1.artifacts.iter().zip(details2.artifacts.iter()) {
        assert_eq!(a.name, b.name);
        assert_eq!(a.age_years, b.age_years);
    }

    for (a, b) in details1.writings.iter().zip(details2.writings.iter()) {
        assert_eq!(a.text, b.text);
        assert_eq!(a.year_written, b.year_written);
    }
}

#[test]
fn ages_are_valid() {
    let (world, settlements) = make_world_with_settlements();
    let config = ProcGenConfig::default();

    let sid = settlements[0];
    let snapshot = procgen::snapshot_from_world(&world, sid, 100).unwrap();
    let details = procgen::generate_settlement_details(&snapshot, &config);

    for person in &details.inhabitants {
        assert!(person.age <= 110, "person age {} exceeds max", person.age);
    }

    for artifact in &details.artifacts {
        assert!(
            artifact.age_years <= 100,
            "artifact age {} exceeds settlement age 100",
            artifact.age_years
        );
    }
}

#[test]
fn no_id_collisions_across_categories() {
    let (world, settlements) = make_world_with_settlements();
    let config = ProcGenConfig::default();

    let sid = settlements[0];
    let snapshot = procgen::snapshot_from_world(&world, sid, 100).unwrap();
    let details = procgen::generate_settlement_details(&snapshot, &config);

    let mut all_ids: Vec<u64> = Vec::new();
    all_ids.extend(details.inhabitants.iter().map(|p| p.id));
    all_ids.extend(details.artifacts.iter().map(|a| a.id));
    all_ids.extend(details.writings.iter().map(|w| w.id));

    let count_before = all_ids.len();
    all_ids.sort();
    all_ids.dedup();
    assert_eq!(
        count_before,
        all_ids.len(),
        "all IDs should be unique across inhabitants, artifacts, and writings"
    );

    // None should collide with simulation entity IDs
    for &id in &all_ids {
        assert!(
            !world.entities.contains_key(&id),
            "procgen ID {id} collides with a simulation entity"
        );
    }
}
