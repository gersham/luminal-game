//! Starting situations.
use crate::celestial::System;
use crate::kinematics::{State, Vec2};
use crate::params::{MAGAZINE_CRUISER, MAGAZINE_FRIGATE};
use crate::rng::Rng;
use crate::units::{AU, LIGHT_SECOND};
#[cfg(test)]
use crate::units::G0;
use crate::world::{BodyId, BodyKind, BodySpec, FactionId, Objective, ShipClass, World};

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
    transport_intercept_class_in_system(seed,class,crate::sol::system(seed))
}

pub fn transport_intercept_class_in_system(seed:u64,class:crate::world::ShipClass,system:System)->World {
    let mut world=transport_intercept_in_system(seed,system);
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
    transport_intercept_in_system(seed,crate::sol::system(seed))
}

pub fn transport_intercept_in_system(seed:u64,system:System)->World {
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
        prize: None,
        wipe: false,
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
        world.bodies[id.0 as usize].magazine=crate::world::ShipClass::Frigate.magazine();
        world.arm_beams(id).unwrap();
    }
    for b in &mut world.bodies {
        b.interceptor_battery=Some(crate::world::interceptor::Battery {
            rounds:if b.kind==BodyKind::Station {20} else if b.magazine.iter().any(|n|*n>0) {40} else {30},launched:0,ready_at:0.0,status:"Ready"});
    }
    world
}

/// Player-facing situations. Escort keeps the transport mission; the others are standalone fights.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scenario { Escort, Raid, HideAndSeek, Armada }

const ARMADA_MIX:[ShipClass;10]=[
    ShipClass::Battleship,ShipClass::Cruiser,ShipClass::Destroyer,ShipClass::Destroyer,
    ShipClass::Frigate,ShipClass::Frigate,ShipClass::Frigate,ShipClass::Picket,ShipClass::Picket,ShipClass::Picket,
];
const RAID_SCREEN:[ShipClass;3]=[ShipClass::Destroyer,ShipClass::Frigate,ShipClass::Frigate];

impl Scenario {
    pub const ALL:[Self;4]=[Self::Escort,Self::Raid,Self::HideAndSeek,Self::Armada];
    pub fn name(self)->&'static str {match self {Self::Escort=>"Escort",Self::Raid=>"Raid",Self::HideAndSeek=>"Hide and Seek",Self::Armada=>"Armada"}}
    pub fn token(self)->&'static str {match self {Self::Escort=>"escort",Self::Raid=>"raid",Self::HideAndSeek=>"hide",Self::Armada=>"armada"}}
    pub fn brief(self)->&'static str {match self {
        Self::Escort=>"Escort the transport past a destroyer.",
        Self::Raid=>"A cruiser raid on a screened station.",
        Self::HideAndSeek=>"Two frigates. Find the other one.",
        Self::Armada=>"Ten ships each, from opposite ends.",
    }}
    pub fn forces(self)->&'static str {match self {
        Self::Escort=>"Destroyer versus destroyer",
        Self::Raid=>"Cruiser versus a station and its escorts",
        Self::HideAndSeek=>"Frigate versus frigate",
        Self::Armada=>"Battleship and a mixed fleet of nine",
    }}
    pub fn detail(self)->&'static str {match self {
        Self::Escort=>"Both sides field a destroyer. The transport runs for the departure region. Defeat the raider.",
        Self::Raid=>"Your cruiser starts away from the base. A destroyer and two frigates screen the station. Destroy the station.",
        Self::HideAndSeek=>"You and the quarry are frigates, placed at random and out of contact. Destroy the quarry.",
        Self::Armada=>"Your battleship leads a cruiser, two destroyers, three frigates and three pickets. The enemy fleet matches you.",
    }}
    pub fn bots(self)->&'static [FactionId] {
        const BOTH:[FactionId;2]=[ESCORT,RAIDER];
        const ONE:[FactionId;1]=[RAIDER];
        match self {Self::Armada=>&BOTH,_=>&ONE}
    }
    pub fn parse(name:&str)->Option<Self> {
        let name=name.trim();
        Self::ALL.into_iter().find(|scenario|scenario.name().eq_ignore_ascii_case(name)||scenario.token().eq_ignore_ascii_case(name))
    }
    pub fn build(self,seed:u64,system:System)->World {
        match self {
            Self::Escort=>transport_intercept_class_in_system(seed,ShipClass::Destroyer,system),
            Self::Raid=>raid(seed,system),
            Self::HideAndSeek=>hide_and_seek(seed,system),
            Self::Armada=>armada(seed,system),
        }
    }
}

