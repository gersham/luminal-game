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

### Missile model

Offensive missiles and interceptors use timed probabilistic engagements,
not physical closest-pass guidance. Their displayed flights follow received
tracks; terminal seekers can improve acquisition and share observations.
Each surviving round resolves once at its deadline and is removed on hit,
miss, or loss of its target. Hit probability depends on launch range, track
quality/uncertainty, target evasion, and ECM/ECCM; point defence remains a
separate layer. LRM proximity bursts and SRM direct hits retain distinct damage.
The animation is intentionally an abstraction, not a fuel-accurate trajectory.

Successful SRM direct hits deliver 2 PJ; LRM proximity hits deliver 1 PJ.
Full salvos are dangerous to ships with exhausted interceptor magazines; see
[salvo calibration](calibration/exhausted-defences.md) for controlled trials.
Own-ship map rings show LRM (1.4 AU), SRM (0.14 AU), and the nominal beam envelope
(6 light-seconds). Missile rings disappear when their magazines are empty.
Beam range is a useful engagement guide, not a hard cutoff against stationary
targets. Emission footprints and point-defence rings are no longer drawn.
Weapon circles use thin yellow dots. Ship forecasts fade toward their endpoints;
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

The raider starts at a seeded random orbital location 5–10 AU from Sol, with
circular orbital velocity and an inward burn. The transport's exit point is
independently randomized 10–20 AU from Sol. The raider reassesses its route from
received tracks: it engages an escort that can meet it before the transport,
and pursues the transport when that route is clear. Known lunar-station sensor
coverage influences its approach; it reduces emissions and skirts or waits
outside that coverage when practical, but prioritizes fighting a nearby escort.
The exit location is known to the raider from the start; without a usable target
track, it heads to that exit to intercept the escaping transport.
The escort wins when the transport escapes or the raider is destroyed; the
raider wins if the transport is destroyed first.
The transport has 25 g nominal acceleration, one quarter of a warship's 100 g.

The playtest starts on **AUTO speed**, tracking your selected frigate, with a bearing-only
enemy contact designated. Use the top-left controls to pause, change speed, fit
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
| E / R / B | Cycle ECM / Screens / Boost: Auto → On → Off |
| A | Toggle automatic active pinging: Off / Auto |
| 1 / 2 / 3 | SHORT / MEDIUM / LONG separation |
| 0 | EVADE |
| Drag with left or middle mouse button | Pan; cancels ship tracking |
| Mouse wheel | Zoom |
| Right-click a friendly | Join 1 LS alongside at maximum burn, then match its motion and burn |
| Shift+right-click map | Append a point to your ship's fly-through curve |
| Click a contact | Designate it; defaults to MATCH manoeuvre |
| Hover an object | Inspect details |

The bottom deck contains weapon controls, your ship's condition, a central
Ping/EF/system-control stack, the target's last observed condition, and manoeuvre orders.

Right-click a friendly to join a formation 1 LS to the nearer side of its current
course. This restores full drive authority for the approach, then matches its
reported burn while correcting formation drift. The orders panel shows the
friendly, separation, relative speed and follow status. Routes and destinations
also display their navigation progress there, with cancel/coast and stop controls.

Yellow rear vectors show burn strength: 120g is four ship-icon lengths, fading
out toward the tip. Automatic zoom waits ten seconds after manual zoom and eases
changes over roughly ten seconds of real time.

Shift+right-click a series of map positions to draw a flight curve. You can pause
while plotting. The ship follows it as closely as acceleration and collision
avoidance allow, then coasts beyond the last point. A new manoeuvre or manual
thrust replaces the route; adding points preserves your weapon target.

- **PING:** send one active sensor pulse. Returns arrive after the round-trip
  light delay. Successful returns grant Identity, including condition, for 60 seconds.
  Reach is the EF-scaled Approximate envelope, reduced by damage and opposing ECM.
- **ACTIVE Auto/Off:** defaults Off. Auto repeats every 60 seconds until explicitly
  switched Off, even without contacts. Manual Ping is independent.
