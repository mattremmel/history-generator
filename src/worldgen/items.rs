use rand::{Rng, RngCore};

use crate::model::{
    EntityData, EntityKind, ItemType, RelationshipKind, ResourceType, SimTimestamp, World,
};

// ---------------------------------------------------------------------------
// Material tables
// ---------------------------------------------------------------------------

const METAL_MATERIALS: &[&str] = &["iron", "bronze", "copper", "steel"];
const STONE_MATERIALS: &[&str] = &["granite", "obsidian", "marble"];
const PRECIOUS_MATERIALS: &[&str] = &["gold", "silver", "jade", "amber"];
const ORGANIC_MATERIALS: &[&str] = &["bone", "wood", "ivory", "horn"];

/// Generate items for each settlement during world generation.
///
/// Each settlement gets 1–3 items based on population. Faction leaders
/// receive a Crown or Seal as a symbol of authority.
pub fn generate_items(
    world: &mut World,
    _config: &crate::worldgen::config::WorldGenConfig,
    rng: &mut dyn RngCore,
    genesis_event: u64,
) {
    debug_assert!(
        world
            .entities
            .values()
            .any(|e| e.kind == EntityKind::Settlement),
        "items step requires settlements to exist"
    );

    // Collect settlement info
    struct SettlementInfo {
        id: u64,
        population: u32,
        resources: Vec<ResourceType>,
        origin_year: u32,
        faction_id: Option<u64>,
    }

    let settlements: Vec<SettlementInfo> = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
        .filter_map(|e| {
            let sd = e.data.as_settlement()?;
            let faction_id = e.active_rel(RelationshipKind::MemberOf);
            Some(SettlementInfo {
                id: e.id,
                population: sd.population,
                resources: sd.resources.clone(),
                origin_year: e.origin.map(|t| t.year()).unwrap_or(0),
                faction_id,
            })
        })
        .collect();

    // Track factions that already received leader items
    let mut factions_with_leader_items: std::collections::HashSet<u64> =
        std::collections::HashSet::new();

    for s in &settlements {
        // Number of items: 1 for small, 2 for medium, 3 for large
        let item_count = if s.population >= 500 {
            rng.random_range(2..=3)
        } else if s.population >= 200 {
            rng.random_range(1..=2)
        } else {
            1
        };

        for _ in 0..item_count {
            let item_type = pick_item_type(rng, &s.resources);
            let material = pick_material(rng, &s.resources);
            let name = format!("{} {}", capitalize(&material), item_type);

            // Older items have more starting resonance
            let age = s.origin_year; // years since "creation" at year 0
            let age_resonance = (age as f64 * 0.001).min(0.15);
            let resonance = age_resonance + rng.random_range(0.0..0.05);

            let mut data = EntityData::default_for_kind(EntityKind::Item);
            let EntityData::Item(ref mut id) = data else {
                unreachable!()
            };
            id.item_type = item_type;
            id.material = material;
            id.resonance = resonance;
            id.condition = rng.random_range(0.5..1.0);
            id.created = SimTimestamp::from_year(s.origin_year);

            let item_id = world.add_entity(
                EntityKind::Item,
                name,
                Some(SimTimestamp::from_year(s.origin_year)),
                data,
                genesis_event,
            );
            world.add_relationship(
                item_id,
                s.id,
                RelationshipKind::HeldBy,
                SimTimestamp::from_year(s.origin_year),
                genesis_event,
            );
        }

        // Faction leader gets a Crown or Seal (one per faction)
        if let Some(faction_id) = s.faction_id
            && factions_with_leader_items.insert(faction_id)
        {
            // Find the faction leader
            if let Some(leader) = world.faction_leader_id(faction_id) {
                let (item_type, material) = if rng.random_bool(0.6) {
                    (ItemType::Crown, "gold")
                } else {
                    (ItemType::Seal, "silver")
                };
                let name = format!("{} {} of {}", capitalize(material), item_type, {
                    world
                        .entities
                        .get(&faction_id)
                        .map(|e| e.name.as_str())
                        .unwrap_or("Unknown")
                });

                let mut data = EntityData::default_for_kind(EntityKind::Item);
                let EntityData::Item(ref mut id) = data else {
                    unreachable!()
                };
                id.item_type = item_type;
                id.material = material.to_string();
                id.resonance = 0.1 + rng.random_range(0.0..0.1);
                id.condition = rng.random_range(0.7..1.0);
                id.created = SimTimestamp::from_year(s.origin_year);

                let item_id = world.add_entity(
                    EntityKind::Item,
                    name,
                    Some(SimTimestamp::from_year(s.origin_year)),
                    data,
                    genesis_event,
                );
                world.add_relationship(
                    item_id,
                    leader,
                    RelationshipKind::HeldBy,
                    SimTimestamp::from_year(s.origin_year),
                    genesis_event,
                );
            }
        }
    }
}

