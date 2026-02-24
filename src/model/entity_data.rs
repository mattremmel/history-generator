use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use super::cultural_value::{CulturalValue, NamingStyle};
use super::entity::EntityKind;
use super::traits::Trait;
use crate::sim::population::{NUM_BRACKETS, PopulationBreakdown};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BuildingType {
    Mine,
    Port,
    Market,
    Granary,
    Temple,
    Workshop,
    Aqueduct,
    Library,
}

impl fmt::Display for BuildingType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl BuildingType {
    pub fn as_str(&self) -> &str {
        match self {
            BuildingType::Mine => "mine",
            BuildingType::Port => "port",
            BuildingType::Market => "market",
            BuildingType::Granary => "granary",
            BuildingType::Temple => "temple",
            BuildingType::Workshop => "workshop",
            BuildingType::Aqueduct => "aqueduct",
            BuildingType::Library => "library",
        }
    }
}

impl From<BuildingType> for String {
    fn from(bt: BuildingType) -> Self {
        bt.as_str().to_string()
    }
}

impl TryFrom<String> for BuildingType {
    type Error = String;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        match s.as_str() {
            "mine" => Ok(BuildingType::Mine),
            "port" => Ok(BuildingType::Port),
            "market" => Ok(BuildingType::Market),
            "granary" => Ok(BuildingType::Granary),
            "temple" => Ok(BuildingType::Temple),
            "workshop" => Ok(BuildingType::Workshop),
            "aqueduct" => Ok(BuildingType::Aqueduct),
            "library" => Ok(BuildingType::Library),
            other => Err(format!("unknown building type: {other}")),
        }
    }
}

