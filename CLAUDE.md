# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Fantasy History Generator — a Rust simulation that generates historical and social dynamics for fantasy worlds. Outputs JSONL, loaded into Postgres for querying. Currently in early development (placeholder code only).

## Commands

Uses [just](https://github.com/casey/just) as a command runner. Run `just --list` to see all recipes.

```bash
just build           # compile
just test            # run all tests
just test-one <name> # run a single test by name
just check           # clippy with warnings denied
just fmt             # format code
just fmt-check       # check formatting without modifying
```

Rust edition 2024, no external dependencies yet. Serde will be added for serialization.

## Architecture (Planned)

The system is a year-by-year tick-based simulation. Core structs (`Entity`, `Relationship`, `Event`, `EventParticipant`) use flat integer IDs. A `World` struct holds collections in `BTreeMap`/`HashMap`.

**Pipeline:** Rust simulation → JSONL flush (every ~50 simulated years) → Postgres loader → query layer.

**Tick loop design:** Main loop dispatches to per-system tick methods. Each system (demographics, politics, conflicts, knowledge, items, culture, etc.) gets its own method.

**Struct design:** Optimize for cache locality. Store relationships inline as `Vec<Relationship>` on entities during simulation, normalize to relational model only at flush time.

Development follows a 10-phase roadmap in `docs/project-idea.md`, building end-to-end pipeline first, then adding one system at a time with 1000-year test runs after each.

## Code Evolution Policy

No backwards compatibility constraints. This is a greenfield project with no external consumers. Freely refactor, rearchitect, or rewrite existing code when it produces a cleaner, more idiomatic result. Prefer clean breaks over shims, deprecations, or compatibility layers.

## Data Model Policy

All simulation state must be first-class: typed struct fields for entity data, typed enum variants for `EventKind`/`RelationshipKind`.

- **`EventKind::Custom(String)`** — reserved for 3rd-party plugins/mods. Never use in simulation or worldgen code. Add a new enum variant instead.
- **`RelationshipKind::Custom(String)`** — same rule. Add a new variant to the enum.
- **`Entity.extra` HashMap** — reserved for 3rd-party plugins/mods. Never use in simulation or worldgen code. Add a typed field to the appropriate `EntityData` struct (with `#[serde(default)]`).
- **`Event.data` (serde_json::Value)** — OK for structured event metadata (e.g. disaster type+phase). Not a substitute for entity state.
