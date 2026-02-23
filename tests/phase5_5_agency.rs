use history_gen::model::action::ActionSource;
use history_gen::model::traits::{Trait, get_npc_traits};
use history_gen::model::{EntityKind, EventKind, World};
use history_gen::sim::{
    ActionSystem, AgencySystem, ConflictSystem, DemographicsSystem, EconomySystem,
    PoliticsSystem, SimConfig, SimSystem, run,
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
        Box::new(AgencySystem::new()),
        Box::new(ActionSystem),
        Box::new(ConflictSystem),
    ];
    run(&mut world, &mut systems, SimConfig::new(1, num_years, seed));
    world
}

#[test]
fn npcs_have_traits_at_birth() {
    let world = generate_and_run(42, 100);

    let persons_with_traits: Vec<_> = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Person && e.has_property("traits"))
        .collect();

    assert!(
        !persons_with_traits.is_empty(),
        "should have persons with traits after 100 years"
    );

    for person in &persons_with_traits {
        let traits = get_npc_traits(person);
        assert!(
            traits.len() >= 2 && traits.len() <= 4,
            "NPC {} has {} traits, expected 2-4",
            person.name,
            traits.len()
        );
    }
}

#[test]
fn traits_respect_opposing_pairs() {
    let world = generate_and_run(99, 200);

    let opposing_pairs = [
        (Trait::Ambitious, Trait::Content),
        (Trait::Aggressive, Trait::Cautious),
        (Trait::Charismatic, Trait::Reclusive),
        (Trait::Honorable, Trait::Ruthless),
        (Trait::Pious, Trait::Skeptical),
        (Trait::Cunning, Trait::Straightforward),
    ];

    for person in world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Person && e.has_property("traits"))
    {
        let traits = get_npc_traits(person);
        for (a, b) in &opposing_pairs {
            assert!(
                !(traits.contains(a) && traits.contains(b)),
                "NPC {} has opposing traits {:?} and {:?}",
                person.name,
                a,
                b
            );
        }
    }
}

#[test]
fn autonomous_actions_appear_in_results() {
    // Run long enough for agency system to produce actions
    let world = generate_and_run(42, 500);

    let autonomous_results: Vec<_> = world
        .action_results
        .iter()
        .filter(|r| matches!(r.source, ActionSource::Autonomous))
        .collect();

    assert!(
        !autonomous_results.is_empty(),
        "should have autonomous action results after 500 years"
    );
}

#[test]
fn trait_distribution_is_role_weighted() {
    let world = generate_and_run(42, 200);

    let mut warrior_aggressive = 0u32;
    let mut warrior_total = 0u32;
    let mut scholar_cunning = 0u32;
    let mut scholar_total = 0u32;

    for person in world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Person && e.has_property("traits"))
    {
        let role = person.get_property::<String>("role").unwrap_or_default();
        let traits = get_npc_traits(person);

        match role.as_str() {
            "warrior" => {
                warrior_total += 1;
                if traits.contains(&Trait::Aggressive) {
                    warrior_aggressive += 1;
                }
            }
            "scholar" => {
                scholar_total += 1;
                if traits.contains(&Trait::Cunning) {
                    scholar_cunning += 1;
                }
            }
            _ => {}
        }
    }

    // Warriors should have a noticeable skew toward Aggressive
    if warrior_total > 20 {
        let warrior_rate = warrior_aggressive as f64 / warrior_total as f64;
        assert!(
            warrior_rate > 0.1,
            "warriors should skew aggressive: {warrior_aggressive}/{warrior_total} = {warrior_rate:.2}"
        );
    }

    // Scholars should have a noticeable skew toward Cunning
    if scholar_total > 10 {
        let scholar_rate = scholar_cunning as f64 / scholar_total as f64;
        assert!(
            scholar_rate > 0.1,
            "scholars should skew cunning: {scholar_cunning}/{scholar_total} = {scholar_rate:.2}"
        );
    }
}

