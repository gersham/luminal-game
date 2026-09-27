# TL7 ship combat mechanics — design handoff

## 1. Status, authority, and scope

This document consolidates the ship-combat discussion into a handoff for another agent. It is a design specification, not a claim that the game exists.

Implementation was explicitly paused. The current request authorizes writing this document, not resuming game implementation. The repository has a substantially implemented TL7 ship designer, engineering evaluator, dossier, and detection calculator. No playable game engine or deployed-probe simulation has been implemented.

The discussion has evolved beyond the earlier Claude game-design draft. This document takes precedence wherever they differ. In particular:

- Ships: **100 g** maximum acceleration.
- Probes: **500 g**, **10-tonne class**, few carried.
- Missiles: **1,000 g**, **100 kg class**, many carried.
- Missile payloads: **standoff nuclear-pumped laser**, **near-contact nuclear**, **kinetic kill**.
- Screens: energy-storing, self-emitting fields with temperature, a maximum temperature, an operating emission floor, and gradual activation/deactivation.
- FTL: tactical, blind, conspicuous repositioning; the working proposal makes it incompatible with established screens.
- **Displacers and gridfire are excluded for now.** Existing catalog entries are not a mandate to include them in combat.

Do not silently restore the old 1,000 g probe / 2,000 g missile specifications, 20-tonne probe payload, generic shield-HP damage model, or strategic-only FTL rules.

### Commitment levels

**Established direction:** explicit user choices and the core physical/accounting principles below.

**Working proposal:** a concrete interpretation offered during discussion, suitable for prototyping but not a final balance decision.

**Unresolved:** parameters or mechanisms that have not been selected. Do not present illustrative numbers as approved constants.

TL7 is the combat scope. TL3/TL5 remain reference shipbuilder profiles. Earlier requested eventual game mode: computer versus computer, with an omniscient human spectator and strictly non-omniscient bots. This remains a future implementation requirement, not authorization to resume work.

## 2. Combat identity

Combat is a contest of prediction, observation, concealment, and committed trajectories, with brief violent firing opportunities. It is not stationary ships exchanging generic damage points.

The defensive sequence is:

1. Avoid detection.
2. Degrade the enemy track.
3. Spoil the intercept.
4. Intercept or deceive the weapon.
5. Capture the attack in the screen.
6. Limit residual damage with hull protection and redundancy.

Surviving a hit can still cost concealment, sensor performance, screen capacity, and freedom to use FTL.

Objectives matter. Without a convoy, interception deadline, destination, defended installation, or similar constraint, avoiding combat indefinitely may be rational. Objectives create predictable geometry without imposing artificial space boundaries.

## 3. Units, motion, and conservation

Use explicit physical units. Suggested simulation units: kilometres, seconds, kilograms or tonnes with explicit conversions, joules, watts, kelvin.

- c = 299,792.458 km/s.
- 1 g = 9.80665 m/s².
- Positions and velocities persist.
- Facing is not velocity.
- Turning the hull does not turn its existing velocity vector.
- Thrust changes velocity; ending a movement interval does not stop the ship.
- Weapons and probes inherit their carrier's velocity at release.
- Co-moving displays subtract one common reference-frame velocity; they do not perform free matching burns.

At 0.1c and 100 g, Newtonian reference values are approximately:

- Braking time: 8.49 hours.
- Braking distance: 3.06 AU.
- Constant-speed turn radius: 6.1 AU.

These are reference calculations, not instant maneuver permissions. Small unpredictable burns can spoil a shot even when major trajectory changes take hours.

Conserve energy and account for momentum. Capturing a projectile's energy does not eliminate its momentum. Screen recoil, momentum carried away in emission, and any fictional field coupling require an explicit model. Do not grant arbitrary impact energy based on speed relative to the star: **relative impact velocity is what matters**.

The treatment of relativistic motion, maximum speeds, and the acceleration/thrust-direction limits remains to be selected. Do not silently import an earlier arbitrary speed governor as an agreed rule.

## 4. Information and sensing

All ordinary sensing and communication propagate at light speed, including fictional field sensors, probe laser links, active returns, weapon effects, and FTL-event signatures.

