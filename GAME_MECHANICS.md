> Current implementation update (2026-09-28): the missile and jump sections in
> [README.md](README.md#missile-model) describe the playable tuning. Offensive
> missiles now fly physical fuel-budgeted trajectories; nominal range circles
> are not hard cutoffs. LRMs have a 120-minute lifetime and a 0.4/0.9/0.1 AU
> boost/coast/terminal profile at 1.4 AU. SRMs remain powered throughout flight.
> Auto-evade evaluates benefit before burning, unseen LRM cruise tracks require
> acquisition, and received active echoes improve missile fire control.
> Destroyer and larger jump drives spool for ten minutes and preserve velocity.
> Older conceptual values below remain design history where they differ.

# TL7 ship combat mechanics — design handoff

## Current playtest calibration (September 27)

This block supersedes historical calibration figures below. Offensive payloads are
LRM (nuclear proximity, 1,500 g, 29,979 km/s delta-v, 75 TJ maximum coupling) and
SRM (kinetic shotgun, 3,000 g, half the LRM delta-v, 1 kg impacting shot mass).
Pumped missiles are removed. Each warship has 20 of each type, with independent
launchers: SRM every 5 seconds, LRM every 60 seconds. Rapid clicks queue rounds.
LRM permits speculative launches at bearing-only or low-confidence contacts;
SRM retains the 1% estimated-hit-chance gate. Neither receives target truth.
Active detection reaches 1 AU for the reference suite (SNR 9); resolution is
guaranteed for an unobstructed received return within half that radius (0.5 AU,
SNR 144). Outer-band returns provide bearings only. Triangulated bearings do not
identify a class. An intercepted ping loud enough to localize is a position and a
firing solution, still without class, screens, or heat; a quieter ping stays a bearing.
Class waits for direct passive localisation or a usable active return. The station currently
has direction finding only: active and passive sensors are disabled from startup.
Clicking LRM or SRM directly queues that type; there is no separate Launch step. Ship interceptor
stock is 30, station stock 20. Interceptors never target interceptors and do not
launch inside 5 ls. Their outer gate is 0.27 AU and base terminal kill chance 60%.
PD lasers fire out to 1 ls but are last-ditch: 50% per shot at 0.006 ls with a
sixth-power falloff. Warships fire at 2 Hz, transport at 0.5 Hz, station at 1 Hz.

Main beams emit 75 TJ per pulse with a 5-second minimum interval. Capacitor
recharge is 15 TW and conventional cooling has a 60-second time constant, both
twice their previous rates to support doubled sustained fire. Point defence,
screen cooling and pulse damage are unchanged. Missile energy intercepted by screens deposits
40% as stored heat; the rest dissipates outside the field. Unscreened damage is
unchanged by that screen coupling. A missile screen puncture has a 35% chance,
leaks 1% energy and can damage a component; beams retain the small original leak
chance. This aims for about three-quarters of a matched salvo intercepted,
roughly half screen heat and one or two system casualties before the beam fight.
These are statistical targets, not guaranteed per-ship outcomes.

Warship hulls now have 1,000 HP; armour remains 100 HP and
screen capacity remains 450 TJ. Each HP retains its previous energy value.
Hull is only a points pool, not a subsystem or system chip.
Ship maximum thrust is 75% at or below 50% hull and 50% at or below 25% hull;
these limits multiply propulsion/power damage effects and recover with hull repair.
Damage control restores
1% maximum hull per ten effective minutes (10 HP at the standard warship fit).
Penetrating hits have a 20% subsystem-hit chance, rising to 40% below 50% hull
and 80% below 25% hull, using hull remaining after the hit. Component-hit selection weights propulsion at 2, power at 1 (halved), every other
installed non-destroyed subsystem at 1. Hull is not a randomly selected system.
Damaged propulsion disables thrust. Damaged jump drives cannot spool or jump.
Damaged power disables propulsion, jump, active sensors, EW and weapons;
screens instead retain a 50% cap with normal regeneration. Damaged screens have
the same non-stacking cap; repair restores the ceiling, then charge regenerates.
their chips turn grey until power is repaired, retaining underlying damage states.
Passive sensors, direction finding and ship mind use backup power; crew and damage
control also remain operational and keep their own condition colours. The ship
coasts with passive awareness. Destroyed power destroys the ship even with hull
remaining. Damage control repairs power before itself or any other damaged system;
emergency power repair remains possible during an outage, but hull repair pauses.
System repairs complete one damaged component per 120 effective seconds, slowed
by damaged crew or damage control. The current component has a progress line
and remaining-time tooltip. Random selection occurs when work starts and remains
stable; power and damage-control priorities can preempt it. Destroyed components
cannot be repaired. Hull repair rate is unchanged.
Before the subsequent doubling of subsystem-hit chance, five stationary knife-range trials produced four hull losses after 40–93 minutes,
with 3–8 affected components on the lost ship, including one destroyed component;
one trial reached the two-hour limit without a hull loss. System knockouts can
therefore leave a mission-killed ship afloat. See `calibration/systems-1000hp.csv`.

AI launch decisions account for range, position/velocity uncertainty and terminal
correction authority. Player speculative shots remain allowed. Missile seekers
have a 10-degree search half-angle, active acquisition within 5 ls, passive before
terminal; a wrong estimated aim can therefore cause a genuine miss. Ground-truth
closest approach resolves physical intersections, never missile guidance.

Map contacts are T1, T2 until resolved, then use class plus the same integer:
FF for frigate, BB for battleship, CV for cruiser (requested game convention),
TR for transport. Missiles remain × markers; defensive interceptors are points.
Target screen heat and thrust come only from the received sensor picture.
LRM/SRM fire buttons carry a bottom-edge estimated-delivery bar, computed
from received position/velocity uncertainty, search footprint, correction budget
and endurance. Below 1% the fire button is inhibited. This is a
heuristic before enemy defences, not a Monte Carlo calibrated probability.
The four-quarter command deck is commands, own status, target status, target orders.
No game event changes the selected time warp.

Manoeuvres: MATCH comes alongside and matches velocity; FLYBY burns for a maximum-speed
pass; LONG holds 3 AU, MEDIUM 1 AU, SHORT 0.03 AU; EVADE burns away at maximum thrust.
Selecting an enemy defaults to LONG. LONG/MEDIUM/SHORT burn towards a fresh
bearing-only contact, then brake or withdraw to hold the selected separation once
a range solution is received. EVADE burns away from a direction-only contact.
Stale evidence causes coasting; MATCH and FLYBY require a fresh resolved range.
Hidden bearing triangulation does not override the bearing-only manoeuvre. All use received information and collision
avoidance. The raider now starts twice the previous separation (about 1.28 AU).
The opening briefing supplies only a bearing, never an enemy position/course.
Bearing fade is measured from receipt, not emission; hostile ping indications no
longer replace the chevron of a resolved ship.

## Range balance principle

Combat should change character with range, not simply scale accuracy:

- Long range, beyond 2 AU: hide and seek. Emissions and shared, light-delayed observations
  suggest where to investigate; a direction indication is not a firing solution.
- Medium range, 0.1–2 AU and centred around 1 AU: opportunistic missile shots. Favour a good solution, surprise,
  overwhelming firepower or an already damaged target; a healthy equal opponent
  should not routinely die to a single full-magazine exchange at 1 AU.
- Short range, below 0.1 AU: commit missile salvos. Reduced warning and fewer defensive
  re-engagement opportunities should make concentrated fire consequential.
- Knife-fighting range, within about 3 light-seconds: main beams become decisive; point-defence lasers remain
  the last defensive layer, behind interceptor missiles.

These are soft tactical bands, not physical cutoffs. Beams can hit stationary,
non-evading targets much farther out. Directed beam fire has no range limit;
automatic fire beyond 3 light-seconds requires at least 1 GJ of expected coupled
energy, estimated from the received track's transverse uncertainty at arrival,
beam spreading and pointing error. Actual energy still follows physical pulse
arrival and the target's motion, never the prediction used to authorise fire.

Current AI holds missile fire beyond 2 AU, makes individual medium-range shots
while reserving four rounds of each payload, and commits the remaining magazine
inside 0.1 AU (the physical launcher still fires one missile per second). A known
escort stretches the medium-range interval from two minutes to six. Recognising
damage or coordinating overwhelming allied firepower remains future doctrine work.

This is the balance target, not a claim that all four regimes are calibrated.
The first benchmark is two stationary, screened frigates at 1 AU: roughly a
couple of delivered missile hits per ship, with destruction uncommon. Sensor
uncertainty, manoeuvring and fleet saturation require separate trials.

Offensive missiles inherit launcher velocity. Their own delta-v budget is
29,979 km/s (about 0.1c), with 60% allocated to initial boost (about 0.06c from
rest). This is a propulsion budget, not an absolute speed cap: fast missile
attacks require fast launch platforms, whose approach can expose them through
drive emissions. Interceptor propulsion is separate and unchanged.

## Current command and reconnaissance rules

### Desktop debugging start

The desktop client opens at 50×, with the player's frigate selected, the raider
targeted, and an intercept/match-velocity order already issued. Its initial camera
frames the two combatants. A supplied briefing uses the raider's historical light
and velocity, not a continuing truth feed; ordinary sensor rules apply afterward.
The top-bar **Restart** button recreates this initial setup, resets all combat
state and UI selections, and replaces `logs/latest.log`. The original unbriefed
scenario remains available to headless fixtures. Screenshot fixtures remain paused.

### Damage and damage control

Ships and stations currently start with 100 hull HP and 100 armour points,
independently configurable per platform. One point is 2 TJ (a
200 TJ hull budget expressed as 100 points). Screens still absorb into a thermal
reservoir. Screen heat starts at zero and rises only when absorbing incoming hits;
there is no ambient or maintenance heating. Weapon waste heat stays in the ship's
separate thermal reservoir. A screened hit has a provisional 5% chance to leak 1% of its energy;
that energy is subtracted from the amount captured by the screen, not duplicated.

Armour absorbs 50% of energy that gets past screens until its points are depleted.
The remainder always damages hull, including any absorption the depleted armour
could not provide. Zero hull destroys the platform. Each penetrating hit of at
least 1 MJ has a provisional 10% chance to hit one randomly chosen fitted,
non-destroyed subsystem. First hit damages it; second hit destroys it. Subsystems
have no separate hit points. Hull status is derived from hull HP, not this lottery.

System cards use green/intact, orange/damaged, red/destroyed, grey/unknown. Damaged
systems have half effectiveness; destroyed systems provide none:

- PASS, ACTV, DIRF: passive SNR, transmitted active power, direction-finding SNR.
- ECMS, ECCM: passive electronic-warfare signal ratio; historical target ECM is
  compared with the receiving platform's ECCM. Equal intact equipment cancels out.
- PROP, POWR, SCRN: acceleration limit, reactor recharge, screen capacity/build rate.
- BEAM, LNCH, PDMS, PDLS: firing throughput (half rate when damaged), not half ammo.
- MIND: control efficiency multiplies powered sensor/drive/weapon effectiveness.
- CREW: damage-control labour efficiency. Losing crew does not destroy the hull.
- DCTL: repairs one random damaged component per hour of effective work, choosing
  itself first when damaged. Also restores 1% of maximum hull per ten effective minutes.
  Damaged DCTL operates at half rate; crew damage further reduces repair labour.
  Destroyed systems cannot be repaired and armour cannot be regenerated. Idle
  component-repair time is not banked for instant repairs after a future hit.

The command ship's card is live. Enemy cards start unknown. As a gameplay
abstraction, a resolved active echo can inspect damage; its card reports the
historical snapshot at reflection time only after the echo and any allied relay
arrive. Newer unobserved damage is never exposed by this card.

Map designators are **Tn** before classification and **FFn/BBn/CVn** for resolved ship classes. “Resolution” describes solution quality, not the map name. The number stays stable. Resolved missiles retain × markers. These class codes and sequential numbers are game conventions.

**Probes are temporarily disabled for gameplay balancing.** All starting probe inventories are zero, and the simulation rejects probe launches from both player and AI. The implementation and intended three-per-military-ship inventory remain available for later re-enabling.

Reconnaissance probe stock is three per military ship: the frigate and raider each start with three. The transport and station have none, regardless of their defensive weapon fit. Deployed probes carry no probes themselves.

Point-defence missiles are a separate automatic magazine: 30 on each ship, including the transport, and 10 on the station. Current design: 1 launch/s, 0.3 AU outer launch gate, 3,000g acceleration, 180 seconds of full-thrust fuel and about 2.4 hours of boost-and-coast endurance. Their higher acceleration and fuel budget give terminal corrections an advantage over offensive missiles. Local sensor samples establish a velocity solution, then each launcher solves for a reachable intercept. Allied commitments deconflict after their reports arrive at light speed. These magazines do not consume offensive missile stock.

An interceptor must physically reach a provisional 100 km terminal envelope before a hit roll. The revised probability is `0.75 / (1 + (relative_speed/(0.25c))^4)`, with zero probability and no launch at encounter speeds of 0.5c or above. This supersedes the old 0.1c veto, which prevented interception of long-range offensive salvos. Interceptors use local light-delayed sensing, expend delta-v on manoeuvres, share observations at light speed and retire after a miss or endurance expiry. Half their propulsion is reserved after the initial departure burn. Cruise guidance uses one-second observations and a quadratic velocity fit; the final ten seconds use fine guidance. Correction authority is fuel-budgeted so distant noisy fits cannot consume the entire terminal reserve. These remain provisional balance values.

### Sustained frigate combat calibration

The repeatable headless fixtures distinguish a finite full-magazine exchange at
1 AU from a sustained close-range battle. The latter arms repeating main beams,
pings every minute, and delays the second ship's opening fire by a configurable
number of seconds. It does not refill magazines, reset damage, supply future
truth, or disable repair. Initial exact tracks are fixture inputs; subsequent
tracking is the normal causal simulation. Both ships remain stationary.

Screens retain their 450 TJ capacity, but peak radiation is now about 41 GW
(coefficient `0.000001 W/K^4`). Sustained accurate close fire can overload them;
disengagement gives them time to shed heat. Hull and armour together provide
400 TJ of initial protection after screens, instead of a single-pulse knockout.
Screen leakage, subsystem casualties and hourly damage control remain active.

Nuclear rounds are safe within two blast radii of their launch position. A noisy
initial velocity fit cannot trigger the estimated pass fuse beside the launcher.
Acquiring a nearby target no longer cancels the missile's departure boost before
it has useful closing speed. See `calibration/frigate-balance.md` for results,
limitations and reproduction commands.

Resolved broad platform classes are a gameplay assumption: missile ×, probe diamond, station square, ship arrow. Bearing-only indications do not expose class. Each physical source still has only one fused resolution; a resolved probe is labelled Probe rather than masquerading as a second ship resolution.

All four starting platforms (frigate, raider, transport and lunar station) carry one autonomous point-defence laser emplacement. The transport and station still have no offensive weapons or screens. Each emplacement defaults to one shot per simulation second, independently configurable on the platform. Local passive fire control engages detected hostile missiles, nearest first, and relays its observations home at light speed.

Point-defence laser lethality is `1 / (1 + (range_ls / 0.006)^6)`, with a 1 ls firing limit. Warships fire twice per second, transport once per two seconds, station once per second. A successful light-speed pulse kills one missile without triggering its warhead, respects occlusion and is not recalled when the shooter dies. Dedicated PD energy consumption is not yet tied to the main-beam capacitor.

A position-bearing sensor solution is called a **resolution**; a bearing without range is a **direction indication**. As an explicit gameplay assumption, resolving a missile also identifies its missile class. Resolved hostile missiles use small × markers, not ship arrows or Track labels, while unresolved directions do not reveal platform class. Internal contact IDs remain stable across resolution changes.

All events preserve player-selected time warp; no automatic slowdown occurs.

The escort scenario starts with the frigate and incoming raider's screens already raised (established field, initially cold). The transport and lunar station have no screen equipment and cannot raise screens or absorb damage into a screen reservoir.

Screen emissivity increases with stored heat: 1× cold, rising linearly to 100× at rated energy capacity. Radiation is this multiplier times the thermal radiation law, and removes the corresponding energy from the screen. Switching a hot screen off does not erase its stored heat or its emission; it cools and fades over time. The emissivity multiplier is capped at 100×, not the total radiated power, which also depends on temperature.

Platform emission is the sum of a fixed baseline, actual applied thrust output, and screen/hull thermal radiation. Baseline multipliers are transport 1×, stealth frigate 0.5× and station 2×; other platforms currently default to 1× of their class baseline. These multipliers do not hide drive exhaust or screen heat. Remote sensors sample emission at the retarded emission time, not the target's current thrust/screen state.

Each platform has three independently fitted sensor channels: passive, active and direction finding. Presence is separate from operation. Missiles carry passive and active sensors but no direction finding. They remain passive before terminal flight, then automatically ping; local target observations and returned echoes relay to the command ship at light speed. Other current platform defaults fit all three channels, with probe sensitivity reduced.

Established: every physical source has one persistent track per faction. Reports from different sensors and sensing modes uniquely associate with that source; ambiguous association and duplicate tracks are not simulated. Position and velocity can remain uncertain. A newer received ping replaces the previous indication for that track, without overlapping old indications.

The player commands one assigned ship: the frigate in the escort scenario. Allied ships, probes, missiles and stations are autonomous and share sensor reports at light speed, including the sensor-to-command-ship relay delay. Inspecting another platform never transfers command.

The allied lunar sensor station orbits the Moon, is drawn as a small square, carries no weapons or screens, and pings automatically once per simulation minute. Its outgoing ping circles are hidden from the player, but its received sensor reports are shared. See [REFINEMENTS.md](REFINEMENTS.md) for implemented uncertainty, local missile sensors, probe behavior and current prototype limits.

## Project principle: modern Royal Navy terminology

Established by the user, 2026-09-27: use **modern, post-Second World War Royal Navy terminology** consistently throughout the game's player-facing language and design documentation, including contacts, tracks, weapons, manoeuvring, sensors and command/status reports. Prefer contemporary usage; avoid age-of-sail, First World War and Second World War terminology unless the term remains in modern service.

- Prefer verified contemporary Royal Navy usage, with shared NATO maritime terminology where appropriate. Post-war historical sources may establish provenance, but do not by themselves establish current usage. Do not present US-only terminology or invented jargon as Royal Navy practice.
- Use **Track 1, Track 2, …** as the game's readable track designators. These simple sequential labels are a game convention, not a claim to reproduce a particular operational data-link numbering scheme.
- Keep a contact's track number, tracking quality and identification separate. A resolved position/velocity does not establish identity or hostility; a track number remains stable as knowledge improves.
- Adapt naval language to space where necessary, and document the adaptation. Prefer clear wording when a suitable Royal Navy term cannot be verified.
- Apply terminology consistently across map labels, panels, orders, weapon controls, alerts and documentation. Existing internal identifiers need not be renamed solely for vocabulary consistency.

Historical basis for track-number terminology: [Royal Navy Comprehensive Display System discussion in David L. Boslaugh's firsthand history](https://ethw.org/First-Hand:No_Damned_Computer_is_Going_to_Tell_Me_What_to_DO_-_The_Story_of_the_Naval_Tactical_Data_System,_NTDS). This supports the terminology, not a universal current Royal Navy numbering format.

## 1. Status, authority, and scope

This document consolidates the ship-combat discussion into a handoff for another agent. It is a design specification, not a claim that the game exists.

Implementation was explicitly paused. The current request authorizes writing this document, not resuming game implementation. The repository has a substantially implemented TL7 ship designer, engineering evaluator, dossier, and detection calculator. No playable game engine or deployed-probe simulation has been implemented.

The discussion has evolved beyond the earlier Claude game-design draft. This document takes precedence wherever they differ. In particular:

- Ships: **100 g** maximum acceleration.
- Probes: **500 g**, **10-tonne class**, few carried.
- Missiles: **1,500 g**, **100 kg class**, many carried; **59,958 km/s delta-v** propulsion budget.
- Missile payloads: **LRM nuclear proximity**, **SRM kinetic shotgun**.
- Screens: energy-storing, self-emitting fields with temperature, a maximum temperature, no ambient heating, and gradual activation/deactivation.
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

Active sensing on the command ship is a manually commanded single ping, not a continuous toggle. A white outline ring centred on the emission position displays the round-trip detection envelope at half light speed, fading toward the nominal 1 AU useful range. This is a display envelope, not the physical outbound signal or a hard detection cutoff. Autonomous stations and probes use their own schedules. Selecting or inspecting targets must preserve the assigned command ship.

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

Beam engagement is a standing order against a designated track: automatically fire within range and repeat on recharge until Cease fire. The prototype uses a provisional 3-light-second estimated-range envelope and 10-second recharge; missing tracks or out-of-range estimates hold fire. Already emitted pulses are not cancelled by Cease fire.

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
- Maximum acceleration 1,500 g.
- Many carried compared with probes; exact inventories and launcher capacities are unselected.
- Finite propulsion budget sufficient for a brief initial burn and some terminal maneuvering.
- Burn, cruise, terminal phases.
- Inherited launch velocity and persistent motion.

At 1,500 g, full thrust provides approximately 14.71 km/s of delta-v per second. Examples:

| Full-thrust time | Delta-v |
|---:|---:|
| 10 s | 147 km/s |
| 30 s | 441 km/s |
| 60 s | 883 km/s |

The current fuel allowance is 59,958 km/s of delta-v, doubled from 29,979 km/s: roughly 68 minutes at maximum thrust. A sustained 100 g ship accumulates the same delta-v in fifteen minutes as a 1,500 g missile in one minute. Initial geometry still determines whether it can evade before interception.

### Burn

Commit to a predicted encounter trajectory. Relatively conspicuous. Initial acceleration consumes budget that cannot also be spent in terminal correction.

### Cruise

Coast without losing velocity. Quieter, not invisible. Receive delayed updates. Any correction consumes propulsion budget; calling it cruise does not make maneuvering free.

### Terminal

Use local sensing and remaining maneuver authority. The missile must solve its own short-delay encounter, not wait for distant carrier approval. Seeker limitations, deception, fuel, and closing velocity matter.

Simple lateral correction reference:

    correctable displacement approximately 0.5 * available acceleration * remaining time²

At 1,500 g this gives approximately 735 km in 10 s, 7.35 km in 1 s, and 73.5 m in 0.1 s, before sensor delay, acceleration-direction constraints, or existing lateral velocity are considered.

At a 30,000 km/s closing speed, the last 3,000 km lasts only 0.1 s. High closing speed increases impact energy but reduces terminal recovery time.

### Retired payload: standoff nuclear-pumped laser

Removed from the playable game. The following concept is historical only; offensive magazines contain LRM and SRM rounds exclusively.

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

    dE/dt = P_captured - P_emit - P_transferred

Every transfer retains an energy destination. Moving energy into another onboard system does not dispose of it.

When background is negligible, emission at 75%, 50%, and 25% of maximum absolute temperature is respectively about 31.6%, 6.25%, and 0.39% of maximum emission. Use kelvin, not Celsius fractions.

### Cooling

For constant heat capacity, fixed area/emissivity, negligible background, and no ongoing input:

    time(T_i -> T_f) = C_s / (3 * epsilon * sigma * A_eff)
                      * (1/T_f^3 - 1/T_i^3)

There is no powered-idle heat input. During collapse, field area/capacity may change and require explicit accounting.

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

Established direction: screens have no ambient heat buildup, take time to energize, and take time to switch off. Running without screens is a valid tactic.

Working state machine:

| State | Protection | Screen emission |
|---|---|---|
| Off | None | None; ordinary ship signatures remain |
| Building | Increasing capture/storage capability | None unless carrying absorbed heat |
| Established | Rated capability subject to heat and damage | Radiation from absorbed hit energy only |
| Collapsing | Decreasing capability | Continues while stored energy is discharged |

An unhit screen stays at zero heat; raising it or leaving it raised adds none.
Absorbed energy still radiates over time and increases emissivity up to 100× at
maximum heat. Ordinary platform and thrust signatures remain separate.

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
