# Migration & Refactor to Bevy ECS

This document is a top-to-bottom, ordered migration plan for moving the history-gen simulation from the current monolithic `World` + `SimSystem` trait architecture to Bevy ECS. Each phase is designed to be independently testable before moving to the next.

---

## Table of Contents

1. [Motivation & Tradeoff Analysis](#1-motivation--tradeoff-analysis)
2. [Architecture Overview](#2-architecture-overview)
3. [Dependencies](#3-dependencies)
4. [Calendar & Tick Constants](#4-calendar--tick-constants)
5. [Phase 0: Foundation — Bevy App Shell & Tick Control](#5-phase-0-foundation--bevy-app-shell--tick-control)
6. [Phase 1: Data Model — Components & Resources](#6-phase-1-data-model--components--resources)
7. [Phase 2: Event Architecture — Command Events](#7-phase-2-event-architecture--command-events)
8. [Signal Flow Map — Producer/Consumer Reference](#8-signal-flow-map--producerconsumer-reference)
9. [Phase 3: System Migration Order](#9-phase-3-system-migration-order)
10. [Helper Function Migration](#10-helper-function-migration)
11. [Phase 4: Plugin Decomposition](#11-phase-4-plugin-decomposition)
12. [Phase 5: Parallelism & Scheduling](#12-phase-5-parallelism--scheduling)
13. [Phase 6: Flush & Serialization](#13-phase-6-flush--serialization)
14. [Phase 7: Testing Strategy](#14-phase-7-testing-strategy)
15. [Phase 8: Cleanup & Final Integration](#15-phase-8-cleanup--final-integration)
16. [Determinism Strategy](#16-determinism-strategy)
17. [Procedural Generation & Non-Notable NPCs](#17-procedural-generation--non-notable-npcs)
18. [Risk Register](#18-risk-register)

---

## 1. Motivation & Tradeoff Analysis

### Why Migrate

**Performance via parallelism.** The current simulation runs 18 systems sequentially. Many system pairs have no data dependencies and could run concurrently. With ~40k LOC of system logic processing thousands of entities, the tick loop is the bottleneck for long simulation runs (1000+ years). Bevy's scheduler automatically parallelizes systems with disjoint data access.

**System decomposition.** Large monolithic systems (PoliticsSystem: 1500+ LOC, ConflictSystem: 2000+ LOC, KnowledgeSystem: 4000+ LOC) are hard to reason about and test. Bevy encourages many small, focused systems grouped into plugins. Each system does one thing and declares its data dependencies explicitly in its function signature.

**Testability.** Bevy systems are plain functions with typed parameters. Testing a system means constructing a minimal `World`, spawning the entities it needs, running it, and asserting on component state. No need for the `TickContext` indirection or mock `SimSystem` trait objects.

**Reactive logic.** Bevy's observers and hooks replace the manual two-phase signal dispatch. A leader dying can directly trigger succession logic via `OnRemove<Leader>` observers rather than requiring all systems to manually check their signal inbox.

**Decoupled tick control.** Bevy's schedule can be run manually (one `schedule.run(&mut world)` per tick), run uncapped in a loop for initial worldgen simulation, or gated behind user input for interactive "play" mode. This is a natural fit for the dual-mode requirement.

### What We Give Up

**Implicit determinism.** The current sequential dispatch is trivially deterministic. With Bevy, determinism requires explicit system ordering and sorted query iteration. This is achievable but requires discipline. See [Determinism Strategy](#16-determinism-strategy).

**Simplicity of the current model.** The monolithic `World` struct with `BTreeMap<u64, Entity>` is conceptually simple. Bevy's archetype-based storage is more performant but has a learning curve (queries, filters, change detection, commands vs direct mutation).

**Inline relationships.** The current `Vec<Relationship>` on each entity is cache-friendly for sequential access. In Bevy, relationships become either Bevy's first-class `Relationship` components (0.16+) or lookup tables in a `Resource`. The Bevy relationship system supports one-to-many but not many-to-many natively.

**Control over event lifecycle.** The current signal system is single-pass, non-cascading, and signals are gone after the tick. Bevy's message/event system has a 2-frame buffer lifetime. For a batch simulation where we control ticks, this is fine — but it's different semantics to be aware of.

**Event buffer lifecycle with manual `schedule.run()`:** With manual tick control, `SimReactiveEvent`s emitted in tick N's `PostUpdate` → `apply_sim_commands` will be readable in tick N's `Reactions` set (same schedule run) AND in tick N+1's schedule run. Bevy's `EventReader` tracks read position per-system, so a handler that reads in Reactions won't re-read the same events next tick. **Risk:** If a reaction handler is added to the Reactions set but also registered elsewhere (e.g., Update), it could see stale events from the previous tick. **Mitigation:** All `SimReactiveEvent` consumers should exclusively be in the `Reactions` system set within PostUpdate. No Update-phase system should read `SimReactiveEvent`. Consider using `Events::update()` explicitly or a `clear_reactive_events` system in `SimPhase::Last` to prevent cross-tick leakage.

### Verdict

The tradeoffs are acceptable. The performance gains, testability improvements, and system decomposition benefits outweigh the complexity costs. The project has no backwards-compatibility constraints, making this an ideal time to migrate.

---

## 2. Architecture Overview

### Current Architecture

```
SimConfig + World + [Box<dyn SimSystem>]
    │
    ▼
run() loop: for each year/month/day...
    │
    ├─ Phase 1: system.tick(&mut TickContext)  ← sequential, each sees latest state
    │       └─ mutates World directly, pushes Signals
    │
    └─ Phase 2: system.handle_signals(&mut TickContext)  ← sequential, reads signal inbox
            └─ mutates World directly, new signals discarded
```

### Target Architecture

```
bevy_ecs::World + Schedule
    │
    ▼
SimTick (manual schedule.run() per tick)
    │
    ├─ PreUpdate: advance SimTimestamp, frequency gating
    │
    ├─ Update: simulation systems (parallel where possible)
    │       └─ read components, emit SimCommand events
    │       └─ DO NOT mutate entity state directly
    │
    ├─ PostUpdate: apply SimCommands to world
    │       └─ single-threaded command application
    │       └─ emit reactive signals (Bevy events) for cross-system reactions
    │       └─ append to EventLog resource
    │
    └─ Last: flush checkpoint (every N years)
```

### Key Design Decisions

**Command-event pattern.** Systems do not mutate entity components directly. Instead, they emit `SimCommand` events describing intended state changes. A centralized `apply_commands` system in `PostUpdate` processes these commands, applies them to the world, and emits reactive events. This gives us:

- A natural event log (every command is a historical event)
- Conflict resolution (two systems can't race to modify the same entity)
- Parallelism (all Update systems only need read access to components + write access to their event writer)
- Auditability (the command stream is the complete history)

**Bevy entity = simulation entity.** Each simulation entity (person, settlement, faction, etc.) becomes a Bevy entity with a `SimEntity` marker component and kind-specific component bundles.

**Relationships as components.** Use Bevy's `Relationship` trait for structural relationships (LocatedIn, MemberOf, LeaderOf). Use a `Relationships` resource or dedicated components for queryable relationship sets (Ally, Enemy, AtWar, TradeRoute).

**Tick frequency as run conditions.** Instead of the current `should_fire()` check, use Bevy run conditions on system sets. A `SimClock` resource tracks the current timestamp, and `run_if(yearly)`, `run_if(monthly)` etc. gate system set execution.

---

## 3. Dependencies

### Cargo.toml Changes

```toml
[dependencies]
# Add — Bevy ECS and app infrastructure (headless, no rendering)
bevy_ecs = "0.18"
bevy_app = "0.18"

# Keep
rand = "0.9"
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# Keep — Postgres loader may eventually move to a separate binary
sqlx = { version = "0.8", features = ["runtime-tokio", "postgres"] }

[dev-dependencies]
# Keep all existing dev-dependencies
tempfile = "3"
testcontainers = "0.23"
testcontainers-modules = { version = "0.11", features = ["postgres"] }
tokio = { version = "1", features = ["rt", "macros"] }
```

**Pin `bevy_ecs` to exact version** (e.g., `=0.18.0`) during migration to avoid breakage from minor releases. Upgrade to 0.19+ only after migration is complete.

**Bevy 0.18 Relationship API verification:** Bevy Relationships (`#[derive(Relationship)]`) first landed in Bevy 0.16 (April 2025) and the API evolved in 0.17 (September 2025) with `set_risky` method changes and renamed methods (`parent` → `related`, `children` → `relationship_sources`). Before implementation, verify the exact Bevy 0.18 relationship API against https://docs.rs/bevy_ecs/0.18.0/bevy_ecs/. Specifically: confirm `#[derive(Relationship)]` still works for single-field structs, confirm `RelationshipTarget` derive syntax, check if `Query` methods for traversing relationships match examples in this document, and note any breaking changes from 0.16 → 0.18 that affect structural relationship definitions.

**Do NOT pull in full `bevy`** — only `bevy_ecs` and `bevy_app`. The simulation is headless; rendering dependencies are unnecessary.

---

## 4. Calendar & Tick Constants

The simulation uses a custom calendar. These constants must be preserved in the Bevy migration and used by the `SimClock` and run conditions:

| Constant | Value | Source |
|----------|-------|--------|
| `DAYS_PER_YEAR` | 360 | `model::timestamp` |
| `DAYS_PER_MONTH` | 30 | `model::timestamp` |
| `MONTHS_PER_YEAR` | 12 | `model::timestamp` |
| `HOURS_PER_DAY` | 24 | `model::timestamp` |
| Weeks per year | ~52 (360/7) | Computed |

**Current tick frequencies and their fire rates:**

| Frequency | Fires per year | Condition |
|-----------|---------------|-----------|
| `Yearly` | 1 | `hour == 0 && day == 1` |
| `Monthly` | 12 | `hour == 0 && day_of_month == 1` |
| `Weekly` | 52 | `hour == 0 && (day - 1) % 7 == 0` |
| `Daily` | 360 | `hour == 0` |
| `Hourly` | 8,640 | Always |

The **finest frequency** across all registered systems determines the inner loop granularity. Currently the finest is `Monthly` (used by Environment, Economy, and Conflicts). The run loop iterates monthly and coarser systems fire on matching boundaries.

---

## 5. Phase 0: Foundation — Bevy App Shell & Tick Control

**Goal:** Get a Bevy app that can run simulation ticks manually, with no simulation logic yet. Prove the tick control model works.

### Steps

1. **Add `bevy_ecs` dependency** (just `bevy_ecs`, not full `bevy`). Also add `bevy_app` for the plugin/schedule infrastructure without pulling in rendering.

2. **Define `SimClock` resource:**
   ```rust
   #[derive(Resource)]
   pub struct SimClock {
       pub time: SimTimestamp,
       pub tick_count: u64,
   }
   ```

3. **Define tick frequency run conditions** (must match current `should_fire()` semantics exactly):
   ```rust
   fn yearly(clock: Res<SimClock>) -> bool {
       clock.time.hour() == 0 && clock.time.day() == 1
   }
   fn monthly(clock: Res<SimClock>) -> bool {
       clock.time.hour() == 0 && clock.time.day_of_month() == 1
   }
   fn weekly(clock: Res<SimClock>) -> bool {
       clock.time.hour() == 0 && (clock.time.day() - 1).is_multiple_of(7)
   }
   fn daily(clock: Res<SimClock>) -> bool {
       clock.time.hour() == 0
   }
   fn hourly(_clock: Res<SimClock>) -> bool {
       true // fires every tick
   }
   ```

4. **Define the `SimSchedule`** — a custom `Schedule` that runs one simulation tick:
   ```rust
   #[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
   pub struct SimTick;
   ```

5. **Define schedule phases using system sets:**
   ```rust
   #[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
   pub enum SimPhase {
       PreUpdate,   // advance clock, check frequency gates
       Update,      // simulation systems emit commands
       PostUpdate,  // apply commands, emit reactive events
       Last,        // flush, cleanup
   }
   ```

6. **Build the app with manual tick control:**
   ```rust
   // Uncapped mode (initial simulation):
   for _ in 0..total_ticks {
       app.world_mut().resource_mut::<SimClock>().advance();
       app.world_mut().run_schedule(SimTick);
   }

   // Play mode (manual trigger):
   // advance clock + run_schedule on user input
   ```

7. **Write tests** proving:
   - Clock advances correctly per tick
   - Yearly systems only fire on year boundaries
   - Monthly systems fire 12x per year
   - Uncapped loop runs N ticks without blocking

### Deliverable
A minimal `bevy_app` that ticks a clock and gates system execution by frequency. No simulation logic — just the harness.

---

## 6. Phase 1: Data Model — Components & Resources

**Goal:** Translate the current `Entity` + `EntityData` enum into Bevy components. Translate `World`-level collections into resources.

### Entity → Component Bundles

Each `EntityKind` becomes a marker component plus a bundle of kind-specific data components. The monolithic `EntityData` enum is decomposed:

```
EntityData::Person(PersonData)  →  (SimEntity, Person, PersonData)
EntityData::Settlement(...)     →  (SimEntity, Settlement, SettlementData)
EntityData::Faction(...)        →  (SimEntity, Faction, FactionData)
...
```

**Marker components:**
```rust
#[derive(Component)] pub struct SimEntity { pub id: u64, pub name: String, pub origin: Option<SimTimestamp>, pub end: Option<SimTimestamp> }
#[derive(Component)] pub struct Person;
#[derive(Component)] pub struct Settlement;
#[derive(Component)] pub struct Faction;
#[derive(Component)] pub struct Army;
// ... one per EntityKind
```

**Data components** — directly reuse existing data structs, just derive `Component`:
```rust
#[derive(Component)]  // add to existing derive
pub struct PersonData { ... }

#[derive(Component)]
pub struct SettlementData { ... }
```

### Decompose Large Data Structs

Some data structs should be split into multiple components for better query granularity and parallelism:

**SettlementData splits into:**
- `SettlementCore` — population, population_breakdown, prosperity, treasury, capacity, resources, prestige, prestige_tier
- `SettlementCulture` — culture_makeup, religion_makeup, dominant_culture, cultural_tension, dominant_religion, religious_tension
- `SettlementDisease` — disease risk factors, plague immunity
- `SettlementTrade` — trade_routes, trade_income, production, surplus, port data
- `SettlementMilitary` — fortification_level, guard_strength
- `SettlementCrime` — crime_rate, bandit_threat
- `SettlementEducation` — literacy_rate
- `SeasonalModifiers` — already a separate struct, becomes its own component
- `BuildingBonuses` — already a separate struct, becomes its own component

**FactionData splits into:**
- `FactionCore` — government_type, stability, happiness, legitimacy, treasury
- `FactionDiplomacy` — grievances, war_goals, tributes
- `FactionMilitary` — war_started, mercenary contracts

**PersonData splits into:**
- `PersonCore` — born, sex, role, traits, last_action
- `PersonReputation` — prestige, prestige_tier
- `PersonSocial` — grievances, secrets, claims, loyalty
- `PersonEducation` — education level

This decomposition is critical for parallelism: a system updating settlement trade doesn't conflict with a system updating settlement culture.

### Field-to-Component Mapping

**SettlementData field assignments (~35 fields):**

| Component | Fields |
|-----------|--------|
| `SettlementCore` | `x`, `y`, `population` (u32), `population_breakdown` (PopulationBreakdown), `prosperity`, `treasury`, `capacity`, `blend_timer`, `last_prophecy_year`, `resources` (Vec\<ResourceType\>), `prestige`, `prestige_tier` |
| `SettlementCulture` | `culture_makeup`, `religion_makeup`, `dominant_culture`, `cultural_tension`, `dominant_religion`, `religious_tension` |
| `SettlementDisease` | `disease_risk` (DiseaseRisk), `plague_immunity` |
| `SettlementTrade` | `trade_routes`, `trade_income`, `production`, `surplus`, `trade_happiness_bonus`, `is_coastal` |
| `SettlementMilitary` | `fortification_level`, `guard_strength` |
| `SettlementCrime` | `crime_rate`, `bandit_threat` |
| `SettlementEducation` | `literacy_rate` |
| `SeasonalModifiers` | Already a separate struct — becomes its own component |
| `BuildingBonuses` | Already a separate struct — becomes its own component |

**FactionData field assignments (~28 fields):**

| Component | Fields |
|-----------|--------|
| `FactionCore` | `government_type`, `stability`, `happiness`, `legitimacy`, `treasury`, `primary_culture`, `primary_religion`, `prestige`, `prestige_tier`, `literacy_rate`, `succession_crisis_at` |
| `FactionDiplomacy` | `grievances`, `war_goals`, `tributes`, `alliance_strength`, `marriage_alliances`, `loyalty`, `trade_partner_routes`, `secrets`, `diplomatic_trust`, `betrayal_count`, `last_betrayal`, `last_betrayed_by` |
| `FactionMilitary` | `war_started`, `mercenary_wage`, `unpaid_months`, `economic_motivation` |

**PersonData field assignments (~14 fields):**

| Component | Fields |
|-----------|--------|
| `PersonCore` | `born`, `sex`, `role`, `traits`, `last_action`, `culture_id`, `widowed_at` |
| `PersonReputation` | `prestige`, `prestige_tier` |
| `PersonSocial` | `grievances`, `secrets`, `claims`, `loyalty` |
| `PersonEducation` | `education` |

### Embedded Complex Types

Several important types embedded in data structs need explicit migration decisions:

- **`PopulationBreakdown`** — 8 age brackets × 2 sexes, used by Demographics and Disease. **Keep embedded** in `SettlementCore` (always accessed together with population).
- **`ActiveSiege`, `ActiveDisease`, `ActiveDisaster`** — currently `Option<...>` on SettlementData. **Make separate components** (not embedded in SettlementDisease/SettlementMilitary) — this enables efficient query filtering (e.g., `Query<&SettlementCore, With<ActiveDisease>>` to only process settlements with active disease, vs checking `Option::is_some()` on every settlement). These are added/removed dynamically via commands, not stored in the field mapping tables above.
- **`Grievance`, `SecretDesire`, `Claim`, `TributeObligation`, `WarGoal`** — these `BTreeMap<u64, T>` fields contain sim-IDs that need `SimEntityMap` lookups in Bevy. The command applicator must resolve sim-IDs to Bevy Entities when processing these. During migration, replace `u64` keys with Bevy `Entity` handles.
- **`SeasonalModifiers`, `BuildingBonuses`, `DiseaseRisk`, `TradeRoute`** — already separate structs, become their own components or stay embedded in their parent component per the field mapping above.

### Sub-Struct Field Reference

These sub-structs are referenced in the field mapping tables above. Their fields are listed here for migration reference, since each field may need explicit component-or-embedded decisions and sim-ID resolution.

**SeasonalModifiers (7 fields):**
`food: f64`, `trade: f64`, `disease: f64`, `army: f64`, `construction_blocked: bool`, `construction_months: u32`, `food_annual: f64`

**BuildingBonuses (13 fields):**
`mine: f64`, `workshop: f64`, `market: f64`, `port_trade: f64`, `port_range: f64`, `happiness: f64`, `capacity: f64`, `food_buffer: f64`, `library: f64`, `temple_knowledge: f64`, `temple_religion: f64`, `academy: f64`, `fishing: f64`

**TradeRoute (4 fields):**
`target: u64`, `path: Vec<u64>`, `distance: u32`, `resource: String`

**DiseaseRisk (4 fields):**
`refugee: f64`, `post_conquest: f64`, `post_disaster: f64`, `siege_bonus: f64`

**ActiveSiege (5 fields):**
`attacker_army_id: u64`, `attacker_faction_id: u64`, `started: SimTimestamp`, `months_elapsed: u32`, `civilian_deaths: u32`

**ActiveDisease (5 fields):**
`disease_id: u64`, `started: SimTimestamp`, `infection_rate: f64`, `peak_reached: bool`, `total_deaths: u32`

**ActiveDisaster (5 fields):**
`disaster_type: DisasterType`, `severity: f64`, `started: SimTimestamp`, `months_remaining: u32`, `total_deaths: u32`

**Grievance (4 fields):**
`severity: f64`, `sources: Vec<String>`, `peak: f64`, `updated: SimTimestamp`

**SecretDesire (3 fields):**
`motivation: SecretMotivation`, `sensitivity: f64`, `accuracy_threshold: f64` (default 0.3)

**Claim (3 fields):**
`strength: f64`, `source: String`, `year: u32`

**TributeObligation (3 fields):**
`amount: f64`, `years_remaining: u32`, `treaty_event_id: u64`

**WarGoal (5 variants — tagged enum):**
`Territorial { target_settlements }`, `Economic { reparation_demand }`, `Punitive`, `SuccessionClaim { claimant_id }`, `Expansion { target_settlements, motivation: ExpansionMotivation }`

**Sim-ID resolution note:** The following sub-struct fields contain sim-IDs (u64) that must be resolved to Bevy `Entity` handles via `SimEntityMap` during command application: `ActiveSiege.attacker_army_id`, `ActiveSiege.attacker_faction_id`, `ActiveDisease.disease_id`, `TradeRoute.target`, `TradeRoute.path`, `TributeObligation.treaty_event_id`, and `WarGoal` target IDs. `Claim.year` is a calendar year, not an entity ID.

**Remaining entity types — keep as single components** (small enough that splitting adds complexity without parallelism benefit):

| EntityKind | Component | Fields | Notes |
|---|---|---|---|
| `Army` | `ArmyData` | 9 fields (morale, supply, strength, faction_id, home_region_id, besieging_settlement_id, months_campaigning, starting_strength, is_mercenary) | `faction_id` and `home_region_id` could become Bevy relationships, but keep as raw u64 initially for simplicity |
| `Building` | `BuildingData` | 7 fields (building_type, output_resource, x, y, condition, level, constructed) | |
| `Region` | `RegionData` | 5 fields (terrain, terrain_tags, x, y, resources) | Static after worldgen |
| `Knowledge` | `KnowledgeData` | 7 fields (category, source_event_id, origin_settlement_id, origin_time, significance, ground_truth, revealed_at) | |
| `Manifestation` | `ManifestationData` | 10 fields (knowledge_id, medium, content, accuracy, completeness, distortions, derived_from_id, derivation_method, condition, created) | |
| `Item` | `ItemData` | 7 fields (item_type, material, resonance, condition, created, resonance_tier, last_transferred) | |
| `Religion` | `ReligionData` | 4 fields (fervor, proselytism, orthodoxy, tenets) | |
| `Deity` | `DeityData` | 2 fields (domain, worship_strength) | |
| `Culture` | `CultureData` | 3 fields (values, naming_style, resistance) | |
| `Disease` | `DiseaseData` | 4 fields (virulence, lethality, duration_years, bracket_severity) | |
| `River` | `RiverData` | 2 fields (region_path, length) | |
| `GeographicFeature` | `GeographicFeatureData` | 3 fields (feature_type, x, y) | |
| `ResourceDeposit` | `ResourceDepositData` | 6 fields (resource_type, quantity, quality, discovered, x, y) | |
| `Creature` | — | No typed data (`EntityData::None`) | Just marker component `Creature` + `SimEntity` |

### Relationships

**Complete relationship kind mapping** (all 15 current `RelationshipKind` variants):

**Structural relationships (N:1) → Bevy Relationships:**
- `LocatedIn(Entity)` — settlement/building → region
- `MemberOf(Entity)` — person/settlement/army → faction
- `LeaderOf(Entity)` — person → faction
- `HeldBy(Entity)` — item → person/settlement
- `HiredBy(Entity)` — mercenary faction → employer faction
- `FlowsThrough(Entity)` — river → region
- `Exploits(Entity)` — settlement → resource deposit

These use Bevy's `#[derive(Relationship)]` for automatic indexing and cleanup. Each is a dedicated component type.

**Geographic relationships → Dedicated resource:**
- `AdjacentTo` — symmetric, static after worldgen, heavily used by BFS pathfinding. Best as a dedicated `RegionAdjacency` resource (adjacency list) rather than per-entity relationships, since the BFS helpers need fast neighbor lookup.

```rust
#[derive(Resource)]
pub struct RegionAdjacency {
    /// region Bevy Entity → sorted Vec of adjacent region Bevy Entities
    adjacency: BTreeMap<Entity, Vec<Entity>>,
}
```

**Graph relationships (M:N) → Resource:**
```rust
#[derive(Resource)]
pub struct RelationshipGraph {
    // Diplomatic — bidirectional, between factions
    allies: BTreeSet<(Entity, Entity)>,       // canonical order: min first
    enemies: BTreeSet<(Entity, Entity)>,      // canonical order: min first
    at_war: BTreeSet<(Entity, Entity)>,       // canonical order: min first

    // Family — between persons
    parent_child: BTreeMap<Entity, Vec<Entity>>,  // parent → children
    spouses: BTreeSet<(Entity, Entity)>,          // canonical order: min first

    // Economic — between settlements, with associated data
    trade_routes: BTreeMap<(Entity, Entity), TradeRouteData>,
}
```

**IMPORTANT:** All collections use `BTreeSet`/`BTreeMap`, never `HashSet`/`HashMap`. This preserves deterministic iteration order for the `SimpleExecutor` debug mode. See [Determinism Strategy](#16-determinism-strategy).

This resource is read by many systems and written only by the command applicator. Using a resource rather than per-entity components avoids the archetype fragmentation that would come from dynamic relationship components.

**Design decision — relationship-as-entity pattern:** An alternative is storing relationships as dedicated entities with components like `(AllyLink, SourceEntity, TargetEntity, RelationshipMeta { start, end })`. This is more ECS-idiomatic and allows querying relationships directly, but adds entity count overhead. Evaluate during Phase 1 implementation which pattern fits better — the `RelationshipGraph` resource is simpler to start with.

### Relationship Read/Write Patterns by System

For Bevy query planning, this table maps each relationship type to the systems that read and write it.

**Structural Relationships (Bevy Relationship components):**

| Relationship | Read By | Written By (via commands) |
|-------------|---------|---------------------------|
| `LocatedIn` | Demographics, Buildings, Economy, Conflicts, Migration, Disease, Knowledge, Culture, Religion | Migration (relocate), Conflicts (conquest transfer) |
| `MemberOf` | Demographics, Economy, Politics, Conflicts, Agency, Actions, Migration | Politics (faction split), Conflicts (conquest transfer) |
| `LeaderOf` | Demographics, Politics, Reputation, Conflicts, Agency | Politics (succession), Conflicts (leader death) |
| `HeldBy` | Items, Knowledge | Items (transfer on death/conquest/theft) |
| `HiredBy` | Conflicts, Politics, Economy | Politics (mercenary contracts) |
| `FlowsThrough` | Worldgen only | Static after worldgen |
| `Exploits` | Economy | Static after worldgen |

**Graph Relationships (RelationshipGraph resource):**

| Relationship | Read By | Written By (via commands) |
|-------------|---------|---------------------------|
| `Ally` | Politics, Conflicts, Reputation, Agency, Actions | Politics (form/betray alliance) |
| `Enemy` | Politics, Conflicts | Politics (rivalry) |
| `AtWar` | Conflicts, Economy, Politics, Reputation, Crime, Agency | Conflicts (declare/end war) |
| `Parent/Child` | Demographics | Demographics (births) |
| `Spouse` | Demographics | Demographics (marriages) |
| `TradeRoute` | Economy, Culture, Religion, Knowledge, Disease | Economy (establish/sever) |

**`AdjacentTo` (RegionAdjacency resource):**
Read by: Conflicts (BFS), Migration (BFS), Crime (raids), Disease (spread), Knowledge (propagation), Economy (trade route pathfinding).
Written by: Static after worldgen.

### Temporal Data on Relationships

All current relationships carry `start: SimTimestamp` and `end: Option<SimTimestamp>`. Bevy's `#[derive(Relationship)]` is structural (parent-child) and has no concept of temporal start/end. This needs explicit handling:

**Decision:** Store temporal data in the `RelationshipGraph` resource alongside the relationship data. For structural Bevy Relationships (LocatedIn, MemberOf, etc.), pair each with a `RelationshipMeta` component:

```rust
#[derive(Component)]
pub struct RelationshipMeta {
    pub start: SimTimestamp,
    pub end: Option<SimTimestamp>,
}
```

For graph relationships in `RelationshipGraph`, embed temporal data directly in the stored tuples/maps (e.g., `allies: BTreeMap<(Entity, Entity), RelationshipMeta>` instead of `BTreeSet<(Entity, Entity)>`).

The `EventLog` also records `StateChange::RelationshipStarted`/`RelationshipEnded`, so temporal data is reconstructable from the audit trail if needed.

### Relationship Idempotency

The current `World::add_relationship()` is **idempotent** — it no-ops if an active relationship of the same kind already exists between the two entities. The `AddRelationship` command in the command applicator **must** preserve this behavior. This is especially important for bidirectional relationships (Ally, Enemy, AtWar) where both directions need to be checked before creating a new relationship.

### World-Level Resources

```rust
#[derive(Resource)] pub struct SimClock { pub time: SimTimestamp, pub tick_count: u64 }
#[derive(Resource)] pub struct SimConfig { pub start_year: u32, pub num_years: u32, pub seed: u64, pub flush_interval: Option<u32>, pub output_dir: PathBuf }
#[derive(Resource)] pub struct IdGenerator { ... }  // generates stable u64 sim IDs
#[derive(Resource)] pub struct SimRng(pub SmallRng);
#[derive(Resource)] pub struct RelationshipGraph { ... }
#[derive(Resource)] pub struct RegionAdjacency { ... }
#[derive(Resource)] pub struct PendingActions(pub Vec<Action>);
#[derive(Resource)] pub struct ActionResults(pub Vec<ActionResult>);

// Bi-directional entity mapping — critical for all systems
#[derive(Resource)]
pub struct SimEntityMap {
    /// Stable sim ID (u64) → Bevy Entity handle
    to_bevy: BTreeMap<u64, Entity>,
    /// Bevy Entity handle → stable sim ID (u64)
    to_sim: BTreeMap<Entity, u64>,
}

// Event log — accumulates between flushes
#[derive(Resource)]
pub struct EventLog {
    pub events: Vec<Event>,
    pub participants: Vec<EventParticipant>,
    pub effects: Vec<EventEffect>,
}
```

**`SimEntityMap`** is essential because systems need to resolve sim-ID references (e.g., from relationship targets, `ArmyData.faction_id`, `KnowledgeData.origin_settlement_id`) into Bevy `Entity` handles for queries. Every entity spawn must register in this map. Every entity end should mark it (but not remove — historical queries need the mapping for JSONL flush).

**SimEntityMap growth strategy:** Over a 1000+ year simulation with thousands of births/deaths, army musters/disbands, and item creations, this map grows without bound. The expected entity count over 1000 years (~tens of thousands) is small enough that unbounded growth is acceptable — accept the memory cost. If profiling shows this becomes an issue, consider post-flush cleanup: after a flush checkpoint, remove ended entities that have already been serialized from `SimEntityMap` (the flush has already converted Bevy Entity handles to sim-IDs).

**`EventLog`** preserves the current audit trail:
- `events` — same `Event` struct (id, kind, timestamp, description, caused_by, data)
- `participants` — same `EventParticipant` struct (event_id, entity_id, role)
- `effects` — same `EventEffect` struct with `StateChange` variants (`EntityCreated`, `EntityEnded`, `NameChanged`, `RelationshipStarted`, `RelationshipEnded`, `PropertyChanged`, `Custom`)

### Migration Steps

1. Add `#[derive(Component)]` to all data structs (PersonData, SettlementData, etc.)
2. Define marker components for each EntityKind
3. Define the component decomposition for SettlementData, FactionData, and PersonData per the field mapping above
4. Define the `RelationshipGraph` resource with temporal data (`RelationshipMeta`)
5. Replace `Entity.extra` HashMap with an `IsPlayer` marker component (the only current usage is `IS_PLAYER`). **Mod extensibility:** `Entity.extra` is the designated 3rd-party plugin/mod extension point per CLAUDE.md. In Bevy, mod extensibility is handled naturally by Bevy's component system — mods define their own `Component` types and insert them on entities. This preserves the design intent without carrying forward the HashMap.
6. Write a `spawn_entity()` helper that creates a Bevy entity with the right bundle
7. Port `World::new()` → app initialization with resources
8. Port `World::queue_action()` → `PendingActions` resource (`pending_actions.0.push(action)`)
9. Port worldgen to spawn entities into the Bevy world instead of inserting into BTreeMap (see WorldGenPipeline note below)
10. Write round-trip tests: spawn entities, query them back, verify data integrity

**WorldGenPipeline note:** The existing `WorldGenPipeline` builder pattern (`pipeline.step("name", fn).run()`) and the `make_test_world()` helper also need migration. The pipeline struct should be adapted to spawn into `bevy_ecs::World` instead of the current monolithic `World`.

### Deliverable
All entity data expressible as Bevy components. Worldgen spawns into Bevy world. Queries can retrieve all settlements, all persons in a faction, etc.

---

## 7. Phase 2: Event Architecture — Command Events

**Goal:** Define the `SimCommand` event type and the centralized command applicator. This is the backbone of the new architecture.

### SimCommand Design

Every mutation that a system wants to perform is expressed as a `SimCommand`. Commands are **intent-based** — they describe what happened in simulation terms, not what fields to change. This makes the command stream meaningful as a historical event log for Postgres queries ("DraftSoldiers" is a queryable event; "AdjustPopulation bracket 2 delta -50" is not).

**Common fields** shared by all commands via a wrapper:

```rust
#[derive(Event, Clone)]
pub struct SimCommand {
    pub kind: SimCommandKind,
    pub description: String,
    pub caused_by: Option<u64>,  // event_id for causal chains
    pub event_kind: EventKind,   // what EventKind to record in EventLog
    pub participants: Vec<(Entity, ParticipantRole)>,  // event participants
    pub event_data: serde_json::Value,  // structured event metadata (Event.data field)
}
```

```rust
pub enum SimCommandKind {
    // --- Entity Lifecycle ---
    SpawnEntity {
        kind: EntityKind,
        name: String,
        origin: Option<SimTimestamp>,
        data: EntityData,
    },
    EndEntity {
        entity: Entity,  // Bevy Entity
    },
    RenameEntity {
        entity: Entity,
        new_name: String,
    },

    // --- Relationships ---
    AddRelationship {
        source: Entity,
        target: Entity,
        kind: RelationshipKind,
    },
    EndRelationship {
        source: Entity,
        target: Entity,
        kind: RelationshipKind,
    },

    // --- Intent-Based Mutations (examples) ---
    // Demographics
    GrowPopulation { settlement: Entity, bracket: usize, delta: i32 },
    PersonDied { person: Entity },
    PersonBorn { settlement: Entity, name: String, sex: Sex, role: Role },
    Marriage { person_a: Entity, person_b: Entity },

    // Economy
    CollectTaxes { faction: Entity, amount: f64 },
    EstablishTradeRoute { from: Entity, to: Entity, route: TradeRoute },
    SeverTradeRoute { from: Entity, to: Entity },
    PayArmyMaintenance { faction: Entity, amount: f64 },
    UpdateProduction { settlement: Entity, production: BTreeMap<ResourceType, f64> },

    // Military
    DeclareWar { attacker: Entity, defender: Entity, war_goal: WarGoal },
    MusterArmy { faction: Entity, settlement: Entity, strength: u32 },
    MarchArmy { army: Entity, target_region: Entity },
    ResolveBattle { attacker_army: Entity, defender_army: Entity },
    BeginSiege { army: Entity, settlement: Entity },
    ResolveAssault { army: Entity, settlement: Entity },
    CaptureSettlement { settlement: Entity, old_faction: Entity, new_faction: Entity },
    SignTreaty { winner: Entity, loser: Entity, reparations: f64, tribute_years: u32 },

    // Politics
    SucceedLeader { faction: Entity, new_leader: Entity },
    AttemptCoup { faction: Entity, instigator: Entity },
    FormAlliance { faction_a: Entity, faction_b: Entity },
    BetrayAlliance { betrayer: Entity, victim: Entity },
    SplitFaction { parent_faction: Entity, settlement: Entity },

    // Culture/Religion
    CulturalShift { settlement: Entity, old_culture: Entity, new_culture: Entity },
    FoundReligion { settlement: Entity, founder: Option<Entity> },
    ReligiousSchism { parent: Entity, settlement: Entity },
    ConvertFaction { faction: Entity, new_religion: Entity },

    // Knowledge
    CreateKnowledge { settlement: Entity, category: KnowledgeCategory, significance: f64 },
    CreateManifestation { knowledge: Entity, settlement: Entity, medium: Medium },
    DestroyManifestation { manifestation: Entity, cause: String },
    RevealSecret { knowledge: Entity, keeper: Entity },

    // Items
    CraftItem { settlement: Entity, crafter: Option<Entity>, item_type: ItemType },
    TransferItem { item: Entity, old_holder: Entity, new_holder: Entity },

    // Crime
    FormBanditGang { region: Entity },
    BanditRaid { bandit_faction: Entity, settlement: Entity },

    // Disease
    StartPlague { settlement: Entity, disease: Entity },
    EndPlague { settlement: Entity },
    SpreadPlague { from: Entity, to: Entity, disease: Entity },
    UpdateInfection { settlement: Entity, infection_rate: f64, deaths: u32 },

    // Environment/Disaster
    TriggerDisaster { settlement: Entity, disaster_type: DisasterType, severity: f64 },
    StartPersistentDisaster { settlement: Entity, disaster_type: DisasterType, severity: f64, months: u32 },
    EndDisaster { settlement: Entity },

    // Migration
    MigratePopulation { source: Entity, destination: Entity, count: u32 },
    RelocatePerson { person: Entity, old_settlement: Entity, new_settlement: Entity },

    // Buildings (additional)
    DamageBuilding { building: Entity, damage: f64, cause: String },
    UpgradeBuilding { building: Entity },

    // Reputation/Prestige (or use SetField)
    AdjustPrestige { entity: Entity, delta: f64, source: String },

    // Generic field mutation — fallback for simple changes that don't need
    // a dedicated intent variant (e.g., adjusting prestige, setting literacy)
    SetField {
        entity: Entity,
        field: String,
        old_value: serde_json::Value,
        new_value: serde_json::Value,
    },
}
```

This design ensures:
- Each command is a meaningful historical event (queryable in Postgres)
- The command applicator knows exactly what state changes to apply and what `EventEffect`s to record
- `SetField` serves as an escape hatch for simple property changes without creating a dedicated variant for every field

### SimCommandKind → EventKind Mapping

Each `SimCommand` carries an `event_kind: EventKind` field. The mapping from command variant to event kind is:

| SimCommandKind | EventKind | Notes |
|---------------|-----------|-------|
| `SpawnEntity` | Varies by entity kind | `Birth`, `SettlementFounded`, `FactionFormed`, `MercenaryFormed`, etc. |
| `EndEntity` | `Death` (persons), varies | |
| `RenameEntity` | `Renamed` | |
| `DeclareWar` | `WarDeclared` | |
| `SignTreaty` | `Treaty` | |
| `CaptureSettlement` | `Conquest` | |
| `PersonDied` | `Death` | |
| `PersonBorn` | `Birth` | |
| `Marriage` | `Union` | |
| `MusterArmy` | `Muster` | |
| `MarchArmy` | `March` | |
| `ResolveBattle` | `Battle` | |
| `BeginSiege` | `Siege` | |
| `ResolveAssault` | `Assault` | |
| `SucceedLeader` | `Succession` | |
| `AttemptCoup` | `Coup` or `FailedCoup` | Depends on outcome |
| `FormAlliance` | `Alliance` | |
| `BetrayAlliance` | `Betrayal` | |
| `SplitFaction` | `Dissolution` | |
| `CulturalShift` | `CulturalShift` | |
| `FoundReligion` | `Founded` | |
| `ReligiousSchism` | `Schism` | |
| `ConvertFaction` | `Conversion` | |
| `CreateKnowledge` | `Discovery` | |
| `CreateManifestation` | `Propagation` or `Transcription` | Depends on derivation method |
| `CraftItem` | `Crafted` | |
| `TransferItem` | — | No dedicated EventKind; uses `event_data` |
| `FormBanditGang` | `BanditFormed` | |
| `BanditRaid` | `Raid` | |
| `RevealSecret` | `SecretRevealed` | |
| `EstablishTradeRoute` | `TradeEstablished` | |
| `SeverTradeRoute` | — | No dedicated EventKind; informational |
| `CollectTaxes` | — | Bookkeeping — `EventEffect`s only, no `Event` entry |
| `PayArmyMaintenance` | — | Bookkeeping — `EventEffect`s only, no `Event` entry |
| `GrowPopulation` | — | Bookkeeping — `EventEffect`s only, no `Event` entry |
| `StartPlague` | `Disaster` | |
| `EndPlague` | `Disaster` | |
| `TriggerDisaster` | `Disaster` | |
| `StartPersistentDisaster` | `Disaster` | |
| `EndDisaster` | `Disaster` | |
| `MigratePopulation` | `Migration` | |
| `UpgradeBuilding` | `Upgrade` | |
| `SetField` | — | Bookkeeping — `EventEffect`s only, no `Event` entry |

Not every `SimCommandKind` variant needs its own `Event` entry. "Bookkeeping" commands produce `EventEffect`s for the audit trail but don't create full `Event` entries in the event log.

**All 68 EventKind variants** (for reference):
```
Birth, Death, SettlementFounded, FactionFormed, Union, Dissolution, Joined, Left,
Succession, Conquest, Coup, WarDeclared, Battle, Siege, Treaty, Migration, Exile,
Abandoned, Construction, Destruction, Crafted, Discovery, Schism, Disaster, Burial,
Ceremony, Renamed, CulturalShift, Rebellion, SuccessionCrisis, Muster, March,
Retreat, Attrition, Assault, TreatyBroken, BanditFormed, Raid, FailedCoup,
Election, Rivalry, Betrayal, Defection, TrustRecovered, Assassination, Alliance,
Intrigue, TradeEstablished, TributeEnded, TributeDefaulted, Upgrade, Propagation,
Transcription, SecretRevealed, SecretLeaked, SecretCaptured, Founded, CultureBlended,
Prophecy, Conversion, Genesis, ExpansionWar, MercenaryFormed, MercenaryHired,
MercenaryDeserted, MercenarySwitched, MercenaryDisbanded, Custom(String)
```

**SetField granularity guidance:** Systems like Reputation that adjust prestige by small deltas on every entity every tick should **not** create hundreds of `SetField` commands. Instead, use a dedicated intent command (e.g., `DecayPrestige { entity, delta }`) that the applicator can batch or treat as a "silent" bookkeeping change (recording `EventEffect`s but not creating an `Event` entry). Reserve `SetField` for truly ad-hoc changes. The goal is to keep the event log meaningful — "prestige decayed by 0.01" on 200 entities is noise, not history.

### Command Applicator

A single system in `PostUpdate` processes all commands. For each command it:
1. Creates an `Event` in the `EventLog` (preserving `caused_by` chains)
2. Records `EventParticipant` entries (from the command's `participants` field)
3. Applies state changes to components
4. Records `EventEffect` entries (the audit trail for Postgres)
5. Emits `SimReactiveEvent` if other systems need to react

```rust
fn apply_sim_commands(
    mut commands: Commands,
    mut cmd_reader: EventReader<SimCommand>,
    mut event_log: ResMut<EventLog>,
    mut id_gen: ResMut<IdGenerator>,
    mut entity_map: ResMut<SimEntityMap>,
    mut rel_graph: ResMut<RelationshipGraph>,
    mut region_adj: ResMut<RegionAdjacency>,
    clock: Res<SimClock>,
    // Component queries for each decomposed type
    mut sim_entities: Query<&mut SimEntity>,
    mut settlement_core: Query<&mut SettlementCore>,
    mut faction_core: Query<&mut FactionCore>,
    mut person_core: Query<&mut PersonCore>,
    mut armies: Query<&mut ArmyData>,
    // ... other component queries as needed
    mut reactive_events: EventWriter<SimReactiveEvent>,
) {
    for cmd in cmd_reader.read() {
        // 1. Create event in log
        let event_id = id_gen.next_id();
        event_log.events.push(Event {
            id: event_id,
            kind: cmd.event_kind.clone(),
            timestamp: clock.time,
            description: cmd.description.clone(),
            caused_by: cmd.caused_by,
            data: cmd.event_data.clone(),
        });

        // 2. Record participants
        for (entity, role) in &cmd.participants {
            if let Some(&sim_id) = entity_map.to_sim.get(entity) {
                event_log.participants.push(EventParticipant {
                    event_id,
                    entity_id: sim_id,
                    role: role.clone(),
                });
            }
        }

        // 3-4. Apply state changes + record effects (per command kind)
        match &cmd.kind {
            SimCommandKind::EndEntity { entity } => {
                if let Ok(mut sim) = sim_entities.get_mut(*entity) {
                    sim.end = Some(clock.time);
                    let sim_id = entity_map.to_sim[entity];
                    event_log.effects.push(EventEffect {
                        event_id,
                        entity_id: sim_id,
                        effect: StateChange::EntityEnded,
                    });
                }
                // 5. Emit reactive event
                reactive_events.send(SimReactiveEvent::EntityDied {
                    entity: *entity, event_id
                });
            }
            // ... handle each command variant
        }
    }
}
```

**Key invariant:** The command applicator is the **only** code that mutates entity components during the tick. All Update systems are read-only with respect to components — they only write to `EventWriter<SimCommand>`.

### Command Applicator Decomposition

The `apply_sim_commands` function will need to handle 40+ `SimCommandKind` variants, each with different state changes, event effects, and reactive event emissions. To avoid a 2000+ LOC monolith, decompose into sub-functions by domain:

```
apply_sim_commands (orchestrator)
├── apply_entity_lifecycle(SpawnEntity, EndEntity, RenameEntity)
├── apply_relationship(AddRelationship, EndRelationship)
├── apply_demographics(GrowPopulation, PersonDied, PersonBorn, Marriage)
├── apply_economy(CollectTaxes, EstablishTradeRoute, SeverTradeRoute, PayArmyMaintenance, UpdateProduction)
├── apply_military(DeclareWar, MusterArmy, MarchArmy, ResolveBattle, BeginSiege, ResolveAssault, CaptureSettlement, SignTreaty)
├── apply_politics(SucceedLeader, AttemptCoup, FormAlliance, BetrayAlliance, SplitFaction)
├── apply_culture(CulturalShift, FoundReligion, ReligiousSchism, ConvertFaction)
├── apply_knowledge(CreateKnowledge, CreateManifestation, DestroyManifestation, RevealSecret)
├── apply_items(CraftItem, TransferItem)
├── apply_crime(FormBanditGang, BanditRaid)
├── apply_disease(StartPlague, EndPlague, SpreadPlague, UpdateInfection)
├── apply_disaster(TriggerDisaster, StartPersistentDisaster, EndDisaster)
├── apply_migration(MigratePopulation, RelocatePerson)
├── apply_buildings(DamageBuilding, UpgradeBuilding)
├── apply_reputation(AdjustPrestige)
└── apply_set_field(SetField)
```

Each sub-function takes the same shared resources (`EventLog`, `IdGenerator`, `SimEntityMap`, etc.) and returns a list of `SimReactiveEvent`s to emit. The orchestrator collects and sends all reactive events after all commands from the current tick are processed.

### Reactive Events (replacing Signals)

```rust
#[derive(Event)]
pub enum SimReactiveEvent {
    // --- War & Conflict ---
    WarStarted { attacker: Entity, defender: Entity, event_id: u64 },
    WarEnded { winner: Entity, loser: Entity, event_id: u64 },
    SettlementCaptured { settlement: Entity, old_faction: Entity, new_faction: Entity, event_id: u64 },
    LeaderVacancy { faction: Entity, previous_leader: Entity, event_id: u64 },
    SiegeStarted { army: Entity, settlement: Entity, event_id: u64 },
    SiegeEnded { settlement: Entity, event_id: u64 },
    AllianceBetrayed { betrayer: Entity, victim: Entity, event_id: u64 },
    SuccessionCrisis { faction: Entity, event_id: u64 },

    // --- Faction & Politics ---
    FactionSplit { parent: Entity, child: Entity, event_id: u64 },
    CulturalRebellion { settlement: Entity, event_id: u64 },
    SecretRevealed { knowledge: Entity, keeper: Entity, event_id: u64 },

    // --- Demographics & Migration ---
    EntityDied { entity: Entity, event_id: u64 },
    RefugeesArrived { settlement: Entity, source: Entity, event_id: u64 },

    // --- Disease & Disaster ---
    PlagueStarted { settlement: Entity, disease: Entity, event_id: u64 },
    PlagueEnded { settlement: Entity, event_id: u64 },
    DisasterStruck { settlement: Entity, event_id: u64 },
    DisasterStarted { settlement: Entity, event_id: u64 },
    DisasterEnded { settlement: Entity, event_id: u64 },

    // --- Economy ---
    TradeRouteEstablished { from: Entity, to: Entity, event_id: u64 },
    TradeRouteRaided { settlement: Entity, event_id: u64 },
    TreasuryDepleted { faction: Entity, event_id: u64 },
    BanditRaid { settlement: Entity, event_id: u64 },
    BanditGangFormed { region: Entity, event_id: u64 },

    // --- Buildings ---
    BuildingConstructed { building: Entity, settlement: Entity, event_id: u64 },
    BuildingUpgraded { building: Entity, event_id: u64 },

    // --- Knowledge & Items ---
    KnowledgeCreated { knowledge: Entity, event_id: u64 },
    ManifestationCreated { manifestation: Entity, event_id: u64 },
    ItemCrafted { item: Entity, event_id: u64 },
    ItemTierPromoted { item: Entity, event_id: u64 },

    // --- Religion ---
    ReligionFounded { religion: Entity, event_id: u64 },
    ReligionSchism { parent: Entity, child: Entity, event_id: u64 },
    ProphecyDeclared { settlement: Entity, event_id: u64 },

    // --- Special ---
    FailedCoup { faction: Entity, instigator: Entity, event_id: u64 },
}
```

All 33 active signals have explicit variants. The 14 log-only signals (see [Signal Flow Map](#8-signal-flow-map--producerconsumer-reference)) do **not** need variants — they are recorded in the `EventLog` by the command applicator but no system reacts to them.

These reactive events are consumed in a `Reactions` system set that runs after command application but still within `PostUpdate`:

```rust
SimPhase::PostUpdate
    ├── ApplyCommands (apply_sim_commands)
    ├── apply_deferred  // sync point
    └── Reactions
        ├── handle_leader_vacancy  // reads SimReactiveEvent, emits more SimCommands
        ├── handle_war_started
        └── ...
```

Reactions can emit additional `SimCommand`s. To prevent infinite cascading, limit to **1 reaction pass**, matching the current single-pass non-cascading behavior. If a reaction needs to propagate further, it should mutate state that a later tick's Update phase will observe.

### Causal Chain Flow

The current `World::add_caused_event()` validates that the cause event exists and that timestamps are non-decreasing. The `SimCommand` carries `caused_by: Option<u64>` (an event_id) and `SimReactiveEvent` variants carry `event_id: u64`. Reaction handlers use the triggering event's `event_id` as `caused_by` in their own commands, preserving causal chains across the command/reaction boundary:

```
System emits SimCommand(caused_by: None)
  → Applicator creates Event(id: 42), emits SimReactiveEvent(event_id: 42)
  → Reaction handler reads event_id: 42, emits SimCommand(caused_by: Some(42))
  → Applicator creates Event(id: 43, caused_by: Some(42))
```

This ensures causal chains (used heavily by Postgres queries) are preserved. **All `SimReactiveEvent` variants must carry `event_id: u64`** — this is the architectural invariant that makes causal chain propagation work.

### Design Decision: Outcome Determination

Several system mutations are conditional — they check state, roll RNG, and branch (e.g., coup attempts depend on stability/legitimacy/traits/RNG; battle resolution depends on army strengths/terrain/morale/RNG; alliance betrayal depends on vulnerability/trust/RNG).

In the current architecture, the system reads state, rolls dice, and applies the result in one step. In the command-event pattern, the system reads state and emits a command — but **who determines the outcome?**

**Chosen approach: Systems determine outcomes, emit result-specific commands.** The system reads state, performs the RNG roll, and emits the appropriate result command (e.g., `SucceedLeader` for successful succession, `AttemptCoup` only when it succeeds, `FailedCoup` as a separate reactive event). The command applicator just applies the pre-determined outcome — it does not contain game logic or require RNG access.

This means:
- `apply_sim_commands` does **not** need `ResMut<SimRng>` access
- The applicator is a pure state-application function, making it easy to test
- Systems encapsulate all decision-making logic (consistent with current architecture)
- `AttemptCoup` as a `SimCommandKind` variant represents a **successful** coup (the system already determined success); failed coups emit only a `FailedCoup` reactive event without a state-mutating command
- `ResolveBattle` carries pre-computed results (casualties, morale changes, winner) — the system did the resolution, the applicator applies it

### TickContext → Bevy Parameter Mapping

The current `TickContext` provides systems with access to the world, RNG, and signals. In Bevy, these map to system parameters:

| Current (`TickContext`) | Bevy Equivalent | Notes |
|-------------------------|----------------|-------|
| `ctx.world` | Component queries (`Query<&T>`, `Query<&mut T>`) | Systems declare exactly the components they need |
| `ctx.rng` | `ResMut<SimRng>` or per-system `Local<SmallRng>` | See [Determinism Strategy](#16-determinism-strategy) for seeding protocol |
| `ctx.signals` | `EventWriter<SimCommand>` | Commands replace direct signal emission |
| `ctx.inbox` | `EventReader<SimReactiveEvent>` | Reactive events replace signal inbox |

### record_change() Pattern Migration

Currently, systems directly mutate entity fields and then call `world.record_change(entity_id, event_id, field, old, new)` for the audit trail. This is pervasive (used in nearly every system). In the command-event pattern, the command applicator handles recording:

- **Intent-based commands** (e.g., `DraftSoldiers`, `CaptureSettlement`): The applicator knows which fields to change and records `EventEffect::PropertyChanged` for each field it modifies. Each intent command produces a meaningful `Event` in the log.
- **SetField commands** (escape hatch): For simple property changes that don't warrant a dedicated command variant (e.g., prestige decay, literacy drift), `SetField` carries the old/new values and the applicator records them as `PropertyChanged` effects.
- **Silent changes**: Some field changes (like adjusting prestige by small deltas every tick) are not historical events. These should use `SetField` commands but the applicator should batch them into a single `Event` per entity per tick, or skip `Event` creation while still recording `EventEffect`s for the audit trail.

The key question: **not every field mutation needs its own Event entry, but every mutation needs an EventEffect for Postgres auditability.** The command applicator should distinguish between event-worthy commands (which create Events with participants) and bookkeeping commands (which only create EventEffects).

### Command Ordering and Conflict Resolution

Multiple systems may emit conflicting commands for the same entity in a single tick (e.g., Demographics marks a person as dead via old age, while Conflicts kills them in battle). The command applicator must handle this:

- **Processing order:** Commands are processed in the order systems run within the schedule. In `MultiThreaded` mode, the order of systems within a parallel group is non-deterministic, but the command applicator processes all commands from a given tick atomically.
- **Conflict resolution:** Use idempotent checks. The first `EndEntity` command for a given entity succeeds; subsequent `EndEntity` commands for the same entity in the same tick are no-ops (log a warning but don't panic). This matches the current `end_entity()` behavior but gracefully handles the race condition.
- **Deterministic mode:** With `SimpleExecutor`, commands are processed in system insertion order, making conflicts deterministic.

### Migration Steps

1. Define `SimCommand` enum with all mutation variants
2. Define mutation sub-enums (SettlementMutation, FactionMutation, etc.)
3. Define `SimReactiveEvent` enum (33 active variants + 14 log-only that don't need variants)
4. Implement `apply_sim_commands` system
5. Implement reaction handlers (port current `handle_signals` logic)
6. Write tests: emit commands → verify world state changes → verify reactive events fire

### Deliverable
The command pipeline works end-to-end. A test can emit a `SimCommand::EndEntity` and verify the entity is marked ended, the event log records it (with `EventEffect::EntityEnded`), and a `SimReactiveEvent::EntityDied` is emitted.

---

## 8. Signal Flow Map — Producer/Consumer Reference

This section maps every current `SignalKind` to its producers (which system emits it) and consumers (which systems handle it in `handle_signals()`). This determines which signals become `SimReactiveEvent` variants with handler systems, and which are **log-only** (emitted for the event log but no system reacts to them).

### Signals With Active Consumers (→ need `SimReactiveEvent` + reaction handlers)

Every entry below is verified against the actual `handle_signals()` code in each system via `grep SignalKind::`.

| Signal | Produced By | Consumed By |
|--------|------------|-------------|
| `WarStarted` | Conflicts | Economy, Politics, Agency |
| `WarEnded` | Conflicts | Crime, Politics, Reputation, Knowledge, Agency |
| `SettlementCaptured` | Conflicts | Buildings, Economy, Crime, Disease, Culture, Religion, Politics, Reputation, Items, Knowledge, Agency |
| `LeaderVacancy` | Conflicts, Demographics | Politics, Agency |
| `FactionSplit` | Culture, Politics | Culture, Religion, Politics, Reputation, Knowledge, Agency |
| `RefugeesArrived` | Migration | Disease, Culture, Religion, Politics |
| `EntityDied` | Disease, Conflicts | Items, Knowledge, Reputation |
| `PlagueStarted` | Disease | Economy, Politics |
| `PlagueEnded` | Disease | Crime, Reputation, Knowledge |
| `SiegeStarted` | Conflicts | Economy, Disease, Politics |
| `SiegeEnded` | Conflicts | Disease, Politics, Items, Reputation, Knowledge |
| `DisasterStruck` | Environment | Economy, Crime, Disease, Religion, Politics, Reputation, Knowledge |
| `DisasterStarted` | Environment | Economy, Politics |
| `DisasterEnded` | Environment | Disease, Politics, Reputation |
| `AllianceBetrayed` | Politics | Politics, Reputation, Knowledge, Agency |
| `SuccessionCrisis` | Politics | Reputation, Knowledge, Agency |
| `CulturalRebellion` | Culture | Politics, Reputation, Knowledge |
| `BanditRaid` | Crime | Economy, Items, Politics, Reputation |
| `BanditGangFormed` | Crime | Politics, Reputation |
| `TradeRouteRaided` | Crime | Politics |
| `TradeRouteEstablished` | Economy | Culture, Religion, Reputation |
| `BuildingConstructed` | Buildings | Religion, Reputation, Knowledge |
| `BuildingUpgraded` | Buildings | Reputation |
| `KnowledgeCreated` | Knowledge | Reputation, Knowledge (self) |
| `ManifestationCreated` | Knowledge | Knowledge (self) |
| `ItemCrafted` | Items | Knowledge |
| `ItemTierPromoted` | Items | Reputation, Knowledge |
| `ReligionFounded` | Religion | Reputation, Knowledge |
| `ReligionSchism` | Religion | Reputation, Knowledge |
| `ProphecyDeclared` | Religion | Reputation |
| `TreasuryDepleted` | Economy | Reputation |
| `SecretRevealed` | Knowledge | Politics, Reputation |
| `Custom("failed_coup")` | Politics/Coups | Knowledge |

**Notable consumers by volume:** Reputation handles **23** signal types, Knowledge handles **18**, Politics handles **17**. These three systems account for the majority of reactive event handlers.

**Knowledge self-consumption:** Knowledge consumes its own `KnowledgeCreated` and `ManifestationCreated` signals. In the current code, signals from Phase 1 (tick) are delivered in Phase 2 (handle_signals) of the same tick. In Bevy, these self-referential signals will be emitted by the command applicator and consumed in the Reactions set within the same PostUpdate cycle. The 1-reaction-pass limit means Knowledge's reaction handlers can read reactive events emitted by `apply_sim_commands` but cannot cascade further — any knowledge created by reactions won't trigger additional knowledge reactions until the next tick.

### Signals Without Consumers (→ log-only, no `SimReactiveEvent` needed)

These signals are emitted for the historical event log but no system currently handles them in `handle_signals()`. They should be recorded as events in the `EventLog` by the command applicator but do **not** need reactive event variants:

| Signal | Produced By | Notes |
|--------|------------|-------|
| `PopulationChanged` | Disease, Demographics | No system handles this via handle_signals; systems read population state directly |
| `ResourceDepleted` | Economy | Rare; systems read deposit state directly |
| `TradeRouteSevered` | Economy | Informational for event log |
| `CulturalShift` | Migration, Culture | No system handles this via handle_signals; used for event log |
| `PlagueSpreading` | Disease | Informational for event log |
| `BuildingDestroyed` | Buildings | Informational for event log |
| `PrestigeThresholdCrossed` | Reputation | Informational for event log |
| `ManifestationDestroyed` | Knowledge | Informational for event log |
| `ItemTransferred` | Items | Informational for event log |
| `ReligiousShift` | Religion | Informational for event log |
| `FactionConverted` | Religion | Informational for event log |
| `MercenaryHired` | Politics | Informational for event log |
| `MercenaryDeserted` | Politics | Informational for event log |
| `MercenaryContractEnded` | Politics | Informational for event log |

**Summary:** 33 active signals need `SimReactiveEvent` variants and handler systems. 14 log-only signals are naturally handled by the command applicator recording them in the `EventLog`.

---

## 9. Phase 3: System Migration Order

**Goal:** Port all 17 current `SimSystem` implementations to 18 Bevy systems (KnowledgeDerivation is promoted from a sub-module of KnowledgeSystem to a standalone system), one at a time. Each migrated system emits `SimCommand`s instead of mutating `World` directly.

### Migration order

Systems are ordered by dependency: leaf systems (no signal dependencies) first, then systems that react to signals, then highly-connected systems last.

#### Wave 1: Read-Only Leaf Systems (no cross-system dependencies)

These systems read entity state and emit commands. They don't consume signals from other systems.

1. **EnvironmentSystem** → `EnvironmentPlugin` — **Monthly**
   - Reads: regions, settlements (seasonal data)
   - Emits: season updates, disaster commands (`DisasterStruck`, `DisasterStarted`, `DisasterEnded`)
   - Reacts to: nothing (has `handle_signals` but it's a no-op — no reactive handlers needed)
   - Why first: self-contained, affects other systems via seasonal modifiers but doesn't depend on any

2. **BuildingSystem** → `BuildingPlugin` — **Yearly**
   - Reads: settlements, buildings
   - Emits: construction, decay, upgrade commands (`BuildingConstructed`, `BuildingDestroyed`, `BuildingUpgraded`)
   - Reacts to: `SettlementCaptured` (conquest damages buildings)
   - Why early: simple lifecycle logic, output feeds economy

3. **EducationSystem** → `EducationPlugin` — **Yearly**
   - Reads: settlements, persons, buildings
   - Emits: literacy changes, education updates
   - Why early: minimal dependencies

#### Wave 2: Economic & Demographic Core

4. **DemographicsSystem** → `DemographicsPlugin` — **Yearly**
   - Reads: settlements, persons, regions, factions
   - Emits: birth, death, marriage, aging commands (`PopulationChanged`, `EntityDied`)
   - Does NOT handle signals (reads state directly for war/disaster mortality modifiers)

5. **EconomySystem** → `EconomyPlugin` — **Monthly**
   - Reads: settlements, factions, regions, buildings, trade routes
   - Emits: production, taxation, treasury, trade route commands (`TradeRouteEstablished`, `TradeRouteSevered`, `TreasuryDepleted`, `ResourceDepleted`)
   - Reacts to (7 signals): `WarStarted` (severs cross-faction trade), `SettlementCaptured` (adjusts trade routes), `PlagueStarted` (severs trade routes), `SiegeStarted` (severs trade routes), `DisasterStruck` (severs trade routes), `DisasterStarted` (severs trade routes), `BanditRaid` (reduces prosperity)

6. **DiseaseSystem** → `DiseasePlugin` — **Yearly**
   - Reads: settlements, diseases, trade routes
   - Emits: outbreak, spread, recovery commands (`PlagueStarted`, `PlagueSpreading`, `PlagueEnded`, `PopulationChanged`, `EntityDied`)
   - Reacts to: `RefugeesArrived`, `SettlementCaptured`, `SiegeStarted`, `SiegeEnded`, `DisasterStruck`, `DisasterEnded`

#### Wave 3: Social Systems

7. **CultureSystem** → `CulturePlugin` — **Yearly**
   - Reads: settlements, cultures, factions
   - Emits: cultural shift, blending, rebellion commands (`CulturalShift`, `CulturalRebellion`, `FactionSplit`)
   - Reacts to (4 signals): `SettlementCaptured` (cultural disruption from conquest), `RefugeesArrived` (culture pressure from refugees), `TradeRouteEstablished` (culture drift along trade), `FactionSplit` (culture pressure on new faction)

8. **ReligionSystem** → `ReligionPlugin` — **Yearly**
   - Reads: settlements, religions, deities, factions
   - Emits: conversion, schism, prophecy commands (`ReligionFounded`, `ReligionSchism`, `ReligiousShift`, `ProphecyDeclared`, `FactionConverted`)
   - Reacts to (6 signals): `SettlementCaptured` (religious disruption from conquest), `RefugeesArrived` (religion pressure from refugees), `TradeRouteEstablished` (religion drift along trade), `FactionSplit` (religion pressure on new faction), `BuildingConstructed` (temple bonus), `DisasterStruck` (fervor spike)

9. **ReputationSystem** → `ReputationPlugin` (decomposed into subsystems — see Phase 4) — **Yearly**
   - Reads: persons, factions, settlements
   - Emits: prestige changes, tier promotions (`PrestigeThresholdCrossed`)
   - Reacts to (23 signals): `WarEnded`, `SettlementCaptured`, `SiegeEnded`, `BuildingConstructed`, `BuildingUpgraded`, `TradeRouteEstablished`, `PlagueEnded`, `FactionSplit`, `CulturalRebellion`, `TreasuryDepleted`, `EntityDied`, `DisasterStruck`, `DisasterEnded`, `KnowledgeCreated`, `BanditGangFormed`, `BanditRaid`, `ItemTierPromoted`, `ReligionSchism`, `ProphecyDeclared`, `ReligionFounded`, `AllianceBetrayed`, `SuccessionCrisis`, `SecretRevealed`
   - **Note:** Reputation is the largest signal consumer. Each signal triggers prestige adjustments (bonuses or penalties) for involved entities. See Phase 4 for decomposition plan.

10. **CrimeSystem** → `CrimePlugin` — **Yearly**
    - Reads: settlements, factions, regions
    - Emits: crime rate changes, bandit formation, raid commands (`BanditGangFormed`, `BanditRaid`, `TradeRouteRaided`)
    - Reacts to: `SettlementCaptured`, `WarEnded`, `PlagueEnded`, `DisasterStruck`
    - Each reaction calls `apply_crime_spike()` with different multiplier constants: `SettlementCaptured` → `CRIME_SPIKE_CONQUEST`, `WarEnded` (decisive, loser) → `CRIME_SPIKE_WAR_LOSS`, `PlagueEnded` (deaths > threshold) → `CRIME_SPIKE_PLAGUE`, `DisasterStruck` → `CRIME_SPIKE_DISASTER * severity`

#### Wave 4: Knowledge & Items

11. **KnowledgeSystem** → `KnowledgePlugin` — **Yearly**
    - Reads: events (recent), settlements, manifestations, persons
    - Emits: knowledge creation, propagation commands (`KnowledgeCreated`, `ManifestationCreated`, `ManifestationDestroyed`, `SecretRevealed`)
    - Reacts to (18 signals): `EntityDied` (manifestation holder death), `SettlementCaptured` (manifestation destruction/transfer), `KnowledgeCreated` (self — propagation chains), `ManifestationCreated` (self — propagation chains), `WarEnded`, `SiegeEnded`, `FactionSplit`, `DisasterStruck`, `PlagueEnded`, `CulturalRebellion`, `BuildingConstructed`, `ItemTierPromoted`, `ItemCrafted`, `ReligionSchism`, `ReligionFounded`, `AllianceBetrayed`, `SuccessionCrisis`, `Custom("failed_coup")`
    - **Note:** Knowledge is the second-largest signal consumer. Most signals trigger knowledge creation for notable events (battles, conquests, disasters, religious schisms, etc.).

12. **KnowledgeDerivationSystem** → part of `KnowledgePlugin` — **Yearly**
    - Reads: manifestations, knowledge, persons
    - Emits: transcription, copying, distortion commands

13. **ItemSystem** → `ItemPlugin` — **Yearly**
    - Reads: items, persons, settlements
    - Emits: crafting, transfer, degradation commands (`ItemCrafted`, `ItemTierPromoted`, `ItemTransferred`)
    - Reacts to (4 signals): `EntityDied` (transfer possessions on death), `SettlementCaptured` (item transfers on conquest), `SiegeEnded` (resonance boost for surviving items), `BanditRaid` (steal notable items)

#### Wave 5: Political & Military Core

14. **PoliticsSystem** → `PoliticsPlugin` (decomposed into subsystems — see Phase 4) — **Yearly**
    - Reads: factions, persons, settlements
    - Emits: happiness/stability/legitimacy updates, succession, election, coup commands (`SuccessionCrisis`, `AllianceBetrayed`, `FactionSplit`, `MercenaryHired`, `MercenaryDeserted`, `MercenaryContractEnded`)
    - Reacts to (17 signals): `LeaderVacancy` (succession), `WarStarted` (happiness/stability), `WarEnded` (tribute/reparations), `SettlementCaptured` (stability penalty, grievances), `RefugeesArrived` (stability), `CulturalRebellion` (stability penalty), `PlagueStarted` (stability penalty), `SiegeStarted` (morale shift), `SiegeEnded` (morale recovery), `DisasterStruck` (stability penalty), `DisasterStarted` (stability penalty), `DisasterEnded` (recovery), `BanditGangFormed` (stability hit), `BanditRaid` (stability penalty), `TradeRouteRaided` (stability penalty), `AllianceBetrayed` (trust/stability penalty), `SecretRevealed` (prestige/stability effects)
    - **Note:** Politics is the third-largest signal consumer. See Phase 4 for decomposition including reactive handler subsystems.
    - Sub-modules: `coups.rs`, `diplomacy.rs` (shared helpers used by both Politics and Actions systems)

15. **ConflictSystem** → `ConflictPlugin` (decomposed — see Phase 4) — **Monthly**
    - Reads: factions, armies, settlements, regions
    - Emits: war declaration, battle, siege, retreat, conquest commands (`WarStarted`, `WarEnded`, `SettlementCaptured`, `SiegeStarted`, `SiegeEnded`, `LeaderVacancy`)
    - Reacts to: nothing (has **no** `handle_signals` method — only produces signals, never consumes them)
    - Sub-modules: `siege.rs`, `mercenaries.rs`

16. **MigrationSystem** → `MigrationPlugin` — **Yearly**
    - Reads: settlements, factions, regions
    - Emits: refugee movement, settlement abandonment commands (`RefugeesArrived`, `CulturalShift`)
    - Does NOT handle signals (reads state directly for war/conquest/poverty/plague effects)

#### Wave 6: Agent Systems

17. **AgencySystem** → `AgencyPlugin` — **Yearly**
    - Reads: persons (notable — those with traits), factions, settlements
    - Emits: action queuing (`PendingActions` resource)
    - Reacts to: `LeaderVacancy`, `WarStarted`, `WarEnded`, `SettlementCaptured`, `FactionSplit`, `AllianceBetrayed`, `SuccessionCrisis`
    - **Migration challenge:** Agency stores ALL incoming signals (not just specific types) into `self.recent_signals` in `handle_signals`. It then consumes them in the next `tick` to inform NPC decision-making. This cross-tick state requires a dedicated `AgencyMemory` resource (not `Local`, since we need testability). A dedicated `capture_agency_signals` system in the Reactions set should clone ALL `SimReactiveEvent` variants into the `AgencyMemory` resource. The resource stores reactive events from the previous tick's PostUpdate phase for use in the next tick's Update phase. **This is the only system with this capture-all-signals pattern.**

18. **ActionSystem** → `ActionPlugin` — **Yearly**
    - Reads: `PendingActions` resource, persons, factions, settlements
    - Emits: assassination, coup attempt, defection, betrayal commands; populates `ActionResults` resource
    - Processes queued actions from AgencySystem
    - **10 ActionKind variants:** `Assassinate`, `SupportFaction`, `UndermineFaction`, `BrokerAlliance`, `DeclareWar`, `AttemptCoup`, `Defect`, `SeekOffice`, `BetrayAlly`, `PressClaim`
    - **Action pipeline:** `Action { actor_id, source: ActionSource, kind: ActionKind }` → processing → `ActionResult { actor_id, source, outcome: ActionOutcome }`
    - `ActionSource` variants: `Player` (external input), `Autonomous` (NPC decision), `Order { ordered_by }` (delegation)
    - Uses shared helpers from `politics/diplomacy.rs`

### Systems Without `handle_signals`

Five systems have no `handle_signals` method and need **no reactive handlers** during migration:
- **Demographics** — reads state directly for mortality modifiers from war/disease/disaster
- **Education** — pure tick logic, no signal consumption
- **Migration** — reads state directly for push/pull factors
- **Actions** — processes `PendingActions` queue, not signals
- **Conflicts** — producer-only, emits signals but never consumes them

Additionally, **Environment** has a `handle_signals` method but it's a no-op — no reactive handlers needed.

### Per-System Migration Procedure

For each system:

1. **Read** the current `tick()` and `handle_signals()` implementations
2. **Identify** all world reads (what queries are needed) and all world writes (what commands to emit)
3. **Write** the Bevy system function(s) with appropriate query parameters
4. **Replace** direct `ctx.world` mutations with `SimCommand` events
5. **Replace** `ctx.signals.push(Signal { ... })` with direct inclusion in the command (the command applicator emits reactive events)
6. **Port** `handle_signals()` logic to a reaction handler system (skip for the 6 systems above that don't need reactive handlers)
7. **Write** focused unit tests using Bevy's test infrastructure
8. **Run** 1000-year integration test, compare event log output with the old system

### Cross-System Dependency Summary

Consolidated reference showing which entity types each system reads and writes, and which resources it needs. Use this during migration to determine Bevy system parameter signatures.

| System | Frequency | Entity Types Read | Entity Types Written (via commands) | Resources Read |
|--------|-----------|-------------------|-------------------------------------|----------------|
| **Environment** | Monthly | Settlement, Region | Settlement (seasonal_modifiers, active_disaster) | SimClock |
| **Buildings** | Yearly | Settlement, Building | Building (condition, level), Settlement (building_bonuses) | SimClock |
| **Education** | Yearly | Settlement, Person, Building | Settlement (literacy_rate), Person (education) | SimClock |
| **Demographics** | Yearly | Settlement, Person, Culture, Region | Settlement (population), Person (births/deaths/marriages) | SimClock, SimEntityMap |
| **Economy** | Monthly | Settlement, Faction, Army, Region, Building | Settlement (treasury, prosperity, trade), Faction (treasury) | SimClock, RelationshipGraph |
| **Disease** | Yearly | Settlement, Disease, Region | Settlement (active_disease, population), Person (death) | SimClock, RelationshipGraph (trade routes) |
| **Culture** | Yearly | Settlement, Culture, Faction | Settlement (culture_makeup, dominant_culture), Faction (split) | SimClock |
| **Religion** | Yearly | Settlement, Religion, Deity, Faction | Settlement (religion_makeup), Religion (fervor), Faction (conversion) | SimClock |
| **Reputation** | Yearly | Person, Faction, Settlement | Person (prestige), Faction (prestige), Settlement (prestige) | SimClock |
| **Crime** | Yearly | Settlement, Faction, Region | Settlement (crime_rate, bandit_threat), Faction (bandit gangs) | SimClock, RegionAdjacency |
| **Knowledge** | Yearly | Settlement, Knowledge, Manifestation, Person | Knowledge (new), Manifestation (new/decay/destroy) | SimClock, EventLog |
| **Items** | Yearly | Item, Person, Settlement | Item (crafting/transfer/degradation) | SimClock |
| **Politics** | Yearly | Faction, Person, Settlement | Faction (happiness, stability, legitimacy, etc.), Person (claims, prestige) | SimClock, RelationshipGraph |
| **Conflicts** | Monthly | Faction, Army, Settlement, Region, Person | Army (all fields), Settlement (siege, population), Faction (war), Person (death) | SimClock, RegionAdjacency, RelationshipGraph |
| **Migration** | Yearly | Settlement, Faction, Region | Settlement (population transfer), Person (relocation) | SimClock, RegionAdjacency |
| **Agency** | Yearly | Person (notable), Faction, Settlement | PendingActions resource | SimClock, AgencyMemory |
| **Actions** | Yearly | Person, Faction, Settlement | Various (assassination, coup, defection, etc.) | PendingActions, SimClock, RelationshipGraph |

**Note:** Under the command-event pattern, all systems in the Update phase have read-only access to entity components and write-only access to `EventWriter<SimCommand>`. The "Entity Types Written" column shows what the system's commands will eventually mutate (applied by the command applicator in PostUpdate).

### Component-Level Settlement Dependency Table

For Bevy query signatures and parallelism analysis, this table shows which specific settlement sub-components each system reads and writes:

| System | Settlement Components Read | Settlement Components Written (via commands) |
|--------|---------------------------|----------------------------------------------|
| **Environment** | SettlementCore (population), SeasonalModifiers | SeasonalModifiers, (ActiveDisaster via commands) |
| **Demographics** | SettlementCore (population, capacity, population_breakdown), SettlementTrade (is_coastal), BuildingBonuses, SeasonalModifiers | SettlementCore (population, population_breakdown, capacity) |
| **Buildings** | SettlementCore (x, y), SettlementTrade (is_coastal, resources) | BuildingBonuses |
| **Education** | SettlementEducation, BuildingBonuses, SettlementCulture (dominant_culture) | SettlementEducation |
| **Economy** | SettlementCore (population, prosperity, treasury, resources), SettlementTrade, BuildingBonuses, SeasonalModifiers | SettlementCore (treasury, prosperity), SettlementTrade |
| **Disease** | SettlementCore (population, population_breakdown), SettlementDisease, SettlementTrade (is_coastal, trade_routes), BuildingBonuses | SettlementCore (population), SettlementDisease, (ActiveDisease via commands) |
| **Culture** | SettlementCulture, SettlementCore (population, prosperity), SettlementTrade (trade_routes) | SettlementCulture |
| **Religion** | SettlementCulture (religion_makeup, dominant_religion, religious_tension), SettlementCore | SettlementCulture |
| **Crime** | SettlementCore (population, prosperity), SettlementCrime, SettlementTrade (is_coastal), SettlementMilitary | SettlementCrime |
| **Reputation** | SettlementCore (prestige, prestige_tier) | SettlementCore (prestige, prestige_tier) |
| **Conflicts** | SettlementCore (population), SettlementMilitary (fortification_level) | SettlementCore (population), (ActiveSiege via commands) |
| **Migration** | SettlementCore (population, prosperity), SettlementTrade (is_coastal) | SettlementCore (population) |
| **Knowledge** | SettlementCore (x, y), SettlementTrade (trade_routes, is_coastal), BuildingBonuses, SettlementEducation | — (creates Knowledge/Manifestation entities, not Settlement mutations) |

**Parallelism insight:** Since all writes go through commands (not direct mutation), systems that read different settlement components can run in parallel. For query declarations, systems still need to declare `&SettlementCore` or `&SettlementCulture` etc. to get the right data, but read-only access to overlapping components does not create scheduling conflicts.

### Deliverable
All 18 systems ported. Old `SimSystem` trait and `TickContext` can be deleted. Integration tests pass.

---

## 10. Helper Function Migration

**Goal:** Migrate the ~36 shared helper functions in `src/sim/helpers.rs` and `src/sim/politics/diplomacy.rs` to work with Bevy's query-based architecture.

### Category 1: Pure Classification (migrate trivially)

These are pure functions that don't touch the world. They move unchanged:

- `is_food_resource(resource) -> bool`
- `is_mining_resource(resource) -> bool`
- `MINING_RESOURCES` constant

### Category 2: Read-Only Queries (become free functions taking query/resource params)

These currently take `&World` and do lookups. They become free functions that accept Bevy query parameters or references to resources:

| Current | Bevy Equivalent | Notes |
|---------|----------------|-------|
| `adjacent_regions(world, region_id)` | `adjacent_regions(adj: &RegionAdjacency, region: Entity)` | Uses `RegionAdjacency` resource |
| `faction_leader(world, faction_id)` | `faction_leader(query: &Query<(Entity, &SimEntity), With<LeaderOf>>)` or lookup via Bevy relationship |
| `settlement_faction(world, settlement_id)` | `settlement_faction(query: &Query<&MemberOf>, entity)` | Uses Bevy Relationship |
| `faction_settlements(world, faction_id)` | `faction_settlements(query: &Query<Entity, (With<Settlement>, With<MemberOf>)>, faction)` | Filter by MemberOf target |
| `settlement_building_count(world, settlement_id)` | Query buildings with `LocatedIn(settlement)` |
| `entity_name(world, entity_id)` | `entity_name(query: &Query<&SimEntity>, entity)` |
| `faction_stability/happiness/legitimacy(world, faction_id)` | Direct component access: `faction_core.get(entity)?.stability` |
| `settlement_literacy(world, settlement_id)` | Direct component access |
| `settlement_has_port/is_coastal(world, settlement_id)` | Direct component access |
| `region_is_water(world, region_id)` | Direct component access |
| `region_has_port_settlement(world, region_id)` | Query settlements in region |
| `total_faction_population(world, faction_id)` | Query + sum |
| `faction_resource_set(world, faction_id)` | Query + collect |
| `collect_faction_region_ids(world, faction_id)` | Query + collect |
| `factions_are_adjacent(world, a, b)` | Uses `RegionAdjacency` + settlement queries |
| `is_non_state_faction/is_mercenary_faction(world, faction_id)` | Check `FactionCore.government_type` |
| `mercenary_employer(world, faction_id)` | Check `HiredBy` relationship |
| `employer_or_self(world, faction_id)` | Check `HiredBy` relationship |
| `faction_capital_oldest/largest(world, faction_id)` | Query + sort |
| `has_active_rel_of_kind(world, a, b, kind)` | Check `RelationshipGraph` resource |
| `active_rel_target(world, entity_id, kind)` | Check `RelationshipGraph` resource or Bevy Relationship query |
| `faction_leader_entity(world, faction_id)` | Query `(Entity, &SimEntity)` with `LeaderOf` filter |

### Category 3: BFS Pathfinding (become functions taking adjacency resource)

These are graph traversal functions used by multiple systems:

| Current | Bevy Equivalent |
|---------|----------------|
| `bfs_next_step(world, start, goal)` | `bfs_next_step(adj: &RegionAdjacency, start: Entity, goal: Entity)` |
| `bfs_nearest(world, start, predicate)` | `bfs_nearest(adj: &RegionAdjacency, start: Entity, pred: impl Fn(Entity) -> bool)` |
| `bfs_next_step_naval(world, start, goal, can_embark)` | `bfs_next_step_naval(adj: &RegionAdjacency, regions: &Query<&RegionData>, settlements: &Query<...>, ...)` |
| `bfs_nearest_naval(world, start, can_embark, predicate)` | Same pattern |

These are performance-critical (called per-army per-tick in Conflicts). Consider caching adjacency/port data in a resource for fast access.

### Category 4: Mutation Helpers (must emit SimCommands instead)

These currently mutate the world directly. Under the command-event pattern, they either:
- Become functions that **return** a `Vec<SimCommand>` instead of mutating
- Get inlined into the systems that call them

| Current | Migration Strategy |
|---------|-------------------|
| `end_all_person_relationships(world, person_id, time, event_id)` | Return `Vec<SimCommand>` of `EndRelationship` commands |
| `end_ally_relationship(world, a, b, time, event_id)` | Return `SimCommand::EndRelationship` for both directions |
| `apply_stability_delta(world, faction_id, delta, event_id)` | Return `SimCommand::SetField { entity, field: "stability", ... }` |
| `damage_buildings(world, signals, settlement_id, ...)` | Return `Vec<SimCommand>` of building damage/destruction |
| `transfer_settlement_npcs(world, settlement_id, old_faction, new_faction, ...)` | Return `Vec<SimCommand>` of relationship transfers |

### Category 5: Shared Logic Modules

`src/sim/politics/diplomacy.rs` contains shared helper functions used by both `PoliticsSystem` and `ActionSystem`. These stay as a shared utility module but their signatures change to take query params instead of `&mut World`.

**Diplomacy functions (3 pub(crate) + 1 pub(super) orchestrator):**

| Function | Visibility | Signature | Migration Notes |
|----------|-----------|-----------|----------------|
| `update_diplomacy` | `pub(super)` | `(ctx: &mut TickContext, time: SimTimestamp, current_year: u32)` | Orchestrates alliance formation, enemy formation, and trust drift. Calls the 3 helpers below. Maps to `DiplomacySystem` tick function in PoliticsPlugin decomposition (Section 11). |
| `calculate_alliance_strength` | `pub(crate)` | `(world: &World, faction_a: u64, faction_b: u64) -> f64` | Read-only; uses trade routes + shared enemies + marriage alliances + prestige. Takes query params for FactionDiplomacy, RelationshipGraph. |
| `get_diplomatic_trust` | `pub(crate)` | `(world: &World, faction_id: u64) -> f64` | Read-only; default 1.0. Direct component access on FactionDiplomacy.diplomatic_trust. |
| `compute_ally_vulnerability` | `pub(crate)` | `(world: &World, ally_id: u64) -> f64` | Read-only; 0.0–1.0 based on war/plague/stability/treasury. Takes query params for FactionCore, active disease/disaster state. |

### Category 6: Grievance Helpers (`src/sim/grievance.rs`)

A shared mutation module used by Politics, Conflicts, and Agency. **This module is completely separate from `helpers.rs`** and must not be overlooked during migration.

| Function | Signature | Category | Migration Strategy |
|----------|-----------|----------|-------------------|
| `get_grievance` | `(world: &World, holder: u64, target: u64) -> f64` | Read-only | Query `FactionDiplomacy.grievances` or `PersonSocial.grievances` directly |
| `add_grievance` | `(world: &mut World, holder: u64, target: u64, delta: f64, source: &str, time: SimTimestamp, event_id: u64)` | Mutation | Return `SimCommand::SetField` for grievance changes |
| `reduce_grievance` | `(world: &mut World, holder: u64, target: u64, delta: f64, threshold: f64)` | Mutation | Return `SimCommand::SetField` for grievance changes |
| `remove_grievance` | `(world: &mut World, holder: u64, target: u64)` | Mutation | Return `SimCommand::SetField` for grievance removal |
| `trait_decay_multiplier` | `(traits: &[Trait]) -> f64` | Pure | Moves unchanged (pure function) |

### Category 7: Loyalty Helpers (`src/sim/loyalty.rs`)

A `pub(crate)` shared mutation module that mirrors the `grievance.rs` pattern — works on both FactionData and PersonData via dual-dispatch. Used by the mercenary system and potentially other loyalty-dependent systems.

| Function | Signature | Category | Migration Strategy |
|----------|-----------|----------|-------------------|
| `get_loyalty` | `(world: &World, holder: u64, target: u64) -> f64` | Read-only (default 0.5) | Query `FactionDiplomacy.loyalty` or `PersonSocial.loyalty` directly |
| `set_loyalty` | `(world: &mut World, holder: u64, target: u64, value: f64)` | Mutation (clamped 0.0–1.0) | Return `SimCommand::SetField` for loyalty changes |
| `adjust_loyalty` | `(world: &mut World, holder: u64, target: u64, delta: f64)` | Mutation | Return `SimCommand::SetField` for loyalty changes |
| `loyalty_below` | `(world: &World, holder: u64, target: u64, threshold: f64) -> bool` | Read-only | Query `FactionDiplomacy.loyalty` or `PersonSocial.loyalty` directly |
| `remove_loyalty` | `(world: &mut World, holder: u64, target: u64)` | Mutation | Return `SimCommand::SetField` for loyalty removal |

### Migration Strategy

Migrate helpers alongside the systems that use them (not as a separate phase). When porting system N:
1. Identify which helpers it calls
2. Convert those helpers to Bevy-compatible signatures
3. If a helper is shared with a not-yet-ported system, provide both old and new versions temporarily

---

## 11. Phase 4: Plugin Decomposition

**Goal:** Break the large monolithic systems into focused Bevy plugins containing many small systems.

### PoliticsPlugin Decomposition

The current PoliticsSystem (~1500 LOC) handles happiness, stability, legitimacy, succession, elections, coups, diplomacy, and betrayal. Decompose into:

```
PoliticsPlugin
├── ── TICK SYSTEMS (yearly, in Update phase) ──
│
├── HappinessSystem        — yearly, calculates target happiness per faction
│   Reads: FactionCore, SettlementCore, RelationshipGraph (allies/enemies),
│          CultureTension, ReligiousTension, BuildingBonuses
│   Emits: ModifyFaction { happiness }
│
├── StabilitySystem        — yearly, calculates target stability per faction
│   Reads: FactionCore (happiness, legitimacy), LeaderOf relationships
│   Emits: ModifyFaction { stability }
│
├── LegitimacySystem       — yearly, calculates legitimacy drift
│   Reads: FactionCore (happiness), PersonReputation (leader prestige)
│   Emits: ModifyFaction { legitimacy }
│
├── ElectionSystem         — yearly, for elective governments
│   Reads: FactionCore (government_type), Persons in faction
│   Emits: election events, leader change commands
│
├── CoupSystem             — yearly, evaluates coup conditions
│   Reads: FactionCore (stability, legitimacy), Persons (ambitious trait)
│   Emits: coup attempt commands
│
├── DiplomacySystem        — yearly, manages alliances, trust, non-aggression
│   Reads: FactionCore, RelationshipGraph, FactionDiplomacy
│   Emits: alliance/enemy relationship commands
│
├── BetrayalSystem         — yearly, evaluates betrayal conditions
│   Reads: FactionCore, Persons (traits), RelationshipGraph (allies at war)
│   Emits: betrayal commands, trust changes
│
├── TerritorialAmbitionSystem — yearly, evaluates expansion targets
│   Reads: FactionCore, neighboring factions, army strength
│   Emits: DeclareWar commands
│
├── ── REACTIVE SYSTEMS (in Reactions set, PostUpdate phase) ──
│
├── SuccessionSystem       — reactive, fires on LeaderVacancy
│   Reads: Persons with MemberOf faction, Claims, PersonCore (traits)
│   Emits: AddRelationship(LeaderOf), succession events
│
├── FactionSplitSystem     — reactive, fires on extreme low stability/happiness
│   Reads: FactionCore, Settlements in faction
│   Emits: SpawnEntity(Faction), relationship transfers
│
└── PoliticsReactiveSystem — reactive, handles remaining 15 signal types
    Reads: SimReactiveEvent, FactionCore, FactionDiplomacy
    Emits: stability/happiness/trust adjustments
    Signal groupings:
      War/Siege: WarStarted, WarEnded, SettlementCaptured, SiegeStarted, SiegeEnded
      Disaster/Disease: PlagueStarted, DisasterStruck, DisasterStarted, DisasterEnded
      Crime: BanditGangFormed, BanditRaid, TradeRouteRaided
      Diplomacy: AllianceBetrayed, SecretRevealed
      Culture: RefugeesArrived, CulturalRebellion
```

**Implementation note:** The 17 reactive signal handlers for Politics mostly follow the same pattern: apply stability/happiness penalties or bonuses to the affected faction. A single `PoliticsReactiveSystem` with a `match` on `SimReactiveEvent` variants handles the bulk, while `SuccessionSystem` and `FactionSplitSystem` remain separate due to their complex logic.

**Parallelism gains:** `HappinessSystem`, `StabilitySystem`, and `LegitimacySystem` can all read `FactionCore` concurrently (read-only). They each emit commands that are applied after all three run. `CoupSystem` and `ElectionSystem` are independent. `DiplomacySystem` and `BetrayalSystem` are independent.

**Ordering constraints:**
- `HappinessSystem` → `StabilitySystem` → `LegitimacySystem` (each depends on the previous tick's output, but within a single tick they can use stale values — the drift-toward-target design already handles this)
- Actually, since these all emit commands rather than mutating directly, they can run in parallel reading the same snapshot. The drift calculations are already designed to converge over multiple ticks, not require same-tick ordering.

### ConflictPlugin Decomposition

The current ConflictSystem (~2000 LOC) handles war declaration, army mustering, marching, battle resolution, siege, retreat, conquest, and peace terms. Decompose into:

```
ConflictPlugin
├── WarDeclarationSystem   — yearly, evaluates casus belli and war readiness
│   Reads: FactionCore, FactionDiplomacy (grievances, war_goals), army strength
│   Emits: DeclareWar commands
│
├── MusterSystem           — monthly, raises armies when at war
│   Reads: Factions at war, Settlements (population for draft), Armies
│   Emits: SpawnEntity(Army), ModifySettlement (population loss from draft)
│
├── MarchSystem            — monthly, moves armies toward targets
│   Reads: Armies (position, target), Regions (adjacency, terrain)
│   Emits: ModifyArmy (position, supply, months_campaigning)
│
├── BattleSystem           — monthly, resolves combat when armies meet
│   Reads: Armies in same region, ArmyData (strength, morale), terrain
│   Emits: battle events, ModifyArmy (casualties, morale), EntityDied (persons)
│
├── SiegeSystem            — monthly, manages ongoing sieges
│   Reads: Armies besieging settlements, SettlementMilitary (fortification)
│   Emits: siege progress, assault, siege outcome commands
│
├── AttritionSystem        — monthly, applies supply and disease attrition
│   Reads: Armies (supply, months), Regions (terrain, allegiance), seasons
│   Emits: ModifyArmy (strength loss, morale loss)
│
├── RetreatSystem          — monthly, evaluates retreat conditions
│   Reads: Armies (morale threshold), battles lost
│   Emits: ModifyArmy (retreat), end siege commands
│
├── ConquestSystem         — reactive, fires on siege victory
│   Reads: Settlements, Factions
│   Emits: SettlementCaptured, relationship transfers
│
├── PeaceSystem            — yearly, evaluates war exhaustion and peace terms
│   Reads: Factions at war, war duration, battle outcomes
│   Emits: WarEnded, tribute/reparation commands
│
└── MercenarySystem        — monthly, manages mercenary hiring and loyalty
    Reads: Mercenary factions, employer factions, armies
    Emits: hiring, desertion, contract commands
```

**Parallelism gains:** `MarchSystem`, `AttritionSystem`, and `SiegeSystem` are all monthly systems that operate on different aspects of army/settlement state. With the command pattern, they can run in parallel.

### EconomyPlugin Decomposition

```
EconomyPlugin
├── ProductionSystem       — monthly, calculates per-settlement production
│   Reads: Settlements, Regions (resources), Buildings, SeasonalModifiers
│   Emits: ModifySettlement (production values)
│
├── TaxationSystem         — yearly, collects taxes into faction treasury
│   Reads: Settlements (population, prosperity), Factions
│   Emits: ModifyFaction (treasury), ModifySettlement (treasury)
│
├── TradeSystem            — monthly, evaluates trade route income
│   Reads: Settlements (trade routes), Buildings (market/port bonuses)
│   Emits: ModifySettlement (trade income, prosperity)
│
├── TreasurySystem         — monthly, pays army maintenance and upkeep
│   Reads: Factions (treasury), Armies (strength = maintenance cost)
│   Emits: ModifyFaction (treasury), TreasuryDepleted events
│
├── FortificationSystem    — yearly, manages defense structure construction
│   Reads: Settlements, Factions (treasury), Buildings
│   Emits: construction/upgrade commands
│
└── ProsperitySystem       — yearly, calculates settlement prosperity
    Reads: SettlementCore (population), FactionCore (stability), trade, production
    Emits: ModifySettlement (prosperity)
```

### KnowledgePlugin Decomposition

```
KnowledgePlugin
├── ── TICK SYSTEMS (yearly, in Update phase) ──
│
├── PropagationSystem        — yearly, spreads knowledge along trade routes
│   Reads: Manifestations, Settlements (trade routes)
│   Emits: SpawnEntity(Manifestation) at new settlements
│
├── DerivationSystem         — monthly, copies/transcribes/distorts
│   Reads: Manifestations, Persons (scholars)
│   Emits: SpawnEntity(Manifestation) with accuracy drift
│
├── DecaySystem              — yearly, degrades manifestation quality
│   Reads: Manifestations (medium, age)
│   Emits: ModifyManifestation (completeness), EndEntity (if fully decayed)
│
├── SecretSystem             — yearly, manages knowledge suppression/revelation
│   Reads: Knowledge (suppressed), Persons (secret-keepers)
│   Emits: SecretRevealed events
│
├── ── REACTIVE SYSTEMS (in Reactions set, PostUpdate phase) ──
│
├── KnowledgeCreationSystem  — reactive, creates knowledge from 18 signal types
│   Reads: SimReactiveEvent, Settlements, Persons
│   Emits: SpawnEntity(Knowledge), SpawnEntity(Manifestation)
│   Signals: WarEnded, SettlementCaptured, SiegeEnded, FactionSplit,
│            DisasterStruck, PlagueEnded, CulturalRebellion, BuildingConstructed,
│            ItemTierPromoted, ItemCrafted, ReligionSchism, ReligionFounded,
│            AllianceBetrayed, SuccessionCrisis, Custom("failed_coup"),
│            EntityDied (manifestation holder death)
│
└── KnowledgeSelfPropagationSystem — reactive, handles KnowledgeCreated + ManifestationCreated
    Reads: SimReactiveEvent (self-referential), Knowledge, Manifestations
    Emits: additional propagation commands
    Note: runs in same Reactions pass as KnowledgeCreationSystem;
          can read events from apply_sim_commands but not cascade further
```

### ReputationPlugin Decomposition

Reputation is the largest signal consumer (23 signal types). The current ReputationSystem handles prestige decay, prestige bonuses/penalties from events, and tier threshold crossing. Decompose into:

```
ReputationPlugin
├── PrestigeDecaySystem       — yearly, applies passive prestige decay toward baseline
│   Reads: PersonReputation, FactionCore, SettlementCore
│   Emits: SetField (prestige adjustments) — bookkeeping, not event-worthy
│
├── PrestigeTierSystem        — yearly, checks threshold crossings
│   Reads: PersonReputation, FactionCore, SettlementCore
│   Emits: PrestigeThresholdCrossed events, tier updates
│
├── ReputationReactiveSystem  — reactive, handles all 23 signal types
│   Reads: SimReactiveEvent, PersonReputation, FactionCore, SettlementCore
│   Emits: prestige adjustments per signal type
│   Signal groupings:
│     War/Conflict: WarEnded, SettlementCaptured, SiegeEnded, AllianceBetrayed,
│                   SuccessionCrisis, EntityDied (leader death penalty)
│     Economy/Crime: TradeRouteEstablished, TreasuryDepleted, BanditGangFormed,
│                    BanditRaid, BuildingConstructed, BuildingUpgraded
│     Culture/Religion: FactionSplit, CulturalRebellion, ReligionSchism,
│                       ProphecyDeclared, ReligionFounded, SecretRevealed
│     Disease/Disaster: PlagueEnded, DisasterStruck, DisasterEnded
│     Knowledge/Items: KnowledgeCreated, ItemTierPromoted
│
└── (All reactive handlers can be a single system that matches on
     SimReactiveEvent variants, since they all do the same thing:
     adjust prestige values on involved entities)
```

**Implementation note:** Since all 23 signal handlers follow the same pattern (look up involved entities, apply prestige delta), a single reactive system with a `match` on `SimReactiveEvent` variants is cleaner than 23 separate handler systems. The prestige deltas are simple arithmetic, not complex logic.

### Other Plugin Decompositions

**DemographicsPlugin:**
- `AgingSystem` — yearly, age bracket transitions and natural death
- `BirthSystem` — yearly, new births based on population and prosperity
- `MarriageSystem` — yearly, notable person marriages (including cross-faction)
- `NotableGenerationSystem` — yearly, generates named NPCs for settlements

**CulturePlugin:**
- `AssimilationSystem` — yearly, gradual culture drift in multi-cultural settlements
- `CulturalShiftSystem` — yearly, detects dominant culture changes
- `BlendingSystem` — yearly, creates blended cultures after long coexistence
- `CulturalRebellionSystem` — yearly, fires when minority culture is oppressed

**ReligionPlugin:**
- `ConversionSystem` — yearly, gradual religious conversion
- `SchismSystem` — yearly, detects conditions for religious splits
- `ProphecySystem` — yearly, generates prophecies
- `FervorSystem` — yearly, adjusts fervor based on events

### Deliverable
Each current system is decomposed into a plugin with 3-10 small, focused systems. System sets within each plugin define internal ordering. Cross-plugin ordering uses broader set constraints.

---

## 12. Phase 5: Parallelism & Scheduling

**Goal:** Configure system ordering to maximize parallel execution while maintaining correctness.

### System Set Hierarchy

```
SimTick schedule:
│
├── SimPhase::PreUpdate
│   └── advance_clock
│
├── SimPhase::Update
│   ├── EnvironmentSet          ── (no ordering constraints, runs first conceptually)
│   ├── BuildingSet             ── after EnvironmentSet
│   │
│   ├── ── PARALLEL GROUP 1 ──
│   │   ├── DemographicsSet     ── after EnvironmentSet
│   │   ├── EconomySet          ── after BuildingSet
│   │   ├── DiseaseSet          ── after EnvironmentSet
│   │   └── EducationSet        ── after BuildingSet
│   │
│   ├── ── PARALLEL GROUP 2 ──
│   │   ├── CultureSet          ── (independent)
│   │   ├── ReligionSet         ── (independent)
│   │   ├── CrimeSet            ── (independent)
│   │   └── ReputationSet       ── (independent)
│   │
│   ├── ── PARALLEL GROUP 3 ──
│   │   ├── KnowledgeSet        ── (independent)
│   │   ├── ItemSet             ── (independent)
│   │   └── MigrationSet        ── (independent)
│   │
│   ├── ── PARALLEL GROUP 4 ──
│   │   ├── PoliticsSet         ── (independent, reads snapshot)
│   │   └── ConflictSet         ── (independent, reads snapshot)
│   │
│   └── AgencySet               ── after all above (reads full state to make decisions)
│       └── ActionSet            ── after AgencySet
│
├── SimPhase::PostUpdate
│   ├── ApplyCommandsSet
│   │   └── apply_sim_commands  ── (exclusive: &mut World access)
│   ├── apply_deferred          ── sync point
│   └── ReactionsSet
│       ├── handle_leader_vacancy
│       ├── handle_war_outcomes
│       ├── handle_settlement_capture
│       └── ...
│
└── SimPhase::Last
    └── flush_checkpoint (conditional)
```

### Why This Works

With the command-event pattern, **all Update systems only need read access** to entity components. They write to `EventWriter<SimCommand>`, which is per-system (each system gets its own writer, no conflicts). This means:

- Groups 1-4 can all run in parallel with each other
- Within each group, all systems can run in parallel
- The only serialization point is `ApplyCommands` in `PostUpdate`

The key insight: because systems emit commands rather than mutating state, **every Update system reads the same consistent snapshot** of the world. This is equivalent to the current Phase 1 behavior where all systems see the state as of the start of the tick.

### Ordering Constraints That Must Be Preserved

1. `advance_clock` must run before all Update systems
2. All Update systems must complete before `apply_sim_commands`
3. `apply_sim_commands` must complete before reaction handlers
4. Reaction handlers may need internal ordering (leader vacancy before succession crisis)
5. Flush must run after all mutations are applied

### Handling Tick Frequency

Systems within a set use run conditions for frequency gating:

```rust
app.add_systems(
    SimTick,
    (
        calculate_production.run_if(monthly),
        evaluate_trade_routes.run_if(monthly),
        collect_taxes.run_if(yearly),
        calculate_prosperity.run_if(yearly),
    )
        .in_set(EconomySet)
);
```

### Deliverable
Schedule configuration with explicit ordering. Benchmark showing improvement over sequential execution. 1000-year integration test still produces valid output.

---

## 13. Phase 6: Flush & Serialization

**Goal:** Port the JSONL flush mechanism to work with Bevy's world.

### Changes

The current flush extracts data from `BTreeMap<u64, Entity>` and `BTreeMap<u64, Event>`. The new flush must query Bevy's world.

**Flush system:**
```rust
fn flush_to_jsonl(
    query: Query<(&SimEntity, &EntityKindMarker, /* all data components */)>,
    event_log: Res<EventLog>,
    rel_graph: Res<RelationshipGraph>,
    clock: Res<SimClock>,
    // ...
) {
    // 1. Query all entities, serialize to entities.jsonl
    // 2. Extract relationships from RelationshipGraph, serialize to relationships.jsonl
    // 3. Serialize EventLog to events.jsonl, event_participants.jsonl, event_effects.jsonl
}
```

**Key considerations:**
- Entity IDs: The `SimEntity.id` field (u64) is the stable ID written to JSONL, not the Bevy `Entity` (which is an internal handle). The `SimEntityMap` resource provides the mapping.
- The `EventLog` resource accumulates events throughout the simulation. At flush time, write the accumulated events and clear the buffer.
- Relationships come from two sources that must be merged:
  - **Structural** (Bevy Relationships): query `LocatedIn`, `MemberOf`, `LeaderOf`, `HeldBy`, `HiredBy`, `FlowsThrough`, `Exploits` — convert to `Relationship` structs with sim-IDs
  - **Graph** (`RelationshipGraph` resource): `allies`, `enemies`, `at_war`, `parent_child`, `spouses`, `trade_routes` — convert to `Relationship` structs with sim-IDs
  - **Adjacency** (`RegionAdjacency` resource): `AdjacentTo` relationships — convert similarly

**JSONL output format** (5 files, unchanged from current):

| File | Source | Notes |
|------|--------|-------|
| `entities.jsonl` | Query all entities with `SimEntity` component | Must reconstruct `EntityData` enum from decomposed components for serialization compatibility |
| `relationships.jsonl` | Merge structural relationships + `RelationshipGraph` + `RegionAdjacency` | Each becomes `Relationship { source_entity_id, target_entity_id, kind, start, end }` using sim-IDs |
| `events.jsonl` | `EventLog.events` | Direct serialization |
| `event_participants.jsonl` | `EventLog.participants` | Direct serialization |
| `event_effects.jsonl` | `EventLog.effects` | Direct serialization |

**Entity serialization challenge:** The current flush serializes `Entity` structs which contain an `EntityData` enum. In Bevy, the data is split across multiple components. The flush system must reassemble the `EntityData` enum from components for each entity kind. Consider adding a `fn to_entity_data()` method on each component bundle, or serializing the decomposed components directly (which would require changing the Postgres schema).

### Postgres Loader

No changes needed to the Postgres loader or SQL schemas if we preserve the current JSONL format. The loader consumes the same 5 files. If the entity serialization format changes (e.g., flat components instead of nested `EntityData`), the loader and schema must be updated to match.

### Flush Timing

Flush is triggered by `SimConfig.flush_interval` (every N years) or at the end of the simulation run. In Bevy, this becomes a system in `SimPhase::Last` with a run condition:

```rust
fn should_flush(clock: Res<SimClock>, config: Res<SimConfig>) -> bool {
    if let Some(interval) = config.flush_interval {
        clock.time.is_year_start() && clock.time.year() % interval == 0
    } else {
        false
    }
}
```

### Deliverable
Flush produces identical JSONL output to the current system. Postgres round-trip tests pass.

---

## 14. Phase 7: Testing Strategy

### Unit Tests

Each decomposed system gets focused unit tests:

```rust
#[test]
fn happiness_increases_with_prosperity() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.init_resource::<SimClock>();

    let faction = app.world_mut().spawn((
        SimEntity { id: 1, name: "Test".into(), .. },
        Faction,
        FactionCore { happiness: 0.5, stability: 0.5, legitimacy: 0.5, .. },
    )).id();

    let settlement = app.world_mut().spawn((
        SimEntity { id: 2, name: "Town".into(), .. },
        Settlement,
        SettlementCore { prosperity: 0.9, population: 1000, .. },
        MemberOf(faction),
    )).id();

    app.add_systems(Update, calculate_happiness);
    app.update();

    // Check that a ModifyFaction command was emitted with higher happiness
}
```

### Integration Tests

1. **1000-year smoke test:** Run full simulation, verify no panics, entity counts within expected ranges
2. **Event log validation:** Every event has valid timestamps, caused_by chains don't cycle, all participant entities exist
3. **Relationship consistency:** No dangling references, no self-relationships, all ended relationships have valid timestamps
4. **Command coverage:** Every `SimCommand` variant is emitted at least once in a 1000-year run
5. **Determinism test:** Run same seed twice with `SimpleExecutor`, verify identical event logs (if determinism is enabled)

### Regression Tests

Before deleting the old system code, run both old and new systems on the same seed and compare:
- Total entity counts by kind
- Total event counts by kind
- Population curves (aggregate, not per-settlement — stochastic variation is expected)
- War frequency and duration distributions

These are statistical comparisons, not exact matches, since the command-event pattern changes the order of operations.

### Worldgen Pipeline Migration

The current worldgen pipeline has 11 composable steps with signature `fn(&mut World, &WorldGenConfig, &mut dyn RngCore, u64)`:

1. `geography::generate_regions`
2. `rivers::generate_rivers`
3. `features::generate_features`
4. `deposits::generate_deposits`
5. `settlements::generate_settlements`
6. `buildings::generate_buildings`
7. `factions::generate_factions`
8. `items::generate_items`
9. `cultures::generate_cultures`
10. `religions::generate_religions`
11. `knowledge::generate_knowledge`

**Migration strategy:** Worldgen runs as initialization code **before** the tick loop starts, not as part of the `SimTick` schedule. Each step becomes a function that takes `&mut bevy_ecs::World` (or `Commands`) and spawns entities directly:

```rust
pub type WorldGenStep = fn(&mut bevy_ecs::World, &WorldGenConfig, &mut dyn RngCore, u64);
```

These run sequentially during app setup, using Bevy's direct world access (not the scheduler). After all worldgen steps complete, the `SimEntityMap` and `RegionAdjacency` resources are fully populated and the tick loop can begin.

**WorldGenConfig** — currently a standalone struct, should become a Bevy `Resource` (or fields merged into `SimConfig`):

```rust
pub struct WorldGenConfig {
    pub seed: u64,
    pub map: MapConfig,
    pub terrain: TerrainConfig,
    pub rivers: RiverConfig,
}

pub struct MapConfig {
    pub num_regions: u32,        // default: 25
    pub width: f64,              // default: 1000.0
    pub height: f64,             // default: 1000.0
    pub num_biome_centers: u32,  // default: 6
    pub adjacency_k: u32,       // default: 4
}

pub struct TerrainConfig {
    pub water_fraction: f64,     // default: 0.2
}

pub struct RiverConfig {
    pub num_rivers: u32,         // default: 4
}
```

These defaults matter for test setup — named test scenarios rely on them.

### Scenario Builder Migration

The `Scenario` builder (`src/scenario.rs`) is critical for test setup. It currently creates entities via `World::add_entity()` and relationships via `World::add_relationship()`.

**Migration strategy:** Convert `Scenario` to work with `App::world_mut()`. The Scenario has two terminal methods, both needing Bevy equivalents:

| Method | Current Signature | Bevy Equivalent | Usage |
|--------|-------------------|-----------------|-------|
| `build(self)` | `-> World` | `-> App` | Creates world without running systems. Used by many unit tests that set up state and call individual system functions. |
| `run(self, systems, num_years, seed)` | `-> World` | `-> App` | Creates world + runs tick loop. Used by integration tests. |

```rust
impl Scenario {
    pub fn build(self) -> App {
        let mut app = App::new();
        // Add all plugins
        app.add_plugins(SimPlugin);
        // Spawn entities directly into Bevy world
        for entity_spec in self.entities {
            let bevy_entity = app.world_mut().spawn(entity_spec.into_bundle()).id();
            app.world_mut().resource_mut::<SimEntityMap>().insert(entity_spec.id, bevy_entity);
        }
        // Add relationships
        // ...
        app
    }

    pub fn run(self, num_years: u32, seed: u64) -> App {
        let mut app = self.build();
        // Configure seed, run tick loop
        // ...
        app
    }
}
```

Preserve the composable API. The `build()` method sets up the Bevy app with all plugins and spawned entities but doesn't run the `SimTick` schedule. The `run()` method calls `build()` then runs the tick loop. **Without a `build()` equivalent, unit tests that set up state and call individual systems cannot be ported.**

**The Scenario API has ~100+ public methods** that all need migration. The major categories:

**1. Builder-Style Ref Types (16 types, all `#[must_use]`):**
`FactionRef`, `SettlementRef`, `PersonRef`, `ArmyRef`, `BuildingRef`, `RegionRef`, `CultureRef`, `DiseaseRef`, `KnowledgeRef`, `GeographicFeatureRef`, `RiverRef`, `ResourceDepositRef`, `ManifestationRef`, `ItemRef`, `ReligionRef`, `DeityRef`

Each has `.with(closure)` for chaining and `.id()` to terminate. These need conversion from `&mut World` mutation to `App::world_mut()` spawn/insert patterns.

**2. Composite Builders (create multiple entities at once):**
- `add_kingdom(name)` / `_with()` → `KingdomIds { faction, region, settlement, leader }`
- `add_rival_kingdom(name, neighbor)` / `_with()` → `KingdomIds` adjacent to existing
- `add_settlement_standalone(name)` / `_with()` → `SettlementSetup { settlement, faction, region }`
- `add_war_between(attacker, defender, strength)` → `WarIds { attacker: KingdomIds, defender: KingdomIds, army }`
- `add_mercenary_company(name, region, strength)` / `_with()` → `MercenarySetup { faction, army, leader }`

**3. Complex State Helpers:**
`start_siege()`, `add_active_disaster()`, `add_active_disease_on()`, `add_tribute()`, `set_diplomatic_trust()`, `set_betrayal_count()`, `add_claim()`, `add_grievance()`, `add_secret()`, `queue_action()`, `hire_mercenary()`

**4. Network Topology Helpers:**
`connect_ring(regions)`, `connect_hub_and_spoke(hub, spokes)`, `connect_all(regions)`, `connect_trade_ring(settlements)`, `connect_trade_hub(hub, spokes)`

**5. Bulk Operations:**
`add_population()`, `add_people()`, `modify_all_settlements()`, `modify_all_factions()`, `spread_disease()`, `spread_disaster()`

**Migration challenge:** The Scenario currently mutates `World` directly via entity creation. In Bevy, it should either:
- Use `App::world_mut().spawn(bundle)` for direct spawning (simpler, maintains current pattern)
- Use `Commands` for deferred spawning (more Bevy-idiomatic but requires `apply_deferred`)

The ref types' mutation pattern (`scenario.faction_mut(id).treasury(1000.0).stability(0.8).id()`) translates to component insertion/mutation on the Bevy world.

### Test Utility Migration

`src/testutil.rs` helpers need Bevy equivalents:

The actual `src/testutil.rs` has **~35 public functions**. All need Bevy equivalents:

**Tick execution:**

| Current | Bevy Equivalent |
|---------|----------------|
| `tick_system(world, system, num_years, seed)` | `tick_system(app: &mut App, num_years, seed)` — run `SimTick` schedule N times |
| `tick_system_at(world, system, time, seed)` | `tick_system_at(app: &mut App, time: SimTimestamp, seed)` — run at specific timestamp |
| `full_tick(world, system, year, seed)` | `full_tick(app: &mut App, year, seed)` — run tick + reactions cycle |
| `deliver_signals(world, system, signals)` | Insert signals as Bevy events, run reaction schedule |
| `run_years(world, systems, num_years, seed)` | `run_years(app: &mut App, num_years, seed)` — run multiple years with standard loop |
| `generate_and_run(seed, num_years, systems)` | `generate_and_run(seed, num_years)` — worldgen + run (full pipeline) |

**System constructors:**

| Current | Bevy Equivalent |
|---------|----------------|
| `core_systems() / all_systems()` | `CorePlugin` / `AllPlugins` — Bevy plugin groups |
| `combat_systems()` | `CombatPluginGroup` — Bevy plugin group |

**Query helpers:**

| Current | Bevy Equivalent |
|---------|----------------|
| `is_alive(world, id)` | `is_alive(app: &App, sim_id: u64)` — query `SimEntity` via `SimEntityMap` |
| `living_entities(world, kind)` | Query `SimEntity` with `end.is_none()` filter |
| `count_living(world, kind)` | Count query results |
| `has_relationship(world, a, b, kind)` | Check `RelationshipGraph` resource or Bevy relationships |
| `relationship_targets(world, entity, kind)` | Query `RelationshipGraph` or Bevy relationship targets |
| `people_in_settlement(world, settlement)` | Query `MemberOf`/`LocatedIn` relationships |
| `armies_in_region(world, region)` | Query `LocatedIn` relationships |
| `related_living(world, target, rel_kind, entity_kind)` | Filtered relationship query |
| `faction_leader(world, faction)` | Query `LeaderOf` relationship |
| `faction_settlements(world, faction)` | Query `MemberOf` relationship |
| `settlement_owner(world, settlement)` | Query `MemberOf` relationship |
| `extra_f64/extra_bool/extra_str/has_extra` | Check `IsPlayer` marker component (the only current usage) |

**Event/signal helpers:**

| Current | Bevy Equivalent |
|---------|----------------|
| `count_events(world, kind)` | Query `EventLog` resource |
| `events_of_kind(world, kind)` | Filter `EventLog.events` |
| `events_involving(world, entity_id)` | Filter `EventLog.participants` |
| `events_with_role(world, entity_id, role)` | Filter `EventLog.participants` |
| `count_signals(signals, predicate)` | Count matching `SimReactiveEvent`s |
| `has_signal(signals, kind)` | Check for specific `SimReactiveEvent` variant |
| `property_changes(world, entity_id, field)` | Filter `EventLog.effects` for `PropertyChanged` |
| `assert_property_changed(world, entity_id, field)` | Assert `PropertyChanged` exists in `EventLog.effects` |

**Assertion helpers:**

| Current | Bevy Equivalent |
|---------|----------------|
| `assert_deterministic()` | Run same scenario twice with `SimpleExecutor`, compare `EventLog` |
| `assert_alive(world, id)` | Assert `SimEntity.end.is_none()` |
| `assert_dead(world, id)` | Assert `SimEntity.end.is_some()` |
| `assert_related(world, a, b, kind)` | Assert relationship exists |
| `assert_event_exists(world, kind)` | Assert event in `EventLog` |
| `assert_event_with_participant(world, kind, entity_id, role)` | Assert event with specific participant |
| `assert_approx(actual, expected, epsilon)` | Floating-point comparison |

**Named scenarios (return structs with named fields — need Bevy equivalents returning `App`):**

| Current | Return Type | Description |
|---------|-------------|-------------|
| `war_scenario()` | `WarSetup` | Two factions at war with armies |
| `migration_scenario()` | `MigrationSetup` | Multiple settlements for migration testing |
| `economic_scenario(population, treasury)` | `EconomicSetup` | Single faction/settlement for economy |
| `political_scenario()` | `PoliticalSetup` | Faction + leader + settlement |
| `action_scenario()` | `ActionSetup` | Minimal world with player character |
| `mercenary_scenario()` | `MercenarySetup` | Two factions at war + hired mercenary |

Each named scenario returns a struct with Bevy `Entity` handles (not sim-IDs) for the pre-spawned entities, plus the `App` instance.

### Postgres Loader (`src/db/`) Migration

The `src/db/` module handles loading JSONL output into Postgres. Migration scope decision:

- **The db loader continues to consume JSONL output** — no changes needed as long as the flush system (Phase 6) produces the same 5-file JSONL format. The loader is downstream of the simulation and does not interact with the Bevy world.
- If the JSONL format changes (e.g., flat components instead of nested `EntityData`), the loader and Postgres schema must be updated to match.
- The db module is **deferred/out-of-scope for the core migration** — it can be updated after Phase 6 (Flush) is complete and the JSONL format is finalized.

### Procedural Generation (`src/procgen/`) Migration

Section 17 covers `generate_inhabitant()` but not the rest of the `src/procgen/` module (6 files). Public types that may need migration:

| Type | File | Current Usage | Migration Impact |
|------|------|---------------|-----------------|
| `GeneratedPerson` | inhabitants.rs | Pure function (no World access) | **No changes needed** — unaffected by Bevy migration |
| `GeneratedArtifact` | artifacts.rs | Reads from World | Needs query-based signatures |
| `GeneratedWriting` | writings.rs | Reads from World | Needs query-based signatures |
| `ProcGenConfig` | mod.rs | Configuration | Becomes a Bevy `Resource` or stays as-is |
| `SettlementDetails` | mod.rs | Reads from World | Needs query-based signatures |
| `SettlementSnapshot` | mod.rs | Reads from World | Needs query-based signatures |

Types that read from `World` need their function signatures updated to take Bevy query parameters instead of `&World`. `GeneratedPerson` (the inhabitant generator) is pure and unaffected, as noted in Section 17. Migrate these alongside Phase 8 (Cleanup) or when the consuming code is ported.

### Deliverable
Full test suite covering unit tests for each decomposed system, integration tests for the full pipeline, and regression comparisons.

---

## 15. Phase 8: Cleanup & Final Integration

### Delete Old Code

1. Remove `SimSystem` trait and all implementations (`sim/system.rs`)
2. Remove `TickContext` struct (`sim/context.rs`)
3. Remove `dispatch_systems()` and `run()` from `sim/runner.rs`
4. Remove `Signal` and `SignalKind` (replaced by `SimReactiveEvent`)
5. Remove the monolithic `World` struct (`model/world.rs` — Bevy's `World` replaces it)
6. Remove `sim/helpers.rs` (replaced by Bevy-compatible query-based helpers)
7. Remove `EntityData` enum dispatch and `entity_data_accessors!` macro (replaced by direct component access)
8. Remove `world_data_accessors!` macro (no longer needed — systems access components via queries)
9. Remove `sim/runner.rs` entirely (replaced by Bevy schedule runner)

### Migrate Unchanged Utility Modules

The following pure data/utility modules have no World access and migrate trivially (move unchanged):

- `pub mod names` — general name generation utilities
- `pub mod faction_names` — faction name tables
- `pub mod culture_names` — culture name tables
- `pub mod religion_names` — religion name tables

These modules contain name generation data and functions used by worldgen and demographics. They have no system registration, no `&World` references, and no migration work beyond verifying they compile in the new crate structure.

### Update Entry Points

1. `main.rs` / lib entry point: construct Bevy `App`, add all plugins, run tick loop
2. Worldgen: spawns entities into Bevy world via direct `World` access
3. Scenario builder: creates test scenarios using `App::world_mut()` spawn commands
4. Procgen: reads from Bevy world via queries
5. Flush: system in `SimPhase::Last` queries Bevy world, writes JSONL

### Update CLAUDE.md

Update architecture documentation to reflect the new Bevy-based design.

### Deliverable
Clean codebase with no dead code. All tests pass. Documentation updated.

---

## 16. Determinism Strategy

### Requirements

- **Procedural generation must be deterministic:** Given a settlement ID + person index, the same non-notable NPC is always generated. This is seed-based and doesn't require ECS determinism.
- **Event log does NOT require determinism:** The order of events within a tick can vary across runs. Different runs with the same seed may produce different histories — this is acceptable.
- **Testing benefits from optional determinism:** For debugging, it's useful to reproduce exact runs.

### Implementation

**Default mode: non-deterministic scheduling.** Use `MultiThreaded` executor for maximum performance. Accept that event order within a tick may vary.

**Debug mode: deterministic scheduling.** Use `SimpleExecutor` (runs systems in insertion order, applies commands immediately). Enable via feature flag or config option:

```rust
if config.deterministic {
    schedule.set_executor_kind(ExecutorKind::Simple);
}
```

**RNG strategy:** Use a master `SimRng` resource seeded from config. For parallel systems, derive per-system RNGs at the start of each tick into per-system resources. This ensures each system gets a deterministic RNG stream regardless of execution order.

**RNG distribution to systems:** A `distribute_rng` system runs in `SimPhase::PreUpdate` (after `advance_clock`) and derives per-system `SmallRng` instances from the master seed + system name + tick count. Each derived RNG is stored in a dedicated resource:

```rust
#[derive(Resource)]
pub struct SystemRng<S: SystemLabel>(pub SmallRng);

// In PreUpdate:
fn distribute_rng(
    master: Res<SimRng>,
    clock: Res<SimClock>,
    mut demographics_rng: ResMut<SystemRng<DemographicsSystem>>,
    mut economy_rng: ResMut<SystemRng<EconomySystem>>,
    // ... one per system that needs randomness
) {
    demographics_rng.0 = derive_system_rng(&master, "demographics", clock.tick_count);
    economy_rng.0 = derive_system_rng(&master, "economy", clock.tick_count);
    // ...
}

fn derive_system_rng(master: &SimRng, system_name: &str, tick: u64) -> SmallRng {
    let mut hasher = DefaultHasher::new();
    master.seed().hash(&mut hasher);
    system_name.hash(&mut hasher);
    tick.hash(&mut hasher);
    SmallRng::seed_from_u64(hasher.finish())
}
```

This approach allows full parallelism (each system has exclusive access to its own `SystemRng<S>`) while maintaining deterministic RNG streams. An alternative is using `Local<SmallRng>` per system, but dedicated resources are more testable and inspectable.

**Query iteration order:** Not enforced globally. Systems that need deterministic iteration should sort by `SimEntity.id` locally. Since we're relaxing determinism for the event log, most systems don't need this.

**Collection types:** All simulation resources must use `BTreeMap`/`BTreeSet`, never `HashMap`/`HashSet`. This matches the existing codebase convention (see CLAUDE.md) and ensures deterministic iteration in `SimpleExecutor` mode. `Entity.extra` (the only HashMap in the current model) is only accessed via `.get(key)`, never iterated in sim code.

---

## 17. Procedural Generation & Non-Notable NPCs

### Current Approach

Notable persons are stored as full entities in the world. Non-notable persons are represented only as population counts in `PopulationBreakdown` brackets.

### Bevy Approach

This doesn't change with Bevy. Non-notable NPCs are never spawned as entities. They remain implicit in population counts.

**Procedural generation of non-notable details** uses a deterministic function:

```rust
fn generate_inhabitant(settlement_id: u64, index: u8, seed: u64) -> InhabitantDetails {
    let rng = SmallRng::seed_from_u64(hash(seed, settlement_id, index));
    // Generate name, appearance, occupation, etc.
    // This is pure, stateless, and always produces the same result
}
```

This function exists in `procgen/inhabitants.rs` already and is unaffected by the Bevy migration. It doesn't touch the ECS world at all — it's a pure function from (settlement_id, index, seed) → details.

---

## 18. Risk Register

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Bevy breaking changes (0.18→0.19) during migration | High | Medium | Pin to exact bevy_ecs version (`=0.18.0`). Complete migration on one version, upgrade after. |
| Performance regression from command indirection | Low | Medium | Benchmark early (Phase 0). Command application is O(n) in commands, which is bounded by entity count. |
| Archetype fragmentation from too many component combinations | Low | Low | Use consistent bundles per entity kind. Avoid adding/removing components dynamically. |
| Loss of causal event chains across command application | Medium | High | Every `SimCommand` carries `caused_by: Option<u64>` and `participants: Vec<(Entity, ParticipantRole)>`. The command applicator links new events to their causes and records all participants. |
| Many-to-many relationships awkward in Bevy | Medium | Medium | Use `RelationshipGraph` resource with `BTreeSet`/`BTreeMap`. Evaluate relationship-as-entity pattern during Phase 1. |
| RNG determinism across parallel systems | Low | Low | Per-system derived RNG. Seed from master + system name + tick. |
| Migration takes longer than expected | High | Low | Each phase is independently valuable. The codebase can run with a mix of old and new systems during migration (systems still emit commands, old systems still mutate world — run them in separate schedule phases). |
| Test coverage regression during migration | Medium | Medium | Port tests alongside systems. Don't delete old tests until new tests cover the same behavior. |
| Entity serialization mismatch at flush time | Medium | Medium | Decomposed components must be reassembled into `EntityData` enum for JSONL compatibility. Write round-trip tests early. Consider changing JSONL format if reassembly is too complex. |
| Helper function migration cascades | Medium | Low | Migrate helpers alongside consuming systems. Provide both old and new signatures temporarily for helpers shared across not-yet-migrated systems. |
| `SimEntityMap` becomes stale or inconsistent | Low | High | All entity spawning/ending goes through the command applicator, which is the single point of truth for map updates. Never bypass it. |

---

## Execution Summary

| Phase | Description | Depends On |
|-------|-------------|------------|
| 0 | Bevy app shell, tick control, clock | — |
| 1 | Components, resources, entity spawning, `SimEntityMap` | Phase 0 |
| 2 | SimCommand events, command applicator, `EventLog` with effects/participants | Phase 1 |
| 3+4 | Migrate systems AND decompose into plugins simultaneously (6 waves) | Phase 2 |
| 5 | Parallelism, scheduling, benchmarks | Phases 3+4 |
| 6 | JSONL flush from Bevy world | Phase 1 |
| 7 | Testing strategy, worldgen/scenario/testutil migration, regression tests | Phases 3-5 |
| 8 | Cleanup, delete old code, update docs | All above |

**Recommended approach:** Phases 3 and 4 should be done **together** — decompose each system into its plugin as you migrate it, rather than migrating monolithically first and decomposing later. This avoids porting a system twice.

Phase 6 (flush) can be done in parallel with system migration since it only depends on the data model (Phase 1).

Helper function migration (§10) happens incrementally during Phase 3+4 — migrate each helper alongside the first system that uses it.
