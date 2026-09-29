# Jump, missile, sensor and damage balance — 2026-09-28

## Reproduction

```sh
cargo test --workspace
cargo build --release -p luminal-cli -p luminal-app
./target/release/luminal-cli --missile-envelope 64
./target/release/luminal-cli --class-balance 3 all stock .35 9000
LUMINAL_DUEL_MISSILES=off ./target/release/luminal-cli --class-balance 2 all stock .01 8000
```

Raw data: [envelope](envelope-final.csv), [closing duels](duels-final.csv),
[beam-only](beams-final.csv). Envelope CSV seeds are offsets from 5000.
The normal workspace suite passed **313 tests**, with eight existing expensive
surveys ignored. The CLI runs above replace those surveys for this balance pass.
The native client also rendered a Destroyer screenshot successfully; headless
UI coverage exercises the jump button, map destination, spool and cancellation.

## Missile physics

LRM: 1,500 g, 55,000 km/s acceleration-integral budget, 7,200-second reactor.
The boost allocation is 42,060.5 km/s. Approximately 4,993.2 km/s is reserved for
the nominal terminal burn, leaving 7,946.3 km/s for correction. The measured
stationary 1.4 AU shot boosts **0.40019 AU**, enters terminal at **0.09909 AU**,
and reaches the target in **6,430.8 s (107.2 min)**, with about 7,990 km/s spare.
The slight phase-distance discrepancy comes from sampled seeker/guidance updates.

SRM: 3,000 g, 40,000 km/s budget, 1,320-second reactor. Continuous powered flight,
with terminal acquisition inside 0.04 AU. Stationary 0.14 AU takes **1,195.1 s
(19.9 min)** and leaves about 4,839 km/s spare. Both types inherit launcher
velocity. Actual flight integrates relativistic motion; ETA planning is approximate.

The LRM ideal terminal roll is exactly **75% at 1.4 AU**, verified independently
of the sampled trials. A recent qualifying resolved ping makes that **85%**.
Physical proximity is mandatory before either roll: accuracy cannot rescue an
expired missile or one whose fuel runs out before interception.

### Isolated results (64 shots per condition)

| Shot | Passive hits | Active-ping hits |
|---|---:|---:|
| LRM, stationary, 1.4 AU | 44/64 | 52/64 |
| SRM, stationary, 0.14 AU | 50/64 | 58/64 |

Samples need not equal analytic probabilities. Defence, ECM and screens are off.
Received seeker measurements and random detection still consume deterministic RNG.

At nominal range, a target approaching at 5,000 km/s was reached in **95.8 min
by LRM** and **17.3 min by SRM**. Receding at 5,000 km/s caused **0/64 hits** for
both payloads: reactor expiry at exactly 120 and 22 minutes. At 1.68 AU, stationary
LRMs also expired. Nominal range rings are therefore not hard distance walls.

Known-boost LRM shots at 1.4 AU fell from 44/64 hits to **0/64 under sustained
evasion**, without left/right reversals. At close SRM range (0.014 AU), auto-evade
left the helm order alone and both trials hit 62/64. Near the SRM outer envelope
(0.168 AU), evasion delayed arrival from 1,310 to about 1,319 seconds, leaving
almost no reactor margin. SRM evasion at nominal range was not a reliable escape;
its small sampled hit-rate differences should not be treated as an accuracy bonus.

The evasion decision uses received motion, nominal class reserve and heat cost,
including the incoming missile's potential acceleration. It does not inspect
actual enemy fuel. This conservative heuristic can still attempt a maneuver that
ultimately fails. A committed direction prevents small seeker changes from
reversing thrust; collision avoidance and a substantially better threat can override it.

## Equal-class combat

Final fits favor launcher width over magazine depth. SRM/LRM rounds and launchers:
Picket 20/0 with 1/0; Frigate 12/10 with 1/1; Destroyer 24/18 with 4/3;
Cruiser 24/36 with 6/6; Battleship 48/48 with 12/8.

Initial wide magazines killed too consistently during the missile stage. Final
strike energies are **0.3 PJ LRM / 0.25 PJ SRM**, with SRMs cycling twice as fast.
Main beam and spinal energy were retained: the existing battleship spinal already
emits ten times its main beam energy, with alignment and a long cooldown.

Three seeds per class, 0.35 AU initial separation, 2,000 km/s closing speed, stock
finite defences, actual thermal/damage rules, received-track pilots and periodic
pings. Both sides close through SRM range and then beam standoff as magazines empty.
Damage below totals both ships, hull plus armour, averaged over the three trials.

| Class | LRM HP | SRM HP | Beam HP | Beam finishes | Eight-hour timeout |
|---|---:|---:|---:|---:|---:|
| Picket | 0 | 975 | 0 | 0/3 | 0/3 |
| Frigate | 200 | 1,598 | 213 | 3/3 | 0/3 |
| Destroyer | 150 | 4,239 | 309 | 1/3 | 2/3 |
| Cruiser | 88 | 1,378 | 3,157 | 3/3 | 0/3 |
| Battleship | 50 | 8,735 | 8,240 | 3/3 | 0/3 |

This supports LRM leakage, substantial SRM damage and beam finishes for survivors.
Picketers have no beams. The two destroyer stalemates are a real limitation:
propulsion/system casualties can prevent either pilot reaching beam range. Damaged
propulsion now stops completely, as requested; destroyed components cannot repair.
No artificial closing teleport or guaranteed late kill was added to hide this.
These small samples establish behavior, not a statistically complete balance claim.

Beam-only controls started at 0.01 AU. All eight armed-class fights finished with
beams in **7–14 simulated minutes**, including battleships in **7–10.5 minutes**.
Two beamless Picket controls correctly timed out.

## Sensor and damage regressions

Tests verify unseen cruise LRMs remain unresolved until a causal ping echo,
interceptors stay idle without a solution, acquired boosts retain their tracks,
SRMs and terminal LRMs always resolve, and ship/station active fire-control support
expires. Automatic missile seeker echoes do not grant the ship-ping accuracy bonus.

Jump tests cover class eligibility, 50 AU map bound, ten-minute inhibition, heat,
cancellation, vector-preserving travel, light-cone gaps, and no swept weapon hit
across a jump. The new installed JUMP component can be damaged/destroyed; jump or
power failure aborts spooling, and stale departure events cannot execute afterward.

Damaged propulsion and power prohibit thrust. Either damaged power or damaged
screens caps screen strength at 50%, without stacking. Tests verify normal recharge
at the cap and no instantaneous charge restoration when repair raises the cap.
