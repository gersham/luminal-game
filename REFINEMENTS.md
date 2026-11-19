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

# Polish and optimization pass — 18 November 2026

## Simulation speed

No rule changed in this part: every doctrine survey reproduces its earlier outcome byte for byte. The work was removing repeated work.

- A beam director builds one firing solution per contact each second, and looks up its delivered picture once per ship. Before, each worth test propagated the same track again, for both pictures.
- Telemetry and platform snapshots no longer copy a ship's whole committed trajectory before discarding it. `known_body` lends the command ship instead of cloning it.
- Gravity computes each planet once per sample, so a moon does not recompute its parent.
- The release profile uses fat LTO and one codegen unit. The link is slower; the installed build is the one that benefits.

Measured on Base, one run at a time: Armada, both fleets on doctrine, 14 simulated hours, 131 s before and 26 s after. The core test suite went from 56 s to 10 s.

## Guidance and doctrine

These two do change outcomes, for both sides, so the surveys no longer match the earlier ones and `LUMINAL_LEGACY=both` does not reproduce them.

- **Range orders plan the stop around a slowing target.** `keep_range` planned its braking curve with the whole drive. A target that is itself braking takes part of the drive just to keep pace, so the ship fell behind the curve, passed at speed and spent hours returning. The plan now uses what is left, never less than a quarter of the drive. Only a target that is slowing counts: one accelerating head-on will brake for its own range, and planning as if it never will is an early stop. This applies to every range order, the player's STANDOFF and CLOSE included.
- **A clear run at the prize ends in a stop.** A raider with no escort in the way used to fly through an unscreened transport at tens of thousands of km/s. It now switches from the full-thrust run to a range order once the stop, at the thrust its radiators can sustain, needs the remaining distance. `Doctrine::legacy()` flies without the run-in, for comparison; the guidance change above is not gated.

Twelve Escort seeds, both sides on doctrine. Before, capped at 44 hours: the escort destroyer was lost in seven runs and the transport was destroyed in none. After, capped at 46: the destroyer was lost in five, the raider then withdrew in one, and of the other four the transport was destroyed inside the cap in two. A trace of a third shows the kill at 48 hours. The raider still often needs a second pass, because the full-thrust run aims at where the transport is, not where it will be. Lead pursuit in `flyby` is the next step.

Elsewhere: three seeds each of Raid, Convoy, Relief and Last Ship kept their winners, and one of three Hide and Seek seeds changed hands. Armada seed 1 has its first loss at the same minute as before (943) and the same winner, ending at 48.7 hours instead of 41.8.

Known and left alone: in Escort, once the player's ship and the transport are both lost the scenario never ends. `escort_and_convoy_continue_while_the_transport_lives` asserts exactly that, so it is a rule to decide, not a bug fixed here. The lunar station also decays into the Moon at about 39 hours.

## Graphics

All of it is presentation; nothing reads truth.

- A fixed starfield behind the plot, tinted per theme, built once per map size.
- Additive light (`fx.rs`): corona and limb-darkened photosphere on the star, limb shading and a sunward atmosphere on large planets, drive plumes, beam glow, and blooms for hits, bursts and jumps. Each effect is one mesh, replacing stacks of flat translucent circles.
- The route forecast is kept between frames and integrated again four times a second, or when thrust changes.
- A paused game redraws 20 times a second instead of every display frame, and at full rate while the camera is moving.

Paused frame cost, headless at 1920×1080 on the same scenes before and after: Escort 0.63 ms to 0.42 ms, Armada as spectator 2.3 ms to 1.8 ms, Armada as a faction 1.9 ms to 1.7 ms. The combat effects (beam glow, hit and burst blooms, jump blooms) were not captured in a screenshot; the star, planets, sky and drive plume were.

## Particles and sound