Knowledge levels are distinct:

- **Detection:** something is present in a direction or uncertain region.
- **Track:** observations suggest motion.
- **Localization:** position and velocity are sufficiently constrained for a particular purpose.
- **Firing solution:** predicted uncertainty at weapon arrival is small enough for that weapon.

A bright detection is not automatically a precise firing solution. A precise old observation can be less useful than a rough fresh one.

Every observation should retain:

- Time the target state was measured/emitted/reflected.
- Time the sensor received it.
- Time the deciding ship received any relayed report.
- Sensor origin and uncertainty.

Passive sensing includes target-to-sensor delay. Active sensing includes outbound pulse and return echo. A probe report adds probe-to-carrier delay; commanding the probe adds carrier-to-probe delay.

Already-emitted signals survive destruction, shutdown, or departure of their source. No instant enemy health, kill, probe-loss, or jump notifications.

AI decisions must use only the ship's own state and causally received information. World truth may resolve physical intersections and supply the spectator, not guide hostile targeting. Missile seekers also need local observations rather than hidden world-state access.

ECM degrades the sensing mechanisms it actually affects. It is also an emission source. Active sensing improves some measurements but exposes the emitter. Neither should be a universally beneficial always-on setting.

## 5. Energy emitters and probability of hit

Ship energy emitters have a useful combat envelope of **several light-seconds**, rather than a universal hard range cutoff. Predictable targets may remain vulnerable farther away than actively maneuvering ships.

For a direct observer/shooter:

    prediction interval = age of target information + weapon flight time

For fresh direct observations and a beam, approximately T = 2R/c. Real moving geometry should use actual propagation paths.

At three light-seconds, a beam intercept is approximately six seconds beyond the observed target state. Unexpected 100 g lateral acceleration can produce about 18 km of displacement from a constant-velocity prediction over that interval. This is a maximum-acceleration example, not a Gaussian standard deviation or guaranteed miss.

### Illustrative range curve, not approved balance

A proposed initial approximation was:

    P_hit(R) = P_close / (1 + (R / R_half)^4)

R_half is the range at which probability falls to half P_close. With P_close = 0.95 and R_half = 2 light-seconds:

| Range, light-seconds | Hit probability |
|---:|---:|
| 0.5 | 94.6% |
| 1 | 89.4% |
| 2 | 47.5% |
| 3 | 15.7% |
| 5 | 2.4% |
| 10 | 0.15% |

These are tuning examples, not calculated weapon performance. Eventually accuracy should emerge from tracking uncertainty, information age, target maneuvering, pointing, and weapon characteristics. Do not penalize the same uncertainty again with an independent generic range or ECM modifier.

Separate:

1. Probability of intersecting the target effectively.
2. Energy delivered and concentrated sufficiently to cause an effect.

Beam spreading can reduce coupled energy even when aim succeeds. A powerful narrow beam can miss completely.

## 6. Missiles as space torpedoes

### Established specifications

- 100 kg class objects; clarify whether catalog mass means fueled launch mass or remaining impact mass.
- Maximum acceleration 1,000 g.
- Many carried compared with probes; exact inventories and launcher capacities are unselected.
- Finite propulsion budget sufficient for a brief initial burn and some terminal maneuvering.
- Burn, cruise, terminal phases.
- Inherited launch velocity and persistent motion.

At 1,000 g, full thrust provides approximately 9.81 km/s of delta-v per second. Examples:

| Full-thrust time | Delta-v |
|---:|---:|
| 10 s | 98 km/s |
| 30 s | 294 km/s |
| 60 s | 588 km/s |

These do not select the fuel allowance. A sustained 100 g ship accumulates the same delta-v in ten minutes as a 1,000 g missile in one minute. Initial geometry still determines whether it can evade before interception.

### Burn

Commit to a predicted encounter trajectory. Relatively conspicuous. Initial acceleration consumes budget that cannot also be spent in terminal correction.

### Cruise

Coast without losing velocity. Quieter, not invisible. Receive delayed updates. Any correction consumes propulsion budget; calling it cruise does not make maneuvering free.

### Terminal

