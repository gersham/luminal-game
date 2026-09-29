# Weapon flavour validation — 2026-09-29

## Seeded duel regression

Compared the CLI built from previous commit `fbaba28` against the new build:

```sh
LUMINAL_DUEL_FIRE_RANGE=envelope <binary> --class-balance 4 all stock 0.35 52000
```

All 20 battles (four seeds per combat hull) produced byte-identical CSV output:
[before](before.csv), [after](after.csv). This includes outcome, duration,
missile expenditure, intercepts, damage and beam finishing statistics. Ordinary
solo doctrine remains in damage mode. This is evidence that the cosmetic fits
do not change these sampled battles, not a proof of all possible scenarios.

## Controlled projector survey

```sh
cargo test -p luminal-core projector_range_survey -- --ignored --nocapture
```

100 seeds per distance, stationary ships, precise initial received tracks, ECM
and screens off. Every shot uses the production beam emission and arrival path.

| Distance (light-seconds) | Successful disruptions / 100 |
| --- | --- |
| 1 | 100 |
| 3 | 100 |
| 5 | 99 |
| 6 | 88 |
| 6.1 | 0 (orders rejected) |

The projector works reliably at close range and loses reliability at its outer
boundary. Every successful application has the same capped 15% effect. These
figures are not estimates for moving, evading or poorly tracked targets.
Raw test output is in [projector-survey.txt](projector-survey.txt).

Behavioral tests cover causal arrival and expiry, recharge/energy/heat costs,
fit and damaged-system restrictions, jump lockout, range, occlusion, nonstacking
refresh, exact main-beam and spinal coupling reduction, preservation of shots
already in flight, and one-escort AI selection with a heavy ally.

Full mixed-fleet victory-rate and composition testing remains necessary when
fleet scenarios arrive. In particular, the 15% reduction costs a frigate's
whole damage shot; it should be chosen for protection, not assumed to be an
optimal damage trade. No new raw damage, magazine depth or launcher capacity
was added.

Final verification: `cargo test --release --workspace` passed 344 tests, with
nine opt-in surveys ignored. The projector survey above was run separately.
Release app and CLI built successfully. Native startup and in-game screenshots
were inspected for the class roles and the frigate mode control.
