-- Load JSONL data into Postgres via JSONB temp tables.
-- Usage: psql -d history_gen -v datadir="'/path/to/output'" -f sql/load.sql
--
-- TRUNCATE before load for idempotent reloads during development.

BEGIN;

-- Clear existing data (order matters for FK constraints)
TRUNCATE event_participants, relationships, events, entities CASCADE;

-- Entities
CREATE TEMP TABLE _entities_raw (data JSONB) ON COMMIT DROP;
\copy _entities_raw(data) FROM :'datadir'/entities.jsonl
INSERT INTO entities (id, kind, name, birth_year, death_year)
SELECT
    (data->>'id')::BIGINT,
    data->>'kind',
    data->>'name',
    (data->>'birth_year')::INTEGER,
    (data->>'death_year')::INTEGER
FROM _entities_raw;

-- Events (load before participants due to FK)
CREATE TEMP TABLE _events_raw (data JSONB) ON COMMIT DROP;
\copy _events_raw(data) FROM :'datadir'/events.jsonl
INSERT INTO events (id, kind, year, description)
SELECT
    (data->>'id')::BIGINT,
    data->>'kind',
    (data->>'year')::INTEGER,
    data->>'description'
FROM _events_raw;

-- Relationships
CREATE TEMP TABLE _relationships_raw (data JSONB) ON COMMIT DROP;
\copy _relationships_raw(data) FROM :'datadir'/relationships.jsonl
INSERT INTO relationships (source_entity_id, target_entity_id, kind, start_year, end_year)
SELECT
    (data->>'source_entity_id')::BIGINT,
    (data->>'target_entity_id')::BIGINT,
    data->>'kind',
    (data->>'start_year')::INTEGER,
    (data->>'end_year')::INTEGER
FROM _relationships_raw;

-- Event participants
CREATE TEMP TABLE _participants_raw (data JSONB) ON COMMIT DROP;
\copy _participants_raw(data) FROM :'datadir'/event_participants.jsonl
INSERT INTO event_participants (event_id, entity_id, role)
SELECT
    (data->>'event_id')::BIGINT,
    (data->>'entity_id')::BIGINT,
    data->>'role'
FROM _participants_raw;

COMMIT;
