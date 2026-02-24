use rand::Rng;
use rand::distr::Distribution;

pub use crate::model::terrain::{Terrain, TerrainTag};

impl Distribution<Terrain> for rand::distr::StandardUniform {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> Terrain {
        Terrain::LAND[rng.random_range(0..Terrain::LAND.len())]
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
        assert!(result.unwrap_err().contains("unknown Terrain"));
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
        assert!(result.unwrap_err().contains("unknown TerrainTag"));
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
