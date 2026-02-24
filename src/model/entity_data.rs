use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::cultural_value::{CulturalValue, NamingStyle};
use super::entity::EntityKind;
use super::population::{NUM_BRACKETS, PopulationBreakdown};
use super::terrain::{Terrain, TerrainTag};
use super::traits::Trait;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum Sex {
    Male,
    Female,
}

string_enum!(Sex {
    Male => "male",
    Female => "female",
});

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum Role {
    Common,
    Warrior,
    Scholar,
    Merchant,
    Artisan,
    Elder,
    Custom(String),
}

string_enum_open!(Role, "Role", {
    Common => "common",
    Warrior => "warrior",
    Scholar => "scholar",
    Merchant => "merchant",
    Artisan => "artisan",
    Elder => "elder",
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
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

string_enum!(BuildingType {
    Mine => "mine",
    Port => "port",
    Market => "market",
    Granary => "granary",
    Temple => "temple",
    Workshop => "workshop",
    Aqueduct => "aqueduct",
    Library => "library",
});

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PersonData {
    pub birth_year: u32,
    pub sex: Sex,
    pub role: Role,
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

impl SettlementData {
    /// Synchronize `self.population` with the total from `self.population_breakdown`.
    pub fn sync_population(&mut self) {
        self.population = self.population_breakdown.total();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActiveSiege {
    pub attacker_army_id: u64,
    pub attacker_faction_id: u64,
    pub started_year: u32,
    pub started_month: u32,
    pub months_elapsed: u32,
    pub civilian_deaths: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum DisasterType {
    Earthquake,
    Flood,
    Drought,
    VolcanicEruption,
    Wildfire,
    Storm,
    Tsunami,
}

string_enum!(DisasterType {
    Earthquake => "earthquake",
    Flood => "flood",
    Drought => "drought",
    VolcanicEruption => "volcanic_eruption",
    Wildfire => "wildfire",
    Storm => "storm",
    Tsunami => "tsunami",
});

impl DisasterType {
    /// Returns true if this disaster type persists across multiple months.
    pub fn is_persistent(&self) -> bool {
        matches!(
            self,
            DisasterType::Drought | DisasterType::Flood | DisasterType::Wildfire
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum SiegeOutcome {
    Conquered,
    Lifted,
    Abandoned,
}

string_enum!(SiegeOutcome {
    Conquered => "conquered",
    Lifted => "lifted",
    Abandoned => "abandoned",
});

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum GovernmentType {
    Hereditary,
    Elective,
    Chieftain,
}

string_enum!(GovernmentType {
    Hereditary => "hereditary",
    Elective => "elective",
    Chieftain => "chieftain",
});

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FactionData {
    pub government_type: GovernmentType,
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
    pub terrain: Terrain,
    #[serde(default)]
    pub terrain_tags: Vec<TerrainTag>,
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum FeatureType {
    Cave,
    MountainPass,
    Clearing,
    Grove,
    Sinkhole,
    HotSpring,
    LavaTube,
    Harbor,
    LavaField,
    FaultLine,
    Crater,
    Custom(String),
}

string_enum_open!(FeatureType, "feature type", {
    Cave => "cave",
    MountainPass => "mountain_pass",
    Clearing => "clearing",
    Grove => "grove",
    Sinkhole => "sinkhole",
    HotSpring => "hot_spring",
    LavaTube => "lava_tube",
    Harbor => "harbor",
    LavaField => "lava_field",
    FaultLine => "fault_line",
    Crater => "crater",
});

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeographicFeatureData {
    pub feature_type: FeatureType,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum ResourceType {
    Grain,
    Timber,
    Game,
    Horses,
    Cattle,
    Sheep,
    Herbs,
    Peat,
    Furs,
    Freshwater,
    Iron,
    Stone,
    Copper,
    Gold,
    Gems,
    Obsidian,
    Sulfur,
    Clay,
    Glass,
    Ivory,
    Ore,
    Salt,
    Pearls,
    Spices,
    Dyes,
    Fish,
    Whales,
    Custom(String),
}

string_enum_open!(ResourceType, "resource type", {
    Grain => "grain",
    Timber => "timber",
    Game => "game",
    Horses => "horses",
    Cattle => "cattle",
    Sheep => "sheep",
    Herbs => "herbs",
    Peat => "peat",
    Furs => "furs",
    Freshwater => "freshwater",
    Iron => "iron",
    Stone => "stone",
    Copper => "copper",
    Gold => "gold",
    Gems => "gems",
    Obsidian => "obsidian",
    Sulfur => "sulfur",
    Clay => "clay",
    Glass => "glass",
    Ivory => "ivory",
    Ore => "ore",
    Salt => "salt",
    Pearls => "pearls",
    Spices => "spices",
    Dyes => "dyes",
    Fish => "fish",
    Whales => "whales",
});

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResourceDepositData {
    pub resource_type: ResourceType,
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
    pub length: u32,
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
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

string_enum!(KnowledgeCategory {
    Battle => "battle",
    Conquest => "conquest",
    Dynasty => "dynasty",
    Disaster => "disaster",
    Founding => "founding",
    Cultural => "cultural",
    Diplomatic => "diplomatic",
    Construction => "construction",
});

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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
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

string_enum!(Medium {
    Memory => "memory",
    OralTradition => "oral_tradition",
    WrittenBook => "written_book",
    Scroll => "scroll",
    CarvedStone => "carved_stone",
    Song => "song",
    Painting => "painting",
    Tapestry => "tapestry",
    Tattoo => "tattoo",
    Dream => "dream",
    MagicalImprint => "magical_imprint",
    EncodedCipher => "encoded_cipher",
});

impl Medium {
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum DerivationMethod {
    #[default]
    Witnessed,
    Copied,
    Memorized,
    Retold,
    TranscribedFromOral,
    SetToMusic,
    Taught,
    Carved,
    MagicallyRecorded,
    Derived,
    Dreamed,
    Custom(String),
}

string_enum_open!(DerivationMethod, "derivation method", {
    Witnessed => "witnessed",
    Copied => "copied",
    Memorized => "memorized",
    Retold => "retold",
    TranscribedFromOral => "transcribed_from_oral",
    SetToMusic => "set_to_music",
    Taught => "taught",
    Carved => "carved",
    MagicallyRecorded => "magically_recorded",
    Derived => "derived",
    Dreamed => "dreamed",
});

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
    /// Applied distortions (each element describes one drift/mutation).
    #[serde(default)]
    pub distortions: Vec<serde_json::Value>,
    /// Parent manifestation entity ID (None = original/eyewitness).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derived_from_id: Option<u64>,
    /// How this manifestation was created.
    #[serde(default)]
    pub derivation_method: DerivationMethod,
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
    pub fn default_for_kind(kind: EntityKind) -> Self {
        match kind {
            EntityKind::Person => EntityData::Person(PersonData {
                birth_year: 0,
                sex: Sex::Male,
                role: Role::Common,
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
                government_type: GovernmentType::Chieftain,
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
                terrain: Terrain::Plains,
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
                feature_type: FeatureType::Crater,
                x: 0.0,
                y: 0.0,
            }),
            EntityKind::ResourceDeposit => EntityData::ResourceDeposit(ResourceDepositData {
                resource_type: ResourceType::Iron,
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
                distortions: Vec::new(),
                derived_from_id: None,
                derivation_method: DerivationMethod::default(),
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
        let data = EntityData::default_for_kind(EntityKind::Person);
        assert!(data.as_person().is_some());
    }

    #[test]
    fn default_for_kind_settlement() {
        let data = EntityData::default_for_kind(EntityKind::Settlement);
        let s = data.as_settlement().unwrap();
        assert_eq!(s.population, 0);
        assert!((s.prosperity - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn default_for_kind_unknown_returns_none() {
        let data = EntityData::default_for_kind(EntityKind::Deity);
        assert!(matches!(data, EntityData::None));
    }

    #[test]
    fn accessor_mut_works() {
        let mut data = EntityData::default_for_kind(EntityKind::Faction);
        data.as_faction_mut().unwrap().stability = 0.9;
        assert!((data.as_faction().unwrap().stability - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn serde_round_trip() {
        let data = EntityData::Person(PersonData {
            birth_year: 100,
            sex: Sex::Male,
            role: Role::Warrior,
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
