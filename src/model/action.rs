//! Action types for the unified action queue system.
//!
//! External code queues `Action`s on the world; the `ActionSystem`
//! drains them each tick and produces `ActionResult`s.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionSource {
    /// External player input
    Player,
    /// NPC decided autonomously (future: goal system feeds here)
    Autonomous,
    /// Ordered by another entity
    Order { ordered_by: u64 },
}

#[derive(Debug)]
pub struct Action {
    pub actor_id: u64,
    pub source: ActionSource,
    pub kind: ActionKind,
}

#[derive(Debug)]
pub enum ActionKind {
    Assassinate { target_id: u64 },
    SupportFaction { faction_id: u64 },
    UndermineFaction { faction_id: u64 },
    BrokerAlliance { faction_a: u64, faction_b: u64 },
    DeclareWar { target_faction_id: u64 },
}

#[derive(Debug)]
pub struct ActionResult {
    pub actor_id: u64,
    pub source: ActionSource,
    pub outcome: ActionOutcome,
}

#[derive(Debug)]
pub enum ActionOutcome {
    Success { event_id: u64 },
    Failed { reason: String },
}
