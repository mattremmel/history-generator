//! Action types for the unified action queue system.
//!
//! External code queues `Action`s on the world; the `ActionSystem`
//! drains them each tick and produces `ActionResult`s.

use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionSource {
    /// External player input
    Player,
    /// NPC decided autonomously (future: goal system feeds here)
    Autonomous,
    /// Ordered by another entity
    Order { ordered_by: u64 },
}

impl fmt::Display for ActionSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Player => write!(f, "player"),
            Self::Autonomous => write!(f, "autonomous"),
            Self::Order { ordered_by } => write!(f, "order(by {ordered_by})"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Action {
    pub actor_id: u64,
    pub source: ActionSource,
    pub kind: ActionKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    Assassinate { target_id: u64 },
    SupportFaction { faction_id: u64 },
    UndermineFaction { faction_id: u64 },
    BrokerAlliance { faction_a: u64, faction_b: u64 },
    DeclareWar { target_faction_id: u64 },
    AttemptCoup { faction_id: u64 },
    Defect { from_faction: u64, to_faction: u64 },
    SeekOffice { faction_id: u64 },
    BetrayAlly { ally_faction_id: u64 },
    PressClaim { target_faction_id: u64 },
}

impl fmt::Display for ActionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Assassinate { target_id } => write!(f, "assassinate({target_id})"),
            Self::SupportFaction { faction_id } => write!(f, "support_faction({faction_id})"),
            Self::UndermineFaction { faction_id } => {
                write!(f, "undermine_faction({faction_id})")
            }
            Self::BrokerAlliance {
                faction_a,
                faction_b,
            } => write!(f, "broker_alliance({faction_a}, {faction_b})"),
            Self::DeclareWar { target_faction_id } => {
                write!(f, "declare_war({target_faction_id})")
            }
            Self::AttemptCoup { faction_id } => write!(f, "attempt_coup({faction_id})"),
            Self::Defect {
                from_faction,
                to_faction,
            } => write!(f, "defect({from_faction} -> {to_faction})"),
            Self::SeekOffice { faction_id } => write!(f, "seek_office({faction_id})"),
            Self::BetrayAlly { ally_faction_id } => {
                write!(f, "betray_ally({ally_faction_id})")
            }
            Self::PressClaim { target_faction_id } => {
                write!(f, "press_claim({target_faction_id})")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionResult {
    pub actor_id: u64,
    pub source: ActionSource,
    pub outcome: ActionOutcome,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionOutcome {
    Success { event_id: u64 },
    Failed { reason: String },
}

impl fmt::Display for ActionOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Success { event_id } => write!(f, "success(event {event_id})"),
            Self::Failed { reason } => write!(f, "failed: {reason}"),
        }
    }
}
