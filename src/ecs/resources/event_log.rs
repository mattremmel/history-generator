use bevy_ecs::resource::Resource;

use crate::model::{Event, EventEffect, EventParticipant};

/// Accumulates events, participants, and effects between flushes.
#[derive(Resource, Debug, Clone, Default)]
pub struct EventLog {
    pub events: Vec<Event>,
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
