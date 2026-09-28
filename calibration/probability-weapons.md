# Probability-based weapons validation — 2026-09-27

Historical baseline: these simulation results predate the subsequent SRM
range increase from 0.1 to 0.2 AU. The new limit and launch availability at
0.14/0.2 AU are covered by regression tests; this table is not a rerun of
the expanded envelope.

Missiles now make one terminal probability roll after an animated flight;
interceptors make one defence roll. Neither uses the old closest-pass/fuel
steering solver. Destroyed targets retire their weapons, and misses cannot
come around for a second pass. Local terminal sensor fixes still arrive at
light speed and improve acquisition. Beams retain their existing model.

## Reproduction

```
cargo test --workspace
cargo build --release -p luminal-cli
target/release/luminal-cli --missile-balance 20
target/release/luminal-cli --survey 10
target/release/luminal-cli --frigate-battle 10 30 0.002 10
target/release/luminal-cli --frigate-battle 10 30 0.05 10
target/release/luminal-cli --frigate-battle 10 30 1 10
```

Seeds begin at 1000. The missile grid has 480 trials: two payloads, four
ranges, three initial errors (0, 0.5 and 5 light-seconds), twenty seeds.
The moving-target survey adds 320 trials over closure/evasion combinations.
All 800 terminate. Both payloads correctly score zero beyond their range.

| Range | LRM hits / 60 | SRM hits / 60 |
|---|---:|---:|
| 0.03 AU | 43 | 33 |
| 0.1 AU | 52 | 7 |
| 1 AU | 39 | 0 |
| 2.5 AU | 0 | 0 |

These aggregate uncertain and accurate shots, so they are not a pure
range-probability curve: longer LRM flight can permit more terminal sensing.
Closest-distance fields in the legacy CSV are animation diagnostics, not
hit criteria. Sample sizes are useful regression checks, not precise balance
estimates.

Ten equal-frigate battles per range use stationary ships, thirty interceptors
per ship, full offensive magazines, automatic beams, and a ten-second firing
reaction offset. At 0.002 AU (inside the interceptor minimum distance), ten
battles produced ten ship losses and an average first loss at 560 simulated
seconds, with 12.9 damaged/destroyed systems across the pair at the end.
At 0.05 AU, one of twenty ships was lost; average remaining hull was 959/938,
with 356 interceptor kills versus 17 laser-PD kills over the ten battles.
Thus interceptor missiles remain the primary defence outside knife range.
At 1 AU, all twenty ships survived with full hull; defences scored 320
interceptor kills and 25 laser-PD kills across the ten battles.

No blanket damage buff was needed. AI now projects velocity uncertainty to
arrival time and requires a resolved track before salvos; players retain
speculative LRM launches. Hard range limits prevent far-away probability
hits, and flight deadlines never precede light travel. This does not establish
perfect campaign balance; moving duels and human tactics remain playtest work.
