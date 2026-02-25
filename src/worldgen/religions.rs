use std::collections::BTreeMap;

use rand::Rng;
use rand::RngCore;

use crate::model::cultural_value::CulturalValue;
use crate::model::entity_data::{DeityData, DeityDomain, ReligionData, ReligiousTenet};
use crate::model::{EntityData, EntityKind, RelationshipKind, World};
use crate::sim::religion_names::{generate_deity_name, generate_religion_name};
use crate::worldgen::config::WorldGenConfig;

/// All tenet variants for random selection.
const ALL_TENETS: [ReligiousTenet; 8] = [
    ReligiousTenet::WarGod,
    ReligiousTenet::NatureWorship,
    ReligiousTenet::AncestorCult,
    ReligiousTenet::Prophecy,
    ReligiousTenet::Asceticism,
    ReligiousTenet::Commerce,
    ReligiousTenet::Knowledge,
    ReligiousTenet::Death,
];

/// All deity domains for random selection.
const ALL_DOMAINS: [DeityDomain; 10] = [
    DeityDomain::Sky,
    DeityDomain::Earth,
    DeityDomain::Sea,
    DeityDomain::War,
    DeityDomain::Death,
    DeityDomain::Harvest,
    DeityDomain::Craft,
    DeityDomain::Wisdom,
    DeityDomain::Storm,
    DeityDomain::Fire,
];

/// Pick 1-2 tenets, biased by the faction's culture values.
fn pick_tenets(rng: &mut dyn RngCore, culture_values: &[CulturalValue]) -> Vec<ReligiousTenet> {
    let count = rng.random_range(1..=2);
    let mut chosen = Vec::with_capacity(count);

    for _ in 0..count {
        // Build weighted candidates
        let mut candidates: Vec<(ReligiousTenet, u32)> = Vec::new();
        for &tenet in &ALL_TENETS {
            if chosen.contains(&tenet) {
                continue;
            }
            let weight = tenet_weight(tenet, culture_values);
            candidates.push((tenet, weight));
        }
        if candidates.is_empty() {
            break;
        }
        let total: u32 = candidates.iter().map(|(_, w)| w).sum();
        let mut roll = rng.random_range(0..total);
        for (tenet, w) in &candidates {
            if roll < *w {
                chosen.push(*tenet);
                break;
            }
            roll -= w;
        }
    }
    chosen
}

/// Weight a tenet based on culture values — biased choices feel natural.
fn tenet_weight(tenet: ReligiousTenet, values: &[CulturalValue]) -> u32 {
    match tenet {
        ReligiousTenet::WarGod => {
            if values.contains(&CulturalValue::Martial) {
                4
            } else {
                1
            }
        }
        ReligiousTenet::NatureWorship => {
            if values.contains(&CulturalValue::Agrarian)
                || values.contains(&CulturalValue::Spiritual)
            {
                3
            } else {
                1
            }
        }
        ReligiousTenet::AncestorCult => {
            if values.contains(&CulturalValue::Spiritual) {
                3
            } else {
                1
            }
        }
        ReligiousTenet::Knowledge => {
            if values.contains(&CulturalValue::Scholarly) {
                4
            } else {
                1
            }
        }
        ReligiousTenet::Commerce => {
            if values.contains(&CulturalValue::Mercantile) {
                3
            } else {
                1
            }
        }
        ReligiousTenet::Asceticism => {
            if values.contains(&CulturalValue::Spiritual)
                || values.contains(&CulturalValue::Isolationist)
            {
                2
            } else {
                1
            }
        }
        _ => 1,
    }
}

/// Pick a deity domain influenced by the religion's tenets.
fn pick_domain(rng: &mut dyn RngCore, tenets: &[ReligiousTenet]) -> DeityDomain {
    let mut candidates: Vec<(DeityDomain, u32)> = ALL_DOMAINS
        .iter()
        .map(|&d| {
            let w = domain_weight(d, tenets);
            (d, w)
        })
        .collect();
    let total: u32 = candidates.iter().map(|(_, w)| w).sum();
    let mut roll = rng.random_range(0..total);
    for (domain, w) in &candidates {
        if roll < *w {
            return *domain;
        }
        roll -= w;
    }
    candidates.pop().unwrap().0
}