Use local sensing and remaining maneuver authority. The missile must solve its own short-delay encounter, not wait for distant carrier approval. Seeker limitations, deception, fuel, and closing velocity matter.

Simple lateral correction reference:

    correctable displacement approximately 0.5 * available acceleration * remaining time²

At 1,000 g this gives approximately 490 km in 10 s, 4.9 km in 1 s, and 49 m in 0.1 s, before sensor delay, acceleration-direction constraints, or existing lateral velocity are considered.

At a 30,000 km/s closing speed, the last 3,000 km lasts only 0.1 s. High closing speed increases impact energy but reduces terminal recovery time.

### Payload A: standoff nuclear-pumped laser

Deliver a one-shot local beam platform into a useful firing position. No contact or velocity matching is required. Local sensing and short beam flight reduce the information problem.

Only some source energy becomes a useful directed beam; not the full explosive yield. Conversion, pointing, beam concentration, and target coupling need a defined model. Reliable practical bomb-pumped lasers are a TL7 technological assumption, not an established deployed capability.

The defender must stop the missile **before it fires**, not merely before hull contact. Destroying it after emission does not recall the pulse.

### Payload B: near-contact nuclear

Pass close enough that the energy intercepted by the ship is damaging. No atmospheric blast wave exists in vacuum. Radiation and any material effects must be explicitly coupled to the target.

Total yield is not automatically energy absorbed by the screen. Most broadly emitted energy may miss the ship. Useful proximity radius and yield remain unselected.

### Payload C: kinetic kill

Physically intersect the target. Most demanding terminal geometry, potentially extraordinary coupled energy.

At approximately 0.1c relative speed:

| Remaining impact mass | Approximate kinetic energy | TNT equivalent |
|---:|---:|---:|
| 1 kg | 0.45 PJ | 108 kilotonnes |
| 100 kg | 45 PJ | 10.8 megatonnes |
| 1 tonne | 450 PJ | 108 megatonnes |

Relativistic corrections are small but nonzero at 0.1c. Exact accounting uses E_k = (gamma - 1)mc². Actual deposited energy depends on the interaction, not just nominal projectile energy.

Contact missiles remain useful with early reliable acquisition, favorable crossing geometry, constrained/damaged targets, or attacks against defenses poorly suited to physical impact. They are not guaranteed failures against active evasion, nor guaranteed hits merely because missile acceleration exceeds ship acceleration.

### Two-layer success model

Separate delivery probability from terminal success:

- Can the missile reach a useful encounter, survive defenses, and acquire the target?
- Given that encounter, can this payload connect effectively?

Launch range primarily affects delivery. Local range, closing velocity, remaining delta-v, and track quality govern terminal success. One launch-range accuracy table cannot represent every encounter.

## 7. Probes

Established revised specifications:

- 10-tonne class.
- 500 g maximum acceleration.
- Few carried; expensive information assets relative to individual missiles.
- Independent active/passive sensing, significantly weaker/shorter-range than warship arrays.
- Fragile; no meaningful screen or armor protection.
- Directional laser reports and commands at c.
- Stowed or deployed probes do not add their sensor strength to the carrier's own arrays.

Earlier retained concept: committed fixed-axis launches, inherited carrier velocity, no homing or course corrections. A fixed thrust axis can produce a curved world trajectory during burn if inherited velocity is not aligned with it. After fuel exhaustion, the probe coasts rather than stops.

Earlier discussion proposed one hour of full-thrust probe fuel and a hull-dependent inventory capped at 50. The later revision changed acceleration/mass and emphasized few probes, without fixing a replacement endurance/cap formula. Confirm these before implementation; do not silently reuse old 1,000 g delta-v figures or old fitting costs.

A probe is launched into a predicted encounter region. Wrong predictions waste it. Its burn or ping exposes the probe, not magically the carrier; carrier inference must come from observed evidence.

Local probe knowledge and delayed carrier knowledge are separate. Reports already transmitted survive probe destruction. The carrier infers loss through delayed evidence or missed reports, not truth-state notification.

Probes also provide external observations when the parent ship's hot screen degrades its sensors. Their weak instruments, fixed trajectories, link geometry, and report delay prevent them from being a perfect substitute.

