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
        };

        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["id"], 10);
        assert_eq!(json["kind"], "birth");
        assert_eq!(json["timestamp"]["year"], 100);
        assert_eq!(json["timestamp"]["day"], 1);
        assert_eq!(json["timestamp"]["hour"], 0);
        assert_eq!(json["description"], "A child is born");
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
