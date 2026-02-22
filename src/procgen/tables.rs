/// Occupation definition with base weight and resource affinity.
pub struct OccupationDef {
    pub name: &'static str,
    pub weight: u32,
    /// Resources that double this occupation's weight when present.
    pub resource_affinity: &'static [&'static str],
}

pub const OCCUPATIONS: &[OccupationDef] = &[
    OccupationDef { name: "farmer", weight: 30, resource_affinity: &["grain", "cattle"] },
    OccupationDef { name: "laborer", weight: 20, resource_affinity: &[] },
    OccupationDef { name: "craftsman", weight: 12, resource_affinity: &["timber", "clay"] },
    OccupationDef { name: "miner", weight: 5, resource_affinity: &["iron", "copper", "gems", "stone"] },
    OccupationDef { name: "fisher", weight: 8, resource_affinity: &["fish"] },
    OccupationDef { name: "merchant", weight: 6, resource_affinity: &[] },
    OccupationDef { name: "soldier", weight: 5, resource_affinity: &[] },
    OccupationDef { name: "herbalist", weight: 4, resource_affinity: &["herbs"] },
    OccupationDef { name: "shepherd", weight: 6, resource_affinity: &["sheep", "cattle"] },
    OccupationDef { name: "woodcutter", weight: 5, resource_affinity: &["timber"] },
    OccupationDef { name: "hunter", weight: 5, resource_affinity: &["game", "furs"] },
    OccupationDef { name: "tanner", weight: 3, resource_affinity: &["furs", "game"] },
    OccupationDef { name: "potter", weight: 3, resource_affinity: &["clay"] },
    OccupationDef { name: "smith", weight: 4, resource_affinity: &["iron", "copper"] },
    OccupationDef { name: "brewer", weight: 3, resource_affinity: &["grain"] },
    OccupationDef { name: "priest", weight: 2, resource_affinity: &[] },
    OccupationDef { name: "scribe", weight: 1, resource_affinity: &[] },
];

/// Artifact material mapped from settlement resource name.
pub struct MaterialMapping {
    pub material: &'static str,
    pub from_resources: &'static [&'static str],
}

pub const MATERIAL_MAPPINGS: &[MaterialMapping] = &[
    MaterialMapping { material: "iron", from_resources: &["iron"] },
    MaterialMapping { material: "copper", from_resources: &["copper"] },
    MaterialMapping { material: "stone", from_resources: &["stone"] },
    MaterialMapping { material: "wood", from_resources: &["timber"] },
    MaterialMapping { material: "clay", from_resources: &["clay"] },
    MaterialMapping { material: "bone", from_resources: &["game"] },
    MaterialMapping { material: "obsidian", from_resources: &["obsidian"] },
    MaterialMapping { material: "gold", from_resources: &["gold"] },
    MaterialMapping { material: "leather", from_resources: &["furs"] },
    MaterialMapping { material: "shell", from_resources: &["pearls"] },
    MaterialMapping { material: "salt", from_resources: &["salt"] },
    MaterialMapping { material: "glass", from_resources: &["glass"] },
];

/// Universal materials always available regardless of resources.
pub const UNIVERSAL_MATERIALS: &[&str] = &["stone", "wood", "bone"];

pub const ARTIFACT_TYPES: &[&str] = &[
    "tool", "weapon", "pottery", "jewelry", "idol", "tablet", "chest", "seal", "amulet",
];

pub const TOMBSTONE_TEMPLATES: &[&str] = &[
    "Here lies {name}, {occupation} of {settlement}, who lived {age} years",
    "In memory of {name}, taken by the gods in the year {year}",
    "Rest eternal, {name}. {settlement} remembers",
    "{name}, beloved {occupation}, year {year}",
];

pub const TRADE_RECORD_TEMPLATES: &[&str] = &[
    "Year {year}: {quantity} units of {resource} stored in {settlement}",
    "Ledger of {settlement}, year {year}: surplus {resource} recorded",
    "Contract: {name} of {settlement} supplies {resource} for {years} years",
    "Year {year}: {settlement} trades {resource} with neighboring settlements",
];

pub const PROCLAMATION_TEMPLATES: &[&str] = &[
    "By decree of {settlement}, year {year}: all {occupation}s shall tithe",
    "Let it be known: {settlement} claims dominion over surrounding {terrain}",
    "In the year {year}, {settlement} declares a festival of {resource}",
    "Year {year}: the council of {settlement} establishes new laws for {occupation}s",
];

/// Select an occupation using weighted random, boosting weights for resource affinity matches.
pub fn select_occupation(resources: &[String], rng: &mut dyn rand::RngCore) -> &'static str {
    use rand::Rng;

    let weights: Vec<u32> = OCCUPATIONS
        .iter()
        .map(|occ| {
            let has_affinity = occ
                .resource_affinity
                .iter()
                .any(|r| resources.iter().any(|sr| sr == r));
            if has_affinity { occ.weight * 2 } else { occ.weight }
        })
        .collect();

    let total: u32 = weights.iter().sum();
    let roll = rng.random_range(0..total);
    let mut cumulative = 0;
    for (i, &w) in weights.iter().enumerate() {
        cumulative += w;
        if roll < cumulative {
            return OCCUPATIONS[i].name;
        }
    }
    OCCUPATIONS[0].name
}

/// Collect available materials for a settlement based on its resources.
pub fn available_materials(resources: &[String]) -> Vec<&'static str> {
    let mut materials: Vec<&'static str> = Vec::new();

    for mapping in MATERIAL_MAPPINGS {
        if mapping
            .from_resources
            .iter()
            .any(|r| resources.iter().any(|sr| sr == r))
        {
            materials.push(mapping.material);
        }
    }

    // Add universal materials if not already present
    for &m in UNIVERSAL_MATERIALS {
        if !materials.contains(&m) {
            materials.push(m);
        }
    }

    materials
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_occupation_weights_positive() {
        for occ in OCCUPATIONS {
            assert!(occ.weight > 0, "occupation {} has zero weight", occ.name);
        }
    }

    #[test]
    fn templates_have_placeholders() {
        for t in TOMBSTONE_TEMPLATES {
            assert!(t.contains('{'), "tombstone template missing placeholder: {t}");
        }
        for t in TRADE_RECORD_TEMPLATES {
            assert!(t.contains('{'), "trade record template missing placeholder: {t}");
        }
        for t in PROCLAMATION_TEMPLATES {
            assert!(
                t.contains('{'),
                "proclamation template missing placeholder: {t}"
            );
        }
    }

    #[test]
    fn universal_materials_always_present() {
        let materials = available_materials(&[]);
        for &m in UNIVERSAL_MATERIALS {
            assert!(materials.contains(&m), "missing universal material: {m}");
        }
    }

    #[test]
    fn resource_adds_materials() {
        let resources = vec!["iron".to_string(), "timber".to_string()];
        let materials = available_materials(&resources);
        assert!(materials.contains(&"iron"));
        assert!(materials.contains(&"wood"));
    }

    #[test]
    fn select_occupation_returns_valid() {
        use rand::SeedableRng;
        use rand::rngs::SmallRng;
        let mut rng = SmallRng::seed_from_u64(42);
        let resources = vec!["iron".to_string()];
        for _ in 0..100 {
            let occ = select_occupation(&resources, &mut rng);
            assert!(
                OCCUPATIONS.iter().any(|o| o.name == occ),
                "invalid occupation: {occ}"
            );
        }
    }
}
