mod common;

use history_gen::SimTimestamp;
use history_gen::db::{load_world, migrate};
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use testcontainers::ContainerAsync;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;

async fn setup() -> (PgPool, ContainerAsync<Postgres>) {
    let container = Postgres::default().start().await.unwrap();
    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(5432).await.unwrap();
    let pool = PgPoolOptions::new()
        .connect(&format!(
            "postgres://postgres:postgres@{}:{}/postgres",
            host, port
        ))
        .await
        .unwrap();
    (pool, container)
}

#[tokio::test]
#[ignore]
async fn load_populates_all_tables() {
    let (pool, _container) = setup().await;
    let world = common::build_test_world();

    migrate(&pool).await.unwrap();
    load_world(&pool, &world).await.unwrap();

    let entity_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM entities")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(entity_count, 5);

    let rel_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM relationships")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rel_count, 4);

    let event_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(event_count, 13);

    let part_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM event_participants")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(part_count, 3);

    let effect_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM event_effects")
        .fetch_one(&pool)
        .await
        .unwrap();
    // 5 entity_created + 4 relationship_started + 1 property_changed + 1 entity_ended + 1 relationship_ended = 12
    assert_eq!(effect_count, 12);
}

#[tokio::test]
#[ignore]
async fn loaded_data_matches_source_values() {
    let (pool, _container) = setup().await;
    let world = common::build_test_world();

    migrate(&pool).await.unwrap();
    load_world(&pool, &world).await.unwrap();

    // --- Entities ---
    let rows = sqlx::query(
        "SELECT id, kind, name, origin_ts, end_ts, properties FROM entities ORDER BY id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 5);

    // Alice — has properties (mana: 42)
    assert_eq!(rows[0].get::<String, _>("kind"), "person");
    assert_eq!(rows[0].get::<String, _>("name"), "Alice");
    assert_eq!(
        rows[0].get::<Option<i32>, _>("origin_ts"),
        Some(SimTimestamp::from_year(100).as_u32() as i32)
    );
    assert_eq!(rows[0].get::<Option<i32>, _>("end_ts"), None);
    let alice_props: serde_json::Value = rows[0].get("properties");
    assert_eq!(alice_props["mana"], 42);

    // Bob — empty properties
    assert_eq!(rows[1].get::<String, _>("kind"), "person");
    assert_eq!(rows[1].get::<String, _>("name"), "Bob");
    assert_eq!(
        rows[1].get::<Option<i32>, _>("origin_ts"),
        Some(SimTimestamp::from_year(105).as_u32() as i32)
    );
    let bob_props: serde_json::Value = rows[1].get("properties");
    assert_eq!(bob_props, serde_json::json!({}));

    // Ironhold
    assert_eq!(rows[2].get::<String, _>("kind"), "settlement");
    assert_eq!(rows[2].get::<String, _>("name"), "Ironhold");
    assert_eq!(rows[2].get::<Option<i32>, _>("origin_ts"), None);

    // Merchant Guild
    assert_eq!(rows[3].get::<String, _>("kind"), "faction");
    assert_eq!(rows[3].get::<String, _>("name"), "Merchant Guild");

    // Smaug — custom entity kind
    assert_eq!(rows[4].get::<String, _>("kind"), "dragon");
    assert_eq!(rows[4].get::<String, _>("name"), "Smaug");

    // --- Relationships (ordered by start_ts) ---
    let rels = sqlx::query(
        "SELECT source_entity_id, target_entity_id, kind, start_ts, end_ts \
         FROM relationships ORDER BY start_ts",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(rels.len(), 4);

    // apprentice_of (start year 115) — custom relationship kind
    assert_eq!(rels[0].get::<String, _>("kind"), "apprentice_of");

    // member_of (start year 120)
    assert_eq!(rels[1].get::<String, _>("kind"), "member_of");
    assert_eq!(rels[1].get::<Option<i32>, _>("end_ts"), None);

    // spouse (start year 125)
    assert_eq!(rels[2].get::<String, _>("kind"), "spouse");

    // ruler_of (start year 130)
    assert_eq!(rels[3].get::<String, _>("kind"), "ruler_of");

    // --- Events ---
    let events = sqlx::query(
        "SELECT id, kind, timestamp, description, caused_by, data FROM events ORDER BY id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(events.len(), 13);

    // First event is the marriage (no cause)
    assert_eq!(events[0].get::<String, _>("kind"), "marriage");
    assert_eq!(
        events[0].get::<i32, _>("timestamp"),
        SimTimestamp::from_year(125).as_u32() as i32
    );
    assert_eq!(
        events[0].get::<String, _>("description"),
        "Alice and Bob wed in Ironhold"
    );
    assert_eq!(events[0].get::<Option<i64>, _>("caused_by"), None);
    // Marriage has no data
    assert_eq!(events[0].get::<Option<serde_json::Value>, _>("data"), None);

    // Founding event has data (population, terrain)
    let founding_row = events
        .iter()
        .find(|r| r.get::<String, _>("description") == "Ironhold founded")
        .expect("founding event");
    let founding_data: serde_json::Value = founding_row.get("data");
    assert_eq!(founding_data["population"], 200);
    assert_eq!(founding_data["terrain"], "hills");

    // Custom event kind appears correctly
    let plague_row = events
        .iter()
        .find(|r| r.get::<String, _>("description") == "Plague strikes Ironhold")
        .expect("plague event");
    assert_eq!(plague_row.get::<String, _>("kind"), "plague_outbreak");

    // Last event (spouse_end) should reference the death event (second-to-last)
    let death_id = events[events.len() - 2].get::<i64, _>("id");
    let spouse_end = &events[events.len() - 1];
    assert_eq!(
        spouse_end.get::<Option<i64>, _>("caused_by"),
        Some(death_id)
    );

    // --- Event participants (ordered by role for determinism) ---
    let parts =
        sqlx::query("SELECT event_id, entity_id, role FROM event_participants ORDER BY role")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0].get::<String, _>("role"), "object");
    assert_eq!(parts[1].get::<String, _>("role"), "subject");
    assert_eq!(parts[2].get::<String, _>("role"), "subject");

    // --- Event effects ---
    let effects = sqlx::query(
        "SELECT event_id, entity_id, effect_type, effect_data \
         FROM event_effects ORDER BY event_id, entity_id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(effects.len(), 12);

    // First should be entity_created for Alice
    let first_type = effects[0].get::<String, _>("effect_type");
    assert!(
        first_type == "entity_created" || first_type == "relationship_started",
        "expected entity_created or relationship_started, got {first_type}"
    );
}

#[tokio::test]
#[ignore]
async fn temporal_reconstruction_query() {
    let (pool, _container) = setup().await;
    let mut world = common::build_test_world();

    // Find Ironhold's entity ID (3rd entity added, but IDs are from shared generator)
    let ironhold_id = world
        .entities
        .iter()
        .find(|(_, e)| e.name == "Ironhold")
        .unwrap()
        .0
        .to_owned();

    // Rename Ironhold via an event
    let rename_ev = world.add_event(
        history_gen::EventKind::SettlementFounded,
        SimTimestamp::from_year(200),
        "Ironhold renamed to Ironhaven".to_string(),
    );
    world.rename_entity(ironhold_id, "Ironhaven".to_string(), rename_ev);

    migrate(&pool).await.unwrap();
    load_world(&pool, &world).await.unwrap();

    // Query: what was Ironhold's name at year 150? (before rename at year 200)
    // Should get the entity_created effect with the original name
    let target_ts = SimTimestamp::from_year(150).as_u32() as i32;
    let row = sqlx::query(
        "SELECT effect_data->>'name' AS name \
         FROM event_effects ee \
         JOIN events e ON e.id = ee.event_id \
         WHERE ee.entity_id = $1 AND ee.effect_type = 'entity_created' \
           AND e.timestamp <= $2 \
         ORDER BY e.timestamp DESC LIMIT 1",
    )
    .bind(ironhold_id as i64)
    .bind(target_ts)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(row.get::<String, _>("name"), "Ironhold");

    // Query: what was the name after the rename?
    let after_ts = SimTimestamp::from_year(250).as_u32() as i32;
    let row = sqlx::query(
        "SELECT effect_data->>'new' AS name \
         FROM event_effects ee \
         JOIN events e ON e.id = ee.event_id \
         WHERE ee.entity_id = $1 AND ee.effect_type = 'name_changed' \
           AND e.timestamp <= $2 \
         ORDER BY e.timestamp DESC LIMIT 1",
    )
    .bind(ironhold_id as i64)
    .bind(after_ts)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(row.get::<String, _>("name"), "Ironhaven");

    // Verify causal chain via recursive CTE
    // The test world has: death -> spouse_end (caused_by death)
    let death_ev_id = world
        .events
        .values()
        .find(|e| e.description == "Alice dies")
        .unwrap()
        .id;

    let chain = sqlx::query(
        "WITH RECURSIVE chain AS ( \
             SELECT id, kind, description, caused_by, 0 AS depth FROM events WHERE id = $1 \
             UNION ALL \
             SELECT e.id, e.kind, e.description, e.caused_by, c.depth + 1 \
             FROM events e JOIN chain c ON e.caused_by = c.id \
         ) \
         SELECT id, kind, description, depth FROM chain ORDER BY depth",
    )
    .bind(death_ev_id as i64)
    .fetch_all(&pool)
    .await
    .unwrap();

    assert_eq!(chain.len(), 2, "death event should have one consequence");
    assert_eq!(chain[0].get::<String, _>("description"), "Alice dies");
    assert_eq!(chain[0].get::<i32, _>("depth"), 0);
    assert_eq!(
        chain[1].get::<String, _>("description"),
        "Spouse bond dissolved by death"
    );
    assert_eq!(chain[1].get::<i32, _>("depth"), 1);

    // Verify unpack_timestamp function works
    let unpacked = sqlx::query(
        "SELECT (unpack_timestamp($1)).year AS year, \
                (unpack_timestamp($1)).day AS day, \
                (unpack_timestamp($1)).hour AS hour",
    )
    .bind(SimTimestamp::new(125, 45, 8).as_u32() as i32)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(unpacked.get::<i32, _>("year"), 125);
    assert_eq!(unpacked.get::<i32, _>("day"), 45);
    assert_eq!(unpacked.get::<i32, _>("hour"), 8);
}
