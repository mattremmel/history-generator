use rand::Rng;
use rand::distr::Distribution;

use serde::{Deserialize, Serialize};

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

    pub fn as_str(self) -> &'static str {
        match self {
            Terrain::Plains => "plains",
            Terrain::Forest => "forest",
            Terrain::Mountains => "mountains",
            Terrain::Hills => "hills",
            Terrain::Desert => "desert",
            Terrain::Swamp => "swamp",
            Terrain::Coast => "coast",
            Terrain::Tundra => "tundra",
            Terrain::Jungle => "jungle",
            Terrain::Volcanic => "volcanic",
            Terrain::ShallowWater => "shallow_water",
            Terrain::DeepWater => "deep_water",
        }
    }

    pub fn is_water(self) -> bool {
        matches!(self, Terrain::ShallowWater | Terrain::DeepWater)
    }

    /// Default resources available in this terrain type.
    pub fn resources(self) -> &'static [&'static str] {
        match self {
            Terrain::Plains => &["grain", "horses", "cattle"],
            Terrain::Forest => &["timber", "game", "herbs"],
            Terrain::Mountains => &["iron", "stone", "gems"],
            Terrain::Hills => &["copper", "clay", "sheep"],
            Terrain::Desert => &["salt", "gold", "glass"],
            Terrain::Swamp => &["peat", "fish", "herbs"],
            Terrain::Coast => &["fish", "salt", "pearls"],
            Terrain::Tundra => &["furs", "ivory", "stone"],
            Terrain::Jungle => &["spices", "timber", "dyes"],
            Terrain::Volcanic => &["obsidian", "sulfur", "gems"],
            Terrain::ShallowWater => &["fish", "salt", "pearls"],
            Terrain::DeepWater => &["fish", "whales"],
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

    pub fn as_str(self) -> &'static str {
        match self {
            TerrainTag::Forested => "forested",
            TerrainTag::Coastal => "coastal",
            TerrainTag::Riverine => "riverine",
            TerrainTag::Fertile => "fertile",
            TerrainTag::Arid => "arid",
            TerrainTag::Mineral => "mineral",
            TerrainTag::Sacred => "sacred",
            TerrainTag::Rugged => "rugged",
            TerrainTag::Sheltered => "sheltered",
        }
    }

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
    pub fn additional_resources(self) -> &'static [&'static str] {
        match self {
            TerrainTag::Forested => &["timber"],
            TerrainTag::Coastal => &["salt", "fish"],
            TerrainTag::Riverine => &["fish", "freshwater"],
            TerrainTag::Fertile => &[],
            TerrainTag::Arid => &[],
            TerrainTag::Mineral => &["ore"],
            TerrainTag::Sacred => &[],
            TerrainTag::Rugged => &[],
            TerrainTag::Sheltered => &[],
        }
    }

    /// Multiplicative modifier to population range.
    pub fn population_modifier(self) -> f64 {
        match self {
            TerrainTag::Forested => 1.0,
            TerrainTag::Coastal => 1.0,
            TerrainTag::Riverine => 1.0,
            TerrainTag::Fertile => 1.30,
            TerrainTag::Arid => 0.60,
            TerrainTag::Mineral => 1.0,
            TerrainTag::Sacred => 1.0,
            TerrainTag::Rugged => 1.0,
            TerrainTag::Sheltered => 1.0,
        }
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