fn domain_weight(domain: DeityDomain, tenets: &[ReligiousTenet]) -> u32 {
    match domain {
        DeityDomain::War => {
            if tenets.contains(&ReligiousTenet::WarGod) {
                4
            } else {
                1
            }
        }
        DeityDomain::Death => {
            if tenets.contains(&ReligiousTenet::Death)
                || tenets.contains(&ReligiousTenet::AncestorCult)
            {
                3
            } else {
                1
            }
        }
        DeityDomain::Harvest | DeityDomain::Earth => {
            if tenets.contains(&ReligiousTenet::NatureWorship) {
                3
            } else {
                1
            }
        }
        DeityDomain::Wisdom => {
            if tenets.contains(&ReligiousTenet::Knowledge) {
                3
            } else {
                1
            }
        }
        DeityDomain::Craft => {
            if tenets.contains(&ReligiousTenet::Commerce) {
                2
            } else {
                1
            }
        }
        _ => 1,
    }
}

/// Pipeline-compatible step that creates initial religions, one per faction.
pub fn generate_religions(
    world: &mut World,
    _config: &WorldGenConfig,
    rng: &mut dyn RngCore,
    _genesis_event: u64,
) {
    debug_assert!(
        world
            .entities
            .values()
            .any(|e| e.kind == EntityKind::Faction),
        "religions step requires factions to exist"
    );

    let faction_ids: Vec<u64> = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        .map(|e| e.id)
        .collect();

    if faction_ids.is_empty() {
        return;
    }

    for &faction_id in &faction_ids {
        // Get faction's culture values for tenet biasing
        let culture_values = get_faction_culture_values(world, faction_id);

        // Pick tenets
        let tenets = pick_tenets(rng, &culture_values);

        // Generate religion parameters
        let fervor = 0.4 + rng.random_range(0..=30) as f64 / 100.0; // 0.4-0.7
        let proselytism = 0.1 + rng.random_range(0..=40) as f64 / 100.0; // 0.1-0.5
        let orthodoxy = 0.3 + rng.random_range(0..=40) as f64 / 100.0; // 0.3-0.7

        // Create Religion entity
        let name = generate_religion_name(rng);
        let ev = world.add_event(
            crate::model::EventKind::Founded,
            crate::model::SimTimestamp::from_year(0),
            format!("{name} established"),
        );

        let religion_id = world.add_entity(
            EntityKind::Religion,
            name,
            Some(crate::model::SimTimestamp::from_year(0)),
            EntityData::Religion(ReligionData {
                fervor,
                proselytism,
                orthodoxy,
                tenets: tenets.clone(),
            }),
            ev,
        );

        // Create 1-3 Deity entities linked to this religion
        let deity_count = rng.random_range(1..=3);
        for _ in 0..deity_count {
            let domain = pick_domain(rng, &tenets);
            let worship_strength = 0.4 + rng.random_range(0..=50) as f64 / 100.0; // 0.4-0.9
            let deity_name = generate_deity_name(rng);
            let deity_id = world.add_entity(
                EntityKind::Deity,
                deity_name,
                Some(crate::model::SimTimestamp::from_year(0)),
                EntityData::Deity(DeityData {
                    domain,
                    worship_strength,
                }),
                ev,
            );
            world.add_relationship(
                deity_id,
                religion_id,
                RelationshipKind::MemberOf,
                crate::model::SimTimestamp::from_year(0),
                ev,
            );
        }

        // Set faction's primary religion
        if let Some(faction) = world.entities.get_mut(&faction_id)
            && let Some(fd) = faction.data.as_faction_mut()
        {
            fd.primary_religion = Some(religion_id);
        }

        // Set each settlement's dominant religion and religion_makeup
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
                sd.dominant_religion = Some(religion_id);
                sd.religion_makeup = BTreeMap::from([(religion_id, 1.0)]);
            }
        }
    }
}

