use bevy_ecs::schedule::{ExecutorKind, IntoScheduleConfigs, Schedule, ScheduleLabel, SystemSet};

use super::clock::advance_clock;

/// Schedule label for the main simulation tick.
/// Run manually each tick via `app.world_mut().run_schedule(SimTick)`.
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct SimTick;

/// Ordered phases within each simulation tick.
///
/// Systems are assigned to phases via `.in_set(SimPhase::Update)` etc.
/// Phases run in declaration order: PreUpdate < Update < PostUpdate < Reactions < Last.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum SimPhase {
    PreUpdate,
    Update,
    PostUpdate,
    Reactions,
    Last,
}

/// Build a configured `SimTick` schedule with phase ordering and single-threaded execution.
pub fn configure_sim_schedule() -> Schedule {
    let mut schedule = Schedule::new(SimTick);
    schedule.set_executor_kind(ExecutorKind::SingleThreaded);
    schedule.configure_sets(
        (
            SimPhase::PreUpdate,
            SimPhase::Update,
            SimPhase::PostUpdate,
            SimPhase::Reactions,
            SimPhase::Last,
        )
            .chain(),
    );
    schedule.add_systems(advance_clock.in_set(SimPhase::Last));
    schedule
}
