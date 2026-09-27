# Luminal

A native, top-down space-combat prototype about fighting across light-seconds and
astronomical units. Command a frigate, hunt uncertain contacts, launch missiles,
and manage a ship that can lose individual systems before its hull gives out.

Information travels at light speed: the map shows what your ship has observed,
not an omniscient live view. Long-range contacts may be bearings only; active
pings improve the picture but reveal your presence.

**Status:** an actively changing playtest, developed and tested on Linux.
Balance and interface details are experimental. No packaged releases yet.

## Install and run

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
present. Other platforms are autonomous. Probes are currently disabled, and the
station currently has direction finding only.
The transport has half the nominal acceleration of a warship.

The playtest starts at **50× speed**, with your frigate selected and a bearing-only
enemy contact designated. Use the top-left controls to pause, change speed, fit
the map, or restart. Combat events do not automatically change game speed.

| Control | Action |
| --- | --- |
| Space | Pause / resume |
| F / FIT | Fit the map |
| T | Toggle camera tracking of your frigate; preserves zoom |
| L / S | Queue an LRM / SRM at the designated contact (same launch gates as buttons) |
| P | Active sensor ping |
| 1 / 2 / 3 | SHORT / MEDIUM / LONG separation |
| 0 | EVADE |
| Drag with left or middle mouse button | Pan; cancels ship tracking |
| Mouse wheel | Zoom |
| Click a contact | Designate it; defaults to LONG manoeuvre |
| Hover an object | Inspect details |

The bottom deck contains your weapon controls, your ship's condition, the target's
last observed condition, and manoeuvre orders.

- **PING:** send one active sensor pulse. Returns arrive after the round-trip
  light delay. The reference suite detects out to about 1 AU and resolves within
  half that range. Outlying contacts remain bearings rather than identified ships.
- **SCREEN ON/OFF:** toggle defensive screens. They store intercepted energy as
  heat and radiate it away, increasing your signature.
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
| LONG | Seek a separation of 3 AU |
| MEDIUM | Seek a separation of 1 AU |
| SHORT | Seek a separation of 0.03 AU |
| EVADE | Burn away from the contact |

LONG, MEDIUM and SHORT approach a fresh bearing-only contact until a range fix is
available, then brake or withdraw to hold the requested separation. MATCH and
FLYBY require a fresh resolved range. Stale evidence causes coasting. Ships still
retain momentum with their engines off.

### Damage and information

Warships have 1,000 hull points, ablative armour, screens, and discrete systems.
Green systems are intact, orange damaged, and red destroyed. Grey means a known
power/dependency outage; blue-grey means unknown or stale, **not confirmed disabled**.
Target cards contain historical sensor reports, so they can lag visible actions.

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
