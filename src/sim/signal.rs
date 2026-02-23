use serde::{Deserialize, Serialize};

/// A signal emitted by one system and consumed by others.
/// Carries the event_id that caused it, enabling `caused_by` chains
/// when reacting systems create follow-up events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signal {
    /// The event that triggered this signal (for causal linking).
    pub event_id: u64,
    /// What happened.
    pub kind: SignalKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SignalKind {
    /// An entity died or was destroyed this tick.
    EntityDied { entity_id: u64 },

    /// A settlement's population changed significantly.
    PopulationChanged {
        settlement_id: u64,
        old: u32,
        new: u32,
    },

    /// A war started between two factions.
    WarStarted { attacker_id: u64, defender_id: u64 },

    /// A war ended between two factions.
    WarEnded {
        winner_id: u64,
        loser_id: u64,
        decisive: bool,
        reparations: f64,
        tribute_years: u32,
    },

    /// A settlement was captured and transferred to a new faction.
    SettlementCaptured {
        settlement_id: u64,
        old_faction_id: u64,
        new_faction_id: u64,
    },

    /// A resource deposit was exhausted.
    ResourceDepleted { deposit_id: u64, region_id: u64 },

    /// A faction lost its leader (death, exile, etc).
    LeaderVacancy {
        faction_id: u64,
        previous_leader_id: u64,
    },

    /// A settlement split off from its faction, forming a new one.
    FactionSplit {
        old_faction_id: u64,
        new_faction_id: u64,
        settlement_id: u64,
    },

    /// A trade route was established between two settlements.
    TradeRouteEstablished {
        from_settlement: u64,
        to_settlement: u64,
        from_faction: u64,
        to_faction: u64,
    },

    /// A trade route was severed (war, capture, etc).
    TradeRouteSevered {
        from_settlement: u64,
        to_settlement: u64,
    },

    /// A faction's treasury hit zero.
    TreasuryDepleted { faction_id: u64 },

    /// Refugees arrived at a settlement from another settlement.
    RefugeesArrived {
        settlement_id: u64,
        source_settlement_id: u64,
        count: u32,
    },

    /// The dominant culture in a settlement shifted.
    CulturalShift {
        settlement_id: u64,
        old_culture: u64,
        new_culture: u64,
    },

    /// A cultural rebellion erupted in a settlement.
    CulturalRebellion {
        settlement_id: u64,
        faction_id: u64,
        culture_id: u64,
    },

    /// A plague broke out in a settlement.
    PlagueStarted { settlement_id: u64, disease_id: u64 },

    /// A plague spread from one settlement to another.
    PlagueSpreading {
        settlement_id: u64,
        disease_id: u64,
        from_settlement_id: u64,
    },

    /// A plague ended in a settlement.
    PlagueEnded {
        settlement_id: u64,
        disease_id: u64,
        deaths: u32,
    },

    /// A siege began on a settlement.
    SiegeStarted {
        settlement_id: u64,
        attacker_faction_id: u64,
        defender_faction_id: u64,
    },

    /// A siege ended on a settlement.
    SiegeEnded {
        settlement_id: u64,
        attacker_faction_id: u64,
        defender_faction_id: u64,
        /// "conquered", "lifted", or "abandoned"
        outcome: String,
    },

    /// A building was constructed in a settlement.
    BuildingConstructed {
        building_id: u64,
        settlement_id: u64,
        building_type: String,
    },

    /// A building was destroyed (decay, siege, etc).
    BuildingDestroyed {
        building_id: u64,
        settlement_id: u64,
        building_type: String,
        cause: String,
    },

    /// A building was upgraded to a new level.
    BuildingUpgraded {
        building_id: u64,
        settlement_id: u64,
        building_type: String,
        new_level: u8,
    },

    /// Extensible: any system can emit a custom signal.
    Custom {
        name: String,
        data: serde_json::Value,
    },
}
