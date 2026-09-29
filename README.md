# Luminal

### Sound

The client includes subtle generated interface, sensor, launch, beam, impact,
explosion and alert effects, plus a beatless space-operatic ambient loop.
The top-left inset has separate effects/music volume sliders and a global
mute. Defaults are deliberately quiet (35% effects, 25% music). Combat sounds
follow the player's light-delayed picture and are throttled in real time at
high simulation speeds. Audio hardware is optional; the game works silently
if no output device is available. Linux source builds require ALSA development
headers (`alsa-lib` on Arch, `libasound2-dev` on Debian/Ubuntu).

### Jump drives

Destroyers, cruisers and battleships have a **JUMP DRIVE** helm button. Select it,
then left-click any destination within 50 AU of Sol; the map switches to a crosshair.
Escape or right-click cancels destination selection. A jump spools for 600 simulation
seconds, with a blinking triangle and countdown. During spooling, thrust, evasion,
screens, beam weapons and PD lasers are disabled. **CANCEL JUMP** aborts the spool;
screens rebuild from zero under their existing control mode, and accumulated heat remains.
Missiles and interceptor missiles remain usable during the spool. The jump drive
is a damageable installed component: damage or destruction prevents use, and a
jump-drive or power failure aborts an active spool. Repair permits a fresh spool.
Damaged propulsion or power also disables ordinary thrust. Damaged screens or
power cap screen charge at 50% (not 25% when both are damaged); recharge continues
at its normal rate. Repair restores the cap to 100%, followed by normal recharge.

Transit takes distance / (1 AU/s), cannot be cancelled, and preserves the ship's
normal-space velocity at departure. The ship is absent from normal space in transit;
its UI marker shows jump progress. Arrival resumes ballistic motion, with screens
recharging from zero. Jump is the game's only faster-than-light movement; sensor
information still travels at light speed, including old images of the departure.

Initial heat balance: spool input is **4× full-thrust drive heat**, scaled by class
thermal capacity, and arrival adds **100% of class heat capacity**. This intentionally
leaves a very hot ship whose normal thermal limits can inhibit thrust and lasers
after arrival. Heat dumping is unavailable during the jump sequence. These tuning
constants live in `crates/luminal-core/src/jump.rs`.

### Missile model

Offensive missiles inherit their launcher's velocity and physically accelerate,
coast and steer using received tracks. A hit roll occurs only after the missile
actually reaches its target's proximity envelope. Running away consumes its time
and fuel; launching aft into a closing pursuer benefits from relative motion.

| | LRM | SRM |
|---|---:|---:|
| Nominal range circle | 1.4 AU | 0.14 AU |
| Acceleration | 1,500 g | 3,000 g |
| Propulsion budget | 55,000 km/s | 40,000 km/s |
| Reactor lifetime | 120 min | 22 min |
| Terminal acquisition | 0.1 AU | 0.04 AU |
| Hit energy | 0.3 PJ | 0.25 PJ |
| Launcher cycle | 10 s | 5 s |

A stationary LRM shot at 1.4 AU boosts over approximately 0.4 AU, coasts over
0.9 AU and powers through the final 0.1 AU, taking about 107 minutes. Its baseline
hit roll is 75% before uncertainty, ECM and defence. SRMs have no cruise phase.
Range circles are nominal envelopes: inherited velocity, target motion, fuel and
reactor expiry determine actual reach. Fuel figures are acceleration integrals,
not instantaneous speeds; ordinary flight remains relativistic.

SRMs and terminal LRMs are always resolved. An LRM acquired during boost stays
tracked during cruise. Otherwise its cold cruise is difficult to acquire, so
interceptors may have no target until a ping or terminal acquisition reveals it.
Operational passive or active sensors identify ships inside the 0.14 AU SRM
envelope regardless of cold signature or ECM; reports still obey light delay.
Pings automatically resolve LRMs within their effective detection radius once
the echo returns, and extend ship resolution from 1 AU to 2 AU at full sensor
strength. A fresh resolved ship/station echo supplies missile fire control:
40% of the remaining hit-roll failure chance is removed (75% becomes 85%).
Support lasts 60 seconds after receipt and fades over 15 seconds. The weapon
card marks supported estimates with PING. Pinging also reveals the transmitter.

