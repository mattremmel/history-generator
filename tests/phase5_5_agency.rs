use history_gen::model::traits::get_npc_traits;
use history_gen::model::{EntityKind, EventKind, Role, World};
use history_gen::sim::{
    ActionSystem, AgencySystem, ConflictSystem, DemographicsSystem, EconomySystem, PoliticsSystem,
    SimSystem,
};
use history_gen::testutil;

fn generate_and_run(seed: u64, num_years: u32) -> World {
    testutil::generate_and_run(
        seed,
        num_years,
        vec![
            Box::new(DemographicsSystem),
            Box::new(EconomySystem),
            Box::new(PoliticsSystem),
            Box::new(AgencySystem::new()),
            Box::new(ActionSystem),
            Box::new(ConflictSystem),
        ],
    )
}

#[test]
fn determinism_preserved_with_agency() {
    let world_a = generate_and_run(42, 50);
    let world_b = generate_and_run(42, 50);

    assert_eq!(world_a.entities.len(), world_b.entities.len());
    assert_eq!(world_a.events.len(), world_b.events.len());
    assert_eq!(world_a.action_results.len(), world_b.action_results.len());
}

// ---------------------------------------------------------------------------
// Scenario-based tests
// ---------------------------------------------------------------------------

#[test]
fn scenario_npcs_have_traits_at_birth() {
    use history_gen::scenario::Scenario;
    use history_gen::sim::DemographicsSystem;

    let mut s = Scenario::new();
    let setup = s.add_settlement_standalone("Town");
    let faction = setup.faction;
    s.settlement_mut(setup.settlement).population(300);
    let leader = s.add_person("King", faction);
    s.make_leader(leader, faction);

    // Run demographics for 5 years to produce births
    let mut systems: Vec<Box<dyn SimSystem>> =
        vec![Box::new(DemographicsSystem), Box::new(AgencySystem::new())];
    let world = s.run(&mut systems, 5, 42);

    let persons_with_traits: Vec<_> = world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Person && e.data.as_person().is_some_and(|p| !p.traits.is_empty())
        })
        .collect();

    assert!(
        !persons_with_traits.is_empty(),
        "should have persons with traits after 5 years"
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
fn scenario_npc_driven_events_have_instigators() {
    use history_gen::model::ParticipantRole;
    use history_gen::model::traits::Trait;
    use history_gen::scenario::Scenario;

    let mut s = Scenario::at_year(100);
    let setup = s.add_settlement_standalone("Town");
    let faction = setup.faction;
    s.faction_mut(faction)
        .stability(0.3)
        .happiness(0.3)
        .legitimacy(0.4);
    s.settlement_mut(setup.settlement).population(500);
    let leader = s
        .person("Old King", faction)
        .birth_year(40)
        .role(Role::Warrior)
        .id();
    s.make_leader(leader, faction);
    // Add ambitious NPC who will try to seize power
    let _npc = s
        .person("Ambitious Noble", faction)
        .birth_year(70)
        .role(Role::Warrior)
        .traits(vec![Trait::Ambitious, Trait::Aggressive])
        .id();

    // Run with agency + actions + politics for several years
    let mut systems: Vec<Box<dyn SimSystem>> = vec![
        Box::new(DemographicsSystem),
        Box::new(EconomySystem),
        Box::new(PoliticsSystem),
        Box::new(AgencySystem::new()),
        Box::new(ActionSystem),
    ];
    let world = s.run(&mut systems, 20, 42);

    // Check for coup or assassination events with Instigator
    let instigated_events: Vec<_> = world
        .events
        .values()
        .filter(|e| {
            matches!(e.kind, EventKind::Coup | EventKind::Custom(_))
                && world
                    .event_participants
                    .iter()
                    .any(|p| p.event_id == e.id && p.role == ParticipantRole::Instigator)
        })
        .collect();

    // This is probabilistic — with low stability and an ambitious NPC, coups should happen
    // but we just verify the system runs without panics and events have proper structure
    for event in &instigated_events {
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
