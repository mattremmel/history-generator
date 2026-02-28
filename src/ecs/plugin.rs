use bevy_app::{App, Plugin};

use super::systems::actions::ActionsPlugin;
use super::systems::agency::AgencyPlugin;
use super::systems::buildings::BuildingsPlugin;
use super::systems::conflicts::ConflictsPlugin;
use super::systems::crime::CrimePlugin;
use super::systems::culture::CulturePlugin;
use super::systems::demographics::DemographicsPlugin;
use super::systems::disease::DiseasePlugin;
use super::systems::economy::EconomyPlugin;
use super::systems::education::EducationPlugin;
use super::systems::environment::EnvironmentPlugin;
use super::systems::items::ItemsPlugin;
use super::systems::knowledge::KnowledgePlugin;
use super::systems::migration::MigrationPlugin;
use super::systems::politics::PoliticsPlugin;
use super::systems::religion::ReligionPlugin;
use super::systems::reputation::ReputationPlugin;

/// Aggregate plugin that installs all 17 simulation domain plugins.
pub struct SimPlugin;

impl Plugin for SimPlugin {
    fn build(&self, app: &mut App) {
        // Split into two add_plugins calls (Bevy tuple limit is 15)
        app.add_plugins((
            EnvironmentPlugin,
            BuildingsPlugin,
            DemographicsPlugin,
            EconomyPlugin,
            EducationPlugin,
            DiseasePlugin,
            CulturePlugin,
            ReligionPlugin,
            CrimePlugin,
            ReputationPlugin,
        ));
        app.add_plugins((
            KnowledgePlugin,
            ItemsPlugin,
            MigrationPlugin,
            PoliticsPlugin,
            ConflictsPlugin,
            AgencyPlugin,
            ActionsPlugin,
        ));
    }
}

#[cfg(test)]
mod tests {
    use bevy_ecs::schedule::ExecutorKind;

    use super::SimPlugin;
    use crate::ecs::app::{
        build_sim_app_deterministic, build_sim_app_seeded, build_sim_app_with_executor,
    };
    use crate::ecs::components::*;
    use crate::ecs::relationships::{LeaderOf, LocatedIn, MemberOf, RegionAdjacency};
    use crate::ecs::resources::{EcsIdGenerator, EventLog, SimEntityMap};
    use crate::ecs::spawn;
    use crate::ecs::test_helpers::{tick_months, tick_years};
    use crate::ecs::time::SimTime;
    use crate::model::population::PopulationBreakdown;
    use crate::worldgen::terrain::Terrain;

