# EF, sensing and system-controls playtest

**Historical duel results below:** these used the old EF/100 normalization.
They do not describe current combat balance after the sensing correction.

## Sensing correction

Sensing now scales by EF/1.4: the reference frigate is size 7, stealth 50%,
nominal full thrust, raised cold screens, ECM off. Passive identity/resolution/
approximate/bearing ranges are 0.01/0.1/2/10 AU. Ping identification has its own
1 AU reference range. Sensor damage and ECM still reduce range. Default ECM
raises signature by 1.5 but halves resolution/ping reach against ECCM 50,
giving that reference ship 0.075 AU passive resolution and 0.75 AU ping reach.

The current `--sensor-sweep` includes opposing ECM, nine distances and five
signature states. Verified: reference passive resolution at 0.1 AU; ping identity
at 1 AU but not 1.25 AU; bearing at 3 and 10 AU; damaged DF cannot reach 10 AU.
Boundary tests exercise just inside/outside each band, and twelve-seed echo
tests verify actual light-delayed in-range/out-of-range returns.

The transport has an independent visibility multiplier of 2; the station is
size 20. The multiplier scales EF and therefore all detection ranges, but does
not manufacture DF activity in a silent platform. The old duel trials below
have not been rerun after this correction.

2026-09-27. Current EF/100 sensing, ECM, boost and screen overload rules.
Three seeds per range (1000–1002), 30 interceptor rounds per frigate, stationary
equal ships, accurate initial fix, all offensive missiles queued, automatic
beams and 60-second pings. B reacts 10 seconds later. Two-hour simulated limit.
This is a controlled stress test, not the moving scenario's AI doctrine.

| Separation | Result across three duels |
| --- | --- |
| 0.002 AU (~1 LS) | Two knockouts at 50.6 and 88.8 minutes; one reached two hours with both alive. No opening-volley kills. |
| 0.03 AU (~15 LS) | All survived two hours. One ship ended at 685 HP; the other five at 1000 HP. 31–36 interceptor kills and 9–20 laser-PD kills per duel. |
| 1 AU | No damaging hits in any run. |
| 3 AU | No damaging hits in any run. |

At knife range, the losing ships still had 196 and 423 hull HP: system loss can
finish a ship before hull depletion. The two-hour survivor pair had 238/393 HP.
Three seeds are insufficient to conclude first-shot advantage is balanced.
CSV hit totals include beams, not just missiles, so they cannot establish the
desired 75% offensive-missile interception rate.

## Main balance concern

The explicit EF/100 scaling makes even a fully thrusting frigate with cold raised
screens and ECM have only EF 2.1 (without boost): approximate acquisition reaches
0.042 AU and resolution 0.0042 AU before ECM penalties. A hot fighting frigate
at EF 41.58 reaches 0.8316 AU acquisition; default opposing ECM/ECCM halves its
ping reach to 0.4158 AU. Quiet ships deliberately produce no DF bearing.

This supports hide-and-seek but leaves the intended 1 AU opportunistic band
ineffective in these tests. Recommend revisiting EF normalization or granting
active ranging a separate baseline before increasing weapon lethality. The
user-specified divisor and base ranges have **not** been silently changed.

## Reproduce

```sh
cargo run --release -p luminal-cli -- --sensor-sweep
cargo run --release -p luminal-cli -- --frigate-battle 3 30 0.002 10
cargo run --release -p luminal-cli -- --frigate-battle 3 30 0.03 10
cargo run --release -p luminal-cli -- --frigate-battle 3 30 1 10
cargo run --release -p luminal-cli -- --frigate-battle 3 30 3 10
```

The sensor sweep displays raw EF-derived bands; opposing ECM further reduces
resolution/identity/ping ranges. Sol scenery does not affect this isolated duel.
