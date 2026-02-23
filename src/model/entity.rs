use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::entity_data::EntityData;
use super::relationship::Relationship;
use super::timestamp::SimTimestamp;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum EntityKind {
    Person,
    Settlement,
    Faction,
    Region,
    Building,
    Item,
    Army,
    Deity,
    Creature,
    River,
    GeographicFeature,
    ResourceDeposit,
    Culture,
    Disease,
    Custom(String),
}

impl From<EntityKind> for String {
    fn from(kind: EntityKind) -> Self {
        match kind {
            EntityKind::Person => "person".into(),
            EntityKind::Settlement => "settlement".into(),
            EntityKind::Faction => "faction".into(),
            EntityKind::Region => "region".into(),
            EntityKind::Building => "building".into(),
            EntityKind::Item => "item".into(),
            EntityKind::Army => "army".into(),
            EntityKind::Deity => "deity".into(),
            EntityKind::Creature => "creature".into(),
            EntityKind::River => "river".into(),
            EntityKind::GeographicFeature => "geographic_feature".into(),
            EntityKind::ResourceDeposit => "resource_deposit".into(),
            EntityKind::Culture => "culture".into(),
            EntityKind::Disease => "disease".into(),
            EntityKind::Custom(s) => s,
        }
    }
}

impl TryFrom<String> for EntityKind {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        match s.as_str() {
            "person" => Ok(EntityKind::Person),
            "settlement" => Ok(EntityKind::Settlement),
            "faction" => Ok(EntityKind::Faction),
            "region" => Ok(EntityKind::Region),
            "building" => Ok(EntityKind::Building),
            "item" => Ok(EntityKind::Item),
            "army" => Ok(EntityKind::Army),
            "deity" => Ok(EntityKind::Deity),
            "creature" => Ok(EntityKind::Creature),
            "river" => Ok(EntityKind::River),
            "geographic_feature" => Ok(EntityKind::GeographicFeature),
            "resource_deposit" => Ok(EntityKind::ResourceDeposit),
            "culture" => Ok(EntityKind::Culture),
            "disease" => Ok(EntityKind::Disease),
            "" => Err("entity kind cannot be empty".into()),
            _ => Ok(EntityKind::Custom(s)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Entity {
    pub id: u64,
    pub kind: EntityKind,
    pub name: String,
    pub origin: Option<SimTimestamp>,
    pub end: Option<SimTimestamp>,

    /// Typed data specific to this entity's kind.
    pub data: EntityData,

    /// Dynamic/extensible properties not captured by EntityData
    /// (e.g. production maps, trade routes, computed bonuses).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, serde_json::Value>,

    /// Inline relationships during simulation, normalized at flush time.
    /// Skipped during serialization — extracted via `World::collect_relationships()`.
    #[serde(skip)]
    pub relationships: Vec<Relationship>,
}

#[cfg(test)]
mod tests {
    use super::super::entity_data::PersonData;
    use super::super::traits::Trait;
    use super::*;

    #[test]
    fn serializes_expected_shape() {
        let entity = Entity {
            id: 1,
            kind: EntityKind::Person,
            name: "Aldric".to_string(),
            origin: Some(SimTimestamp::from_year(100)),
            end: None,
            data: EntityData::Person(PersonData {
                birth_year: 100,
                sex: "male".to_string(),
                role: "warrior".to_string(),
                traits: vec![Trait::Ambitious],
                last_action_year: 0,
                culture_id: None,
            }),
            extra: HashMap::new(),
            relationships: vec![],
        };

        let json = serde_json::to_value(&entity).unwrap();
        assert_eq!(json["id"], 1);
        assert_eq!(json["kind"], "person");
        assert_eq!(json["name"], "Aldric");
        assert_eq!(json["origin"]["year"], 100);
        assert!(json["end"].is_null());
        assert!(json.get("relationships").is_none());
        // Empty extra is omitted
        assert!(json.get("extra").is_none());
        // Typed data is present
        assert_eq!(json["data"]["birth_year"], 100);
    }

    #[test]
    fn enum_snake_case() {
        assert_eq!(
            serde_json::to_string(&EntityKind::Settlement).unwrap(),
            "\"settlement\""
        );
        assert_eq!(
            serde_json::to_string(&EntityKind::Person).unwrap(),
            "\"person\""
        );
        assert_eq!(
            serde_json::to_string(&EntityKind::Faction).unwrap(),
            "\"faction\""
        );
    }

    #[test]
    fn custom_entity_kind_serializes_as_plain_string() {
        let kind = EntityKind::Custom("dragon".to_string());
        assert_eq!(serde_json::to_string(&kind).unwrap(), "\"dragon\"");
    }

    #[test]
    fn unknown_string_deserializes_to_custom() {
        let kind: EntityKind = serde_json::from_str("\"dragon\"").unwrap();
        assert_eq!(kind, EntityKind::Custom("dragon".to_string()));
    }

    #[test]
    fn core_entity_kind_round_trips() {
        for kind in [
            EntityKind::Person,
            EntityKind::Settlement,
            EntityKind::Faction,
            EntityKind::Region,
            EntityKind::Building,
            EntityKind::Item,
            EntityKind::Army,
            EntityKind::Deity,
            EntityKind::Creature,
            EntityKind::River,
            EntityKind::GeographicFeature,
            EntityKind::ResourceDeposit,
            EntityKind::Culture,
            EntityKind::Disease,
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            let back: EntityKind = serde_json::from_str(&json).unwrap();
            assert_eq!(back, kind);
        }
    }

    #[test]
    fn custom_entity_kind_round_trips() {
        let kind = EntityKind::Custom("dragon".to_string());
        let json = serde_json::to_string(&kind).unwrap();
        let back: EntityKind = serde_json::from_str(&json).unwrap();
        assert_eq!(back, kind);
    }

    #[test]
    fn relationships_skipped_in_serialization() {
        use super::super::relationship::RelationshipKind;

        let entity = Entity {
            id: 1,
            kind: EntityKind::Person,
            name: "Test".to_string(),
            origin: None,
            end: None,
            data: EntityData::default_for_kind(&EntityKind::Person),
            extra: HashMap::new(),
            relationships: vec![Relationship {
                source_entity_id: 1,
                target_entity_id: 2,
                kind: RelationshipKind::Ally,
                start: SimTimestamp::from_year(100),
                end: None,
            }],
        };

        let json = serde_json::to_string(&entity).unwrap();
        assert!(!json.contains("relationships"));
        assert!(!json.contains("source_entity_id"));
    }

    #[test]
    fn extra_serialized_when_nonempty() {
        let mut extra = HashMap::new();
        extra.insert("mana".to_string(), serde_json::json!(50));

        let entity = Entity {
            id: 1,
            kind: EntityKind::Person,
            name: "Gandalf".to_string(),
            origin: None,
            end: None,
            data: EntityData::default_for_kind(&EntityKind::Person),
            extra,
            relationships: vec![],
        };

        let json = serde_json::to_value(&entity).unwrap();
        assert_eq!(json["extra"]["mana"], 50);
    }
}
