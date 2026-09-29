Later changes and final 3 LS defence validation: [fire-control-and-defence.md](fire-control-and-defence.md).

# Matched-class combat balance — 28 September 2026

Two passes were run: initial weapon/defence tuning, then a larger validation pass
with different seeds, close starts, fast encounters, beam-only fights, full
scenarios, and investigation of every unexpected timeout. Validation found
additional guidance, event-ordering, and memory failures. The CSV files here were
rerun after those fixes; they are the final results, not the earlier tuning runs.

## Standard closing duels

Each class fights an equal fitted ship. Seeds 2000–2011; initial separation
0.35 AU, total closing speed 2,000 km/s, eight-hour limit. Both ships use their
received sensor pictures, AUTO evasion and local defence. The scripted pilots
fire LRMs while closing, open SRMs inside 0.02 AU, then close to 2 LS for beams.
Screens start charged. Heat dumping starts above the throttle threshold and ends
below 25%. Only the initial resolved position/velocity is a test fixture;
subsequent sensing, interception, damage, repair and heat use the game model.

These are controlled weapon-balance duels, not a claim that the scenario raider
uses exactly this firing policy. A trial ends at the first loss, so incoming
counterfire after that instant is not counted.

| Class | Beam finishes | Missile finishes | Unfinished | Duels with penetrating LRM damage | Mean SRM armour+hull damage, both sides | Mean beam armour+hull damage, both sides |
|---|---:|---:|---:|---:|---:|---:|
| Picket | 0/12 | 10/12 | 2/12 | — | 1,042 | 0 |
| Frigate | 10/12 | 2/12 | 0 | 4/12 | 1,114 | 1,184 |
| Destroyer | 11/12 | 1/12 | 0 | 6/12 | 2,510 | 1,695 |
| Cruiser | 11/12 | 1/12 | 0 | 11/12 | 3,581 | 4,884 |
| Battleship | 12/12 | 0/12 | 0 | 7/12 | 2,803 | 16,521 |

Damage columns sum actual armour and hull loss, excluding screen absorption.
Repairs mean cumulative damage can exceed a ship's starting HP. Spinal shots are
included in beam damage. `reached_beams` requires at least 5% of one ship's maximum
hull HP in cumulative beam penetration while both are alive; a harmless grazing
or screen-only hit does not qualify. `loss_reason` distinguishes hull destruction
from a destroyed reactor.

Pickets have finite SRMs and no beam: two trials exhausted ammunition with both
ships alive. That limitation is retained rather than inventing a replacement gun
or forcing a winner. The larger classes all reached an actual loss in this batch.
Twelve seeds per class describe this sample, not precise universal win rates.

## Additional validation

- **15 close starts:** three seeds per class, 0.01 AU, initially stationary.
  All ended to missiles in 340–480 simulation seconds. Close volleys are punishing.
- **Six fast encounters:** frigate and destroyer, seeds 4000–4002, 0.35 AU and
  10,000 km/s total closure. Five ended to beams within eight hours. Destroyer
  seed 4002 was extended: a damaged survivor was eventually killed through its
  hull at 30,415 seconds (8h 27m), with meaningful beam damage throughout.
- **Three battleship beam-only starts:** initially stationary at roughly 40 LS,
  seeds 2000–2002. All ended through hull depletion in 1,360–1,480 seconds, rather
  than a tiny grazing beam randomly destroying an otherwise intact reactor.
- **One additional battleship memory run:** seed 1005; measured elapsed time and
  peak RSS are recorded in `battleship-memory.txt` beside its combat CSV.
- **Full scenario smoke runs:** raider AI enabled, player escort left on its
  initial orders. Frigate seed 2001 ran for 24h; battleship seed 2000 ran for 24h
  and then a fresh 48h run. These completed without a crash. They did **not** end
  the mission within those limits. In the battleship run, the raider began its
  LRM attack after 24h and damaged the transport. Long travel/AI positioning and
  a passive player are distinct from the controlled duel results above.

This is 84 distinct main/variant duel cases, one extended rerun, and one additional
memory-measurement duel. The scenario transcripts are separate smoke evidence.

## Failures found and fixed

1. **Microscopic reactor criticals.** The live loss was two power-system hits
   roughly 20 seconds apart with 7,999.88/8,000 hull remaining. Critical rolls now
   require penetrating energy worth at least 1% of maximum hull HP, before armour
   shares that penetration. Tiny hits still cause proportional HP damage.
2. **Stale-in-transit missile guidance.** A current resolved datalink picture was
   rejected after spending more than 120 seconds in transit. Guidance now checks
   its resolution at the sending picture's epoch, then predicts to current time.
   Approximate shots retain their fixed ellipse-centre aim until local acquisition.
