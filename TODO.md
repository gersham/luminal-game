# Luminal — handoff for the next session

Read [GAME_MECHANICS.md](GAME_MECHANICS.md) (rules authority) and [ARCHITECTURE.md](ARCHITECTURE.md) (structure authority) first. This file is the working state at handoff.

## Latest: single command ship, lunar station and tactical refinements

See [REFINEMENTS.md](REFINEMENTS.md) for current implemented behavior and remaining limitations. The frigate alone accepts player orders; the lunar sensor station is autonomous, unarmed and unscreened, pinging every minute without visible pulse rings. Allied platform observations relay at light speed. Persistent sensor errors, covariance-sized uncertainty, local missile solutions and reconnaissance probes now affect simulation, not just rendering. Balance remains provisional.

## 2026-09-27 sensor and movement revision (historical progress below)

- Beam engagement is a standing target order, automatically pulsing every 10 seconds when the received track is within a provisional 3-light-second envelope; Cease fire cancels future shots, not already emitted light. Missile delta-v is now doubled to 59,958 km/s (about 68 minutes at 1,500g).

- Active sensing is a one-shot Ping command, never a toggle. Own emission rings remain anchored at emission, expand at c/2 (round-trip range), and fade over 0.7–1 AU. Actual outbound signals still travel at c. Inspection no longer replaces the selected command ship. Missile acceleration increased from 1,000g to 1,500g, with unchanged delta-v.

- Transport is scenario-controlled (`controllable = false`), automatically navigates to the escape region, and cannot receive player orders through the session or UI. The frigate is selected on startup. Regression verifies the transport reaches the objective unaided.
- Full-simulation salvo regression reproduced nine nuclear missiles flying on after the lead warhead killed the target. Fuses now also detect the pass against the received track, without consulting target death, so remaining warheads detonate. All missile types retire after a completed pass even with fuel remaining. Laser payloads may fire during the initial burn; previously they could pass the target with their firing logic disabled. Historical trajectories remain for light-delay physics, but spent missiles are absent from live body views. Validation: 84 tests passing, 3 manual surveys ignored, strict clippy and build clean.

- Armed scenario ships now carry 10 kinetic, 10 nuclear and 10 laser missiles each. Repeated fire clicks reserve ammunition and enqueue shots in click order, with one shared launcher per ship firing at most once per simulation second across all payloads. The UI shows available stock and queued counts; a queued shot whose track disappears is cancelled without consuming its reserved round. Removed the extra velocity arrow extending from the front of resolved contact markers. Validation: 80 passing tests and strict clippy clean.

- Latest display rule: uncertainty rings appear only at **60 light-seconds or farther from the nearest friendly ship**, using the estimated contact position. This replaces the earlier 8-pixel precision gate; zoom and missile positions do not affect eligibility. Rings still grow with age to their existing cap.

- Nuclear fuses now burst on their first closest pass regardless of whether that pass is inside the damage radius. The radius still limits damage. Interval minima and exact update-boundary passes are covered; still-closing missiles do not burst early. Active sensors resolve both starting-scenario targets in a 1200-second regression. Resolved contacts now show a hollow ship arrow oriented along estimated velocity; direction-only contacts retain bearing lines. The age ring is hidden when the projected 2-sigma position uncertainty fits within the marker (8 px), otherwise grows with age to its cap. Contact UI explicitly distinguishes TRACK from DIRECTION ONLY.

- Ship beam emitters are now implemented: `Command::FireBeam` on armed ships, independent of remaining missiles, with tracked-contact Beam buttons, recharge status and a fading own firing-solution line. Placeholder pulse energy 15 TJ, recharge 10 s, divergence 0.3 microradian and pointing jitter 0.1 microradian. Aim is predicted solely from faction tracks; pulses survive shooter destruction, propagate at c, obey occlusion, and couple decreasing energy as their spot spreads. Received energy uses the existing screen/hull model; shooter telemetry records emitted energy. Reactor budgets, waste heat, beam emission detection and a complete energy ledger remain future work. No automatic fire or enemy hit confirmation is added.
- Missiles are autonomous: excluded from the active ships list and map selection; the session rejects all player commands to missile bodies with `NotControllable`. Their map rendering remains. Updated validation: 75 tests pass, 3 manual surveys ignored, strict workspace clippy passes; desktop firing/recharge and missile-list exclusion visually verified.