fn clear_of_celestials(system:&System,pos:Vec2)->bool {
    system.bodies.iter().enumerate().all(|(i,b)|(pos-system.state(i,0.0).pos).length()>b.radius+0.03*AU)
}
fn spec(name:&str,kind:BodyKind,faction:FactionId,pos:Vec2,magazine:u32)->BodySpec {
    BodySpec {name:name.into(),kind,faction,state:State {pos,vel:Vec2::ZERO},thrust:Vec2::ZERO,magazine}
}
fn fit_combatant(world:&mut World,id:BodyId,class:ShipClass,screens:bool) {
    {
        let b=&mut world.bodies[id.0 as usize];
        b.controls.ecm_rating=class.sensor_rating();b.controls.eccm_rating=class.sensor_rating()*0.5;b.baseline_emission_factor=0.5;
        b.ship_class=Some(class);
        b.damage.hull_max=crate::damage::FRIGATE_HULL_HP*class.scale();b.damage.hull=b.damage.hull_max;
        b.damage.armour_max=500.0*class.protection();b.damage.armour=b.damage.armour_max;
        b.magazine=class.magazine();b.armed=true;
        b.thermal.capacity_scale=class.scale();
        b.thermal.capacitor_multiplier=if class==ShipClass::Battleship {2.0} else {1.0};
        b.thermal.capacitor_j=b.thermal.capacitor_capacity();
        b.drive_limit=class.max_g()*crate::units::G0;
        b.beam_auto=class!=ShipClass::Picket;b.controllable=true;b.has_screen=true;
    }
    world.fit_point_defence(id);
    {let pd=world.bodies[id.0 as usize].point_defence.as_mut().unwrap();pd.lasers=class.pd_lasers();pd.rate_hz=0.2;}
    world.bodies[id.0 as usize].interceptor_battery=Some(crate::world::interceptor::Battery {rounds:class.interceptors(),launched:0,ready_at:0.0,status:"Ready"});
    if screens {
        world.set_screen(id,true).expect("combatant has screens");
        let b=&mut world.bodies[id.0 as usize];
        b.thermal.field=1.0;b.controls.screens=crate::world::controls::Mode::Auto;
    } else {
        world.set_screen(id,false).expect("combatant has screens");
        world.bodies[id.0 as usize].controls.ecm=crate::world::controls::Mode::Off;
    }
    if class!=ShipClass::Picket {world.arm_beams(id).expect("combatant mounts a beam");}
    world.reset_platform_history(id);
}
fn fit_station(world:&mut World,id:BodyId) {
    {
        let b=&mut world.bodies[id.0 as usize];
        b.sensors=crate::sensors::SensorSuite::FULL;b.controllable=false;b.armed=false;
        b.damage.hull_max=crate::damage::FRIGATE_HULL_HP*ShipClass::Cruiser.scale();b.damage.hull=b.damage.hull_max;
        b.damage.armour_max=500.0*ShipClass::Cruiser.protection();b.damage.armour=b.damage.armour_max;
    }
    world.fit_point_defence(id);
    {let pd=world.bodies[id.0 as usize].point_defence.as_mut().unwrap();pd.lasers=ShipClass::Cruiser.pd_lasers();pd.rate_hz=1.0;}
    world.bodies[id.0 as usize].interceptor_battery=Some(crate::world::interceptor::Battery {rounds:ShipClass::Cruiser.interceptors(),launched:0,ready_at:0.0,status:"Ready"});
    world.reset_platform_history(id);
}
fn silence_probes(world:&mut World) {
    world.probes_enabled=crate::params::PROBES_ENABLED;
    if !world.probes_enabled {for b in &mut world.bodies {b.probes=0;}}
}
fn fleet_slots(anchor:Vec2,facing:Vec2)->Vec<Vec2> {
    let facing=facing.normalized();
    let back=facing*-1.0;
    let side=Vec2::new(-facing.y,facing.x);
    let spacing=0.045*AU;
    let mut slots=vec![anchor];
    for row in 1..=3 {for col in [-1.0_f64,0.0,1.0] {slots.push(anchor+back*(row as f64*spacing)+side*(col*spacing));}}
    slots
}
fn screen_offsets()->[Vec2;3] {[Vec2::new(3.0,0.0)*LIGHT_SECOND,Vec2::new(-2.0,2.2)*LIGHT_SECOND,Vec2::new(-2.0,-2.2)*LIGHT_SECOND]}

