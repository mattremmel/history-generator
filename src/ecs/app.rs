use bevy_app::App;
use bevy_ecs::message::MessageRegistry;
use bevy_ecs::schedule::{ExecutorKind, IntoScheduleConfigs};
use rand::SeedableRng;
use rand::rngs::SmallRng;

use super::clock::SimClock;
use super::commands::{SimCommand, apply_sim_commands};
use super::events::SimReactiveEvent;
use super::relationships::RelationshipGraph;
use super::resources::{
    ActionsRng, AgencyRng, BuildingsRng, ConflictsRng, CrimeRng, CultureRng, DemographicsRng,
    DiseaseRng, EconomyRng, EcsIdGenerator, EducationRng, EnvironmentRng, EventLog, ItemsRng,
    KnowledgeRng, MigrationRng, PoliticsRng, ReligionRng, ReputationRng, SimEntityMap, SimRng,
    distribute_rng,
};
use super::schedule::{SimPhase, configure_sim_schedule};

/// Build a headless Bevy app with simulation clock, core resources,
/// message types, and the command applicator.
///
/// Manual tick control:
/// ```no_run
/// # use history_gen::ecs::{build_sim_app, SimTick};
/// let mut app = build_sim_app(100);
/// for _ in 0..518_400 {  // 1 year of minute-level ticks
///     app.world_mut().run_schedule(SimTick);
/// }
/// ```
pub fn build_sim_app(start_year: u32) -> App {
    build_sim_app_seeded(start_year, 42)
}

/// Build a headless Bevy app with a specific RNG seed and multi-threaded executor.
pub fn build_sim_app_seeded(start_year: u32, seed: u64) -> App {
    build_sim_app_with_executor(start_year, seed, ExecutorKind::MultiThreaded)
}

/// Build a headless Bevy app with single-threaded executor for reproducible determinism.
///
/// Use this when exact RNG consumption order across ticks must be identical across runs.
pub fn build_sim_app_deterministic(start_year: u32, seed: u64) -> App {
    build_sim_app_with_executor(start_year, seed, ExecutorKind::SingleThreaded)
}

