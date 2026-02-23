use history_gen::model::{EntityKind, RelationshipKind, World};
use history_gen::sim::{
    ActionSystem, ConflictSystem, DemographicsSystem, EconomySystem, PoliticsSystem, SimConfig,
    SimSystem, run,
};
use history_gen::worldgen::{self, config::WorldGenConfig};

fn main() {
    let config = WorldGenConfig { seed: 42, ..WorldGenConfig::default() };
    let mut world = worldgen::generate_world(&config);
    let mut systems: Vec<Box<dyn SimSystem>> = vec![
        Box::new(ActionSystem),
        Box::new(DemographicsSystem),
        Box::new(EconomySystem),
        Box::new(ConflictSystem),
        Box::new(PoliticsSystem),
    ];
    run(&mut world, &mut systems, SimConfig::new(1, 50, 42));
    
    // Check faction happiness/stability
    for e in world.entities.values() {
        if e.kind == EntityKind::Faction && e.end.is_none() {
            let happiness = e.properties.get("happiness").and_then(|v| v.as_f64()).unwrap_or(-1.0);
            let stability = e.properties.get("stability").and_then(|v| v.as_f64()).unwrap_or(-1.0);
            let treasury = e.properties.get("treasury").and_then(|v| v.as_f64()).unwrap_or(-1.0);
            let econ_motivation = e.properties.get("economic_war_motivation").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let has_enemies = e.relationships.iter().any(|r| r.kind == RelationshipKind::Enemy && r.end.is_none());
            let has_allies = e.relationships.iter().any(|r| r.kind == RelationshipKind::Ally && r.end.is_none());
            eprintln!("Faction {} ({}): happiness={:.3} stability={:.3} treasury={:.1} econ_motivation={:.3} enemies={} allies={}", 
                e.id, e.name, happiness, stability, treasury, econ_motivation, has_enemies, has_allies);
        }
    }
    
    // Check settlement prosperity
    let mut prosperities: Vec<f64> = Vec::new();
    for e in world.entities.values() {
        if e.kind == EntityKind::Settlement && e.end.is_none() {
            let prosperity = e.properties.get("prosperity").and_then(|v| v.as_f64()).unwrap_or(-1.0);
            prosperities.push(prosperity);
        }
    }
    eprintln!("Settlement prosperities: min={:.3} max={:.3} avg={:.3}", 
        prosperities.iter().copied().fold(f64::INFINITY, f64::min),
        prosperities.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        prosperities.iter().sum::<f64>() / prosperities.len() as f64);
    
    // Count wars
    let wars = world.events.values().filter(|e| e.kind == history_gen::model::EventKind::WarDeclared).count();
    let battles = world.events.values().filter(|e| e.kind == history_gen::model::EventKind::Battle).count();
    eprintln!("Wars: {} Battles: {}", wars, battles);
}
