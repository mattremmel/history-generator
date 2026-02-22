use rand::RngCore;

use super::signal::Signal;
use crate::model::World;

/// Context passed to each system on every tick.
///
/// Bundled so we can add fields later (config, logger) without changing
/// the `SimSystem` trait signature.
pub struct TickContext<'a> {
    pub world: &'a mut World,
    pub rng: &'a mut dyn RngCore,
    /// Systems push signals here during tick/handle_signals.
    pub signals: &'a mut Vec<Signal>,
    /// Signals emitted by other systems in the previous pass (read-only).
    pub inbox: &'a [Signal],
}