/// Build a headless Bevy app with a specific executor kind.
pub fn build_sim_app_with_executor(start_year: u32, seed: u64, executor: ExecutorKind) -> App {
    let mut app = App::empty();

    // Core resources
    app.insert_resource(SimClock::new(start_year));
    app.insert_resource(EventLog::new());
    app.insert_resource(EcsIdGenerator::default());
    app.insert_resource(SimEntityMap::new());
    app.insert_resource(RelationshipGraph::new());
    app.insert_resource(SimRng {
        rng: SmallRng::seed_from_u64(seed),
        seed,
    });

    // Per-domain RNG resources (reseeded each tick by distribute_rng)
    app.init_resource::<EnvironmentRng>();
    app.init_resource::<BuildingsRng>();
    app.init_resource::<DemographicsRng>();
    app.init_resource::<EconomyRng>();
    app.init_resource::<EducationRng>();
    app.init_resource::<DiseaseRng>();
    app.init_resource::<CultureRng>();
    app.init_resource::<ReligionRng>();
    app.init_resource::<CrimeRng>();
    app.init_resource::<ReputationRng>();
    app.init_resource::<KnowledgeRng>();
    app.init_resource::<ItemsRng>();
    app.init_resource::<MigrationRng>();
    app.init_resource::<PoliticsRng>();
    app.init_resource::<ConflictsRng>();
    app.init_resource::<AgencyRng>();
    app.init_resource::<ActionsRng>();

    // Register message types
    MessageRegistry::register_message::<SimCommand>(app.world_mut());
    MessageRegistry::register_message::<SimReactiveEvent>(app.world_mut());

    // Build schedule with message rotation + applicator + RNG distribution
    let mut schedule = configure_sim_schedule(executor);
    schedule.add_systems(bevy_ecs::message::message_update_system.in_set(SimPhase::PreUpdate));
    schedule.add_systems(distribute_rng.in_set(SimPhase::PreUpdate));
    schedule.add_systems(apply_sim_commands.in_set(SimPhase::PostUpdate));
    app.add_schedule(schedule);
    app
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    use bevy_ecs::schedule::IntoScheduleConfigs;
    use bevy_ecs::system::Res;

    use super::*;
    use crate::ecs::conditions::{hourly, monthly, yearly};
    use crate::ecs::schedule::{SimPhase, SimTick};
    use crate::ecs::time::{MINUTES_PER_HOUR, MINUTES_PER_MONTH, MINUTES_PER_YEAR};

    #[test]
    fn app_builds_without_panic() {
        let _app = build_sim_app(100);
    }

    #[test]
    fn clock_starts_at_given_year() {
        let app = build_sim_app(100);
        let clock = app.world().resource::<SimClock>();
        assert_eq!(clock.time.year(), 100);
        assert_eq!(clock.time.month(), 1);
        assert_eq!(clock.time.minute(), 0);
    }

    #[test]
    fn single_tick_advances_one_minute() {
        let mut app = build_sim_app(100);
        app.world_mut().run_schedule(SimTick);
        let clock = app.world().resource::<SimClock>();
        assert_eq!(clock.time.year(), 100);
        assert_eq!(clock.time.minute(), 1);
    }

    #[test]
    fn sixty_ticks_advance_one_hour() {
        let mut app = build_sim_app(100);
        for _ in 0..MINUTES_PER_HOUR {
            app.world_mut().run_schedule(SimTick);
        }
        let clock = app.world().resource::<SimClock>();
        assert_eq!(clock.time.year(), 100);
        assert_eq!(clock.time.hour(), 1);
        assert_eq!(clock.time.minute(), 0);
    }

    #[test]
    fn yearly_system_fires_once_per_year() {
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let mut app = build_sim_app(100);
        app.add_systems(
            SimTick,
            (move |_clock: Res<SimClock>| {
                counter_clone.fetch_add(1, Ordering::Relaxed);
            })
            .run_if(yearly)
            .in_set(SimPhase::Update),
        );

        for _ in 0..MINUTES_PER_YEAR {
            app.world_mut().run_schedule(SimTick);
        }
        // Yearly fires at tick 0 (Y100 start), then not again until Y101
        // which is tick MINUTES_PER_YEAR. So 1 fire in MINUTES_PER_YEAR ticks.
        assert_eq!(counter.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn hourly_system_fires_once_per_60_ticks() {
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let mut app = build_sim_app(100);
        app.add_systems(
            SimTick,
            (move |_clock: Res<SimClock>| {
                counter_clone.fetch_add(1, Ordering::Relaxed);
            })
            .run_if(hourly)
            .in_set(SimPhase::Update),
        );

        // Run 120 ticks (2 hours)
        for _ in 0..(MINUTES_PER_HOUR * 2) {
            app.world_mut().run_schedule(SimTick);
        }
        // Fires at minute 0 (start) and minute 60 → 2 fires
        assert_eq!(counter.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn monthly_system_fires_twelve_per_year() {
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let mut app = build_sim_app(100);
        app.add_systems(
            SimTick,
            (move |_clock: Res<SimClock>| {
                counter_clone.fetch_add(1, Ordering::Relaxed);
            })
            .run_if(monthly)
            .in_set(SimPhase::Update),
        );

        for _ in 0..MINUTES_PER_YEAR {
            app.world_mut().run_schedule(SimTick);
        }
        // Monthly fires at each month start: 12 times per year
        assert_eq!(counter.load(Ordering::Relaxed), 12);
    }

    #[test]
    fn one_year_of_ticks() {
        let mut app = build_sim_app(100);
        for _ in 0..MINUTES_PER_YEAR {
            app.world_mut().run_schedule(SimTick);
        }
        let clock = app.world().resource::<SimClock>();
        assert_eq!(clock.time.year(), 101);
        assert_eq!(clock.time.month(), 1);
        assert_eq!(clock.tick_count, MINUTES_PER_YEAR as u64);
    }

    #[test]
    fn one_month_of_ticks() {
        let mut app = build_sim_app(100);
        for _ in 0..MINUTES_PER_MONTH {
            app.world_mut().run_schedule(SimTick);
        }
        let clock = app.world().resource::<SimClock>();
        assert_eq!(clock.time.year(), 100);
        assert_eq!(clock.time.month(), 2);
    }

    #[test]
    fn phase_ordering_respected() {
        let log = Arc::new(std::sync::Mutex::new(Vec::<&'static str>::new()));

        let log1 = log.clone();
        let log2 = log.clone();
        let log3 = log.clone();
        let log4 = log.clone();

        let mut app = build_sim_app(100);
        app.add_systems(
            SimTick,
            (move || {
                log1.lock().unwrap().push("pre_update");
            })
            .in_set(SimPhase::PreUpdate),
        );
        app.add_systems(
            SimTick,
            (move || {
                log2.lock().unwrap().push("update");
            })
            .in_set(SimPhase::Update),
        );
        app.add_systems(
            SimTick,
            (move || {
                log3.lock().unwrap().push("post_update");
            })
            .in_set(SimPhase::PostUpdate),
        );
        app.add_systems(
            SimTick,
            (move || {
                log4.lock().unwrap().push("last");
            })
            .in_set(SimPhase::Last),
        );

        app.world_mut().run_schedule(SimTick);

        let entries = log.lock().unwrap();
        let pre_idx = entries.iter().position(|&s| s == "pre_update").unwrap();
        let update_idx = entries.iter().position(|&s| s == "update").unwrap();
        let post_idx = entries.iter().position(|&s| s == "post_update").unwrap();
        let last_idx = entries.iter().position(|&s| s == "last").unwrap();
        assert!(pre_idx < update_idx);
        assert!(update_idx < post_idx);
        assert!(post_idx < last_idx);
    }
}
