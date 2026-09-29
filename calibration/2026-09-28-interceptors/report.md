# Interceptor guidance against powered SRMs — 2026-09-28

The last recorded battle (seed 1790656329186319555) had 19 SRM kills from
190 interceptor attempts, versus 54 LRM kills from 144 attempts. The summed
advertised probabilities were 141.12 and 53.55 respectively. These probabilities
apply only after physical contact: the SRM failures were dominated by guidance
missing the kill envelope, not an insufficient terminal roll.

Interceptors now fit acceleration from received track history, propagate it
through light delay, solve an accelerating launch rendezvous, and update the
lead from successive light-delayed seeker velocity measurements. No current
thrust or future target state is used. Correction acceleration and fuel, the
100 km contact requirement, and terminal probabilities are unchanged (nominal
75% against SRMs and 45% against LRMs, reduced at high relative speed).
The outcome log now includes miss distance to distinguish geometry from rolls.

## Reproduction and results

Baseline: clean commit `6bc82a1`, built in an isolated directory. After: the
interceptor guidance change accompanying this report. Both use the same seeds.

```sh
cargo test --workspace
cargo build --release -p luminal-cli -p luminal-app
./target/release/luminal-cli --srm-salvo 12
./target/release/luminal-cli --class-balance 3 cruiser stock .35 9000
```

Raw results: [salvos before](srm-before.csv), [salvos after](srm-after.csv),
[cruisers before](cruisers-before.csv), [cruisers after](cruisers-after.csv).

At 0.1 AU, across 12 salvo scenarios per configuration:

| Defender interceptor stock | Measure | Before | After |
| --- | --- | ---: | ---: |
| 0 | SRM hits | 131 | 131 |
| 0 | Defender losses | 11/12 | 11/12 |
| 30 | Interceptor kills | 0 | 88 |
| 30 | SRM hits | 123 | 79 |
| 30 | Defender losses | 11/12 | 0/12 |

The 0.19 AU CSV rows are outside the scenario's firing gate and have no salvos;
they are not evidence of successful interception.

Three stock Cruiser duels starting at 0.35 AU all finished with beams both
before and after. SRM hits fell from 25/24/23 to 0/0/0; total interceptor kills
rose from 62/66/69 to 110/102/111. Fresh, deep interceptor magazines now stop
these finite SRM waves very effectively. Depleted defenses still allow damaging
SRM salvos, as the stock-30 cases show. This small deterministic sample does not
establish a universal balance outcome; no additional probability buff was made.

Validation: 315 workspace tests passed, eight expensive surveys ignored. The new
controlled crossing-target regression covers sustained SRM-like acceleration at
0.03, 0.14 and 0.25 AU with 64 seeds each; it requires actual physical kills,
not merely favorable predicted odds. The native release client was rebuilt.
