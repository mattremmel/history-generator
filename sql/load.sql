-- Load JSONL data into Postgres via JSONB temp tables.
-- Usage: psql -d history_gen -v datadir="'/path/to/output'" -f sql/load.sql
--
-- Timestamps are stored as packed u32 integers. The JSONL fields serialize
-- SimTimestamp as {"year":N,"day":N,"hour":N}; we pack them on insert:
--   (year << 14) | (day << 5) | hour
--
-- TRUNCATE before load for idempotent reloads during development.

BEGIN;

-- Clear existing data (order matters for FK constraints)
TRUNCATE event_effects, event_participants, relationships, events, entities CASCADE;

-- Entities
CREATE TEMP TABLE _entities_raw (data JSONB) ON COMMIT DROP;
\copy _entities_raw(data) FROM :'datadir'/entities.jsonl
INSERT INTO entities (id, kind, name, origin_ts, end_ts)
SELECT
    (data->>'id')::BIGINT,
    data->>'kind',
    data->>'name',
    CASE WHEN data->'origin' IS NOT NULL AND data->'origin' != 'null'::jsonb
         THEN (((data->'origin'->>'year')::INTEGER) << 14)
            | (((data->'origin'->>'day')::INTEGER) << 5)
            | ((data->'origin'->>'hour')::INTEGER)
         ELSE NULL END,
    CASE WHEN data->'end' IS NOT NULL AND data->'end' != 'null'::jsonb
         THEN (((data->'end'->>'year')::INTEGER) << 14)
            | (((data->'end'->>'day')::INTEGER) << 5)
            | ((data->'end'->>'hour')::INTEGER)
         ELSE NULL END
FROM _entities_raw;

-- Events (load before participants due to FK)
CREATE TEMP TABLE _events_raw (data JSONB) ON COMMIT DROP;
\copy _events_raw(data) FROM :'datadir'/events.jsonl
INSERT INTO events (id, kind, timestamp, description)
SELECT
    (data->>'id')::BIGINT,
    data->>'kind',
    (((data->'timestamp'->>'year')::INTEGER) << 14)
        | (((data->'timestamp'->>'day')::INTEGER) << 5)
        | ((data->'timestamp'->>'hour')::INTEGER),
    data->>'description'
FROM _events_raw;

-- Relationships
CREATE TEMP TABLE _relationships_raw (data JSONB) ON COMMIT DROP;
\copy _relationships_raw(data) FROM :'datadir'/relationships.jsonl
INSERT INTO relationships (source_entity_id, target_entity_id, kind, start_ts, end_ts)
SELECT
    (data->>'source_entity_id')::BIGINT,
    (data->>'target_entity_id')::BIGINT,
    data->>'kind',
    (((data->'start'->>'year')::INTEGER) << 14)
        | (((data->'start'->>'day')::INTEGER) << 5)
        | ((data->'start'->>'hour')::INTEGER),
    CASE WHEN data->'end' IS NOT NULL AND data->'end' != 'null'::jsonb
         THEN (((data->'end'->>'year')::INTEGER) << 14)
            | (((data->'end'->>'day')::INTEGER) << 5)
            | ((data->'end'->>'hour')::INTEGER)
         ELSE NULL END
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

-- Event effects
CREATE TEMP TABLE _effects_raw (data JSONB) ON COMMIT DROP;
\copy _effects_raw(data) FROM :'datadir'/event_effects.jsonl
INSERT INTO event_effects (event_id, entity_id, effect_type, effect_data)
SELECT
    (data->>'event_id')::BIGINT,
    (data->>'entity_id')::BIGINT,
    data->'effect'->>'type',
    data->'effect'
FROM _effects_raw;

COMMIT;