## 8. Screens: field battery and emitter

### Established physical concept

The screen itself captures energy, stores it, and emits it later. It has a thermal state, a maximum temperature, and a temperature-dependent emission curve.

This temperature belongs to the fictional field reservoir, not ordinary hull material. Depositing tens of petajoules directly into conventional ship structure would be catastrophic.

Do not use the previous abstract shield-HP bar as if it represented this accounting.

### Quantities

- E(T): stored field energy as a function of temperature.
- T_max: maximum safe field temperature.
- Capture limits: which incident pulses/fluxes/impulses the screen can accept.
- A_eff: effective radiating area.
- epsilon: effective emissivity.
- Thermal leakage into conventional ship systems.
- Activation fraction / established field extent.

Working first approximation: constant effective heat capacity C_s:

    delta E = C_s * delta T

Assuming greybody thermal emission:

    P_emit(T) = epsilon * sigma * A_eff * (T^4 - T_background^4)

    dE/dt = P_captured + P_maintenance_heat - P_emit - P_transferred

Every transfer retains an energy destination. Moving energy into another onboard system does not dispose of it.

When background is negligible, emission at 75%, 50%, and 25% of maximum absolute temperature is respectively about 31.6%, 6.25%, and 0.39% of maximum emission. Use kelvin, not Celsius fractions.

### Cooling

For constant heat capacity, fixed area/emissivity, negligible background, and no ongoing input:

    time(T_i -> T_f) = C_s / (3 * epsilon * sigma * A_eff)
                      * (1/T_f^3 - 1/T_i^3)

This is not the complete powered-idle or collapsing-field equation. During maintenance, cooling approaches the powered equilibrium; during collapse, field area/capacity may change and require explicit accounting.

The behavior is a bright initial flare, useful recovery of absorption headroom, and a long cooling tail. Becoming quiet can take much longer than becoming able to survive another hit.

Scale example: 45 PJ discharged at a constant 100 TW takes 450 s, or 7.5 minutes. Actual temperature-dependent cooling is not constant power. Do not use this quotient as the complete cooling model.

### Saturation and damage

Working failure rule: energy that cannot be captured within the screen's current limits reaches the underlying ship. Stored energy is not reset or deleted.

At maximum temperature, steady captured power cannot exceed outgoing discharge power indefinitely. A brief pulse can exceed remaining capacity despite a manageable long-term average.

Loss of field power or catastrophic field failure must eventually specify the destination of stored energy. No implementation may erase it on shutdown, destruction, or mode change.

### Screen telemetry

Show temperature, stored energy, remaining absorption headroom, emitted power/spectrum, activation state, and estimated cooling time to a defined threshold.

## 9. Screen emissions, stealth, and sensors

A hot screen inevitably compromises concealment under the fixed emission model. The ship cannot accept a huge hit and simply choose to remain dark.

Emission becomes observable only after its light reaches each observer. Greater brightness improves detection but does not remove positional uncertainty or weapon flight delay.

Working sensor model: progressive, band-dependent degradation from local screen radiation and interference:

- Background glare and photon noise.
- Scattering/internal reflections.
- Detector saturation.
- Restricted observing apertures/directions.

Subtracting a predictable background does not remove photon noise or undo saturation. A screen that absorbs a wavelength cannot automatically be perfectly transparent to sensing in that same band; apertures or selective transmission need an explicit technological assumption.

Faint contacts disappear first. Strong close threats remain observable longer. Dedicated point-defense sensors should be more robust, not omniscient or immune. Active sensing may recover some measurements but adds emissions and still obeys propagation delays.

Interference with fictional field sensors is a separate rule choice, not automatically implied by thermal radiation.

Avoid blanket blindness at an arbitrary heat percentage and avoid double-counting tracking degradation as an additional unrelated weapon-accuracy penalty.

## 10. Screen posture and cold running

Established direction: screens have an operating emission floor, take time to energize, and take time to switch off. Running without screens is a valid tactic.

Working state machine:

