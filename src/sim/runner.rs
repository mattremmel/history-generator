use std::path::PathBuf;

use rand::rngs::SmallRng;
use rand::{RngCore, SeedableRng};

use super::context::TickContext;
use super::system::{SimSystem, TickFrequency};
use crate::flush::flush_to_jsonl;
use crate::model::timestamp::{DAYS_PER_MONTH, DAYS_PER_YEAR, HOURS_PER_DAY, MONTHS_PER_YEAR};
use crate::model::{SimTimestamp, World};

/// Configuration for a simulation run.
pub struct SimConfig {
    pub start_year: u32,
    pub num_years: u32,
    pub seed: u64,
    /// If set, flush world state every N years.
    pub flush_interval: Option<u32>,
    /// Directory to write flush checkpoints into.
    pub output_dir: Option<PathBuf>,
}

impl SimConfig {
    pub fn new(start_year: u32, num_years: u32, seed: u64) -> Self {
        Self {
            start_year,
            num_years,
            seed,
            flush_interval: None,
            output_dir: None,
        }
    }
}

/// Returns true if a system with the given frequency should fire at this timestamp.
pub fn should_fire(freq: TickFrequency, time: SimTimestamp) -> bool {
    match freq {
        TickFrequency::Hourly => true,
        TickFrequency::Daily => time.hour() == 0,
        TickFrequency::Monthly => time.hour() == 0 && time.day_of_month() == 1,
        TickFrequency::Yearly => time.hour() == 0 && time.day() == 1,
    }
}

/// Set `world.current_time` and call each system whose frequency matches.
///
/// Signal delivery is **single-pass, non-cascading**:
///
/// 1. **Phase 1 (tick):** Each system's `tick()` runs in registration order.
///    All signals emitted during this phase are collected into a shared buffer.
/// 2. **Phase 2 (react):** If any signals were emitted, each system's
///    `handle_signals()` is called with the full signal buffer as `ctx.inbox`.
///    Systems may mutate the world and push new signals during this phase,
///    but those new signals are **not** delivered — they are discarded at the
///    end of the dispatch cycle.
///
/// This means a signal emitted in Phase 2 will never trigger further reactions
/// within the same tick. This is intentional: it prevents infinite cascades and
/// keeps each tick's side-effects bounded. If a reaction needs to propagate,
/// it should mutate world state that a later tick's Phase 1 will observe.
pub fn dispatch_systems(
    world: &mut World,
    systems: &mut [Box<dyn SimSystem>],
    rng: &mut dyn RngCore,
    time: SimTimestamp,
) {
    world.current_time = time;

    // Phase 1: tick systems, collecting signals
    let mut signals = Vec::new();
    for system in systems.iter_mut() {
        if should_fire(system.frequency(), time) {
            let mut ctx = TickContext {
                world,
                rng,
                signals: &mut signals,
                inbox: &[],
            };
            system.tick(&mut ctx);
        }
    }

    // Phase 2: deliver signals for reaction (only if any were emitted)
    if !signals.is_empty() {
        for system in systems.iter_mut() {
            if should_fire(system.frequency(), time) {
                let mut new_signals = Vec::new();
                let mut ctx = TickContext {
                    world,
                    rng,
                    signals: &mut new_signals,
                    inbox: &signals,
                };
                system.handle_signals(&mut ctx);
            }
        }
    }
}

