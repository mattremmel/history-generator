/// Configuration for world generation.
pub struct WorldGenConfig {
    /// RNG seed for deterministic generation.
    pub seed: u64,
    /// Number of regions to generate.
    pub num_regions: u32,
    /// Map width in abstract units.
    pub map_width: f64,
    /// Map height in abstract units.
    pub map_height: f64,
    /// Number of biome seed centers for terrain clustering.
    pub num_biome_centers: u32,
    /// K-nearest neighbors for adjacency graph.
    pub adjacency_k: u32,
}

impl Default for WorldGenConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            num_regions: 25,
            map_width: 1000.0,
            map_height: 1000.0,
            num_biome_centers: 6,
            adjacency_k: 4,
        }
    }
}