- **ECM On/Off/Auto:** defaults Auto; emits while a resolved enemy ship is known.
  Ratings default to ECM 100 and ECCM 50. Net advantage is target ECM minus observer
  ECCM, scaled by system health. Resolution reduction is `min(50%, net/(50+net))`
  for positive net advantage; otherwise zero. Bearing/Approximate reach is unaffected,
  and ECM still increases EF by 50%.
- **SCREENS On/Off/Auto:** defaults Auto and latches on after resolving an enemy ship.
  The scenario retains its initially raised warship screens. Raising and lowering
  each take 60 seconds at full effectiveness. Stored absorption heat blocks lowering;
  an Off request waits without discarding stored energy.
- **BOOST On/Off/Auto:** defaults Auto. Boost adds 20% to full thrust and pauses main
  and point-defence laser recharge; existing charged shots remain available. Auto
  stops boosting when a received ship/missile position is within 10 light-seconds.
- **LRM / SRM:** click to queue a launch. Long-range nuclear proximity missiles
  allow speculative bearing-only shots; short-range kinetic missiles need a
  minimally useful firing solution. Launch intervals are 60 s and 5 s respectively.
  The line below each button estimates hit chance before enemy defences.
- **Main beam AUTO / DIRECT / HOLD:** automatic engagement, directed engagement
  of the selected contact, or cease fire. Point defence operates automatically.

### Manoeuvres

| Order | Behaviour |
| --- | --- |
| MATCH | Come alongside and match velocity |
| FLYBY | Accelerate for a high-speed pass without matching velocity |
| LONG · LRM | Hold 0.7 AU: half the LRM engagement envelope |
| MEDIUM · SRM | Hold 0.07 AU: half the SRM's 0.14 AU engagement envelope |
| SHORT · BEAM | Hold 2 LS: inside the beam knife-fight envelope |
| EVADE | Burn away from the contact |

LONG, MEDIUM and SHORT approach a fresh bearing-only contact until a range fix is
available, then brake or withdraw to hold the requested separation. MATCH is the
default on target selection and restart: it approaches a fresh bearing, then
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

EF is `(1 + thrust%/100) × (1 + screen heat%/10) × (2 if screens active) ×
(size/10) × (1 - stealth/100) × (1.5 if ECM emitting) ×
(1.2 if missiles fired in the last minute) × (1.5 if beams fired in the last minute)`.
An additional platform visibility multiplier scales the entire EF: normally ×1,
but ×2 for the transport. Frigates use size 7; battleships 20; stations 20;
other classes provisionally 10. Point-defence
fire contributes. Boosted thrust can reach 120%.

Multiply base sensing ranges by **target EF / 1.4**: passive Identity 0.01 AU,
Resolved 0.1 AU, Approximate 2 AU, Bearing 10 AU; active ping Identity has a
separate 1 AU baseline. EF 1.4 is a size-7, 50%-stealth frigate at full nominal
thrust with raised, cold screens and ECM off. ECM and sensor damage still reduce
effective ranges. Approximate contacts have a biased ellipse
and estimated motion; resolved contacts gain class identity. Direction finding
requires operational DF and screens, thrust, or recent weapons on the target.
ECM alone does not qualify. Missiles are invisible to direction finding, including
their seeker pings. Map bearing spikes show only the command ship's measurements.
Damaged sensors halve their range. Ping identity expires
60 seconds after original sensor receipt, not after an allied relay.
Learned class identity remains; condition reports become historical.

Damaged power disables propulsion, active sensors, screens and weapons. Passive
sensors, direction finding and the ship mind have backup power; crew and damage
control remain operational. Destroyed power or an exhausted hull destroys a ship.

Damage control repairs one damaged system per **two effective minutes**, with
power first, then damage control itself. A progress line and hover text show the
current repair. Crew and damage-control damage slow repairs. Destroyed systems
cannot be repaired. Hull repair is separate: 1% per ten effective minutes.

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
