# Frigate duel balance — 2026-09-27

**Snapshot note:** these measured duel timings predate the subsequent removal of
ambient screen heating. Screens now stay cold until hit; rerun the commands below
to measure the new thermal rule. The recorded CSVs are retained as a baseline.

## Fixtures and acceptance

Two equal stationary frigates, initially fully raised cold screens, 100 hull and
100 armour, ten offensive rounds of each type, normal sensors, point defence,
damage, repair, light-time and finite fuel. An initial exact track isolates weapon
delivery from discovery; it is not refreshed from truth. Both fire full magazines.

`--frigate-duel` stops after the offensive rounds have finished. `--frigate-battle`
adds repeating automatic main beams and one active ping per minute, with a
configurable delay before the second ship opens fire. The sustained fixture runs
for two simulated hours, or until both ships are lost / 120 seconds after the
first loss. It does not reload magazines or move the ships closer.

Acceptance: both sides have time to return fire; no opening-salvo knockout of a
fresh equal frigate; sustained close-range pressure can eventually knock one out.
The second shooter must not be guaranteed to lose. Medium-range exchanges should
be survivable, not decisive. This is a first balance pass, not a statistical proof
of equal win rates or a moving/evasive-fleet calibration.

## Selected tuning

- 30 defensive interceptors per ship (station remains 10); offensive stock unchanged.
- Hull: 200 TJ / 100 HP; armour another 200 TJ / 100 points. Previously the hull
  had only 2 TJ and a penetrating beam could kill outright.
- Screen capacity unchanged at 450 TJ; radiation coefficient now `1e-6 W/K^4`,
  about 41 GW at rated heat. The old cooling made sustained beams ineffective.
- Interceptors now accelerate at 3,000g (twice offensive missiles), with 180 seconds
  of full-thrust fuel. The range gate stays 0.3 AU, with about 2.4-hour endurance.
  Their departure burn reserves half the fuel, with fuel-budgeted terminal
  corrections. One-second cruise updates, 0.1-second final guidance; quadratic
  local velocity fitting handles accelerating missile targets.
- No artificial guaranteed interceptions: physical closest approach must still be
  within 100 km before the 75%-base speed-dependent kill roll.
- Nuclear arming distance: two blast radii from launch. Estimated pass detection
  also requires proximity to the estimated target. This fixes launch-side suicide
  bursts caused by noisy early velocity estimates.
- A nearby seeker acquisition no longer cancels a missile's departure boost
  before it builds useful closing speed.

## Sustained battle results

Range 0.002 AU (0.998 light-seconds), 30 interceptors each, seeds 1000–1004,
second ship opens 10 seconds late:

| Seed | First knockout, simulated minutes | Winner | Interceptor kills, both sides |
|---|---:|---|---:|
| 1000 | 59.8 | First shooter | 28 |
| 1001 | 57.5 | First shooter | 22 |
| 1002 | None by 120 | Unfinished | 20 |
| 1003 | None by 120 | Unfinished | 23 |
| 1004 | 116.0 | Second shooter | 20 |

Both ships took dozens of beam/payload hits including screen absorption. The
opening exchange was survivable in every trial. At 10× warp, these knockouts take
roughly 6–12 real minutes if the machine sustains the requested rate. These are
stationary knife-range tests; range alone does not make a stationary 1-AU exchange
turn into a knockout when neither side closes and missile magazines are finite.

Raw final sustained results are in `frigate-battles.csv`; simultaneous-fire results
are in `frigate-simultaneous.csv`. CSV `damage_*` is incident energy, not hull
damage: use `hull_*`, `armour_*` and `first_loss_s` to interpret survival.

Simultaneous-fire seeds 1000–1002 produced knockouts at 74.2 and 81.8 minutes;
the third remained contested at two hours. There were no opening-salvo kills.

## Medium range and magazine selection

Final 1-AU full-magazine exchanges, 30 interceptors each:

| Seed | Hits received, A / B | Interceptor kills | Ships destroyed |
|---|---:|---:|---:|
| 1000 | 0 / 4 | 17 | 0 |
| 1001 | 10 / 10 | 0 | 0 |
| 1002 | 8 / 10 | 5 | 0 |

Screens absorbed almost all incident energy; all six ships finished at full hull
after damage control. This passes survivability, **not** the earlier tighter
"roughly two hits per ship" aspiration. Long-range local guidance and finite
correction fuel still produce high seed-to-seed variance; a nominal 75% kill roll
is not a 75% guarantee of reaching a manoeuvring missile. This remains a known
balance limitation, not a solved numerical target.

The 0.03-AU magazine comparison (seeds 1000–1001) gave 29/28 interceptor kills
with depth 25, versus 32/31 with depth 30. Average incident hits per ship were
5.75 versus 5.5; the sample is too small to claim an optimum. Depth 30 is the
conservative provisional choice: one defensive round per offensive round in the
opponent's full magazine, without making close combat immune to penetration.
Raw data: `frigate-medium.csv` and `frigate-magazines.csv`.

## Performance and correctness

The 210-second, 0.03-AU, 60-offensive-round benchmark fell from approximately
15.5 seconds before the optimisation work to 2.00 seconds locally in the final
build (1.92 CPU seconds). This is an approximate 8× improvement, not a frame-rate
claim. Release build, logging disabled, same seed and stock; the final run was
measured after background calibration jobs finished. Under concurrent calibration
load a 200-second run took 3.65 seconds. The old harness's 200-second request
actually stopped at 210; the final comparison explicitly requests 210.

Work removed includes trajectory-history rescans, cloned target histories,
reconstructed contact maps and excessive light-cone bisections. Safeguarded Newton
roots retain light-time precision; direct echo associations avoid ambiguous body
lookups. GUI updates have an 8 ms between-event budget and do not discard events.
Under overload, achieved warp can fall below requested warp rather than freezing
input. A single large sensor event can still exceed that budget.

Regression gates cover quadratic velocity fitting, launch-side nuclear safety,
continued departure boost, physical interception, deterministic defensive
reduction of hits, and a sustained battle with return fire and a delayed knockout.

## Reproduce

```sh
cargo build --release -p luminal-cli
target/release/luminal-cli --frigate-duel 3 25,30 1
target/release/luminal-cli --frigate-battle 5 30 0.002 10
target/release/luminal-cli --frigate-battle 3 30 0.002 0
LUMINAL_DUEL_STOP_S=210 target/release/luminal-cli --frigate-duel 1 30 0.03
cargo test --workspace
cargo test --release -p luminal-core stationary_duel_is_repeatable_and_interceptors_reduce_hits -- --ignored
cargo test --release -p luminal-core sustained_frigate_battle_survives_opening_and_reaches_knockout -- --ignored
```

`LUMINAL_DUEL_LOG=/tmp/duel.log` enables detailed per-trial diagnostics, overwriting
that file for each trial. Normal calibration runs do not write the game's
`logs/latest.log`. `LUMINAL_PROFILE=1` prints event-category wall-clock timings.
