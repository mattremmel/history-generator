use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::entity_data::EntityData;
use super::relationship::{Relationship, RelationshipKind};
use super::timestamp::SimTimestamp;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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
    Knowledge,
    Manifestation,
    Religion,
}

string_enum!(EntityKind {
    Person => "person",
    Settlement => "settlement",
    Faction => "faction",
    Region => "region",
    Building => "building",
    Item => "item",
    Army => "army",
    Deity => "deity",
    Creature => "creature",
    River => "river",
    GeographicFeature => "geographic_feature",
    ResourceDeposit => "resource_deposit",
    Culture => "culture",
    Disease => "disease",
    Knowledge => "knowledge",
    Manifestation => "manifestation",
    Religion => "religion",
});

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

impl Entity {
    /// Get an `f64` extra field, or `None` if missing/wrong type.
    pub fn extra_f64(&self, key: &str) -> Option<f64> {
        self.extra.get(key).and_then(|v| v.as_f64())
    }

    /// Get an `f64` extra field, falling back to `default`.
    pub fn extra_f64_or(&self, key: &str, default: f64) -> f64 {
        self.extra_f64(key).unwrap_or(default)
    }

    /// Get a `u64` extra field, or `None` if missing/wrong type.
    pub fn extra_u64(&self, key: &str) -> Option<u64> {
        self.extra.get(key).and_then(|v| v.as_u64())
    }

    /// Get a `u64` extra field, falling back to `default`.
    pub fn extra_u64_or(&self, key: &str, default: u64) -> u64 {
        self.extra_u64(key).unwrap_or(default)
    }

    /// Get a `bool` extra field, defaulting to `false`.
    pub fn extra_bool(&self, key: &str) -> bool {
        self.extra
            .get(key)
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    /// Get a `&str` extra field, or `None` if missing/wrong type.
    pub fn extra_str(&self, key: &str) -> Option<&str> {
        self.extra.get(key).and_then(|v| v.as_str())
    }

    /// Returns `true` if this entity has not been ended (no `end` timestamp).
    pub fn is_alive(&self) -> bool {
        self.end.is_none()
    }

    /// First active relationship target of the given kind.
    pub fn active_rel(&self, kind: RelationshipKind) -> Option<u64> {
        self.relationships
            .iter()
            .find(|r| r.kind == kind && r.is_active())
            .map(|r| r.target_entity_id)
    }

    /// All active relationship targets of the given kind.
    pub fn active_rels(&self, kind: RelationshipKind) -> impl Iterator<Item = u64> + '_ {
        self.relationships
            .iter()
            .filter(move |r| r.kind == kind && r.is_active())
            .map(|r| r.target_entity_id)
    }

    /// Whether an active relationship of the given kind to the specific target exists.
    pub fn has_active_rel(&self, kind: RelationshipKind, target: u64) -> bool {
        self.relationships
            .iter()
            .any(|r| r.kind == kind && r.target_entity_id == target && r.is_active())
    }
}

#[cfg(test)]
mod tests {
    use super::super::entity_data::{PersonData, Role, Sex};
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
                born: SimTimestamp::from_year(100),
                sex: Sex::Male,
                role: Role::Warrior,
                traits: vec![Trait::Ambitious],
                last_action: SimTimestamp::default(),
                culture_id: None,
                prestige: 0.0,
                grievances: std::collections::BTreeMap::new(),
                secrets: std::collections::BTreeMap::new(),
                claims: std::collections::BTreeMap::new(),
                prestige_tier: 0,
                widowed_at: None,
                loyalty: std::collections::BTreeMap::new(),
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
        assert_eq!(json["data"]["born"]["year"], 100);
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
    fn unknown_string_returns_error() {
        let result: Result<EntityKind, _> = serde_json::from_str("\"dragon\"");
        assert!(result.is_err());
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
            EntityKind::Knowledge,
            EntityKind::Manifestation,
            EntityKind::Religion,
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            let back: EntityKind = serde_json::from_str(&json).unwrap();
            assert_eq!(back, kind);
        }
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
            data: EntityData::default_for_kind(EntityKind::Person),
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
            data: EntityData::default_for_kind(EntityKind::Person),
            extra,
            relationships: vec![],
        };

        let json = serde_json::to_value(&entity).unwrap();
        assert_eq!(json["extra"]["mana"], 50);
    }
}
