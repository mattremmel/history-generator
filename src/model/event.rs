use serde::{Deserialize, Serialize};

use super::timestamp::SimTimestamp;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum EventKind {
    Birth,
    Death,
    SettlementFounded,
    FactionFormed,
    Union,
    Dissolution,
    Joined,
    Left,
    Succession,
    Conquest,
    Coup,
    WarDeclared,
    Battle,
    Siege,
    Treaty,
    Migration,
    Exile,
    Abandoned,
    Construction,
    Destruction,
    Crafted,
    Discovery,
    Schism,
    Disaster,
    Burial,
    Ceremony,
    Renamed,
    CulturalShift,
    Rebellion,
    SuccessionCrisis,
    Custom(String),
}

string_enum_open!(EventKind, "event kind", {
    Birth => "birth",
    Death => "death",
    SettlementFounded => "settlement_founded",
    FactionFormed => "faction_formed",
    Union => "union",
    Dissolution => "dissolution",
    Joined => "joined",
    Left => "left",
    Succession => "succession",
    Conquest => "conquest",
    Coup => "coup",
    WarDeclared => "war_declared",
    Battle => "battle",
    Siege => "siege",
    Treaty => "treaty",
    Migration => "migration",
    Exile => "exile",
    Abandoned => "abandoned",
    Construction => "construction",
    Destruction => "destruction",
    Crafted => "crafted",
    Discovery => "discovery",
    Schism => "schism",
    Disaster => "disaster",
    Burial => "burial",
    Ceremony => "ceremony",
    Renamed => "renamed",
    CulturalShift => "cultural_shift",
    Rebellion => "rebellion",
    SuccessionCrisis => "succession_crisis",
});

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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum ParticipantRole {
    Subject,
    Object,
    Location,
    Witness,
    Attacker,
    Defender,
    Origin,
    Destination,
    Parent,
    Instigator,
    Custom(String),
}

string_enum_open!(ParticipantRole, "participant role", {
    Subject => "subject",
    Object => "object",
    Location => "location",
    Witness => "witness",
    Attacker => "attacker",
    Defender => "defender",
    Origin => "origin",
    Destination => "destination",
    Parent => "parent",
    Instigator => "instigator",
});

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
        assert_eq!(
            serde_json::to_string(&EventKind::WarDeclared).unwrap(),
            "\"war_declared\""
        );
        assert_eq!(
            serde_json::to_string(&EventKind::Disaster).unwrap(),
            "\"disaster\""
        );
    }

    #[test]
    fn custom_event_kind_serializes_as_plain_string() {
        let kind = EventKind::Custom("spell_cast".to_string());
        assert_eq!(serde_json::to_string(&kind).unwrap(), "\"spell_cast\"");
    }

    #[test]
    fn unknown_string_deserializes_to_custom() {
        let kind: EventKind = serde_json::from_str("\"spell_cast\"").unwrap();
        assert_eq!(kind, EventKind::Custom("spell_cast".to_string()));
    }

    #[test]
    fn core_event_kind_round_trips() {
        for kind in [
            EventKind::Birth,
            EventKind::Death,
            EventKind::SettlementFounded,
            EventKind::FactionFormed,
            EventKind::Union,
            EventKind::Dissolution,
            EventKind::Joined,
            EventKind::Left,
            EventKind::Succession,
            EventKind::Conquest,
            EventKind::Coup,
            EventKind::WarDeclared,
            EventKind::Battle,
            EventKind::Siege,
            EventKind::Treaty,
            EventKind::Migration,
            EventKind::Exile,
            EventKind::Abandoned,
            EventKind::Construction,
            EventKind::Destruction,
            EventKind::Crafted,
            EventKind::Discovery,
            EventKind::Schism,
            EventKind::Disaster,
            EventKind::Burial,
            EventKind::Ceremony,
            EventKind::Renamed,
            EventKind::CulturalShift,
            EventKind::Rebellion,
            EventKind::SuccessionCrisis,
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            let back: EventKind = serde_json::from_str(&json).unwrap();
            assert_eq!(back, kind);
        }
    }

    #[test]
    fn custom_event_kind_round_trips() {
        let kind = EventKind::Custom("plague_outbreak".to_string());
        let json = serde_json::to_string(&kind).unwrap();
        let back: EventKind = serde_json::from_str(&json).unwrap();
        assert_eq!(back, kind);
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

        // Verify new roles serialize correctly
        let p2 = EventParticipant {
            event_id: 10,
            entity_id: 2,
            role: ParticipantRole::Attacker,
        };
        let json2 = serde_json::to_value(&p2).unwrap();
        assert_eq!(json2["role"], "attacker");
    }

    #[test]
    fn core_participant_role_round_trips() {
        for role in [
            ParticipantRole::Subject,
            ParticipantRole::Object,
            ParticipantRole::Location,
            ParticipantRole::Witness,
            ParticipantRole::Attacker,
            ParticipantRole::Defender,
            ParticipantRole::Origin,
            ParticipantRole::Destination,
            ParticipantRole::Parent,
            ParticipantRole::Instigator,
        ] {
            let json = serde_json::to_string(&role).unwrap();
            let back: ParticipantRole = serde_json::from_str(&json).unwrap();
            assert_eq!(back, role);
        }
    }

    #[test]
    fn custom_participant_role_round_trips() {
        let role = ParticipantRole::Custom("sacrifice".to_string());
        let json = serde_json::to_string(&role).unwrap();
        assert_eq!(json, "\"sacrifice\"");
        let back: ParticipantRole = serde_json::from_str(&json).unwrap();
        assert_eq!(back, role);
    }

    #[test]
    fn unknown_string_deserializes_to_custom_role() {
        let role: ParticipantRole = serde_json::from_str("\"herald\"").unwrap();
        assert_eq!(role, ParticipantRole::Custom("herald".to_string()));
    }
}
