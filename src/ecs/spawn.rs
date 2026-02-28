use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;

use crate::ecs::components::*;
use crate::ecs::resources::SimEntityMap;
use crate::ecs::time::SimTime;

fn register(world: &mut World, id: u64, entity: Entity) {
    // Graceful when SimEntityMap is temporarily removed from the world
    // (e.g. during apply_sim_commands, which extracts it into ApplyCtx).
    // In that case, the caller (apply_* functions) handles registration
    // via ctx.entity_map.insert() instead.
    if let Some(mut map) = world.get_resource_mut::<SimEntityMap>() {
        map.insert(id, entity);
    }
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_person(
    world: &mut World,
    id: u64,
    name: String,
    origin: Option<SimTime>,
    core: PersonCore,
    reputation: PersonReputation,
    social: PersonSocial,
    education: PersonEducation,
) -> Entity {
    let entity = world
        .spawn((
            SimEntity {
                id,
                name,
                origin,
                end: None,
            },
            Person,
            core,
            reputation,
            social,
            education,
        ))
        .id();
    register(world, id, entity);
    entity
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_settlement(
    world: &mut World,
    id: u64,
    name: String,
    origin: Option<SimTime>,
    core: SettlementCore,
    culture: SettlementCulture,
    disease: SettlementDisease,
    trade: SettlementTrade,
    military: SettlementMilitary,
    crime: SettlementCrime,
    education: SettlementEducation,
    seasonal: EcsSeasonalModifiers,
    bonuses: EcsBuildingBonuses,
) -> Entity {
    let entity = world
        .spawn((
            SimEntity {
                id,
                name,
                origin,
                end: None,
            },
            Settlement,
            core,
            culture,
            disease,
            trade,
            military,
            crime,
            education,
            seasonal,
            bonuses,
        ))
        .id();
    register(world, id, entity);
    entity
}

pub fn spawn_faction(
    world: &mut World,
    id: u64,
    name: String,
    origin: Option<SimTime>,
    core: FactionCore,
    diplomacy: FactionDiplomacy,
    military: FactionMilitary,
) -> Entity {
    let entity = world
        .spawn((
            SimEntity {
                id,
                name,
                origin,
                end: None,
            },
            Faction,
            core,
            diplomacy,
            military,
        ))
        .id();
    register(world, id, entity);
    entity
}

pub fn spawn_army(
    world: &mut World,
    id: u64,
    name: String,
    origin: Option<SimTime>,
    state: ArmyState,
) -> Entity {
    let entity = world
        .spawn((
            SimEntity {
                id,
                name,
                origin,
                end: None,
            },
            Army,
            state,
        ))
        .id();
    register(world, id, entity);
    entity
}

pub fn spawn_region(
    world: &mut World,
    id: u64,
    name: String,
    origin: Option<SimTime>,
    state: RegionState,
) -> Entity {
    let entity = world
        .spawn((
            SimEntity {
                id,
                name,
                origin,
                end: None,
            },
            Region,
            state,
        ))
        .id();
    register(world, id, entity);
    entity
}

pub fn spawn_building(
    world: &mut World,
    id: u64,
    name: String,
    origin: Option<SimTime>,
    state: BuildingState,
) -> Entity {
    let entity = world
        .spawn((
            SimEntity {
                id,
                name,
                origin,
                end: None,
            },
            Building,
            state,
        ))
        .id();
    register(world, id, entity);
    entity
}

pub fn spawn_item(
    world: &mut World,
    id: u64,
    name: String,
    origin: Option<SimTime>,
    state: ItemState,
) -> Entity {
    let entity = world
        .spawn((
            SimEntity {
                id,
                name,
                origin,
                end: None,
            },
            ItemMarker,
            state,
        ))
        .id();
    register(world, id, entity);
    entity
}

pub fn spawn_deity(
    world: &mut World,
    id: u64,
    name: String,
    origin: Option<SimTime>,
    state: DeityState,
) -> Entity {
    let entity = world
        .spawn((
            SimEntity {
                id,
                name,
                origin,
                end: None,
            },
            Deity,
            state,
        ))
        .id();
    register(world, id, entity);
    entity
}

pub fn spawn_creature(world: &mut World, id: u64, name: String, origin: Option<SimTime>) -> Entity {
    let entity = world
        .spawn((
            SimEntity {
                id,
                name,
                origin,
                end: None,
            },
            Creature,
        ))
        .id();
    register(world, id, entity);
    entity
}

pub fn spawn_river(
    world: &mut World,
    id: u64,
    name: String,
    origin: Option<SimTime>,
    state: RiverState,
) -> Entity {
    let entity = world
        .spawn((
            SimEntity {
                id,
                name,
                origin,
                end: None,
            },
            River,
            state,
        ))
        .id();
    register(world, id, entity);
    entity
}

pub fn spawn_geographic_feature(
    world: &mut World,
    id: u64,
    name: String,
    origin: Option<SimTime>,
    state: GeographicFeatureState,
) -> Entity {
    let entity = world
        .spawn((
            SimEntity {
                id,
                name,
                origin,
                end: None,
            },
            GeographicFeature,
            state,
        ))
        .id();
    register(world, id, entity);
    entity
}

pub fn spawn_resource_deposit(
    world: &mut World,
    id: u64,
    name: String,
    origin: Option<SimTime>,
    state: ResourceDepositState,
) -> Entity {
    let entity = world
        .spawn((
            SimEntity {
                id,
                name,
                origin,
                end: None,
            },
            ResourceDeposit,
            state,
        ))
        .id();
    register(world, id, entity);
    entity
}

pub fn spawn_culture(
    world: &mut World,
    id: u64,
    name: String,
    origin: Option<SimTime>,
    state: CultureState,
) -> Entity {
    let entity = world
        .spawn((
            SimEntity {
                id,
                name,
                origin,
                end: None,
            },
            Culture,
            state,
        ))
        .id();
    register(world, id, entity);
    entity
}

pub fn spawn_disease(
    world: &mut World,
    id: u64,
    name: String,
    origin: Option<SimTime>,
    state: DiseaseState,
) -> Entity {
    let entity = world
        .spawn((
            SimEntity {
                id,
                name,
                origin,
                end: None,
            },
            Disease,
            state,
        ))
        .id();
    register(world, id, entity);
    entity
}

pub fn spawn_knowledge(
    world: &mut World,
    id: u64,
    name: String,
    origin: Option<SimTime>,
    state: KnowledgeState,
) -> Entity {
    let entity = world
        .spawn((
            SimEntity {
                id,
                name,
                origin,
                end: None,
            },
            Knowledge,
            state,
        ))
        .id();
    register(world, id, entity);
    entity
}

pub fn spawn_manifestation(
    world: &mut World,
    id: u64,
    name: String,
    origin: Option<SimTime>,
    state: ManifestationState,
) -> Entity {
    let entity = world
        .spawn((
            SimEntity {
                id,
                name,
                origin,
                end: None,
            },
            Manifestation,
            state,
        ))
        .id();
    register(world, id, entity);
    entity
}

pub fn spawn_religion(
    world: &mut World,
    id: u64,
    name: String,
    origin: Option<SimTime>,
    state: ReligionState,
) -> Entity {
    let entity = world
        .spawn((
            SimEntity {
                id,
                name,
                origin,
                end: None,
            },
            ReligionMarker,
            state,
        ))
        .id();
    register(world, id, entity);
    entity
}

#[cfg(test)]
mod tests {
    use bevy_ecs::query::With;

    use super::*;
    use crate::ecs::components::dynamic::EcsActiveSiege;
    use crate::ecs::relationships::{
        LocatedIn, LocatedInSources, MemberOf, RegionAdjacency, RelationshipGraph, RelationshipMeta,
    };
    use crate::model::{CulturalValue, NamingStyle, Terrain};

    /// Build a minimal World with SimEntityMap inserted.
    fn test_world() -> World {
        let mut world = World::new();
        world.insert_resource(SimEntityMap::new());
        world
    }

    // -----------------------------------------------------------------------
    // Per-kind spawn + query round-trips
    // -----------------------------------------------------------------------

    #[test]
    fn spawn_and_query_person() {
        let mut world = test_world();
        let e = spawn_person(
            &mut world,
            1,
            "Aldric".into(),
            Some(SimTime::from_year(100)),
            PersonCore::default(),
            PersonReputation::default(),
            PersonSocial::default(),
            PersonEducation::default(),
        );
        let sim = world.get::<SimEntity>(e).unwrap();
        assert_eq!(sim.id, 1);
        assert_eq!(sim.name, "Aldric");
        assert!(sim.is_alive());
        assert!(world.get::<Person>(e).is_some());
    }

    #[test]
    fn spawn_and_query_settlement() {
        let mut world = test_world();
        let e = spawn_settlement(
            &mut world,
            2,
            "Ironhold".into(),
            None,
            SettlementCore::default(),
            SettlementCulture::default(),
            SettlementDisease::default(),
            SettlementTrade::default(),
            SettlementMilitary::default(),
            SettlementCrime::default(),
            SettlementEducation::default(),
            EcsSeasonalModifiers::default(),
            EcsBuildingBonuses::default(),
        );
        assert!(world.get::<Settlement>(e).is_some());
        assert!(world.get::<SettlementCore>(e).is_some());
    }

    #[test]
    fn spawn_and_query_faction() {
        let mut world = test_world();
        let e = spawn_faction(
            &mut world,
            3,
            "Iron Legion".into(),
            None,
            FactionCore::default(),
            FactionDiplomacy::default(),
            FactionMilitary::default(),
        );
        assert!(world.get::<Faction>(e).is_some());
        assert!(world.get::<FactionCore>(e).is_some());
    }

    #[test]
    fn spawn_and_query_army() {
        let mut world = test_world();
        let e = spawn_army(
            &mut world,
            4,
            "1st Legion".into(),
            None,
            ArmyState::default(),
        );
        assert!(world.get::<Army>(e).is_some());
        assert!(world.get::<ArmyState>(e).is_some());
    }

    #[test]
    fn spawn_and_query_region() {
        let mut world = test_world();
        let e = spawn_region(
            &mut world,
            5,
            "Greenfield".into(),
            None,
            RegionState::default(),
        );
        assert!(world.get::<Region>(e).is_some());
        assert!(world.get::<RegionState>(e).is_some());
    }

    #[test]
    fn spawn_and_query_building() {
        let mut world = test_world();
        let e = spawn_building(
            &mut world,
            6,
            "Iron Mine".into(),
            None,
            BuildingState::default(),
        );
        assert!(world.get::<Building>(e).is_some());
        assert!(world.get::<BuildingState>(e).is_some());
    }

    #[test]
    fn spawn_and_query_item() {
        let mut world = test_world();
        let e = spawn_item(
            &mut world,
            7,
            "Dragonbane".into(),
            None,
            ItemState::default(),
        );
        assert!(world.get::<ItemMarker>(e).is_some());
        assert!(world.get::<ItemState>(e).is_some());
    }

    #[test]
    fn spawn_and_query_deity() {
        let mut world = test_world();
        let e = spawn_deity(
            &mut world,
            8,
            "Solaris".into(),
            None,
            DeityState {
                domain: crate::model::entity_data::DeityDomain::Sky,
                worship_strength: 0.8,
            },
        );
        assert!(world.get::<Deity>(e).is_some());
        assert!(world.get::<DeityState>(e).is_some());
    }

    #[test]
    fn spawn_and_query_creature() {
        let mut world = test_world();
        let e = spawn_creature(&mut world, 9, "Dragon".into(), None);
        assert!(world.get::<Creature>(e).is_some());
    }

    #[test]
    fn spawn_and_query_river() {
        let mut world = test_world();
        let e = spawn_river(
            &mut world,
            10,
            "Silverstream".into(),
            None,
            RiverState {
                region_path: vec![1, 2, 3],
                length: 100,
            },
        );
        assert!(world.get::<River>(e).is_some());
        assert!(world.get::<RiverState>(e).is_some());
    }

    #[test]
    fn spawn_and_query_geographic_feature() {
        let mut world = test_world();
        let e = spawn_geographic_feature(
            &mut world,
            11,
            "Dark Cave".into(),
            None,
            GeographicFeatureState {
                feature_type: crate::model::FeatureType::Cave,
                x: 10.0,
                y: 20.0,
            },
        );
        assert!(world.get::<GeographicFeature>(e).is_some());
    }

    #[test]
    fn spawn_and_query_resource_deposit() {
        let mut world = test_world();
        let e = spawn_resource_deposit(
            &mut world,
            12,
            "Gold Vein".into(),
            None,
            ResourceDepositState {
                resource_type: crate::model::ResourceType::Gold,
                quantity: 500,
                quality: 0.9,
                discovered: true,
                x: 5.0,
                y: 5.0,
            },
        );
        assert!(world.get::<ResourceDeposit>(e).is_some());
    }

    #[test]
    fn spawn_and_query_culture() {
        let mut world = test_world();
        let e = spawn_culture(
            &mut world,
            13,
            "Northern".into(),
            None,
            CultureState {
                values: vec![CulturalValue::Martial],
                naming_style: NamingStyle::Nordic,
                resistance: 0.5,
            },
        );
        assert!(world.get::<Culture>(e).is_some());
        assert!(world.get::<CultureState>(e).is_some());
    }

    #[test]
    fn spawn_and_query_disease() {
        let mut world = test_world();
        let e = spawn_disease(
            &mut world,
            14,
            "Red Plague".into(),
            None,
            DiseaseState {
                virulence: 0.7,
                lethality: 0.3,
                duration_years: 2,
                bracket_severity: [0.1, 0.05, 0.08, 0.15, 0.4, 0.6, 0.8, 1.0],
            },
        );
        assert!(world.get::<Disease>(e).is_some());
        assert!(world.get::<DiseaseState>(e).is_some());
    }

    #[test]
    fn spawn_and_query_knowledge() {
        let mut world = test_world();
        let e = spawn_knowledge(
            &mut world,
            15,
            "Battle of Iron Pass".into(),
            None,
            KnowledgeState {
                category: crate::model::KnowledgeCategory::Battle,
                source_event_id: 100,
                origin_settlement_id: 2,
                origin_time: SimTime::from_year(50),
                significance: 0.8,
                ground_truth: serde_json::json!({"winner": "Iron Legion"}),
                revealed_at: None,
                secret_sensitivity: None,
                secret_motivation: None,
            },
        );
        assert!(world.get::<Knowledge>(e).is_some());
        assert!(world.get::<KnowledgeState>(e).is_some());
    }

    #[test]
    fn spawn_and_query_manifestation() {
        let mut world = test_world();
        let e = spawn_manifestation(
            &mut world,
            16,
            "Song of Iron Pass".into(),
            None,
            ManifestationState {
                knowledge_id: 15,
                medium: crate::model::Medium::Song,
                content: serde_json::json!({"lyrics": "..."}),
                accuracy: 0.7,
                completeness: 0.9,
                distortions: vec![],
                derived_from_id: None,
                derivation_method: crate::model::DerivationMethod::Witnessed,
                condition: 1.0,
                created: SimTime::from_year(51),
            },
        );
        assert!(world.get::<Manifestation>(e).is_some());
        assert!(world.get::<ManifestationState>(e).is_some());
    }

    #[test]
    fn spawn_and_query_religion() {
        let mut world = test_world();
        let e = spawn_religion(
            &mut world,
            17,
            "Faith of the Sun".into(),
            None,
            ReligionState::default(),
        );
        assert!(world.get::<ReligionMarker>(e).is_some());
        assert!(world.get::<ReligionState>(e).is_some());
    }

    // -----------------------------------------------------------------------
    // SimEntityMap tests
    // -----------------------------------------------------------------------

    #[test]
    fn entity_map_bidirectional_lookup() {
        let mut world = test_world();
        let e = spawn_region(&mut world, 42, "Test".into(), None, RegionState::default());

        let map = world.resource::<SimEntityMap>();
        assert_eq!(map.get_bevy(42), Some(e));
        assert_eq!(map.bevy(42), e);
        assert_eq!(map.get_sim(e), Some(42));
        assert_eq!(map.sim(e), 42);
        assert_eq!(map.len(), 1);
        assert!(!map.is_empty());
    }

    #[test]
    #[should_panic(expected = "duplicate sim_id 1")]
    fn entity_map_duplicate_panics() {
        let mut world = test_world();
        spawn_region(&mut world, 1, "A".into(), None, RegionState::default());
        spawn_region(&mut world, 1, "B".into(), None, RegionState::default());
    }

    // -----------------------------------------------------------------------
    // Dynamic component add/remove
    // -----------------------------------------------------------------------

    #[test]
    fn dynamic_component_add_remove() {
        let mut world = test_world();
        let e = spawn_settlement(
            &mut world,
            1,
            "Town".into(),
            None,
            SettlementCore::default(),
            SettlementCulture::default(),
            SettlementDisease::default(),
            SettlementTrade::default(),
            SettlementMilitary::default(),
            SettlementCrime::default(),
            SettlementEducation::default(),
            EcsSeasonalModifiers::default(),
            EcsBuildingBonuses::default(),
        );

        // No siege initially
        assert!(world.get::<EcsActiveSiege>(e).is_none());

        // Add siege
        world.entity_mut(e).insert(EcsActiveSiege {
            attacker_army_id: 10,
            attacker_faction_id: 20,
            started: SimTime::from_year(100),
            months_elapsed: 0,
            civilian_deaths: 0,
        });
        assert!(world.get::<EcsActiveSiege>(e).is_some());

        // Query with filter
        let count = world
            .query_filtered::<&SimEntity, With<EcsActiveSiege>>()
            .iter(&world)
            .count();
        assert_eq!(count, 1);

        // Remove siege
        world.entity_mut(e).remove::<EcsActiveSiege>();
        assert!(world.get::<EcsActiveSiege>(e).is_none());
    }

    // -----------------------------------------------------------------------
    // Structural relationships
    // -----------------------------------------------------------------------

    #[test]
    fn structural_relationship_located_in() {
        let mut world = test_world();
        let region = spawn_region(
            &mut world,
            1,
            "Plains".into(),
            None,
            RegionState {
                terrain: Terrain::Plains,
                ..RegionState::default()
            },
        );
        let settlement = spawn_settlement(
            &mut world,
            2,
            "Town".into(),
            None,
            SettlementCore::default(),
            SettlementCulture::default(),
            SettlementDisease::default(),
            SettlementTrade::default(),
            SettlementMilitary::default(),
            SettlementCrime::default(),
            SettlementEducation::default(),
            EcsSeasonalModifiers::default(),
            EcsBuildingBonuses::default(),
        );

        // Add LocatedIn relationship
        world.entity_mut(settlement).insert(LocatedIn(region));

        // Verify the source side
        let loc = world.get::<LocatedIn>(settlement).unwrap();
        assert_eq!(loc.0, region);

        // Verify the target side (auto-populated by Bevy)
        let sources = world.get::<LocatedInSources>(region).unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0], settlement);
    }

    // -----------------------------------------------------------------------
    // RegionAdjacency
    // -----------------------------------------------------------------------

    #[test]
    fn region_adjacency_bidirectional() {
        let mut world = test_world();
        let r1 = spawn_region(&mut world, 1, "A".into(), None, RegionState::default());
        let r2 = spawn_region(&mut world, 2, "B".into(), None, RegionState::default());
        let r3 = spawn_region(&mut world, 3, "C".into(), None, RegionState::default());

        let mut adj = RegionAdjacency::new();
        adj.add_edge(r1, r2);
        adj.add_edge(r1, r3);

        assert!(adj.are_adjacent(r1, r2));
        assert!(adj.are_adjacent(r2, r1)); // bidirectional
        assert!(adj.are_adjacent(r1, r3));
        assert!(!adj.are_adjacent(r2, r3));

        // Sorted neighbors
        let neighbors = adj.neighbors(r1);
        assert_eq!(neighbors.len(), 2);
    }

    #[test]
    fn region_adjacency_duplicate_edge_idempotent() {
        let mut world = test_world();
        let r1 = spawn_region(&mut world, 1, "A".into(), None, RegionState::default());
        let r2 = spawn_region(&mut world, 2, "B".into(), None, RegionState::default());

        let mut adj = RegionAdjacency::new();
        adj.add_edge(r1, r2);
        adj.add_edge(r1, r2); // duplicate

        assert_eq!(adj.neighbors(r1).len(), 1);
        assert_eq!(adj.neighbors(r2).len(), 1);
    }

    // -----------------------------------------------------------------------
    // RelationshipGraph
    // -----------------------------------------------------------------------

    #[test]
    fn relationship_graph_canonical_pair() {
        let mut world = test_world();
        let a = spawn_faction(
            &mut world,
            1,
            "A".into(),
            None,
            FactionCore::default(),
            FactionDiplomacy::default(),
            FactionMilitary::default(),
        );
        let b = spawn_faction(
            &mut world,
            2,
            "B".into(),
            None,
            FactionCore::default(),
            FactionDiplomacy::default(),
            FactionMilitary::default(),
        );

        let pair_ab = RelationshipGraph::canonical_pair(a, b);
        let pair_ba = RelationshipGraph::canonical_pair(b, a);
        assert_eq!(pair_ab, pair_ba);
    }

    #[test]
    fn relationship_graph_ally_war_queries() {
        let mut world = test_world();
        let a = spawn_faction(
            &mut world,
            1,
            "A".into(),
            None,
            FactionCore::default(),
            FactionDiplomacy::default(),
            FactionMilitary::default(),
        );
        let b = spawn_faction(
            &mut world,
            2,
            "B".into(),
            None,
            FactionCore::default(),
            FactionDiplomacy::default(),
            FactionMilitary::default(),
        );
        let c = spawn_faction(
            &mut world,
            3,
            "C".into(),
            None,
            FactionCore::default(),
            FactionDiplomacy::default(),
            FactionMilitary::default(),
        );

        let mut graph = RelationshipGraph::new();
        let pair_ab = RelationshipGraph::canonical_pair(a, b);
        graph
            .allies
            .insert(pair_ab, RelationshipMeta::new(SimTime::from_year(100)));

        let pair_ac = RelationshipGraph::canonical_pair(a, c);
        graph
            .at_war
            .insert(pair_ac, RelationshipMeta::new(SimTime::from_year(110)));

        assert!(graph.are_allies(a, b));
        assert!(graph.are_allies(b, a)); // symmetric
        assert!(!graph.are_allies(a, c));

        assert!(graph.are_at_war(a, c));
        assert!(graph.are_at_war(c, a)); // symmetric
        assert!(!graph.are_at_war(a, b));
    }

    // -----------------------------------------------------------------------
    // Integration: spawn_mini_world
    // -----------------------------------------------------------------------

    #[test]
    fn spawn_mini_world() {
        let mut world = test_world();

        // 2 adjacent regions
        let region1 = spawn_region(
            &mut world,
            1,
            "Plains of Valor".into(),
            Some(SimTime::from_year(0)),
            RegionState {
                terrain: Terrain::Plains,
                x: 0.0,
                y: 0.0,
                ..RegionState::default()
            },
        );
        let region2 = spawn_region(
            &mut world,
            2,
            "Iron Hills".into(),
            Some(SimTime::from_year(0)),
            RegionState {
                terrain: Terrain::Hills,
                x: 1.0,
                y: 0.0,
                ..RegionState::default()
            },
        );

        let mut adjacency = RegionAdjacency::new();
        adjacency.add_edge(region1, region2);
        world.insert_resource(adjacency);

        // 1 faction with leader
        let faction = spawn_faction(
            &mut world,
            3,
            "Iron Legion".into(),
            Some(SimTime::from_year(50)),
            FactionCore::default(),
            FactionDiplomacy::default(),
            FactionMilitary::default(),
        );

        let leader = spawn_person(
            &mut world,
            4,
            "Lord Commander".into(),
            Some(SimTime::from_year(70)),
            PersonCore::default(),
            PersonReputation::default(),
            PersonSocial::default(),
            PersonEducation::default(),
        );
        world.entity_mut(leader).insert(MemberOf(faction));

        // 2 settlements, one per region, both in faction
        let settlement1 = spawn_settlement(
            &mut world,
            5,
            "Ironhold".into(),
            Some(SimTime::from_year(50)),
            SettlementCore {
                x: 0.0,
                y: 0.0,
                population: 500,
                ..SettlementCore::default()
            },
            SettlementCulture::default(),
            SettlementDisease::default(),
            SettlementTrade::default(),
            SettlementMilitary::default(),
            SettlementCrime::default(),
            SettlementEducation::default(),
            EcsSeasonalModifiers::default(),
            EcsBuildingBonuses::default(),
        );
        world
            .entity_mut(settlement1)
            .insert((LocatedIn(region1), MemberOf(faction)));

        let settlement2 = spawn_settlement(
            &mut world,
            6,
            "Hillwatch".into(),
            Some(SimTime::from_year(60)),
            SettlementCore {
                x: 1.0,
                y: 0.0,
                population: 200,
                ..SettlementCore::default()
            },
            SettlementCulture::default(),
            SettlementDisease::default(),
            SettlementTrade::default(),
            SettlementMilitary::default(),
            SettlementCrime::default(),
            SettlementEducation::default(),
            EcsSeasonalModifiers::default(),
            EcsBuildingBonuses::default(),
        );
        world
            .entity_mut(settlement2)
            .insert((LocatedIn(region2), MemberOf(faction)));

        // 2 more persons in settlements
        let person2 = spawn_person(
            &mut world,
            7,
            "Warrior".into(),
            Some(SimTime::from_year(80)),
            PersonCore::default(),
            PersonReputation::default(),
            PersonSocial::default(),
            PersonEducation::default(),
        );
        world.entity_mut(person2).insert(MemberOf(faction));

        let person3 = spawn_person(
            &mut world,
            8,
            "Scholar".into(),
            Some(SimTime::from_year(85)),
            PersonCore::default(),
            PersonReputation::default(),
            PersonSocial::default(),
            PersonEducation::default(),
        );
        world.entity_mut(person3).insert(MemberOf(faction));

        // 1 army in faction, in region
        let army = spawn_army(
            &mut world,
            9,
            "Iron Guard".into(),
            Some(SimTime::from_year(100)),
            ArmyState {
                strength: 100,
                faction_id: 3,
                home_region_id: 1,
                ..ArmyState::default()
            },
        );
        world.entity_mut(army).insert(LocatedIn(region1));

        // ---------------------------------------------------------------
        // Verifications
        // ---------------------------------------------------------------

        // Query settlements with LocatedIn
        let settlement_count = world
            .query_filtered::<&SimEntity, (With<Settlement>, With<LocatedIn>)>()
            .iter(&world)
            .count();
        assert_eq!(settlement_count, 2);

        // RegionAdjacency works
        let adj = world.resource::<RegionAdjacency>();
        assert!(adj.are_adjacent(region1, region2));
        assert!(!adj.are_adjacent(region1, region1));

        // SimEntityMap bidirectional lookups
        let map = world.resource::<SimEntityMap>();
        assert_eq!(map.len(), 9);
        assert_eq!(map.bevy(1), region1);
        assert_eq!(map.bevy(5), settlement1);
        assert_eq!(map.sim(leader), 4);
        assert_eq!(map.sim(army), 9);

        // RelationshipGraph ally/war queries
        let mut graph = RelationshipGraph::new();
        let pair = RelationshipGraph::canonical_pair(faction, faction);
        assert!(!graph.are_allies(faction, faction));
        // Add an ally for testing
        let dummy_faction = spawn_faction(
            &mut world,
            10,
            "Allies".into(),
            None,
            FactionCore::default(),
            FactionDiplomacy::default(),
            FactionMilitary::default(),
        );
        let pair = RelationshipGraph::canonical_pair(faction, dummy_faction);
        graph
            .allies
            .insert(pair, RelationshipMeta::new(SimTime::from_year(100)));
        assert!(graph.are_allies(faction, dummy_faction));

        // Dynamic component add/remove (EcsActiveSiege)
        assert!(world.get::<EcsActiveSiege>(settlement1).is_none());
        world.entity_mut(settlement1).insert(EcsActiveSiege {
            attacker_army_id: 9,
            attacker_faction_id: 10,
            started: SimTime::from_year(110),
            months_elapsed: 0,
            civilian_deaths: 0,
        });
        let siege_count = world
            .query_filtered::<&SimEntity, With<EcsActiveSiege>>()
            .iter(&world)
            .count();
        assert_eq!(siege_count, 1);
        world.entity_mut(settlement1).remove::<EcsActiveSiege>();
        let siege_count = world
            .query_filtered::<&SimEntity, With<EcsActiveSiege>>()
            .iter(&world)
            .count();
        assert_eq!(siege_count, 0);
    }
}