#[test]
fn thousand_year_agency_simulation() {
    // Full 1000-year simulation with all systems including agency
    let world = generate_and_run(777, 1000);

    // Basic sanity: persons exist, traits assigned
    let trait_count = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Person && e.has_property("traits"))
        .count();
    assert!(
        trait_count > 50,
        "should have many NPCs with traits after 1000 years, got {trait_count}"
    );

    // Autonomous actions occurred
    let autonomous_count = world
        .action_results
        .iter()
        .filter(|r| matches!(r.source, ActionSource::Autonomous))
        .count();
    assert!(
        autonomous_count > 0,
        "should have autonomous actions in a 1000-year sim"
    );

    // Coups still happen (system didn't break existing behavior)
    let coups = world
        .events
        .values()
        .filter(|e| e.kind == EventKind::Coup)
        .count();
    assert!(coups > 0, "should still have coups in a 1000-year sim");

    // Wars still happen
    let wars = world
        .events
        .values()
        .filter(|e| e.kind == EventKind::WarDeclared)
        .count();
    assert!(wars > 0, "should still have wars in a 1000-year sim");
}

#[test]
fn determinism_preserved_with_agency() {
    let world_a = generate_and_run(42, 100);
    let world_b = generate_and_run(42, 100);

    assert_eq!(world_a.entities.len(), world_b.entities.len());
    assert_eq!(world_a.events.len(), world_b.events.len());
    assert_eq!(world_a.action_results.len(), world_b.action_results.len());
}

#[test]
fn npc_driven_events_have_instigators() {
    use history_gen::model::ParticipantRole;

    let world = generate_and_run(42, 500);

    // Find assassination events from autonomous actions
    let assassinations: Vec<_> = world
        .events
        .values()
        .filter(|e| e.kind == EventKind::Custom("assassination".to_string()))
        .collect();

    // Find coup events
    let coups: Vec<_> = world
        .events
        .values()
        .filter(|e| e.kind == EventKind::Coup)
        .collect();

    let all_events: Vec<_> = assassinations.iter().chain(coups.iter()).collect();

    if !all_events.is_empty() {
        // Every assassination/coup should have an Instigator participant
        for event in all_events {
            let has_instigator = world
                .event_participants
                .iter()
                .any(|p| p.event_id == event.id && p.role == ParticipantRole::Instigator);
            assert!(
                has_instigator,
                "event {:?} '{}' should have an Instigator participant",
                event.kind, event.description
            );
        }
    }
}

#[test]
fn defections_occur_in_long_simulation() {
    // Try multiple seeds since defections require specific conditions
    let mut total_defections = 0;
    for seed in [42, 99, 123, 777] {
        let world = generate_and_run(seed, 1000);
        total_defections += world
            .events
            .values()
            .filter(|e| e.kind == EventKind::Custom("defection".to_string()))
            .count();
    }
    assert!(
        total_defections > 0,
        "expected at least one defection across 4 seeds x 1000 years, got {total_defections}"
    );
}

#[test]
fn seek_office_events_occur() {
    // Try multiple seeds since seek_office requires elective governments
    let mut total_successions = 0;
    let mut total_failed_elections = 0;
    for seed in [42, 99, 123, 777] {
        let world = generate_and_run(seed, 1000);
        // Count succession events that mention "claimed" or "elected" (from SeekOffice)
        total_successions += world
            .events
            .values()
            .filter(|e| {
                e.kind == EventKind::Succession
                    && (e.description.contains("claimed leadership")
                        || e.description.contains("was elected"))
            })
            .count();
        total_failed_elections += world
            .events
            .values()
            .filter(|e| e.kind == EventKind::Custom("failed_election".to_string()))
            .count();
    }
    assert!(
        total_successions + total_failed_elections > 0,
        "expected at least one seek_office attempt across 4 seeds x 1000 years (successions: {total_successions}, failed: {total_failed_elections})"
    );
}
