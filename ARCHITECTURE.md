# Luminal — architecture

**How the game is built to satisfy [GAME_MECHANICS.md](GAME_MECHANICS.md). The mechanics document is authority on rules; this document is authority on structure.**

Current implementation details and prototype limits: [Tactical refinement pass](REFINEMENTS.md).

## 1. Product shape

- **Top-down 2D real-time strategy**, native Linux desktop application.
- The player commands one assigned ship, receiving allied sensor reports at light speed. Other platforms are autonomous. Opponents are bots. AI-versus-AI with an omniscient spectator is supported; replay remains planned.
- **Solar-system map scale**: the strategic map covers roughly 10–50 AU around a star; zoom reaches down to km for encounters.
- **Real time with time compression.** Engagements span hours of approach and sub-second terminal encounters. Only player controls change warp; no contact, damage or order event changes it automatically. The debugging scenario starts at 50×.
- First milestone: the §15 experiment — a cruiser intercepting a transport before a departure region, with a defending frigate — playable, and runnable headless as comparable variants.

## 2. Non-negotiable structural rules

Player-facing terminology follows the **modern, post-WWII Royal Navy terminology principle** in GAME_MECHANICS.md, favouring contemporary usage. Shared track labels are formatted centrally through `ContactId` so map, orders, alerts and weapons use the same designation.

The compact bottom command deck has four equal quarters: own commands/magazines, own status, target status, target orders. Each status quarter stacks hull, armour and screen-heat bars above grouped system chips, with a narrow vertical thrust gauge. Target heat is the last received active-echo report, not truth. Unknown target thrust stays unknown. A top-left control inset and transparent top-right eight-line fading combat log overlay the map. Bottom-left navigation shows speed/range/closure; bottom-right shows tactical scale or hovered-object details. No View As control is exposed. Unresolved contacts use T1; resolution changes the prefix to the configured class (FF1, BB1, CV1), preserving the contact number. CV for cruiser is the requested game convention, not a claim about real naval classification.

1. **The player is a `Mind`.** The player's map, contact list and telemetry are rendered from that faction's `Perception`, exactly as a bot would receive it. Only spectator mode renders truth.
2. **No truth leaks through the `mind` boundary.** Bots, missile seekers, probes and the player UI consume `Perception`; nothing reachable from them can query world state.
3. **All information is a light-front.** Sensing, communication, beam weapons, screen flares and FTL transients are emission records resolved by one retarded-time engine.
4. **Motion is committed data.** Every body has an append-only trajectory of constant-acceleration segments. Orders append; history is never rewritten.
5. **Every joule has a destination.** Energy and momentum transfers go through a double-entry ledger with invariant checks in tests.
6. **Deterministic.** Initial state + parameter hash + seed + timestamped order log reproduces a run exactly, regardless of warp or frame rate.
7. **Every unresolved constant is labelled.** `params` tags each value `Established`, `Proposal` or `Placeholder`; the UI can show the tag.

## 3. Networking

Local play first, internet multiplayer if the idea works. The structure is chosen now so that step is additive:

- **Server-authoritative simulation.** Only the server holds world truth.
- **Clients receive their faction's `View` only** — the causally received picture — and send `Command`s. A modified client cannot reveal what its faction has not seen, because that information is never sent. This rules out deterministic lockstep, which requires every client to hold full truth.
- **The session is the network boundary.** `luminal-core::session` defines `Role`, `Command`, `Rejection` and `View`. Local play runs the session in-process; a later `luminal-server` wraps the same session behind a transport. The client never touches `World` directly.
- **Spectator role** receives truth. It is for local exploration, AI-versus-AI and post-game replay, not for competitive players.
- **Shared clock.** In multiplayer, warp is a server decision (e.g. the minimum requested by players, with automatic drops on events); the server clock is authoritative and views carry sim time.

## 4. Workspace

```
crates/
  luminal-core   pure simulation library; no I/O, no rendering, no wall clock
  luminal-bots   doctrine bots and tracking filters, depending only on core's mind API
  luminal-app    eframe/egui desktop client: map, panels, time controls, spectator
  luminal-cli    headless runner: scenario variants, belief-vs-truth reports, sim speed
  luminal-server (later) the same session behind an internet transport
```

### luminal-core modules

| Module | Responsibility |
|---|---|
| `units` | Constants (c, g), unit conventions: km, s, kg, J, W, K |
| `params` | All tunable values with commitment tags |
| `kinematics` | Piecewise constant-acceleration trajectories (total accel and thrust per segment), exact state queries, termination |
| `celestial` | Star, planets, moons on analytic ephemerides; gravity, integration step size, surface impact, light-path occlusion |
| `lightcone` | Emission records, retarded-time solve, beam propagation and intersection |
| `sensors` | Bands, SNR with own-screen glare, bearing/range measurement noise |
| `screens` | E(T), greybody emission, Off/Building/Established/Collapsing, capture limits |
| `ledger` | Energy and momentum accounting |
| `missile` | Payloads (standoff laser, near-contact nuclear, kinetic), burn/cruise/terminal guidance, seeker line-of-sight tracking, closest approach |
| `autopilot` | Standing orders (move-and-stop, orbit, intercept) and collision avoidance |
| `scheduler` | Discrete-event queue, deterministic tie-breaking |
| `rng` | Seeded random streams |
| `mind` | `Perception`, `Observation`, per-contact Kalman tracks — the only agent-facing API |
| `session` | Roles, commands, rejections and per-role views — the client/network boundary |
| `world` | Owns truth; advances time; resolves physical interactions |
| `record` | Order log, snapshots, belief and decision logs for replay and inspection |