/// Get a faction's culture values (if it has a primary culture).
fn get_faction_culture_values(world: &World, faction_id: u64) -> Vec<CulturalValue> {
    let culture_id = world
        .entities
        .get(&faction_id)
        .and_then(|e| e.data.as_faction())
        .and_then(|fd| fd.primary_culture);

    culture_id
        .and_then(|cid| world.entities.get(&cid))
        .and_then(|e| e.data.as_culture())
        .map(|cd| cd.values.clone())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    use crate::worldgen::config::{MapConfig, WorldGenConfig};
    use crate::worldgen::cultures::generate_cultures;
    use crate::worldgen::factions::generate_factions;
    use crate::worldgen::geography::generate_regions;
    use crate::worldgen::settlements::generate_settlements;

    fn make_world_with_cultures() -> (World, u64) {
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
        crate::worldgen::make_test_world(
            &config,
            &[
                generate_regions,
                generate_settlements,
                generate_factions,
                generate_cultures,
            ],
        )
    }

    #[test]
    fn religions_created_per_faction() {
        let (mut world, ev) = make_world_with_cultures();
        let mut rng = SmallRng::seed_from_u64(42);
        generate_religions(&mut world, &WorldGenConfig::default(), &mut rng, ev);

        let faction_count = world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
            .count();

        let religion_count = world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Religion && e.end.is_none())
            .count();

        assert_eq!(
            religion_count, faction_count,
            "should have one religion per faction"
        );
    }

    #[test]
    fn factions_have_primary_religion() {
        let (mut world, ev) = make_world_with_cultures();
        let mut rng = SmallRng::seed_from_u64(42);
        generate_religions(&mut world, &WorldGenConfig::default(), &mut rng, ev);

        for faction in world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        {
            let fd = faction.data.as_faction().unwrap();
            assert!(
                fd.primary_religion.is_some(),
                "faction {} should have primary religion",
                faction.name
            );
        }
    }

    #[test]
    fn settlements_have_dominant_religion() {
        let (mut world, ev) = make_world_with_cultures();
        let mut rng = SmallRng::seed_from_u64(42);
        generate_religions(&mut world, &WorldGenConfig::default(), &mut rng, ev);

        for settlement in world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
        {
            let sd = settlement.data.as_settlement().unwrap();
            let has_faction = settlement.active_rel(RelationshipKind::MemberOf).is_some();
            if has_faction {
                assert!(
                    sd.dominant_religion.is_some(),
                    "settlement {} should have dominant religion",
                    settlement.name
                );
                assert!(
                    !sd.religion_makeup.is_empty(),
                    "settlement {} should have religion makeup",
                    settlement.name
                );
            }
        }
    }

    #[test]
    fn deities_linked_to_religions() {
        let (mut world, ev) = make_world_with_cultures();
        let mut rng = SmallRng::seed_from_u64(42);
        generate_religions(&mut world, &WorldGenConfig::default(), &mut rng, ev);

        let deity_count = world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Deity && e.end.is_none())
            .count();

        assert!(deity_count > 0, "should create deities");

        // Every deity should have a MemberOf→religion
        for deity in world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Deity && e.end.is_none())
        {
            let religion_id = deity.active_rel(RelationshipKind::MemberOf);
            assert!(
                religion_id.is_some(),
                "deity {} should be linked to a religion",
                deity.name
            );
            let rid = religion_id.unwrap();
            let religion = world.entities.get(&rid).unwrap();
            assert_eq!(religion.kind, EntityKind::Religion);
        }
    }

    #[test]
    fn religion_data_within_bounds() {
        let (mut world, ev) = make_world_with_cultures();
        let mut rng = SmallRng::seed_from_u64(42);
        generate_religions(&mut world, &WorldGenConfig::default(), &mut rng, ev);

        for religion in world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Religion)
        {
            let rd = religion.data.as_religion().unwrap();
            assert!(
                (0.0..=1.0).contains(&rd.fervor),
                "fervor out of range: {}",
                rd.fervor
            );
            assert!(
                (0.0..=1.0).contains(&rd.proselytism),
                "proselytism out of range: {}",
                rd.proselytism
            );
            assert!(
                (0.0..=1.0).contains(&rd.orthodoxy),
                "orthodoxy out of range: {}",
                rd.orthodoxy
            );
            assert!(
                !rd.tenets.is_empty() && rd.tenets.len() <= 2,
                "should have 1-2 tenets, got {}",
                rd.tenets.len()
            );
        }
    }
}
