use serde::{Deserialize, Serialize};

use super::entity::EntityKind;
use super::relationship::RelationshipKind;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EventEffect {
    pub event_id: u64,
    pub entity_id: u64,
    pub effect: StateChange,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StateChange {
    EntityCreated {
        kind: EntityKind,
        name: String,
    },
    EntityEnded,
    NameChanged {
        old: String,
        new: String,
    },
    RelationshipStarted {
        target_entity_id: u64,
        kind: RelationshipKind,
    },
    RelationshipEnded {
        target_entity_id: u64,
        kind: RelationshipKind,
    },
    PropertyChanged {
        field: String,
        old_value: String,
        new_value: String,
    },
}

impl StateChange {
    /// Return the serde tag string for this variant (for Postgres COPY without parsing JSON).
    pub fn effect_type_str(&self) -> &'static str {
        match self {
            StateChange::EntityCreated { .. } => "entity_created",
            StateChange::EntityEnded => "entity_ended",
            StateChange::NameChanged { .. } => "name_changed",
            StateChange::RelationshipStarted { .. } => "relationship_started",
            StateChange::RelationshipEnded { .. } => "relationship_ended",
            StateChange::PropertyChanged { .. } => "property_changed",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tagged_serde_entity_created() {
        let effect = EventEffect {
            event_id: 1,
            entity_id: 2,
            effect: StateChange::EntityCreated {
                kind: EntityKind::Person,
                name: "Aldric".to_string(),
            },
        };

        let json = serde_json::to_value(&effect).unwrap();
        assert_eq!(json["event_id"], 1);
        assert_eq!(json["entity_id"], 2);
        assert_eq!(json["effect"]["type"], "entity_created");
        assert_eq!(json["effect"]["kind"], "person");
        assert_eq!(json["effect"]["name"], "Aldric");
    }

    #[test]
    fn tagged_serde_entity_ended() {
        let effect = EventEffect {
            event_id: 5,
            entity_id: 3,
            effect: StateChange::EntityEnded,
        };

        let json = serde_json::to_value(&effect).unwrap();
        assert_eq!(json["effect"]["type"], "entity_ended");
    }

    #[test]
    fn tagged_serde_name_changed() {
        let effect = EventEffect {
            event_id: 10,
            entity_id: 4,
            effect: StateChange::NameChanged {
                old: "Ironhold".to_string(),
                new: "Ironhaven".to_string(),
            },
        };

        let json = serde_json::to_value(&effect).unwrap();
        assert_eq!(json["effect"]["type"], "name_changed");
        assert_eq!(json["effect"]["old"], "Ironhold");
        assert_eq!(json["effect"]["new"], "Ironhaven");
    }

    #[test]
    fn tagged_serde_relationship_started() {
        let effect = EventEffect {
            event_id: 7,
            entity_id: 1,
            effect: StateChange::RelationshipStarted {
                target_entity_id: 2,
                kind: RelationshipKind::Spouse,
            },
        };

        let json = serde_json::to_value(&effect).unwrap();
        assert_eq!(json["effect"]["type"], "relationship_started");
        assert_eq!(json["effect"]["target_entity_id"], 2);
        assert_eq!(json["effect"]["kind"], "spouse");
    }

    #[test]
    fn serde_round_trip() {
        let effect = EventEffect {
            event_id: 1,
            entity_id: 2,
            effect: StateChange::PropertyChanged {
                field: "culture".to_string(),
                old_value: "northern".to_string(),
                new_value: "imperial".to_string(),
            },
        };

        let json = serde_json::to_string(&effect).unwrap();
        let parsed: EventEffect = serde_json::from_str(&json).unwrap();
        assert_eq!(effect, parsed);
    }

    #[test]
    fn effect_type_str_matches_serde_tag() {
        assert_eq!(
            StateChange::EntityCreated {
                kind: EntityKind::Person,
                name: "X".to_string()
            }
            .effect_type_str(),
            "entity_created"
        );
        assert_eq!(StateChange::EntityEnded.effect_type_str(), "entity_ended");
        assert_eq!(
            StateChange::NameChanged {
                old: "a".to_string(),
                new: "b".to_string()
            }
            .effect_type_str(),
            "name_changed"
        );
        assert_eq!(
            StateChange::RelationshipStarted {
                target_entity_id: 1,
                kind: RelationshipKind::Ally
            }
            .effect_type_str(),
            "relationship_started"
        );
        assert_eq!(
            StateChange::RelationshipEnded {
                target_entity_id: 1,
                kind: RelationshipKind::Enemy
            }
            .effect_type_str(),
            "relationship_ended"
        );
        assert_eq!(
            StateChange::PropertyChanged {
                field: "x".to_string(),
                old_value: "a".to_string(),
                new_value: "b".to_string()
            }
            .effect_type_str(),
            "property_changed"
        );
    }
}
