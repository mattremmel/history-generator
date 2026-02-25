# Simulation Gaps Analysis

Review of current systems against project goals for emergent, realistic historical behavior.

## Current System Status

| System | Status | Emergent Potential |
|--------|--------|-------------------|
| Core Data Model | Complete | Causal event chains, relationships, signals |
| WorldGen | Complete | Static geography with terrain, rivers, resources, adjacency |
| Demographics | Complete | Population brackets, NPC births with traits/roles, prosperity |
| Politics | Complete | Succession, coups, diplomacy, happiness/legitimacy/stability, faction splits |
| Conflicts | Complete | Wars, armies, supply/attrition, battles, sieges, retreats, conquest |
| Agency | Complete | Signal reactions, trait-modulated probability, defection, elections |
| Economy | Complete | Resource production, multi-hop trade routes, treasuries, alliance strength |
| Environment | Complete | Seasons, climate zones, instant + persistent disasters, geographic feature creation |
| Buildings | Complete | 7 building types, construction/upgrade/decay, cross-system bonuses |
| Disease | Complete | Disease entities, outbreaks, trade-route spread, immunity, quarantine |
| Culture | Complete | Culture entities, values, naming styles, drift/blending/tension, rebellions |
| Religion | Complete | Religions, deities, tenets, fervor, proselytism, schisms, prophecy |
| Crime | Complete | Crime rates, guard strength, bandit factions, trade/settlement raiding |
| Knowledge | Complete | Ground-truth knowledge, manifestations, derivation chains, distortions |
| Reputation | Complete | Prestige for persons/factions/settlements, tier promotions, cross-system integration |
| Items | Complete | Crafting, resonance accumulation, tier promotion, provenance tracking |
| Migration | Complete | Conquest flight, economic emigration, NPC relocation, refugees |

## Critical Gaps

### ~~1. Economy is completely absent~~ ✅ Resolved

### ~~2. No family/genealogy tracking~~ ✅ Resolved

### ~~3. Agency system is a skeleton~~ ✅ Resolved

### ~~4. No migration or refugees~~ ✅ Resolved

### ~~5. No disease/plague mechanics~~ ✅ Resolved

## Significant Gaps

### ~~6. Diplomacy is coin-flip, not motivated~~ Mostly ✅ Resolved

- ~~Reason *why* two factions ally beyond shared-enemy multiplier~~ ✅ trade routes and alliance strength system
- ~~Trade agreements~~ ✅ trade routes between factions, defensive pacts, non-aggression treaties
- ~~Marriage alliances (needs genealogy)~~ ✅ cross-faction marriages now create/strengthen alliances
- ~~Betrayal of alliances for gain~~ ✅ alliance betrayal system with trust, cooldowns, third-party cascade
- ~~Tribute/vassalage relationships~~ ✅ tribute system with yearly payments and duration tracking

### ~~7. War motivations are shallow~~ ✅ Resolved

- ~~Resources (no economy)~~ ✅ resource scarcity and wealth inequality now drive economic_war_motivation
- ~~Succession claims~~ ✅ claims from blood relatives, crises, PressClaim wars, peace terms install claimant
- ~~Religious differences~~ ✅ religious fervor now adds war motivation between factions of different religions
- ~~Revenge for past wrongs (no faction memory of grievances)~~ ✅ grievance memory system with grudges, escalation, revenge motivation
- ~~Territorial ambition beyond "attack neighbor"~~ ✅ expansionist AI targets weak neighbors, strategic land grabs

### ~~8. No siege mechanics or fortifications~~ ✅ Resolved

Sieges with multi-month duration, starvation, surrender checks, assault attempts. Fortification levels 0-3 with population/treasury requirements. Cross-system integration with economy, politics, disease.

### 9. Population breakdown disconnected from individuals

Architecturally fine — named NPCs are a thin slice of aggregate population. NPC-driven events (coups, assassinations, defection) affect leadership and politics while demographics handles the bulk.

### ~~10. No cultural identity or drift~~ ✅ Resolved

## Smaller Gaps

| Gap | Status | Why It Matters |
|-----|--------|---------------|
| ~~No buildings/infrastructure~~ | ✅ Resolved | 7 building types with construction, upgrade, decay, cross-system bonuses |
| No literacy/education | Open | No basis for knowledge propagation speed or accuracy modifiers |
| ~~No seasonal effects~~ | ✅ Resolved | Monthly seasons with food/trade/construction/disease/army modifiers |
| No naval capability | Open | Coastal factions can't project power across water |
| ~~No mercenaries~~ | ✅ Resolved | Mercenary companies, hiring, combat integration, loyalty |
| ~~No crime/banditry~~ | ✅ Resolved | Crime rates, guard strength, bandit factions, trade/settlement raiding |
| ~~No natural disasters~~ | ✅ Resolved | 7 disaster types (instant + persistent), terrain-gated, geographic feature creation |
| ~~No reputation/prestige~~ | ✅ Resolved | Prestige for persons/factions/settlements, tier promotions, cross-system modifiers |
| ~~No war goals/peace terms~~ | ✅ Resolved | Structured war goals, peace terms with reparations, tribute system |

## Remaining Open Items

### ~~Alliance Betrayal~~ ✅ Resolved

Alliance betrayal system with diplomatic trust, betrayal cooldowns, trait-modulated decisions (Cunning, Ruthless, Honorable), third-party cascade reactions, and cross-system integration (reputation, politics, knowledge).

### ~~Succession Claims~~ ✅ Resolved

Succession claims from blood relatives (children, siblings, grandchildren, spouses) when Hereditary faction leaders die. Claims decay yearly. Succession crises fire when strong claimants exist. Leaders can press claims via agency starting succession wars. Winning installs claimant as target faction leader. Coups and faction splits also create claims.

### ~~Grievance Memory~~ ✅ Resolved

Grievance memory system with faction grudges, cross-leader persistence, escalating tensions from repeated conflicts, and revenge-driven war motivation.

### ~~Territorial Ambition~~ ✅ Resolved

Expansionist AI that targets weak neighbors regardless of enmity, strategic land grabs for resources or chokepoints.

### Other
- Naval capability (coastal power projection)
- ~~Mercenaries (small factions punching above weight)~~ ✅ mercenary companies, hiring, combat integration, loyalty
- Literacy/education (knowledge propagation modifiers)
- Burials (Phase 7 — graves, epitaphs, goods)
- Language/phonology (Phase 8 — name generation from linguistic rules)
- Monsters/ecology (Phase 9 — adventure sites, threats)
- Settlement-to-ruin lifecycle (Phase 9 — abandoned settlements become dungeons)

## What's Working Well

- **Signal system** — leader dies → vacancy → succession → instability → coup → war cascades
- **Causal event chains** — `caused_by` on events enables narrative tracing
- **Monthly conflict resolution** — supply, morale, and movement create realistic campaign arcs
- **Terrain-dependent attrition** — mountain campaigns are grueling, swamps deadly, deserts impassable
- **Faction splits from misery** — low stability + low happiness → breakaway factions → new conflicts
- **Trait-weighted decisions** — Aggressive leaders start wars more often, Ambitious NPCs coup more
- **Cross-system signal propagation** — disasters → disease → economic collapse → migration → cultural mixing
- **Prestige feedback loops** — victories build prestige → harsher peace terms → tribute → more wealth → more prestige
- **Religious tension** — different religions in same settlement → tension → schisms → new factions
- **Crime feedback** — instability → crime → bandits → trade raiding → more instability
