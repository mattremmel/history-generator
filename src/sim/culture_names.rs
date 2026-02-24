use rand::Rng;
use rand::RngCore;

use crate::model::cultural_value::NamingStyle;
use crate::model::{EntityKind, World};

use super::names::{
    generate_person_name, generate_person_name_with_surname, generate_unique_person_name, EPITHETS,
};

// --- Nordic: hard consonants, -ric/-ulf ---

const NORDIC_PREFIXES: &[&str] = &[
    "Bjorn", "Thor", "Sig", "Ulf", "Rag", "Frey", "Stein", "Hald", "Brun", "Grim", "Arn", "Sven",
    "Ingr", "Tyr", "Hild", "Vald", "Knut", "Dag", "Eirik", "Gunn", "Alf", "Hroth", "Orm", "Rolf",
    "Vigr",
];

const NORDIC_SUFFIXES: &[&str] = &[
    "ar", "ric", "ulf", "mund", "gar", "nar", "var", "vald", "mar", "geir", "a", "hild", "id",
    "run", "dis",
];

const NORDIC_SURNAMES: &[&str] = &[
    "Stormvald",
    "Ironheim",
    "Frostborn",
    "Blackmount",
    "Wolfsbane",
    "Ravenshield",
    "Greystone",
    "Oakhelm",
    "Thunderfell",
    "Icevein",
    "Steelhammer",
    "Whitepeak",
    "Bouldercrag",
    "Ashwood",
    "Northwind",
];

// --- Elvish: flowing vowels, -iel/-wen ---

const ELVISH_PREFIXES: &[&str] = &[
    "Cael", "Ael", "Thal", "Gal", "Elar", "Sil", "Aur", "Lor", "Ithil", "Fen", "Aran", "Nol",
    "Cel", "Mir", "Alar", "Luth", "Ear", "Hal", "Tar", "Gil", "Fin", "Faer", "Nin", "Glor", "Ber",
];

const ELVISH_SUFFIXES: &[&str] = &[
    "indra", "iel", "wen", "oth", "anor", "iel", "amir", "iel", "as", "ith", "wen", "il", "ia",
    "ondil", "orn",
];

const ELVISH_SURNAMES: &[&str] = &[
    "Silverwen",
    "Starweave",
    "Moonveil",
    "Dawnmist",
    "Leafsong",
    "Crystalbrook",
    "Sunwhisper",
    "Nightbloom",
    "Dewglade",
    "Windharper",
    "Brightmantle",
    "Thornrose",
    "Starhollow",
    "Greenmantle",
    "Shimmerleaf",
];

// --- Desert: Arabic-inspired, -id/-im ---

const DESERT_PREFIXES: &[&str] = &[
    "Rash", "Khal", "Amir", "Zah", "Nas", "Fahr", "Tar", "Shaz", "Mal", "Sar", "Jam", "Had", "Qas",
    "Yaz", "Sal", "Nar", "Bah", "Kar", "Mur", "Zan", "Alk", "Jab", "Raq", "Sad", "Hak",
];

const DESERT_SUFFIXES: &[&str] = &[
    "ara", "id", "im", "ik", "ul", "an", "een", "ad", "ir", "ira", "aya", "ia", "ane", "esh", "um",
];

const DESERT_SURNAMES: &[&str] = &[
    "Khalvane",
    "Sandseer",
    "Duskthorn",
    "Miragecrest",
    "Ashwind",
    "Sunforge",
    "Dunewalker",
    "Emberveil",
    "Scorchmark",
    "Oasisheart",
    "Stormveil",
    "Dawncaller",
    "Ironblaze",
    "Dustweaver",
    "Sandhollow",
];

// --- Steppe: Mongolic, -ur/-khan ---

const STEPPE_PREFIXES: &[&str] = &[
    "Tem", "Bork", "Batu", "Jochi", "Mong", "Sor", "Erg", "Chag", "Sub", "Tog", "Kublai", "Nom",
    "Orda", "Kesh", "Merk", "Tar", "Och", "Esen", "Jam", "Bur", "Khor", "Altan", "Qut", "Bayan",
    "Tul",
];

const STEPPE_SUFFIXES: &[&str] = &[
    "ura", "ai", "khan", "un", "or", "an", "uk", "tu", "ge", "ir", "ei", "in", "ar", "ul", "ag",
];

const STEPPE_SURNAMES: &[&str] = &[
    "Borkhaan",
    "Ironhorde",
    "Swiftsteed",
    "Skyrider",
    "Windbow",
    "Thundermane",
    "Ashensteppe",
    "Eagletalon",
    "Wolftrail",
    "Stoneherd",
    "Flamemarch",
    "Dusthoof",
    "Hawkeye",
    "Ironstride",
    "Longbow",
];

// --- Imperial: Latin-inspired, -us/-ia ---

