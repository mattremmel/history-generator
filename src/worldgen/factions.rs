use rand::Rng;
use rand::RngCore;

use crate::model::{EntityKind, EventKind, RelationshipKind, SimTimestamp, World};
use crate::sim::faction_names::generate_faction_name;
use crate::worldgen::config::WorldGenConfig;

/// Pipeline-compatible step that creates initial factions from settlement clusters.
pub fn generate_factions_step(world: &mut World, _config: &WorldGenConfig, rng: &mut dyn RngCore) {
    generate_factions(world, rng);
}

const GOVERNMENT_TYPES: &[&str] = &["hereditary", "elective", "chieftain"];

/// Group settlements by region and create one faction per inhabited region.
pub fn generate_factions(world: &mut World, rng: &mut dyn RngCore) {
    // Collect settlements grouped by their region (via LocatedIn relationship)
    struct SettlementInfo {
        id: u64,
        region_id: u64,
    }

    let settlements: Vec<SettlementInfo> = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
        .filter_map(|e| {
            let region_id = e
                .relationships
                .iter()
                .find(|r| r.kind == RelationshipKind::LocatedIn && r.end.is_none())
                .map(|r| r.target_entity_id)?;
            Some(SettlementInfo {
                id: e.id,
                region_id,
            })
        })
        .collect();

    // Group by region — use BTreeMap for deterministic iteration
    let mut by_region: std::collections::BTreeMap<u64, Vec<u64>> =
        std::collections::BTreeMap::new();
    for s in &settlements {
        by_region.entry(s.region_id).or_default().push(s.id);
    }

    // Create one faction per inhabited region
    for settlement_ids in by_region.values() {
        let name = generate_faction_name(rng);
        let gov_type = GOVERNMENT_TYPES[rng.random_range(0..GOVERNMENT_TYPES.len())];
        let stability: f64 = rng.random_range(0.6..1.0);

        let ev = world.add_event(
            EventKind::FactionFormed,
            SimTimestamp::from_year(0),
            format!("{name} established"),
        );

        let faction_id = world.add_entity(
            EntityKind::Faction,
            name,
            Some(SimTimestamp::from_year(0)),
            ev,
        );

        world.set_property(
            faction_id,
            "government_type".to_string(),
            serde_json::json!(gov_type),
            ev,
        );
        world.set_property(
            faction_id,
            "stability".to_string(),
            serde_json::json!(stability),
            ev,
        );
        world.set_property(
            faction_id,
            "happiness".to_string(),
            serde_json::json!(rng.random_range(0.55..0.85)),
            ev,
        );
        world.set_property(
            faction_id,
            "legitimacy".to_string(),
            serde_json::json!(1.0),
            ev,
        );

        // Each settlement in this region joins the faction
        for &settlement_id in settlement_ids {
            world.add_relationship(
                settlement_id,
                faction_id,
                RelationshipKind::MemberOf,
                SimTimestamp::from_year(0),
                ev,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    use crate::model::World;
    use crate::worldgen::config::{MapConfig, WorldGenConfig};
    use crate::worldgen::geography::generate_regions;
    use crate::worldgen::settlements::generate_settlements;

    fn make_world_with_settlements() -> World {
        let config = WorldGenConfig {
            seed: 12345,
            map: MapConfig {
                num_regions: 15,
                width: 500.0,
                height: 500.0,
                num_biome_centers: 4,
                adjacency_k: 3,
            },
            ..WorldGenConfig::default()
        };
        let mut world = World::new();
        let mut rng = SmallRng::seed_from_u64(config.seed);
        generate_regions(&mut world, &config, &mut rng);
        generate_settlements(&mut world, config.map.width, config.map.height, &mut rng);
        world
    }

    #[test]
    fn factions_created_for_regions_with_settlements() {
        let mut world = make_world_with_settlements();
        let mut rng = SmallRng::seed_from_u64(99);
        generate_factions(&mut world, &mut rng);

        let faction_count = world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Faction)
            .count();
        assert!(faction_count > 0, "expected at least one faction");

        // Should have FactionFormed events
        let formed_count = world
            .events
            .values()
            .filter(|e| e.kind == EventKind::FactionFormed)
            .count();
        assert_eq!(faction_count, formed_count);
    }

    #[test]
    fn every_settlement_belongs_to_exactly_one_faction() {
        let mut world = make_world_with_settlements();
        let mut rng = SmallRng::seed_from_u64(99);
        generate_factions(&mut world, &mut rng);

        for entity in world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Settlement)
        {
            let faction_memberships: Vec<_> = entity
                .relationships
                .iter()
                .filter(|r| {
                    r.kind == RelationshipKind::MemberOf
                        && r.end.is_none()
                        && world
                            .entities
                            .get(&r.target_entity_id)
                            .is_some_and(|t| t.kind == EntityKind::Faction)
                })
                .collect();
            assert_eq!(
                faction_memberships.len(),
                1,
                "settlement {} should belong to exactly 1 faction, got {}",
                entity.name,
                faction_memberships.len()
            );
        }
    }

    #[test]
    fn factions_have_required_properties() {
        let mut world = make_world_with_settlements();
        let mut rng = SmallRng::seed_from_u64(99);
        generate_factions(&mut world, &mut rng);

        for faction in world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Faction)
        {
            assert!(
                faction.has_property("government_type"),
                "faction {} missing government_type",
                faction.name
            );
            assert!(
                faction.has_property("stability"),
                "faction {} missing stability",
                faction.name
            );

            let gov = faction.properties["government_type"].as_str().unwrap();
            assert!(
                GOVERNMENT_TYPES.contains(&gov),
                "invalid government_type: {gov}"
            );

            let stability = faction.properties["stability"].as_f64().unwrap();
            assert!(
                (0.0..=1.0).contains(&stability),
                "stability out of range: {stability}"
            );
        }
    }

    #[test]
    fn deterministic_factions() {
        let mut world1 = make_world_with_settlements();
        let mut rng1 = SmallRng::seed_from_u64(99);
        generate_factions(&mut world1, &mut rng1);

        let mut world2 = make_world_with_settlements();
        let mut rng2 = SmallRng::seed_from_u64(99);
        generate_factions(&mut world2, &mut rng2);

        let names1: Vec<&str> = world1
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Faction)
            .map(|e| e.name.as_str())
            .collect();
        let names2: Vec<&str> = world2
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Faction)
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(names1, names2);
    }
}
