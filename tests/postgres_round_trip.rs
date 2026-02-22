mod common;

use history_gen::db::{load_world, migrate};
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use testcontainers::runners::AsyncRunner;
use testcontainers::ContainerAsync;
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
    assert_eq!(entity_count, 4);

    let rel_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM relationships")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rel_count, 3);

    let event_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(event_count, 1);

    let part_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM event_participants")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(part_count, 2);
}

#[tokio::test]
#[ignore]
async fn loaded_data_matches_source_values() {
    let (pool, _container) = setup().await;
    let world = common::build_test_world();

    migrate(&pool).await.unwrap();
    load_world(&pool, &world).await.unwrap();

    // --- Entities ---
    let rows = sqlx::query("SELECT id, kind, name, origin_year, end_year FROM entities ORDER BY id")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(rows.len(), 4);

    // Alice
    assert_eq!(rows[0].get::<String, _>("kind"), "person");
    assert_eq!(rows[0].get::<String, _>("name"), "Alice");
    assert_eq!(rows[0].get::<Option<i32>, _>("origin_year"), Some(100));
    assert_eq!(rows[0].get::<Option<i32>, _>("end_year"), None);

    // Bob
    assert_eq!(rows[1].get::<String, _>("kind"), "person");
    assert_eq!(rows[1].get::<String, _>("name"), "Bob");
    assert_eq!(rows[1].get::<Option<i32>, _>("origin_year"), Some(105));

    // Ironhold
    assert_eq!(rows[2].get::<String, _>("kind"), "settlement");
    assert_eq!(rows[2].get::<String, _>("name"), "Ironhold");
    assert_eq!(rows[2].get::<Option<i32>, _>("origin_year"), None);

    // Merchant Guild
    assert_eq!(rows[3].get::<String, _>("kind"), "faction");
    assert_eq!(rows[3].get::<String, _>("name"), "Merchant Guild");

    // --- Relationships (ordered by start_year) ---
    let rels = sqlx::query(
        "SELECT source_entity_id, target_entity_id, kind, start_year, end_year \
         FROM relationships ORDER BY start_year",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(rels.len(), 3);

    // member_of (start_year 120)
    assert_eq!(rels[0].get::<String, _>("kind"), "member_of");
    assert_eq!(rels[0].get::<Option<i32>, _>("end_year"), None);

    // spouse (start_year 125)
    assert_eq!(rels[1].get::<String, _>("kind"), "spouse");

    // ruler_of (start_year 130)
    assert_eq!(rels[2].get::<String, _>("kind"), "ruler_of");

    // --- Event ---
    let events = sqlx::query("SELECT id, kind, year, description FROM events")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].get::<String, _>("kind"), "marriage");
    assert_eq!(events[0].get::<i32, _>("year"), 125);
    assert_eq!(
        events[0].get::<String, _>("description"),
        "Alice and Bob wed in Ironhold"
    );

    // --- Event participants (ordered by role for determinism) ---
    let parts = sqlx::query("SELECT event_id, entity_id, role FROM event_participants ORDER BY role")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0].get::<String, _>("role"), "object");
    assert_eq!(parts[1].get::<String, _>("role"), "subject");
}
