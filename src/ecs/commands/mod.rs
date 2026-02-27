pub mod applicator;
mod apply_buildings;
mod apply_demographics;
mod apply_disease;
mod apply_economy;
mod apply_environment;
mod apply_lifecycle;
mod apply_military;
mod apply_relationship;
mod apply_set_field;

use bevy_ecs::entity::Entity;
use bevy_ecs::message::Message;

use crate::model::Sex;
use crate::model::entity_data::{BuildingType, DisasterType, FeatureType, Role};
use crate::model::event::{EventKind, ParticipantRole};
use crate::model::relationship::RelationshipKind;
use crate::model::traits::Trait;

pub use applicator::apply_sim_commands;

/// A command describing an intended state change in the simulation.
///
/// Systems emit these via `MessageWriter<SimCommand>`. The centralized applicator
/// in `SimPhase::PostUpdate` processes them: applies state changes, records audit
/// trail entries in `EventLog`, and emits `SimReactiveEvent` messages.
#[derive(Message, Clone, Debug)]
pub struct SimCommand {
    /// The intent — what state change to apply.
    pub kind: SimCommandKind,
    /// Human-readable description for the EventLog.
    pub description: String,
    /// Causal chain: event_id of the event that triggered this command.
    pub caused_by: Option<u64>,
    /// What EventKind to record in the EventLog (ignored for bookkeeping commands).
    pub event_kind: EventKind,
    /// Entities involved and their roles.
    pub participants: Vec<(Entity, ParticipantRole)>,
    /// Structured metadata for the Event.data field.
    pub event_data: serde_json::Value,
    /// If true, no Event entry is recorded (only effects).
    bookkeeping: bool,
}

impl SimCommand {
    /// Create a command that records a full Event in the log.
    pub fn new(
        kind: SimCommandKind,
        event_kind: EventKind,
        description: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            description: description.into(),
            caused_by: None,
            event_kind,
            participants: Vec::new(),
            event_data: serde_json::Value::Null,
            bookkeeping: false,
        }
    }

    /// Create a bookkeeping-only command (no Event entry, only effects).
    pub fn bookkeeping(kind: SimCommandKind) -> Self {
        Self {
            kind,
            description: String::new(),
            caused_by: None,
            // Unused for bookkeeping, but needs a value
            event_kind: EventKind::Custom("bookkeeping".to_string()),
            participants: Vec::new(),
            event_data: serde_json::Value::Null,
            bookkeeping: true,
        }
    }

    /// Whether this command is bookkeeping-only (no Event entry).
    pub fn is_bookkeeping(&self) -> bool {
        self.bookkeeping
    }

    /// Set the causal chain event_id.
    pub fn caused_by(mut self, event_id: u64) -> Self {
        self.caused_by = Some(event_id);
        self
    }

    /// Add a participant.
    pub fn with_participant(mut self, entity: Entity, role: ParticipantRole) -> Self {
        self.participants.push((entity, role));
        self
    }

    /// Set the event data.
    pub fn with_data(mut self, data: serde_json::Value) -> Self {
        self.event_data = data;
        self
    }
}

/// All possible state-change intents, organized by domain.
///
/// Every variant is defined upfront (enums are cheap). Unimplemented variants
/// produce a warning in the applicator but don't panic, allowing incremental
/// implementation.
#[derive(Clone, Debug)]
pub enum SimCommandKind {
    // -- Entity Lifecycle --
    EndEntity {
        entity: Entity,
    },
    RenameEntity {
        entity: Entity,
        new_name: String,
    },

    // -- Relationships --
    AddRelationship {
        source: Entity,
        target: Entity,
        kind: RelationshipKind,
    },
    EndRelationship {
        source: Entity,
        target: Entity,
        kind: RelationshipKind,
    },

    // -- Demographics --
    GrowPopulation {
        settlement: Entity,
        amount: u32,
    },
    PersonDied {
        person: Entity,
    },
    PersonBorn {
        name: String,
        faction: Entity,
        settlement: Entity,
        sex: Sex,
        role: Role,
        traits: Vec<Trait>,
        culture_id: Option<u64>,
        father: Option<Entity>,
        mother: Option<Entity>,
    },
    Marriage {
        person_a: Entity,
        person_b: Entity,
    },

    // -- Economy --
    CollectTaxes {
        faction: Entity,
    },
    EstablishTradeRoute {
        settlement_a: Entity,
        settlement_b: Entity,
    },
    SeverTradeRoute {
        settlement_a: Entity,
        settlement_b: Entity,
    },
    PayArmyMaintenance {
        faction: Entity,
    },
    UpdateProduction {
        settlement: Entity,
    },