- **Particles** (`particles.rs`): a thin stream of exhaust motes behind a burning ship or round, embers from open radiators, light drawn in by a spooling jump drive, field light on an absorbed hit, sparks and embers on a penetrating one, and bursts for warheads, kills, point defence and jumps. Motes are anchored to what the view already shows and drift in screen pixels on the wall clock. At most 900 exist, drawn as one additive mesh; none is released while paused.
- **Sound**: the effects are resynthesized in stereo by `scripts/generate-sfx.py`, placed left to right by where they happened, varied slightly between plays, and joined by a drive loop that follows thrust. Details and the caveat that nothing was auditioned are in `assets/audio/README.md`.

## Tools

- `luminal-cli --trace <scenario> <seed> <hours> <every-min> [from-hour]` prints both sides' ships, contacts and orders at intervals.
- `LUMINAL_LEGACY=escort|raider|both` flies that side of a `--scenario-survey` on the previous doctrine.
- `cargo test --release -p luminal-app frame_cost -- --ignored --nocapture` times paused frames headless.
- `LUMINAL_ZOOM_KM=<km per pixel>` frames the command ship for a `LUMINAL_SCREENSHOT`, `LUMINAL_RUN=1` leaves the clock running for it, and `LUMINAL_DEMO_FX=1` stages one of each combat effect around the ship.

# Gameplay pass — 18 November 2026

The rules are in GAME_MECHANICS.md under "Fleet orders, deception, aimed fire, terrain and events". This is what was built and what it did to the scenarios.

## What was added

- **Fleet orders.** ENGAGE (a named track, standing), WEAPONS HOLD and EMISSION SILENCE join SCREEN, CLOSE and WEAPONS FREE. One order per ship, at light speed, each replacing the last posture. The six buttons share the one row on the helm.
- **Decoys.** Two per combatant, `D` or the DECOY button beside PING. The enemy's track on the ship passes to the decoy.
- **Aim points.** The fourth button on the beam card steps through HULL, DRIVE, SENSORS, WEAPONS and HEAT.
- **Terrain.** Giant planets mask passive looks at whatever is in front of them, debris fields clutter active returns, and a stellar flare dazzles passive arrays as its front passes. Debris is drawn on the plot; a flare is announced when its light reaches the command ship.
- **Arranged events.** Flares, drive casualties and reinforcements, scheduled per scenario from the seed. Reports appear in the combat log when they arrive.

## What the AI uses

- It lays beams on the drive of a ship faster than itself, and on the hull of anything else.
- A quarry that cuts its drive to go dark releases a decoy first.
- It ignores a track it has seen exposed as a decoy.
- It does not use fleet orders (its ships are each flown by doctrine), the heat aim, or decoys in any other situation. Those are the player's.

`Doctrine::legacy()` does none of the above. Terrain and events apply to everyone and are not gated.

## Known limits

- The beam card's expected-energy figure and the automatic fire gate do not allow for the halving of an aimed pulse. The spinal mount ignores the aim point.
- The AI's drive aim rarely applies as tuned: a ship with missiles left holds range well outside the 3 light-seconds an aimed pulse needs.
- Nothing on the plot tells the player that a contact is being degraded by a giant or by debris. The flare has a banner; the other two do not.
- A reinforcement changes no victory condition. In Relief the game still ends when the original raider is dealt with, whatever the newcomer is doing.
- A reinforcement on a side with no track sits and waits, as any doctrine ship with no track does.
- A round retargeted to a decoy is retargeted at the moment of release.

## Effect on the scenarios

Each mechanic was landed inert first: with no AI use and no scenario content, the doctrine surveys reproduced the previous build exactly. Terrain, events and AI use were then switched on. Same seeds, both sides on doctrine, before and after:

| Scenario | Before | After |
|---|---|---|
| Escort, 12 seeds, 46 h cap | escort 8, unresolved 4 | escort 8, unresolved 4 |
| Raid, 3 seeds | raider 3 | raider 3 |
| Hide and Seek, 3 seeds | escort 1, raider 2 | escort 2, raider 1 |
| Convoy, 3 seeds | escort 3 | escort 3 |
| Relief, 3 seeds | escort 3 | escort 3, about four to five hours later in the two seeds where a frigate jumps in |
| Last Ship, 3 seeds | escort 3 | escort 3 |
| Armada, seed 1 | raider at 48.7 h, 19 losses | raider at 56.2 h, 19 losses |

