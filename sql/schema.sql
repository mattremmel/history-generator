-- Fantasy History Generator — schema
-- All enum-like columns use TEXT to avoid rigid ALTER TYPE migrations.
-- Timestamp columns store packed SimTimestamp u32 values.

CREATE TABLE IF NOT EXISTS entities (
    id          BIGINT PRIMARY KEY,
    kind        TEXT NOT NULL,
    name        TEXT NOT NULL,
    origin_ts   INTEGER,
    end_ts      INTEGER,
    properties  JSONB NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS relationships (
    source_entity_id  BIGINT NOT NULL REFERENCES entities(id),
    target_entity_id  BIGINT NOT NULL REFERENCES entities(id),
    kind              TEXT NOT NULL,
    start_ts          INTEGER NOT NULL,
    end_ts            INTEGER,
    PRIMARY KEY (source_entity_id, target_entity_id, kind, start_ts)
);

CREATE TABLE IF NOT EXISTS events (
    id          BIGINT PRIMARY KEY,
    kind        TEXT NOT NULL,
    timestamp   INTEGER NOT NULL,
    description TEXT NOT NULL,
    caused_by   BIGINT REFERENCES events(id),
    data        JSONB
);

CREATE TABLE IF NOT EXISTS event_participants (
    event_id    BIGINT NOT NULL REFERENCES events(id),
    entity_id   BIGINT NOT NULL REFERENCES entities(id),
    role        TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS event_effects (
    event_id     BIGINT NOT NULL REFERENCES events(id),
    entity_id    BIGINT NOT NULL REFERENCES entities(id),
    effect_type  TEXT NOT NULL,
    effect_data  JSONB NOT NULL
);

-- Indexes for common query patterns
CREATE INDEX IF NOT EXISTS idx_entities_kind ON entities(kind);
CREATE INDEX IF NOT EXISTS idx_relationships_source ON relationships(source_entity_id);
CREATE INDEX IF NOT EXISTS idx_relationships_target ON relationships(target_entity_id);
CREATE INDEX IF NOT EXISTS idx_events_timestamp ON events(timestamp);
CREATE INDEX IF NOT EXISTS idx_events_kind ON events(kind);
CREATE INDEX IF NOT EXISTS idx_events_caused_by ON events(caused_by);
CREATE INDEX IF NOT EXISTS idx_entities_properties ON entities USING GIN (properties);
CREATE INDEX IF NOT EXISTS idx_events_data ON events USING GIN (data) WHERE data IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_event_participants_event ON event_participants(event_id);
CREATE INDEX IF NOT EXISTS idx_event_participants_entity ON event_participants(entity_id);
CREATE INDEX IF NOT EXISTS idx_event_effects_event ON event_effects(event_id);
CREATE INDEX IF NOT EXISTS idx_event_effects_entity ON event_effects(entity_id);
CREATE INDEX IF NOT EXISTS idx_event_effects_type ON event_effects(effect_type);
CREATE INDEX IF NOT EXISTS idx_event_effects_entity_type ON event_effects(entity_id, effect_type);

-- Unpack a packed SimTimestamp integer into human-readable components.
-- Bit layout: [year:18][day:9][hour:5]
CREATE OR REPLACE FUNCTION unpack_timestamp(ts INTEGER)
RETURNS TABLE(year INTEGER, day INTEGER, hour INTEGER) AS $$
BEGIN
    RETURN QUERY SELECT
        (ts >> 14)::INTEGER AS year,
        ((ts >> 5) & 511)::INTEGER AS day,
        (ts & 31)::INTEGER AS hour;
END;
$$ LANGUAGE plpgsql IMMUTABLE;
