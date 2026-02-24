use history_gen::model::{EntityKind, KnowledgeCategory};
use history_gen::worldgen::{self, config::WorldGenConfig};

#[test]
fn worldgen_creates_founding_knowledge() {
    let config = WorldGenConfig {
        seed: 42,
        ..WorldGenConfig::default()
    };
    let world = worldgen::generate_world(&config);

    let founding_count = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Knowledge)
        .filter(|e| {
            e.data
                .as_knowledge()
                .is_some_and(|kd| kd.category == KnowledgeCategory::Founding)
        })
        .count();

    let settlement_count = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement)
        .count();

    assert!(
        founding_count > 0,
        "worldgen should create founding knowledge"
    );
    assert_eq!(
        founding_count, settlement_count,
        "each settlement should have a founding knowledge entry"
    );
}
