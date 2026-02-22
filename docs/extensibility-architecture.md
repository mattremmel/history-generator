Extensibility Architecture for history-gen

The Core Problem

Your current enums are closed:

pub enum EntityKind { Person, Settlement, Faction }
pub enum EventKind { Birth, Death, Marriage, SettlementFounded, FactionFormed }
pub enum RelationshipKind { Parent, Child, Spouse, Ally, Enemy, MemberOf, RulerOf }

Adding SpellCast for D&D or FtlJump for sci-fi means editing the core. Every setting change requires recompiling the core types. This won't scale.

Approaches Considered

Full ECS (bevy_ecs, specs, legion) — Not recommended

ECS frameworks optimize for iterating millions of homogeneous entities at 60fps. Your simulation has thousands of heterogeneous entities ticking yearly. The conceptual overhead
(archetype storage, system scheduling, query syntax) doesn't pay for itself. You'd be fighting the framework more than benefiting from it.

Feature-gated enums (#[cfg(feature = "dnd")]) — Not recommended

Keeps type safety but all settings must live in the same crate. Not truly pluggable — adding a setting still means editing core enums.

RimWorld-style full dynamic dispatch — Partial inspiration

RimWorld's Def + ThingComp system is maximally flexible but relies on reflection, linear GetComp<T>() searches, and C# garbage collection. The performance characteristics are wrong for
Rust, and the complexity is premature for your stage.

Recommended: Hybrid Typed Core + String-Keyed Extensions

This keeps your current typed enums for universal concepts (every setting has births, deaths, relationships) while adding open extension points for setting-specific data. It plays to
Rust's strengths and your existing serde_json pipeline.

1. Split kinds into Core + Custom

// Universal concepts — every setting has these
pub enum CoreEventKind {
Birth, Death, Marriage, SettlementFounded, FactionFormed,
}

// Open for extension
pub enum EventKind {
Core(CoreEventKind),
Custom(String), // "spell_cast", "ftl_jump", "plague_outbreak"
}

Same pattern for EntityKind and RelationshipKind. The Core variants give you exhaustive match where it matters (demographics system always handles Birth/Death). The Custom variant lets
settings inject anything without touching the core.

Serde can serialize this cleanly — Core(Birth) → "birth", Custom("spell_cast") → "spell_cast". Postgres sees the same TEXT column either way.

2. Property bag on Entity

pub struct Entity {
pub id: u64,
pub kind: EntityKind,
pub name: String,
pub origin: Option<SimTimestamp>,
pub end: Option<SimTimestamp>, #[serde(skip)]
pub relationships: Vec<Relationship>,
/// Setting-specific properties. D&D: {"mana": 50, "class": "wizard"}
/// Sci-fi: {"tech_level": 3, "cybernetics": ["neural_link"]} #[serde(default, skip_serializing_if = "HashMap::is_empty")]
pub properties: HashMap<String, serde_json::Value>,
}

Why serde_json::Value instead of a TypeMap<TypeId, Box<dyn Any>>?

- You already depend on serde_json
- Properties serialize directly to JSONL with zero conversion
- Maps directly to Postgres jsonb columns for querying
- No downcast_ref boilerplate
- When a system needs typed access, it deserializes on demand: serde_json::from_value::<MagicStats>(entity.properties["magic"].clone())

If profiling later shows repeated deserialization is hot, you can add a TypeMap cache alongside. But start simple.

3. Event data payload

pub struct Event {
pub id: u64,
pub kind: EventKind,
pub timestamp: SimTimestamp,
pub description: String,
pub caused_by: Option<u64>,
/// Setting-specific event data. Birth: {"mother": 42, "father": 17}
/// Spell: {"school": "evocation", "power": 8, "target": 99} #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
pub data: serde_json::Value,
}

This lets any setting attach structured data to events without the core knowing what "spell school" means.

4. SimSystem trait for pluggable tick logic

pub trait SimSystem {
/// Unique name for logging/ordering
fn name(&self) -> &str;

      /// Called once per simulated year. Mutate world state, create events, etc.
      fn tick(&mut self, world: &mut World, rng: &mut impl Rng);

}

A D&D setting registers its systems:

fn build_dnd_systems() -> Vec<Box<dyn SimSystem>> {
vec![
          Box::new(DemographicsSystem),  // core: births, deaths, aging
          Box::new(MagicSystem),         // setting: spell research, mana regeneration
          Box::new(DivineSystem),        // setting: deity influence, miracles
          Box::new(ConflictSystem),      // core: wars, raids
          Box::new(PlanarSystem),        // setting: planar incursions
      ]
}

A medieval-realism setting skips Magic/Divine/Planar and adds:

fn build_medieval_systems() -> Vec<Box<dyn SimSystem>> {
vec![
          Box::new(DemographicsSystem),
          Box::new(AgricultureSystem),   // crop yields, famine
          Box::new(DiseaseSystem),       // plague, epidemics
          Box::new(FeudalSystem),        // oaths, vassalage
          Box::new(ConflictSystem),
      ]
}

The main loop is setting-agnostic:

pub fn run_simulation(world: &mut World, systems: &mut [Box<dyn SimSystem>], years: u32) {
for year in 0..years {
world.current_time = SimTimestamp::from_year(year);
for system in systems.iter_mut() {
system.tick(world, &mut rng);
}
if year % 50 == 0 {
flush_jsonl(world, &output_dir);
}
}
}

5. StateChange extension

Your StateChange enum also needs to be open:

pub enum StateChange {
EntityCreated { kind: EntityKind, name: String },
EntityEnded,
NameChanged { old: String, new: String },
RelationshipStarted { target_entity_id: u64, kind: RelationshipKind },
RelationshipEnded { target_entity_id: u64, kind: RelationshipKind },
PropertyChanged { field: String, old_value: serde_json::Value, new_value: serde_json::Value },
}

Notice PropertyChanged already exists in your code — it just needs to use serde_json::Value instead of String so it can represent structured changes. This is already your escape hatch.
Setting-specific mutations all go through PropertyChanged.

6. Data-driven templates (future, not now)

When you have multiple settings, define entity/event templates in RON files:

// settings/dnd/templates.ron
(
entity_templates: {
"archmage": (
kind: Person,
properties: {
"class": "wizard",
"mana": 100,
"spell_slots": [4, 3, 3, 3, 2, 1, 1, 1, 1],
},
),
},
event_templates: {
"spell_cast": (
kind: Custom("spell_cast"),
required_fields: ["caster", "school", "power"],
),
},
)

But this is Phase N, not now. You don't need templates until you have at least two settings to compare.

What NOT to do

- Don't adopt bevy_ecs — your access patterns (mutate one entity based on its relationships, create events linking multiple entities) require &mut World access patterns that fight ECS
  borrow rules
- Don't use trait objects for components — Box<dyn ComponentData> with downcast_ref is more complex than serde_json::Value and doesn't buy you anything when your flush target is JSON
  anyway
- Don't build a plugin/WASM system — compile-time composition via Cargo features/workspace crates is sufficient until you need third-party mods
- Don't convert all enums to strings immediately — keep CoreEventKind etc. typed so match exhaustiveness catches bugs in universal systems

Migration Path

This can be done incrementally without breaking your existing pipeline:

1. (COMPLETE) Add properties: HashMap<String, serde_json::Value> to Entity — defaults to empty, existing tests unchanged
2. (COMPLETE) Add data: serde_json::Value to Event — defaults to Value::Null, existing tests unchanged
3. (COMPLETE) Split EventKind into Core + Custom — update serde impl, existing JSONL/Postgres stays compatible since output is still string tags
4. (COMPLETE) Same for EntityKind, RelationshipKind
5. (COMPLETE) Define SimSystem trait — extract tick dispatch from wherever it lives
6. Change PropertyChanged values to serde_json::Value — more expressive effect tracking
7. Update Postgres schema — add jsonb columns for entity properties and event data

Steps 1-2 are backward compatible. Steps 3-4 require updating match arms. Steps 5-7 can wait until you build the tick loop (Phase 3).

Performance Characteristics

- HashMap<String, Value> lookups: ~50ns per access. At 10K entities × 10 properties × yearly tick = 1M lookups = ~50ms/year. Negligible.
- Dynamic dispatch on SimSystem::tick(): One vtable call per system per year. Completely irrelevant.
- serde_json::from_value deserialization: ~200ns for a small struct. Only needed when a system wants typed access to a specific property. Most systems will just read/write Value
  directly.
- No archetype fragmentation, no sparse set overhead, no component migration costs. Just HashMap and BTreeMap, which Rust optimizes very well.

The actual bottleneck in a mature simulation will be the decision logic inside systems (evaluating relationships, resolving conflicts), not the data access pattern. This architecture
keeps the data access cheap and puts all complexity where it belongs — in the system logic.
