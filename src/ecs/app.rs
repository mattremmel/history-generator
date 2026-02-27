use bevy_app::App;

use super::clock::SimClock;
use super::schedule::configure_sim_schedule;

/// Build a headless Bevy app with simulation clock and tick schedule.
///
/// Manual tick control:
/// ```no_run
/// # use history_gen::ecs::{build_sim_app, SimTick};
/// let mut app = build_sim_app(100);
/// for _ in 0..120 {  // 10 years x 12 months
///     app.world_mut().run_schedule(SimTick);
/// }
/// ```
pub fn build_sim_app(start_year: u32) -> App {
    let mut app = App::empty();
    app.insert_resource(SimClock::new(start_year));
    app.add_schedule(configure_sim_schedule());
    app
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    use bevy_ecs::schedule::IntoScheduleConfigs;
    use bevy_ecs::system::Res;

    use super::*;
    use crate::ecs::conditions::{monthly, yearly};
    use crate::ecs::schedule::{SimPhase, SimTick};

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
    }

    #[test]
    fn single_tick_advances_clock() {
        let mut app = build_sim_app(100);
        // Before tick: Y100.M1
        app.world_mut().run_schedule(SimTick);
        // After tick: advance_clock moved to Y100.M2
        let clock = app.world().resource::<SimClock>();
        assert_eq!(clock.time.year(), 100);
        assert_eq!(clock.time.month(), 2);
    }

    #[test]
    fn twelve_ticks_advance_one_year() {
        let mut app = build_sim_app(100);
        for _ in 0..12 {
            app.world_mut().run_schedule(SimTick);
        }
        let clock = app.world().resource::<SimClock>();
        assert_eq!(clock.time.year(), 101);
        assert_eq!(clock.time.month(), 1);
    }

    #[test]
    fn yearly_system_fires_once_per_12_ticks() {
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

        for _ in 0..12 {
            app.world_mut().run_schedule(SimTick);
        }
        // Yearly fires at tick 1 (Y100.M1), then not again until Y101.M1
        // which would be tick 13. So 1 fire in 12 ticks.
        assert_eq!(counter.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn monthly_system_fires_every_tick() {
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

        for _ in 0..12 {
            app.world_mut().run_schedule(SimTick);
        }
        // Monthly fires every tick since the clock always points to a month start.
        assert_eq!(counter.load(Ordering::Relaxed), 12);
    }

    #[test]
    fn uncapped_loop_runs_1000_years() {
        let mut app = build_sim_app(100);
        for _ in 0..12_000 {
            app.world_mut().run_schedule(SimTick);
        }
        let clock = app.world().resource::<SimClock>();
        assert_eq!(clock.time.year(), 1100);
        assert_eq!(clock.time.month(), 1);
        assert_eq!(clock.tick_count, 12_000);
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
