# Capital defense correction and endgame pacing

## Applied change

Cruiser interceptor capacity is now **80** (formerly 200); Battleship capacity
is **120** (formerly 240). Other classes, launcher cadence, interceptor guidance,
kill probabilities, missile flight parameters and damage are unchanged. The
native release game and CLI were rebuilt. Existing battles already have their
magazines instantiated; a new battle uses the updated fittings.

The [preceding review](../2026-09-29-review/report.md) identified these fits using
seeds 24000–24007. This follow-up validates them with 16 fresh seeds per class,
40000–40015, firing SRMs across their useful envelope. It also runs eight actual
Doctrine missions and four diagnostic runs. Raw CSVs and mission transcripts
are stored alongside this report.

| Class | Interceptors | Mean LRM / SRM / beam damage | Beam finishes | Timeouts |
| --- | ---: | --- | ---: | ---: |
| Cruiser | 80 | 239 / 2862 / 3238 | 16/16 | 0/16 |
| Battleship | 120 | 75 / 2305 / 12130 | 14/16 | 0/16 |

Damage is combined hull plus armour HP across both combatants. It is not a
per-shot value. Both new fits allow substantial SRM damage while preserving
beam finishes in most fights. These fresh-seed results support the direction
of the adjustment without establishing a universal matchup outcome.

The eight mission trials (two classes, four seeds each) produced one raider
victory and seven runs without an outcome at 12 hours. Both matched Cruiser
seeds remained unfinished; Battleship seed 32000 now ended in the player's
Battleship being destroyed. The fitting change restores missile damage, but
does not by itself solve encounter/AI pacing. This small mission sample is not
a win-rate estimate.

Validation: 315 release-workspace tests passed, eight existing expensive surveys
ignored. No endgame rule changes have been implemented in this follow-up.

## Two different kinds of slow ending

The diagnostics use unchanged Destroyer fittings, so they isolate the endgame
issue from the capital ammunition correction.

**A chase that needed more time:** seed 12004 had a healthy attacker closing on
a drifting opponent at eight hours. The opponent had destroyed propulsion,
passive sensors and ship mind. Extending the run produced a beam finish at
31,320 seconds: **8 h 42 min**, only 42 minutes beyond the earlier cutoff.
A timeout in this case was not proof of a deadlock.

**A fight with no remaining way to finish:** seed 12010 had one ship with intact
mobility, a destroyed main beam and exhausted missiles. Its opponent had damaged
power/propulsion and destroyed damage control. It could not repair its power,
and powered weapons could not operate. The ships remained about two light-seconds
apart, alive and without a decisive hit, even after **16 hours**.

[Final eight-hour system states](endgame-states.txt) capture the distinction.
Destroyed systems are not repaired by the current repair routine. Damaged systems
normally take 20 minutes each at full repair effectiveness, longer with damaged
crew, mind or damage control. Power and damage-control repairs take priority;
remaining work uses a persistent randomly chosen target.

## Proposed pacing changes, in implementation order

### 1. Make the AI recognize when its combat plan is impossible

Before selecting weapon standoff, evaluate usable weapons, remaining ammunition,
power, repair prospects, movement and jump capability. A ship with no recoverable
weapons should not keep issuing a beam-range order. Give it an explicit choice:
withdraw, attempt a jump, or offer surrender if escape is unavailable.

Use the ship's own system state for its decision. Opponents must learn about
withdrawal or surrender through normal light-delayed reports, not hidden damage
state. Lost sensors or a quiet target alone must not count as proof of surrender.

This directly addresses the seed-12010 failure without weakening the rule that
damaged propulsion or power disables thrust.

### 2. Add deliberate disengagement and surrender outcomes

Allow a combatant to concede the contested objective and withdraw. For an escort
mission, a raider abandoning the attack can count as an escort success even if
its hull survives. A departing escort cannot claim success while leaving a
credible raider threat to the transport.

Suggested initial flow: a combat-ineffective AI transmits a surrender offer; the
player may accept or continue attacking. Accepted surrender ends that ship's
hostile actions and makes the result explicit. Withdrawal must actually remove
the threat or forfeit the objective; it must not be a free button to avoid a
missile already about to arrive.

Do not implement a blanket "damaged engine = dead ship" rule. An immobile ship
with live weapons may still be dangerous. If neither side can fight or achieve
its objective, offer a disengagement/draw outcome rather than awarding a hidden
win based on hull percentage.

### 3. Fast-forward waits that still have a meaningful destination

Provide **Continue to next tactical event** for pursuit, repair and jump spool
waits. It should stop on a newly received threat, relevant contact change,
repair completion, arrival at useful weapon range, jump transition, or mission
outcome. Keep ordinary pause/control available and show what the ship is waiting
for, with an estimate when possible.

Advance the normal simulation and preserve light delay, fuel, heat and damage.
This would make the extra 42-minute chase tolerable without altering its result.
It should not reveal hidden enemy events before the player's sensors receive them.

### 4. Make repair priorities follow the plan

Keep power and damage control first. Then select repairs according to intent:
propulsion for a chase, an available primary weapon for a fight, or jump drive
for withdrawal. Expose a small player choice such as **Fight / Escape / Automatic**
and the current repair ETA.

Avoid repeatedly switching targets and discarding repair progress. Keep destroyed
systems destroyed. Faster repair is not the first recommendation: making useful
repairs happen first is less likely to trivialize subsystem damage.

### 5. Let a capable losing ship attempt a jump

A Destroyer or larger ship with working power and jump drive can choose to spool
when continuing combat is hopeless. The existing ten-minute spool, dropped
screens, heat and inability to thrust provide a visible interruption window.
Use a signalled withdrawal state so the player knows why the enemy has stopped
maneuvering and can decide whether to close and prevent escape.

A damaged jump drive still fails. Spooling is not instant safety, and starting a
jump must not itself award an escape result.

## Recommendation

Implement **AI capability checks and explicit disengagement/surrender** first;
then add event-based fast-forward. Follow with goal-based repairs and jump-aware
withdrawal. These address unwinnable fights and long but valid pursuits separately.
Keep beam damage and the requested propulsion/power damage behavior intact.

## Reproduction

```sh
cargo build --release -p luminal-cli -p luminal-app
cargo test --release --workspace
LUMINAL_DUEL_FIRE_RANGE=envelope ./target/release/luminal-cli --class-balance 16 Cruiser stock .35 40000 > calibration/2026-09-29-capital-fits/cruiser-holdout.csv
LUMINAL_DUEL_FIRE_RANGE=envelope ./target/release/luminal-cli --class-balance 16 Battleship stock .35 40000 > calibration/2026-09-29-capital-fits/battleship-holdout.csv
python3 calibration/2026-09-29-capital-fits/run_missions.py
LUMINAL_DUEL_LOG=/tmp/luminal-endgame-12004.log ./target/release/luminal-cli --class-balance 1 Destroyer stock .35 12004
LUMINAL_DUEL_LOG=/tmp/luminal-endgame-12010.log ./target/release/luminal-cli --class-balance 1 Destroyer stock .35 12010
LUMINAL_DUEL_STOP_S=57600 ./target/release/luminal-cli --class-balance 1 Destroyer stock .35 12004
LUMINAL_DUEL_STOP_S=57600 ./target/release/luminal-cli --class-balance 1 Destroyer stock .35 12010
```
