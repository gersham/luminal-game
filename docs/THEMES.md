# Fleet vocabulary and visual direction

Themes select vocabulary, ship identities and distinct physical star systems.
Ship fits and weapon rules remain common, while gravity, occlusion and travel
geometry vary with the selected system. The original Luminal vocabulary and Sol
system remain available. The selected theme survives scenario restarts;
a fresh application launch randomly preselects a theme and combat ship class.
`LUMINAL_THEME` and `LUMINAL_SHIP` override those choices for previews.
`LUMINAL_SEED` makes the random selections repeatable.

The startup screen previews these aliases before deployment. Full ship types now follow each theme, including distinct opposing fleets.
The five underlying balance tiers and their abbreviations remain stable.
Comparison stats and technical combat reports keep their original units. The themed names appear in weapon controls, range
labels, movement standoffs, jump controls, screen controls and system tooltips.
They are creative setting-inspired aliases for the same mechanics.

| Mechanic | Grim Dark | Imperium (Traveller-inspired) | Culture |
| --- | --- | --- | --- |
| LRM | Void torpedo | Smart missile | Guided dart |
| SRM | Strike salvo | Burst missile | Shard swarm |
| Beam | Lance | Beam laser | Coherent beam |
| Spinal | Nova lance | Spinal laser | Grid lance |
| Jump drive | Warp engine | Jump drive | Displacer |
| Screens | Void shields | Screens | Fields |
| Power | Plasma shrine | Power generation | Energy bank |
| Ship mind | Servitor | AI | Mind |
| Damage control | Enginseer crews | Damage control | Repair drones |

A Warp Engine or Displacer still takes ten minutes to spool, drops the same
systems, travels at 1 AU/s and preserves velocity. A void torpedo still uses
the existing LRM flight profile and damage model. No setting-specific capability
is implied or added by its name.

## Ideas for a later cosmetic pass

- **Grim Dark:** brass rules, dark red alert accents, engraved fleet seals,
  gothic ship names and solemn engineering reports. Damaged systems might read
  “Servitor impaired”; a repair completes with “Rite concluded.” Keep
  crucial state words such as damaged/destroyed visible alongside the flavor.
- **Imperium:** navy registry cards, hex-sector motifs, merchant convoy manifests,
  jump-route navigation terminology and professional naval brevity. Ship names
  could use ports, worlds and historical commanders. Withdrawal wording:
  “Breaking contact; jump solution accepted.”
- **Culture:** spacious white/cyan displays, geometric hull drawings, witty ship
  names and dry Mind commentary. Retreat could read “I propose we be elsewhere.”
  Put wit in secondary text; leave firing solutions and damage readouts precise.

These are future directions, not implemented palette, audio, lore or mechanic
changes. Keep the naval recognition-card layout across themes, with themed
silhouettes as a possible later asset pass. Avoid flavor that obscures range,
fuel, damage or the vulnerable withdrawal window.

## Verification

The native startup and Grim Dark tactical HUD were rendered and visually checked.
The app regressions include clicking a theme, selecting a card,
deploying it and restarting. That regression compares ship class, ammunition,
position, velocity and damage across all four themes under the same seeded
scenario. Cosmetic identity is stored separately from mechanical class; tests verify that naming does not alter trajectory, damage or thermal evolution.

## Named ships and stations

[The full roster](SHIP_ROSTERS.md) contains 480 names: ten per player combat
class, ten per opposing combat class, ten shared civilian names and ten station
names per theme. Civilian pools apply to all unarmed civilian hulls, not just
transports. A separate random stream selects names at scenario creation, so
cosmetic selection never consumes the combat or sensing random sequence.
Names remain stable for that scenario. Opponent names appear only after a
received identity-level observation, and remain remembered when precision
telemetry expires. Full class names appear once the contact is resolved.

Grim Dark uses a Servitor, Imperium an AI and Culture a
Mind for the command-computer system. Existing control logic is unchanged.

The former ocean-ship sketches have been replaced by generated, setting-inspired
spacecraft line art. See [asset provenance](../assets/ships/README.md). The owner
intends this as a personal game and does not intend to distribute it.

## Physical systems

These are original setting-inspired locations, not canonical atlas recreations.

| Theme | Star | Planets | Moons | Starting world / station moon |
| --- | --- | --- | --- | --- |
| Luminal | Sol | 8 | 13 | Earth / Moon |
| Grim Dark | Vesper | 6 | 7 | Saint Verena / Reliquary |
| Imperium | Kestrel | 5 | 6 | Kestrel Prime / Portfall |
| Culture | Quiet Reach | 4 | 8 | Lilt / Aside |

Vesper has inner furnace/forge worlds, a shrine world at 1.35 AU and gas giants
at 7.4 and 24 AU. Kestrel's trade world sits at 0.88 AU, with refuelling giants
at 4.1 and 13.2 AU. Quiet Reach starts at 1.65 AU around a two-moon inhabited
world, with moon-rich giants at 6.2 and 18.6 AU. Stellar masses, planetary
masses/radii, satellite spacing and seeded phases differ between systems.

Celestials remain frozen during play as in the original Sol model. Ships still
experience their gravity and occlusion. The same escort mission is initialized
using the selected homeworld and station moon. Weapon balance is unchanged;
mission geometry and therefore difficulty can vary. Jump radius is measured
from the selected central star, and stays 50 AU.