fn raid(seed:u64,system:System)->World {
    let home=system.state(1,0.0).pos;
    let mut rng=Rng::stream(seed,0x52414944);
    let base=std::f64::consts::TAU*rng.uniform();
    let (player_pos,station_pos)=(0..72).find_map(|step| {
        let angle=base+step as f64*std::f64::consts::TAU/72.0;
        let axis=Vec2::new(angle.cos(),angle.sin());
        let station=home+axis*(5.0*AU);
        let player=home-axis*(4.0*AU);
        let screens=screen_offsets().map(|off|station+off);
        (clear_of_celestials(&system,station)&&clear_of_celestials(&system,player)&&screens.iter().all(|p|clear_of_celestials(&system,*p))).then_some((player,station))
    }).unwrap_or((home-Vec2::new(8.0*AU,0.0),home+Vec2::new(8.0*AU,0.0)));
    let mut specs=vec![spec("Cruiser",BodyKind::Ship,ESCORT,player_pos,1),spec("Station",BodyKind::Station,RAIDER,station_pos,0)];
    for (i,off) in screen_offsets().into_iter().enumerate() {specs.push(spec(&format!("Screen {i}"),BodyKind::Ship,RAIDER,station_pos+off,1));}
    let mut world=World::new(system,specs,12000.0,seed);
    fit_combatant(&mut world,BodyId(0),ShipClass::Cruiser,true);
    fit_station(&mut world,BodyId(1));
    for (i,class) in RAID_SCREEN.into_iter().enumerate() {
        let id=BodyId(2+i as u32);
        fit_combatant(&mut world,id,class,true);
        let limit=class.max_g()*crate::units::G0;
        world.set_follow(id,BodyId(1)).expect("escorts screen the station");
        world.bodies[id.0 as usize].drive_limit=limit;
    }
    world.objective=Some(Objective {sensor_site:None,name:"enemy station".into(),center:station_pos,radius:0.08*AU,
        protect:BodyId(0),player:Some(BodyId(0)),defeat:None,prize:Some(BodyId(1)),wipe:false,defender:RAIDER,attacker:ESCORT});
    silence_probes(&mut world);
    world
}

fn placed(rng:&mut Rng,system:&System,home:Vec2,avoid:&[Vec2])->Vec2 {
    for attempt in 0..4000 {
        let span=10.0+attempt as f64*0.002;
        let radius=(2.0+(span-2.0)*rng.uniform())*AU;
        let angle=std::f64::consts::TAU*rng.uniform();
        let pos=home+Vec2::new(angle.cos(),angle.sin())*radius;
        let separation=if attempt>2500 {3.0*AU} else {4.0*AU};
        if clear_of_celestials(system,pos)&&avoid.iter().all(|p|(pos-*p).length()>=separation) {return pos;}
    }
    home+Vec2::new(8.0*AU,4.0*AU)
}
fn hide_and_seek(seed:u64,system:System)->World {
    let home=system.state(1,0.0).pos;
    let mut rng=Rng::stream(seed,0x48494445);
    let player=placed(&mut rng,&system,home,&[]);
    let quarry=placed(&mut rng,&system,home,&[player]);
    let hunt=placed(&mut rng,&system,home,&[player,quarry]);
    let mut world=World::new(system,vec![spec("Hunter",BodyKind::Ship,ESCORT,player,1),spec("Quarry",BodyKind::Ship,RAIDER,quarry,1)],0.0,seed);
    fit_combatant(&mut world,BodyId(0),ShipClass::Frigate,false);
    fit_combatant(&mut world,BodyId(1),ShipClass::Frigate,false);
    world.set_move(BodyId(1),hunt).expect("the quarry can cross open space");
    world.objective=Some(Objective {sensor_site:None,name:"hunting ground".into(),center:hunt,radius:0.5*AU,
        protect:BodyId(0),player:Some(BodyId(0)),defeat:Some(BodyId(1)),prize:None,wipe:false,defender:ESCORT,attacker:RAIDER});
    silence_probes(&mut world);
    world
}

