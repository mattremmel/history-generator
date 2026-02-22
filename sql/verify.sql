-- Post-load verification queries.
-- Run after load.sql to confirm data integrity.

\echo '=== Row counts ==='
SELECT 'entities' AS table_name, COUNT(*) AS row_count FROM entities
UNION ALL
SELECT 'relationships', COUNT(*) FROM relationships
UNION ALL
SELECT 'events', COUNT(*) FROM events
UNION ALL
SELECT 'event_participants', COUNT(*) FROM event_participants
UNION ALL
SELECT 'event_effects', COUNT(*) FROM event_effects
ORDER BY table_name;

\echo ''
\echo '=== Entity kinds ==='
SELECT kind, COUNT(*) FROM entities GROUP BY kind ORDER BY kind;

\echo ''
\echo '=== Event kinds ==='
SELECT kind, COUNT(*) FROM events GROUP BY kind ORDER BY kind;

\echo ''
\echo '=== Sample: entities with their relationships ==='
SELECT
    e.name AS entity,
    e.kind AS entity_kind,
    r.kind AS rel_kind,
    t.name AS target
FROM entities e
JOIN relationships r ON r.source_entity_id = e.id
JOIN entities t ON t.id = r.target_entity_id
ORDER BY e.name
LIMIT 20;

\echo ''
\echo '=== Sample: events with participants (unpacked timestamps) ==='
SELECT
    (unpack_timestamp(ev.timestamp)).year AS year,
    (unpack_timestamp(ev.timestamp)).day AS day,
    ev.kind AS event_kind,
    ev.description,
    en.name AS participant,
    ep.role
FROM events ev
JOIN event_participants ep ON ep.event_id = ev.id
JOIN entities en ON en.id = ep.entity_id
ORDER BY ev.timestamp, ep.role
LIMIT 20;

\echo ''
\echo '=== Sample: event effects ==='
SELECT
    ee.effect_type,
    en.name AS entity,
    ee.effect_data
FROM event_effects ee
JOIN entities en ON en.id = ee.entity_id
LIMIT 20;
