use std::collections::BTreeMap;

use rand::Rng;
use rand::RngCore;
use rand::seq::SliceRandom;

use crate::model::cultural_value::{CulturalValue, NamingStyle, generate_cultural_values};
use crate::model::entity_data::CultureData;
use crate::model::{EntityData, EntityKind, RelationshipKind, World};
use crate::sim::culture_names::generate_culture_entity_name;
use crate::worldgen::config::WorldGenConfig;

/// Pipeline-compatible step that creates initial cultures, one per faction.
pub fn generate_cultures(world: &mut World, _config: &WorldGenConfig, rng: &mut dyn RngCore) {
    debug_assert!(
        world
            .entities
            .values()
            .any(|e| e.kind == EntityKind::Faction),
        "cultures step requires factions to exist"
    );
    // Collect living factions
    let faction_ids: Vec<u64> = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        .map(|e| e.id)
        .collect();

    if faction_ids.is_empty() {
        return;
    }

    // Shuffle the 6 core naming styles, assign cyclically
    let mut styles: Vec<NamingStyle> = NamingStyle::ALL.to_vec();
    styles.shuffle(rng);

    for (idx, &faction_id) in faction_ids.iter().enumerate() {
        let style = styles[idx % styles.len()].clone();

        // Generate 2-3 cultural values (no opposing pairs)
        let value_count = rng.random_range(2..=3);
        let values = generate_cultural_values(rng, value_count);

        // Compute resistance
        let mut resistance: f64 = 0.5;
        if values.contains(&CulturalValue::Martial) {
            resistance += 0.15;
        }
        if values.contains(&CulturalValue::Isolationist) {
            resistance += 0.10;
        }
        if values.contains(&CulturalValue::Mercantile) {
            resistance -= 0.10;
        }
        resistance = resistance.clamp(0.0, 1.0);

        // Create Culture entity
        let name = generate_culture_entity_name(rng);
        let ev = world.add_event(
            crate::model::EventKind::Custom("culture_founded".to_string()),
            crate::model::SimTimestamp::from_year(0),
            format!("{name} culture established"),
        );

        let culture_id = world.add_entity(
            EntityKind::Culture,
            name,
            Some(crate::model::SimTimestamp::from_year(0)),
            EntityData::Culture(CultureData {
                values,
                naming_style: style,
                resistance,
            }),
            ev,
        );

        // Set faction's primary culture
        if let Some(faction) = world.entities.get_mut(&faction_id)
            && let Some(fd) = faction.data.as_faction_mut()
        {
            fd.primary_culture = Some(culture_id);
        }

        // For each settlement in this faction: set dominant_culture and culture_makeup
        let settlement_ids: Vec<u64> = world
            .entities
            .values()
            .filter(|e| {
                e.kind == EntityKind::Settlement
                    && e.end.is_none()
                    && e.has_active_rel(RelationshipKind::MemberOf, faction_id)
            })
            .map(|e| e.id)
            .collect();

        for &sid in &settlement_ids {
            if let Some(settlement) = world.entities.get_mut(&sid)
                && let Some(sd) = settlement.data.as_settlement_mut()
            {
                sd.dominant_culture = Some(culture_id);
                sd.culture_makeup = BTreeMap::from([(culture_id, 1.0)]);
            }
        }

        // For each living person who is a member of this faction: set culture_id
        let person_ids: Vec<u64> = world
            .entities
            .values()
            .filter(|e| {
                e.kind == EntityKind::Person
                    && e.end.is_none()
                    && e.has_active_rel(RelationshipKind::MemberOf, faction_id)
            })
            .map(|e| e.id)
            .collect();

        for &pid in &person_ids {
            if let Some(person) = world.entities.get_mut(&pid)
                && let Some(pd) = person.data.as_person_mut()
            {
                pd.culture_id = Some(culture_id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    use crate::worldgen::config::{MapConfig, WorldGenConfig};
    use crate::worldgen::factions::generate_factions;
    use crate::worldgen::geography::generate_regions;
    use crate::worldgen::settlements::generate_settlements;

    fn make_world_with_factions() -> World {
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
        generate_settlements(&mut world, &config, &mut rng);
        generate_factions(&mut world, &config, &mut rng);
        world
    }

    #[test]
    fn cultures_created_per_faction() {
        let mut world = make_world_with_factions();
        let mut rng = SmallRng::seed_from_u64(42);
        generate_cultures(&mut world, &WorldGenConfig::default(), &mut rng);

        let faction_count = world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
            .count();

        let culture_count = world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Culture && e.end.is_none())
            .count();

        assert_eq!(
            culture_count, faction_count,
            "should have one culture per faction"
        );
    }

    #[test]
    fn factions_have_primary_culture() {
        let mut world = make_world_with_factions();
        let mut rng = SmallRng::seed_from_u64(42);
        generate_cultures(&mut world, &WorldGenConfig::default(), &mut rng);

        for faction in world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        {
            let fd = faction.data.as_faction().unwrap();
            assert!(
                fd.primary_culture.is_some(),
                "faction {} should have primary culture",
                faction.name
            );
        }
    }

    #[test]
    fn settlements_have_dominant_culture() {
        let mut world = make_world_with_factions();
        let mut rng = SmallRng::seed_from_u64(42);
        generate_cultures(&mut world, &WorldGenConfig::default(), &mut rng);

        for settlement in world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
        {
            let sd = settlement.data.as_settlement().unwrap();
            // Settlements that belong to a faction should have dominant culture
            let has_faction = settlement.active_rel(RelationshipKind::MemberOf).is_some();
            if has_faction {
                assert!(
                    sd.dominant_culture.is_some(),
                    "settlement {} should have dominant culture",
                    settlement.name
                );
                assert!(
                    !sd.culture_makeup.is_empty(),
                    "settlement {} should have culture makeup",
                    settlement.name
                );
            }
        }
    }

    #[test]
    fn culture_entities_have_valid_data() {
        let mut world = make_world_with_factions();
        let mut rng = SmallRng::seed_from_u64(42);
        generate_cultures(&mut world, &WorldGenConfig::default(), &mut rng);

        for culture in world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Culture)
        {
            let cd = culture.data.as_culture().unwrap();
            assert!(!cd.values.is_empty(), "culture should have values");
            assert!(
                cd.values.len() >= 2 && cd.values.len() <= 3,
                "culture should have 2-3 values, got {}",
                cd.values.len()
            );
            assert!(
                (0.0..=1.0).contains(&cd.resistance),
                "resistance out of range: {}",
                cd.resistance
            );
        }
    }
}
