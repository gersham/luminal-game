//! Starting situations. Geometry here is placeholder, not a balanced scenario.
use crate::celestial::System;
use crate::kinematics::{State, Vec2};
use crate::params::{MAGAZINE_CRUISER, MAGAZINE_FRIGATE};
use crate::units::{AU, G0};
use crate::world::{BodyId, BodyKind, BodySpec, FactionId, Objective, World};

pub const ESCORT: FactionId = FactionId(0);
pub const RAIDER: FactionId = FactionId(1);

pub const DAY: f64 = 86_400.0;

/// Deterministic Sol snapshot for tests and CLI scenarios.
pub fn home_system() -> System {
    crate::sol::system(42)
}

/// Desktop testing preset: retain the escort scenario, but supply the frigate
/// with a historical direction indication, not a free range/course solution.
pub fn transport_intercept_debug()->World {
    transport_intercept_debug_seeded(42)
}

pub fn transport_intercept_debug_seeded(seed: u64)->World {
    let mut world=transport_intercept_seeded(seed);
    let contact=world.seed_debug_contact(BodyId(1),BodyId(2));
    world.set_intercept(BodyId(1),crate::world::InterceptTarget::Contact(contact)).unwrap();
    world
}

/// Radius of the escorts' starting orbit about the planet, km.
const PARKING_ORBIT_KM: f64 = 60_000.0;


/// GAME_MECHANICS.md §15: a cruiser intercepting a transport before it reaches a
/// departure region, with a defending frigate.
///
/// The frigate starts in a planetary parking orbit; the transport departs from
/// beside the lunar station. Raider and departure region are independently
/// randomized in heliocentric bands: raider 5–10 AU, departure 10–20 AU. The raider starts with circular
/// orbital velocity and an inward approach burn. Geometry remains experimental.
pub fn transport_intercept() -> World {
    transport_intercept_seeded(42)
}

