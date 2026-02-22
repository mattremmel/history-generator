use history_gen::model::*;

pub fn build_test_world() -> World {
    let mut world = World::new();

    // 4 entities: 2 people, 1 settlement, 1 faction
    let alice = world.add_entity(EntityKind::Person, "Alice".to_string(), Some(100));
    let bob = world.add_entity(EntityKind::Person, "Bob".to_string(), Some(105));
    let ironhold = world.add_entity(EntityKind::Settlement, "Ironhold".to_string(), None);
    let guild = world.add_entity(EntityKind::Faction, "Merchant Guild".to_string(), None);

    // 3 relationships
    world.add_relationship(alice, bob, RelationshipKind::Spouse, 125);
    world.add_relationship(alice, guild, RelationshipKind::MemberOf, 120);
    world.add_relationship(bob, ironhold, RelationshipKind::RulerOf, 130);

    // 1 event with 2 participants
    let marriage = world.add_event(
        EventKind::Marriage,
        125,
        "Alice and Bob wed in Ironhold".to_string(),
    );
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
