use std::collections::HashMap;

/// Configuration for world generation.
#[derive(Debug, Clone)]
pub struct WorldGenConfig {
    /// RNG seed for deterministic generation.
    pub seed: u64,
    pub map: MapConfig,
    pub terrain: TerrainConfig,
    pub rivers: RiverConfig,
    /// Extensible: system-specific config as JSON.
    pub extensions: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct MapConfig {
    /// Number of regions to generate.
    pub num_regions: u32,
    /// Map width in abstract units.
    pub width: f64,
    /// Map height in abstract units.
    pub height: f64,
    /// Number of biome seed centers for terrain clustering.
    pub num_biome_centers: u32,
    /// K-nearest neighbors for adjacency graph.
    pub adjacency_k: u32,
}

#[derive(Debug, Clone)]
pub struct TerrainConfig {
    /// Target fraction of regions that are water (0.0–1.0).
    pub water_fraction: f64,
}

#[derive(Debug, Clone)]
pub struct RiverConfig {
    /// Number of rivers to generate.
    pub num_rivers: u32,
}

impl Default for WorldGenConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            map: MapConfig::default(),
            terrain: TerrainConfig::default(),
            rivers: RiverConfig::default(),
            extensions: HashMap::new(),
        }
    }
}

impl Default for MapConfig {
    fn default() -> Self {
        Self {
            num_regions: 25,
            width: 1000.0,
            height: 1000.0,
            num_biome_centers: 6,
            adjacency_k: 4,
        }
    }
}

impl Default for TerrainConfig {
    fn default() -> Self {
        Self {
            water_fraction: 0.2,
        }
    }
}

impl Default for RiverConfig {
    fn default() -> Self {
        Self { num_rivers: 4 }
    }
}
