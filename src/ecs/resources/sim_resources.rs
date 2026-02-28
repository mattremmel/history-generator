use std::hash::{DefaultHasher, Hash, Hasher};

use bevy_ecs::resource::Resource;
use bevy_ecs::world::World;
use rand::SeedableRng;
use rand::rngs::SmallRng;

use crate::IdGenerator;
use crate::model::action::{Action, ActionResult};

/// Simulation configuration (start year, duration, output settings).
#[derive(Resource, Debug, Clone)]
pub struct EcsSimConfig {
    pub start_year: u32,
    pub num_years: u32,
    pub seed: u64,
    pub flush_interval: u32,
    pub output_dir: String,
}

impl Default for EcsSimConfig {
    fn default() -> Self {
        Self {
            start_year: 0,
            num_years: 1000,
            seed: 42,
            flush_interval: 50,
            output_dir: "output".to_string(),
        }
    }
}

/// Deterministic RNG for the simulation.
#[derive(Resource)]
pub struct SimRng {
    pub rng: SmallRng,
    pub seed: u64,
}

// ---------------------------------------------------------------------------
// Per-domain RNG resources
// ---------------------------------------------------------------------------

macro_rules! domain_rng {
    ($name:ident, $doc:expr) => {
        #[doc = $doc]
        #[derive(Resource)]
        pub struct $name(pub SmallRng);

        impl Default for $name {
            fn default() -> Self {
                Self(SmallRng::seed_from_u64(0))
            }
        }
    };
}

domain_rng!(EnvironmentRng, "Per-domain RNG for Environment systems.");
domain_rng!(BuildingsRng, "Per-domain RNG for Buildings systems.");
domain_rng!(DemographicsRng, "Per-domain RNG for Demographics systems.");
domain_rng!(EconomyRng, "Per-domain RNG for Economy systems.");
domain_rng!(EducationRng, "Per-domain RNG for Education systems.");
domain_rng!(DiseaseRng, "Per-domain RNG for Disease systems.");
domain_rng!(CultureRng, "Per-domain RNG for Culture systems.");
domain_rng!(ReligionRng, "Per-domain RNG for Religion systems.");
domain_rng!(CrimeRng, "Per-domain RNG for Crime systems.");
domain_rng!(ReputationRng, "Per-domain RNG for Reputation systems.");
domain_rng!(KnowledgeRng, "Per-domain RNG for Knowledge systems.");
domain_rng!(ItemsRng, "Per-domain RNG for Items systems.");
domain_rng!(MigrationRng, "Per-domain RNG for Migration systems.");
domain_rng!(PoliticsRng, "Per-domain RNG for Politics systems.");
domain_rng!(ConflictsRng, "Per-domain RNG for Conflicts systems.");
domain_rng!(AgencyRng, "Per-domain RNG for Agency systems.");
domain_rng!(ActionsRng, "Per-domain RNG for Actions systems.");

/// Derive a deterministic per-domain seed from the global seed, domain name, and tick count.
fn derive_domain_seed(seed: u64, domain: &str, tick: u64) -> u64 {
    let mut hasher = DefaultHasher::new();
    seed.hash(&mut hasher);
    domain.hash(&mut hasher);
    tick.hash(&mut hasher);
    hasher.finish()
}

/// Exclusive system that re-seeds all per-domain RNGs each tick.
/// Runs in `SimPhase::PreUpdate` before any domain systems.
pub fn distribute_rng(world: &mut World) {
    let seed = world.resource::<SimRng>().seed;
    let tick = world.resource::<crate::ecs::clock::SimClock>().tick_count;

    macro_rules! reseed {
        ($res:ty, $label:expr) => {
            world.resource_mut::<$res>().0 = SmallRng::seed_from_u64(derive_domain_seed(seed, $label, tick));
        };
    }

    reseed!(EnvironmentRng, "environment");
    reseed!(BuildingsRng, "buildings");
    reseed!(DemographicsRng, "demographics");
    reseed!(EconomyRng, "economy");
    reseed!(EducationRng, "education");
    reseed!(DiseaseRng, "disease");
    reseed!(CultureRng, "culture");
    reseed!(ReligionRng, "religion");
    reseed!(CrimeRng, "crime");
    reseed!(ReputationRng, "reputation");
    reseed!(KnowledgeRng, "knowledge");
    reseed!(ItemsRng, "items");
    reseed!(MigrationRng, "migration");
    reseed!(PoliticsRng, "politics");
    reseed!(ConflictsRng, "conflicts");
    reseed!(AgencyRng, "agency");
    reseed!(ActionsRng, "actions");
}

/// Global ID generator for simulation entities.
#[derive(Resource, Default)]
pub struct EcsIdGenerator(pub IdGenerator);

/// Actions queued for processing this tick.
#[derive(Resource, Debug, Clone, Default)]
pub struct PendingActions(pub Vec<Action>);

/// Results from processed actions.
#[derive(Resource, Debug, Clone, Default)]
pub struct ActionResults(pub Vec<ActionResult>);

/// Captures reactive events from the current tick for Agency to consume next tick.
/// Mirrors the old `AgencySystem.recent_signals` pattern.
#[derive(Resource, Debug, Clone, Default)]
pub struct AgencyMemory(pub Vec<crate::ecs::events::SimReactiveEvent>);
