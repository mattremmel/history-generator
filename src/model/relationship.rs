use serde::{Deserialize, Serialize};

use super::timestamp::SimTimestamp;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipKind {
    Parent,
    Child,
    Spouse,
    Ally,
    Enemy,
    MemberOf,
    RulerOf,
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
}