/// Run the simulation for the configured number of years.
///
/// Creates a deterministic RNG from `config.seed`, so the same seed always
/// produces the same simulation. The loop iterates at the finest granularity
/// needed by any registered system, avoiding wasted cycles when all systems
/// are coarse.
pub fn run(world: &mut World, systems: &mut [Box<dyn SimSystem>], config: SimConfig) {
    if systems.is_empty() || config.num_years == 0 {
        return;
    }

    let mut rng = SmallRng::seed_from_u64(config.seed);
    let finest = systems.iter().map(|s| s.frequency()).max().unwrap();

    for year_offset in 0..config.num_years {
        let year = config.start_year + year_offset;
        match finest {
            TickFrequency::Yearly => {
                dispatch_systems(world, systems, &mut rng, SimTimestamp::new(year, 1, 0));
            }
            TickFrequency::Monthly => {
                for month in 0..MONTHS_PER_YEAR {
                    let day = month * DAYS_PER_MONTH + 1;
                    dispatch_systems(world, systems, &mut rng, SimTimestamp::new(year, day, 0));
                }
            }
            TickFrequency::Daily => {
                for day in 1..=DAYS_PER_YEAR {
                    dispatch_systems(world, systems, &mut rng, SimTimestamp::new(year, day, 0));
                }
            }
            TickFrequency::Hourly => {
                for day in 1..=DAYS_PER_YEAR {
                    for hour in 0..HOURS_PER_DAY {
                        dispatch_systems(
                            world,
                            systems,
                            &mut rng,
                            SimTimestamp::new(year, day, hour),
                        );
                    }
                }
            }
        }

        // Flush checkpoint at configured interval
        if let (Some(interval), Some(dir)) = (config.flush_interval, &config.output_dir) {
            let is_last_year = year_offset == config.num_years - 1;
            if is_last_year || (year_offset > 0 && (year_offset + 1) % interval == 0) {
                let checkpoint_dir = dir.join(format!("year_{year:06}"));
                flush_to_jsonl(world, &checkpoint_dir).expect("failed to write flush checkpoint");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use super::*;
    use crate::model::{EntityKind, EventKind};

    // -- Test helpers --

    struct CountingSystem {
        sys_name: String,
        freq: TickFrequency,
        count: Rc<Cell<u32>>,
    }

    impl CountingSystem {
        fn new(name: &str, freq: TickFrequency, count: Rc<Cell<u32>>) -> Self {
            Self {
                sys_name: name.to_string(),
                freq,
                count,
            }
        }
    }

    impl SimSystem for CountingSystem {
        fn name(&self) -> &str {
            &self.sys_name
        }
        fn frequency(&self) -> TickFrequency {
            self.freq
        }
        fn tick(&mut self, _ctx: &mut TickContext) {
            self.count.set(self.count.get() + 1);
        }
    }

    // -- should_fire tests --

    #[test]
    fn should_fire_yearly_only_at_year_start() {
        assert!(should_fire(
            TickFrequency::Yearly,
            SimTimestamp::new(1, 1, 0)
        ));
        assert!(!should_fire(
            TickFrequency::Yearly,
            SimTimestamp::new(1, 1, 5)
        ));
        assert!(!should_fire(
            TickFrequency::Yearly,
            SimTimestamp::new(1, 2, 0)
        ));
        assert!(!should_fire(
            TickFrequency::Yearly,
            SimTimestamp::new(1, 31, 0)
        ));
        assert!(!should_fire(
            TickFrequency::Yearly,
            SimTimestamp::new(1, 180, 12)
        ));
    }

    #[test]
    fn should_fire_monthly_at_month_starts() {
        // All 12 month-start days should fire
        let month_starts: Vec<u32> = (0..MONTHS_PER_YEAR)
            .map(|m| m * DAYS_PER_MONTH + 1)
            .collect();
        assert_eq!(
            month_starts,
            vec![1, 31, 61, 91, 121, 151, 181, 211, 241, 271, 301, 331]
        );

        for &day in &month_starts {
            assert!(
                should_fire(TickFrequency::Monthly, SimTimestamp::new(1, day, 0)),
                "expected monthly fire at day {day}"
            );
            // Non-zero hour should not fire
            assert!(
                !should_fire(TickFrequency::Monthly, SimTimestamp::new(1, day, 5)),
                "expected no monthly fire at day {day} hour 5"
            );
        }
        // Mid-month should not fire
        assert!(!should_fire(
            TickFrequency::Monthly,
            SimTimestamp::new(1, 15, 0)
        ));
    }

    #[test]
    fn should_fire_daily_at_hour_zero() {
        assert!(should_fire(
            TickFrequency::Daily,
            SimTimestamp::new(1, 1, 0)
        ));
        assert!(should_fire(
            TickFrequency::Daily,
            SimTimestamp::new(1, 180, 0)
        ));
        assert!(should_fire(
            TickFrequency::Daily,
            SimTimestamp::new(1, 360, 0)
        ));
        assert!(!should_fire(
            TickFrequency::Daily,
            SimTimestamp::new(1, 1, 1)
        ));
        assert!(!should_fire(
            TickFrequency::Daily,
            SimTimestamp::new(1, 1, 23)
        ));
    }

    #[test]
    fn should_fire_hourly_always() {
        assert!(should_fire(
            TickFrequency::Hourly,
            SimTimestamp::new(1, 1, 0)
        ));
        assert!(should_fire(
            TickFrequency::Hourly,
            SimTimestamp::new(1, 180, 12)
        ));
        assert!(should_fire(
            TickFrequency::Hourly,
            SimTimestamp::new(1, 360, 23)
        ));
    }

    // -- run() tests --

    #[test]
    fn empty_systems_noop() {
        let mut world = World::new();
        let original_time = world.current_time;
        let mut systems: Vec<Box<dyn SimSystem>> = vec![];
        run(&mut world, &mut systems, SimConfig::new(0, 10, 0));
        assert_eq!(world.current_time, original_time);
        assert!(world.entities.is_empty());
    }

    #[test]
    fn zero_years_noop() {
        let count = Rc::new(Cell::new(0));
        let mut systems: Vec<Box<dyn SimSystem>> = vec![Box::new(CountingSystem::new(
            "test",
            TickFrequency::Yearly,
            count.clone(),
        ))];
        let mut world = World::new();
        run(&mut world, &mut systems, SimConfig::new(0, 0, 0));
        assert_eq!(count.get(), 0);
    }

    #[test]
    fn yearly_system_ticked_per_year() {
        let count = Rc::new(Cell::new(0));
        let mut systems: Vec<Box<dyn SimSystem>> = vec![Box::new(CountingSystem::new(
            "yearly",
            TickFrequency::Yearly,
            count.clone(),
        ))];
        let mut world = World::new();
        run(&mut world, &mut systems, SimConfig::new(0, 10, 0));
        assert_eq!(count.get(), 10);
    }

    #[test]
    fn monthly_system_ticked_twelve_per_year() {
        let count = Rc::new(Cell::new(0));
        let mut systems: Vec<Box<dyn SimSystem>> = vec![Box::new(CountingSystem::new(
            "monthly",
            TickFrequency::Monthly,
            count.clone(),
        ))];
        let mut world = World::new();
        run(&mut world, &mut systems, SimConfig::new(0, 1, 0));
        assert_eq!(count.get(), 12);
    }

    #[test]
    fn daily_system_ticked_360_per_year() {
        let count = Rc::new(Cell::new(0));
        let mut systems: Vec<Box<dyn SimSystem>> = vec![Box::new(CountingSystem::new(
            "daily",
            TickFrequency::Daily,
            count.clone(),
        ))];
        let mut world = World::new();
        run(&mut world, &mut systems, SimConfig::new(0, 1, 0));
        assert_eq!(count.get(), 360);
    }

    #[test]
    fn hourly_system_ticked_8640_per_year() {
        let count = Rc::new(Cell::new(0));
        let mut systems: Vec<Box<dyn SimSystem>> = vec![Box::new(CountingSystem::new(
            "hourly",
            TickFrequency::Hourly,
            count.clone(),
        ))];
        let mut world = World::new();
        run(&mut world, &mut systems, SimConfig::new(0, 1, 0));
        assert_eq!(count.get(), 8640);
    }

    #[test]
    fn mixed_yearly_and_daily() {
        let yearly_count = Rc::new(Cell::new(0));
        let daily_count = Rc::new(Cell::new(0));
        let mut systems: Vec<Box<dyn SimSystem>> = vec![
            Box::new(CountingSystem::new(
                "yearly",
                TickFrequency::Yearly,
                yearly_count.clone(),
            )),
            Box::new(CountingSystem::new(
                "daily",
                TickFrequency::Daily,
                daily_count.clone(),
            )),
        ];
        let mut world = World::new();
        run(&mut world, &mut systems, SimConfig::new(0, 2, 0));
        assert_eq!(yearly_count.get(), 2);
        assert_eq!(daily_count.get(), 720);
    }

    #[test]
    fn mixed_monthly_and_daily() {
        let monthly_count = Rc::new(Cell::new(0));
        let daily_count = Rc::new(Cell::new(0));
        let mut systems: Vec<Box<dyn SimSystem>> = vec![
            Box::new(CountingSystem::new(
                "monthly",
                TickFrequency::Monthly,
                monthly_count.clone(),
            )),
            Box::new(CountingSystem::new(
                "daily",
                TickFrequency::Daily,
                daily_count.clone(),
            )),
        ];
        let mut world = World::new();
        run(&mut world, &mut systems, SimConfig::new(0, 1, 0));
        assert_eq!(monthly_count.get(), 12);
        assert_eq!(daily_count.get(), 360);
    }

    #[test]
    fn world_time_set_to_final_tick() {
        let count = Rc::new(Cell::new(0));
        let mut systems: Vec<Box<dyn SimSystem>> = vec![Box::new(CountingSystem::new(
            "daily",
            TickFrequency::Daily,
            count.clone(),
        ))];
        let mut world = World::new();
        run(&mut world, &mut systems, SimConfig::new(5, 3, 0));
        // Last tick: year 7, day 360, hour 0
        assert_eq!(world.current_time, SimTimestamp::new(7, 360, 0));
    }

    #[test]
    fn system_can_mutate_world() {
        struct EntityCreatingSystem;

        impl SimSystem for EntityCreatingSystem {
            fn name(&self) -> &str {
                "entity_creator"
            }
            fn frequency(&self) -> TickFrequency {
                TickFrequency::Yearly
            }
            fn tick(&mut self, ctx: &mut TickContext) {
                let time = ctx.world.current_time;
                let ev = ctx
                    .world
                    .add_event(EventKind::Birth, time, "Test birth".to_string());
                ctx.world.add_entity(
                    EntityKind::Person,
                    "Test".to_string(),
                    Some(time),
                    crate::model::entity_data::EntityData::default_for_kind(EntityKind::Person),
                    ev,
                );
            }
        }

        let mut systems: Vec<Box<dyn SimSystem>> = vec![Box::new(EntityCreatingSystem)];
        let mut world = World::new();
        run(&mut world, &mut systems, SimConfig::new(0, 5, 0));
        assert_eq!(world.entities.len(), 5);
        assert_eq!(world.events.len(), 5);
    }

    #[test]
    fn systems_called_in_registration_order() {
        struct LoggingSystem {
            sys_name: String,
            freq: TickFrequency,
            log: Rc<RefCell<Vec<String>>>,
        }

        impl SimSystem for LoggingSystem {
            fn name(&self) -> &str {
                &self.sys_name
            }
            fn frequency(&self) -> TickFrequency {
                self.freq
            }
            fn tick(&mut self, _ctx: &mut TickContext) {
                self.log.borrow_mut().push(self.sys_name.clone());
            }
        }

        let log = Rc::new(RefCell::new(Vec::new()));
        let mut systems: Vec<Box<dyn SimSystem>> = vec![
            Box::new(LoggingSystem {
                sys_name: "A".to_string(),
                freq: TickFrequency::Yearly,
                log: log.clone(),
            }),
            Box::new(LoggingSystem {
                sys_name: "B".to_string(),
                freq: TickFrequency::Yearly,
                log: log.clone(),
            }),
        ];
        let mut world = World::new();
        run(&mut world, &mut systems, SimConfig::new(0, 2, 0));
        assert_eq!(*log.borrow(), vec!["A", "B", "A", "B"]);
    }

    // -- Signal bus tests --

    #[test]
    fn signal_emitted_and_received() {
        use crate::sim::signal::{Signal, SignalKind};

        struct EmitterSystem {
            emitted: Rc<Cell<u32>>,
        }

        impl SimSystem for EmitterSystem {
            fn name(&self) -> &str {
                "emitter"
            }
            fn frequency(&self) -> TickFrequency {
                TickFrequency::Yearly
            }
            fn tick(&mut self, ctx: &mut TickContext) {
                self.emitted.set(self.emitted.get() + 1);
                ctx.signals.push(Signal {
                    event_id: 0,
                    kind: SignalKind::EntityDied { entity_id: 42 },
                });
            }
        }

        struct ReceiverSystem {
            received: Rc<Cell<u32>>,
        }

        impl SimSystem for ReceiverSystem {
            fn name(&self) -> &str {
                "receiver"
            }
            fn frequency(&self) -> TickFrequency {
                TickFrequency::Yearly
            }
            fn tick(&mut self, _ctx: &mut TickContext) {}
            fn handle_signals(&mut self, ctx: &mut TickContext) {
                for signal in ctx.inbox {
                    if let SignalKind::EntityDied { entity_id: 42 } = signal.kind {
                        self.received.set(self.received.get() + 1);
                    }
                }
            }
        }

        let emitted = Rc::new(Cell::new(0));
        let received = Rc::new(Cell::new(0));
        let mut systems: Vec<Box<dyn SimSystem>> = vec![
            Box::new(EmitterSystem {
                emitted: emitted.clone(),
            }),
            Box::new(ReceiverSystem {
                received: received.clone(),
            }),
        ];
        let mut world = World::new();
        run(&mut world, &mut systems, SimConfig::new(0, 3, 0));
        assert_eq!(emitted.get(), 3);
        assert_eq!(received.get(), 3);
    }

    #[test]
    fn signals_not_accumulated_across_ticks() {
        use crate::sim::signal::{Signal, SignalKind};

        struct EmitterSystem;

        impl SimSystem for EmitterSystem {
            fn name(&self) -> &str {
                "emitter"
            }
            fn frequency(&self) -> TickFrequency {
                TickFrequency::Yearly
            }
            fn tick(&mut self, ctx: &mut TickContext) {
                ctx.signals.push(Signal {
                    event_id: 0,
                    kind: SignalKind::EntityDied { entity_id: 1 },
                });
            }
        }

        struct CounterSystem {
            max_inbox_len: Rc<Cell<usize>>,
        }

        impl SimSystem for CounterSystem {
            fn name(&self) -> &str {
                "counter"
            }
            fn frequency(&self) -> TickFrequency {
                TickFrequency::Yearly
            }
            fn tick(&mut self, _ctx: &mut TickContext) {}
            fn handle_signals(&mut self, ctx: &mut TickContext) {
                // Track the maximum inbox length across all ticks
                let len = ctx.inbox.len();
                if len > self.max_inbox_len.get() {
                    self.max_inbox_len.set(len);
                }
            }
        }

        let max_inbox_len = Rc::new(Cell::new(0));
        let mut systems: Vec<Box<dyn SimSystem>> = vec![
            Box::new(EmitterSystem),
            Box::new(CounterSystem {
                max_inbox_len: max_inbox_len.clone(),
            }),
        ];
        let mut world = World::new();
        run(&mut world, &mut systems, SimConfig::new(0, 5, 0));
        // Each tick should only see 1 signal (from that tick), not accumulated
        assert_eq!(max_inbox_len.get(), 1);
    }
}
