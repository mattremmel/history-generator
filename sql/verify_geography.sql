-- verify_geography.sql
-- Verification queries for Phase 2: Geography & Settlements

-- Count regions and settlements
SELECT 'Region count' AS label, COUNT(*) AS value
FROM entities WHERE kind = 'region'
UNION ALL
SELECT 'Settlement count', COUNT(*)
FROM entities WHERE kind = 'settlement';

-- Settlements near a coordinate (within distance 200 of point 500,500)
SELECT
    e.id,
    e.name,
    (e.properties->>'x')::float AS x,
    (e.properties->>'y')::float AS y,
    (e.properties->>'population')::int AS population,
    sqrt(
        power((e.properties->>'x')::float - 500, 2)
        + power((e.properties->>'y')::float - 500, 2)
    ) AS distance
FROM entities e
WHERE e.kind = 'settlement'
  AND sqrt(
        power((e.properties->>'x')::float - 500, 2)
        + power((e.properties->>'y')::float - 500, 2)
      ) < 200
ORDER BY distance;

-- Regions adjacent to a given region (first region found)
WITH target_region AS (
    SELECT id, name FROM entities WHERE kind = 'region' LIMIT 1
)
SELECT
    tr.name AS source_region,
    e.name AS adjacent_region,
    e.properties->>'terrain' AS terrain
FROM target_region tr
JOIN relationships r ON r.source_entity_id = tr.id AND r.kind = 'adjacent_to'
JOIN entities e ON e.id = r.target_entity_id;

-- Settlements in a region via located_in join
SELECT
    region.name AS region_name,
    region.properties->>'terrain' AS terrain,
    settlement.name AS settlement_name,
    (settlement.properties->>'population')::int AS population
FROM relationships r
JOIN entities region ON region.id = r.target_entity_id AND region.kind = 'region'
JOIN entities settlement ON settlement.id = r.source_entity_id AND settlement.kind = 'settlement'
WHERE r.kind = 'located_in'
ORDER BY region.name, settlement.name;