Totals hide movement in individual seeds. A trace of Armada seed 1 shows the relief destroyer arriving at hour 18 for the side with fewer ships and being flown by doctrine onto a target. A trace of Relief seed 1 shows the frigate arriving at hour 6.6 and waiting with no track. Armada still runs 14 simulated hours in about 24 s, so terrain costs nothing measurable.

None of this has been played by a person. The numbers are placeholders chosen to be noticeable, and the event table in particular changes how hard each scenario is.


# Graphics pass two — 18 November 2026

Presentation only. The one core edit is a refactor, and the two doctrine surveys re-run after it (Hide and Seek, Last Ship) match the previous build exactly. A paused frame costs what it did (Escort 0.44 ms before, 0.46 ms after; Armada as spectator 3.43 and 3.47).

## Renderer and bloom

The client now runs on OpenGL so that a bloom pass can read what egui has drawn. `bloom.rs` runs as a paint callback after the plot's symbols and before its labels: it copies the plot, keeps the excess above a luminance knee at quarter size, blurs it and adds it back. Labels, readouts and the command deck are drawn afterward and are untouched.

- Without bloom, OpenGL and the previous wgpu renderer draw the star scene to within 0.4% RMS of each other, two pixels apart beyond a 2% tolerance.
- `LUMINAL_RENDERER=wgpu` runs the previous renderer, without bloom. If OpenGL fails to start, the game starts itself again that way.
- `LUMINAL_BLOOM=0`, or the BLOOM button under QUIET DETECTIONS, turns the pass off.
- The pass lines up with the plot at display scales 1 and 1.5, on Base's Intel graphics only. GPU time is not measured; the frame benchmark runs headless and paused, so it sees neither the pass nor trails being sampled.
- Orbit names and waypoint numbers are drawn before the pass. They are dim enough not to bloom; the two banners and all labels are drawn after it.
- The automatic restart on wgpu has not been exercised. Only `LUMINAL_RENDERER=wgpu` has.

Separately from bloom: the command deck is laid out in fixed pixel widths. It is clean at 1600 logical pixels wide and the system-mode labels collide at 1440. That was true before this pass.

## On the plot

- **Sky.** Three star layers that slide slightly with a pan, never with a zoom, over a per-theme nebula.
- **Planets.** They turn with the game clock (a day for a rocky world, ten hours for a giant; moons keep one face in). The home world shows settlement lights on its night side. Some giants carry rings. The surface tiles do not wrap, so the map is mirrored each half turn and the surface appears to turn back. Cloud shadows are not done: they need art that does not exist.
- **Trails.** Ships, rounds and contacts leave a fading trail of where they have been drawn, sampled on the wall clock. A contact's trail is the trail of its believed position. Trails break on a jump or a large track correction.
- **Ping.** The command ship's ping is a front with depth behind it. A contact glints when an echo from it has actually arrived. Overlapping fleet pings share their light.
- **Flare.** While a flare's light is on the command ship, the plot is washed from the star's side.
- **Weapons.** Our own latest pulse is drawn as it was laid: wide and amber for heat, a fine white line on a system group. Raised screens shimmer on the command ship, and on the inspected target as of its last echo report.
- **Terrain.** The zone a giant masks from the command ship's passive sensors is drawn as a faint wedge toward it. Debris fields have a few larger turning rocks, and three specks mark a field too small to draw.
- **Camera.** A hit that gets through the screens jolts the plot, capped so a burst is one jolt. A jump landing on the plot breathes the scale. Scroll zoom was already eased by egui and is unchanged.
- **HUD.** Status bars and the thrust gauge ease to new values, a system chip lights when it takes a casualty, and a button's label sinks with the press. Animations are keyed by ship, so a new target snaps to its values.

`LUMINAL_SCALE=<pixels per point>` draws at a given display scale for a screenshot.