    /// Spawn a minimal world with two factions, two regions, two settlements,
    /// and one person per faction. Enough entities for most systems to exercise
    /// their logic without panicking.
    fn spawn_minimal_world(app: &mut bevy_app::App) {
        app.insert_resource(RegionAdjacency::new());

        // Advance ID generator past manually-assigned sim_ids (max is 101)
        *app.world_mut().resource_mut::<EcsIdGenerator>() =
            EcsIdGenerator(crate::id::IdGenerator::starting_from(1000));

        let world = app.world_mut();

        let r1 = spawn::spawn_region(
            world, 1, "Plains".into(), Some(SimTime::from_year(0)),
            RegionState { terrain: Terrain::Plains, ..RegionState::default() },
        );
        let r2 = spawn::spawn_region(
            world, 2, "Forest".into(), Some(SimTime::from_year(0)),
            RegionState { terrain: Terrain::Forest, ..RegionState::default() },
        );

        // Connect regions
        world.resource_mut::<RegionAdjacency>().add_edge(r1, r2);

        let f1 = spawn::spawn_faction(
            world, 10, "Kingdom A".into(), Some(SimTime::from_year(50)),
            FactionCore {
                stability: 0.5, happiness: 0.5, legitimacy: 0.5,
                treasury: 200.0,
                ..FactionCore::default()
            },
            FactionDiplomacy::default(),
            FactionMilitary::default(),
        );
        let f2 = spawn::spawn_faction(
            world, 11, "Kingdom B".into(), Some(SimTime::from_year(50)),
            FactionCore {
                stability: 0.5, happiness: 0.5, legitimacy: 0.5,
                treasury: 200.0,
                ..FactionCore::default()
            },
            FactionDiplomacy::default(),
            FactionMilitary::default(),
        );

        let s1 = spawn::spawn_settlement(
            world, 20, "Townburg".into(), Some(SimTime::from_year(50)),
            SettlementCore {
                population: 300,
                population_breakdown: PopulationBreakdown::from_total(300),
                prosperity: 0.5, capacity: 500,
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
        world.entity_mut(s1).insert((LocatedIn(r1), MemberOf(f1)));

        let s2 = spawn::spawn_settlement(
            world, 21, "Villageton".into(), Some(SimTime::from_year(50)),
            SettlementCore {
                population: 200,
                population_breakdown: PopulationBreakdown::from_total(200),
                prosperity: 0.4, capacity: 400,
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
        world.entity_mut(s2).insert((LocatedIn(r2), MemberOf(f2)));

        // One person per faction (for agency/politics)
        let p1 = spawn::spawn_person(
            world, 100, "Alice".into(), Some(SimTime::from_year(80)),
            PersonCore { born: SimTime::from_year(80), ..PersonCore::default() },
            PersonReputation::default(), PersonSocial::default(), PersonEducation::default(),
        );
        world.entity_mut(p1).insert((LocatedIn(s1), MemberOf(f1), LeaderOf(f1)));

        let p2 = spawn::spawn_person(
            world, 101, "Bob".into(), Some(SimTime::from_year(80)),
            PersonCore { born: SimTime::from_year(80), ..PersonCore::default() },
            PersonReputation::default(), PersonSocial::default(), PersonEducation::default(),
        );
        world.entity_mut(p2).insert((LocatedIn(s2), MemberOf(f2), LeaderOf(f2)));
    }

    #[test]
    fn sim_plugin_smoke_test_multithreaded() {
        let mut app = build_sim_app_seeded(100, 42);
        app.add_plugins(SimPlugin);
        spawn_minimal_world(&mut app);
        // Run 1 year — exercises yearly and monthly systems without excessive runtime
        tick_years(&mut app, 1);
        let clock = app.world().resource::<crate::ecs::clock::SimClock>();
        assert_eq!(clock.time.year(), 101);
    }

    #[test]
    fn sim_plugin_smoke_test_singlethreaded() {
        let mut app = build_sim_app_deterministic(100, 42);
        app.add_plugins(SimPlugin);
        spawn_minimal_world(&mut app);
        tick_years(&mut app, 1);
        let clock = app.world().resource::<crate::ecs::clock::SimClock>();
        assert_eq!(clock.time.year(), 101);
    }

    #[test]
    fn deterministic_singlethreaded_produces_identical_event_logs() {
        let mut app1 = build_sim_app_deterministic(100, 42);
        app1.add_plugins(SimPlugin);
        spawn_minimal_world(&mut app1);
        tick_months(&mut app1, 6);

        let mut app2 = build_sim_app_deterministic(100, 42);
        app2.add_plugins(SimPlugin);
        spawn_minimal_world(&mut app2);
        tick_months(&mut app2, 6);

        let log1 = app1.world().resource::<EventLog>();
        let log2 = app2.world().resource::<EventLog>();
        assert_eq!(log1.events.len(), log2.events.len(),
            "Event count mismatch: {} vs {}", log1.events.len(), log2.events.len());
        for (i, (e1, e2)) in log1.events.iter().zip(log2.events.iter()).enumerate() {
            assert_eq!(e1.kind, e2.kind, "Event kind mismatch at index {i}");
            assert_eq!(e1.timestamp, e2.timestamp, "Event time mismatch at index {i}");
        }
    }

    #[test]
    fn both_executors_produce_valid_worlds() {
        let mut app_mt = build_sim_app_with_executor(100, 99, ExecutorKind::MultiThreaded);
        app_mt.add_plugins(SimPlugin);
        spawn_minimal_world(&mut app_mt);
        tick_months(&mut app_mt, 6);

        let mut app_st = build_sim_app_with_executor(100, 99, ExecutorKind::SingleThreaded);
        app_st.add_plugins(SimPlugin);
        spawn_minimal_world(&mut app_st);
        tick_months(&mut app_st, 6);

        // Both should have produced some events
        let log_mt = app_mt.world().resource::<EventLog>();
        let log_st = app_st.world().resource::<EventLog>();
        assert!(!log_mt.events.is_empty(), "MultiThreaded produced no events");
        assert!(!log_st.events.is_empty(), "SingleThreaded produced no events");

        // Entity counts should be equal (same seed, same RNG consumption order per domain)
        let map_mt = app_mt.world().resource::<SimEntityMap>();
        let map_st = app_st.world().resource::<SimEntityMap>();
        assert_eq!(map_mt.len(), map_st.len(),
            "Entity count mismatch: MT={} vs ST={}", map_mt.len(), map_st.len());
    }
}