- Three channels: passive localization ~0.1 AU, active ranging ~1 AU, direction-only ~10 AU. Calibration reference is a 10g-burning ship, with 75% acquisition per frame/return at those ranges. Passive ranging is an explicitly tagged TL7 sensor proposal with noisy range and bearing; no exact positions or velocities enter the faction view. Direction-only contacts still require triangulation for a track.
- Ping emissions snapshot the source signature and expose it at 10× passive/localization and direction-finding range. Ping, echo, and relay delays/occlusion still apply, including after sensors turn off. Future hull size/stealth tuning should change emitted signature and reflecting cross-section, not add separate detection rolls.
- Ship right-click orders now use `Flyby`, burning to the earliest predicted encounter without braking and coasting after passage. The side-panel “Intercept and match velocity” checkbox switches to/from the existing rendezvous order. Point stops and celestial orbits are unchanged.
- Contact map labels omit sensor age. A separate age ring grows from 8 to 40 px over one hour and stays capped; covariance remains uncapped in the simulation and visible in details.
- Regression coverage includes single-ship passive tracking, nominal channel probabilities and 10× exposure, causal ping/echo arrival after shutdown, flyby passage and switching to velocity matching. The older bearing-only and short-range echo tests were updated to the new requirements.
- Corrected the same standing-thrust bug in `closing_trial` that was already fixed in `range_trial`: use `World::set_thrust`, so guidance does not overwrite the intended jinks. Missile hit percentages remain prototype calibration targets, not verified promises.
- Earlier sensor validation: 69 tests passed. Desktop screenshot verified the flyby status/toggle and age ring. Post-change survey (`LUMINAL_SURVEY_N=20 LUMINAL_SURVEY_AT=1 cargo test --release -p luminal-core range_survey -- --ignored --nocapture`): kinetic 20/20 at 0.01 AU, nuclear 5/20 at 1 AU, laser 3/20 at 10 AU. These do not meet the older 50% goals; missile tuning remains outstanding.

## Decisions made with the user (settled)

- **Modern, post-WWII Royal Navy terminology is a project principle** (2026-09-27), with contemporary usage preferred. See GAME_MECHANICS.md for the authoritative rule. Player-facing track designators are `Track 1`, `Track 2`, etc.; tracking quality and identification remain separate. Verify modern naval usage and document space-specific adaptations; avoid obsolete period language.

- Top-down **2D** real-time **strategy** game, native **Linux** desktop app. Rust workspace; client is **eframe/egui 0.36** (chosen over Bevy for fast builds and panel-heavy UI; the sim core has no renderer dependency, so this is swappable).
- **Vector graphics.** Ships are little arrows with velocity-vector tails. Planets, suns and other objects come later.
- **Map scale is a solar system, ~10–50 AU**, zoomable down to km.
- Local play is for exploring the idea; if it works it goes **internet multiplayer**. Therefore: **server-authoritative sim; clients only ever receive their faction's causally received `View`**; no lockstep (it would ship truth to clients and break the light-speed fog). `luminal-core::session` is the client/network boundary; local play runs it in-process.
- The player is a `Mind` like a bot — the player's display is built from their faction's perception only. Omniscient **spectator** role for local exploration, AI-vs-AI and replays.
- Unresolved GAME_MECHANICS §15 values: the agent picks physically consistent **placeholders**, all in `crates/luminal-core/src/params.rs`, each tagged `Established` / `Proposal` / `Placeholder`, and lists them for the user to review.
- First playable milestone: the **§15 experiment** (cruiser intercepting a transport before a departure region, defending frigate, variants: passive/active sensing, probes/no probes, payload choices).
- The Asterion designer at `~/Sources/personal/starship-construction` (on the arch box) is **inspiration only**; this game has its own architecture.
- **Scenario speeds are planetary-scale**: ships start within a few km/s of the local orbital motion, not at a percent of c (user, 2026-09-26).
- **Celestial bodies**: the scenario has a sun, a planet and a moon. They **have gravity** and **contact is fatal** (user, 2026-09-26).
- **Right-click orders** (user, 2026-09-26): on a planet/moon/star → settle into a convenient orbit; on a ship or tracked contact → intercept it; on empty space → thrust toward the point at the manual setting.
- **Right-click on empty space = fly there in minimal time and stop** (burn, flip over half way, brake), not a constant-thrust heading (user, 2026-09-26). The point moves with the celestial body whose sphere of influence contains it, and the ship holds there against gravity. A per-ship drive limit caps thrust for all orders (default: ship maximum); All stop brakes to rest in the local frame.
- **Orbit and intercept auto-throttle** (user, 2026-09-26): they choose their own thrust up to the ship maximum. Intercept is implemented as close, match velocity and hold station (1,000 km standoff, placeholder).
- **Automatic collision avoidance** (user, 2026-09-26): ships adjust their vector to miss celestial bodies unless it is impossible.
- **Celestial bodies cast sensor shadows**: nothing sees through them (user, 2026-09-26). Applies to passive light, pings, echoes and laser relays.

