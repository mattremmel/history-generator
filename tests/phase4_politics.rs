use history_gen::model::{EventKind, GovernmentType};
use history_gen::scenario::Scenario;
use history_gen::testutil;
use history_gen::{
    ActionSystem, AgencySystem, ConflictSystem, DemographicsSystem, EconomySystem, PoliticsSystem,
    SimSystem,
};

#[test]
fn determinism_same_seed() {
    let world1 = testutil::generate_and_run(99, 50, testutil::core_systems());
    let world2 = testutil::generate_and_run(99, 50, testutil::core_systems());

    testutil::assert_deterministic(&world1, &world2);
}

#[test]
fn succession_claims_and_crises_occur() {
    // Chain: coup in unstable hereditary faction → deposed claims for cross-faction
    // relatives → Agency generates PressClaim desire → Actions presses claim →
    // WarDeclared with "pressed their claim". We set up conditions where coups fire
    // quickly and deposed leaders have children in rival factions.
    let mut total_crises = 0;
    let mut total_claim_wars = 0;

    for seed in 0u64..50 {
        let mut s = Scenario::at_year(100);

        // Create unstable hereditary kingdoms primed for coups
        let ka = s.add_kingdom_with(
            "Dynasty A",
            |fd| {
                fd.government_type = GovernmentType::Hereditary;
                fd.stability = 0.2;
                fd.happiness = 0.15;
                fd.legitimacy = 0.2;
            },
            |sd| sd.population = 200,
            |_| {},
        );
        let kb = s.add_rival_kingdom_with(
            "Dynasty B",
            ka.region,
            |fd| {
                fd.government_type = GovernmentType::Hereditary;
                fd.stability = 0.2;
                fd.happiness = 0.15;
                fd.legitimacy = 0.2;
            },
            |sd| sd.population = 200,
            |_| {},
        );

        // Cross-faction children: when a leader is deposed by coup, their child in
        // the rival faction gets a deposed claim (0.7 strength), and if that child
        // becomes leader the Agency system will generate a PressClaim desire
        let child_in_b = s
            .person_in("Prince of A", kb.faction, kb.settlement)
            .birth_year(70)
            .id();
        s.make_parent_child(ka.leader, child_in_b);
        let child_in_a = s
            .person_in("Prince of B", ka.faction, ka.settlement)
            .birth_year(70)
            .id();
        s.make_parent_child(kb.leader, child_in_a);

        // Coup instigator candidates
        for i in 0..4 {
            s.person_in(&format!("Noble A-{i}"), ka.faction, ka.settlement)
                .birth_year(70)
                .id();
            s.person_in(&format!("Noble B-{i}"), kb.faction, kb.settlement)
                .birth_year(70)
                .id();
        }

        let mut systems: Vec<Box<dyn SimSystem>> = vec![
            Box::new(DemographicsSystem),
            Box::new(EconomySystem),
            Box::new(ConflictSystem),
            Box::new(PoliticsSystem),
            Box::new(AgencySystem::default()),
            Box::new(ActionSystem),
        ];
        let world = s.run(&mut systems, 50, seed);

        total_crises += world
            .events
            .values()
            .filter(|e| e.kind == EventKind::SuccessionCrisis)
            .count();

        total_claim_wars += world
            .events
            .values()
            .filter(|e| {
                e.kind == EventKind::WarDeclared && e.description.contains("pressed their claim")
            })
            .count();

        if total_crises > 0 || total_claim_wars > 0 {
            break;
        }
    }

    assert!(
        total_crises > 0 || total_claim_wars > 0,
        "expected at least one succession crisis or claim war across 50 seeds × 50-year runs \
         (got {total_crises} crises, {total_claim_wars} claim wars)"
    );
}