pub fn transport_intercept_seeded(seed: u64) -> World {
    let system = crate::sol::system(seed);
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
    let mut layout=crate::rng::Rng::stream(seed,0x4c41594f5554);
    let mut outer_position=|min:f64,max:f64|loop {
        let radius=(min+(max-min)*layout.uniform())*AU;
        let angle=std::f64::consts::TAU*layout.uniform();
        let pos=Vec2::new(angle.cos(),angle.sin())*radius;
        if system.bodies.iter().enumerate().all(|(i,b)|
            (pos-system.state(i,0.0).pos).length()>b.radius+0.03*AU) {break pos;}
    };
    let cruiser_pos=outer_position(5.0,10.0);
    let departure=outer_position(10.0,20.0);
    let radial=cruiser_pos.normalized();
    let cruiser_vel=Vec2::new(-radial.y,radial.x)*(system.bodies[0].gm/cruiser_pos.length()).sqrt();
    let cruiser_heading=(station_pos-cruiser_pos).normalized();
    let specs = vec![
        ship("Transport", ESCORT, transport_pos, transport_vel, Vec2::ZERO, 0.0),
        ship("Frigate", ESCORT, frigate_pos, frigate_vel, Vec2::ZERO, MAGAZINE_FRIGATE.value),
        ship("Cruiser", RAIDER, cruiser_pos-planet.pos, cruiser_vel-planet.vel, cruiser_heading * (20.0 * G0), MAGAZINE_CRUISER.value),
        BodySpec { name: "Lunar sensor station".into(), kind: BodyKind::Station, faction: ESCORT,
            state: State { pos: station_pos, vel: station_vel },
            thrust: Vec2::ZERO, magazine: 0 },
    ];
    let mut world = World::new(system, specs, 7200.0, seed);
    world.bodies[0].baseline_emission_factor=crate::params::TRANSPORT_EMISSION_FACTOR.value;
    world.bodies[0].visibility_multiplier=2.0;
    world.bodies[1].baseline_emission_factor=crate::params::FRIGATE_EMISSION_FACTOR.value;
    world.objective = Some(Objective {
        sensor_site:Some(crate::world::SensorSite {pos:station_pos,sensors:crate::sensors::SensorSuite::FULL}),
        name: "departure region".into(),
        center: departure,
        radius: 0.02 * AU,
        protect: BodyId(0),
        defeat: Some(BodyId(2)),
        defender: ESCORT,
        attacker: RAIDER,
    });
    let destination = world.objective.as_ref().unwrap().center;
    world.bodies[0].ship_class=Some(crate::world::ShipClass::Transport);
    world.set_move(BodyId(0), destination).expect("escape destination is navigable");
    world.bodies[0].has_screen=false;
    world.bodies[3].sensors=crate::sensors::SensorSuite::FULL;
    // These combatants raised their screens before the scenario began.
    for id in [BodyId(1),BodyId(2)] {
        world.set_screen(id,true).expect("combatant has screens");
        world.bodies[id.0 as usize].thermal.field=1.0;
        // Preserve the scenario's established screens, now under latched Auto.
        world.bodies[id.0 as usize].controls.screens=crate::world::controls::Mode::Auto;
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
    fn random_desktop_starts_have_enough_light_history() {
        for seed in 0..64 {
            let world=transport_intercept_debug_seeded(seed);
            assert!(world.bodies[1].autopilot.is_some());
        }
    }

    #[test]
    fn transport_boost_is_limited_to_thirty_g() {
        let mut world=transport_intercept();
        world.bodies[0].controls.boost_active=true;
        assert!((world.bodies[0].max_accel()/G0-30.0).abs()<1e-9);
    }

    #[test]
    fn transport_has_quarter_warship_acceleration_from_its_first_order() {
        let world=transport_intercept();
        let transport=&world.bodies[0];
        let frigate=&world.bodies[1];
        assert_eq!(transport.max_accel(),frigate.max_accel()*0.25);
        assert!(transport.trajectory.last().thrust.length()<=transport.max_accel()+1e-9);
        assert_eq!(world.bodies[2].max_accel(),frigate.max_accel());
    }

    #[test]
    fn station_has_full_sensors_and_autonomous_pings() {
        let mut w=transport_intercept();
        let suite=w.bodies[3].sensors;
        assert!(suite.direction_finding && suite.active && suite.passive);
        w.advance_to(180.0);
        assert!(w.ping_emissions.iter().filter(|(id,_)|*id==BodyId(3)).count()>=3);
    }

    #[test]
    fn transport_starts_beside_station_and_raider_in_outer_band() {
        let w=transport_intercept();
        let transport=w.bodies[0].trajectory.state_at(0.0).unwrap();
        let station=w.bodies[3].trajectory.state_at(0.0).unwrap();
        let raider=w.bodies[2].trajectory.state_at(0.0).unwrap();
        assert!(((transport.pos-station.pos).length()-1_000.0).abs()<50.0);
        assert!((transport.vel-station.vel).length()<0.05);
        assert!((5.0..=10.0).contains(&(raider.pos.length()/AU)));
    }

    #[test]
    fn outer_layout_is_seeded_independent_and_clear_of_planets() {
        let positions=|seed| {
            let w=transport_intercept_seeded(seed);
            let raider=w.bodies[2].trajectory.state_at(0.0).unwrap().pos;
            let departure=w.objective.as_ref().unwrap().center;
            for (pos,band) in [(raider,5.0..=10.0),(departure,10.0..=20.0)] {
                assert!(band.contains(&(pos.length()/AU)));
                for (i,b) in w.system.bodies.iter().enumerate() {
                    assert!((pos-w.system.state(i,0.0).pos).length()>b.radius+0.02*AU);
                }
            }
            assert!((raider-departure).length()>1.0);
            (raider,departure)
        };
        assert_eq!(positions(42),positions(42));
        let first=positions(0);
        for seed in 1..32 {let next=positions(seed);assert_ne!(next.0,first.0);assert_ne!(next.1,first.1);}
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
    fn civilian_visibility_and_station_size_feed_the_shared_ef() {
        let w=transport_intercept();
        let transport=w.bodies[0].emissivity_factors(0.0);
        assert_eq!(transport.visibility_multiplier,2.0);
        let normal=crate::sensors::EmissivityFactors {visibility_multiplier:1.0,..transport};
        assert_eq!(transport.value(),2.0*normal.value());
        let a=crate::sensors::detection_ranges(transport.value());
        let b=crate::sensors::detection_ranges(normal.value());
        for i in 0..4 {assert_eq!(a[i],2.0*b[i]);}
        assert_eq!(w.bodies[3].emissivity_factors(0.0).size,20.0);
        assert_eq!(w.bodies[1].visibility_multiplier,1.0);
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
        world.advance_to(7.0*DAY);
        assert_eq!(world.outcome.as_ref().map(|o| o.winner), Some(ESCORT));
        assert!(!world.losses.iter().any(|l| l.body == BodyId(0)));
    }
}