    // -- Military --
    DeclareWar {
        attacker: Entity,
        defender: Entity,
    },
    MusterArmy {
        faction: Entity,
        region: Entity,
    },
    MarchArmy {
        army: Entity,
        target_region: Entity,
    },
    ResolveBattle {
        attacker_army: Entity,
        defender_army: Entity,
        attacker_casualties: u32,
        defender_casualties: u32,
        attacker_won: bool,
    },
    BeginSiege {
        army: Entity,
        settlement: Entity,
    },
    ResolveAssault {
        army: Entity,
        settlement: Entity,
        succeeded: bool,
        attacker_casualties: u32,
        defender_casualties: u32,
    },
    CaptureSettlement {
        settlement: Entity,
        new_faction: Entity,
    },
    SignTreaty {
        faction_a: Entity,
        faction_b: Entity,
    },

    // -- Politics --
    SucceedLeader {
        faction: Entity,
        new_leader: Entity,
    },
    AttemptCoup {
        faction: Entity,
        instigator: Entity,
    },
    FormAlliance {
        faction_a: Entity,
        faction_b: Entity,
    },
    BetrayAlliance {
        betrayer: Entity,
        betrayed: Entity,
    },
    SplitFaction {
        parent_faction: Entity,
        new_faction_name: String,
    },

    // -- Culture / Religion --
    CulturalShift {
        settlement: Entity,
        new_culture: Entity,
    },
    FoundReligion {
        founder: Entity,
        name: String,
    },
    ReligiousSchism {
        parent_religion: Entity,
        new_name: String,
    },
    ConvertFaction {
        faction: Entity,
        religion: Entity,
    },

    // -- Knowledge --
    CreateKnowledge {
        name: String,
        settlement: Entity,
    },
    CreateManifestation {
        knowledge: Entity,
        settlement: Entity,
    },
    DestroyManifestation {
        manifestation: Entity,
    },
    RevealSecret {
        knowledge: Entity,
    },

    // -- Items --
    CraftItem {
        crafter: Entity,
        name: String,
    },
    TransferItem {
        item: Entity,
        new_holder: Entity,
    },

    // -- Crime --
    FormBanditGang {
        region: Entity,
    },
    BanditRaid {
        settlement: Entity,
    },

    // -- Disease --
    StartPlague {
        settlement: Entity,
        disease_name: String,
        virulence: f64,
        lethality: f64,
        duration_years: u32,
        bracket_severity: [f64; 8],
    },
    EndPlague {
        settlement: Entity,
    },
    SpreadPlague {
        from_settlement: Entity,
        to_settlement: Entity,
        disease_name: String,
        virulence: f64,
        lethality: f64,
        duration_years: u32,
        bracket_severity: [f64; 8],
    },
    UpdateInfection {
        settlement: Entity,
    },

    // -- Environment --
    TriggerDisaster {
        settlement: Entity,
        disaster_type: DisasterType,
        severity: f64,
        pop_loss_frac: f64,
        building_damage: f64,
        prosperity_hit: f64,
        sever_trade: bool,
        create_feature: Option<(String, FeatureType)>,
    },
    StartPersistentDisaster {
        settlement: Entity,
        disaster_type: DisasterType,
        severity: f64,
        months: u32,
    },
    EndDisaster {
        settlement: Entity,
    },
    CreateGeographicFeature {
        name: String,
        region: Entity,
        feature_type: FeatureType,
        x: f64,
        y: f64,
    },

    // -- Migration --
    MigratePopulation {
        from_settlement: Entity,
        to_settlement: Entity,
        count: u32,
    },
    RelocatePerson {
        person: Entity,
        to_settlement: Entity,
    },

    // -- Buildings --
    ConstructBuilding {
        settlement: Entity,
        faction: Entity,
        building_type: BuildingType,
        cost: f64,
        x: f64,
        y: f64,
    },
    DamageBuilding {
        building: Entity,
        damage: f64,
        cause: String,
    },
    UpgradeBuilding {
        building: Entity,
        new_level: u8,
        cost: f64,
        faction: Entity,
    },

    // -- Reputation --
    AdjustPrestige {
        entity: Entity,
        delta: f64,
    },

    // -- Generic --
    SetField {
        entity: Entity,
        field: String,
        old_value: serde_json::Value,
        new_value: serde_json::Value,
    },
}
