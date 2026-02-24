use rand::Rng;
use rand::distr::Distribution;

use serde::{Deserialize, Serialize};

struct TerrainDef {
    str_id: &'static str,
    resources: &'static [&'static str],
    settlement_probability: f64,
    population_range: (u32, u32),
    is_water: bool,
}

struct TerrainTagDef {
    str_id: &'static str,
    settlement_probability_modifier: f64,
    additional_resources: &'static [&'static str],
    population_modifier: f64,
}

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

impl Terrain {
    #[cfg(test)]
    const ALL: [Terrain; 12] = [
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
    const LAND: [Terrain; 10] = [
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

    /// All terrain data in one place.
    fn def(self) -> &'static TerrainDef {
        match self {
            Terrain::Plains => &TerrainDef {
                str_id: "plains",
                resources: &["grain", "horses", "cattle"],
                settlement_probability: 0.8,
                population_range: (200, 800),
                is_water: false,
            },
            Terrain::Forest => &TerrainDef {
                str_id: "forest",
                resources: &["timber", "game", "herbs"],
                settlement_probability: 0.5,
                population_range: (100, 400),
                is_water: false,
            },
            Terrain::Mountains => &TerrainDef {
                str_id: "mountains",
                resources: &["iron", "stone", "gems"],
                settlement_probability: 0.3,
                population_range: (50, 200),
                is_water: false,
            },
            Terrain::Hills => &TerrainDef {
                str_id: "hills",
                resources: &["copper", "clay", "sheep"],
                settlement_probability: 0.6,
                population_range: (100, 500),
                is_water: false,
            },
            Terrain::Desert => &TerrainDef {
                str_id: "desert",
                resources: &["salt", "gold", "glass"],
                settlement_probability: 0.2,
                population_range: (50, 150),
                is_water: false,
            },
            Terrain::Swamp => &TerrainDef {
                str_id: "swamp",
                resources: &["peat", "fish", "herbs"],
                settlement_probability: 0.2,
                population_range: (30, 120),
                is_water: false,
            },
            Terrain::Coast => &TerrainDef {
                str_id: "coast",
                resources: &["fish", "salt", "pearls"],
                settlement_probability: 0.7,
                population_range: (200, 700),
                is_water: false,
            },
            Terrain::Tundra => &TerrainDef {
                str_id: "tundra",
                resources: &["furs", "ivory", "stone"],
                settlement_probability: 0.15,
                population_range: (20, 100),
                is_water: false,
            },
            Terrain::Jungle => &TerrainDef {
                str_id: "jungle",
                resources: &["spices", "timber", "dyes"],
                settlement_probability: 0.25,
                population_range: (50, 200),
                is_water: false,
            },
            Terrain::Volcanic => &TerrainDef {
                str_id: "volcanic",
                resources: &["obsidian", "sulfur", "gems"],
                settlement_probability: 0.1,
                population_range: (20, 80),
                is_water: false,
            },
            Terrain::ShallowWater => &TerrainDef {
                str_id: "shallow_water",
                resources: &["fish", "salt", "pearls"],
                settlement_probability: 0.05,
                population_range: (10, 50),
                is_water: true,
            },
            Terrain::DeepWater => &TerrainDef {
                str_id: "deep_water",
                resources: &["fish", "whales"],
                settlement_probability: 0.0,
                population_range: (0, 0),
                is_water: true,
            },
        }
    }

    pub fn as_str(self) -> &'static str {
        self.def().str_id
    }

    pub fn is_water(self) -> bool {
        self.def().is_water
    }

    /// Default resources available in this terrain type.
    fn resources(self) -> &'static [&'static str] {
        self.def().resources
    }

    /// Probability that a settlement will form in this terrain (0.0–1.0).
    fn settlement_probability(self) -> f64 {
        self.def().settlement_probability
    }

    /// Base population range (min, max) for settlements in this terrain.
    fn population_range(self) -> (u32, u32) {
        self.def().population_range
    }
}

