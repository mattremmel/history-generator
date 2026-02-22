mod common;

use history_gen::flush::flush_to_jsonl;

#[test]
fn flush_produces_valid_jsonl_files() {
    let world = common::build_test_world();
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
    let entities_lines = common::read_lines(&entities_path);
    let rels_lines = common::read_lines(&rels_path);
    let events_lines = common::read_lines(&events_path);
    let participants_lines = common::read_lines(&participants_path);

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
    let world = common::build_test_world();
    let dir = tempfile::tempdir().unwrap();

    flush_to_jsonl(&world, dir.path()).unwrap();

    let entities_lines = common::read_lines(&dir.path().join("entities.jsonl"));

    // First entity: Alice (person, origin_year 100, no end_year)
    let alice: serde_json::Value = serde_json::from_str(&entities_lines[0]).unwrap();
    assert_eq!(alice["kind"], "person");
    assert_eq!(alice["name"], "Alice");
    assert_eq!(alice["origin_year"], 100);
    assert!(alice["end_year"].is_null());

    // Third entity: Ironhold (settlement, no birth/death year)
    let ironhold: serde_json::Value = serde_json::from_str(&entities_lines[2]).unwrap();
    assert_eq!(ironhold["kind"], "settlement");
    assert_eq!(ironhold["name"], "Ironhold");
    assert!(ironhold["origin_year"].is_null());

    // Relationships: check kind is snake_case
    let rels_lines = common::read_lines(&dir.path().join("relationships.jsonl"));
    let spouse_rel: serde_json::Value = serde_json::from_str(&rels_lines[0]).unwrap();
    assert_eq!(spouse_rel["kind"], "spouse");

    let member_rel: serde_json::Value = serde_json::from_str(&rels_lines[1]).unwrap();
    assert_eq!(member_rel["kind"], "member_of");

    let ruler_rel: serde_json::Value = serde_json::from_str(&rels_lines[2]).unwrap();
    assert_eq!(ruler_rel["kind"], "ruler_of");

    // Event: marriage
    let events_lines = common::read_lines(&dir.path().join("events.jsonl"));
    let event: serde_json::Value = serde_json::from_str(&events_lines[0]).unwrap();
    assert_eq!(event["kind"], "marriage");
    assert_eq!(event["year"], 125);
    assert_eq!(event["description"], "Alice and Bob wed in Ironhold");

    // Participants: check roles
    let parts_lines = common::read_lines(&dir.path().join("event_participants.jsonl"));
    let p1: serde_json::Value = serde_json::from_str(&parts_lines[0]).unwrap();
    assert_eq!(p1["role"], "subject");

    let p2: serde_json::Value = serde_json::from_str(&parts_lines[1]).unwrap();
    assert_eq!(p2["role"], "object");
}