const IMPERIAL_PREFIXES: &[&str] = &[
    "Aurel", "Maxim", "Cassi", "Valer", "Lucr", "Octav", "Jul", "Sext", "Tull", "Fab", "Marc",
    "Tiber", "Corn", "Serv", "Claud", "Gaer", "Pomp", "Cras", "Decim", "Aemil", "Flav", "Quintil",
    "Silv", "Titul", "Nerv",
];

const IMPERIAL_SUFFIXES: &[&str] = &[
    "ius", "ia", "us", "ar", "is", "es", "ina", "anus", "eus", "ix", "ius", "ax", "ex", "ona",
    "ura",
];

const IMPERIAL_SURNAMES: &[&str] = &[
    "Maximar",
    "Aurelian",
    "Valorcrest",
    "Ironlegate",
    "Crownsward",
    "Goldenhelm",
    "Marbleguard",
    "Steelbrow",
    "Highcrest",
    "Pillarstone",
    "Eaglemark",
    "Grandfort",
    "Sunarch",
    "Greymantle",
    "Stormgate",
];

// --- Sylvan: nature-rooted ---

const SYLVAN_PREFIXES: &[&str] = &[
    "Fern", "Moss", "Briar", "Thorn", "Ash", "Oak", "Elm", "Wil", "Ivy", "Sage", "Glen", "Brook",
    "Reed", "Haze", "Rowan", "Alder", "Holly", "Cedar", "Lark", "Wren", "Birch", "Hazel", "Dew",
    "Mist", "Dale",
];

const SYLVAN_SUFFIXES: &[&str] = &[
    "hollow", "weald", "glade", "wick", "dale", "mere", "brook", "leaf", "wood", "thorn", "song",
    "bloom", "shade", "fall", "glen",
];

const SYLVAN_SURNAMES: &[&str] = &[
    "Mossweald",
    "Fernhollow",
    "Deeproot",
    "Greenbark",
    "Willowmere",
    "Thornbloom",
    "Stonebrook",
    "Acornheart",
    "Oakglade",
    "Pineshade",
    "Dewhollow",
    "Ivyvale",
    "Briarstone",
    "Hazelthorn",
    "Birchfield",
];

struct StyleTables {
    prefixes: &'static [&'static str],
    suffixes: &'static [&'static str],
    surnames: &'static [&'static str],
}

fn tables_for(style: &NamingStyle) -> StyleTables {
    match style {
        NamingStyle::Nordic => StyleTables {
            prefixes: NORDIC_PREFIXES,
            suffixes: NORDIC_SUFFIXES,
            surnames: NORDIC_SURNAMES,
        },
        NamingStyle::Elvish => StyleTables {
            prefixes: ELVISH_PREFIXES,
            suffixes: ELVISH_SUFFIXES,
            surnames: ELVISH_SURNAMES,
        },
        NamingStyle::Desert => StyleTables {
            prefixes: DESERT_PREFIXES,
            suffixes: DESERT_SUFFIXES,
            surnames: DESERT_SURNAMES,
        },
        NamingStyle::Steppe => StyleTables {
            prefixes: STEPPE_PREFIXES,
            suffixes: STEPPE_SUFFIXES,
            surnames: STEPPE_SURNAMES,
        },
        NamingStyle::Imperial => StyleTables {
            prefixes: IMPERIAL_PREFIXES,
            suffixes: IMPERIAL_SUFFIXES,
            surnames: IMPERIAL_SURNAMES,
        },
        NamingStyle::Sylvan => StyleTables {
            prefixes: SYLVAN_PREFIXES,
            suffixes: SYLVAN_SUFFIXES,
            surnames: SYLVAN_SURNAMES,
        },
        NamingStyle::Custom(_) => StyleTables {
            prefixes: &[],
            suffixes: &[],
            surnames: &[],
        },
    }
}

/// Generate a culture-specific person name.
pub fn generate_culture_person_name(style: &NamingStyle, rng: &mut dyn RngCore) -> String {
    let tables = tables_for(style);
    if tables.prefixes.is_empty() {
        return generate_person_name(rng);
    }
    let prefix = tables.prefixes[rng.random_range(0..tables.prefixes.len())];
    let suffix = tables.suffixes[rng.random_range(0..tables.suffixes.len())];
    let surname = tables.surnames[rng.random_range(0..tables.surnames.len())];
    format!("{prefix}{suffix} {surname}")
}

/// Generate a culture-specific person name unique among living persons.
pub fn generate_unique_culture_person_name(
    world: &World,
    style: &NamingStyle,
    rng: &mut dyn RngCore,
) -> String {
    let tables = tables_for(style);
    if tables.prefixes.is_empty() {
        return generate_unique_person_name(world, rng);
    }

    for _ in 0..5 {
        let name = generate_culture_person_name(style, rng);
        let is_taken = world
            .entities
            .values()
            .any(|e| e.kind == EntityKind::Person && e.end.is_none() && e.name == name);
        if !is_taken {
            return name;
        }
    }
    let base = generate_culture_person_name(style, rng);
    let epithet = EPITHETS[rng.random_range(0..EPITHETS.len())];
    format!("{base} the {epithet}")
}