| State | Protection | Screen emission |
|---|---|---|
| Off | None | None; ordinary ship signatures remain |
| Building | Increasing capture/storage capability | Rising toward operating floor |
| Established | Rated capability subject to heat and damage | Idle floor or higher when hot |
| Collapsing | Decreasing capability | Continues while stored energy is discharged |

The emission floor is supplied by maintenance losses, establishing an idle field temperature. Do not invent source-free luminosity or add the same baseline heat twice.

During buildup, partial field capacity must be real. A partly established screen is not a full screen hidden behind a progress indicator.

Working shutdown constraint: contract only as fast as the remaining field can contain its remaining energy. A hot screen takes longer to lower. Final residual energy may be transferred to conventional thermal systems only within their limits and remains accounted for.

Illustrative timing discussed: about 60 s to establish; about 120 s to lower a cool screen. **These are unapproved tuning values.** Hot shutdown can take substantially longer.

A ship cannot observe a laser launch and raise its screen before that same beam arrives. It needs earlier situational warning.

Tactics: cold approach, early defensive commitment, delayed commitment, cooling during disengagement, and threatening attacks intended to force an enemy to raise its screen and reveal itself.

## 11. Armor and physical damage

Screens make surviving principal combat energies possible. Armor is damage limitation, not an equivalent second health bar.

Useful protection includes:

- Ablation against limited beam exposure and residual radiation.
- Debris protection within a defined mass/velocity envelope.
- Internal shielding, compartmentation, and redundancy.
- Protection of critical systems against local failures.

A clean 100 kg impact at 0.1c is not a routine armor-point deduction. Shield-down is a mission-threatening emergency, though destruction still requires an effective attack.

Even small fractions matter: 0.01% of 45 PJ is 4.5 TJ. A one-gram fragment at 0.1c carries about 0.45 TJ. Fragmenting a weapon late does not necessarily neutralize it.

Damage depends on fluence, concentration, duration, spectrum, penetration, geometry, and coupling. Exact damage/system-failure rules remain unselected.

## 12. Additional weapons: candidates, not commitments

Discussed candidates:

- Unguided kinetic guns: predictable targets and prepared crossings; launch energy/recoil accounted for.
- Distributed kinetic salvos: trade energy per impact for more threatening trajectories.
- Sensor dazzlers: disrupt particular sensing channels, not universal blindness.
- Defensive interceptor missiles: defeat threats before their firing/contact opportunity; debris trajectory still matters.
- Deployable torpedoes/mines: prepared moving trajectories near constrained routes/objectives, not stationary universal space obstacles.
- Deception payloads: consume attention/ammunition or induce defensive emissions; convincing emissions require actual power.
- Particle emitters: only if their propagation and screen interaction add a distinct role; no free shield bypass.

The strongest suggested core is direct energy emitters, the three torpedo payloads, unguided kinetics, defensive interceptors, and deception/dazzling. None of the additional candidates is yet a fully selected catalog specification.

Displacers and gridfire are explicitly parked. Do not reintroduce remote interior bombs, unrestricted external energy sources, or equivalent mechanics under different names.

## 13. Tactical FTL

### Established direction

FTL is noisy repositioning, a desperate escape, or a reckless charge—not merely a strategic travel abstraction. Ships are blind while using it and it is highly emissive.

Working proposal accepted for exploration: FTL cannot operate with screens up. Screen collapse, particularly when hot, therefore gates entry. Blindness is explicitly attributed to drive-field interference with external sensing/communications, not solely to outrunning photons.

### Sequence

1. **Prepare:** finish collapsing the screen; drive preparation becomes conspicuous; ordinary sensing remains available until engagement.
2. **Transit:** no screen, no fresh external sensing or communication; follow committed navigation conditions.
3. **Drop out:** strong local arrival transient; still unprotected; sensors resume receiving actual available light.
4. **Recover:** build screen and reacquire tracks; any additional drive recovery interval remains to be selected.

A cold ship can prepare entry sooner but has accepted unshielded risk. A hot ship cannot delete its stored screen energy to escape.

### Causality of the signatures

Departure, transit emissions, and arrival propagate locally at c. No global jump notification or destination disclosure.

An FTL ship can outrun its own departure warning. Example: travel ten light-seconds in one second. A destination observer can witness arrival before the departure light arrives. This is surprise, not stealth: the arrival itself is bright.