fn armada(seed:u64,system:System)->World {
    let home=system.state(1,0.0).pos;
    let mut rng=Rng::stream(seed,0x41524D41);
    let base=std::f64::consts::TAU*rng.uniform();
    let (player_slots,enemy_slots)=(0..72).find_map(|step| {
        let angle=base+step as f64*std::f64::consts::TAU/72.0;
        let axis=Vec2::new(angle.cos(),angle.sin());
        let player=fleet_slots(home-axis*(7.0*AU),axis);
        let enemy=fleet_slots(home+axis*(7.0*AU),-axis);
        player.iter().chain(&enemy).all(|p|clear_of_celestials(&system,*p)).then_some((player,enemy))
    }).unwrap_or_else(|| {
        let axis=Vec2::new(1.0,0.0);
        (fleet_slots(home-axis*(16.0*AU),axis),fleet_slots(home+axis*(16.0*AU),-axis))
    });
    let mut specs=Vec::new();
    for (i,pos) in player_slots.iter().enumerate() {specs.push(spec(&format!("Ally {i}"),BodyKind::Ship,ESCORT,*pos,1));}
    for (i,pos) in enemy_slots.iter().enumerate() {specs.push(spec(&format!("Enemy {i}"),BodyKind::Ship,RAIDER,*pos,1));}
    let mut world=World::new(system,specs,12000.0,seed);
    for (i,class) in ARMADA_MIX.into_iter().enumerate() {
        fit_combatant(&mut world,BodyId(i as u32),class,true);
        fit_combatant(&mut world,BodyId(10+i as u32),class,true);
        world.set_move(BodyId(10+i as u32),player_slots[i]).expect("the enemy fleet can close");
    }
    world.objective=Some(Objective {sensor_site:None,name:"opposing fleet".into(),center:enemy_slots[0],radius:0.25*AU,
        protect:BodyId(0),player:Some(BodyId(0)),defeat:None,prize:None,wipe:true,defender:RAIDER,attacker:ESCORT});
    silence_probes(&mut world);
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

    #[test]
    fn escort_scenario_is_destroyer_versus_destroyer() {
        let w=Scenario::Escort.build(42,home_system());
        assert_eq!(w.bodies[1].ship_class,Some(ShipClass::Destroyer));
        assert_eq!(w.bodies[2].ship_class,Some(ShipClass::Destroyer));
        let o=w.objective.as_ref().unwrap();
        assert_eq!((o.player,o.defeat,o.prize,o.wipe),(Some(BodyId(1)),Some(BodyId(2)),None,false));
    }

    #[test]
    fn raid_is_a_cruiser_against_a_screened_station() {
        let w=Scenario::Raid.build(42,home_system());
        assert_eq!(w.bodies[0].ship_class,Some(ShipClass::Cruiser));
        assert_eq!(w.bodies[0].faction,ESCORT);
        assert_eq!(w.bodies[1].kind,BodyKind::Station);
        assert_eq!(w.bodies[1].faction,RAIDER);
        assert!(w.bodies[1].sensors.active&&w.bodies[1].sensors.passive&&w.bodies[1].damage.hull>=ShipClass::Cruiser.scale()*1000.0);
        assert_eq!(w.bodies[2..].iter().map(|b|b.ship_class.unwrap()).collect::<Vec<_>>(),RAID_SCREEN.to_vec());
        for b in &w.bodies[2..] {
            assert_eq!(b.faction,RAIDER);
            assert!(b.ship_class.unwrap().scale()<ShipClass::Cruiser.scale());
            assert!(matches!(b.autopilot.map(|a|a.order),Some(crate::world::Order::Follow {target:BodyId(1),..})));
        }
        let o=w.objective.as_ref().unwrap();
        assert_eq!((o.prize,o.player,o.attacker,o.defender),(Some(BodyId(1)),Some(BodyId(0)),ESCORT,RAIDER));
        let player=w.bodies[0].trajectory.state_at(0.0).unwrap().pos;
        let station=w.bodies[1].trajectory.state_at(0.0).unwrap().pos;
        assert!((player-station).length()>6.0*AU);
        assert_eq!(player,Scenario::Raid.build(42,home_system()).bodies[0].trajectory.state_at(0.0).unwrap().pos);
    }

    #[test]
    fn hide_and_seek_places_two_frigates_out_of_contact() {
        let w=Scenario::HideAndSeek.build(42,home_system());
        assert_eq!((w.bodies[0].ship_class,w.bodies[1].ship_class),(Some(ShipClass::Frigate),Some(ShipClass::Frigate)));
        assert_eq!((w.bodies[0].magazine,w.bodies[0].damage.hull,w.bodies[0].damage.armour),(w.bodies[1].magazine,w.bodies[1].damage.hull,w.bodies[1].damage.armour));
        assert!(!w.bodies[0].screen_up&&!w.bodies[1].screen_up);
        let hunter=w.bodies[0].trajectory.state_at(0.0).unwrap().pos;
        let quarry=w.bodies[1].trajectory.state_at(0.0).unwrap().pos;
        assert!((hunter-quarry).length()>=4.0*AU-1.0);
        let o=w.objective.as_ref().unwrap();
        assert_eq!((o.player,o.defeat),(Some(BodyId(0)),Some(BodyId(1))));
        assert!((o.center-hunter).length()>AU&&(o.center-quarry).length()>AU);
        let session=crate::session::LocalSession::new(w);
        assert!(session.view(crate::session::Role::Faction(ESCORT)).contacts.is_empty());
        assert!(session.view(crate::session::Role::Faction(RAIDER)).contacts.is_empty());
        assert_eq!(hunter,Scenario::HideAndSeek.build(42,home_system()).bodies[0].trajectory.state_at(0.0).unwrap().pos);
        assert_ne!(hunter,Scenario::HideAndSeek.build(7,home_system()).bodies[0].trajectory.state_at(0.0).unwrap().pos);
    }

    #[test]
    fn armada_mirrors_ten_ships_from_opposite_ends() {
        let w=Scenario::Armada.build(42,home_system());
        assert_eq!(w.bodies.len(),20);
        let allies:Vec<_>=w.bodies[..10].iter().map(|b|b.ship_class.unwrap()).collect();
        let enemies:Vec<_>=w.bodies[10..].iter().map(|b|b.ship_class.unwrap()).collect();
        assert_eq!(allies,ARMADA_MIX.to_vec());
        assert_eq!(enemies,allies);
        assert!(w.bodies[..10].iter().all(|b|b.faction==ESCORT));
        assert!(w.bodies[10..].iter().all(|b|b.faction==RAIDER));
        assert!(w.bodies[0].autopilot.is_none());
        assert!(w.bodies[10..].iter().all(|b|matches!(b.autopilot.map(|a|a.order),Some(crate::world::Order::MoveTo {..}))));
        let player=w.bodies[0].trajectory.state_at(0.0).unwrap().pos;
        let enemy=w.bodies[10].trajectory.state_at(0.0).unwrap().pos;
        assert!((player-enemy).length()>10.0*AU);
        let o=w.objective.unwrap();
        assert!(o.wipe&&o.prize.is_none()&&o.player==Some(BodyId(0)));
        assert!((o.center-enemy).length()<1.0);
        let mut session=crate::session::LocalSession::new(Scenario::Armada.build(42,home_system()));
        session.enable_bot(ESCORT,true);session.enable_bot(RAIDER,true);
        session.command(crate::session::Role::Spectator,crate::session::Command::SetPaused(false)).unwrap();
        // Wing orders travel at light speed across the formation, about 70s to the back rank.
        session.tick(120.0);
        let view=session.view(crate::session::Role::Spectator);
        assert!(view.bodies.iter().find(|b|b.id==BodyId(0)).unwrap().autopilot.is_none(),"the player's battleship stays on manual helm");
        let idle:Vec<_>=view.bodies.iter().filter(|b|b.kind==BodyKind::Ship && b.id!=BodyId(0) && b.autopilot.is_none()).map(|b|b.id).collect();
        assert!(idle.is_empty(),"ships without helm orders: {idle:?}");
    }
}