fn pick_item_type(rng: &mut dyn RngCore, resources: &[ResourceType]) -> ItemType {
    let has_metal = resources.iter().any(|r| {
        matches!(
            r,
            ResourceType::Iron | ResourceType::Copper | ResourceType::Ore
        )
    });
    let has_precious = resources.iter().any(|r| {
        matches!(
            r,
            ResourceType::Gold | ResourceType::Gems | ResourceType::Pearls
        )
    });
    let has_stone = resources
        .iter()
        .any(|r| matches!(r, ResourceType::Stone | ResourceType::Obsidian));
    let has_clay = resources.iter().any(|r| matches!(r, ResourceType::Clay));

    let mut candidates: Vec<(ItemType, u32)> = Vec::new();
    if has_metal {
        candidates.push((ItemType::Weapon, 3));
        candidates.push((ItemType::Tool, 2));
    }
    if has_precious {
        candidates.push((ItemType::Jewelry, 3));
        candidates.push((ItemType::Amulet, 2));
    }
    if has_stone {
        candidates.push((ItemType::Tablet, 2));
        candidates.push((ItemType::Idol, 2));
    }
    if has_clay {
        candidates.push((ItemType::Pottery, 3));
    }
    candidates.push((ItemType::Tool, 1));
    candidates.push((ItemType::Chest, 1));

    let total: u32 = candidates.iter().map(|(_, w)| w).sum();
    let mut roll = rng.random_range(0..total);
    for (item_type, weight) in &candidates {
        if roll < *weight {
            return *item_type;
        }
        roll -= weight;
    }
    ItemType::Tool
}

fn pick_material(rng: &mut dyn RngCore, resources: &[ResourceType]) -> String {
    let has_metal = resources.iter().any(|r| {
        matches!(
            r,
            ResourceType::Iron | ResourceType::Copper | ResourceType::Ore
        )
    });
    let has_precious = resources.iter().any(|r| {
        matches!(
            r,
            ResourceType::Gold | ResourceType::Gems | ResourceType::Pearls
        )
    });
    let has_stone = resources
        .iter()
        .any(|r| matches!(r, ResourceType::Stone | ResourceType::Obsidian));

    let pool: &[&str] = if has_precious && rng.random_bool(0.3) {
        PRECIOUS_MATERIALS
    } else if has_metal && rng.random_bool(0.5) {
        METAL_MATERIALS
    } else if has_stone && rng.random_bool(0.4) {
        STONE_MATERIALS
    } else {
        ORGANIC_MATERIALS
    };

    pool[rng.random_range(0..pool.len())].to_string()
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::config::WorldGenConfig;
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    fn make_world_with_factions() -> (World, u64) {
        let config = WorldGenConfig::default();
        crate::worldgen::make_test_world(
            &config,
            &[
                crate::worldgen::geography::generate_regions,
                crate::worldgen::settlements::generate_settlements,
                crate::worldgen::factions::generate_factions,
            ],
        )
    }

    #[test]
    fn items_created_for_settlements() {
        let (mut world, ev) = make_world_with_factions();
        let config = WorldGenConfig::default();
        let mut rng = SmallRng::seed_from_u64(42);
        generate_items(&mut world, &config, &mut rng, ev);

        let item_count = world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Item)
            .count();
        assert!(item_count > 0, "should create item entities");
    }

    #[test]
    fn items_have_held_by() {
        let (mut world, ev) = make_world_with_factions();
        let config = WorldGenConfig::default();
        let mut rng = SmallRng::seed_from_u64(42);
        generate_items(&mut world, &config, &mut rng, ev);

        for e in world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Item)
        {
            let has_held_by = e
                .relationships
                .iter()
                .any(|r| r.kind == RelationshipKind::HeldBy && r.end.is_none());
            assert!(
                has_held_by,
                "item '{}' should have HeldBy relationship",
                e.name
            );
        }
    }

    #[test]
    fn leader_items_created_when_leaders_exist() {
        // Worldgen factions step doesn't create leaders, so we manually
        // create a faction + leader to test the leader item path.
        use crate::model::{EntityData, EventKind, PersonData, Role, Sex, SimTimestamp};

        let (mut world, ev) = make_world_with_factions();

        // Find any faction and add a leader
        let faction_id = world
            .entities
            .values()
            .find(|e| e.kind == EntityKind::Faction && e.is_alive())
            .map(|e| e.id)
            .expect("should have at least one faction");

        let leader_data = EntityData::Person(PersonData {
            born: SimTimestamp::default(),
            sex: Sex::Male,
            role: Role::Warrior,
            traits: vec![],
            last_action: SimTimestamp::default(),
            culture_id: None,
            prestige: 0.0,
            grievances: std::collections::BTreeMap::new(),
        });
        let leader_id = world.add_entity(
            EntityKind::Person,
            "TestLeader".to_string(),
            Some(SimTimestamp::from_year(0)),
            leader_data,
            ev,
        );
        world.add_relationship(
            leader_id,
            faction_id,
            RelationshipKind::LeaderOf,
            SimTimestamp::from_year(0),
            ev,
        );

        let config = WorldGenConfig::default();
        let mut rng = SmallRng::seed_from_u64(42);
        generate_items(&mut world, &config, &mut rng, ev);

        // Check that at least one Crown or Seal exists held by the leader
        let authority_items = world
            .entities
            .values()
            .filter(|e| {
                e.kind == EntityKind::Item
                    && e.data
                        .as_item()
                        .is_some_and(|id| matches!(id.item_type, ItemType::Crown | ItemType::Seal))
                    && e.has_active_rel(RelationshipKind::HeldBy, leader_id)
            })
            .count();
        assert!(
            authority_items > 0,
            "should create Crown/Seal for faction leaders"
        );
    }
}