impl Serialize for BuildingType {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for BuildingType {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        BuildingType::try_from(s).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PersonData {
    pub birth_year: u32,
    pub sex: String,
    pub role: String,
    pub traits: Vec<Trait>,
    #[serde(default)]
    pub last_action_year: u32,
    #[serde(default)]
    pub culture_id: Option<u64>,
    /// Personal renown: 0.0 (nobody) to 1.0 (legendary). Decays toward baseline.
    #[serde(default)]
    pub prestige: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SettlementData {
    pub population: u32,
    pub population_breakdown: PopulationBreakdown,
    pub x: f64,
    pub y: f64,
    pub resources: Vec<String>,
    #[serde(default)]
    pub prosperity: f64,
    #[serde(default)]
    pub treasury: f64,
    #[serde(default)]
    pub dominant_culture: Option<u64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub culture_makeup: BTreeMap<u64, f64>,
    #[serde(default)]
    pub cultural_tension: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_disease: Option<ActiveDisease>,
    #[serde(default)]
    pub plague_immunity: f64,
    #[serde(default)]
    pub fortification_level: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_siege: Option<ActiveSiege>,
    /// Settlement renown: 0.0 (forgotten hamlet) to 1.0 (legendary city). Decays toward baseline.
    #[serde(default)]
    pub prestige: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_disaster: Option<ActiveDisaster>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActiveSiege {
    pub attacker_army_id: u64,
    pub attacker_faction_id: u64,
    pub started_year: u32,
    pub started_month: u32,
    pub months_elapsed: u32,
    pub civilian_deaths: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DisasterType {
    Earthquake,
    Flood,
    Drought,
    VolcanicEruption,
    Wildfire,
    Storm,
    Tsunami,
}

impl fmt::Display for DisasterType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl DisasterType {
    pub fn as_str(&self) -> &str {
        match self {
            DisasterType::Earthquake => "earthquake",
            DisasterType::Flood => "flood",
            DisasterType::Drought => "drought",
            DisasterType::VolcanicEruption => "volcanic_eruption",
            DisasterType::Wildfire => "wildfire",
            DisasterType::Storm => "storm",
            DisasterType::Tsunami => "tsunami",
        }
    }

    /// Returns true if this disaster type persists across multiple months.
    pub fn is_persistent(&self) -> bool {
        matches!(
            self,
            DisasterType::Drought | DisasterType::Flood | DisasterType::Wildfire
        )
    }
}

impl From<DisasterType> for String {
    fn from(dt: DisasterType) -> Self {
        dt.as_str().to_string()
    }
}

impl TryFrom<String> for DisasterType {
    type Error = String;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        match s.as_str() {
            "earthquake" => Ok(DisasterType::Earthquake),
            "flood" => Ok(DisasterType::Flood),
            "drought" => Ok(DisasterType::Drought),
            "volcanic_eruption" => Ok(DisasterType::VolcanicEruption),
            "wildfire" => Ok(DisasterType::Wildfire),
            "storm" => Ok(DisasterType::Storm),
            "tsunami" => Ok(DisasterType::Tsunami),
            other => Err(format!("unknown disaster type: {other}")),
        }
    }
}

impl Serialize for DisasterType {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for DisasterType {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        DisasterType::try_from(s).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActiveDisaster {
    pub disaster_type: DisasterType,
    pub severity: f64,
    pub started_year: u32,
    pub started_month: u32,
    pub months_remaining: u32,
    #[serde(default)]
    pub total_deaths: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActiveDisease {
    pub disease_id: u64,
    pub started_year: u32,
    pub infection_rate: f64,
    pub peak_reached: bool,
    /// Running total of deaths caused by this outbreak in this settlement.
    #[serde(default)]
    pub total_deaths: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FactionData {
    pub government_type: String,
    #[serde(default)]
    pub stability: f64,
    #[serde(default)]
    pub happiness: f64,
    #[serde(default)]
    pub legitimacy: f64,
    #[serde(default)]
    pub treasury: f64,
    #[serde(default)]
    pub alliance_strength: f64,
    #[serde(default)]
    pub primary_culture: Option<u64>,
    /// Faction prestige: 0.0 (obscure) to 1.0 (hegemonic). Decays toward baseline.
    #[serde(default)]
    pub prestige: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegionData {
    pub terrain: String,
    #[serde(default)]
    pub terrain_tags: Vec<String>,
    pub x: f64,
    pub y: f64,
    #[serde(default)]
    pub resources: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ArmyData {
    #[serde(default)]
    pub morale: f64,
    #[serde(default)]
    pub supply: f64,
    #[serde(default)]
    pub strength: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeographicFeatureData {
    pub feature_type: String,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResourceDepositData {
    pub resource_type: String,
    pub quantity: u32,
    pub quality: f64,
    pub discovered: bool,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BuildingData {
    pub building_type: BuildingType,
    #[serde(default)]
    pub output_resource: Option<String>,
    pub x: f64,
    pub y: f64,
    /// Structural condition: 0.0 (ruined) to 1.0 (pristine).
    #[serde(default = "default_condition")]
    pub condition: f64,
    /// Upgrade level: 0 (basic), 1 (improved), 2 (grand).
    #[serde(default)]
    pub level: u8,
    /// Year the building was constructed.
    #[serde(default)]
    pub construction_year: u32,
}

fn default_condition() -> f64 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RiverData {
    pub region_path: Vec<u64>,
    pub length: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CultureData {
    pub values: Vec<CulturalValue>,
    pub naming_style: NamingStyle,
    /// 0.0-1.0: higher means harder to assimilate.
    pub resistance: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiseaseData {
    /// 0.0-1.0: how easily it spreads between settlements.
    pub virulence: f64,
    /// 0.0-1.0: base death rate among infected population.
    pub lethality: f64,
    /// How many years an outbreak typically lasts in a settlement.
    pub duration_years: u32,
    /// Per-bracket mortality multipliers (indexes match population brackets).
    pub bracket_severity: [f64; NUM_BRACKETS],
}

// ---------------------------------------------------------------------------
// Knowledge & Manifestation data
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum KnowledgeCategory {
    Battle,
    Conquest,
    Dynasty,
    Disaster,
    Founding,
    Cultural,
    Diplomatic,
    Construction,
}

impl KnowledgeCategory {
    pub fn as_str(&self) -> &str {
        match self {
            KnowledgeCategory::Battle => "battle",
            KnowledgeCategory::Conquest => "conquest",
            KnowledgeCategory::Dynasty => "dynasty",
            KnowledgeCategory::Disaster => "disaster",
            KnowledgeCategory::Founding => "founding",
            KnowledgeCategory::Cultural => "cultural",
            KnowledgeCategory::Diplomatic => "diplomatic",
            KnowledgeCategory::Construction => "construction",
        }
    }
}

impl fmt::Display for KnowledgeCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<KnowledgeCategory> for String {
    fn from(kc: KnowledgeCategory) -> Self {
        kc.as_str().to_string()
    }
}

impl TryFrom<String> for KnowledgeCategory {
    type Error = String;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        match s.as_str() {
            "battle" => Ok(KnowledgeCategory::Battle),
            "conquest" => Ok(KnowledgeCategory::Conquest),
            "dynasty" => Ok(KnowledgeCategory::Dynasty),
            "disaster" => Ok(KnowledgeCategory::Disaster),
            "founding" => Ok(KnowledgeCategory::Founding),
            "cultural" => Ok(KnowledgeCategory::Cultural),
            "diplomatic" => Ok(KnowledgeCategory::Diplomatic),
            "construction" => Ok(KnowledgeCategory::Construction),
            other => Err(format!("unknown knowledge category: {other}")),
        }
    }
}

impl Serialize for KnowledgeCategory {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for KnowledgeCategory {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        KnowledgeCategory::try_from(s).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KnowledgeData {
    pub category: KnowledgeCategory,
    pub source_event_id: u64,
    pub origin_settlement_id: u64,
    pub origin_year: u32,
    /// 0.0-1.0: gates propagation range and derivation likelihood.
    pub significance: f64,
    /// The actual facts — DM's version.
    pub ground_truth: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Medium {
    Memory,
    OralTradition,
    WrittenBook,
    Scroll,
    CarvedStone,
    Song,
    Painting,
    Tapestry,
    Tattoo,
    Dream,
    MagicalImprint,
    EncodedCipher,
}

impl Medium {
    pub fn as_str(&self) -> &str {
        match self {
            Medium::Memory => "memory",
            Medium::OralTradition => "oral_tradition",
            Medium::WrittenBook => "written_book",
            Medium::Scroll => "scroll",
            Medium::CarvedStone => "carved_stone",
            Medium::Song => "song",
            Medium::Painting => "painting",
            Medium::Tapestry => "tapestry",
            Medium::Tattoo => "tattoo",
            Medium::Dream => "dream",
            Medium::MagicalImprint => "magical_imprint",
            Medium::EncodedCipher => "encoded_cipher",
        }
    }

    /// Annual decay rate for this medium's physical condition.
    pub fn decay_rate(&self) -> f64 {
        match self {
            Medium::Memory => 0.05,
            Medium::OralTradition => 0.02,
            Medium::Song => 0.01,
            Medium::WrittenBook => 0.005,
            Medium::Scroll => 0.008,
            Medium::CarvedStone => 0.001,
            Medium::Painting => 0.004,
            Medium::Tapestry => 0.003,
            Medium::Tattoo => 0.03,
            Medium::Dream => 0.10,
            Medium::MagicalImprint => 0.0,
            Medium::EncodedCipher => 0.005,
        }
    }
}

impl fmt::Display for Medium {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<Medium> for String {
    fn from(m: Medium) -> Self {
        m.as_str().to_string()
    }
}

impl TryFrom<String> for Medium {
    type Error = String;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        match s.as_str() {
            "memory" => Ok(Medium::Memory),
            "oral_tradition" => Ok(Medium::OralTradition),
            "written_book" => Ok(Medium::WrittenBook),
            "scroll" => Ok(Medium::Scroll),
            "carved_stone" => Ok(Medium::CarvedStone),
            "song" => Ok(Medium::Song),
            "painting" => Ok(Medium::Painting),
            "tapestry" => Ok(Medium::Tapestry),
            "tattoo" => Ok(Medium::Tattoo),
            "dream" => Ok(Medium::Dream),
            "magical_imprint" => Ok(Medium::MagicalImprint),
            "encoded_cipher" => Ok(Medium::EncodedCipher),
            other => Err(format!("unknown medium: {other}")),
        }
    }
}

impl Serialize for Medium {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Medium {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Medium::try_from(s).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ManifestationData {
    pub knowledge_id: u64,
    pub medium: Medium,
    /// What THIS version says (may diverge from ground_truth).
    pub content: serde_json::Value,
    /// 0.0-1.0 accuracy vs ground truth.
    pub accuracy: f64,
    /// 0.0-1.0 how much of the original is present.
    pub completeness: f64,
    /// JSON array of applied distortions.
    #[serde(default)]
    pub distortions: serde_json::Value,
    /// Parent manifestation entity ID (None = original/eyewitness).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derived_from_id: Option<u64>,
    /// How this manifestation was created: "witnessed", "copied", "retold", etc.
    #[serde(default)]
    pub derivation_method: String,
    /// Physical condition: 1.0 pristine → 0.0 destroyed.
    pub condition: f64,
    pub created_year: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EntityData {
    Person(PersonData),
    Settlement(SettlementData),
    Faction(FactionData),
    Region(RegionData),
    Army(ArmyData),
    GeographicFeature(GeographicFeatureData),
    ResourceDeposit(ResourceDepositData),
    Building(BuildingData),
    River(RiverData),
    Culture(CultureData),
    Disease(DiseaseData),
    Knowledge(KnowledgeData),
    Manifestation(ManifestationData),
    None,
}

macro_rules! entity_data_accessors {
    ($( $variant:ident, $data_ty:ident, $as_ref:ident, $as_mut:ident; )*) => {
        $(
            pub fn $as_ref(&self) -> Option<&$data_ty> {
                match self {
                    EntityData::$variant(d) => Some(d),
                    _ => None,
                }
            }

            pub fn $as_mut(&mut self) -> Option<&mut $data_ty> {
                match self {
                    EntityData::$variant(d) => Some(d),
                    _ => None,
                }
            }
        )*
    };
}

impl EntityData {
    pub fn default_for_kind(kind: &EntityKind) -> Self {
        match kind {
            EntityKind::Person => EntityData::Person(PersonData {
                birth_year: 0,
                sex: String::new(),
                role: "common".to_string(),
                traits: Vec::new(),
                last_action_year: 0,
                culture_id: None,
                prestige: 0.0,
            }),
            EntityKind::Settlement => EntityData::Settlement(SettlementData {
                population: 0,
                population_breakdown: PopulationBreakdown::empty(),
                x: 0.0,
                y: 0.0,
                resources: Vec::new(),
                prosperity: 0.5,
                treasury: 0.0,
                dominant_culture: None,
                culture_makeup: BTreeMap::new(),
                cultural_tension: 0.0,
                active_disease: None,
                plague_immunity: 0.0,
                fortification_level: 0,
                active_siege: None,
                prestige: 0.0,
                active_disaster: None,
            }),
            EntityKind::Faction => EntityData::Faction(FactionData {
                government_type: "chieftain".to_string(),
                stability: 0.5,
                happiness: 0.5,
                legitimacy: 0.5,
                treasury: 0.0,
                alliance_strength: 0.0,
                primary_culture: None,
                prestige: 0.0,
            }),
            EntityKind::Culture => EntityData::Culture(CultureData {
                values: Vec::new(),
                naming_style: NamingStyle::Nordic,
                resistance: 0.5,
            }),
            EntityKind::Region => EntityData::Region(RegionData {
                terrain: String::new(),
                terrain_tags: Vec::new(),
                x: 0.0,
                y: 0.0,
                resources: Vec::new(),
            }),
            EntityKind::Army => EntityData::Army(ArmyData {
                morale: 1.0,
                supply: 1.0,
                strength: 0,
            }),
            EntityKind::GeographicFeature => EntityData::GeographicFeature(GeographicFeatureData {
                feature_type: String::new(),
                x: 0.0,
                y: 0.0,
            }),
            EntityKind::ResourceDeposit => EntityData::ResourceDeposit(ResourceDepositData {
                resource_type: String::new(),
                quantity: 0,
                quality: 0.0,
                discovered: false,
                x: 0.0,
                y: 0.0,
            }),
            EntityKind::Building => EntityData::Building(BuildingData {
                building_type: BuildingType::Mine,
                output_resource: None,
                x: 0.0,
                y: 0.0,
                condition: 1.0,
                level: 0,
                construction_year: 0,
            }),
            EntityKind::River => EntityData::River(RiverData {
                region_path: Vec::new(),
                length: 0,
            }),
            EntityKind::Disease => EntityData::Disease(DiseaseData {
                virulence: 0.5,
                lethality: 0.3,
                duration_years: 3,
                bracket_severity: [1.0; NUM_BRACKETS],
            }),
            EntityKind::Knowledge => EntityData::Knowledge(KnowledgeData {
                category: KnowledgeCategory::Battle,
                source_event_id: 0,
                origin_settlement_id: 0,
                origin_year: 0,
                significance: 0.0,
                ground_truth: serde_json::Value::Null,
            }),
            EntityKind::Manifestation => EntityData::Manifestation(ManifestationData {
                knowledge_id: 0,
                medium: Medium::Memory,
                content: serde_json::Value::Null,
                accuracy: 1.0,
                completeness: 1.0,
                distortions: serde_json::json!([]),
                derived_from_id: None,
                derivation_method: String::new(),
                condition: 1.0,
                created_year: 0,
            }),
            _ => EntityData::None,
        }
    }

    entity_data_accessors! {
        Person, PersonData, as_person, as_person_mut;
        Settlement, SettlementData, as_settlement, as_settlement_mut;
        Faction, FactionData, as_faction, as_faction_mut;
        Region, RegionData, as_region, as_region_mut;
        Army, ArmyData, as_army, as_army_mut;
        GeographicFeature, GeographicFeatureData, as_geographic_feature, as_geographic_feature_mut;
        ResourceDeposit, ResourceDepositData, as_resource_deposit, as_resource_deposit_mut;
        Building, BuildingData, as_building, as_building_mut;
        River, RiverData, as_river, as_river_mut;
        Culture, CultureData, as_culture, as_culture_mut;
        Disease, DiseaseData, as_disease, as_disease_mut;
        Knowledge, KnowledgeData, as_knowledge, as_knowledge_mut;
        Manifestation, ManifestationData, as_manifestation, as_manifestation_mut;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_for_kind_person() {
        let data = EntityData::default_for_kind(&EntityKind::Person);
        assert!(data.as_person().is_some());
    }

    #[test]
    fn default_for_kind_settlement() {
        let data = EntityData::default_for_kind(&EntityKind::Settlement);
        let s = data.as_settlement().unwrap();
        assert_eq!(s.population, 0);
        assert!((s.prosperity - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn default_for_kind_unknown_returns_none() {
        let data = EntityData::default_for_kind(&EntityKind::Deity);
        assert!(matches!(data, EntityData::None));
    }

    #[test]
    fn accessor_mut_works() {
        let mut data = EntityData::default_for_kind(&EntityKind::Faction);
        data.as_faction_mut().unwrap().stability = 0.9;
        assert!((data.as_faction().unwrap().stability - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn serde_round_trip() {
        let data = EntityData::Person(PersonData {
            birth_year: 100,
            sex: "male".to_string(),
            role: "warrior".to_string(),
            traits: vec![Trait::Ambitious, Trait::Aggressive],
            last_action_year: 105,
            culture_id: None,
            prestige: 0.0,
        });
        let json = serde_json::to_string(&data).unwrap();
        let back: EntityData = serde_json::from_str(&json).unwrap();
        assert_eq!(data, back);
    }
}
