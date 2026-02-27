use bevy_app::App;

use crate::ecs::clock::SimClock;
use crate::ecs::schedule::SimTick;
use crate::ecs::time::{MINUTES_PER_MONTH, MINUTES_PER_YEAR};

/// Fast-forward the clock to the next yearly boundary, then run that many ticks.
/// Each call runs enough ticks to span `n` full years.
pub fn tick_years(app: &mut App, n: u32) {
    let total_minutes = n * MINUTES_PER_YEAR;
    for _ in 0..total_minutes {
        app.world_mut().run_schedule(SimTick);
    }
}

/// Fast-forward the clock by `n` months worth of minute-ticks.
pub fn tick_months(app: &mut App, n: u32) {
    let total_minutes = n * MINUTES_PER_MONTH;
    for _ in 0..total_minutes {
        app.world_mut().run_schedule(SimTick);
    }
}

/// Return the current simulation year from the clock resource.
pub fn current_year(app: &App) -> u32 {
    app.world().resource::<SimClock>().time.year()
}
