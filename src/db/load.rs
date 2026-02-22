use serde::Serialize;
use sqlx::PgPool;

use crate::model::World;

/// Load an entire `World` into Postgres using COPY FROM STDIN (text format).
///
/// Order respects FK constraints: entities → events → relationships → event_participants.
pub async fn load_world(pool: &PgPool, world: &World) -> Result<(), sqlx::Error> {
    // Entities
    {
        let mut buf = String::new();
        for e in world.entities.values() {
            buf.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\n",
                e.id,
                escape(&enum_str(&e.kind)),
                escape(&e.name),
                opt_i32(e.birth_year),
                opt_i32(e.death_year),
            ));
        }
        copy_in(pool, include_str!("../../sql/copy_entities.sql"), &buf).await?;
    }

    // Events (before participants due to FK)
    {
        let mut buf = String::new();
        for ev in world.events.values() {
            buf.push_str(&format!(
                "{}\t{}\t{}\t{}\n",
                ev.id,
                escape(&enum_str(&ev.kind)),
                ev.year,
                escape(&ev.description),
            ));
        }
        copy_in(pool, include_str!("../../sql/copy_events.sql"), &buf).await?;
    }

    // Relationships
    {
        let mut buf = String::new();
        for r in world.collect_relationships() {
            buf.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\n",
                r.source_entity_id,
                r.target_entity_id,
                escape(&enum_str(&r.kind)),
                r.start_year,
                opt_i32(r.end_year),
            ));
        }
        copy_in(pool, include_str!("../../sql/copy_relationships.sql"), &buf).await?;
    }

    // Event participants
    {
        let mut buf = String::new();
        for p in &world.event_participants {
            buf.push_str(&format!(
                "{}\t{}\t{}\n",
                p.event_id,
                p.entity_id,
                escape(&enum_str(&p.role)),
            ));
        }
        copy_in(pool, include_str!("../../sql/copy_event_participants.sql"), &buf).await?;
    }

    Ok(())
}

/// Execute a COPY FROM STDIN with the given text-format payload.
async fn copy_in(pool: &PgPool, statement: &str, data: &str) -> Result<(), sqlx::Error> {
    let mut conn = pool.acquire().await?;
    let mut copy = conn.copy_in_raw(statement).await?;
    copy.send(data.as_bytes()).await?;
    copy.finish().await?;
    Ok(())
}

/// Escape a string for Postgres COPY text format.
/// Backslash must be escaped first, then the special whitespace characters.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out
}

/// Render an optional i32 as a COPY text value (`\N` for NULL).
fn opt_i32(v: Option<i32>) -> String {
    match v {
        Some(n) => n.to_string(),
        None => "\\N".to_string(),
    }
}

/// Serialize a serde enum variant to its snake_case string (strips JSON quotes).
fn enum_str<T: Serialize>(val: &T) -> String {
    let json = serde_json::to_string(val).expect("enum serialization");
    // serde_json wraps string enums in quotes: "\"value\""
    json[1..json.len() - 1].to_string()
}
