use rand::RngCore;

use crate::model::World;

/// Context passed to each system on every tick.
///
/// Bundled so we can add fields later (config, logger) without changing
/// the `SimSystem` trait signature.
pub struct TickContext<'a> {
    pub world: &'a mut World,
    pub rng: &'a mut dyn RngCore,
}