impl From<Terrain> for String {
    fn from(terrain: Terrain) -> Self {
        terrain.as_str().to_string()
    }
}

impl TryFrom<String> for Terrain {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        match s.as_str() {
            "plains" => Ok(Terrain::Plains),
            "forest" => Ok(Terrain::Forest),
            "mountains" => Ok(Terrain::Mountains),
            "hills" => Ok(Terrain::Hills),
            "desert" => Ok(Terrain::Desert),
            "swamp" => Ok(Terrain::Swamp),
            "coast" => Ok(Terrain::Coast),
            "tundra" => Ok(Terrain::Tundra),
            "jungle" => Ok(Terrain::Jungle),
            "volcanic" => Ok(Terrain::Volcanic),
            "shallow_water" => Ok(Terrain::ShallowWater),
            "deep_water" => Ok(Terrain::DeepWater),
            _ => Err(format!("unknown terrain: {s}")),
        }
    }
}

impl Distribution<Terrain> for rand::distr::StandardUniform {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> Terrain {
        Terrain::LAND[rng.random_range(0..Terrain::LAND.len())]
    }
}

// --- TerrainTag ---

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

impl TerrainTag {
    #[cfg(test)]
    const ALL: [TerrainTag; 9] = [
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

    /// All terrain tag data in one place.
    fn def(self) -> &'static TerrainTagDef {
        match self {
            TerrainTag::Forested => &TerrainTagDef {
                str_id: "forested",
                settlement_probability_modifier: 1.10,
                additional_resources: &["timber"],
                population_modifier: 1.0,
            },
            TerrainTag::Coastal => &TerrainTagDef {
                str_id: "coastal",
                settlement_probability_modifier: 1.15,
                additional_resources: &["salt", "fish"],
                population_modifier: 1.0,
            },
            TerrainTag::Riverine => &TerrainTagDef {
                str_id: "riverine",
                settlement_probability_modifier: 1.15,
                additional_resources: &["fish", "freshwater"],
                population_modifier: 1.0,
            },
            TerrainTag::Fertile => &TerrainTagDef {
                str_id: "fertile",
                settlement_probability_modifier: 1.20,
                additional_resources: &[],
                population_modifier: 1.30,
            },
            TerrainTag::Arid => &TerrainTagDef {
                str_id: "arid",
                settlement_probability_modifier: 0.70,
                additional_resources: &[],
                population_modifier: 0.60,
            },
            TerrainTag::Mineral => &TerrainTagDef {
                str_id: "mineral",
                settlement_probability_modifier: 1.0,
                additional_resources: &["ore"],
                population_modifier: 1.0,
            },
            TerrainTag::Sacred => &TerrainTagDef {
                str_id: "sacred",
                settlement_probability_modifier: 1.0,
                additional_resources: &[],
                population_modifier: 1.0,
            },
            TerrainTag::Rugged => &TerrainTagDef {
                str_id: "rugged",
                settlement_probability_modifier: 0.60,
                additional_resources: &[],
                population_modifier: 1.0,
            },
            TerrainTag::Sheltered => &TerrainTagDef {
                str_id: "sheltered",
                settlement_probability_modifier: 1.10,
                additional_resources: &[],
                population_modifier: 1.0,
            },
        }
    }

    pub fn as_str(self) -> &'static str {
        self.def().str_id
    }

    /// Multiplicative modifier to settlement probability.
    fn settlement_probability_modifier(self) -> f64 {
        self.def().settlement_probability_modifier
    }

    /// Additional resources granted by this tag.
    fn additional_resources(self) -> &'static [&'static str] {
        self.def().additional_resources
    }

    /// Multiplicative modifier to population range.
    fn population_modifier(self) -> f64 {
        self.def().population_modifier
    }
}

