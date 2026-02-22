mod common;

use history_gen::flush::flush_to_jsonl;

#[test]
fn flush_produces_valid_jsonl_files() {
    let world = common::build_test_world();
    let dir = tempfile::tempdir().unwrap();

    flush_to_jsonl(&world, dir.path()).unwrap();

    // All 5 files exist
    let entities_path = dir.path().join("entities.jsonl");
    let rels_path = dir.path().join("relationships.jsonl");
    let events_path = dir.path().join("events.jsonl");
    let participants_path = dir.path().join("event_participants.jsonl");
    let effects_path = dir.path().join("event_effects.jsonl");

    assert!(entities_path.exists());
    assert!(rels_path.exists());
    assert!(events_path.exists());
    assert!(participants_path.exists());
    assert!(effects_path.exists());

    // Correct line counts
    let entities_lines = common::read_lines(&entities_path);
    let rels_lines = common::read_lines(&rels_path);
    let events_lines = common::read_lines(&events_path);
    let participants_lines = common::read_lines(&participants_path);
    let effects_lines = common::read_lines(&effects_path);

    assert_eq!(entities_lines.len(), 4, "expected 4 entities");
    assert_eq!(rels_lines.len(), 3, "expected 3 relationships");
    // 7 original + 1 prop_ev + 2 new (death + spouse_end) = 10
    assert_eq!(events_lines.len(), 10, "expected 10 events");
    // 2 original + 1 death participant
    assert_eq!(participants_lines.len(), 3, "expected 3 participants");
    // 4 entity_created + 3 relationship_started + 1 property_changed + 1 entity_ended + 1 relationship_ended = 10
    assert_eq!(effects_lines.len(), 10, "expected 10 event effects");

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
        assert!(v.get("start").is_some());
    }

    for line in &events_lines {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert!(v.get("id").is_some());
        assert!(v.get("kind").is_some());
        assert!(v.get("timestamp").is_some());
        assert!(v.get("description").is_some());
    }

    for line in &participants_lines {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert!(v.get("event_id").is_some());
        assert!(v.get("entity_id").is_some());
        assert!(v.get("role").is_some());
    }

    for line in &effects_lines {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert!(v.get("event_id").is_some());
        assert!(v.get("entity_id").is_some());
        assert!(v.get("effect").is_some());
        // Tagged enum: effect must have a "type" field
        assert!(v["effect"].get("type").is_some());
    }

    // Properties appear on entities that have them
    let alice: serde_json::Value = serde_json::from_str(&entities_lines[0]).unwrap();
    assert_eq!(
        alice["properties"]["mana"], 42,
        "Alice should have mana property"
    );

    // Entities without properties omit the field
    let bob: serde_json::Value = serde_json::from_str(&entities_lines[1]).unwrap();
    assert!(
        bob.get("properties").is_none(),
        "Bob should have no properties"
    );

    // Event data appears when non-null
    let has_data = events_lines.iter().any(|line| {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        v.get("data").is_some()
    });
    assert!(has_data, "at least one event should have data");

    // Events without data omit the field
    let no_data = events_lines.iter().any(|line| {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        v.get("data").is_none()
    });
    assert!(no_data, "at least one event should omit data");
}