Earlier discussion proposed effective detectability of at least 1e10 sig for spooling and 1e12 sig for a departure transient, visible across a planetary system even to weak probes after propagation. These were calibration proposals, not physical watts or final approved values. The latest requirement is highly emissive tactical FTL. Actual emitted energy, detectability units, spectrum, duration, and transit signature require a consistent selection.

### Blind commitment

The ship retains its own navigation state and prior observations. It may follow timed/navigation-based exit or abort conditions, but cannot respond to unseen external changes.

On exit, sensing uses photons/signals arriving at the new location. Do not provide instantaneous local truth or impose a fictitious extra wait for photons that are already arriving.

### Momentum and reference frame

Working rule: FTL relocates the ship without freely changing its ordinary velocity vector, expressed in a defined system reference frame. It does not provide free braking, turning, or arbitrary kinetic energy.

Define an FTL charge as dropping out to attack, not superluminal ramming. FTL collision behavior and navigation hazards require an explicit rule before implementation.

A preferred simulation frame/causality convention must be specified: FTL is fictional and cannot simply be added to unrestricted special-relativistic frame transformations without causality issues.

### Tactical uses

- Reposition outside the immediate hostile firing envelope, trading a live track and protection for geometry.
- Escape during an exposed screen-down interval; ongoing hits can delay it by adding stored energy.
- Charge by dropping out nearby, bright, unshielded, and using an old target prediction. Potentially surprising, potentially suicidal.

FTL speed, preparation duration, minimum/maximum transit, steering/abort limits, exit uncertainty, gravity restrictions, interference/disruption, and repeat-use limits remain unselected. Do not assume earlier strategic spool/recovery numbers are final tactical rules.

## 14. Eventual simulation and spectator requirements

If implementation is later authorized:

- Default computer-versus-computer mode.
- Omniscient spectator can inspect truth and compare each ship's delayed knowledge.
- Bots and autonomous seekers have restricted causal inputs.
- Inspectable observations, timestamps, track uncertainty, decisions, orders, and rejection reasons.
- Pause, single-step, wall-clock playback speed, deterministic seeds, and replay.
- Actual navigable board with momentum, velocity vectors, trajectories, and optional common co-moving frame.
- Probe local knowledge distinguishable from carrier-received reports.
- Screen energy/temperature/emission histories visible for tuning.
- No scripts forcing detections, hits, or outcomes to make a scenario appear successful.

Turn length, integration scheme, hit sampling, AI tracking algorithm, numerical bounds, and endpoint/API design are not fixed by this discussion. Earlier implementation contracts may provide ideas but are not authority over these mechanics.

## 15. Decisions still needed before implementation

1. Screen E(T), maximum temperature, effective area/emissivity, idle losses, capture limits, leakage, and field-failure energy destination.
2. How activation fraction changes capacity, capture capability, and radiating area without violating conservation.
3. Final activation/shutdown durations and safe handling of residual energy.
4. Sensor bands, screen transmission/apertures, noise model, and tracking uncertainty.
5. Missile fuel/delta-v allocation, seeker capability, launch/impact mass, payload coupling, magazines, and launch rates.
6. Revised probe endurance, exact inventory caps, cost, fitting mass, sensor calibration, and communication geometry.
7. Direct-emitter energy, aperture/beam behavior, firing cadence, useful range curves, and power/heat accounting.
8. Screen momentum exchange, physical damage, and armor/system-failure treatment.
9. Tactical FTL transit model, causality frame, timing, emissions, hazards, and restrictions.
10. Objective-driven encounter scenarios and subsequent balance targets.

A useful first exploration is one cruiser intercepting a transport before a departure region, with a defending frigate and incomplete information. Compare passive/active sensing, probes/no probes, and payload choices while retaining the same initial trajectories. This is a proposed experiment, not a mandated default scenario.

## 16. Handoff instruction

Preserve the central accounting and information constraints before tuning outcomes. Clearly distinguish user-selected mechanics from proposed constants. Do not claim the current designer already simulates these rules. Do not resume implementation without authorization.