impl From<TerrainTag> for String {
    fn from(tag: TerrainTag) -> Self {
        tag.as_str().to_string()
    }
}

impl TryFrom<String> for TerrainTag {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        match s.as_str() {
            "forested" => Ok(TerrainTag::Forested),
            "coastal" => Ok(TerrainTag::Coastal),
            "riverine" => Ok(TerrainTag::Riverine),
            "fertile" => Ok(TerrainTag::Fertile),
            "arid" => Ok(TerrainTag::Arid),
            "mineral" => Ok(TerrainTag::Mineral),
            "sacred" => Ok(TerrainTag::Sacred),
            "rugged" => Ok(TerrainTag::Rugged),
            "sheltered" => Ok(TerrainTag::Sheltered),
            _ => Err(format!("unknown terrain tag: {s}")),
        }
    }
}

// --- TerrainProfile ---

#[derive(Debug, Clone)]
pub struct TerrainProfile {
    pub base: Terrain,
    pub tags: Vec<TerrainTag>,
}

impl TerrainProfile {
    pub fn new(base: Terrain, tags: Vec<TerrainTag>) -> Self {
        Self { base, tags }
    }

    /// Base probability × product of tag modifiers, clamped to [0, 1].
    pub fn effective_settlement_probability(&self) -> f64 {
        let base = self.base.settlement_probability();
        let modifier: f64 = self
            .tags
            .iter()
            .map(|t| t.settlement_probability_modifier())
            .product();
        (base * modifier).clamp(0.0, 1.0)
    }

