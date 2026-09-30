//! Starting situations.
use crate::celestial::System;
use crate::kinematics::{State, Vec2};
use crate::params::{MAGAZINE_CRUISER, MAGAZINE_FRIGATE};
use crate::rng::Rng;
use crate::units::{AU, LIGHT_SECOND};
#[cfg(test)]
use crate::units::G0;
use crate::world::{BodyId, BodyKind, BodySpec, FactionId, Objective, ShipClass, Stance, World};

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
        stance: Stance::Intercept,
        withdrawal_continues:true,extract:false,escape_at_center:false,disengage_wins:false,prize_taken:false,escape_by:None,
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

/// One playable situation. Add a builder and a [`Play`] row, then list it in [`Scenario::ALL`].
struct Play {
    name:&'static str,
    /// Accepted by `LUMINAL_SCENARIO`, along with `name`.
    token:&'static str,
    brief:&'static str,
    forces:&'static str,
    detail:&'static str,
    /// Ship on the selection card. Deploy fields this class when `player_class` is set.
    flagship:ShipClass,
    /// Doctrine factions. The human's own ship stays on manual helm.
    bots:&'static [FactionId],
    /// Combatants follow the class passed to [`Scenario::build_with`] until deploy forces `flagship`.
    player_class:bool,
    /// Open the map on the protected body.
    inspect_protect:bool,
    /// Ship selected when the human takes the raider side.
    opposed_ship:Option<BodyId>,
    /// Doctrine factions when the human takes the raider side. Empty keeps `bots`.
    opposed_bots:&'static [FactionId],
    build:fn(u64,System,ShipClass)->World,
}

const RAIDER_SIDE:&[FactionId]=&[RAIDER];
const BOTH_SIDES:&[FactionId]=&[ESCORT,RAIDER];
const ESCORT_SIDE:&[FactionId]=&[ESCORT];
const NO_SIDE:&[FactionId]=&[];

fn escort_world(seed:u64,system:System,class:ShipClass)->World {transport_intercept_class_in_system(seed,class,system)}
fn raid_world(seed:u64,system:System,_:ShipClass)->World {raid(seed,system)}
fn hide_world(seed:u64,system:System,_:ShipClass)->World {hide_and_seek(seed,system)}
fn armada_world(seed:u64,system:System,_:ShipClass)->World {armada(seed,system)}
fn convoy_world(seed:u64,system:System,_:ShipClass)->World {convoy(seed,system)}
fn relief_world(seed:u64,system:System,_:ShipClass)->World {relief(seed,system)}
fn last_world(seed:u64,system:System,_:ShipClass)->World {last_ship(seed,system)}

static ESCORT_PLAY:Play=Play {
    name:"Escort",token:"escort",brief:"Escort the transport past a destroyer.",forces:"Destroyer versus destroyer",
    detail:"Both sides field a destroyer. The transport must reach the departure region. Destroying the raider wins at once. The raider escaping does not.",
    flagship:ShipClass::Destroyer,bots:RAIDER_SIDE,player_class:true,inspect_protect:true,opposed_ship:Some(BodyId(2)),opposed_bots:ESCORT_SIDE,build:escort_world,
};
static RAID_PLAY:Play=Play {
    name:"Raid",token:"raid",brief:"A cruiser raid on a screened station.",forces:"Cruiser versus a station and its escorts",
    detail:"Use a moon's sensor shadow, send a probe, or jump in blind. Destroy the station, then withdraw. The screen holds until you close.",
    flagship:ShipClass::Cruiser,bots:RAIDER_SIDE,player_class:false,inspect_protect:false,opposed_ship:None,opposed_bots:NO_SIDE,build:raid_world,
};
static HIDE_PLAY:Play=Play {
    name:"Hide and Seek",token:"hide",brief:"Two frigates. Find the other one.",forces:"Frigate versus frigate",
    detail:"Two frigates, placed at random and out of contact. Neither ship can jump. The hunting ground is closer to you than to the quarry, which has thirty-six hours to reach it. A slow coast will not get there. Destroy it, or it wins on arrival.",
    flagship:ShipClass::Frigate,bots:RAIDER_SIDE,player_class:false,inspect_protect:false,opposed_ship:None,opposed_bots:NO_SIDE,build:hide_world,
};
static ARMADA_PLAY:Play=Play {
    name:"Armada",token:"armada",brief:"Ten ships each, from opposite ends.",forces:"Battleship and a mixed fleet of nine",
    detail:"Your battleship is at the rear of nine ships, and they hold formation on you. Fleet orders travel at light speed: screen, close, or weapons free.",
    flagship:ShipClass::Battleship,bots:RAIDER_SIDE,player_class:false,inspect_protect:false,opposed_ship:None,opposed_bots:NO_SIDE,build:armada_world,
};
static CONVOY_PLAY:Play=Play {
    name:"Convoy",token:"convoy",brief:"Several transports, one screen.",forces:"Destroyer and a frigate escorting two transports",
    detail:"Two transports run for the departure region. Your destroyer is at the back, and a frigate holds formation on you. One raider destroyer is on the route.",
    flagship:ShipClass::Destroyer,bots:RAIDER_SIDE,player_class:false,inspect_protect:false,opposed_ship:None,opposed_bots:NO_SIDE,build:convoy_world,
};
static RELIEF_PLAY:Play=Play {
    name:"Relief",token:"relief",brief:"Jump into a fight already under way.",forces:"A hot cruiser, an ally destroyer, two frigates",
    detail:"A destroyer is already engaged. You arrive several AU out, hot from the jump, with only the light that has reached you.",
    flagship:ShipClass::Cruiser,bots:BOTH_SIDES,player_class:false,inspect_protect:false,opposed_ship:None,opposed_bots:NO_SIDE,build:relief_world,
};
static LAST_PLAY:Play=Play {
    name:"Last Ship",token:"last",brief:"A damaged cruiser. Repair the drive or run.",forces:"Damaged cruiser versus a frigate",
    detail:"Propulsion is damaged. Repair it and kill the frigate, or withdraw. Being destroyed loses.",
    flagship:ShipClass::Cruiser,bots:RAIDER_SIDE,player_class:false,inspect_protect:false,opposed_ship:None,opposed_bots:NO_SIDE,build:last_world,
};