## 5. Time

The simulation is **event-driven**, not fixed-tick. Between events, kinematics is analytic, so an 8-hour burn and a 0.1 s pass are equally exact. Events include mind wake-ups, segment boundaries, sensor integration frames, predicted light-front arrivals, closest-approach windows and screen thermal thresholds. Screen temperature is integrated adaptively between events (analytic cooling when there is no input).

**Gravity.** Celestial bodies move on known analytic orbits and are not perturbed. Ships feel their gravity through the same committed segments: each body has a recurring step event that samples gravity at the predicted midpoint of the next step and appends a segment (thrust + gravity). Steps are 1 % of the local free-fall time, 0.5–60 s, so they shorten near planets. A step also tests the previous interval for surface contact; contact destroys the body and records a loss. Because past segments are never rewritten, retarded-time queries on history stay exact.

The app drives the world with `world.advance_to(t_target)` each frame, where `t_target` grows by `frame_dt × warp`. Because the world only processes events up to the target, frame rate and warp never change outcomes. Player orders are stamped with the sim time at which they are issued and logged.

## 6. Information

- **Frame convention (proposal):** Newtonian kinematics in a preferred system rest frame; signals propagate at c in that frame. A global time makes tactical FTL causally consistent: it is a relocation in system time.
- **Retarded-time solve:** light emitted by A at `t_e` reaches B at `t` when `|x_B(t) − x_A(t_e)| = c (t − t_e)`. Trajectories are stored, so this is a 1-D root-find on exact positions.
- **Observation** carries emission time, sensor receipt time, decider receipt time, sensor origin and uncertainty.
- **Three sensor channels:** passive localization around 0.1 AU, active ranging around 1 AU, and direction-only finding around 10 AU for a reference military ship burning at 10g. Nominal detection probability is 75% per frame/return. Passive localization uses a proposed noisy TL7 range measurement; direction finding supplies only bearings and may triangulate across sensors. Emission strength changes passive ranges, and cross-section governs active returns. Each emitted ping exposes its source at ten times its passive/localization and direction ranges, with the source signature captured at emission, preserving light delay after shutdown.
- **Occlusion:** celestial bodies block every light path — emission, pings, echoes and laser relays. Their motion is linearised over the transit.
- **Manual ping and standing beam orders:** `Command::Ping` emits one physical front immediately. Session views expose only own emission records; their white outline display expands at c/2 and fades from 0.7 to 1 AU. `EngageBeam` stores a designated track and schedules generation-tagged checks; received estimated range gates automatic pulses at a provisional 3 ls, with 10 s recharge. Cease fire invalidates pending checks without cancelling emitted light. UI inspection is separate from persistent friendly command selection.
- **Relays:** a faction decides at its flagship (lowest-numbered live ship). Other ships' reports travel there at c and can be blocked.
- **Tracking:** each faction's perception holds a constant-acceleration extended Kalman filter per contact, with known gravity in prediction. It lives in `luminal-core::mind` because the session builds the player's view from it; bots consume the same tracks. Firing solutions propagate covariance to weapon arrival time.
- **Hit resolution:** the shooter aims at its estimate; the beam or projectile is then resolved against the target's true trajectory. Probability of hit is emergent. The illustrative P_hit curve is a calibration target, not code.
- **Ship beam pulses:** armed ships may fire independently of missile inventory, subject to a tagged recharge interval. The firing solution extrapolates the causally received faction track to beam arrival; emitted direction never changes. Pulses survive shooter loss, resolve at light-front arrival, and respect celestial occlusion. A bounded Gaussian fluence approximation reduces intercepted energy with spot size and miss distance before passing it to screens/hull. Own views expose cooldown, emitted energy and the firing-solution line, never truth hit results. Reactor supply and waste heat are explicit prototype omissions.

## 7. Relativity and energy (proposals)

- Kinetic energy of impacts uses relative velocity and `(γ − 1) m c²`.
- No speed governor; propellant/delta-v budgets are the constraint.
- Captured energy enters the screen reservoir via the ledger; momentum is recorded separately and is not destroyed by capture.

## 8. Client

- **Map:** top-down 2D vector graphics. Ships are arrows (along thrust, or velocity when coasting) with velocity-vector tails. Celestial bodies are drawn at true size with a minimum dot, with orbit lines, off-screen edge markers and sensor-shadow cones from the selected ship. Forecasts include gravity and mark predicted impacts. Log-scale zoom from 100 AU to km, pan, optional co-moving display frame (display-only; subtracts a common velocity).
- **Overlays:** committed trajectories, velocity vectors, expanding light-front rings, and contact age rings capped at 40 px after one hour (display placeholders). Map labels omit numeric age; actual uncapped covariance remains available in contact details. In spectator mode, belief-versus-truth for any faction.
- **Movement:** points use burn/flip/brake and hold in their celestial frame; celestial bodies use orbit guidance. Right-clicking a ship defaults to full-thrust flyby, then coasting after passage. A side-panel toggle changes the order to intercept and match velocity. Enemy guidance consumes faction tracks only.
- **Panels:** contacts and track quality, screen telemetry (temperature, stored energy, headroom, emission, cooling estimate), magazines, orders and rejection reasons, parameter provenance.
- **Time controls:** pause, single-step to next event, warp, automatic warp drop on perceived events.

## 9. Open items tracked here

Everything in GAME_MECHANICS.md §15 remains open. Placeholder choices live in `luminal-core/src/params.rs` and are listed in the parameter panel with their tags. Changing a placeholder to an approved value is a one-line change plus a tag update.
