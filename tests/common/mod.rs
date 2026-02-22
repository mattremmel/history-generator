use history_gen::model::*;

pub fn build_test_world() -> World {
    let mut world = World::new();

    let ts = |year: u32| SimTimestamp::from_year(year);

    // 1 event: marriage (created first so entities can reference it)
    let marriage = world.add_event(
        EventKind::Marriage,
        ts(125),
        "Alice and Bob wed in Ironhold".to_string(),
    );

    // Birth events for entities
    let birth_alice = world.add_event(EventKind::Birth, ts(100), "Alice is born".to_string());
    let birth_bob = world.add_event(EventKind::Birth, ts(105), "Bob is born".to_string());
    let founding = world.add_event(
        EventKind::SettlementFounded,
        ts(50),
        "Ironhold founded".to_string(),
    );
    let faction_ev = world.add_event(
        EventKind::FactionFormed,
        ts(80),
        "Merchant Guild formed".to_string(),
    );

    // 4 entities: 2 people, 1 settlement, 1 faction
    let alice = world.add_entity(
        EntityKind::Person,
        "Alice".to_string(),
        Some(ts(100)),
        birth_alice,
    );
    let bob = world.add_entity(
        EntityKind::Person,
        "Bob".to_string(),
        Some(ts(105)),
        birth_bob,
    );
    let ironhold = world.add_entity(
        EntityKind::Settlement,
        "Ironhold".to_string(),
        None,
        founding,
    );
    let _guild = world.add_entity(
        EntityKind::Faction,
        "Merchant Guild".to_string(),
        None,
        faction_ev,
    );

    // 3 relationships
    world.add_relationship(alice, bob, RelationshipKind::Spouse, ts(125), marriage);
    let join_ev = world.add_event(
        EventKind::FactionFormed,
        ts(120),
        "Alice joins guild".to_string(),
    );
    world.add_relationship(alice, _guild, RelationshipKind::MemberOf, ts(120), join_ev);
    let rule_ev = world.add_event(
        EventKind::SettlementFounded,
        ts(130),
        "Bob rules Ironhold".to_string(),
    );
    world.add_relationship(bob, ironhold, RelationshipKind::RulerOf, ts(130), rule_ev);

    // Marriage participants
    world.add_event_participant(marriage, alice, ParticipantRole::Subject);
    world.add_event_participant(marriage, bob, ParticipantRole::Object);

    world
}

pub fn read_lines(path: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}
