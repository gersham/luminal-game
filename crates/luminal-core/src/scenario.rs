//! Starting situations. Geometry here is placeholder, not a balanced scenario.
use crate::celestial::System;
use crate::kinematics::{State, Vec2};
use crate::params::{MAGAZINE_CRUISER, MAGAZINE_FRIGATE};
use crate::units::AU;
#[cfg(test)]
use crate::units::G0;
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
    for id in [BodyId(1),BodyId(2)] {world.reset_platform_history(id);}
    world.seed_debug_contact(BodyId(1),BodyId(2));
    world.set_follow(BodyId(1),BodyId(0)).unwrap();
    world
}

/// Symmetric combat platforms, configured before either side receives observations.
pub fn transport_intercept_class(seed:u64,class:crate::world::ShipClass)->World {
    let mut world=transport_intercept_seeded(seed);
    for id in [BodyId(1),BodyId(2)] {
        let b=&mut world.bodies[id.0 as usize];
        b.controls.ecm_rating=class.sensor_rating();b.controls.eccm_rating=class.sensor_rating()*0.5;b.baseline_emission_factor=0.5;
        b.ship_class=Some(class);b.name=if id==BodyId(1) {class.name().into()} else {format!("Raider {}",class.name())};
        b.damage.hull_max=crate::damage::FRIGATE_HULL_HP*class.scale();b.damage.hull=b.damage.hull_max;
        b.damage.armour_max=500.0*class.protection();b.damage.armour=b.damage.armour_max;
        b.point_defence.as_mut().unwrap().lasers=class.pd_lasers();
        b.magazine=class.magazine();b.interceptor_battery.as_mut().unwrap().rounds=class.interceptors();
        b.thermal.capacity_scale=class.scale();b.thermal.capacitor_multiplier=if class==crate::world::ShipClass::Battleship {2.0} else {1.0};b.thermal.capacitor_j=b.thermal.capacitor_capacity();
        b.drive_limit=class.max_g()*crate::units::G0;
        b.beam_auto=class!=crate::world::ShipClass::Picket;
    }
    for id in [BodyId(1),BodyId(2)] {world.reset_platform_history(id);}
    world.seed_debug_contact(BodyId(1),BodyId(2));
    world.set_follow(BodyId(1),BodyId(0)).unwrap();
    world
}

/// Radius of the escorts' starting orbit about the planet, km.
const PARKING_ORBIT_KM: f64 = 60_000.0;


/// GAME_MECHANICS.md §15: a cruiser intercepting a transport before it reaches a
/// departure region, with a defending frigate.
///
/// The frigate starts in a planetary parking orbit; the transport departs from
/// beside the lunar station. Raider and departure region are independently
/// chosen with the departure 6–10 AU out and the raider at rest inside a 4 AU wide
/// ellipse from Sol to 1 AU beyond it or within 5 AU of Sol, excluding the 1 AU region around Earth.
pub fn transport_intercept() -> World {
    transport_intercept_seeded(42)
}

