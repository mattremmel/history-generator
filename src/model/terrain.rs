use serde::{Deserialize, Serialize};

use super::entity_data::ResourceType;

// ---------------------------------------------------------------------------
// Terrain
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum Terrain {
    Plains,
    Forest,
    Mountains,
    Hills,
    Desert,
    Swamp,
    Coast,
    Tundra,
    Jungle,
    Volcanic,
    ShallowWater,
    DeepWater,
}

string_enum!(Terrain {
    Plains => "plains",
    Forest => "forest",
    Mountains => "mountains",
    Hills => "hills",
    Desert => "desert",
    Swamp => "swamp",
    Coast => "coast",
    Tundra => "tundra",
    Jungle => "jungle",
    Volcanic => "volcanic",
    ShallowWater => "shallow_water",
    DeepWater => "deep_water",
});

impl Terrain {
    pub const ALL: [Terrain; 12] = [
        Terrain::Plains,
        Terrain::Forest,
        Terrain::Mountains,
        Terrain::Hills,
        Terrain::Desert,
        Terrain::Swamp,
        Terrain::Coast,
        Terrain::Tundra,
        Terrain::Jungle,
        Terrain::Volcanic,
        Terrain::ShallowWater,
        Terrain::DeepWater,
    ];

    /// Land terrain types only (excludes water).
    pub const LAND: [Terrain; 10] = [
        Terrain::Plains,
        Terrain::Forest,
        Terrain::Mountains,
        Terrain::Hills,
        Terrain::Desert,
        Terrain::Swamp,
        Terrain::Coast,
        Terrain::Tundra,
        Terrain::Jungle,
        Terrain::Volcanic,
    ];

    pub fn is_water(self) -> bool {
        matches!(self, Terrain::ShallowWater | Terrain::DeepWater)
    }

    /// Default resources available in this terrain type.
    pub fn resources(self) -> &'static [ResourceType] {
        match self {
            Terrain::Plains => &[
                ResourceType::Grain,
                ResourceType::Horses,
                ResourceType::Cattle,
            ],
            Terrain::Forest => &[
                ResourceType::Timber,
                ResourceType::Game,
                ResourceType::Herbs,
            ],
            Terrain::Mountains => &[ResourceType::Iron, ResourceType::Stone, ResourceType::Gems],
            Terrain::Hills => &[
                ResourceType::Copper,
                ResourceType::Clay,
                ResourceType::Sheep,
            ],
            Terrain::Desert => &[ResourceType::Salt, ResourceType::Gold, ResourceType::Glass],
            Terrain::Swamp => &[ResourceType::Peat, ResourceType::Fish, ResourceType::Herbs],
            Terrain::Coast => &[ResourceType::Fish, ResourceType::Salt, ResourceType::Pearls],
            Terrain::Tundra => &[ResourceType::Furs, ResourceType::Ivory, ResourceType::Stone],
            Terrain::Jungle => &[
                ResourceType::Spices,
                ResourceType::Timber,
                ResourceType::Dyes,
            ],
            Terrain::Volcanic => &[
                ResourceType::Obsidian,
                ResourceType::Sulfur,
                ResourceType::Gems,
            ],
            Terrain::ShallowWater => {
                &[ResourceType::Fish, ResourceType::Salt, ResourceType::Pearls]
            }
            Terrain::DeepWater => &[ResourceType::Fish, ResourceType::Whales],
        }
    }

    /// Probability that a settlement will form in this terrain (0.0–1.0).
    pub fn settlement_probability(self) -> f64 {
        match self {
            Terrain::Plains => 0.8,
            Terrain::Forest => 0.5,
            Terrain::Mountains => 0.3,
            Terrain::Hills => 0.6,
            Terrain::Desert => 0.2,
            Terrain::Swamp => 0.2,
            Terrain::Coast => 0.7,
            Terrain::Tundra => 0.15,
            Terrain::Jungle => 0.25,
            Terrain::Volcanic => 0.1,
            Terrain::ShallowWater => 0.05,
            Terrain::DeepWater => 0.0,
        }
    }

    /// Base population range (min, max) for settlements in this terrain.
    pub fn population_range(self) -> (u32, u32) {
        match self {
            Terrain::Plains => (200, 800),
            Terrain::Forest => (100, 400),
            Terrain::Mountains => (50, 200),
            Terrain::Hills => (100, 500),
            Terrain::Desert => (50, 150),
            Terrain::Swamp => (30, 120),
            Terrain::Coast => (200, 700),
            Terrain::Tundra => (20, 100),
            Terrain::Jungle => (50, 200),
            Terrain::Volcanic => (20, 80),
            Terrain::ShallowWater => (10, 50),
            Terrain::DeepWater => (0, 0),
        }
    }
}

// ---------------------------------------------------------------------------
// TerrainTag
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum TerrainTag {
    Forested,
    Coastal,
    Riverine,
    Fertile,
    Arid,
    Mineral,
    Sacred,
    Rugged,
    Sheltered,
}

string_enum!(TerrainTag {
    Forested => "forested",
    Coastal => "coastal",
    Riverine => "riverine",
    Fertile => "fertile",
    Arid => "arid",
    Mineral => "mineral",
    Sacred => "sacred",
    Rugged => "rugged",
    Sheltered => "sheltered",
});

impl TerrainTag {
    pub const ALL: [TerrainTag; 9] = [
        TerrainTag::Forested,
        TerrainTag::Coastal,
        TerrainTag::Riverine,
        TerrainTag::Fertile,
        TerrainTag::Arid,
        TerrainTag::Mineral,
        TerrainTag::Sacred,
        TerrainTag::Rugged,
        TerrainTag::Sheltered,
    ];

    /// Multiplicative modifier to settlement probability.
    pub fn settlement_probability_modifier(self) -> f64 {
        match self {
            TerrainTag::Forested => 1.10,
            TerrainTag::Coastal => 1.15,
            TerrainTag::Riverine => 1.15,
            TerrainTag::Fertile => 1.20,
            TerrainTag::Arid => 0.70,
            TerrainTag::Mineral => 1.0,
            TerrainTag::Sacred => 1.0,
            TerrainTag::Rugged => 0.60,
            TerrainTag::Sheltered => 1.10,
        }
    }

    /// Additional resources granted by this tag.
    pub fn additional_resources(self) -> &'static [ResourceType] {
        match self {
            TerrainTag::Forested => &[ResourceType::Timber],
            TerrainTag::Coastal => &[ResourceType::Salt, ResourceType::Fish],
            TerrainTag::Riverine => &[ResourceType::Fish, ResourceType::Freshwater],
            TerrainTag::Mineral => &[ResourceType::Ore],
            _ => &[],
        }
    }

    /// Multiplicative modifier to population range.
    pub fn population_modifier(self) -> f64 {
        match self {
            TerrainTag::Fertile => 1.30,
            TerrainTag::Arid => 0.60,
            _ => 1.0,
        }
    }
}