/// Player-facing situations. Identity is the catalog row, so a new scenario does not grow a match.
#[derive(Clone,Copy)]
pub struct Scenario(&'static Play);

impl PartialEq for Scenario {
    fn eq(&self,other:&Self)->bool {std::ptr::eq(self.0,other.0)}
}
impl Eq for Scenario {}
impl std::fmt::Debug for Scenario {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {f.write_str(self.name())}
}

#[allow(non_upper_case_globals)]
impl Scenario {
    pub const Escort:Self=Self(&ESCORT_PLAY);
    pub const Raid:Self=Self(&RAID_PLAY);
    pub const HideAndSeek:Self=Self(&HIDE_PLAY);
    pub const Armada:Self=Self(&ARMADA_PLAY);
    pub const Convoy:Self=Self(&CONVOY_PLAY);
    pub const Relief:Self=Self(&RELIEF_PLAY);
    pub const LastShip:Self=Self(&LAST_PLAY);
    /// Card order on the startup screen.
    pub const ALL:[Self;7]=[Self::Escort,Self::Raid,Self::HideAndSeek,Self::Armada,Self::Convoy,Self::Relief,Self::LastShip];
    fn play(self)->&'static Play {self.0}
    pub fn name(self)->&'static str {self.play().name}
    pub fn token(self)->&'static str {self.play().token}
    pub fn brief(self)->&'static str {self.play().brief}
    pub fn forces(self)->&'static str {self.play().forces}
    pub fn detail(self)->&'static str {self.play().detail}
    pub fn flagship(self)->ShipClass {self.play().flagship}
    pub fn bots(self)->&'static [FactionId] {self.play().bots}
    /// Doctrine factions. A raider-side human uses `opposed_bots` when the row sets them.
    pub fn bots_for(self,human:Option<FactionId>)->&'static [FactionId] {
        if human==Some(RAIDER) && !self.play().opposed_bots.is_empty() {self.play().opposed_bots} else {self.bots()}
    }
    /// Class written at deploy. `None` leaves the builder's own ships alone.
    pub fn deployed_class(self)->Option<ShipClass> {self.play().player_class.then_some(self.flagship())}
    pub fn opposed_ship(self)->Option<BodyId> {self.play().opposed_ship}
    pub fn inspected(self,world:&World)->Option<BodyId> {
        self.play().inspect_protect.then(||world.objective.as_ref().map(|o|o.protect)).flatten()
    }
    pub fn parse(name:&str)->Option<Self> {
        let name=name.trim();
        Self::ALL.into_iter().find(|scenario|scenario.name().eq_ignore_ascii_case(name)||scenario.token().eq_ignore_ascii_case(name))
    }
    /// Fixed flagship. Escort's startup class override goes through [`Self::build_with`].
    pub fn build(self,seed:u64,system:System)->World {self.build_with(seed,system,self.flagship())}
    pub fn build_with(self,seed:u64,system:System,class:ShipClass)->World {
        let class=if self.play().player_class {class} else {self.flagship()};
        (self.play().build)(seed,system,class)
    }
}

const ARMADA_MIX:[ShipClass;10]=[
    ShipClass::Battleship,ShipClass::Cruiser,ShipClass::Destroyer,ShipClass::Destroyer,
    ShipClass::Frigate,ShipClass::Frigate,ShipClass::Frigate,ShipClass::Picket,ShipClass::Picket,ShipClass::Picket,
];
const RAID_SCREEN:[ShipClass;3]=[ShipClass::Destroyer,ShipClass::Frigate,ShipClass::Frigate];

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
/// `anchor` is the capital at the rear. Lighter ships step forward along `facing`.
fn fleet_slots(anchor:Vec2,facing:Vec2)->Vec<Vec2> {
    let scales:Vec<f64>=ARMADA_MIX[1..].iter().map(|c|c.scale()).collect();
    let mut slots=vec![anchor];
    slots.extend(crate::world::formation_offsets(facing,&scales).into_iter().map(|off|anchor+off));
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
        protect:BodyId(0),player:Some(BodyId(0)),defeat:None,prize:Some(BodyId(1)),wipe:false,defender:RAIDER,attacker:ESCORT,stance:Stance::Screen,
        withdrawal_continues:false,extract:true,escape_at_center:false,disengage_wins:false,prize_taken:false,escape_by:None});
    silence_probes(&mut world);
    // The cruiser carries the only probes. The global gate stays off for every other scenario.
    world.probes_enabled=true;
    world.bodies[0].probes=3;
    world
}

