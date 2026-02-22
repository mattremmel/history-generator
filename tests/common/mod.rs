use history_gen::model::*;

pub fn build_test_world() -> World {
    let mut world = World::new();

    let ts = |year: u32| SimTimestamp::from_year(year);

    // 1 event: union (created first so entities can reference it)
    let union = world.add_event(
        EventKind::Union,
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
    world.add_relationship(alice, bob, RelationshipKind::Spouse, ts(125), union);
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

    // Union participants
    world.add_event_participant(union, alice, ParticipantRole::Subject);
    world.add_event_participant(union, bob, ParticipantRole::Object);

    // Set a property on Alice (exercises the property bag)
    let prop_ev = world.add_event(EventKind::Birth, ts(100), "Mana discovered".to_string());
    world.set_property(alice, "mana".to_string(), serde_json::json!(42), prop_ev);

    // Set data on the founding event (exercises the event data payload)
    world.events.get_mut(&founding).unwrap().data =
        serde_json::json!({"population": 200, "terrain": "hills"});

    // Custom event kind: plague outbreak
    let _plague = world.add_event(
        EventKind::Custom("plague_outbreak".to_string()),
        ts(140),
        "Plague strikes Ironhold".to_string(),
    );

    // Custom entity kind: a dragon
    let dragon_ev = world.add_event(
        EventKind::Custom("dragon_awakened".to_string()),
        ts(10),
        "Smaug awakens".to_string(),
    );
    let dragon = world.add_entity(
        EntityKind::Custom("dragon".to_string()),
        "Smaug".to_string(),
        Some(ts(10)),
        dragon_ev,
    );

    // Custom relationship kind: apprentice_of
    let apprentice_ev = world.add_event(
        EventKind::Custom("apprenticeship".to_string()),
        ts(115),
        "Bob apprentices under Smaug".to_string(),
    );
    world.add_relationship(
        bob,
        dragon,
        RelationshipKind::Custom("apprentice_of".to_string()),
        ts(115),
        apprentice_ev,
    );

    // Death of Alice at year 170, which causes the spouse relationship to end
    let death = world.add_event(EventKind::Death, ts(170), "Alice dies".to_string());
    world.end_entity(alice, ts(170), death);
    world.add_event_participant(death, alice, ParticipantRole::Subject);

    let spouse_end = world.add_caused_event(
        EventKind::Death,
        ts(170),
        "Spouse bond dissolved by death".to_string(),
        death,
    );
    world.end_relationship(alice, bob, &RelationshipKind::Spouse, ts(170), spouse_end);

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
