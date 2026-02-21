use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::Path;

use serde::Serialize;

use crate::model::World;

/// Write an iterator of serializable items to a JSONL file (one JSON object per line).
fn write_jsonl<T: Serialize>(path: &Path, items: impl Iterator<Item = T>) -> io::Result<()> {
    let mut writer = BufWriter::new(File::create(path)?);
    for item in items {
        serde_json::to_writer(&mut writer, &item)?;
        writer.write_all(b"\n")?;
    }
    writer.flush()
}

/// Flush the world state to JSONL files in the given output directory.
///
/// Creates the output directory if it does not exist. Writes 4 files:
/// - `entities.jsonl` — one Entity per line (without inline relationships)
/// - `relationships.jsonl` — normalized relationships extracted from entities
/// - `events.jsonl` — one Event per line
/// - `event_participants.jsonl` — one EventParticipant per line
pub fn flush_to_jsonl(world: &World, output_dir: &Path) -> io::Result<()> {
    fs::create_dir_all(output_dir)?;

    write_jsonl(
        &output_dir.join("entities.jsonl"),
        world.entities.values(),
    )?;
    write_jsonl(
        &output_dir.join("relationships.jsonl"),
        world.collect_relationships(),
    )?;
    write_jsonl(&output_dir.join("events.jsonl"), world.events.values())?;
    write_jsonl(
        &output_dir.join("event_participants.jsonl"),
        world.event_participants.iter(),
    )?;

    Ok(())
}