## Done

- `crates/luminal-core` (58 tests, clippy clean):
  - `units`, `params` — tagged constants. New sensor/signature/tracker values are all `Placeholder` (listed below).
  - `kinematics` — append-only constant-acceleration `Trajectory`; each segment records total accel **and** thrust; trajectories can `terminate` (destroyed body). History cannot be rewritten.
  - `celestial` — `System` of on-rails bodies (fixed star, circular orbits), gravity, adaptive step size (1 % of local free-fall time, 0.5–60 s), midpoint gravity sampling, surface-impact test, **occlusion test for light paths** (body motion linearised over the transit), display forecast.
  - `lightcone` — `retarded_state` (handles destroyed sources: their last light still travels) and `Front::arrival` for discrete emissions.
  - `scheduler` — deterministic event queue (time, then insertion order). `rng` — seeded SplitMix64 streams.
  - `sensors` — isotropic emission (cold + per-g drive), 1/r² passive intensity, radar-style 1/r⁴ echo, noisy bearing / range measurements.
  - `mind` — `Observation` with emission / sensor-receipt / decider-receipt times, sensor origin, SNR and source (emission, echo, their ping); **6-state constant-acceleration EKF tracks** with known gravity in prediction; two-sensor triangulation to start a track; `Perception` per faction. Tracker lives in core (not `luminal-bots`) because the session builds the player's view from it.
  - `world` — event-driven: per-body gravity steps with impact → `Loss`; global sensor frames (10 s) that resolve passive light, pings reaching bodies (echo + target sees the pinger), echoes returning, and **reports relayed at c to the faction's flagship** (lowest-numbered live ship). Every path is occlusion-checked; a blocked laser relay loses the report. Scenario pre-history (1 h, integrated backwards) so light is already in flight at T+0. Deterministic across warp (tested).
  - `session` — faction views are built **only** from `Perception` (own ships exact, contacts as tracks or bearing lines, no identities). The old placeholder observer is gone. Spectator gets truth and may ask for contact→body associations. Commands: thrust, active sensor on/off.
  - `scenario::transport_intercept` — Sun, Planet (1 AU), Moon; Transport and Frigate leaving the planet at 1 g, 1 ls apart; Cruiser ~0.32 AU out burning 20 g toward them.
- `crates/luminal-app` — verified on the real Hyprland display (screenshot hook works): celestials at true size with minimum dot, orbit lines, off-screen edge markers; **sensor-shadow cones from the selected ship**; forecasts under gravity with ✖ impact marker and an impact warning in the panel; contacts as hollow arrows with 2σ ellipses and forecast, or bearing lines; spectator belief overlay; auto-fit (F), Space to pause; label declutter; active-sensor toggle, retro-burn button, losses list.
- Orders (`autopilot` module + `world::guide`, re-planned every sensor frame): orbit (radius = current distance clamped between 1.5× surface radius and min(0.3 Hill radius, 0.6× innermost moon orbit); keeps current sense), rendezvous-intercept (braking-limited closing speed, velocity match, standoff), collision avoidance (1 h forecast under gravity; smallest deviation by thrust then direction; full 100 g if needed; cheap reachability bound first). The orbit law is exempt from avoidance. Enemy intercepts steer by the faction's **track**, never truth; bearing-only contacts are refused (`NoTrack`). UI: dashed target orbit / intercept line, avoidance ring (orange, red if impossible), autopilot status and ETA. Dev hook `LUMINAL_ORDERS` / `LUMINAL_ORDERS_AT`.
- Slingshots: modelled implicitly by gravity, but worth at most ~2× a body's orbital speed (~60 km/s at the planet), which a 100 g ship gains in about a minute; they matter only once propellant budgets exist.
- Missiles (`missile` module + `world::guide_missile`): launch needs a track and a magazine (Cruiser 12, Frigate 6, Transport 0). Inherit launcher velocity; 600 km/s delta-v; burn at 1,000 g along a constant-thrust intercept for 60 % of it, cruise with zero-effort-miss corrections keeping a 30 % reserve, terminal inside 60 s to go on the missile's own passive seeker with proportional navigation, re-planning down to 2 ms. Kinetic hits within the ship radius (100 m), nuclear within 10 km, laser fires at 10,000 km and its beam is resolved when the light reaches the target. No screens yet: an effective hit destroys the ship. Missiles are visible while burning (a full burn shines like a ship at 1 g) and go dark when coasting. UI: payload choice and Fire buttons per tracked contact; missiles drawn as small darts with phase and delta-v.
- Objective and alerts: departure region, win/loss banner, alert-driven warp drop (see Next steps).
- `crates/luminal-cli` — headless runner: `cargo run --release -p luminal-cli -- <hours> <report-minutes>` prints each faction's contacts with track error vs 2σ, and sim speed (~600,000× real time in release).
- Dev profile optimises dependencies and `luminal-core` so `cargo run` stays smooth at 100,000× warp.

