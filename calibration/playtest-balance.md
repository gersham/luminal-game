# September 27 playtest balance

Subsequent changes: independent SRM (5-second) and LRM (60-second) launchers,
direct missile-button firing, increased warship hull, and double component-hit
weight for propulsion/power. The measurements below used a shared 20-second
launcher and the old hull; they must not be presented as current balance results.

Current authoritative samples are `playtest-balanced.csv` and
`playtest-balanced-beams.csv`. Other CSVs are intermediate parameter sweeps;
`expanded-medium-final.csv` was stopped early and is not a final validation.

## Final fit

- 20 LRM and 20 SRM per frigate, shared 20-second offensive launch interval.
- LRM: 1,500 g, 29,979 km/s delta-v, nuclear proximity, 75 TJ maximum hit.
- SRM: 3,000 g, 14,989.5 km/s delta-v, direct 1 kg shotgun impact.
- 30 interceptors per ship, 20 on station; 0.27 AU outer gate, 60% base terminal
  kill probability. No launches inside 5 ls or against defensive interceptors.
- PD laser: 2 Hz frigates, 1 ls maximum, 0.006 ls half-probability range.
- Missile screen heat coupling 40%; screen puncture chance 17.5%, leaking 1%
  energy and causing a component casualty. Unscreened hit energy is unchanged.
- Main beam: 75 TJ, 10-second minimum cycle; capacitor/reactor/thermal limits
  scaled together to retain the previous cadence while halving pulse damage.

## Measurements

Five stationary equal-frigate full-salvo duels at 0.03 AU (400 offensive missiles):

- 319 intercepted: **79.75%**, comprising 166 interceptor and 153 laser kills.
- 8.1 hits per ship on average; **49.44%** average peak screen heat.
- **1.2** damaged/destroyed systems per ship at the end; no ship destroyed.
- Individual damage varies; one ship suffered four system casualties. This is
  a statistical tuning target, not a scripted limit on damage.

Three separate 0.002 AU sustained battles, second ship returning fire after ten
seconds: first knockout at 918, 963 and 1,230 simulated seconds (15.3–20.5 minutes).
Both ships returned sustained fire; no opening-shot knockout. At this distance
interceptors correctly remain inhibited and lasers supply missile defence.

These are small deterministic seed samples, with stationary targets and seeded
initial fixes. The close-beam benchmark is separate from the missile benchmark,
not a continuous approach scenario. Manoeuvring, uncertain tracks, faction RNG
ordering, component cascades and player tactics can change outcomes materially.
Earlier medium-range sweeps were more defensive; full long-range balance is not
claimed by this pass.

## Reproduction

```
cargo build --release -p luminal-cli
target/release/luminal-cli --frigate-duel 5 30 0.03
target/release/luminal-cli --frigate-battle 3 30 0.002 10
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

The UI missile chance bar is a received-track delivery heuristic, before enemy
defence. It includes positional/velocity uncertainty, terminal correction and
endurance; it is not a separately Monte Carlo calibrated hit probability.
