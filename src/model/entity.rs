use std::collections::HashMap;

use serde::{Deserialize, Serialize};

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

    /// Setting-specific properties (e.g. {"mana": 50, "class": "wizard"}).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub properties: HashMap<String, serde_json::Value>,

    /// Inline relationships during simulation, normalized at flush time.
    /// Skipped during serialization — extracted via `World::collect_relationships()`.
    #[serde(skip)]
    pub relationships: Vec<Relationship>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_expected_shape() {
        let entity = Entity {
            id: 1,
            kind: EntityKind::Person,
            name: "Aldric".to_string(),
            origin: Some(SimTimestamp::from_year(100)),
            end: None,
            properties: HashMap::new(),
            relationships: vec![],
        };

        let json = serde_json::to_value(&entity).unwrap();
        assert_eq!(json["id"], 1);
        assert_eq!(json["kind"], "person");
        assert_eq!(json["name"], "Aldric");
        assert_eq!(json["origin"]["year"], 100);
        assert_eq!(json["origin"]["day"], 1);
        assert_eq!(json["origin"]["hour"], 0);
        assert!(json["end"].is_null());
        assert!(json.get("relationships").is_none());
        // Empty properties are omitted
        assert!(json.get("properties").is_none());
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
            properties: HashMap::new(),
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
    fn properties_serialized_when_nonempty() {
        let mut props = HashMap::new();
        props.insert("mana".to_string(), serde_json::json!(50));
        props.insert("class".to_string(), serde_json::json!("wizard"));

        let entity = Entity {
            id: 1,
            kind: EntityKind::Person,
            name: "Gandalf".to_string(),
            origin: None,
            end: None,
            properties: props,
            relationships: vec![],
        };

        let json = serde_json::to_value(&entity).unwrap();
        assert_eq!(json["properties"]["mana"], 50);
        assert_eq!(json["properties"]["class"], "wizard");
    }

    #[test]
    fn properties_deserialized_when_missing() {
        let json = r#"{"id":1,"kind":"person","name":"Test","origin":null,"end":null}"#;
        let entity: Entity = serde_json::from_str(json).unwrap();
        assert!(entity.properties.is_empty());
    }
}
