-- Fantasy History Generator — schema
-- All enum-like columns use TEXT to avoid rigid ALTER TYPE migrations.

CREATE TABLE IF NOT EXISTS entities (
    id          BIGINT PRIMARY KEY,
    kind        TEXT NOT NULL,
    name        TEXT NOT NULL,
    origin_year INTEGER,
    end_year    INTEGER
);

CREATE TABLE IF NOT EXISTS relationships (
    source_entity_id  BIGINT NOT NULL REFERENCES entities(id),
    target_entity_id  BIGINT NOT NULL REFERENCES entities(id),
    kind              TEXT NOT NULL,
    start_year        INTEGER NOT NULL,
    end_year          INTEGER,
    PRIMARY KEY (source_entity_id, target_entity_id, kind, start_year)
);

CREATE TABLE IF NOT EXISTS events (
    id          BIGINT PRIMARY KEY,
    kind        TEXT NOT NULL,
    year        INTEGER NOT NULL,
    description TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS event_participants (
    event_id    BIGINT NOT NULL REFERENCES events(id),
    entity_id   BIGINT NOT NULL REFERENCES entities(id),
    role        TEXT NOT NULL
);

-- Indexes for common query patterns
CREATE INDEX IF NOT EXISTS idx_entities_kind ON entities(kind);
CREATE INDEX IF NOT EXISTS idx_relationships_source ON relationships(source_entity_id);
CREATE INDEX IF NOT EXISTS idx_relationships_target ON relationships(target_entity_id);
CREATE INDEX IF NOT EXISTS idx_events_year ON events(year);
CREATE INDEX IF NOT EXISTS idx_events_kind ON events(kind);
CREATE INDEX IF NOT EXISTS idx_event_participants_event ON event_participants(event_id);
CREATE INDEX IF NOT EXISTS idx_event_participants_entity ON event_participants(entity_id);
