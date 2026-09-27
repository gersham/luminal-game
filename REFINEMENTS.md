# Tactical refinement pass — 27 September 2026

## Current play rules

The player commands **one ship**, the frigate in Transport Intercept. Inspecting another platform does not transfer command. Transport, missiles, probes and the new lunar sensor station are autonomous. The assigned command ship does not transfer to another ally when destroyed.

The lunar station is a small square, initially in a 5,000 km-radius lunar orbit. It has sensors, no weapons or screens, and automatically pings every 60 simulation seconds. Its pulse circles are hidden. Sensors on allied ships, stations, probes and missiles send observations to the command ship at light speed, with occlusion and a separate relay leg after sensing. A remote observation cannot improve the player's picture before its message arrives.

Long-range sensing is deliberately imperfect. Sensor/contact calibration biases persist between observations instead of re-rolling the map marker each frame. Covariance retains a systematic-error floor; measurement weighting includes that error. Older, out-of-order reports remain in the observation log but do not rewind the tracking filter. The map's uncertainty circle uses position covariance, above the 60-light-second display threshold. It is an estimate, not a guarantee that truth lies inside it. Unique source association is an established gameplay assumption: one persistent track per source, with a new ping replacing its previous indication. While a positioned ping envelope is visible, the separate covariance ring is suppressed.

Hostile pings become orange indications only after their light and any relay reach the command ship. No hostile outgoing pulse is drawn. Each indication fades over ten minutes; its possible manoeuvre radius grows at 100g from the estimated emission state. A bearing-only report does not invent a range.

Missiles obtain their own noisy local range/velocity solutions inside three light-seconds. All payloads can make terminal lateral corrections within acceleration and remaining delta-v limits. Laser missiles accumulate several local samples before firing. A late fix can demand more correction than the missile can physically make; the flight list warns about that estimate. Misses retire from the live game, while historical trajectories remain for light-delay physics.

Combatants carry three provisional reconnaissance probes. A probe burns at 500g on its launch heading for ten minutes, then coasts, pinging every five minutes. It has weaker sensors and relays reports at light speed. These inventory/endurance/sensitivity values need playtesting.

## Other refinements implemented

- Combat pulse, detonation, impact and loss feedback; foreign reports use noisy sensing rather than exact foreign positions.
- Shared one-per-second missile launch queue, reservation counts and cancellation.
- Fresh/stale/lost contact states and fire-control holds; automatic beams pause for stale tracks, range, recharge or power/heat limits.
- Delayed allied telemetry, remote order transport and received-picture caches for autonomous platforms.
- Ship capacitor recharge, beam heating, screen buildup/collapse, thermal radiation and screen glare. Reservoir accounting has conservation regression tests; this is not a complete universe-wide energy/momentum ledger.
- A basic opponent doctrine operating on the same faction view: sensing, approach, salvo timing, screens, beams and reconnaissance probes.
- Reproducible headless weapon trials (`cargo run --release -p luminal-cli -- --survey 5`). CSV snapshots live under `calibration/`.

## Remaining limits

Scenario balance is provisional. Earlier tests favored the escort; adding a station changes that balance further. The doctrine is a first implementation, not a sophisticated adversary. The survey starts with a supplied exact track to isolate weapon behavior; cold/coasting versus 10g-jinking cases also differ in brightness. Small deterministic samples are regressions, not validated hit probabilities.

Public scenario outcomes and some order/collision notifications still have simplified timing. Nuclear damage coupling and some combat effects are approximations. There is no multiplayer transport or complete replay implementation yet. Historical design sections in other documents describe ambitions as well as implementation; this file records this pass's current scope.