3. **Terminal correction applied during the whole resolved flight.** The finite
   terminal correction budget was preventing useful cruise updates. Resolved
   cruise can update its course; the terminal phase and all approximate potshots
   remain correction-limited. Interceptors have a separate finite correction budget.
4. **Interception without physical contact / lingering rounds.** Interceptors
   now require a swept pass inside their kill radius before the chance roll can
   kill. A killed missile is immediately inactive, retired from resolved tracking,
   and cannot attack later. The visual bloom follows its former route for one
   wall-clock second. A missed interceptor continues moving during its fade.
5. **Late terminal effects.** Offensive hit/miss events were created after
   retirement had already flushed events to observers. The outcome is now created
   first, so disappearance and the terminal animation arrive together.
6. **Close-range mutual thrust oscillation.** Destroyer seed 2000 and cruiser
   seed 2000 sat at 2 LS firing ineffective beams. Their range controllers copied
   delayed hostile acceleration at unity gain and sustained alternating burns.
   Hostile range-holding now uses 25% acceleration feed-forward plus velocity
   feedback. Both failing seeds now reach damaging beam fights and hull losses.
   Friendly formation following retains its separate controller.
7. **Large-battle memory growth.** Every remote recipient copied entire sensor
   pictures, and new historical pictures kept known-dead missile contacts. The
   old battleship worker reached roughly 17 GiB RSS and contributed to swapping.
   Historical pictures and recipient caches now share immutable storage; new
   pictures omit retired contacts and dead recipients release caches. Tests check
   sharing and that old in-flight pictures remain unchanged. A post-fix battleship
   seed 2000 run took 6.79 seconds with 707,740 KiB peak RSS; different encounter
   histories have different peaks, so this is not an exact before/after benchmark.
8. **Shield and large-ship weapon scaling.** Screens recharged between major
   hits, regular beams did not scale by class, and the enlarged spinal pulse
   exceeded the old capacitor. Recharge is now 2%/minute. Beam and capacitor
   sizing are consistent with the fitted class.

## Final tuning

| Class | SRM / LRM rounds | Interceptors | PD lasers | Main beam pulse |
|---|---:|---:|---:|---:|
| Picket | 20 / 0 | 2 | 1 | None |
| Frigate | 20 / 10 | 40 | 2 | 75 TJ |
| Destroyer | 40 / 20 | 80 | 4 | 150 TJ |
| Cruiser | 40 / 80 | 200 | 6 | 150 TJ |
| Battleship | 160 / 60 | 240 | 8 | 300 TJ |

LRMs deliver 1 PJ and SRMs 0.5 PJ: respectively 10s and 5s launcher reloads keep
equal nominal output per launcher. The SRM burst envelope is 5,000 km with a
10,000 km seeker; the LRM keeps its much larger nuclear-laser strike envelope.
Missile screen punctures have a 35% chance to pass 25% of pulse energy and only
cause a guaranteed component shock when the damage threshold is met.
Laser point defence has an 85% per-shot ceiling, allowing occasional close leaks.
Battleship spinal pulses deliver 3 PJ every 120s, subject to facing, heat and power.

## Reproduction and checks

```sh
cargo build --release -p luminal-cli
# Number of seeds, class, interceptor depth, initial AU, first seed:
target/release/luminal-cli --class-balance 12 all stock 0.35 2000
LUMINAL_DUEL_CLOSURE_KMS=0 target/release/luminal-cli --class-balance 3 all stock 0.01 3000
LUMINAL_DUEL_CLOSURE_KMS=10000 target/release/luminal-cli --class-balance 3 Destroyer stock 0.35 4000
LUMINAL_DUEL_CLOSURE_KMS=0 LUMINAL_DUEL_MISSILES=off target/release/luminal-cli --class-balance 3 Battleship stock 0.08016 2000
# Optional detailed damage, guidance and tracking log; optional duration override:
LUMINAL_DUEL_LOG=/tmp/duel.log LUMINAL_DUEL_STOP_S=57600 target/release/luminal-cli --class-balance 1 Destroyer stock 0.35 4002
target/release/luminal-cli --raider-only 48 120 Battleship 2000
cargo test --workspace
cargo test --release -p luminal-core -- --ignored --nocapture
cargo clippy --workspace --all-targets -- -D warnings
```

Final checks: 282 ordinary tests passed (37 app, 245 core), all seven explicitly
run slow tests/surveys passed, and Clippy passed with warnings denied. Coverage
includes 64-seed paired evasion trials, four 64-seed approximate-target ellipse
sizes, physical interception/removal, terminal-event delivery, moving/fading
effects, sub-GW heat-gauge visibility, thermal conservation and screen recharge.
The obsolete survey helpers were updated to use the current calibrated launcher
instead of a remote spotter fixture that tried to launch before its track arrived.
The installed Omarchy launcher was checked in fullscreen at ship selection.
