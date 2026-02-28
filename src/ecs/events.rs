use bevy_ecs::entity::Entity;
use bevy_ecs::message::Message;

/// Reactive events emitted by the command applicator for cross-system reactions.
///
/// Each variant carries an `event_id` linking back to the EventLog entry that
/// caused it, enabling causal chain propagation through the system.
#[derive(Message, Clone, Debug)]
pub enum SimReactiveEvent {
    // -- Military / Conflict --
    WarStarted {
        event_id: u64,
        attacker: Entity,
        defender: Entity,
    },
    WarEnded {
        event_id: u64,
        winner: Entity,
        loser: Entity,
        decisive: bool,
    },
    SettlementCaptured {
        event_id: u64,
        settlement: Entity,
        old_faction: Entity,
        new_faction: Entity,
    },
    SiegeStarted {
        event_id: u64,
        settlement: Entity,
        attacker: Entity,
    },
    SiegeEnded {
        event_id: u64,
        settlement: Entity,
        defender_faction: Entity,
    },

    // -- Politics / Leadership --
    LeaderVacancy {
        event_id: u64,
        faction: Entity,
        previous_leader: Entity,
    },
    SuccessionCrisis {
        event_id: u64,
        faction: Entity,
    },
    FactionSplit {
        event_id: u64,
        parent_faction: Entity,
        new_faction: Entity,
    },
    FailedCoup {
        event_id: u64,
        faction: Entity,
        instigator: Entity,
    },
    AllianceBetrayed {
        event_id: u64,
        betrayer: Entity,
        betrayed: Entity,
    },

    // -- Demographics / Lifecycle --
    EntityDied {
        event_id: u64,
        entity: Entity,
    },
    RefugeesArrived {
        event_id: u64,
        settlement: Entity,
        source_settlement: Entity,
        count: u32,
    },

    // -- Disease --
    PlagueStarted {
        event_id: u64,
        settlement: Entity,
    },
    PlagueEnded {
        event_id: u64,
        settlement: Entity,
    },

    // -- Environment / Disasters --
    DisasterStruck {
        event_id: u64,
        region: Entity,
    },
    DisasterStarted {
        event_id: u64,
        region: Entity,
    },
    DisasterEnded {
        event_id: u64,
        region: Entity,
    },

    // -- Economy / Trade --
    TradeRouteEstablished {
        event_id: u64,
        settlement_a: Entity,
        settlement_b: Entity,
    },
    TradeRouteRaided {
        event_id: u64,
        settlement_a: Entity,
        settlement_b: Entity,
    },
    TreasuryDepleted {
        event_id: u64,
        faction: Entity,
    },

    // -- Crime --
    BanditRaid {
        event_id: u64,
        settlement: Entity,
    },
    BanditGangFormed {
        event_id: u64,
        region: Entity,
    },

    // -- Buildings --
    BuildingConstructed {
        event_id: u64,
        building: Entity,
        settlement: Entity,
    },
    BuildingUpgraded {
        event_id: u64,
        building: Entity,
    },

    // -- Knowledge --
    KnowledgeCreated {
        event_id: u64,
        knowledge: Entity,
    },
    ManifestationCreated {
        event_id: u64,
        manifestation: Entity,
    },

    // -- Items --
    ItemCrafted {
        event_id: u64,
        item: Entity,
    },
    ItemTierPromoted {
        event_id: u64,
        item: Entity,
    },

    // -- Religion / Culture --
    CulturalRebellion {
        event_id: u64,
        settlement: Entity,
    },
    SecretRevealed {
        event_id: u64,
        knowledge: Entity,
    },
    ReligionFounded {
        event_id: u64,
        religion: Entity,
    },
    ReligionSchism {
        event_id: u64,
        parent_religion: Entity,
        new_religion: Entity,
    },
    ProphecyDeclared {
        event_id: u64,
        deity: Entity,
    },
}