Auto-evade uses received missile motion and class performance to estimate whether
a dodge would demand a meaningful share of its correction reserve. It accounts
for upcoming missile acceleration, rejects low-payoff close dodges, and requires
more benefit when hot. Worthwhile burns commit to a lateral or oblique direction;
minor seeker corrections do not flip the ship back and forth. Standing helm
orders resume when no worthwhile threat remains. This is an estimate, not access
to the enemy's remaining fuel or a guarantee of escape.

Large hulls concentrate missiles into wider volleys; magazine and launcher counts
share one class loadout table in `ShipClass::missile_fit`.

| Class | SRM rounds / launchers | LRM rounds / launchers |
|---|---:|---:|
| Picket | 20 / 1 | — |
| Frigate | 12 / 1 | 10 / 1 |
| Destroyer | 24 / 4 | 18 / 3 |
| Cruiser | 24 / 6 | 36 / 6 |
| Battleship | 48 / 12 | 48 / 8 |

Full salvos are dangerous to ships with exhausted interceptor magazines; see
[salvo calibration](calibration/exhausted-defences.md) for controlled trials.
Own-ship map rings show LRM (1.4 AU), SRM (0.14 AU), and the nominal beam envelope
(6 light-seconds). Missile rings disappear when their magazines are empty.
Beyond the nominal 6 LS beam range, automatic fire requires at least 1 TJ
and 5% of emitted energy expected to couple, with transverse aim uncertainty
no wider than twice the beam radius. Coupling tapers smoothly from 6 to 10 LS, reaching zero at 10 LS.
Explicit directed fire beyond that wastes heat and power. Spinal mounts retain their separate 60 LS envelope. Emission footprints and point-defence rings are no longer drawn.
Weapon circles use thin yellow dots. When their on-screen radius exceeds 320 px
(and at least six label widths), small interior labels repeat every 30 degrees, curve along the circumference,
face inward, and use 50% alpha. Ship forecasts fade toward their endpoints;
red dashed target links show separation in LS or AU. Observed missile and
interceptor hits bloom red, while misses fade out over one real-time second.
Missed rounds keep coasting during the fade at their last received velocity and
the animation's starting time warp. Autozoom includes your target's selected
opponent as well as your own ship and target. The camera is allowed to know that
target relationship for convenience; plotted positions still use your tactical
picture, and the AI receives no extra targeting information.

**Free flight:** Left/Right turn; Up/Down adjust throttle (0–100%). Any arrow
cancels the current automatic manoeuvre or move-to order, retaining the weapon
target and camera tracking. Tap for 3°/5% adjustments; hold for smooth control.
At zero throttle the ship coasts; changing heading does not cancel velocity.
Navigation orders restore autopilot. Control rates use real time, not time warp.
The navigation readout shows free-flight mode and commanded throttle.

