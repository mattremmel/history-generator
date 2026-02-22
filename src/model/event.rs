use serde::{Deserialize, Serialize};

use super::timestamp::SimTimestamp;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Birth,
    Death,
    Marriage,
    SettlementFounded,
    FactionFormed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Event {
    pub id: u64,
    pub kind: EventKind,
    pub timestamp: SimTimestamp,
    pub description: String,
    pub caused_by: Option<u64>,
    /// Setting-specific structured data for this event.
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ParticipantRole {
    Subject,
    Object,
    Location,
    Witness,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EventParticipant {
    pub event_id: u64,
    pub entity_id: u64,
    pub role: ParticipantRole,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_serializes_expected_shape() {
        let event = Event {
            id: 10,
            kind: EventKind::Birth,
            timestamp: SimTimestamp::from_year(100),
            description: "A child is born".to_string(),
            caused_by: None,
            data: serde_json::Value::Null,
        };

        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["id"], 10);
        assert_eq!(json["kind"], "birth");
        assert_eq!(json["timestamp"]["year"], 100);
        assert_eq!(json["timestamp"]["day"], 1);
        assert_eq!(json["timestamp"]["hour"], 0);
        assert_eq!(json["description"], "A child is born");
        assert!(json["caused_by"].is_null());
        // Null data is omitted
        assert!(json.get("data").is_none());
    }

    #[test]
    fn event_with_caused_by_serializes() {
        let event = Event {
            id: 20,
            kind: EventKind::Death,
            timestamp: SimTimestamp::from_year(170),
            description: "Died in battle".to_string(),
            caused_by: Some(10),
            data: serde_json::Value::Null,
        };

        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["caused_by"], 10);
    }

    #[test]
    fn event_data_serialized_when_nonnull() {
        let event = Event {
            id: 30,
            kind: EventKind::Birth,
            timestamp: SimTimestamp::from_year(100),
            description: "A magical birth".to_string(),
            caused_by: None,
            data: serde_json::json!({"omen": "comet", "intensity": 9}),
        };

        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["data"]["omen"], "comet");
        assert_eq!(json["data"]["intensity"], 9);
    }

    #[test]
    fn event_data_deserialized_when_missing() {
        let json = r#"{"id":1,"kind":"birth","timestamp":{"year":100,"day":1,"hour":0},"description":"test","caused_by":null}"#;
        let event: Event = serde_json::from_str(json).unwrap();
        assert!(event.data.is_null());
    }

    #[test]
    fn event_kind_snake_case() {
        assert_eq!(
            serde_json::to_string(&EventKind::SettlementFounded).unwrap(),
            "\"settlement_founded\""
        );
        assert_eq!(
            serde_json::to_string(&EventKind::FactionFormed).unwrap(),
            "\"faction_formed\""
        );
    }

    #[test]
    fn participant_serializes_expected_shape() {
        let p = EventParticipant {
            event_id: 10,
            entity_id: 1,
            role: ParticipantRole::Subject,
        };

        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["event_id"], 10);
        assert_eq!(json["entity_id"], 1);
        assert_eq!(json["role"], "subject");
    }
}