    /// Base resources + tag additional resources, deduplicated.
    pub fn effective_resources(&self) -> Vec<&'static str> {
        let mut resources: Vec<&'static str> = self.base.resources().to_vec();
        for tag in &self.tags {
            for &r in tag.additional_resources() {
                if !resources.contains(&r) {
                    resources.push(r);
                }
            }
        }
        resources
    }

    /// Base range scaled by product of tag population modifiers.
    pub fn effective_population_range(&self) -> (u32, u32) {
        let (min, max) = self.base.population_range();
        let modifier: f64 = self.tags.iter().map(|t| t.population_modifier()).product();
        let scaled_min = (min as f64 * modifier) as u32;
        let scaled_max = (max as f64 * modifier).max(scaled_min as f64) as u32;
        (scaled_min, scaled_max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_terrains_have_resources() {
        for terrain in Terrain::ALL {
            assert!(
                !terrain.resources().is_empty(),
                "{:?} should have resources",
                terrain
            );
        }
    }

    #[test]
    fn settlement_probabilities_are_valid() {
        for terrain in Terrain::ALL {
            let p = terrain.settlement_probability();
            assert!(
                (0.0..=1.0).contains(&p),
                "{:?} probability {} out of range",
                terrain,
                p
            );
        }
    }

    #[test]
    fn population_ranges_are_valid() {
        for terrain in Terrain::ALL {
            let (min, max) = terrain.population_range();
            assert!(min <= max, "{:?} min {} > max {}", terrain, min, max);
        }
    }

    #[test]
    fn as_str_round_trips() {
        for terrain in Terrain::ALL {
            let s = terrain.as_str();
            assert!(!s.is_empty());
            assert!(
                s.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "{:?} as_str should be lowercase/underscore: {}",
                terrain,
                s
            );
        }
    }

    #[test]
    fn random_terrain_is_valid() {
        let mut rng = rand::rng();
        for _ in 0..100 {
            let t: Terrain = rng.random();
            assert!(Terrain::LAND.contains(&t), "random terrain should be land");
        }
    }

    #[test]
    fn terrain_serde_round_trip() {
        for terrain in Terrain::ALL {
            let json = serde_json::to_string(&terrain).unwrap();
            let back: Terrain = serde_json::from_str(&json).unwrap();
            assert_eq!(back, terrain);
        }
    }

    #[test]
    fn terrain_try_from_unknown_errors() {
        let result = Terrain::try_from("atlantis".to_string());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown terrain"));
    }

    #[test]
    fn water_terrain_properties() {
        assert!(Terrain::ShallowWater.is_water());
        assert!(Terrain::DeepWater.is_water());
        assert!(!Terrain::Plains.is_water());
        assert_eq!(Terrain::DeepWater.settlement_probability(), 0.0);
        assert_eq!(Terrain::DeepWater.population_range(), (0, 0));
    }

    // --- TerrainTag tests ---

    #[test]
    fn terrain_tag_serde_round_trip() {
        for tag in TerrainTag::ALL {
            let json = serde_json::to_string(&tag).unwrap();
            let back: TerrainTag = serde_json::from_str(&json).unwrap();
            assert_eq!(back, tag);
        }
    }

    #[test]
    fn terrain_tag_try_from_unknown_errors() {
        let result = TerrainTag::try_from("magical".to_string());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown terrain tag"));
    }

    #[test]
    fn terrain_tag_modifiers_are_valid() {
        for tag in TerrainTag::ALL {
            let m = tag.settlement_probability_modifier();
            assert!(m > 0.0, "{:?} modifier must be positive", tag);
            let p = tag.population_modifier();
            assert!(p > 0.0, "{:?} population modifier must be positive", tag);
        }
    }

    // --- TerrainProfile tests ---

    #[test]
    fn profile_no_tags_matches_base() {
        let profile = TerrainProfile::new(Terrain::Plains, vec![]);
        assert_eq!(
            profile.effective_settlement_probability(),
            Terrain::Plains.settlement_probability()
        );
        assert_eq!(
            profile.effective_resources(),
            Terrain::Plains.resources().to_vec()
        );
        assert_eq!(
            profile.effective_population_range(),
            Terrain::Plains.population_range()
        );
    }

    #[test]
    fn profile_tags_modify_probability() {
        let profile = TerrainProfile::new(Terrain::Plains, vec![TerrainTag::Fertile]);
        let expected = (0.8_f64 * 1.20).clamp(0.0, 1.0);
        assert!(
            (profile.effective_settlement_probability() - expected).abs() < 1e-10,
            "expected {}, got {}",
            expected,
            profile.effective_settlement_probability()
        );
    }

    #[test]
    fn profile_tags_add_resources() {
        let profile = TerrainProfile::new(Terrain::Hills, vec![TerrainTag::Coastal]);
        let resources = profile.effective_resources();
        assert!(resources.contains(&"salt"));
        assert!(resources.contains(&"fish"));
        assert!(resources.contains(&"copper")); // base Hills resource
    }

    #[test]
    fn profile_resources_deduplicated() {
        // Coast already has "salt" and "fish"; Coastal tag also adds them
        let profile = TerrainProfile::new(Terrain::Coast, vec![TerrainTag::Coastal]);
        let resources = profile.effective_resources();
        let salt_count = resources.iter().filter(|&&r| r == "salt").count();
        assert_eq!(salt_count, 1, "salt should not be duplicated");
    }

    #[test]
    fn profile_population_scaled_by_tags() {
        let profile = TerrainProfile::new(Terrain::Plains, vec![TerrainTag::Fertile]);
        let (min, max) = profile.effective_population_range();
        // Plains base: (200, 800), Fertile modifier: 1.30
        assert_eq!(min, (200.0 * 1.30) as u32);
        assert_eq!(max, (800.0 * 1.30) as u32);
    }

    #[test]
    fn profile_probability_clamped() {
        // Stack many positive modifiers to test clamping
        let profile = TerrainProfile::new(
            Terrain::Plains,
            vec![
                TerrainTag::Fertile,
                TerrainTag::Coastal,
                TerrainTag::Riverine,
            ],
        );
        let p = profile.effective_settlement_probability();
        assert!(p <= 1.0, "probability should be clamped to 1.0, got {}", p);
    }
}