The combat log reports observed ship destruction (including the player's ship),
but suppresses missile/interceptor destruction messages. Enemy reports arrive
only after light travel and require a classified ship contact.

A native, top-down space-combat prototype about fighting across light-seconds and
astronomical units. Command a frigate, hunt uncertain contacts, launch missiles,
and manage a ship that can lose individual systems before its hull gives out.

Information travels at light speed: the map shows what your ship has observed,
not an omniscient live view. Long-range contacts may be bearings only; active
pings improve the picture but reveal your presence.

**Status:** an actively changing playtest, developed and tested on Linux.
Balance and interface details are experimental. No packaged releases yet.

## Install and run

**Omarchy:** see [the Omarchy installation guide](docs/OMARCHY.md) for a native
fullscreen game with an application launcher and icon. From an existing checkout,
run `./scripts/install-omarchy.py` after installing the listed dependencies.

### Sol backdrop

The map contains all eight planets, Earth's Moon, Phobos and Deimos, the four
Galilean moons, Titan/Rhea/Iapetus, Titania/Oberon, and Triton. Sizes and orbital
distances are rounded real-world values, with simple coplanar circular orbits
for flavour—not a precision ephemeris. A shared randomized starting epoch sets
their phases. All celestial positions remain frozen throughout the scenario;
ships still experience gravity and can orbit them normally. Restart reshuffles
the layout. Set `LUMINAL_SEED=42` when launching to reproduce a particular game;
the seed is also written to `logs/latest.log`.

### Build

You need Git, a Rust toolchain, a native C/C++ build toolchain, and a desktop
session with working OpenGL drivers. The current build is tested with **Rust
1.98.0**. Install Rust with [rustup](https://rustup.rs/), then install that toolchain:

```sh
rustup toolchain install 1.98.0
```

On Debian/Ubuntu, these packages provide common native build and windowing
dependencies:

```sh
sudo apt install git build-essential pkg-config libx11-dev libxi-dev \
  libxcursor-dev libxrandr-dev libxinerama-dev libxkbcommon-dev \
  libwayland-dev libegl1-mesa-dev libgl1-mesa-dev
```

On Arch Linux:

```sh
sudo pacman -S --needed git base-devel pkgconf libx11 libxi libxcursor \
  libxrandr libxinerama libxkbcommon wayland mesa
```

Clone and start the desktop game:

```sh
git clone https://github.com/gersham/luminal-game.git
cd luminal-game
cargo +1.98.0 run --locked --release -p luminal-app
```

The first build downloads dependencies and may take several minutes. Subsequent
launches can use the built executable directly:

```sh
./target/release/luminal-app
```

Alternatively, install the executable into Cargo's binary directory:

```sh
cargo +1.98.0 install --locked --path crates/luminal-app
luminal-app
```

Ensure `~/.cargo/bin` is on your `PATH` if using the installed command. A graphical
Wayland or X11 session is required; the desktop app is not a headless server.
Windows and macOS installation have not been validated.

## Playing

The scenario gives you control of the escort frigate. A transport heads for its
departure region, an enemy warship threatens it, and an allied lunar station is
present. Other platforms are autonomous. Probes are currently disabled. The lunar
station has passive, active, and direction-finding sensors, with autonomous active
pings every 60 seconds and light-delayed reports to the escort.

The transport's exit point is randomized 6–10 AU from Sol. The raider starts
at rest in the Sol frame, uniformly placed in the union of the region within
5 AU of Sol and an ellipse from Sol to 1 AU beyond the exit, 4 AU wide at its
widest. An exclusion
circle forbids starts within 1 AU of Earth's initial position. These placement
boundaries are not shown on the map. The raider reassesses its route from
received tracks: it engages an escort that can meet it before the transport,
and pursues the transport when that route is clear. Known lunar-station sensor
coverage influences its approach; it reduces emissions and skirts or waits
outside that coverage when practical, but prioritizes fighting a nearby escort.
The exit location is known to the raider from the start; without a usable target
track, it heads to that exit to intercept the escaping transport.
The escort wins when the transport escapes or the raider is destroyed; the
raider wins if the transport is destroyed first.
The transport has 25 g nominal acceleration, one quarter of a warship's 100 g.

The playtest starts on **AUTO speed**, tracking your frigate with the transport
selected and a follow order to join it 1 LS alongside. Use the top-left controls to pause, change speed, fit
the map, or restart. AUTO smoothly ranges from 5× at 1 LS through 10× at 10 LS,
50× at 0.1 AU and 300× at 1 AU to 1000× at 2 AU. It uses the nearest received
enemy track, allowing for uncertainty and two wall-seconds of projected closure.
Bearing-only search uses 100×; no contacts uses 1000×. Acceleration is gradual,
deceleration quicker, and pause freezes the speed. AUTO ramps from 5× at startup.
Selecting 1×, 10×, 50×, 100× or 1000× switches to manual speed; combat events
never override manual speed. Restart restores AUTO.

| Control | Action |
| --- | --- |
| Space | Pause / resume |
| F / FIT | Fit the map |
| T | Track your frigate (default), smoothly auto-zooming to include the selected ranged target |
| L / S | Queue an LRM / SRM at the designated contact (same launch gates as buttons) |
| Shift+L / Shift+S | Queue all remaining LRMs / SRMs at that target; normal launch intervals still apply |
| P | Active sensor ping |
| E / R | Cycle ECM / Screens: Auto → On → Off |
| A | Toggle automatic active pinging: Off / Auto |
| 1 / 2 / 3 | SHORT / MEDIUM / LONG separation |
| 0 | EVADE |
| Drag with left or middle mouse button | Pan; cancels ship tracking |
| Mouse wheel | Zoom |
| Right-click a friendly | Join 1 LS alongside at maximum burn, then match its motion and burn |
| Shift+right-click map | Append a point to your ship's fly-through curve |
| Left-click an object | Set a persistent target; leaves movement unchanged |
| Right-click an enemy ship | Move toward that ship using the current helm mode |
| Right-click empty space / celestial | Fly to the point / orbit the body |
| Hover an object | Inspect details |

The bottom deck contains weapon controls, your ship's condition, a central
Ping/EF/system-control stack, the target's last observed condition, and manoeuvre orders.

Right-click a friendly to join a formation 1 LS to the nearer side of its current
course. This restores full drive authority for the approach, then matches its
reported burn while correcting formation drift. The orders panel shows the
movement destination, separation and status. The helm modes are mutually exclusive;
selecting a mode updates the current movement order without changing the weapon
target. ALONGSIDE also works on enemies using the received sensor estimate,
coasting if the track is lost. Routes and destinations display navigation progress.
COAST cancels the order. Selecting another weapon target never redirects the helm.

Yellow rear vectors show burn strength: 120g is four ship-icon lengths, fading
out toward the tip. Automatic zoom waits ten seconds after manual zoom and eases
changes over roughly ten seconds of real time. Manual ping sweeps hold the
current autozoom until the sweep finishes; manual zoom remains available.

Shift+right-click a series of map positions to draw a flight curve. You can pause
while plotting. The ship follows it as closely as acceleration and collision
avoidance allow, then coasts beyond the last point. A new manoeuvre or manual
thrust replaces the route; adding points preserves your weapon target.

- **PING:** send one active sensor pulse. Returns arrive after the round-trip
  light delay. Successful returns grant Identity, including condition, for 60 seconds.
  Base reach is 5 AU, reduced by damage and opposing ECM. Distant returns give
  approximate positions; close returns resolve position.
- **ACTIVE Auto/Off:** defaults Off. Auto repeats every 60 seconds until explicitly
  switched Off, even without contacts. Manual Ping is independent.
- **ECM On/Off/Auto:** defaults Auto; emits while a resolved enemy ship is known.
  Ratings default to ECM 100 and ECCM 50. Net advantage is target ECM minus observer
  ECCM, scaled by system health. Resolution reduction is `min(50%, net/(50+net))`
  for positive net advantage; otherwise zero. Bearing/Approximate reach is unaffected,
  and ECM still increases EF by 50%.
- **SCREENS On/Off/Auto:** defaults Auto and latches on after resolving an enemy ship.
  Screens recharge at 0.2% per minute; Off immediately removes absorption.
  Absorbed damage and a small idle load heat the shared reservoir.
- **LRM / SRM:** click to queue a launch. Long-range nuclear proximity missiles
  allow speculative bearing-only shots; short-range kinetic missiles need a
  resolved target. Launch intervals are 10 s and 5 s respectively.
  The line below each button estimates hit chance before enemy defences.
- **Main beam AUTO / DIRECT / HOLD:** automatic engagement, directed engagement
  of the selected contact, or cease fire. Point defence operates automatically.

### Manoeuvres

| Order | Behaviour |
| --- | --- |
| ALONGSIDE | Join at a fixed 1 LS offset and mirror observed motion; friendly or enemy |
| INTERCEPT | Close and match velocity |
| FLYBY | Accelerate for a high-speed pass without matching velocity |
| LONG · LRM | Hold 0.7 AU: half the LRM engagement envelope |
| MEDIUM · SRM | Hold 0.01 AU (about 5 LS): close-range shotgun combat |
| SHORT · BEAM | Hold 2 LS: inside the beam knife-fight envelope |
| EVADE | Maximum lateral burn against incoming missiles; coast when clear |

LONG, MEDIUM and SHORT approach a fresh bearing-only contact until a range fix is
available, then brake or withdraw to hold the requested separation. INTERCEPT approaches a fresh bearing, then
brakes and matches velocity once a ranged estimate is available. FLYBY requires
a fresh ranged track. Stale evidence causes coasting. Ships still
retain momentum with their engines off.

### Damage and information

Warships have 1,000 hull points, ablative armour, screens, and discrete systems.
Green systems are intact, orange damaged, and red destroyed. Grey means a known
power/dependency outage; blue-grey means unknown or stale, **not confirmed disabled**.
Target cards contain historical sensor reports, so they can lag visible actions.

A hit pushing stored screen absorption beyond rated capacity destroys the screen
generator, damages 1–3 other distinct installed systems, and removes 20% of maximum
hull, in addition to ordinary penetrating damage. A second hit on a damaged system
destroys it. Generator destruction vents the field and cannot trigger overload again.

### Emissivity and sensing

EF is `(1 + thrust%/100) × (1 + 10 × heat/thermal-limit × dump-signature) ×
(size/10) × (1 - stealth/100) × (1.5 if ECM emitting) ×
(1.2 if missiles fired in the last minute) × (1.5 if beams fired in the last minute)`.
An additional platform visibility multiplier scales the entire EF: normally ×1,
but ×2 for the transport. Frigates use size 7; battleships 20; stations 20;
other classes provisionally 10. Point-defence
fire contributes. Heat and damage limit class-rated thrust.

Multiply base sensing ranges by **target EF**: passive Identity 0.1 AU,
Resolved 1 AU, Approximate 5 AU, Bearing 20 AU; active ping detection has a
separate 5 AU baseline, multiplied by the greater of 1 and target EF. A cold size-7, 50%-stealth frigate at full nominal thrust has EF 0.7
with ECM off. Screens no longer multiply EF. ECM and sensor damage still reduce
effective ranges. Approximate contacts have a biased ellipse
and estimated motion; resolved contacts gain class identity. Approximate passive
and active positions share a fixed random x/y offset per contact. Their ellipse
includes measurement and velocity uncertainty plus movement possible at 120g
since the light left the target (including relay delay). Repeated reports cannot
average away that offset. Resolved positions remove the ellipse and its offset.
Missiles launched at an ellipse aim at its center; their own resolved seeker
fix can correct the course only within acceleration and remaining correction
budget. A missile that cannot reach the target misses. Direction finding
requires operational DF and excess heat, thrust, or recent weapons on the target.
ECM alone does not qualify. Missiles are invisible to direction finding, including
their seeker pings. Map bearing spikes show only the command ship's measurements.
Damaged sensors halve their range. Ping identity expires
60 seconds after original sensor receipt, not after an allied relay.
Learned class identity remains; condition reports become historical.

Damaged propulsion disables thrust. Damaged power disables thrust, jump, active sensors
and weapons, while capping screens at 50%. Passive
sensors, direction finding and the ship mind have backup power; crew and damage
control remain operational. Destroyed power or an exhausted hull destroys a ship.
Subsystem critical probability scales smoothly with penetrating energy before
armour shares the damage: `1 - (1 - base_chance)^(damage / 1% maximum hull)`.
Base chance is 20%, rising to 40% below half hull and 80% below quarter hull.
At healthy hull, a 0.5% penetration has a 10.6% chance; 1% has 20%, and 2% has 36%.
Tiny grazes have correspondingly tiny chances; zero penetration has none. Missile screen punctures pass 25% of their energy
on a 35% roll; only a sufficiently damaging puncture guarantees a component shock.

Damage control repairs one damaged system per **twenty effective minutes**, with
power first, then damage control itself. A progress line and hover text show the
current repair. Crew, mind and damage-control damage slow repairs. Destroyed systems
cannot be repaired. Hull repair is separate: 1% per effective hour.

A destroyed ship mind disables damage control, propulsion, active sensors, ECM/ECCM,
screens and all weapons. Surviving crew retain backup passive and direction sensors.
A surviving mind can operate combat systems without crew, but cannot repair the ship.
With both crew and mind destroyed, every system is inactive and shown grey: a
lifeless hulk coasts without sensors, weapons, reactor charging or repairs.

The current matched-class balance results, commands, failure investigations, and
remaining limits are in [the September combat report](calibration/2026-09-28/report.md).

## Development and simulations

The repository is a Rust workspace:

- `luminal-core`: deterministic simulation, sensing, guidance, combat and damage.
- `luminal-app`: native egui/eframe client consuming faction-specific views.
- `luminal-cli`: headless scenarios and balance trials.

```sh
# Development client
cargo +1.98.0 run --locked -p luminal-app

# Tests and lint checks
cargo +1.98.0 test --locked --workspace
cargo +1.98.0 clippy --locked --workspace --all-targets -- -D warnings

# Five stationary frigate battles: interceptor stock 30,
# separation 0.002 AU, second ship's reaction delay 10 seconds
cargo +1.98.0 run --locked --release -p luminal-cli -- \
  --frigate-battle 5 30 0.002 10

# Cold, thrusting, screened and hot signatures across five ranges
cargo +1.98.0 run --locked --release -p luminal-cli -- --sensor-sweep
```

CLI trials write CSV results to standard output. `calibration/` contains historical
balance experiments; their results may predate current tuning.

The desktop game writes combat and action diagnostics to `logs/latest.log` in its
working directory. Starting a new logged game replaces the previous log. Logs and
`target/` build outputs are excluded from Git.

See [Game mechanics](GAME_MECHANICS.md), [Architecture](ARCHITECTURE.md),
[Refinements](REFINEMENTS.md), and [TODO](TODO.md) for implementation notes and
design history. Some older sections describe earlier experiments or future plans;
the current-playtest section in the mechanics document takes precedence.

## Licence

No open-source licence has been granted. The workspace is currently marked
`UNLICENSED`; public visibility does not grant additional reuse or redistribution
rights.

Speeds above 0.01c display as fractions of light speed. All physical trajectories
are limited to 0.99c, including coasting and weapon courses; continued thrust at
the cap can turn the ship without accumulating excess momentum.
Flyby burns directly toward the estimated target at full available thrust without
approach braking. Evade prioritizes the earliest incoming missile's predicted
close approach and burns laterally at maximum available thrust, continually
reassessing threats until none remain, then coasting. It uses received tracks.
Beam effects connect displayed shooter and target positions. Point-defence lasers
engage inside 3 LS and fire repeatedly. Per-pulse odds are calibrated to about
50% cumulative interception over a full unsaturated approach, using observed
closing speed and the fitted battery cadence. Warship mounts recharge every
five seconds; larger batteries handle more simultaneous threats. Partial
coverage, saturation, heat and damage reduce interception. Both SRMs and LRMs
can be engaged before their respective burst ranges.
The escort scenario ends in defeat if the player's frigate is destroyed, and in
victory if the raider is destroyed. Transport arrival or destruction alone does
not end this scenario.

### Shared heat and radiators

Ships store heat in joules. The center command panel shows whole-number SI units
(J, kJ, MJ, GJ, TJ, PJ, EJ and larger) beside the current emissivity factor.
Internal energy accounting retains fractional joules.

Drive heat is quadratic in actual thrust: a 100% burn generates 100 times the heat
of a 10% burn. From cold, the rated maximum (120g for a frigate, 50g for a
transport) reaches the 100 PJ throttling threshold in about 3.1 hours, including passive
cooling. Baseline cooling offsets 50% rated thrust plus enabled screens for every class. A one-hour
full burn from cold stores 50 PJ. Above this threshold available thrust progressively falls, reaching zero
at 150 PJ. Coasting preserves velocity while cooling; the 0.99c ceiling remains.
Normal cooling combines that class-scaled baseline capacity with a two-hour reservoir time constant. At or below 50% thrust with screens on, a cold ship stays cold and a hot ship cools without dumping. The central gauge shows only net
heat flow: green cooling or red heating, beginning at the shared top of the gauge.
Fill uses a logarithmic 1 MW–1 PW scale, so small but real net rates remain visible.

Main beams and point-defense lasers share the heat budget and hold fire at its
150 PJ ceiling. Absorbed shield damage enters the ship heat reservoir immediately. Screens are
rechargeable hit-point capacity, not a separate thermal reservoir. Heat remains visible after engines stop; screens have no direct
signature multiplier. Enabled screens add 1% of rated full-drive heat, including
at full charge. They regenerate 0.2 percentage points of capacity per minute and
show 0% immediately when off or disabled. Routine navigation reserves heat headroom during
acceleration for braking and defence; Flyby and Evade use all thermally available
thrust. Manual commands also obey thermal limits.

**DUMP HEAT** toggles radiators for five times the normal cooling and 10 times normal thermal signature, cuts thrust and inhibits both main and point-defense lasers. The large exposed-radiator signature is a gameplay multiplier and falls as the ship
cools. Click **STOP DUMP** to close them and resume the retained movement and weapon orders.
The toggle remains on even after the ship has cooled.
Dumping is a visible tactical choice, not an automatic autopilot action. Sensor
reports of the increased signature still arrive at light speed.

The transport cruises toward departure at up to 25g. Once its delivered sensor
picture resolves an enemy ship, it stays in escape mode with a 50g ceiling toward
the destination. It still brakes for arrival and obeys thermal throttling; losing
the contact does not cancel escape mode. Its 50g thermal rating makes a 25g cruise
generate one-quarter of maximum drive heat.

**EVADE AUTO/OFF** (V) watches received tracks for closing, potentially damaging
missiles. AUTO is enabled by default. It temporarily substitutes a full lateral
burn, leaving the existing manual burn, destination, follow or route order intact;
when the threats clear it resumes that order. A new order issued during evasion
becomes the order to resume. Interceptor tracks do not trigger this response.
Heat dumping overrides evasion and keeps the drive off.

Missile evasion now works through physical displacement and fuel demand. The
terminal hit roll does not apply an extra flat evasion penalty after the missile
has already reached proximity. UI estimates still discount difficult crossing
motion, most strongly near nominal range. See the missile model above and the
[balance report](calibration/2026-09-28-rebalance/report.md) for measured outcomes.

Boost and terminal steering use the main engine; cruise correction is limited to
20% of rated acceleration and its allocated correction fuel. All offensive hits
require a swept close pass. SRMs require resolved targets at launch and acquire
terminal solutions inside 0.04 AU; their shotgun burst envelope remains 5,000 km.
LRMs can search approximate ellipses or bearings and acquire terminal solutions
inside 0.1 AU; their strike radius remains 29,979 km. Seeker reports travel at light
speed. Ship ping resolution reaches 2 AU at full strength before rating/ECM scaling.


### Ship selection and volleys

The deployment dialog chooses your ship and an equal raider before simulation starts.
Max G is the normal class-rated ceiling, limited by heat and damage. Rotation is
measured in simulation time for a 180-degree turn; attitude is independent of the
navigation thruster vector. Hull-mounted spinal fire must wait for alignment.

| Class | Hull | Armor | SRM / LRM magazine | SRM / LRM launchers | Interceptors | Max G | Turn |
|---|---:|---:|---:|---:|---:|---:|---:|
| Picket | 500 | 250 | 20 / 0 | 1 / 0 | 2 | 150 | 5s |
| Frigate | 1000 | 500 | 12 / 10 | 1 / 1 | 40 | 120 | 10s |
| Destroyer | 2000 | 1000 | 24 / 18 | 4 / 3 | 80 | 100 | 20s |
| Cruiser | 4000 | 2000 | 24 / 36 | 6 / 6 | 80 | 70 | 35s |
| Battleship | 8000 | 6000 | 48 / 48 | 12 / 8 | 120 | 50 | 60s |

Pickets have no offensive beam. Battleships add a 120-second spinal mount with
10 times its main beam energy (3 PJ), a 60 LS envelope (10 times the nominal beam band),
a broad Gaussian footprint, and a forward 2-degree firing gate. It shares beam
AUTO/DIRECT/HOLD controls and obeys power, heat dump and beam-system damage.
Main beam pulses are 75 TJ on frigates, 150 TJ on destroyers and cruisers, and
300 TJ on battleships. Battleships carry the capacitor capacity and recharge power
needed to fire their 3 PJ spinal pulse. Large ships scale their thermal and screen capacity; battleships have 12 times
frigate screen capacity and eight times the heat reservoir.

Each missile button queues a volley. Available launchers fire together, centered
on the firing bearing with 1 LS separation. Reload is 5s for SRM / 10s for LRM.
Damaged launcher banks halve volley size, rounded up; reload cadence is unchanged.
SRM and LRM banks have independent damage states. Queued volleys reserve rounds;
if damage reduces a queued volley, its unused reservation returns to the magazine.

While an escort is under Follow orders, the transport caps its departure burn at
75% of that escort's available thrust (50% while more than 2 LS from formation),
using received friendly telemetry. It retains the normal 25 G cruise / 50 G alerted
limits, and resumes independent escape when the escort leaves Follow. Follow
catch-up burns are exempt from the routine navigation heat-reserve throttle;
actual overheat and damage limits still apply.

Size signature scales with the cube root of class size: battleships are 2×
frigates at otherwise equal signature factors. Normal heat amplification is
`1 + heat_fraction` (1.65× at 65% heat), with a further 10× multiplier while dumping.
Sensor ratings are Picket 80, Frigate 100, Destroyer 125, Cruiser 160, Battleship 200.
Active/passive ranges scale by rating/100; ECM equals rating, ECCM half rating.

Resolved launchers reveal missile and interceptor launches immediately. Once either is resolved,
it remains tracked for its live flight; this gameplay exception does not expose its seeker reports.

Point-defence laser batteries scale by class: Picket 1, Frigate 2, Destroyer 4,
Cruiser 6, Battleship 8. Each warship laser fires once per five seconds with an independent recharge
clock. Each laser stays assigned to one locally detected missile through reload,
prioritizing the closest unassigned threat when it becomes free. Larger batteries
therefore defend against more simultaneous arrivals. Every shot adds
heat, and heat dumping, thermal limits, and subsystem damage still constrain fire.

### Fire-control solutions

The full-height fire-control panel shows separate LRM, SRM and beam cards.
Missiles show estimated hit chance before defence/ECM, arrival time, energy per
hit, available ammunition, effective volley size, queue and reload status.
Beams show predicted coupled energy, coupling fraction and transverse aim
uncertainty using the same prediction as automatic fire control. These are
estimates from received tracks; unknown values stay unknown. PD shows laser
heat/dump inhibition separately from interceptor stock. The first resolved enemy
ship is selected when there is no hostile target, without changing movement orders.

Each interceptor gets one burst against one missile and is spent on hit or miss.
A defending ship may launch a fresh interceptor at a surviving missile.
Interceptor guidance fits acceleration from received observations and leads the
powered trajectory, including motion during light delay. It must still reach the
100 km kill envelope with its limited correction fuel before rolling for a kill.
See [the interceptor calibration](calibration/2026-09-28-interceptors/report.md)
for accelerating-target tests and SRM salvo comparisons.
The [broader balance review](calibration/2026-09-29-review/report.md) covers
10,240 isolated missile shots, class matchups, defensive ammunition, evasion,
active sensors, alternate tactics and mission pacing, with reproducible runners
and raw results. The recommended Cruiser (80) and Battleship (120) interceptor
magazines are now fitted; see the [follow-up validation and pacing proposals](calibration/2026-09-29-capital-fits/report.md).

### Endgame and jumps

Ships now evaluate whether they can still fight or repair an escape route.
Losing AI prefers a jump withdrawal over destruction, or surrenders when it
cannot recover an escape. **Withdraw (Jump)** concedes the objective only after
the vulnerable ten-minute spool completes and removes the ship from the battle.
**Surrender** concedes immediately. Opponents receive these announcements and
outcomes after light delay.

Choose **Automatic / Fight / Escape** repair goals in the ship panel. Power and
damage control remain first priorities, and changing goals preserves ongoing
repair work. **Next Tactical Event** advances the normal simulation until a
received tactical change, repair, jump or result, with a 24-hour limit.

Jump spooling has a sustained blue halo. Departure and arrival create large
blue blooms lasting four real seconds; withdrawing ships have a departure
bloom only. See the [endgame validation](calibration/2026-09-29-endgame/report.md)
for behavior, screenshots, regression coverage and the 20-mission survey.

### Fleet vocabulary

The startup screen uses selectable WWII-style naval recognition cards with live
class stats. Choose Luminal, Grim Dark, Imperium or Culture (the default) vocabulary
before deploying. Theme choices change display terms only; classes, weapons,
flight rules and balance are identical. Choices survive scenario restart.
See [theme vocabulary and future styling ideas](docs/THEMES.md).
For previews, `LUMINAL_THEME=Culture` selects a vocabulary and
`LUMINAL_SHIP_SELECT=1` includes startup in the screenshot workflow.

Class-specific pulse batteries, canister effects and the frigate's optional
support projector are described in [weapon fits and fleet roles](docs/WEAPON_FITS.md).
