use bevy_app::App;

use crate::ecs::clock::SimClock;
use crate::ecs::schedule::SimTick;
use crate::ecs::time::{MINUTES_PER_MONTH, SimTime};

/// Fast-forward by `n` months, running the schedule only at monthly boundaries.
///
/// All domain systems use `run_if(monthly)` or `run_if(yearly)`, so the
/// ~43,200 non-boundary ticks per month are pure overhead. This sets the
/// clock to each consecutive month-start, runs the schedule once (firing
/// all monthly/yearly systems + reactions + command applicator), then
/// positions the final clock at `start + n * MINUTES_PER_MONTH`.
pub fn tick_months(app: &mut App, n: u32) {
    let start_minutes = app.world().resource::<SimClock>().time.as_minutes();
    let base_month = start_minutes / MINUTES_PER_MONTH;

    for i in 0..n {
        let boundary = (base_month + i) * MINUTES_PER_MONTH;
        app.world_mut().resource_mut::<SimClock>().time = SimTime::from_minutes(boundary);
        app.world_mut().run_schedule(SimTick);
    }

    // Position clock at the end of the simulated period so callers see
    // the expected time (e.g. year 101 after ticking 12 months from year 100).
    let final_minutes = start_minutes + n * MINUTES_PER_MONTH;
    app.world_mut().resource_mut::<SimClock>().time = SimTime::from_minutes(final_minutes);
}

/// Fast-forward by `n` years (= `n * 12` monthly boundary ticks).
pub fn tick_years(app: &mut App, n: u32) {
    tick_months(app, n * 12);
}

/// Return the current simulation year from the clock resource.
pub fn current_year(app: &App) -> u32 {
    app.world().resource::<SimClock>().time.year()
}