#[test]
fn flush_preserves_field_values() {
    let world = common::build_test_world();
    let dir = tempfile::tempdir().unwrap();

    flush_to_jsonl(&world, dir.path()).unwrap();

    let entities_lines = common::read_lines(&dir.path().join("entities.jsonl"));

    // First entity: Alice (person, origin year 100)
    let alice: serde_json::Value = serde_json::from_str(&entities_lines[0]).unwrap();
    assert_eq!(alice["kind"], "person");
    assert_eq!(alice["name"], "Alice");
    assert_eq!(alice["origin"]["year"], 100);
    assert_eq!(alice["origin"]["day"], 1);
    assert_eq!(alice["origin"]["hour"], 0);
    // Alice dies at year 170
    assert_eq!(alice["end"]["year"], 170);

    // Third entity: Ironhold (settlement, no origin)
    let ironhold: serde_json::Value = serde_json::from_str(&entities_lines[2]).unwrap();
    assert_eq!(ironhold["kind"], "settlement");
    assert_eq!(ironhold["name"], "Ironhold");
    assert!(ironhold["origin"].is_null());

    // Relationships: check kind is snake_case and timestamps serialize as objects
    let rels_lines = common::read_lines(&dir.path().join("relationships.jsonl"));
    let spouse_rel: serde_json::Value = serde_json::from_str(&rels_lines[0]).unwrap();
    assert_eq!(spouse_rel["kind"], "spouse");
    assert_eq!(spouse_rel["start"]["year"], 125);

    let member_rel: serde_json::Value = serde_json::from_str(&rels_lines[1]).unwrap();
    assert_eq!(member_rel["kind"], "member_of");

    let ruler_rel: serde_json::Value = serde_json::from_str(&rels_lines[2]).unwrap();
    assert_eq!(ruler_rel["kind"], "ruler_of");

    // Events: check timestamp is object and caused_by field
    let events_lines = common::read_lines(&dir.path().join("events.jsonl"));
    let event: serde_json::Value = serde_json::from_str(&events_lines[0]).unwrap();
    assert_eq!(event["kind"], "marriage");
    assert_eq!(event["timestamp"]["year"], 125);
    assert_eq!(event["description"], "Alice and Bob wed in Ironhold");
    assert!(event["caused_by"].is_null(), "root event has no cause");

    // The founding event should have data (population, terrain)
    let founding_event = events_lines
        .iter()
        .find(|line| {
            let v: serde_json::Value = serde_json::from_str(line).unwrap();
            v["description"] == "Ironhold founded"
        })
        .expect("founding event not found");
    let founding_json: serde_json::Value = serde_json::from_str(founding_event).unwrap();
    assert_eq!(founding_json["data"]["population"], 200);
    assert_eq!(founding_json["data"]["terrain"], "hills");

    // The spouse_end event (last event) should have caused_by set
    let last_event: serde_json::Value = serde_json::from_str(events_lines.last().unwrap()).unwrap();
    assert_eq!(last_event["kind"], "death");
    assert!(
        last_event["caused_by"].is_number(),
        "caused event should reference parent"
    );

    // Participants: check roles
    let parts_lines = common::read_lines(&dir.path().join("event_participants.jsonl"));
    let p1: serde_json::Value = serde_json::from_str(&parts_lines[0]).unwrap();
    assert_eq!(p1["role"], "subject");

    let p2: serde_json::Value = serde_json::from_str(&parts_lines[1]).unwrap();
    assert_eq!(p2["role"], "object");

    // Event effects: verify entity_created effects
    let effects_lines = common::read_lines(&dir.path().join("event_effects.jsonl"));
    // First effect should be entity_created for Alice
    let ef1: serde_json::Value = serde_json::from_str(&effects_lines[0]).unwrap();
    assert_eq!(ef1["effect"]["type"], "entity_created");
    assert_eq!(ef1["effect"]["kind"], "person");
    assert_eq!(ef1["effect"]["name"], "Alice");
}

#[test]
fn flush_timestamp_serializes_as_object() {
    let world = common::build_test_world();
    let dir = tempfile::tempdir().unwrap();

    flush_to_jsonl(&world, dir.path()).unwrap();

    // Verify timestamps are objects not integers
    let events_lines = common::read_lines(&dir.path().join("events.jsonl"));
    let event: serde_json::Value = serde_json::from_str(&events_lines[0]).unwrap();
    assert!(
        event["timestamp"].is_object(),
        "timestamp should be an object"
    );
    assert!(event["timestamp"]["year"].is_number());
    assert!(event["timestamp"]["day"].is_number());
    assert!(event["timestamp"]["hour"].is_number());
}
