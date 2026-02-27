use bevy_ecs::resource::Resource;

use crate::ecs::time::SimTime;
use crate::model::effect::EventEffect;
use crate::model::event::{EventKind, EventParticipant};

/// An event record using ECS-native `SimTime` (minute resolution).
#[derive(Debug, Clone, PartialEq)]
pub struct EcsEvent {
    pub id: u64,
    pub kind: EventKind,
    pub timestamp: SimTime,
    pub description: String,
    pub caused_by: Option<u64>,
    pub data: serde_json::Value,
}

/// Accumulates events, participants, and effects between flushes.
#[derive(Resource, Debug, Clone, Default)]
pub struct EventLog {
    pub events: Vec<EcsEvent>,
    pub participants: Vec<EventParticipant>,
    pub effects: Vec<EventEffect>,
}

impl EventLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.events.clear();
        self.participants.clear();
        self.effects.clear();
    }
}
