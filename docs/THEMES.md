# Fleet vocabulary and visual direction

Themes are presentation choices in the native app. They never enter a simulation
command, seed, loadout, sensor calculation or weapon rule. The original Luminal
vocabulary remains available. The selected theme survives scenario restarts;
a fresh application launch defaults to Luminal unless `LUMINAL_THEME` is set.

The startup screen previews these aliases before deployment. Ship classes and
comparison stats retain their canonical names, as do subsystem short codes and
technical combat reports. The themed names appear in weapon controls, range
labels, movement standoffs, jump controls, screen controls and system tooltips.
They are creative setting-inspired aliases for the same mechanics.

| Mechanic | Starfleet | Grim Dark | Imperium (Traveller-inspired) | Culture |
| --- | --- | --- | --- | --- |
| LRM | Photon torpedo | Void torpedo | Smart missile | Guided dart |
| SRM | Micro torpedo | Strike salvo | Burst missile | Shard swarm |
| Beam | Phaser | Lance | Beam laser | Coherent beam |
| Spinal | Phaser lance | Nova lance | Spinal laser | Grid lance |
| Jump drive | Warp drive | Warp engine | Jump drive | Displacer |
| Screens | Deflectors | Void shields | Screens | Fields |
| Power | Warp core | Plasma shrine | Power generation | Energy bank |
| Ship mind | Main computer | Machine spirit | Ship mind | Mind |
| Damage control | Damage control | Enginseer crews | Damage control | Repair drones |

A Warp Drive or Displacer still takes ten minutes to spool, drops the same
systems, travels at 1 AU/s and preserves velocity. A photon torpedo still uses
the existing LRM flight profile and damage model. No setting-specific capability
is implied or added by its name.

## Ideas for a later cosmetic pass

- **Starfleet:** warm amber and violet instrument accents, rounded department
  tabs, survey-style mission briefings, named bridge stations, and restrained
  computer status announcements. Ship names could draw from explorers and
  scientific instruments. Withdrawal wording: “Disengagement authorized.”
- **Grim Dark:** brass rules, dark red alert accents, engraved fleet seals,
  gothic ship names and solemn engineering reports. Damaged systems might read
  “Machine spirit distressed”; a repair completes with “Rite concluded.” Keep
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
The app test suite passes 45 tests, including clicking a theme, selecting a card,
deploying it and restarting. That regression compares ship class, ammunition,
position, velocity and damage across all five themes under the same seeded
scenario. No core simulation files changed.
