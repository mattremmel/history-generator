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
    WarEnded { winner_id: u64, loser_id: u64 },

    /// A settlement was captured and transferred to a new faction.
    SettlementCaptured {
        settlement_id: u64,
        old_faction_id: u64,
        new_faction_id: u64,
    },

    /// A resource deposit was exhausted.
    ResourceDepleted { deposit_id: u64, region_id: u64 },

    /// A faction lost its ruler (death, exile, etc).
    RulerVacancy {
        faction_id: u64,
        previous_ruler_id: u64,
    },

    /// A settlement split off from its faction, forming a new one.
    FactionSplit {
        old_faction_id: u64,
        new_faction_id: u64,
        settlement_id: u64,
    },

    /// Extensible: any system can emit a custom signal.
    Custom {
        name: String,
        data: serde_json::Value,
    },
}