/// Thirty-six hours. The quarry's longer leg still fits a burn of under an hour, then a coast. A few km/s does not.
const HIDE_ESCAPE_S: f64 = 36.0 * 3600.0;
const HIDE_SEP_MIN_AU: f64 = 4.0;
const HIDE_SEP_MAX_AU: f64 = 5.4;
/// Past the midpoint, toward the hunter, so the pursuer is strictly closer.
const HIDE_HUNTER_BIAS_AU: f64 = 0.15;
/// A fifty-minute burn and a coast still cover this inside the escape clock.
const HIDE_QUARRY_LEG_AU: f64 = 3.0;

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
fn place_quarry(rng:&mut Rng,system:&System,hunter:Vec2)->Vec2 {
    for _ in 0..4000 {
        let sep=(HIDE_SEP_MIN_AU+(HIDE_SEP_MAX_AU-HIDE_SEP_MIN_AU)*rng.uniform())*AU;
        let angle=std::f64::consts::TAU*rng.uniform();
        let pos=hunter+Vec2::new(angle.cos(),angle.sin())*sep;
        if clear_of_celestials(system,pos) {return pos;}
    }
    for step in 0..64 {
        let angle=step as f64*std::f64::consts::TAU/64.0;
        let pos=hunter+Vec2::new(angle.cos(),angle.sin())*(4.5*AU);
        if clear_of_celestials(system,pos) {return pos;}
    }
    hunter+Vec2::new(4.5*AU,0.0)
}
fn hunting_ground(system:&System,quarry:Vec2,hunter:Vec2)->Vec2 {
    let axis=hunter-quarry;
    let axis=if axis.length()>1.0 {axis.normalized()} else {Vec2::new(1.0,0.0)};
    let side=Vec2::new(-axis.y,axis.x);
    let along=(hunter+quarry)*0.5+axis*(HIDE_HUNTER_BIAS_AU*AU);
    let cap=HIDE_QUARRY_LEG_AU*AU;
    let clear=|pos:Vec2| {
        let leg=(pos-quarry).length();
        let near=(pos-hunter).length();
        leg<=cap && near<leg && system.bodies.iter().enumerate().all(|(i,b)|(pos-system.state(i,0.0).pos).length()>b.radius+0.55*AU)
    };
    if clear(along) {return along;}
    for step in 1..40 {
        let lat=step as f64*0.05*AU;
        for sign in [1.0_f64,-1.0] {
            let pos=along+side*(lat*sign);
            if clear(pos) {return pos;}
        }
    }
    along
}
fn hide_and_seek(seed:u64,system:System)->World {
    let home=system.state(1,0.0).pos;
    let mut rng=Rng::stream(seed,0x48494445);
    let player=placed(&mut rng,&system,home,&[]);
    let quarry=place_quarry(&mut rng,&system,player);
    let hunt=hunting_ground(&system,quarry,player);
    let mut world=World::new(system,vec![spec("Hunter",BodyKind::Ship,ESCORT,player,1),spec("Quarry",BodyKind::Ship,RAIDER,quarry,1)],0.0,seed);
    fit_combatant(&mut world,BodyId(0),ShipClass::Frigate,false);
    fit_combatant(&mut world,BodyId(1),ShipClass::Frigate,false);
    world.set_move(BodyId(1),hunt).expect("the quarry can cross open space");
    world.objective=Some(Objective {sensor_site:None,name:"hunting ground".into(),center:hunt,radius:0.5*AU,
        protect:BodyId(0),player:Some(BodyId(0)),defeat:Some(BodyId(1)),prize:None,wipe:false,defender:ESCORT,attacker:RAIDER,stance:Stance::Evade,
        withdrawal_continues:false,extract:false,escape_at_center:true,disengage_wins:false,prize_taken:false,escape_by:Some(HIDE_ESCAPE_S)});
    silence_probes(&mut world);
    world.jumps_enabled=false;
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
    }
    // Wings hold station on their flagship. Only the enemy capital closes; its wing stays in formation until it has a track.
    for i in 1..ARMADA_MIX.len() {
        world.set_station(BodyId(i as u32),BodyId(0),player_slots[i]-player_slots[0]).expect("the wing holds station");
        world.set_station(BodyId(10+i as u32),BodyId(10),enemy_slots[i]-enemy_slots[0]).expect("the enemy wing holds station");
    }
    world.set_move(BodyId(10),player_slots[0]).expect("the enemy flagship can close");
    world.objective=Some(Objective {sensor_site:None,name:"opposing fleet".into(),center:enemy_slots[0],radius:0.25*AU,
        protect:BodyId(0),player:Some(BodyId(0)),defeat:None,prize:None,wipe:true,defender:RAIDER,attacker:ESCORT,stance:Stance::Battle,
        withdrawal_continues:false,extract:false,escape_at_center:false,disengage_wins:false,prize_taken:false,escape_by:None});
    silence_probes(&mut world);
    world
}

