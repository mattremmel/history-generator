-- verify_geography.sql
-- Verification queries for Phase 2b: Geography & Settlements

-- Count by entity kind
SELECT kind, COUNT(*) AS count
FROM entities
WHERE kind IN ('region', 'settlement', 'river', 'geographic_feature', 'resource_deposit', 'building')
GROUP BY kind
ORDER BY kind;

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

-- Rivers and their traversed regions
SELECT
    river.name AS river_name,
    river.properties->>'region_path' AS region_path,
    (river.properties->>'length')::int AS length,
    region.name AS flows_through_region,
    region.properties->>'terrain' AS region_terrain
FROM entities river
JOIN relationships r ON r.source_entity_id = river.id AND r.kind = 'flows_through'
JOIN entities region ON region.id = r.target_entity_id
WHERE river.kind = 'river'
ORDER BY river.name, region.name;

-- Resource deposits by region with quantities
SELECT
    region.name AS region_name,
    deposit.name AS deposit_name,
    deposit.properties->>'resource_type' AS resource_type,
    (deposit.properties->>'quantity')::int AS quantity,
    (deposit.properties->>'quality')::float AS quality,
    (deposit.properties->>'discovered')::boolean AS discovered
FROM entities deposit
JOIN relationships r ON r.source_entity_id = deposit.id AND r.kind = 'located_in'
JOIN entities region ON region.id = r.target_entity_id AND region.kind = 'region'
WHERE deposit.kind = 'resource_deposit'
ORDER BY region.name, deposit.name;

-- Buildings exploiting deposits
SELECT
    building.name AS building_name,
    building.properties->>'building_type' AS building_type,
    building.properties->>'output_resource' AS output_resource,
    deposit.name AS deposit_name,
    deposit.properties->>'resource_type' AS deposit_resource,
    (deposit.properties->>'quantity')::int AS deposit_quantity,
    region.name AS region_name
FROM entities building
JOIN relationships exploit ON exploit.source_entity_id = building.id AND exploit.kind = 'exploits'
JOIN entities deposit ON deposit.id = exploit.target_entity_id
JOIN relationships loc ON loc.source_entity_id = building.id AND loc.kind = 'located_in'
JOIN entities region ON region.id = loc.target_entity_id
WHERE building.kind = 'building'
ORDER BY building.name;

-- Water regions and their coastal neighbors
SELECT
    water.name AS water_region,
    water.properties->>'terrain' AS water_terrain,
    land.name AS coastal_neighbor,
    land.properties->>'terrain' AS land_terrain,
    land.properties->'terrain_tags' AS tags
FROM entities water
JOIN relationships r ON r.source_entity_id = water.id AND r.kind = 'adjacent_to'
JOIN entities land ON land.id = r.target_entity_id
WHERE water.kind = 'region'
  AND water.properties->>'terrain' IN ('shallow_water', 'deep_water')
  AND land.properties->>'terrain' NOT IN ('shallow_water', 'deep_water')
ORDER BY water.name, land.name;
