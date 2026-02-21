use history_gen::flush::flush_to_jsonl;
use history_gen::model::*;

fn build_test_world() -> World {
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

#[test]
fn flush_produces_valid_jsonl_files() {
    let world = build_test_world();
    let dir = tempfile::tempdir().unwrap();

    flush_to_jsonl(&world, dir.path()).unwrap();

    // All 4 files exist
    let entities_path = dir.path().join("entities.jsonl");
    let rels_path = dir.path().join("relationships.jsonl");
    let events_path = dir.path().join("events.jsonl");
    let participants_path = dir.path().join("event_participants.jsonl");

    assert!(entities_path.exists());
    assert!(rels_path.exists());
    assert!(events_path.exists());
    assert!(participants_path.exists());

    // Correct line counts
    let entities_lines = read_lines(&entities_path);
    let rels_lines = read_lines(&rels_path);
    let events_lines = read_lines(&events_path);
    let participants_lines = read_lines(&participants_path);

    assert_eq!(entities_lines.len(), 4, "expected 4 entities");
    assert_eq!(rels_lines.len(), 3, "expected 3 relationships");
    assert_eq!(events_lines.len(), 1, "expected 1 event");
    assert_eq!(participants_lines.len(), 2, "expected 2 participants");

    // Each line is valid JSON with expected fields
    for line in &entities_lines {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert!(v.get("id").is_some());
        assert!(v.get("kind").is_some());
        assert!(v.get("name").is_some());
        // relationships field must NOT appear (serde skip)
        assert!(v.get("relationships").is_none());
    }

    for line in &rels_lines {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert!(v.get("source_entity_id").is_some());
        assert!(v.get("target_entity_id").is_some());
        assert!(v.get("kind").is_some());
        assert!(v.get("start_year").is_some());
    }

    for line in &events_lines {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert!(v.get("id").is_some());
        assert!(v.get("kind").is_some());
        assert!(v.get("year").is_some());
        assert!(v.get("description").is_some());
    }

    for line in &participants_lines {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert!(v.get("event_id").is_some());
        assert!(v.get("entity_id").is_some());
        assert!(v.get("role").is_some());
    }
}

#[test]
fn flush_preserves_field_values() {
    let world = build_test_world();
    let dir = tempfile::tempdir().unwrap();

    flush_to_jsonl(&world, dir.path()).unwrap();

    let entities_lines = read_lines(&dir.path().join("entities.jsonl"));

    // First entity: Alice (person, birth_year 100, no death_year)
    let alice: serde_json::Value = serde_json::from_str(&entities_lines[0]).unwrap();
    assert_eq!(alice["kind"], "person");
    assert_eq!(alice["name"], "Alice");
    assert_eq!(alice["birth_year"], 100);
    assert!(alice["death_year"].is_null());

    // Third entity: Ironhold (settlement, no birth/death year)
    let ironhold: serde_json::Value = serde_json::from_str(&entities_lines[2]).unwrap();
    assert_eq!(ironhold["kind"], "settlement");
    assert_eq!(ironhold["name"], "Ironhold");
    assert!(ironhold["birth_year"].is_null());

    // Relationships: check kind is snake_case
    let rels_lines = read_lines(&dir.path().join("relationships.jsonl"));
    let spouse_rel: serde_json::Value = serde_json::from_str(&rels_lines[0]).unwrap();
    assert_eq!(spouse_rel["kind"], "spouse");

    let member_rel: serde_json::Value = serde_json::from_str(&rels_lines[1]).unwrap();
    assert_eq!(member_rel["kind"], "member_of");

    let ruler_rel: serde_json::Value = serde_json::from_str(&rels_lines[2]).unwrap();
    assert_eq!(ruler_rel["kind"], "ruler_of");

    // Event: marriage
    let events_lines = read_lines(&dir.path().join("events.jsonl"));
    let event: serde_json::Value = serde_json::from_str(&events_lines[0]).unwrap();
    assert_eq!(event["kind"], "marriage");
    assert_eq!(event["year"], 125);
    assert_eq!(event["description"], "Alice and Bob wed in Ironhold");

    // Participants: check roles
    let parts_lines = read_lines(&dir.path().join("event_participants.jsonl"));
    let p1: serde_json::Value = serde_json::from_str(&parts_lines[0]).unwrap();
    assert_eq!(p1["role"], "subject");

    let p2: serde_json::Value = serde_json::from_str(&parts_lines[1]).unwrap();
    assert_eq!(p2["role"], "object");
}

fn read_lines(path: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}