/// Generate a culture-specific person name using a given surname.
pub fn generate_culture_person_name_with_surname(
    world: &World,
    style: &NamingStyle,
    rng: &mut dyn RngCore,
    surname: &str,
) -> String {
    let tables = tables_for(style);
    if tables.prefixes.is_empty() {
        return generate_person_name_with_surname(world, rng, surname);
    }

    for _ in 0..5 {
        let prefix = tables.prefixes[rng.random_range(0..tables.prefixes.len())];
        let suffix = tables.suffixes[rng.random_range(0..tables.suffixes.len())];
        let name = format!("{prefix}{suffix} {surname}");
        let is_taken = world
            .entities
            .values()
            .any(|e| e.kind == EntityKind::Person && e.end.is_none() && e.name == name);
        if !is_taken {
            return name;
        }
    }
    let prefix = tables.prefixes[rng.random_range(0..tables.prefixes.len())];
    let suffix = tables.suffixes[rng.random_range(0..tables.suffixes.len())];
    let epithet = EPITHETS[rng.random_range(0..EPITHETS.len())];
    format!("{prefix}{suffix} {surname} the {epithet}")
}

/// Generate a name for a culture entity itself (e.g. "The Nordhaven Culture").
pub fn generate_culture_entity_name(rng: &mut dyn RngCore) -> String {
    const PREFIXES: &[&str] = &[
        "Aether", "Iron", "Sun", "Moon", "Storm", "Tide", "Flame", "Frost", "Dawn", "Dusk",
        "Stone", "Wind", "Shadow", "Star", "Earth", "Sky", "Thunder", "Ember", "Crystal", "Raven",
    ];
    const SUFFIXES: &[&str] = &[
        "born", "mark", "hold", "vale", "crest", "guard", "heart", "kin", "song", "way", "folk",
        "reach", "forge", "root", "gard",
    ];
    let prefix = PREFIXES[rng.random_range(0..PREFIXES.len())];
    let suffix = SUFFIXES[rng.random_range(0..SUFFIXES.len())];
    format!("{prefix}{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    #[test]
    fn each_style_generates_nonempty_name() {
        let mut rng = SmallRng::seed_from_u64(42);
        for style in &NamingStyle::ALL {
            let name = generate_culture_person_name(style, &mut rng);
            assert!(!name.is_empty(), "empty name for style {style:?}");
            assert!(
                name.contains(' '),
                "name should have first and last: {name}"
            );
        }
    }

    #[test]
    fn custom_style_falls_back_to_generic() {
        let mut rng = SmallRng::seed_from_u64(42);
        let style = NamingStyle::Custom("dwarven".to_string());
        let name = generate_culture_person_name(&style, &mut rng);
        assert!(!name.is_empty());
        assert!(name.contains(' '));
    }

    #[test]
    fn deterministic_with_seed() {
        let mut rng1 = SmallRng::seed_from_u64(123);
        let mut rng2 = SmallRng::seed_from_u64(123);
        let style = NamingStyle::Nordic;
        assert_eq!(
            generate_culture_person_name(&style, &mut rng1),
            generate_culture_person_name(&style, &mut rng2)
        );
    }

    #[test]
    fn unique_name_works_on_empty_world() {
        let world = World::new();
        let mut rng = SmallRng::seed_from_u64(42);
        for style in &NamingStyle::ALL {
            let name = generate_unique_culture_person_name(&world, style, &mut rng);
            assert!(!name.is_empty());
        }
    }

    #[test]
    fn surname_inheritance_works() {
        let world = World::new();
        let mut rng = SmallRng::seed_from_u64(42);
        let name = generate_culture_person_name_with_surname(
            &world,
            &NamingStyle::Imperial,
            &mut rng,
            "Maximar",
        );
        assert!(
            name.contains("Maximar"),
            "name should contain surname: {name}"
        );
    }

    #[test]
    fn culture_entity_name_nonempty() {
        let mut rng = SmallRng::seed_from_u64(42);
        let name = generate_culture_entity_name(&mut rng);
        assert!(!name.is_empty());
    }

    #[test]
    fn styles_produce_different_names() {
        let mut names = std::collections::HashSet::new();
        for style in &NamingStyle::ALL {
            let mut rng = SmallRng::seed_from_u64(42);
            let name = generate_culture_person_name(style, &mut rng);
            names.insert(name);
        }
        // At least 4 of 6 should be distinct (very likely all 6)
        assert!(names.len() >= 4, "expected diverse names, got: {names:?}");
    }
}
