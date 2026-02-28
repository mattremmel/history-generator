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

/// Per-domain system sets within `SimPhase::Update`.
///
/// Cross-domain ordering:
/// ```text
/// Environment → Buildings → [Demographics, Economy, Education, Disease]
///                          → [Culture, Religion, Crime, Reputation]
///                          → [Knowledge, Items, Migration]
///                          → [Politics, Conflicts]
///                          → Agency → Actions
/// ```
///
/// Systems in the same bracket have no ordering constraint between them —
/// Bevy schedules them based on data access (parallel if disjoint, serialized if conflicting).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum DomainSet {
    Environment,
    Buildings,
    Demographics,
    Economy,
    Education,
    Disease,
    Culture,
    Religion,
    Crime,
    Reputation,
    Knowledge,
    Items,
    Migration,
    Politics,
    Conflicts,
    Agency,
    Actions,
}

/// Configure cross-domain ordering within `SimPhase::Update`.
fn configure_domain_ordering(schedule: &mut Schedule) {
    // All DomainSets live inside SimPhase::Update
    schedule.configure_sets(DomainSet::Environment.in_set(SimPhase::Update));
    schedule.configure_sets(DomainSet::Buildings.in_set(SimPhase::Update));
    schedule.configure_sets(DomainSet::Demographics.in_set(SimPhase::Update));
    schedule.configure_sets(DomainSet::Economy.in_set(SimPhase::Update));
    schedule.configure_sets(DomainSet::Education.in_set(SimPhase::Update));
    schedule.configure_sets(DomainSet::Disease.in_set(SimPhase::Update));
    schedule.configure_sets(DomainSet::Culture.in_set(SimPhase::Update));
    schedule.configure_sets(DomainSet::Religion.in_set(SimPhase::Update));
    schedule.configure_sets(DomainSet::Crime.in_set(SimPhase::Update));
    schedule.configure_sets(DomainSet::Reputation.in_set(SimPhase::Update));
    schedule.configure_sets(DomainSet::Knowledge.in_set(SimPhase::Update));
    schedule.configure_sets(DomainSet::Items.in_set(SimPhase::Update));
    schedule.configure_sets(DomainSet::Migration.in_set(SimPhase::Update));
    schedule.configure_sets(DomainSet::Politics.in_set(SimPhase::Update));
    schedule.configure_sets(DomainSet::Conflicts.in_set(SimPhase::Update));
    schedule.configure_sets(DomainSet::Agency.in_set(SimPhase::Update));
    schedule.configure_sets(DomainSet::Actions.in_set(SimPhase::Update));

    // Environment → Buildings (first two always run in order)
    schedule.configure_sets(DomainSet::Buildings.after(DomainSet::Environment));

    // Buildings → middle tier (all unordered relative to each other)
    schedule.configure_sets(DomainSet::Demographics.after(DomainSet::Buildings));
    schedule.configure_sets(DomainSet::Economy.after(DomainSet::Buildings));
    schedule.configure_sets(DomainSet::Education.after(DomainSet::Buildings));
    schedule.configure_sets(DomainSet::Disease.after(DomainSet::Buildings));
    schedule.configure_sets(DomainSet::Culture.after(DomainSet::Buildings));
    schedule.configure_sets(DomainSet::Religion.after(DomainSet::Buildings));
    schedule.configure_sets(DomainSet::Crime.after(DomainSet::Buildings));
    schedule.configure_sets(DomainSet::Reputation.after(DomainSet::Buildings));
    schedule.configure_sets(DomainSet::Knowledge.after(DomainSet::Buildings));
    schedule.configure_sets(DomainSet::Items.after(DomainSet::Buildings));
    schedule.configure_sets(DomainSet::Migration.after(DomainSet::Buildings));
    schedule.configure_sets(DomainSet::Politics.after(DomainSet::Buildings));
    schedule.configure_sets(DomainSet::Conflicts.after(DomainSet::Buildings));

    // Middle tier → Agency (Agency must see all domain state changes)
    schedule.configure_sets(DomainSet::Agency.after(DomainSet::Demographics));
    schedule.configure_sets(DomainSet::Agency.after(DomainSet::Economy));
    schedule.configure_sets(DomainSet::Agency.after(DomainSet::Education));
    schedule.configure_sets(DomainSet::Agency.after(DomainSet::Disease));
    schedule.configure_sets(DomainSet::Agency.after(DomainSet::Culture));
    schedule.configure_sets(DomainSet::Agency.after(DomainSet::Religion));
    schedule.configure_sets(DomainSet::Agency.after(DomainSet::Crime));
    schedule.configure_sets(DomainSet::Agency.after(DomainSet::Reputation));
    schedule.configure_sets(DomainSet::Agency.after(DomainSet::Knowledge));
    schedule.configure_sets(DomainSet::Agency.after(DomainSet::Items));
    schedule.configure_sets(DomainSet::Agency.after(DomainSet::Migration));
    schedule.configure_sets(DomainSet::Agency.after(DomainSet::Politics));
    schedule.configure_sets(DomainSet::Agency.after(DomainSet::Conflicts));

    // Agency → Actions
    schedule.configure_sets(DomainSet::Actions.after(DomainSet::Agency));
}

/// Build a configured `SimTick` schedule with phase ordering.
pub fn configure_sim_schedule(executor: ExecutorKind) -> Schedule {
    let mut schedule = Schedule::new(SimTick);
    schedule.set_executor_kind(executor);
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
    configure_domain_ordering(&mut schedule);
    schedule.add_systems(advance_clock.in_set(SimPhase::Last));
    schedule
}