fn open_axis(system:&System,seed:u64,salt:u64)->Vec2 {
    let mut rng=Rng::stream(seed,salt);
    let home=system.state(1,0.0).pos;
    let base=std::f64::consts::TAU*rng.uniform();
    (0..72).find_map(|step| {
        let angle=base+step as f64*std::f64::consts::TAU/72.0;
        let axis=Vec2::new(angle.cos(),angle.sin());
        let clear=[0.4,1.2,2.2,3.2,4.5,6.5].iter().all(|au|clear_of_celestials(system,home+axis*(*au*AU)));
        clear.then_some(axis)
    }).unwrap_or(Vec2::new(1.0,0.0))
}
fn fit_transport(world:&mut World,id:BodyId) {
    {
        let b=&mut world.bodies[id.0 as usize];
        b.ship_class=Some(ShipClass::Transport);
        b.drive_limit=25.0*crate::units::G0;
        b.armed=false;b.controllable=false;b.has_screen=false;b.screen_up=false;
        b.baseline_emission_factor=crate::params::TRANSPORT_EMISSION_FACTOR.value;
        b.visibility_multiplier=2.0;b.magazine=[0,0];
    }
    world.reset_platform_history(id);
}
fn convoy(seed:u64,system:System)->World {
    let home=system.state(1,0.0).pos;
    let axis=open_axis(&system,seed,0x434F4E56);
    let lead=home+axis*(0.45*AU);
    let second=lead-axis*(0.03*AU);
    let screen=second-axis*(0.04*AU);
    let player=screen-axis*(0.04*AU);
    let departure=home+axis*(2.2*AU);
    let raider=home+axis*(3.1*AU);
    let specs=vec![
        spec("Destroyer",BodyKind::Ship,ESCORT,player,1),
        spec("Transport",BodyKind::Ship,ESCORT,lead,0),
        spec("Transport 2",BodyKind::Ship,ESCORT,second,0),
        spec("Frigate",BodyKind::Ship,ESCORT,screen,1),
        spec("Raider",BodyKind::Ship,RAIDER,raider,1),
    ];
    let mut world=World::new(system,specs,0.0,seed);
    fit_combatant(&mut world,BodyId(0),ShipClass::Destroyer,true);
    fit_transport(&mut world,BodyId(1));
    fit_transport(&mut world,BodyId(2));
    fit_combatant(&mut world,BodyId(3),ShipClass::Frigate,true);
    fit_combatant(&mut world,BodyId(4),ShipClass::Destroyer,true);
    world.set_move(BodyId(1),departure).expect("the lead transport can run");
    let limit=25.0*crate::units::G0;
    world.set_follow(BodyId(2),BodyId(1)).expect("the second transport follows");
    world.bodies[2].drive_limit=limit;
    world.set_station(BodyId(3),BodyId(0),screen-player).expect("the frigate holds station on the destroyer");
    world.objective=Some(Objective {sensor_site:None,name:"departure region".into(),center:departure,radius:0.05*AU,
        protect:BodyId(1),player:Some(BodyId(0)),defeat:None,prize:None,wipe:false,defender:ESCORT,attacker:RAIDER,stance:Stance::Intercept,
        withdrawal_continues:false,extract:false,escape_at_center:false,disengage_wins:false,prize_taken:false,escape_by:None});
    silence_probes(&mut world);
    world
}
fn sight_clear(system:&System,from:Vec2,to:Vec2)->bool {
    (0..=8).all(|i| {
        let p=from+(to-from)*(i as f64/8.0);
        system.bodies.iter().enumerate().all(|(k,b)|(p-system.state(k,0.0).pos).length()>b.radius+0.05*AU)
    })
}
fn relief(seed:u64,system:System)->World {
    let home=system.state(1,0.0).pos;
    let mut rng=Rng::stream(seed,0x52454C49);
    let base=std::f64::consts::TAU*rng.uniform();
    // The cruiser arrives off the planet, or the world sits in the planet's sensor shadow and the fight is invisible.
    let (axis,_side,_fight,player,ally,raider_a,raider_b)=(0..72).find_map(|step| {
        let angle=base+step as f64*std::f64::consts::TAU/72.0;
        let axis=Vec2::new(angle.cos(),angle.sin());
        let side=Vec2::new(-axis.y,axis.x);
        let fight=home+axis*(2.2*AU);
        let player=fight+side*(4.0*AU);
        let ally=fight-axis*(0.16*AU);
        let raider_a=fight+axis*(0.16*AU)+side*(0.04*AU);
        let raider_b=fight+axis*(0.2*AU)-side*(0.04*AU);
        let spots=[fight,player,ally,raider_a,raider_b];
        (spots.iter().all(|p|clear_of_celestials(&system,*p)) && sight_clear(&system,player,raider_a) && sight_clear(&system,player,raider_b))
            .then_some((axis,side,fight,player,ally,raider_a,raider_b))
    }).unwrap_or_else(|| {
        let axis=Vec2::new(1.0,0.0);
        let side=Vec2::new(0.0,1.0);
        let fight=home+axis*(2.2*AU);
        (axis,side,fight,fight+side*(4.0*AU),fight-axis*(0.16*AU),fight+axis*(0.16*AU)+side*(0.04*AU),fight+axis*(0.2*AU)-side*(0.04*AU))
    });
    let burn=40.0*crate::units::G0;
    let mut specs=vec![
        spec("Cruiser",BodyKind::Ship,ESCORT,player,1),
        spec("Ally",BodyKind::Ship,ESCORT,ally,1),
        spec("Raider",BodyKind::Ship,RAIDER,raider_a,1),
        spec("Raider 2",BodyKind::Ship,RAIDER,raider_b,1),
    ];
    specs[1].thrust=axis*burn;
    specs[2].thrust=-axis*burn;
    specs[3].thrust=-axis*burn;
    let mut world=World::new(system,specs,6000.0,seed);
    fit_combatant(&mut world,BodyId(0),ShipClass::Cruiser,false);
    let scale=world.bodies[0].thermal.capacity_scale;
    world.bodies[0].thermal.add_waste_heat(crate::params::SHIP_HEAT_LIMIT_J*scale*crate::world::jump::ARRIVAL_HEAT_FRACTION);
    fit_combatant(&mut world,BodyId(1),ShipClass::Destroyer,true);
    fit_combatant(&mut world,BodyId(2),ShipClass::Frigate,true);
    fit_combatant(&mut world,BodyId(3),ShipClass::Frigate,true);
    let ally_now=world.bodies[1].trajectory.state_at(0.0).unwrap().pos;
    let foe=world.bodies[2].trajectory.state_at(0.0).unwrap().pos;
    world.set_move(BodyId(1),foe).expect("the ally can close");
    world.set_move(BodyId(2),ally_now).expect("the raider can close");
    world.set_move(BodyId(3),ally_now).expect("the second raider can close");
    world.objective=Some(Objective {sensor_site:None,name:"the engagement".into(),center:ally_now,radius:0.25*AU,
        protect:BodyId(0),player:Some(BodyId(0)),defeat:None,prize:None,wipe:true,defender:RAIDER,attacker:ESCORT,stance:Stance::Battle,
        withdrawal_continues:false,extract:false,escape_at_center:false,disengage_wins:false,prize_taken:false,escape_by:None});
    silence_probes(&mut world);
    world.sample_arriving_light();
    world
}
fn last_ship(seed:u64,system:System)->World {
    let home=system.state(1,0.0).pos;
    let axis=open_axis(&system,seed,0x4C415354);
    let player=home+axis*(2.4*AU);
    let hunter=player+axis*(1.8*AU);
    let mut world=World::new(system,vec![
        spec("Cruiser",BodyKind::Ship,ESCORT,player,1),
        spec("Pursuer",BodyKind::Ship,RAIDER,hunter,1),
    ],0.0,seed);
    fit_combatant(&mut world,BodyId(0),ShipClass::Cruiser,false);
    fit_combatant(&mut world,BodyId(1),ShipClass::Frigate,true);
    world.bodies[0].damage.systems[crate::damage::System::Propulsion as usize]=crate::damage::Condition::Damaged;
    world.set_move(BodyId(1),player).expect("the pursuer can close");
    world.objective=Some(Objective {sensor_site:None,name:"the pursuit".into(),center:player,radius:0.2*AU,
        protect:BodyId(0),player:Some(BodyId(0)),defeat:Some(BodyId(1)),prize:None,wipe:false,defender:ESCORT,attacker:RAIDER,stance:Stance::Intercept,
        withdrawal_continues:false,extract:false,escape_at_center:false,disengage_wins:true,prize_taken:false,escape_by:None});
    silence_probes(&mut world);
    world
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_names_and_tokens_are_unique_and_each_scenario_builds() {
        let mut names=Vec::new();
        let mut tokens=Vec::new();
        for scenario in Scenario::ALL {
            assert!(!names.contains(&scenario.name())&&!tokens.contains(&scenario.token()),"{scenario:?}");
            names.push(scenario.name());
            tokens.push(scenario.token());
            assert_eq!(Scenario::parse(scenario.name()),Some(scenario));
            assert_eq!(Scenario::parse(&scenario.token().to_uppercase()),Some(scenario));
            let world=scenario.build(42,home_system());
            let player=world.objective.as_ref().and_then(|o|o.player).expect("a scenario names its player");
            assert!(world.body(player).is_some(),"missing {player:?}");
            assert_eq!(scenario.build(42,home_system()).bodies[0].trajectory.state_at(0.0).unwrap().pos,world.bodies[0].trajectory.state_at(0.0).unwrap().pos);
        }
        assert!(Scenario::parse("").is_none()&&Scenario::parse("skirmish").is_none());
    }

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
            assert!(world.deploy_probe(id,Vec2::new(1.0,0.0),Vec2::new(1.0e6,0.0)).is_err());
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
        assert_eq!((o.player,o.defeat,o.prize,o.wipe,o.stance),(Some(BodyId(1)),Some(BodyId(2)),None,false,Stance::Intercept));
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
        assert_eq!((o.prize,o.player,o.attacker,o.defender,o.stance),(Some(BodyId(1)),Some(BodyId(0)),ESCORT,RAIDER,Stance::Screen));
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
        let sep=(hunter-quarry).length();
        assert!((HIDE_SEP_MIN_AU*AU-1.0..=HIDE_SEP_MAX_AU*AU+1.0).contains(&sep),"ships stay out of contact and inside the clock: {sep}");
        let o=w.objective.as_ref().unwrap();
        assert_eq!((o.player,o.defeat,o.stance),(Some(BodyId(0)),Some(BodyId(1)),Stance::Evade));
        assert_eq!(o.escape_by,Some(HIDE_ESCAPE_S));
        assert!(!w.jumps_enabled);
        let to_hunter=(o.center-hunter).length();
        let to_quarry=(o.center-quarry).length();
        assert!(to_hunter<to_quarry,"the pursuer is closer to the hunting ground");
        assert!(to_hunter>o.radius && to_quarry>o.radius,"neither ship starts inside the ground");
        assert!(9.0*HIDE_ESCAPE_S<to_quarry,"a 9 km/s coast cannot reach the hunting ground");
        let burn=50.0*60.0;
        let reach=ShipClass::Frigate.max_g()*G0*burn*(HIDE_ESCAPE_S-burn/2.0);
        assert!(burn<3600.0 && reach>to_quarry,"a burn under the heat wall can still arrive");
        for seed in [1_u64,7,99,256,1024] {
            let placed=Scenario::HideAndSeek.build(seed,home_system());
            let hunter=placed.bodies[0].trajectory.state_at(0.0).unwrap().pos;
            let quarry=placed.bodies[1].trajectory.state_at(0.0).unwrap().pos;
            let center=placed.objective.as_ref().unwrap().center;
            let sep=(hunter-quarry).length();
            assert!((HIDE_SEP_MIN_AU*AU-1.0..=HIDE_SEP_MAX_AU*AU+1.0).contains(&sep),"{seed}: {sep}");
            assert!((center-hunter).length()<(center-quarry).length(),"{seed}");
            assert!(9.0*HIDE_ESCAPE_S<(center-quarry).length(),"{seed}");
            assert!(!placed.jumps_enabled);
        }
        let session=crate::session::LocalSession::new(w);
        assert!(session.view(crate::session::Role::Faction(ESCORT)).contacts.is_empty());
        assert!(session.view(crate::session::Role::Faction(RAIDER)).contacts.is_empty());
        assert_eq!(hunter,Scenario::HideAndSeek.build(42,home_system()).bodies[0].trajectory.state_at(0.0).unwrap().pos);
        assert_ne!(hunter,Scenario::HideAndSeek.build(7,home_system()).bodies[0].trajectory.state_at(0.0).unwrap().pos);
    }

    #[test]
    fn hide_and_seek_rejects_jump_even_for_a_destroyer() {
        let mut hide=Scenario::HideAndSeek.build(42,home_system());
        for body in &mut hide.bodies {body.ship_class=Some(ShipClass::Destroyer);}
        let dest=Vec2::new(3.0*AU,0.0);
        assert_eq!(hide.start_jump(BodyId(0),dest),Err(crate::world::OrderError::JumpUnavailable));
        assert_eq!(hide.start_jump(BodyId(1),dest),Err(crate::world::OrderError::JumpUnavailable));
        hide.order_fleet_jump(BodyId(0),dest);
        assert!(hide.bodies.iter().all(|b|b.jump.is_none()));
        assert_eq!(hide.pending_orders(ESCORT),0);
        let mut escort=Scenario::Escort.build(42,home_system());
        assert!(escort.jumps_enabled);
        assert!(escort.start_jump(BodyId(1),dest).is_ok());
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
        assert!(w.bodies[1..10].iter().all(|b|b.drive_limit.is_finite() && matches!(b.autopilot.map(|a|a.order),Some(crate::world::Order::Follow {target:BodyId(0),..}))));
        assert!(matches!(w.bodies[10].autopilot.map(|a|a.order),Some(crate::world::Order::MoveTo {..})));
        assert!(w.bodies[11..].iter().all(|b|matches!(b.autopilot.map(|a|a.order),Some(crate::world::Order::Follow {target:BodyId(10),..}))));
        let player=w.bodies[0].trajectory.state_at(0.0).unwrap().pos;
        let enemy=w.bodies[10].trajectory.state_at(0.0).unwrap().pos;
        assert!((player-enemy).length()>10.0*AU);
        let axis=(enemy-player).normalized();
        let ahead=|id:usize|(w.bodies[id].trajectory.state_at(0.0).unwrap().pos-player).dot(axis);
        assert!(w.bodies[1..10].iter().enumerate().all(|(i,_)|ahead(i+1)>0.02*AU),"lighter ships should be ahead of the battleship");
        assert!(ahead(9)>ahead(1),"pickets should be ahead of the cruiser");
        assert!(w.bodies[11..].iter().all(|b|(b.trajectory.state_at(0.0).unwrap().pos-player).length()<(enemy-player).length()-0.02*AU));
        let o=w.objective.unwrap();
        assert!(o.wipe&&o.prize.is_none()&&o.player==Some(BodyId(0))&&o.stance==Stance::Battle);
        assert!((o.center-enemy).length()<1.0);
        let mut session=crate::session::LocalSession::new(Scenario::Armada.build(42,home_system()));
        session.enable_bot(ESCORT,true);session.enable_bot(RAIDER,true);
        session.command(crate::session::Role::Spectator,crate::session::Command::SetPaused(false)).unwrap();
        // Wing orders travel at light speed across four rows, about 90s to the van.
        session.tick(120.0);
        let view=session.view(crate::session::Role::Spectator);
        assert!(view.bodies.iter().find(|b|b.id==BodyId(0)).unwrap().autopilot.is_none(),"the player's battleship stays on manual helm");
        let idle:Vec<_>=view.bodies.iter().filter(|b|b.kind==BodyKind::Ship && b.id!=BodyId(0) && b.autopilot.is_none()).map(|b|b.id).collect();
        assert!(idle.is_empty(),"ships without helm orders: {idle:?}");
        let center=view.objective.as_ref().unwrap().center;
        for b in view.bodies.iter().filter(|b|b.faction==RAIDER) {
            if let Some(crate::world::Order::MoveTo {frame,offset})=b.autopilot.map(|a|a.order) {
                let dest=view.celestials[frame].pos+offset;
                assert!((dest-center).length()>AU,"{b:?} was sent back to its own anchor");
            }
        }
    }

    #[test]
    fn raid_screen_keeps_formation_under_the_bot() {
        let mut session=crate::session::LocalSession::new(Scenario::Raid.build(42,home_system()));
        session.enable_bot(RAIDER,true);
        session.command(crate::session::Role::Spectator,crate::session::Command::SetPaused(false)).unwrap();
        session.tick(60.0);
        let view=session.view(crate::session::Role::Spectator);
        let station=view.bodies.iter().find(|b|b.id==BodyId(1)).unwrap().pos;
        for id in [BodyId(2),BodyId(3),BodyId(4)] {
            let b=view.bodies.iter().find(|b|b.id==id).unwrap();
            assert!(matches!(b.autopilot.map(|a|a.order),Some(crate::world::Order::Follow {target:BodyId(1),..})),"{id:?} {:?}",b.autopilot);
            assert!((b.pos-station).length()<0.05*AU,"{id:?} left the station by {} km",(b.pos-station).length());
            assert!(b.drive_limit.is_finite() && b.drive_limit<=b.ship_class.unwrap().max_g()*G0*1.01);
        }
    }

    #[test]
    fn quarry_runs_without_lighting_itself() {
        let mut session=crate::session::LocalSession::new(Scenario::HideAndSeek.build(42,home_system()));
        session.enable_bot(RAIDER,true);
        session.command(crate::session::Role::Spectator,crate::session::Command::SetPaused(false)).unwrap();
        let start=session.view(crate::session::Role::Spectator).bodies.iter().find(|b|b.id==BodyId(1)).unwrap().pos;
        session.tick(90.0);
        let view=session.view(crate::session::Role::Spectator);
        assert!(view.pings.is_empty(),"the quarry pinged");
        let quarry=view.bodies.iter().find(|b|b.id==BodyId(1)).unwrap();
        assert!(!quarry.screen_up);
        assert_eq!(quarry.controls.screens,crate::world::controls::Mode::Off);
        assert_eq!(quarry.controls.ecm,crate::world::controls::Mode::Off);
        assert_eq!(quarry.controls.active,crate::world::controls::Mode::Off);
        assert!(quarry.autopilot.is_none() && quarry.thrust.length()>G0,"the quarry keeps burning until the ground is in reach: {:?}",quarry.autopilot);
        assert!(quarry.vel.length()>40.0,"ninety seconds at full thrust is already far past a 9 km/s drift");
        assert!((quarry.pos-start).length()>3_000.0);
    }

    #[test]
    fn coast_holds_the_vector_and_fleet_orders_arrive_late() {
        let mut session=crate::session::LocalSession::new(Scenario::HideAndSeek.build(42,home_system()));
        session.command(crate::session::Role::Faction(ESCORT),crate::session::Command::SetThrust {body:BodyId(0),thrust:Vec2::new(20.0*G0,0.0)}).unwrap();
        session.command(crate::session::Role::Spectator,crate::session::Command::SetPaused(false)).unwrap();
        session.tick(8.0);
        let moving=session.view(crate::session::Role::Spectator).bodies.iter().find(|b|b.id==BodyId(0)).unwrap().vel;
        session.command(crate::session::Role::Faction(ESCORT),crate::session::Command::Coast {body:BodyId(0)}).unwrap();
        let coasted=session.view(crate::session::Role::Spectator);
        let coasting=coasted.bodies.iter().find(|b|b.id==BodyId(0)).unwrap();
        assert!(coasting.autopilot.is_none() && coasting.thrust.length()<1e-6);
        session.tick(6.0);
        let later=session.view(crate::session::Role::Spectator).bodies.iter().find(|b|b.id==BodyId(0)).unwrap().vel;
        assert!((later-moving).length()<1.0,"coast changed the vector by {}",(later-moving).length());

        let mut fleet=crate::session::LocalSession::new(Scenario::Armada.build(42,home_system()));
        fleet.command(crate::session::Role::Faction(ESCORT),crate::session::Command::Fleet {body:BodyId(0),order:crate::session::FleetOrder::Screen}).unwrap();
        let view=fleet.view(crate::session::Role::Faction(ESCORT));
        assert_eq!(view.pending_orders,9);
        assert!(view.order_eta.iter().any(|(_,t)|*t>60.0),"{:?}",view.order_eta);
        // The wing is already in formation. The order in transit has not replaced those stations yet.
        let formed=|bodies:&[crate::session::BodyView]| bodies.iter().filter(|b|b.faction==ESCORT && b.id!=BodyId(0)).all(|b|matches!(b.autopilot.map(|a|a.order),Some(crate::world::Order::Follow {target:BodyId(0),..})));
        assert!(formed(&fleet.view(crate::session::Role::Spectator).bodies));
        fleet.command(crate::session::Role::Spectator,crate::session::Command::SetPaused(false)).unwrap();
        fleet.tick(5.0);
        assert!(fleet.view(crate::session::Role::Faction(ESCORT)).pending_orders>0,"a front-rank order has not had time to arrive");
        fleet.tick(130.0);
        let arrived=fleet.view(crate::session::Role::Spectator);
        assert!(arrived.bodies.iter().any(|b|b.faction==ESCORT && b.id!=BodyId(0) && matches!(b.autopilot.map(|a|a.order),Some(crate::world::Order::Follow {target:BodyId(0),..}))));
        assert_eq!(Scenario::Armada.bots(),&[RAIDER]);
        for order in [crate::session::FleetOrder::Close,crate::session::FleetOrder::WeaponsFree] {
            let mut session=crate::session::LocalSession::new(Scenario::Armada.build(42,home_system()));
            session.command(crate::session::Role::Faction(ESCORT),crate::session::Command::Fleet {body:BodyId(0),order}).unwrap();
            let view=session.view(crate::session::Role::Faction(ESCORT));
            assert_eq!(view.pending_orders,9,"{order:?} did not leave the flagship");
            assert!(formed(&session.view(crate::session::Role::Spectator).bodies),"{order:?} changed the formation before its light arrived");
        }
    }

    #[test]
    fn raid_cruiser_is_the_only_ship_that_can_launch_a_probe() {
        let mut world=Scenario::Raid.build(42,home_system());
        assert!(world.probes_enabled);
        assert_eq!(world.bodies[0].probes,3);
        assert!(world.bodies[1..].iter().all(|b|b.probes==0));
        let pos=world.bodies[0].trajectory.state_at(0.0).unwrap().pos;
        assert!(world.deploy_probe(BodyId(0),Vec2::new(1.0,0.0),pos+Vec2::new(1.0e6,0.0)).is_ok());
        assert!(world.bodies.iter().any(|b|b.kind==BodyKind::Probe));
        assert!(world.deploy_probe(BodyId(2),Vec2::new(1.0,0.0),Vec2::ZERO).is_err());
        assert!(!transport_intercept().probes_enabled);
    }

    #[test]
    fn convoy_relief_and_last_ship_build_their_own_fights() {
        let convoy=Scenario::Convoy.build(42,home_system());
        assert_eq!(convoy.bodies[0].ship_class,Some(ShipClass::Destroyer));
        assert!(convoy.bodies[1].kind==BodyKind::Ship && !convoy.bodies[1].armed && !convoy.bodies[2].armed);
        assert!(matches!(convoy.bodies[3].autopilot.map(|a|a.order),Some(crate::world::Order::Follow {target:BodyId(0),..})));
        assert!(convoy.bodies[3].drive_limit.is_finite());
        let o=convoy.objective.as_ref().unwrap();
        let player=convoy.bodies[0].trajectory.state_at(0.0).unwrap().pos;
        let frigate=convoy.bodies[3].trajectory.state_at(0.0).unwrap().pos;
        assert!((frigate-player).dot((o.center-player).normalized())>0.02*AU,"the frigate should be ahead of the destroyer");
        assert_eq!((o.protect,o.player,o.defeat,o.prize,o.wipe),(BodyId(1),Some(BodyId(0)),None,None,false));
        assert_eq!(convoy.bodies[4].faction,RAIDER);

        let relief=Scenario::Relief.build(42,home_system());
        let player=relief.bodies[0].trajectory.state_at(0.0).unwrap().pos;
        let ally=relief.bodies[1].trajectory.state_at(0.0).unwrap().pos;
        assert!((player-ally).length()>3.0*AU);
        assert!(!relief.bodies[0].screen_up);
        assert!(relief.bodies[0].thermal.heat_fraction()>=0.9);
        let o=relief.objective.as_ref().unwrap();
        assert!(o.wipe && o.stance==Stance::Battle && o.player==Some(BodyId(0)));
        assert_eq!(relief.bodies[1].ship_class,Some(ShipClass::Destroyer));
        assert!(relief.bodies[2..].iter().all(|b|b.ship_class==Some(ShipClass::Frigate) && b.faction==RAIDER));
        let session=crate::session::LocalSession::new(relief);
        let picture=session.view(crate::session::Role::Faction(ESCORT));
        assert!(picture.contacts.iter().any(|c|c.last_emitted_at<0.0),"stale light from the fight should already have arrived");

        let last=Scenario::LastShip.build(42,home_system());
        assert_eq!(last.bodies[0].damage.state(crate::damage::System::Propulsion),crate::damage::Condition::Damaged);
        assert_eq!(last.bodies[0].damage.state(crate::damage::System::Repair),crate::damage::Condition::Intact);
        assert!(last.bodies[0].installed_systems()[crate::damage::System::Jump as usize]);
        let o=last.objective.as_ref().unwrap();
        assert!(o.disengage_wins && o.defeat==Some(BodyId(1)) && o.player==Some(BodyId(0)));
    }

    #[test]
    fn hide_quarry_wins_by_reaching_the_hunting_ground() {
        let mut world=Scenario::HideAndSeek.build(42,home_system());
        let center=world.objective.as_ref().unwrap().center;
        world.bodies[1].trajectory=crate::kinematics::Trajectory::new(0.0,State {pos:center,vel:Vec2::ZERO});
        // Arrival is scored on the next integration step, and open space steps are a minute apart.
        world.advance_to(120.0);
        let outcome=world.outcome.as_ref().unwrap();
        assert_eq!(outcome.winner,RAIDER);
        assert!(outcome.reason.contains("reached"),"{}",outcome.reason);
    }

    #[test]
    fn hide_quarry_misses_the_clock_and_the_hunter_wins() {
        let mut world=Scenario::HideAndSeek.build(42,home_system());
        world.objective.as_mut().unwrap().escape_by=Some(60.0);
        world.advance_to(90.0);
        let outcome=world.outcome.as_ref().unwrap();
        assert_eq!(outcome.winner,ESCORT);
        assert!(outcome.reason.contains("missed"),"{}",outcome.reason);
        assert!((outcome.t-60.0).abs()<1e-6);
        assert!(world.time()>=90.0);
    }
}
