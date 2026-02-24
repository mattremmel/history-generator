use serde::{Deserialize, Serialize};

use super::timestamp::SimTimestamp;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum RelationshipKind {
    Parent,
    Child,
    Spouse,
    Ally,
    Enemy,
    AtWar,
    MemberOf,
    LeaderOf,
    AdjacentTo,
    LocatedIn,
    FlowsThrough,
    Exploits,
    TradeRoute,
    HeldBy,
    Custom(String),
}

string_enum_open!(RelationshipKind, "relationship kind", {
    Parent => "parent",
    Child => "child",
    Spouse => "spouse",
    Ally => "ally",
    Enemy => "enemy",
    AtWar => "at_war",
    MemberOf => "member_of",
    LeaderOf => "leader_of",
    AdjacentTo => "adjacent_to",
    LocatedIn => "located_in",
    FlowsThrough => "flows_through",
    Exploits => "exploits",
    TradeRoute => "trade_route",
    HeldBy => "held_by",
});

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Relationship {
    pub source_entity_id: u64,
    pub target_entity_id: u64,
    pub kind: RelationshipKind,
    pub start: SimTimestamp,
    pub end: Option<SimTimestamp>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_expected_shape() {
        let rel = Relationship {
            source_entity_id: 1,
            target_entity_id: 2,
            kind: RelationshipKind::Parent,
            start: SimTimestamp::from_year(100),
            end: Some(SimTimestamp::from_year(150)),
        };

        let json = serde_json::to_value(&rel).unwrap();
        assert_eq!(json["source_entity_id"], 1);
        assert_eq!(json["target_entity_id"], 2);
        assert_eq!(json["kind"], "parent");
        assert_eq!(json["start"]["year"], 100);
        assert_eq!(json["end"]["year"], 150);
    }

    #[test]
    fn null_end() {
        let rel = Relationship {
            source_entity_id: 1,
            target_entity_id: 2,
            kind: RelationshipKind::Ally,
            start: SimTimestamp::from_year(200),
            end: None,
        };

        let json = serde_json::to_value(&rel).unwrap();
        assert!(json["end"].is_null());
    }

    #[test]
    fn enum_snake_case() {
        assert_eq!(
            serde_json::to_string(&RelationshipKind::MemberOf).unwrap(),
            "\"member_of\""
        );
        assert_eq!(
            serde_json::to_string(&RelationshipKind::LeaderOf).unwrap(),
            "\"leader_of\""
        );
    }

    #[test]
    fn custom_relationship_kind_serializes_as_plain_string() {
        let kind = RelationshipKind::Custom("apprentice_of".to_string());
        assert_eq!(serde_json::to_string(&kind).unwrap(), "\"apprentice_of\"");
    }

    #[test]
    fn unknown_string_deserializes_to_custom() {
        let kind: RelationshipKind = serde_json::from_str("\"apprentice_of\"").unwrap();
        assert_eq!(kind, RelationshipKind::Custom("apprentice_of".to_string()));
    }

    #[test]
    fn core_relationship_kind_round_trips() {
        for kind in [
            RelationshipKind::Parent,
            RelationshipKind::Child,
            RelationshipKind::Spouse,
            RelationshipKind::Ally,
            RelationshipKind::Enemy,
            RelationshipKind::AtWar,
            RelationshipKind::MemberOf,
            RelationshipKind::LeaderOf,
            RelationshipKind::AdjacentTo,
            RelationshipKind::LocatedIn,
            RelationshipKind::FlowsThrough,
            RelationshipKind::Exploits,
            RelationshipKind::TradeRoute,
            RelationshipKind::HeldBy,
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            let back: RelationshipKind = serde_json::from_str(&json).unwrap();
            assert_eq!(back, kind);
        }
    }

    #[test]
    fn custom_relationship_kind_round_trips() {
        let kind = RelationshipKind::Custom("apprentice_of".to_string());
        let json = serde_json::to_string(&kind).unwrap();
        let back: RelationshipKind = serde_json::from_str(&json).unwrap();
        assert_eq!(back, kind);
    }
}
