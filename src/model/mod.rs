#[macro_use]
pub mod macros;
pub mod action;
pub mod cultural_value;
pub mod effect;
pub mod entity;
pub mod entity_data;
pub mod event;
pub mod population;
pub mod relationship;
pub mod timestamp;
pub mod traits;
pub mod world;

pub use action::{Action, ActionKind, ActionOutcome, ActionResult, ActionSource};
pub use cultural_value::{CulturalValue, NamingStyle};
pub use effect::{EventEffect, StateChange};
pub use entity::{Entity, EntityKind};
pub use entity_data::{
    ArmyData, BuildingData, BuildingType, CultureData, EntityData, FactionData,
    GeographicFeatureData, KnowledgeCategory, KnowledgeData, ManifestationData, Medium, PersonData,
    RegionData, ResourceDepositData, RiverData, SettlementData, SiegeOutcome,
};
pub use population::PopulationBreakdown;
pub use event::{Event, EventKind, EventParticipant, ParticipantRole};
pub use relationship::{Relationship, RelationshipKind};
pub use timestamp::SimTimestamp;
pub use traits::Trait;
pub use world::World;
