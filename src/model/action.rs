/// Player action types for the action queue system.
///
/// External code queues `PlayerAction`s on the world; the `PlayerActionSystem`
/// drains them each tick and produces `ActionResult`s.

#[derive(Debug)]
pub struct PlayerAction {
    pub player_id: u64,
    pub kind: ActionKind,
}

#[derive(Debug)]
pub enum ActionKind {
    Assassinate { target_id: u64 },
    SupportFaction { faction_id: u64 },
    UndermineFaction { faction_id: u64 },
    BrokerAlliance { faction_a: u64, faction_b: u64 },
}

#[derive(Debug)]
pub enum ActionResult {
    Success { event_id: u64 },
    Failed { reason: String },
}
