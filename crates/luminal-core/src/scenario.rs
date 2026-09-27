//! Starting situations. Geometry here is placeholder, not a balanced scenario.

use crate::celestial::{Celestial, CelestialKind, Orbit, System};
use crate::kinematics::{State, Vec2};
use crate::units::{AU, G0, LIGHT_SECOND};
use crate::world::{BodyKind, BodySpec, FactionId, World};
use std::f64::consts::FRAC_PI_2;

pub const ESCORT: FactionId = FactionId(0);
pub const RAIDER: FactionId = FactionId(1);

pub const DAY: f64 = 86_400.0;

/// A Sun-like star, an Earth-like planet at 1 AU and a Moon-like satellite.
pub fn home_system() -> System {
    System {
        bodies: vec![
            Celestial {
                name: "Sun".into(),
                kind: CelestialKind::Star,
                gm: 1.327_124_4e11,
                radius: 696_000.0,
                orbit: Orbit::Fixed(Vec2::ZERO),
            },
            Celestial {
                name: "Planet".into(),
                kind: CelestialKind::Planet,
                gm: 398_600.4,
                radius: 6_371.0,
                orbit: Orbit::Circular { parent: 0, radius: AU, period: 365.256 * DAY, phase: 0.0 },
            },
            Celestial {
                name: "Moon".into(),
                kind: CelestialKind::Moon,
                gm: 4_902.8,
                radius: 1_737.4,
                orbit: Orbit::Circular { parent: 1, radius: 384_400.0, period: 27.32 * DAY, phase: FRAC_PI_2 },
            },
        ],
    }
}

/// GAME_MECHANICS.md §15: a cruiser intercepting a transport before it reaches a
/// departure region, with a defending frigate.
///
/// The transport and frigate leave the planet outbound at 1 g. The cruiser starts about
/// 160 light-seconds away and is burning hard toward them, so it is visible; a cold
/// approach would be a separate variant.
pub fn transport_intercept() -> World {
    let system = home_system();
    let planet = system.state(1, 0.0);
    let ship = |name: &str, faction, pos: Vec2, vel: Vec2, thrust: Vec2| BodySpec {
        name: name.into(),
        kind: BodyKind::Ship,
        faction,
        state: State { pos: planet.pos + pos, vel: planet.vel + vel },
        thrust,
    };

    let outbound = Vec2::new(1.0, 0.0);
    let cruiser_offset = Vec2::new(0.2 * AU, -0.25 * AU);
    let cruiser_heading = (-cruiser_offset).normalized();
    let specs = vec![
        ship("Transport", ESCORT, Vec2::new(60_000.0, 0.0), outbound * 4.0, outbound * G0),
        ship("Frigate", ESCORT, Vec2::new(60_000.0, -LIGHT_SECOND), outbound * 4.0, outbound * G0),
        ship("Cruiser", RAIDER, cruiser_offset, Vec2::new(-10.0, 15.0), cruiser_heading * (20.0 * G0)),
    ];
    World::new(system, specs, 3600.0, 42)
}
