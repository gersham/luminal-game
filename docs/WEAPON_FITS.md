# Class weapon fits and future fleet roles

The existing LRM → SRM → beam progression remains intact. Pulse batteries and
canister missiles give classes different weapon presentation without changing
launcher counts, ammunition, aim rolls, damage, penetration or defensive targets.
A visible pulse train is one beam shot; canister fragments are one missile impact.

| Hull | Weapon presentation | Intended mixed-fleet role |
| --- | --- | --- |
| Picket | Small 3-streak canisters; no offensive beam | Fast scout and pursuit screen |
| Frigate | 3-pulse battery, 5-streak canisters, optional electronic projector | Escort and electronic support |
| Destroyer | 5-pulse battery, 7-streak canisters | Close assault alongside the escorts |
| Cruiser | Single heavy beam, 5-streak canisters, existing LRM batteries | Long-range missile artillery |
| Battleship | Heavy beam, 11-streak canisters, existing spinal mount | Main battle line and close-range finisher |

The roles describe intended cooperation; this change does not add a fleet setup
screen, formation commands, shared point defence, or new sensor-sharing rules.

## Frigate electronic projector

The beam card's **DAMAGE / DISRUPT** control selects its firing mode. AUTO,
DIRECT and HOLD still control when it fires. Disruption uses the normal beam
recharge, capacitor energy and waste heat, and needs functioning beam and ECM
systems. Jumping prevents firing.

A successful shot reduces the target's offensive beam and spinal energy coupling
by 15% for eight simulation seconds, at up to six light-seconds. It causes no
hull damage. It does not weaken screens, propulsion, missiles or point defence.
Light travel time, received target tracks, pointing error and celestial occlusion
all apply. The effect requires at least 10% geometric coupling. Subsequent hits
refresh expiry to eight seconds after arrival; they never add strength or bank
duration. Beams already emitted retain their original coupling.

This is an optional defensive trade: the frigate gives up its own damage shot.
It is most relevant when keeping a valuable ally alive through an enemy heavy
beam exchange. It is not intended to beat direct damage in every duel.

AI frigates retain damage mode when alone. With an armed heavy ally engaging
within projector range, one eligible escort may disrupt a resolved enemy
Destroyer, Cruiser or Battleship. The lowest-ID eligible frigate volunteers;
other escorts retain damage. Selection uses the faction's received view.
This is a first cooperation rule, not a claim that fleet balance is complete.

## Vocabulary

| Weapon | Culture | Grim Dark | Imperium | Luminal |
| --- | --- | --- | --- | --- |
| Pulse battery | Coherent Burst | Lance Battery | Pulse Lasers | Pulse Battery |
| Canister SRM | Shard Swarm | Frag Torpedo | Canister Missile | Flechette Bus |
| Projector | Effector | Vox-Scourge | Electronic Attack | Directed Jammer |

Themes change wording only. These are game interpretations, not attempts to
reproduce every setting's canon capabilities. Penetrator missiles, decoy buses
and additional particle weapons remain future possibilities.

See [validation](../calibration/2026-09-29-weapon-flavour/report.md).
