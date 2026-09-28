# Full salvos against exhausted defences

2026-09-27. Rebalances the probability weapon model for 1,000 HP frigates.
SRM direct hit: **2 PJ**. LRM proximity hit: **1 PJ**, half the SRM.
No changes to hit probability, launch cadence, magazines, range, screens,
laser PD, or interceptor effectiveness in this pass.

The motivating live salvo had 20 SRMs at about 0.19 AU: four laser-PD kills,
ten misses, six hits. Old 50 TJ strikes deposited only about 120 TJ against
a 450 TJ screen. Even twenty hits could not saturate a fresh screen. This
therefore required a substantial damage adjustment, not another accuracy tweak.

## Full-simulation calibration

`cargo build --release -p luminal-cli`

`target/release/luminal-cli --srm-salvo 50`

`target/release/luminal-cli --lrm-salvo 50`

Each case: one full 20-round salvo, seeds 1000–1049, healthy frigate with
raised screens, 2 Hz laser PD, either zero or thirty interceptors. No return
fire or beam damage. Both ships initially thrust together at 100g (boost and
damage effects remain live), exercising manoeuvre penalties. Received tracks,
terminal acquisition, ECM, cooling, damage control and light delay remain live.
These are controlled cases, not exact replays of the player's trajectory.
The harness waits for queued launches as well as airborne rounds, avoiding a
premature finish in a gap between LRM launches.

| Weapon | Range | Empty interceptors: killed | 30 interceptors: killed |
|---|---:|---:|---:|
| SRM | 0.10 AU | 50/50 | 19/50 |
| SRM | 0.19 AU | 50/50 | 8/50 |
| LRM | 0.10 AU | 50/50 | 10/50 |
| LRM | 1.00 AU | 42/50 | 6/50 |

All 400 trials completed. A stocked ship is substantially safer, not immune:
close SRM saturation remains dangerous. An empty interceptor magazine is now
a major vulnerability. These samples are not guarantees; acquisition, ECM,
evasion, existing damage and stochastic defence rolls still affect outcomes.

## Protected medium-range exchange

`target/release/luminal-cli --frigate-battle 20 30 1 10`

Twenty equal-frigate battles at 1 AU, stocked defences, ten-second reaction
offset: two battles had a ship loss (one on each side across the batch), and
mean final hull was 897/1,000. This preserves survivable medium-range exchanges.
The duel harness now excludes out-of-range payloads: its old SRM-at-1-AU
launches were impossible-to-hit decoys that artificially exhausted defences.
