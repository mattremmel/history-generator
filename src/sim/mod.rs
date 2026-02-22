mod context;
pub mod demographics;
pub mod names;
pub mod population;
mod runner;
pub mod signal;
mod system;

pub use context::TickContext;
pub use demographics::DemographicsSystem;
pub use population::PopulationBreakdown;
pub use runner::{SimConfig, dispatch_systems, run, should_fire};
pub use signal::{Signal, SignalKind};
pub use system::{SimSystem, TickFrequency};