### Placeholder values to review (all in `params.rs`)

missile delta-v 600 km/s · burn 60 % · terminal reserve 30 % · terminal at 60 s to go · seeker noise floor 1e-11 W/m² · missile emission 2.5e3 W cold + 2.5e8 W per g · magazines 12 / 6 · ship radius 100 m · nuclear lethal radius 10 km · laser standoff 10,000 km · intercept standoff 1,000 km · sensor frame 10 s · cold emission 2.5e4 W · drive emission 2.5e9 W per g · passive noise floor 1e-13 W/m² · detection SNR 3 · passive bearing σ 0.03 rad at SNR 1 (echo 1e-4, seeker 1e-4) · ping power 1e12 W · cross-section 1e4 m² · echo noise floor 1e-24 W/m² · range σ 1 km at SNR 1 · tracker manoeuvre 1 g per minute. With these: a cold ship is seen half the time at ~0.27 ls, and a 20 g burner at ~24 ls (detection is P = SNR / (SNR + 3) per frame, a long tail: ~1 % a frame at 10× the 50 % range), a burning one is seen across AU, echoes reach ~10 ls, and a ping is visible for several AU.

## Next steps, in order

1. ~~First commit~~ — done 2026-09-26 with the user's approval. Feature work continues on branches.
2. ~~Warp auto-drop~~ — done: alerts (new contact, own ship lost, collision warning/unavoidable, order complete, game over) stop the clock at the event and drop warp to 1×; game over pauses. "Contact lost" is not yet an alert.
3. Orders to ships other than the flagship should travel at c; today they and the autopilot's use of flagship perception are instant (flagged `PLACEHOLDER` in `world::sensor_frame`).
4. Own-loss news by light: a faction currently learns of its own losses instantly (flagged `PLACEHOLDER` in `session::View::losses`). Also impact flashes as bright emissions.
5. Bearing-only track initiation for a single ship (bearing-rate / own-manoeuvre, or a range-parameterised filter bank). Today a lone Cruiser never forms a track without pinging, which may be the desired pressure — confirm with the user.
6. Data association: contacts are perfectly associated (flagged `PLACEHOLDER` in `world::association`).
7. Screens: E(T), greybody emission, Off/Building/Established/Collapsing, energy ledger with invariant tests; own-screen glare in SNR.
8. ~~Missiles~~ — done (see below). Still to do: beam emitters on ships (hit resolved against truth; no P_hit formula), probes (laser-link reports/commands as light-fronts), point defence / interceptor missiles, detonation and laser flashes as bright emissions, missile datalink delay (today the missile uses its faction's track instantly), active seekers.
9. Doctrine bots for cruiser / frigate / transport in `luminal-bots`; §15 variants in `luminal-cli`.
10. ~~Departure region and win/loss~~ — done: Transport reaching the region (0.25 AU beyond the planet, radius 0.02 AU, placeholder) wins for the Escort; losing it wins for the Raider. Without weapons the Raider cannot win yet.
11. Sun glare: sensing near the star's direction should be degraded, not just occluded.

## Constraints to keep

- Never let anything reachable from a faction `View`/`Mind` read world truth.
- Never hardcode an unresolved value outside `params.rs`; never present a placeholder as approved.
- Displacers and gridfire are excluded. Do not restore the old 1,000 g probe / 2,000 g missile specs or a shield-HP model.
