# Endgame decisions, withdrawal and jump presentation

The five accepted pacing changes are implemented: capability-aware AI, explicit
surrender/withdrawal outcomes, a next-tactical-event control, repair goals, and
vulnerable jump escape. These build on the existing capital magazine correction;
missile flight envelopes and beam damage are unchanged.

## Behavior

AI evaluates usable or repairable weapons, ammunition, propulsion, power and
jump capability from its own received view. An impossible combat plan, permanent
immobility outside useful beam range, damaged power, or hull below 45% commits
it to survival. It prefers a working jump drive, repairs a recoverable escape
system while attempting to open range, and surrenders if escape cannot be
recovered (or hull falls below 20% while waiting for repairs). Healthy ships
continue fighting. Missing enemy contact alone is not evidence of defeat.

Withdrawal uses the normal 600-second spool. It disables thrust, evasion,
screens and lasers, generates heat, and can be cancelled or interrupted by
damage. Starting the spool does not win or remove anything. Successful departure
removes the ship from the battle without destroying it, with no return event.
Surrender removes the ship immediately. Both concede the objective; a departing
player loses, and a departing raider yields an escort victory. The player can
choose Withdraw or explicitly confirm Surrender in the helm.

Withdrawal announcements, interruptions and remote outcomes travel at light
speed. A received withdrawal notice adds a flashing triangle and countdown.
The AI does not get access to hidden enemy damage for these decisions.

Repair choices are Automatic, Fight and Escape. Power and damage control remain
first priorities. Automatic then favors propulsion, Fight favors weapons, and
Escape favors jump capability. Changing the goal preserves the current repair's
progress. Destroyed systems remain irreparable. The ship panel shows the goal
and current repair ETA.

Next Tactical Event runs the ordinary simulation at high warp and pauses on a
received alert, contact change, combat report, completed repair, jump transition,
useful beam-range crossing or mission outcome. It restores the prior warp and
has a 24-hour simulated limit. Stop Advancing and manual ship orders cancel it.
It does not skip physical travel, heat, damage or light delay.

## Jump effects

A pulsing blue halo, concentric rings and orbiting sparks persist throughout
spooling, building in size and intensity. The spool effect ends on cancellation,
interruption or departure. Large blue blooms and expanding rings appear at both
normal-jump endpoints. Each endpoint burst lasts four real seconds, independent
of warp or pause; withdrawal produces only the departure burst. Foreign effects
use received sensor reports. Late pre-jump light cannot restart an already
observed completed spool. Effects remain available during heavy missile traffic.

Native screenshots: [spooling](jump-spooling.png),
[departure and arrival endpoints](jump-endpoints.png).

## Validation and limits

Release workspace regression coverage includes interrupted and cancelled spools,
no stale withdrawal/arrival event, surrender ownership, preserved repair progress,
causal outcome delivery, causal jump endpoints, real-time burst lifetime, UI
buttons, and a combat-incapable bot actually withdrawing to resolve its mission.
The final test totals are recorded in `validation.txt`.

The [20 mission trials](missions.csv) exercise real Doctrine and transport
objectives across five classes and four seeds per class, for up to 12 simulated
hours. Six finished: five surrenders and one successful Cruiser withdrawal.
Fourteen remained unfinished at the cutoff. Reported times are hourly sampling
checkpoints, not exact event timestamps. Raw transcripts are in `missions/`.

This demonstrates working non-destructive endings, including an actual combat
withdrawal. It does not establish a win rate or prove every pursuit will finish
quickly. The remaining long encounters still require encounter/navigation pacing
work; next-event advance makes waiting manageable without inventing a victory.
No additional weapon damage or range changes were made to force shorter fights.

## Reproduction

```sh
cargo test --release --workspace
cargo build --locked --release -p luminal-app -p luminal-cli
python3 calibration/2026-09-29-endgame/run_missions.py
LUMINAL_SHIP=Destroyer LUMINAL_ORDERS='jump:1:2:0' LUMINAL_ADVANCE=300 LUMINAL_SCREENSHOT=/tmp/jump-spool.ppm ./target/release/luminal-app
LUMINAL_SHIP=Destroyer LUMINAL_ORDERS='jump:1:2:0' LUMINAL_ADVANCE=605 LUMINAL_SCREENSHOT=/tmp/jump-endpoints.ppm ./target/release/luminal-app
```