/// Private placement bounds; never exposed in the map view.
#[derive(Clone,Debug)]
struct RaiderSpawnRegion {
    center:Vec2,
    axis:Vec2,
    half_length:f64,
    half_width:f64,
    exclusion_center:Vec2,
    exclusion_radius:f64,
}
impl RaiderSpawnRegion {
    #[cfg(test)]
    fn point(&self,radius:f64,angle:f64)->Vec2 {
        self.center+self.axis*(self.half_length*radius*angle.cos())
            +Vec2::new(-self.axis.y,self.axis.x)*(self.half_width*radius*angle.sin())
    }
    fn contains(&self,pos:Vec2)->bool {
        let d=pos-self.center;
        let side=Vec2::new(-self.axis.y,self.axis.x);
        let ellipse=(d.dot(self.axis)/self.half_length).powi(2)+(d.dot(side)/self.half_width).powi(2)<=1.0+1e-12;
        (ellipse || pos.length()<=5.0*AU) && (pos-self.exclusion_center).length()>=self.exclusion_radius
    }
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
    let departure=outer_position(6.0,10.0);
    let axis=departure.normalized();
    let half_length=(departure.length()+AU)*0.5;
    let raider_spawn=RaiderSpawnRegion {
        center:axis*half_length,axis,half_length,half_width:2.0*AU,
        exclusion_center:planet.pos,exclusion_radius:AU,
    };
    let cruiser_pos=loop {
        // Uniform sampling of the union; overlap is not counted twice.
        let along=-5.0*AU+(2.0*half_length+5.0*AU)*layout.uniform();
        let across=(layout.uniform()*10.0-5.0)*AU;
        let pos=axis*along+Vec2::new(-axis.y,axis.x)*across;
        if raider_spawn.contains(pos) && system.bodies.iter().enumerate().all(|(i,b)|
            (pos-system.state(i,0.0).pos).length()>b.radius+0.03*AU) {break pos;}
    };
    let specs = vec![
        ship("Transport", ESCORT, transport_pos, transport_vel, Vec2::ZERO, 0.0),
        ship("Frigate", ESCORT, frigate_pos, frigate_vel, Vec2::ZERO, MAGAZINE_FRIGATE.value),
        ship("Cruiser", RAIDER, cruiser_pos-planet.pos, -planet.vel, Vec2::ZERO, MAGAZINE_CRUISER.value),
        BodySpec { name: "Lunar sensor station".into(), kind: BodyKind::Station, faction: ESCORT,
            state: State { pos: station_pos, vel: station_vel },
            thrust: Vec2::ZERO, magazine: 0 },
    ];
    let mut world = World::new(system, specs, 12000.0, seed);
    world.bodies[0].baseline_emission_factor=crate::params::TRANSPORT_EMISSION_FACTOR.value;
    world.bodies[0].visibility_multiplier=2.0;
    world.bodies[1].baseline_emission_factor=crate::params::FRIGATE_EMISSION_FACTOR.value;
    world.objective = Some(Objective {
        sensor_site:Some(crate::world::SensorSite {pos:station_pos,sensors:crate::sensors::SensorSuite::FULL}),
        name: "departure region".into(),
        center: departure,
        radius: 0.02 * AU,
        protect: BodyId(0),
        player:Some(BodyId(1)),
        defeat: Some(BodyId(2)),
        defender: ESCORT,
        attacker: RAIDER,
    });
    let destination = world.objective.as_ref().unwrap().center;
    world.bodies[0].ship_class=Some(crate::world::ShipClass::Transport);
    world.bodies[0].drive_limit=25.0*crate::units::G0;
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
    for id in [BodyId(1),BodyId(2)] {world.bodies[id.0 as usize].point_defence.as_mut().unwrap().rate_hz=0.2;}
    for id in [BodyId(1), BodyId(2)] {
        world.bodies[id.0 as usize].damage.hull=crate::damage::FRIGATE_HULL_HP;
        world.bodies[id.0 as usize].damage.hull_max=crate::damage::FRIGATE_HULL_HP;
        world.bodies[id.0 as usize].magazine=[20,10];
        world.arm_beams(id).unwrap();
    }
    for b in &mut world.bodies {
        b.interceptor_battery=Some(crate::world::interceptor::Battery {
            rounds:if b.kind==BodyKind::Station {20} else if b.magazine.iter().any(|n|*n>0) {40} else {30},launched:0,ready_at:0.0,status:"Ready"});
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
    fn transport_thrust_is_limited_to_fifty_g() {
        let mut world=transport_intercept();
        assert!((world.bodies[0].max_accel()/G0-25.0).abs()<1e-9);
        world.bodies[0].controls.transport_alerted=true;
        assert!((world.bodies[0].max_accel()/G0-50.0).abs()<1e-9);
    }

    #[test]
    fn transport_starts_at_25g_with_a_50g_ceiling() {
        let world=transport_intercept();
        let transport=&world.bodies[0];
        let frigate=&world.bodies[1];
        assert!((transport.max_accel()/G0-25.0).abs()<1e-9);
        assert!((transport.heat_rated_accel()/G0-50.0).abs()<1e-9);
        assert!(frigate.max_accel()>transport.max_accel());
        assert!((transport.drive_limit/G0-25.0).abs()<1e-9);
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
    fn transport_starts_beside_station_and_raider_is_at_rest() {
        let w=transport_intercept();
        let transport=w.bodies[0].trajectory.state_at(0.0).unwrap();
        let station=w.bodies[3].trajectory.state_at(0.0).unwrap();
        let raider=w.bodies[2].trajectory.state_at(0.0).unwrap();
        assert!(((transport.pos-station.pos).length()-1_000.0).abs()<50.0);
        assert!((transport.vel-station.vel).length()<0.05);
        assert!(raider.vel.length()<1e-6);
        assert_eq!(w.bodies[2].trajectory.last().thrust,Vec2::ZERO);
    }

    #[test]
    fn outer_layout_is_seeded_independent_and_clear_of_planets() {
        let positions=|seed| {
            let w=transport_intercept_seeded(seed);
            let raider=w.bodies[2].trajectory.state_at(0.0).unwrap().pos;
            let departure=w.objective.as_ref().unwrap().center;
            let axis=departure.normalized();
            let half_length=(departure.length()+AU)*0.5;
            let region=RaiderSpawnRegion {center:axis*half_length,axis,half_length,half_width:2.0*AU,
                exclusion_center:w.system.state(1,0.0).pos,exclusion_radius:AU};
            assert!(region.contains(raider));
            assert!(region.contains(-axis*4.0*AU),"inner-system area extends behind Sol beyond the ellipse");
            assert!(!region.contains(region.exclusion_center));
            assert!(!region.contains(-axis*6.0*AU));
            assert_eq!(region.half_width,2.0*AU);
            assert!((region.point(1.0,std::f64::consts::PI)).length()<1e-5);
            assert!((region.point(1.0,0.0).length()-departure.length()-AU).abs()<1e-5);
            assert!(w.bodies[2].trajectory.state_at(0.0).unwrap().vel.length()<1e-6);
            assert_eq!(w.bodies[2].trajectory.last().thrust,Vec2::ZERO);
            assert!((6.0..=10.0).contains(&(departure.length()/AU)));
            for pos in [raider,departure] {
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
            assert_eq!(b.point_defence.unwrap().rate_hz,match i {0=>0.5,1|2=>0.2,_=>1.0},"warships recharge each laser in five seconds; transport and station retain their rates");
            assert_eq!(b.interceptor_battery.unwrap().rounds,if b.kind==BodyKind::Station {20} else if b.magazine.iter().any(|n|*n>0) {40} else {30});
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
            assert_eq!(b.screen_available(),1.0,"initial shields have full absorption capacity");
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
        assert!(world.outcome.is_none(),"transport arrival does not end the raider scenario");
        let goal=world.objective.as_ref().unwrap();
        assert!((world.bodies[0].trajectory.state_at(world.time()).unwrap().pos-goal.center).length()<goal.radius);
        assert!(!world.losses.iter().any(|l| l.body == BodyId(0)));
    }
}
