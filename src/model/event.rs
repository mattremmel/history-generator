use serde::{Deserialize, Serialize};

use super::timestamp::SimTimestamp;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum EventKind {
    // Lifecycle
    Birth,
    Death,
    SettlementFounded,
    FactionFormed,
    // Relationship
    Union,
    Dissolution,
    // Membership / Authority
    Joined,
    Left,
    Succession,
    Conquest,
    // Conflict
    WarDeclared,
    Battle,
    Siege,
    Treaty,
    // Movement
    Migration,
    Exile,
    // Ending modes
    Abandoned,
    // Construction / Destruction
    Construction,
    Destruction,
    // Items / Artifacts
    Crafted,
    // Knowledge / Culture
    Discovery,
    Schism,
    // Natural
    NaturalDisaster,
    // Ceremonial
    Burial,
    Ceremony,
    // Renaming
    Renamed,
    // Catch-all
    Custom(String),
}

impl From<EventKind> for String {
    fn from(kind: EventKind) -> Self {
        match kind {
            EventKind::Birth => "birth".into(),
            EventKind::Death => "death".into(),
            EventKind::SettlementFounded => "settlement_founded".into(),
            EventKind::FactionFormed => "faction_formed".into(),
            EventKind::Union => "union".into(),
            EventKind::Dissolution => "dissolution".into(),
            EventKind::Joined => "joined".into(),
            EventKind::Left => "left".into(),
            EventKind::Succession => "succession".into(),
            EventKind::Conquest => "conquest".into(),
            EventKind::WarDeclared => "war_declared".into(),
            EventKind::Battle => "battle".into(),
            EventKind::Siege => "siege".into(),
            EventKind::Treaty => "treaty".into(),
            EventKind::Migration => "migration".into(),
            EventKind::Exile => "exile".into(),
            EventKind::Abandoned => "abandoned".into(),
            EventKind::Construction => "construction".into(),
            EventKind::Destruction => "destruction".into(),
            EventKind::Crafted => "crafted".into(),
            EventKind::Discovery => "discovery".into(),
            EventKind::Schism => "schism".into(),
            EventKind::NaturalDisaster => "natural_disaster".into(),
            EventKind::Burial => "burial".into(),
            EventKind::Ceremony => "ceremony".into(),
            EventKind::Renamed => "renamed".into(),
            EventKind::Custom(s) => s,
        }
    }
}

impl TryFrom<String> for EventKind {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        match s.as_str() {
            "birth" => Ok(EventKind::Birth),
            "death" => Ok(EventKind::Death),
            "settlement_founded" => Ok(EventKind::SettlementFounded),
            "faction_formed" => Ok(EventKind::FactionFormed),
            "union" => Ok(EventKind::Union),
            "dissolution" => Ok(EventKind::Dissolution),
            "joined" => Ok(EventKind::Joined),
            "left" => Ok(EventKind::Left),
            "succession" => Ok(EventKind::Succession),
            "conquest" => Ok(EventKind::Conquest),
            "war_declared" => Ok(EventKind::WarDeclared),
            "battle" => Ok(EventKind::Battle),
            "siege" => Ok(EventKind::Siege),
            "treaty" => Ok(EventKind::Treaty),
            "migration" => Ok(EventKind::Migration),
            "exile" => Ok(EventKind::Exile),
            "abandoned" => Ok(EventKind::Abandoned),
            "construction" => Ok(EventKind::Construction),
            "destruction" => Ok(EventKind::Destruction),
            "crafted" => Ok(EventKind::Crafted),
            "discovery" => Ok(EventKind::Discovery),
            "schism" => Ok(EventKind::Schism),
            "natural_disaster" => Ok(EventKind::NaturalDisaster),
            "burial" => Ok(EventKind::Burial),
            "ceremony" => Ok(EventKind::Ceremony),
            "renamed" => Ok(EventKind::Renamed),
            "" => Err("event kind cannot be empty".into()),
            _ => Ok(EventKind::Custom(s)),
        }
    }
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
        assert_eq!(
            serde_json::to_string(&EventKind::WarDeclared).unwrap(),
            "\"war_declared\""
        );
        assert_eq!(
            serde_json::to_string(&EventKind::NaturalDisaster).unwrap(),
            "\"natural_disaster\""
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
            EventKind::NaturalDisaster,
            EventKind::Burial,
            EventKind::Ceremony,
            EventKind::Renamed,
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
    }
}
