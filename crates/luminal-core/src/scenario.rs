//! Starting situations. Geometry here is placeholder, not a balanced scenario.

use crate::celestial::{Celestial, CelestialKind, Orbit, System};
use crate::kinematics::{State, Vec2};
use crate::params::{MAGAZINE_CRUISER, MAGAZINE_FRIGATE};
use crate::units::{AU, G0};
use crate::world::{BodyId, BodyKind, BodySpec, FactionId, Objective, World};
use std::f64::consts::FRAC_PI_2;

pub const ESCORT: FactionId = FactionId(0);
pub const RAIDER: FactionId = FactionId(1);

pub const DAY: f64 = 86_400.0;

/// Desktop testing preset: retain the escort scenario, but supply the frigate
/// with a historical direction indication, not a free range/course solution.
pub fn transport_intercept_debug()->World {
    let mut world=transport_intercept();
    let contact=world.seed_debug_contact(BodyId(1),BodyId(2));
    world.set_tactical_range(BodyId(1),crate::world::InterceptTarget::Contact(contact),Some(3.0*AU)).unwrap();
    world
}

/// Radius of the escorts' starting orbit about the planet, km.
const PARKING_ORBIT_KM: f64 = 60_000.0;

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
/// The frigate starts in a planetary parking orbit; the transport departs from
/// beside the lunar station. The cruiser starts about 320 light-seconds away
/// and is burning hard toward them, so it is visible; a cold approach would be a
/// separate variant.
///
/// Objective: the transport must reach a departure region about 2.8 AU map-north
/// of its starting position. PLACEHOLDER geometry, not balanced.
pub fn transport_intercept() -> World {
    let system = home_system();
    let planet = system.state(1, 0.0);
    let ship = |name: &str, faction, pos: Vec2, vel: Vec2, thrust: Vec2, magazine: f64| BodySpec {
        name: name.into(),
        kind: BodyKind::Ship,
        faction,
        state: State { pos: planet.pos + pos, vel: planet.vel + vel },
        thrust,
        magazine: magazine as u32,
    };

    // Circular parking orbit, counter-clockwise; the frigate trails the transport.
    let parking = |radius: f64, phase: f64| {
        let v = (system.bodies[1].gm / radius).sqrt();
        let (sin, cos) = phase.sin_cos();
        (Vec2::new(cos, sin) * radius, Vec2::new(-sin, cos) * v)
    };
    let (frigate_pos, frigate_vel) = parking(PARKING_ORBIT_KM, -20f64.to_radians());
    let moon = system.state(2,0.0);
    let station_radius = 5_000.0;
    let station_speed = (system.bodies[2].gm/station_radius).sqrt();
    let station_pos=moon.pos+Vec2::new(station_radius,0.0);
    let station_vel=moon.vel+Vec2::new(0.0,station_speed);
    let transport_pos=station_pos+Vec2::new(1_000.0,0.0)-planet.pos;
    let transport_vel=station_vel-planet.vel;
    let old_approach=Vec2::new(0.2*AU,-0.25*AU)-Vec2::new(PARKING_ORBIT_KM,0.0);
    let previous_offset=transport_pos+old_approach*2.0;
    let cruiser_offset=frigate_pos+(previous_offset-frigate_pos)*2.0;
    let cruiser_heading=(-old_approach).normalized();
    let specs = vec![
        ship("Transport", ESCORT, transport_pos, transport_vel, Vec2::ZERO, 0.0),
        ship("Frigate", ESCORT, frigate_pos, frigate_vel, Vec2::ZERO, MAGAZINE_FRIGATE.value),
        ship("Cruiser", RAIDER, cruiser_offset, Vec2::new(-10.0, 15.0), cruiser_heading * (20.0 * G0), MAGAZINE_CRUISER.value),
        BodySpec { name: "Lunar sensor station".into(), kind: BodyKind::Station, faction: ESCORT,
            state: State { pos: station_pos, vel: station_vel },
            thrust: Vec2::ZERO, magazine: 0 },
    ];
    let mut world = World::new(system, specs, 3600.0, 42);
    world.bodies[0].baseline_emission_factor=crate::params::TRANSPORT_EMISSION_FACTOR.value;
    world.bodies[1].baseline_emission_factor=crate::params::FRIGATE_EMISSION_FACTOR.value;
    let transport_start=world.bodies[0].trajectory.state_at(0.0).unwrap().pos;
    let departure_distance=(planet.pos+Vec2::new(2.5*AU,1.2*AU)-transport_start).length();
    world.objective = Some(Objective {
        name: "departure region".into(),
        center: transport_start + Vec2::new(0.0,departure_distance),
        radius: 0.02 * AU,
        protect: BodyId(0),
        defender: ESCORT,
        attacker: RAIDER,
    });
    let destination = world.objective.as_ref().unwrap().center;
    world.bodies[0].ship_class=Some(crate::world::ShipClass::Transport);
    world.set_move(BodyId(0), destination).expect("escape destination is navigable");
    world.bodies[0].has_screen=false;
    // Temporarily listen for bearings only; no station ranging or auto pings.
    world.bodies[3].sensors.passive=false;
    world.bodies[3].sensors.active=false;
    // These combatants raised their screens before the scenario began.
    for id in [BodyId(1),BodyId(2)] {
        world.set_screen(id,true).expect("combatant has screens");
        world.bodies[id.0 as usize].thermal.field=1.0;
    }
    world.bodies[0].controllable = false;
    world.probes_enabled=crate::params::PROBES_ENABLED;
    if !world.probes_enabled {for b in &mut world.bodies {b.probes=0;}}
    for id in [BodyId(0),BodyId(1),BodyId(2),BodyId(3)] {world.fit_point_defence(id);}
    world.bodies[0].point_defence.as_mut().unwrap().rate_hz=0.5;
    for id in [BodyId(1),BodyId(2)] {world.bodies[id.0 as usize].point_defence.as_mut().unwrap().rate_hz=2.0;}
    for id in [BodyId(1), BodyId(2)] {
        world.bodies[id.0 as usize].damage.hull=crate::damage::FRIGATE_HULL_HP;
        world.bodies[id.0 as usize].damage.hull_max=crate::damage::FRIGATE_HULL_HP;
        world.arm_beams(id).unwrap();
    }
    for b in &mut world.bodies {
        b.interceptor_battery=Some(crate::world::interceptor::Battery {
            rounds:if b.kind==BodyKind::Station {20} else {30},launched:0,ready_at:0.0,status:"Ready"});
    }
    world
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transport_has_half_warship_acceleration_from_its_first_order() {
        let world=transport_intercept();
        let transport=&world.bodies[0];
        let frigate=&world.bodies[1];
        assert_eq!(transport.max_accel(),frigate.max_accel()*0.5);
        assert!(transport.trajectory.last().thrust.length()<=transport.max_accel()+1e-9);
        assert_eq!(world.bodies[2].max_accel(),frigate.max_accel());
    }

    #[test]
    fn station_has_direction_finding_only() {
        let mut w=transport_intercept();
        let suite=w.bodies[3].sensors;
        assert!(suite.direction_finding && !suite.active && !suite.passive);
        w.advance_to(180.0);
        assert!(w.ping_emissions.iter().all(|(id,_)|*id!=BodyId(3)));
    }

    #[test]
    fn transport_starts_beside_station_and_raider_starts_twice_as_far() {
        let w=transport_intercept();
        let transport=w.bodies[0].trajectory.state_at(0.0).unwrap();
        let station=w.bodies[3].trajectory.state_at(0.0).unwrap();
        let raider=w.bodies[2].trajectory.state_at(0.0).unwrap();
        assert!(((transport.pos-station.pos).length()-1_000.0).abs()<50.0);
        assert!((transport.vel-station.vel).length()<0.05);
        let old=Vec2::new(0.2*AU,-0.25*AU)-Vec2::new(PARKING_ORBIT_KM,0.0);
        let frigate=w.bodies[1].trajectory.state_at(0.0).unwrap();
        let previous=transport.pos+old*2.0;
        assert!(((raider.pos-frigate.pos).length()/(previous-frigate.pos).length()-2.0).abs()<0.001);
        assert!((raider.pos-transport.pos).normalized().dot(old.normalized())>0.999);
    }

    #[test]
    fn departure_is_due_map_north_at_the_original_travel_distance() {
        let world=transport_intercept();
        let start=world.bodies[0].trajectory.state_at(0.0).unwrap().pos;
        let delta=world.objective.as_ref().unwrap().center-start;
        assert!(delta.x.abs()<1e-6);
        assert!(delta.y>0.0,"positive world Y is up on the map");
        let old=world.system.state(1,0.0).pos+Vec2::new(2.5*AU,1.2*AU);
        assert!((delta.length()-(old-start).length()).abs()<1e-6);
    }

    #[test]
    fn probes_are_disabled_for_every_platform() {
        let mut world=transport_intercept();
        assert!(!world.probes_enabled);
        assert!(world.bodies.iter().all(|b|b.probes==0));
        for id in [BodyId(0),BodyId(1),BodyId(2),BodyId(3)] {
            world.bodies[id.0 as usize].probes=3; // Stock alone cannot bypass the gate.
            assert!(world.deploy_probe(id,Vec2::new(1.0,0.0)).is_err());
        }
        assert!(world.bodies.iter().all(|b|b.kind!=BodyKind::Probe));
    }

    #[test]
    fn scenario_screen_fit_and_initial_posture() {
        let mut world=transport_intercept();
        for (i,b) in world.bodies.iter().enumerate() {
            assert_eq!(b.point_defence.unwrap().rate_hz,match i {0=>0.5,1|2=>2.0,_=>1.0},"frigates fire twice per second; transport and station retain their rates");
            assert_eq!(b.interceptor_battery.unwrap().rounds,if b.kind==BodyKind::Station {20} else {30});
        }
        for id in [BodyId(0),BodyId(3)] {
            assert!(!world.bodies[id.0 as usize].has_screen);
            assert!(!world.bodies[id.0 as usize].screen_up);
            assert!(world.set_screen(id,true).is_err());
        }
        for id in [BodyId(1),BodyId(2)] {
            let b=&world.bodies[id.0 as usize];
            assert!(b.has_screen && b.screen_up);
            assert_eq!(b.damage.hull,crate::damage::FRIGATE_HULL_HP);
            assert_eq!(b.damage.hull_max,crate::damage::FRIGATE_HULL_HP);
            assert_eq!(b.damage.armour,100.0,"hull tuning must not increase armour");
            assert_eq!(b.thermal.field,1.0);
            assert_eq!(b.screen_j,0.0,"raised does not mean full of absorbed damage");
        }
    }

    #[test]
    fn transport_escapes_without_player_orders() {
        let mut world = transport_intercept();
        assert!(!world.bodies[0].controllable);
        assert!(world.bodies[1].controllable);
        assert_eq!(world.bodies[0].baseline_emission_factor,1.0);
        assert_eq!(world.bodies[1].baseline_emission_factor,0.5);
        assert_eq!(world.bodies[3].baseline_emission_factor,2.0);
        assert!(world.bodies[0].autopilot.is_some());
        world.advance_to(DAY);
        assert_eq!(world.outcome.as_ref().map(|o| o.winner), Some(ESCORT));
        assert!(!world.losses.iter().any(|l| l.body == BodyId(0)));
    }
}
