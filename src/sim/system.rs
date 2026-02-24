use super::context::TickContext;

/// How often a simulation system should tick.
///
/// Ordered coarsest-to-finest so `systems.iter().map(|s| s.frequency()).max()`
/// yields the finest granularity needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TickFrequency {
    Yearly,  // 1 tick/year
    Monthly, // 12 ticks/year
    Daily,   // 360 ticks/year
    Hourly,  // 8,640 ticks/year
}

/// A pluggable simulation system that runs each tick.
///
/// Object-safe so systems can be stored as `Box<dyn SimSystem>`.
pub trait SimSystem {
    fn name(&self) -> &str;
    fn frequency(&self) -> TickFrequency;
    fn tick(&mut self, ctx: &mut TickContext);

    /// React to signals emitted by other systems during Phase 1 (`tick()`).
    ///
    /// Called once per dispatch cycle with the full signal buffer in `ctx.inbox`.
    /// Signals pushed to `ctx.signals` here are **not** re-delivered (single-pass).
    /// Default: no-op.
    fn handle_signals(&mut self, ctx: &mut TickContext) {
        let _ = ctx;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frequency_ordering_coarsest_to_finest() {
        assert!(TickFrequency::Yearly < TickFrequency::Monthly);
        assert!(TickFrequency::Monthly < TickFrequency::Daily);
        assert!(TickFrequency::Daily < TickFrequency::Hourly);
    }

    #[test]
    fn max_yields_finest() {
        let freqs = [
            TickFrequency::Yearly,
            TickFrequency::Daily,
            TickFrequency::Monthly,
        ];
        assert_eq!(freqs.iter().max(), Some(&TickFrequency::Daily));
    }
}
