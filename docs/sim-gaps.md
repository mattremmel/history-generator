# Simulation Gaps Analysis

Review of current systems (Phases 1–5.5) against project goals for emergent, realistic historical behavior.

## Current System Status

| System | Status | Emergent Potential |
|--------|--------|-------------------|
| Core Data Model | Complete | Causal event chains, relationships, signals |
| WorldGen | Complete | Static geography with terrain, rivers, resources, adjacency |
| Demographics | Complete | Population brackets, NPC births with traits/roles, prosperity |
| Politics | Complete | Succession, coups, diplomacy, happiness/legitimacy/stability, faction splits |
| Conflicts | Complete | Wars, armies, supply/attrition, battles, retreats, conquest |
| Agency | Stubbed | Traits + desires exist, action execution not wired up |

## Critical Gaps

### 1. Economy is completely absent

The single biggest gap. In real history, economics drives wars, alliances, migration, social unrest, and technological innovation.

- Wars trigger from enemy relationships + adjacency + randomness, never from resource competition
- Prosperity is a vague per-settlement float with noise, not derived from actual production/trade
- No trade routes, resource flow, scarcity, or surplus
- Factions can't be wealthy or poor in any meaningful way
- **Impact**: Removes the #1 real-world motivator for conflict, diplomacy, and migration

### 2. No family/genealogy tracking

NPCs are born with LocatedIn + MemberOf but no family connections.

- No parent-child relationships recorded at birth
- No marriages or pair-bonding
- No inheritance beyond "oldest faction member becomes leader"
- No dynastic politics, blood feuds, or royal lineages
- **Impact**: Eliminates dynastic succession crises, marriage alliances, inheritance disputes

### 3. Agency system is a skeleton

Actions are defined (Assassinate, BrokerAlliance, AttemptCoup, etc.) but never executed.

- Ambitious NPCs never scheme for power beyond generic coup probability
- No personal rivalries drive events
- No NPC is ever the *cause* of a war — it's all faction-level dice rolls
- **Impact**: History reads as "things happened to factions" not "people made things happen"

### 4. No migration or refugees

Demographics has no immigration/emigration.

- People don't flee conquered or ruined settlements
- No refugee pressure on neighboring settlements
- No cultural mixing from population movement
- The roadmap explicitly calls for refugee movements from wars — not implemented
- **Impact**: Removes a major cascading consequence and source of cultural change

### 5. No disease/plague mechanics

Only background mortality rates exist. No epidemics or pandemics.

- Plagues reshape demographics, economics, and politics for centuries
- Disease spreads along trade routes (which also don't exist yet)
- Armies are historically devastated more by disease than battle
- **Impact**: Missing one of history's most powerful disruptors

## Significant Gaps

### 6. Diplomacy is coin-flip, not motivated

Alliances form at 0.8% × happiness. Rivalries at 0.6% × instability. No:

- Reason *why* two factions ally beyond shared-enemy multiplier
- Trade agreements, defensive pacts, non-aggression treaties
- Marriage alliances (needs genealogy)
- Betrayal of alliances for gain
- Tribute/vassalage relationships

### 7. War motivations are shallow

Wars only trigger from existing Enemy relationships + adjacency. No wars over:

- Resources (no economy)
- Succession claims (no genealogy)
- Religious differences (no religion system)
- Revenge for past wrongs (no faction memory of grievances)
- Territorial ambition beyond "attack neighbor"

### 8. No siege mechanics or fortifications

Conquest is instant when an army reaches an undefended settlement. No:

- Walls, defenses, or garrison forces
- Siege duration or supply requirements
- Civilian casualties from siege
- Fortification building as a political/economic decision

### 9. Population breakdown disconnected from individuals

Settlements track aggregate brackets while also spawning named NPCs, but:

- Named NPCs aren't part of bracket counts (parallel systems)
- Only 2–4 NPCs born per settlement per year — tiny fraction of population
- Most people are statistical, not individuals
- Architecturally fine, but NPC-driven events only affect a thin slice

### 10. No cultural identity or drift

Factions have government types but no:

- Cultural traits (martial, mercantile, scholarly, etc.)
- Cultural blending from conquest or trade
- Cultural resistance to foreign rule
- Distinct naming conventions per culture
- Conquered settlements instantly assimilate — no rebellions or cultural tension

## Smaller Gaps

| Gap | Why It Matters |
|-----|---------------|
| No buildings/infrastructure | Can't model walls, temples, markets — no settlement investment |
| No literacy/education | No basis for knowledge system (Phase 6) |
| No seasonal effects | Harvests, winter campaigns, famines don't exist |
| No naval capability | Coastal factions can't project power across water |
| No mercenaries | Small factions can't punch above their weight |
| No crime/banditry | No internal security challenges |
| No natural disasters | No famines, floods, earthquakes disrupting systems |
| No reputation/prestige | Factions and NPCs have no "fame" that influences others |
| No war goals/peace terms | Wars just end — no territorial demands, reparations, or treaties |

## What's Working Well

- **Signal system** — leader dies → vacancy → succession → instability → coup → war cascades
- **Causal event chains** — `caused_by` on events enables narrative tracing
- **Monthly conflict resolution** — supply, morale, and movement create realistic campaign arcs
- **Terrain-dependent attrition** — mountain campaigns are grueling, swamps deadly, deserts impassable
- **Faction splits from misery** — low stability + low happiness → breakaway factions → new conflicts
- **Trait-weighted decisions** — Aggressive leaders start wars more often, Ambitious NPCs coup more

## Recommended Priority Order

Based on cascading impact per unit of effort:

1. **Wire up Agency** — NPCs acting on desires makes history personal. An ambitious NPC assassinating a leader is more compelling than "a coup happened."
2. **Add family/genealogy** — Parent-child + spouse relationships. Dynastic succession, inheritance, marriage alliances. Unlocks blood feuds and succession crises.
3. **Basic economy** — Resource production, trade along adjacency, wealth as a faction property. Makes war motivations real and prosperity meaningful.
4. **Migration/refugees** — Population movement from war, famine, conquest. Cultural mixing, demographic pressure, cascading consequences.
5. **War goals and peace terms** — Purposeful wars and meaningful (breakable) peace treaties.
6. **Cultural identity** — Traits on factions that drift, blend, and create tension under foreign rule. Enables rebellions and cultural resistance.
