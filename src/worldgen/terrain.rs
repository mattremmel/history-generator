use rand::Rng;
use rand::distr::Distribution;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
}

impl Terrain {
    pub const ALL: [Terrain; 10] = [
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
        }
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
        }
    }
}

impl Distribution<Terrain> for rand::distr::StandardUniform {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> Terrain {
        Terrain::ALL[rng.random_range(0..Terrain::ALL.len())]
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
            assert!(min < max, "{:?} min {} >= max {}", terrain, min, max);
        }
    }

    #[test]
    fn as_str_round_trips() {
        for terrain in Terrain::ALL {
            let s = terrain.as_str();
            assert!(!s.is_empty());
            assert!(
                s.chars().all(|c| c.is_ascii_lowercase()),
                "{:?} as_str should be lowercase: {}",
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
            assert!(Terrain::ALL.contains(&t));
        }
    }
}
