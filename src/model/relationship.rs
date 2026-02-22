use serde::de;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::timestamp::SimTimestamp;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RelationshipKind {
    Parent,
    Child,
    Spouse,
    Ally,
    Enemy,
    MemberOf,
    RulerOf,
    Custom(String),
}

impl Serialize for RelationshipKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let s = match self {
            RelationshipKind::Parent => "parent",
            RelationshipKind::Child => "child",
            RelationshipKind::Spouse => "spouse",
            RelationshipKind::Ally => "ally",
            RelationshipKind::Enemy => "enemy",
            RelationshipKind::MemberOf => "member_of",
            RelationshipKind::RulerOf => "ruler_of",
            RelationshipKind::Custom(s) => s.as_str(),
        };
        serializer.serialize_str(s)
    }
}

impl<'de> Deserialize<'de> for RelationshipKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            "parent" => Ok(RelationshipKind::Parent),
            "child" => Ok(RelationshipKind::Child),
            "spouse" => Ok(RelationshipKind::Spouse),
            "ally" => Ok(RelationshipKind::Ally),
            "enemy" => Ok(RelationshipKind::Enemy),
            "member_of" => Ok(RelationshipKind::MemberOf),
            "ruler_of" => Ok(RelationshipKind::RulerOf),
            _ => {
                if s.is_empty() {
                    Err(de::Error::custom("relationship kind cannot be empty"))
                } else {
                    Ok(RelationshipKind::Custom(s))
                }
            }
        }
    }
}

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
            serde_json::to_string(&RelationshipKind::RulerOf).unwrap(),
            "\"ruler_of\""
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
            RelationshipKind::MemberOf,
            RelationshipKind::RulerOf,
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
