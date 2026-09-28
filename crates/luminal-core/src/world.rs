//! World truth. Only the simulation and the spectator may read it.
//!
//! Time advances through a discrete-event queue. Between events all motion is analytic
//! (committed constant-acceleration segments), so outcomes do not depend on frame rate
//! or warp. Events: per-body gravity steps and global sensor frames.

use crate::autopilot::{self, Avoidance};
use crate::celestial::System;
use crate::kinematics::{State, Trajectory, Vec2};
use crate::lightcone::{Front, retarded_state};
use crate::mind::{ContactId, Measurement, Observation, Perception, Source};
use crate::missile::{self, Payload, Phase, Seeker};
use crate::params::*;
use crate::rng::Rng;
use crate::scheduler::Scheduler;
use crate::sensors::{self, bearing_of};
use crate::units::AU;
use std::collections::BTreeMap;
#[path = "refinements.rs"]
mod refinements;
pub use refinements::{CombatEvent, CombatKind};
#[path = "calibration.rs"]
pub mod calibration;
#[path = "point_defence.rs"]
pub mod point_defence;
#[path = "interceptor.rs"]
pub mod interceptor;
#[path = "controls.rs"]
pub mod controls;
#[path = "weapon_probability.rs"]
pub mod weapon_probability;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FactionId(pub u8);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BodyId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyKind {
    Ship,
    Station,
    Probe,
    Missile,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShipClass { Frigate, Battleship, Cruiser, Transport }
impl ShipClass {
    pub fn designator(self)->&'static str {match self {Self::Frigate=>"FF",Self::Battleship=>"BB",Self::Cruiser=>"CV",Self::Transport=>"TR"}}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InterceptTarget {
    /// One of our contacts, steered by our track on it.
    Contact(ContactId),
    /// One of our own ships, known exactly.
    Own(BodyId),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Order {
    /// Circular orbit of `radius` km about celestial body `celestial`, in `sense`
    /// (+1 counter-clockwise, −1 clockwise).
    Orbit { celestial: usize, radius: f64, sense: f64 },
    /// Close on the target, match its velocity and hold station nearby.
    Intercept(InterceptTarget),
    /// Maximum-thrust encounter, then coast through without matching velocity.
    Flyby(InterceptTarget),
    KeepRange(InterceptTarget,f64),
    Evade(InterceptTarget),
    /// Fly to a point in minimal time and stop there. The point is `offset` from
    /// celestial `frame` and moves with it.
    MoveTo { frame: usize, offset: Vec2 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AutopilotStatus {
    Manoeuvring,
    Closing { eta: f64, range: f64 },
    Holding,
    Passed,
    /// The target contact has no track (bearing only) or is gone; coasting.
    NoTrack,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Autopilot {
    pub order: Order,
    pub status: AutopilotStatus,
}

#[derive(Clone, Debug)]
pub struct Body {
    pub controls:controls::Controls,
    pub last_missile_launch:Option<f64>,
    pub ship_class: Option<ShipClass>,
    pub damage:crate::damage::Damage,
    pub interceptor_battery:Option<interceptor::Battery>,
    pub interceptor:Option<interceptor::Interceptor>,
    pub point_defence: Option<point_defence::PointDefence>,
    pub has_screen: bool,
    pub baseline_emission_factor: f64,
    /// Extra signature multiplier for conspicuous, non-stealthy platforms.
    pub visibility_multiplier: f64,
    pub sensors: sensors::SensorSuite,
    pub probes: u32,
    pub probe_burn_until: Option<f64>,
    probe_ping_at: f64,
    pub thermal: crate::thermal::Thermal,
    launch_generation: u64,
    pub name: String,
    pub kind: BodyKind,
    pub faction: FactionId,
    pub trajectory: Trajectory,
    /// Manual thrust order, used when no autopilot order is active.
    pub commanded: Vec2,
    pub autopilot: Option<Autopilot>,
    /// Result of the last collision check.
    pub avoidance: Avoidance,
    /// Cap on autopilot thrust, km/s². Lower is quieter: drive emission scales with it.
    pub drive_limit: f64,
    /// Remaining missiles by payload, indexed by Payload::index().
    pub magazine: [u32; 2],
    /// Independent launcher cooldowns, indexed by payload.
    pub missile_ready_at: [f64; 2],
    pub missile_queued: [u32; 2],
    missile_queue_ready_at: [f64; 2],
    /// Present on missiles.
    pub missile: Option<MissileState>,
    /// Screen raised: incoming energy fills it before reaching the hull.
    pub screen_up: bool,
    /// Energy stored in the radiating field reservoir, J.
    pub screen_j: f64,
    /// Energy the hull has taken, J. The ship is lost at `HULL_INTEGRITY_J`.
    pub hull_j: f64,
    /// Started with weapons aboard: a combatant rather than, say, a transport.
    pub armed: bool,
    pub controllable: bool,
    pub beam_ready_at: f64,
    pub beam_target: Option<ContactId>,
    pub beam_auto: bool,
    beam_order: u64,
    /// Energy emitted by this ship's beam weapon, drawn from its capacitor.
    pub beam_emitted_j: f64,
    /// Own fire-control solution, not a report of a hit.
    pub last_beam: Option<(f64, Vec2, Vec2)>,
}

/// What a missile carries and knows. `target_body` is truth used only by the world to
/// resolve hits and physical sensing; guidance uses the contact and seeker.
#[derive(Clone, Copy, Debug)]
pub struct MissileState {
    pub payload: Payload,
    pub target: ContactId,
    pub launcher: BodyId,
    pub phase: Phase,
    /// Remaining delta-v, km/s.
    pub dv_left: f64,
    /// Remaining initial-burn delta-v, km/s.
    pub burn_left: f64,
    /// Launch bearing from received evidence, never target truth.
    pub search_heading: Vec2,
    pub active_seeker: bool,
    pub seeker: Option<Seeker>,
    pub local_fix: Option<sensors::SeekerFix>,
    pub last_guide: f64,
    /// Closest pass to the target so far, km (truth, for spending a missile that flew by).
    pub(crate) target_body: BodyId,
    /// Where it was launched, and its aim error per km flown (truth): the spread that
    /// makes long shots less accurate.
    pub(crate) launched_at: Vec2,
}

impl Body {
    pub fn emissivity_factors(&self,t:f64)->sensors::EmissivityFactors {
        let recent=|at:Option<f64>|at.is_some_and(|at|t>=at && t-at<60.0);
        let size=if self.kind==BodyKind::Station {20.0} else {match self.ship_class {Some(ShipClass::Frigate)=>7.0,Some(ShipClass::Battleship)=>20.0,_=>10.0}};
        let nominal=SHIP_MAX_ACCEL_G.value*crate::units::G0*if self.ship_class==Some(ShipClass::Transport) {0.5} else {1.0};
        sensors::EmissivityFactors {
            visibility_multiplier:self.visibility_multiplier,
            thrust_percent:100.0*self.trajectory.thrust_at(t).unwrap_or(Vec2::ZERO).length()/nominal,
            screen_percent:100.0*self.screen_j/SCREEN_CAPACITY_J.value,
            screen_on:self.has_screen && (self.screen_up || self.thermal.field>0.0) && self.operating_effectiveness(crate::damage::System::Screens)>0.0,
            size,stealth:100.0*(1.0-self.baseline_emission_factor.clamp(0.0,1.0)),
            ecm_on:self.ecm_strength()>0.0,
            recent_missiles:recent(self.last_missile_launch),
            recent_beams:recent(self.last_beam.map(|(t,_,_)|t)) || recent(self.point_defence.and_then(|p|p.last_shot.map(|(t,_,_)|t))),
        }
    }
    pub fn installed_systems(&self)->[bool;15] {
        use crate::damage::System as S;
        std::array::from_fn(|i|match S::ALL[i] {
            S::Passive=>self.sensors.passive,S::Active=>self.sensors.active,S::Direction=>self.sensors.direction_finding,
            S::Screens=>self.has_screen,S::PdMissiles=>self.interceptor_battery.is_some(),S::PdLaser=>self.point_defence.is_some(),
            S::Beam|S::Launcher=>self.armed,S::Propulsion=>self.kind==BodyKind::Ship,
            _=>matches!(self.kind,BodyKind::Ship|BodyKind::Station),
        })
    }
    pub fn system_effectiveness(&self,system:crate::damage::System)->f64 {
        if matches!(self.kind,BodyKind::Missile|BodyKind::Probe) {return 1.0;}
        if !self.installed_systems()[system as usize] {return 0.0;}
        self.damage.effectiveness(system)
    }
    pub fn operating_effectiveness(&self,system:crate::damage::System)->f64 {
        use crate::damage::System as S;
        if system.independent_power() {return self.system_effectiveness(system);}
        self.system_effectiveness(system)*self.system_effectiveness(S::Power)
            *self.system_effectiveness(S::Mind)
    }
    pub fn advance_thermal(&mut self,t:f64) {
        use crate::damage::System as S;
        let power=if self.controls.boost_active {0.0} else {self.system_effectiveness(S::Power)};
        if self.controls.boost_active && let Some(pd)=self.point_defence.as_mut()
            && pd.next_shot_at>self.thermal.last_t {
            pd.next_shot_at+=(t-self.thermal.last_t).max(0.0);
        }
        let screen=self.operating_effectiveness(S::Screens);
        self.thermal.advance_scaled(t,self.screen_up && screen>0.0,&mut self.screen_j,power,screen);
    }
    pub fn sensor_effectiveness(&self)->[f64;2] {
        [self.operating_effectiveness(crate::damage::System::Passive),self.operating_effectiveness(crate::damage::System::Direction)]
    }
    pub fn alive_at(&self, t: f64) -> bool {
        self.trajectory.state_at(t).is_some()
    }

    pub fn max_accel(&self) -> f64 {
        self.operating_effectiveness(crate::damage::System::Propulsion)*crate::units::G0
            * match self.kind {
                BodyKind::Ship => SHIP_MAX_ACCEL_G.value*self.damage.hull_thrust_factor()*if self.controls.boost_active {1.2} else {1.0}
                    * if self.ship_class==Some(ShipClass::Transport) {0.5} else {1.0},
                BodyKind::Station => 0.0,
                BodyKind::Probe => PROBE_MAX_ACCEL_G.value,
                BodyKind::Missile => if self.interceptor.is_some() {INTERCEPTOR_ACCEL_G.value} else {self.missile.map_or(MISSILE_MAX_ACCEL_G.value,|m|m.payload.acceleration_g())},
            }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderError {
    PowerOrHeat,
    Destroyed,
    NoTrack,
    InvalidTarget,
    EmptyMagazine,
    LauncherRecharging,
    BeamRecharging,
    Unarmed,
}

/// How a body came to an end. Truth only.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LossCause {
    /// Hit the surface of a celestial body (index into `System::bodies`).
    Impact(usize),
    /// Killed by a missile's payload.
    Missile { payload: Payload, missile: BodyId },
    ShipBeam { shooter: BodyId },
    PointDefence { shooter: BodyId },
    Interceptor { missile:BodyId },
    /// A missile that fired, detonated or missed and is spent.
    Expended,
}

/// A payload landing on a body. Truth only.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub body: BodyId,
    pub t: f64,
    pub payload: Payload,
    /// Source body: a missile, or the firing ship for a direct beam.
    pub missile: BodyId,
    /// Energy delivered, and how much of it the screen stored, J.
    pub energy_j: f64,
    pub screened_j: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Loss {
    pub body: BodyId,
    pub t: f64,
    pub cause: LossCause,
}

/// A scenario goal known to every side: `protect` must reach the region.
#[derive(Clone, Debug)]
pub struct Objective {
    pub name: String,
    /// Fixed in the system frame, km.
    pub center: Vec2,
    pub radius: f64,
    pub protect: BodyId,
    /// The side trying to get `protect` there.
    pub defender: FactionId,
    /// The side trying to stop it.
    pub attacker: FactionId,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub winner: FactionId,
    pub t: f64,
    pub reason: String,
}

/// Something a faction has noticed that deserves the player's attention. Raised only
/// from that faction's received information.
#[derive(Clone, Debug, PartialEq)]
pub enum AlertKind {
    ContactLost(ContactId),
    NewContact(ContactId),
    ShipLost(BodyId),
    CollisionWarning(BodyId),
    CollisionUnavoidable(BodyId),
    OrderComplete(BodyId),
    /// One of our ships reported a hit and survived.
    Hit(BodyId),
    LaunchCancelled(BodyId),
    GameOver,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Alert {
    pub t: f64,
    /// `None`: for everyone.
    pub faction: Option<FactionId>,
    pub kind: AlertKind,
}

/// Initial conditions for a body at `t = 0`.
pub struct BodySpec {
    pub name: String,
    pub kind: BodyKind,
    pub faction: FactionId,
    pub state: State,
    /// Thrust held through the prehistory and onward, km/s².
    pub thrust: Vec2,
    /// Initial magazine depth for each missile payload.
    pub magazine: u32,
}

#[derive(Clone, Copy, Debug)]
enum Event {
    InterceptorGuide(BodyId),
    PointDefence(BodyId),
    PointDefencePulse(point_defence::Pulse),
    TacticalFrame,
    BeamControl(BodyId, u64),
    Step(BodyId),
    SensorFrame,
    MissileGuide(BodyId),
    QueuedLaunch(BodyId, ContactId, Payload, u64),
    /// A laser missile's beam, resolved when its light reaches the target.
    Beam(Beam),
}

#[derive(Clone, Copy, Debug)]
struct Beam {
    missile: BodyId,
    target: BodyId,
    front: Front,
    direction: Vec2,
    /// None for the pre-existing missile laser model.
    ship_energy_j: f64,
}

#[derive(Clone, Debug)]
struct Ping {
    search_heading:Option<Vec2>,
    power_w: f64,
    emitter: BodyId,
    /// Signature at emission; later thrust/sensor changes cannot recall this light.
    signature_w: f64,
    front: Front,
    pending: Vec<BodyId>,
}

#[derive(Clone, Debug)]
struct Echo {
    target:BodyId,
    power_w: f64,
    emitter: BodyId,
    front: Front,
    out_km: f64,
}

#[derive(Clone, Debug)]
struct Relay {
    faction: FactionId,
    front: Front,
    obs: Observation,
}

pub struct World {
    profile:Option<SimulationProfile>,
    pub probes_enabled:bool,
    interceptor_solutions:BTreeMap<(BodyId,BodyId),sensors::SeekerFix>,
    probability_flights:BTreeMap<BodyId,weapon_probability::Flight>,
    refinement: refinements::Refinements,
    time: f64,
    pub system: System,
    pub bodies: Vec<Body>,
    pub losses: Vec<Loss>,
    pub hits: Vec<Hit>,
    pub objective: Option<Objective>,
    pub outcome: Option<Outcome>,
    pub alerts: Vec<Alert>,
    scheduler: Scheduler<Event>,
    last_step: Vec<f64>,
    last_frame: f64,
    pings: Vec<Ping>,
    /// Emitted pulses for the owning faction's round-trip range display.
    pub ping_emissions: Vec<(BodyId, Front)>,
    pub(crate) hidden_ping_circles: std::collections::BTreeSet<(BodyId,u64)>,
    echoes: Vec<Echo>,
    relays: Vec<Relay>,
    perceptions: BTreeMap<FactionId, Perception>,
    /// Established gameplay assumption: reports uniquely associate with a source.
    /// Position and velocity remain uncertain; association does not expose truth.
    association: BTreeMap<(FactionId, BodyId), ContactId>,
    reverse_association:BTreeMap<(FactionId,ContactId),BodyId>,
    rng: Rng,
}


/// Pings older than this radius are discarded.
const MAX_FRONT_RADIUS_KM: f64 = 100.0 * AU;

impl World {
    /// Build a world at `t = 0` whose bodies have `history_s` of prior ballistic motion,
    /// so light they emitted before the scenario starts is already in flight.
    pub fn new(system: System, specs: Vec<BodySpec>, history_s: f64, seed: u64) -> Self {
        let bodies: Vec<Body> = specs
            .into_iter()
            .map(|spec| {
                let trajectory = ballistic_history(&system, spec.state, spec.thrust, history_s);
                Body {
                    controls:controls::Controls::default(),last_missile_launch:None,
                    ship_class: (spec.kind==BodyKind::Ship).then_some(ShipClass::Frigate),
                    interceptor_battery:None,interceptor:None,
                    point_defence: None,
                    has_screen: spec.kind==BodyKind::Ship,
                    baseline_emission_factor: if spec.kind==BodyKind::Station {STATION_EMISSION_FACTOR.value} else {1.0},
                    visibility_multiplier:1.0,
                    sensors: match spec.kind {
                        BodyKind::Missile=>sensors::SensorSuite::MISSILE,
                        BodyKind::Station=>sensors::SensorSuite {passive:false,active:false,direction_finding:true},
                        _=>sensors::SensorSuite::FULL,
                    },
                    probes: if spec.kind==BodyKind::Ship && spec.magazine>0 { PROBE_INVENTORY.value as u32 } else { 0 },
                    probe_burn_until: None,
                    probe_ping_at: 0.0,
                    damage:crate::damage::Damage::default(),
                    thermal: crate::thermal::Thermal { capacitor_j: if spec.kind==BodyKind::Station {0.0} else {BEAM_CAPACITOR_J.value}, ..Default::default() },
                    launch_generation: 0,
                    name: spec.name,
                    kind: spec.kind,
                    faction: spec.faction,
                    trajectory,
                    commanded: spec.thrust,
                    autopilot: None,
                    avoidance: Avoidance { thrust: spec.thrust, active: false, impossible: false },
                    drive_limit: f64::INFINITY,
                    armed: spec.magazine > 0,
                    controllable: spec.kind == BodyKind::Ship,
                    beam_ready_at: 0.0,
                    beam_target: None,
                    beam_auto: false,
                    beam_order: 0,
                    beam_emitted_j: 0.0,
                    last_beam: None,
                    magazine: [spec.magazine; 2],
                    missile_ready_at: [0.0; 2],
                    missile_queued: [0; 2],
                    missile_queue_ready_at: [0.0; 2],
                    missile: None,
                    screen_up: false,
                    screen_j: 0.0,
                    hull_j: 0.0,
                }
            })
            .collect();
        let mut scheduler = Scheduler::default();
        for i in 0..bodies.len() {
            scheduler.schedule(0.0, Event::Step(BodyId(i as u32)));
        }
        scheduler.schedule(0.0, Event::SensorFrame);
        scheduler.schedule(0.0, Event::TacticalFrame);
        let perceptions = bodies.iter().map(|b| (b.faction, Perception::new(b.faction))).collect();
        let mut world = Self {
            profile:std::env::var_os("LUMINAL_PROFILE").map(|_|SimulationProfile::default()),
            probes_enabled:true,
            interceptor_solutions:BTreeMap::new(),
            probability_flights:BTreeMap::new(),
            refinement: refinements::Refinements::with_seed(seed),
            time: 0.0,
            system,
            last_step: vec![0.0; bodies.len()],
            bodies,
            losses: vec![],
            hits: vec![],
            objective: None,
            outcome: None,
            alerts: vec![],
            scheduler,
            last_frame: -SENSOR_FRAME_S.value,
            pings: vec![],
            ping_emissions: vec![],
            hidden_ping_circles: Default::default(),
            echoes: vec![],
            relays: vec![],
            perceptions,
            association: BTreeMap::new(),
            reverse_association:BTreeMap::new(),
            rng: Rng::stream(seed, 1),
        };
        world.advance_to(0.0);
        world
    }

    pub fn time(&self) -> f64 {
        self.time
    }

    pub fn body(&self, id: BodyId) -> Option<&Body> {
        self.bodies.get(id.0 as usize)
    }

    pub fn perception(&self, f: FactionId) -> Option<&Perception> {
        self.perceptions.get(&f)
    }

    /// Which body a faction's contact really is. Spectator use only.
    pub fn contact_truth(&self, f: FactionId) -> BTreeMap<ContactId, BodyId> {
        self.association.iter().filter(|((ff, _), _)| *ff == f).map(|((_, b), c)| (*c, *b)).collect()
    }
    pub(crate) fn body_for_contact(&self,f:FactionId,c:ContactId)->Option<BodyId> {
        self.reverse_association.get(&(f,c)).copied()
    }

    /// One assigned command ship; destruction never transfers player command.
    pub fn decider(&self, f: FactionId, t: f64) -> Option<BodyId> {
        self.bodies
            .iter()
            .enumerate()
            .filter(|(_, b)| b.faction == f && b.kind == BodyKind::Ship && b.controllable)
            .min_by_key(|(i,b)| (!b.controllable, !b.armed, *i))
            .filter(|(_,b)| b.alive_at(t))
            .map(|(i, _)| BodyId(i as u32))
    }

    /// Manual thrust from now on. Cancels any autopilot order; collision avoidance
    /// still applies.
    pub fn set_thrust(&mut self, id: BodyId, thrust: Vec2) -> Result<(), OrderError> {
        let b = self.live_body_mut(id)?;
        b.commanded = thrust;
        b.autopilot = None;
        self.guide(id);
        Ok(())
    }

    /// Settle into a convenient circular orbit about a celestial body.
    pub fn set_orbit(&mut self, id: BodyId, celestial: usize) -> Result<(), OrderError> {
        let t = self.time;
        if celestial >= self.system.bodies.len() {
            return Err(OrderError::InvalidTarget);
        }
        let c = self.system.state(celestial, t);
        let s = self.live_body_mut(id)?.trajectory.state_at(t).unwrap();
        let rel = s.pos - c.pos;
        let radius = autopilot::convenient_orbit_radius(&self.system, celestial, rel.length());
        let sense = autopilot::orbit_sense(rel, s.vel - c.vel);
        let order = Order::Orbit { celestial, radius, sense };
        self.live_body_mut(id)?.autopilot = Some(Autopilot { order, status: AutopilotStatus::Manoeuvring });
        self.guide(id);
        Ok(())
    }

    /// Close on a target and hold station near it.
    pub fn set_intercept(&mut self, id: BodyId, target: InterceptTarget) -> Result<(), OrderError> {
        self.set_ship_approach(id, target, true)
    }

    pub fn set_flyby(&mut self, id: BodyId, target: InterceptTarget) -> Result<(), OrderError> {
        self.set_ship_approach(id, target, false)
    }

    pub fn set_tactical_range(&mut self,id:BodyId,target:InterceptTarget,range:Option<f64>)->Result<(),OrderError> {
        let faction=self.live_body_mut(id)?.faction;
        if range.is_some_and(|r|!r.is_finite() || r<0.0) {return Err(OrderError::InvalidTarget);}
        let valid=match target {
            InterceptTarget::Contact(c)=>self.perceptions.get(&faction).is_some_and(|p|p.contacts.contains_key(&c)),
            InterceptTarget::Own(other)=>other!=id && self.body(other).is_some_and(|b|b.faction==faction && b.alive_at(self.time)),
        };
        if !valid {return Err(OrderError::InvalidTarget);}
        let order=range.map_or(Order::Evade(target),|r|Order::KeepRange(target,r));
        self.live_body_mut(id)?.autopilot=Some(Autopilot {order,status:AutopilotStatus::Manoeuvring});
        self.guide(id);Ok(())
    }

    fn set_ship_approach(&mut self, id: BodyId, target: InterceptTarget, match_velocity: bool) -> Result<(), OrderError> {
        let t = self.time;
        let faction = self.live_body_mut(id)?.faction;
        match target {
            InterceptTarget::Own(o) => {
                let ok = o != id && self.body(o).is_some_and(|b| b.faction == faction && b.alive_at(t));
                if !ok {
                    return Err(OrderError::InvalidTarget);
                }
            }
            InterceptTarget::Contact(c) => {
                let has_track = self.received_picture(id).and_then(|p| p.contacts.get(&c)).is_some_and(|c|
                    c.usable_track(t).is_some() || (match_velocity && c.detection(t)>=sensors::DetectionLevel::Bearing
                        && t-c.last.decider_received_at<=TRACK_STALE_S.value));
                if !has_track {
                    return Err(OrderError::NoTrack);
                }
            }
        }
        let order = if match_velocity { Order::Intercept(target) } else { Order::Flyby(target) };
        self.live_body_mut(id)?.autopilot = Some(Autopilot { order, status: AutopilotStatus::Manoeuvring });
        self.guide(id);
        Ok(())
    }

    /// Fly to `point` in minimal time and stop there, in the frame of the celestial body
    /// whose sphere of influence contains it.
    pub fn set_move(&mut self, id: BodyId, point: Vec2) -> Result<(), OrderError> {
        let t = self.time;
        self.live_body_mut(id)?;
        if self.system.inside(point, t, 1.05).is_some() {
            return Err(OrderError::InvalidTarget);
        }
        let frame = self.system.frame_for(point, t);
        let offset = point - self.system.state(frame, t).pos;
        self.live_body_mut(id)?.autopilot = Some(Autopilot { order: Order::MoveTo { frame, offset }, status: AutopilotStatus::Manoeuvring });
        self.guide(id);
        Ok(())
    }

    /// Come to rest relative to the local frame as quickly as the drive allows.
    pub fn set_all_stop(&mut self, id: BodyId) -> Result<(), OrderError> {
        let t = self.time;
        let b = self.live_body_mut(id)?;
        let s = b.trajectory.state_at(t).unwrap();
        let limit = b.max_accel().min(b.drive_limit);
        let frame = self.system.frame_for(s.pos, t);
        let v_rel = s.vel - self.system.state(frame, t).vel;
        let stop_distance = v_rel.dot(v_rel) / (2.0 * 0.8 * limit);
        let point = s.pos + v_rel.normalized() * stop_distance;
        let offset = point - self.system.state(frame, t).pos;
        self.live_body_mut(id)?.autopilot = Some(Autopilot { order: Order::MoveTo { frame, offset }, status: AutopilotStatus::Manoeuvring });
        self.guide(id);
        Ok(())
    }

    /// Cap autopilot thrust at `accel` km/s².
    pub fn set_drive_limit(&mut self, id: BodyId, accel: f64) -> Result<(), OrderError> {
        self.live_body_mut(id)?.drive_limit = accel.max(0.0);
        self.guide(id);
        Ok(())
    }

    /// Launch a missile from `id` at one of its faction's tracked contacts.
    pub fn queue_launch(&mut self, id: BodyId, target: ContactId, payload: Payload) -> Result<(), OrderError> {
        if payload==Payload::Beam {return Err(OrderError::InvalidTarget);}
        let t = self.time;
        let b = self.live_body_mut(id)?;
        if b.kind != BodyKind::Ship { return Err(OrderError::InvalidTarget); }
        let effectiveness=b.operating_effectiveness(crate::damage::System::Launcher);
        if effectiveness==0.0 {return Err(OrderError::PowerOrHeat);}
        if b.magazine[payload.index()] <= b.missile_queued[payload.index()] {
            return Err(OrderError::EmptyMagazine);
        }
        let when = t.max(b.missile_ready_at[payload.index()]).max(b.missile_queue_ready_at[payload.index()]);
        if !self.received_picture(id).is_some_and(|p|p.contacts.contains_key(&target)) {
            return Err(OrderError::NoTrack);
        }
        if when <= t {
            self.launch(id, target, payload)?;
        } else {
            self.bodies[id.0 as usize].missile_queued[payload.index()] += 1;
            self.scheduler.schedule(when, Event::QueuedLaunch(id, target, payload, self.bodies[id.0 as usize].launch_generation));
        }
        self.bodies[id.0 as usize].missile_queue_ready_at[payload.index()] = when + payload.launch_interval()/effectiveness;
        Ok(())
    }

    /// Execute a launch now; queued launches use this same rate and inventory gate.
    pub fn launch(&mut self, id: BodyId, target: ContactId, payload: Payload) -> Result<BodyId, OrderError> {
        if payload==Payload::Beam {return Err(OrderError::InvalidTarget);}
        let t = self.time;
        let b = self.live_body_mut(id)?;
        let effectiveness=b.operating_effectiveness(crate::damage::System::Launcher);
        if effectiveness==0.0 {return Err(OrderError::PowerOrHeat);}
        if b.kind != BodyKind::Ship {
            return Err(OrderError::InvalidTarget);
        }
        if b.magazine[payload.index()] == 0 {
            return Err(OrderError::EmptyMagazine);
        }
        if t < b.missile_ready_at[payload.index()] {
            return Err(OrderError::LauncherRecharging);
        }
        let faction = b.faction;
        let contact=self.received_picture(id).and_then(|p|p.contacts.get(&target)).ok_or(OrderError::NoTrack)?;
        let bearing=match contact.last.measurement {
            Measurement::Bearing {bearing,..}|Measurement::BearingRange {bearing,..}=>bearing,
        };
        let search_heading=Vec2::new(bearing.cos(),bearing.sin());
        let target_body = self.body_for_contact(faction,target).ok_or(OrderError::InvalidTarget)?;

        let fired = self.bodies.iter().filter(|m| m.missile.is_some_and(|ms| ms.launcher == id)).count() + 1;
        let b = &mut self.bodies[id.0 as usize];
        b.magazine[payload.index()] -= 1;
        b.last_missile_launch=Some(t);
        b.missile_ready_at[payload.index()] = t + payload.launch_interval()/effectiveness;
        let name = format!("{} M{fired}", b.name);
        let start = b.trajectory.state_at(t).expect("alive");
        let dv = payload.delta_v();
        let mid = BodyId(self.bodies.len() as u32);
        self.bodies.push(Body {
            controls:controls::Controls::default(),last_missile_launch:None,
            ship_class: None,
            damage:crate::damage::Damage::default(),
            interceptor_battery:None,interceptor:None,
            point_defence: None,
            has_screen: false,
            baseline_emission_factor: 1.0,
            visibility_multiplier:1.0,
            sensors: sensors::SensorSuite::MISSILE,
            probes: 0,
            probe_burn_until: None,
            probe_ping_at: 0.0,
            thermal: crate::thermal::Thermal { last_t: t, ..Default::default() },
            launch_generation: 0,
            name,
            kind: BodyKind::Missile,
            faction,
            trajectory: Trajectory::new(t, start),
            commanded: Vec2::ZERO,
            autopilot: None,
            avoidance: Avoidance { thrust: Vec2::ZERO, active: false, impossible: false },
            drive_limit: f64::INFINITY,
            magazine: [0; 2],
            missile_ready_at: [0.0; 2],
            missile_queued: [0; 2],
            missile_queue_ready_at: [0.0; 2],
            armed: true,
            controllable: false,
            beam_ready_at: 0.0,
            beam_target: None,
            beam_auto: false,
            beam_order: 0,
            beam_emitted_j: 0.0,
            last_beam: None,
            screen_up: false,
            screen_j: 0.0,
            hull_j: 0.0,
            missile: Some(MissileState {
                payload,
                target,
                launcher: id,
                phase: Phase::Burn,
                dv_left: dv,
                burn_left: dv * MISSILE_BURN_FRACTION.value,
                search_heading,
                active_seeker: false,
                seeker: None,
                local_fix: None,
                last_guide: t,
                target_body,
                launched_at: start.pos,
            }),
        });
        self.last_step.push(t);
        self.report_launch(mid);
        self.start_probability_missile(mid);
        self.scheduler.schedule(t, Event::MissileGuide(mid));
        Ok(mid)
    }

    /// One missile guidance cycle: account fuel, check for a hit since the last cycle,
    /// sense, pick the phase, fire or steer, and schedule the next cycle.
    fn guide_missile(&mut self,id:BodyId) {
        self.guide_probability_weapon(id);
    }

    /// Aim only from the faction's causally received track. Truth is retained solely
    /// for resolving the intersection after the emitted pulse travels at c.
    pub fn arm_beams(&mut self, id: BodyId) -> Result<(), OrderError> {
        let b = self.live_body_mut(id)?;
        if b.kind != BodyKind::Ship || !b.armed { return Err(OrderError::Unarmed); }
        b.beam_auto = true;
        b.beam_target = None;
        b.beam_order += 1;
        let order = b.beam_order;
        self.control_beam(id, order);
        Ok(())
    }

    pub fn engage_beam(&mut self, id: BodyId, target: Option<ContactId>) -> Result<(), OrderError> {
        let b = self.live_body_mut(id)?;
        if b.kind != BodyKind::Ship || !b.armed { return Err(OrderError::Unarmed); }
        let faction = b.faction;
        if let Some(target) = target
            && !self.perceptions.get(&faction).and_then(|p| p.contacts.get(&target)).is_some_and(|c| c.track.is_some())
        { return Err(OrderError::NoTrack); }
        let b = &mut self.bodies[id.0 as usize];
        b.beam_target = target;
        b.beam_auto = false;
        b.beam_order += 1;
        let order = b.beam_order;
        self.control_beam(id, order);
        Ok(())
    }

    /// Expected coupled energy using only the received track and its transverse
    /// uncertainty at pulse arrival, including pointing jitter. This is a firing
    /// policy, not knowledge of the target's actual future manoeuvres.
    fn beam_worth_firing(&self,id:BodyId,target:ContactId)->bool {
        let Some(origin)=self.state(id,self.time).map(|s|s.pos) else {return false};
        let Some(track)=self.received_picture(id).and_then(|p|p.contacts.get(&target)).and_then(|c|c.estimate(self.time,&self.system)) else {return false};
        let range=(track.at(self.time,&self.system).pos()-origin).length();
        if range<=SHIP_BEAM_AUTO_RANGE_LS.value*crate::units::LIGHT_SECOND {return true;}
        let predicted=track.at(self.time+range/crate::units::C,&self.system);
        let direction=(predicted.pos()-origin).normalized();
        let transverse=Vec2::new(-direction.y,direction.x);
        let p=predicted.pos_cov();
        let variance=(transverse.x*transverse.x*p[0][0]+2.0*transverse.x*transverse.y*p[0][1]
            +transverse.y*transverse.y*p[1][1]).max(0.0)+(range*SHIP_BEAM_POINTING_RAD.value).powi(2);
        let spot=(range*SHIP_BEAM_DIVERGENCE.value).max(SHIP_RADIUS_KM.value);
        let expected=SHIP_BEAM_ENERGY_J.value*(SHIP_RADIUS_KM.value/spot).powi(2)
            / (1.0+2.0*variance/(spot*spot)).sqrt();
        expected>=SHIP_BEAM_MIN_EXPECTED_J.value
    }

    fn control_beam(&mut self, id: BodyId, order: u64) {
        let b = &self.bodies[id.0 as usize];
        if !b.alive_at(self.time) || b.beam_order != order { return; }
        let automatic = b.beam_auto;
        let faction = b.faction;
        let origin = b.trajectory.state_at(self.time).unwrap().pos;
        let target = if automatic {
            self.received_picture(id).into_iter().flat_map(|p| p.contacts.values())
                .filter(|c| !self.contact_retired(faction, c.id) && self.track_fresh(id, c.id))
                .filter(|c| self.body_for_contact(faction,c.id).is_none_or(|target|
                    self.bodies[target.0 as usize].kind!=BodyKind::Missile))
                .filter_map(|c| c.track.as_ref().map(|tr| (c.id, (tr.at(self.time, &self.system).pos() - origin).length())))
                .filter(|(target, _)| self.beam_worth_firing(id,*target))
                .min_by(|a, b| a.1.total_cmp(&b.1)).map(|(id, _)| id)
        } else { b.beam_target };
        if automatic { self.bodies[id.0 as usize].beam_target = target; }
        let Some(target) = target else {
            if automatic { self.scheduler.schedule(self.time + 1.0, Event::BeamControl(id, order)); }
            return;
        };
        if self.track_fresh(id,target) && (!automatic || self.beam_worth_firing(id,target)) {
            let _ = self.fire_beam(id, target);
        }
        let next = (self.time + 1.0).max(self.bodies[id.0 as usize].beam_ready_at);
        self.scheduler.schedule(next, Event::BeamControl(id, order));
    }

    pub fn fire_beam(&mut self, id: BodyId, target: ContactId) -> Result<(), OrderError> {
        let t = self.time;
        let b = self.live_body_mut(id)?;
        if b.kind != BodyKind::Ship || !b.armed { return Err(OrderError::Unarmed); }
        let effectiveness=b.operating_effectiveness(crate::damage::System::Beam);
        if effectiveness==0.0 {return Err(OrderError::PowerOrHeat);}
        if t < b.beam_ready_at { return Err(OrderError::BeamRecharging); }
        b.advance_thermal(t);
        if !b.thermal.can_fire() { return Err(OrderError::PowerOrHeat); }
        let faction = b.faction;
        let origin = b.trajectory.state_at(t).unwrap().pos;
        let track = self.received_picture(id).and_then(|p| p.contacts.get(&target))
            .and_then(|c| c.estimate(t,&self.system)).ok_or(OrderError::NoTrack)?;
        let mut flight = (track.at(t, &self.system).pos() - origin).length() / crate::units::C;
        for _ in 0..12 {
            flight = (track.at(t + flight, &self.system).pos() - origin).length() / crate::units::C;
        }
        let aim = track.at(t + flight, &self.system).pos() - origin;
        if !flight.is_finite() || aim.length() == 0.0 { return Err(OrderError::InvalidTarget); }
        let target_body = self.body_for_contact(faction,target).ok_or(OrderError::InvalidTarget)?;
        let angle = bearing_of(aim) + SHIP_BEAM_POINTING_RAD.value * self.rng.gaussian();
        let direction = Vec2::new(angle.cos(), angle.sin());
        let beam = Beam { missile: id, target: target_body, front: Front { origin, t_emit: t }, direction,
            ship_energy_j: SHIP_BEAM_ENERGY_J.value };
        self.bodies[id.0 as usize].beam_ready_at = t + SHIP_BEAM_RECHARGE_S.value/effectiveness;
        self.bodies[id.0 as usize].thermal.fire();
        self.bodies[id.0 as usize].beam_emitted_j += SHIP_BEAM_ENERGY_J.value;
        self.bodies[id.0 as usize].last_beam = Some((t, origin, origin + direction * aim.length()));
        self.record_combat(t, origin, CombatKind::BeamPulse, Some(id), Some(faction));
        self.scheduler.schedule(t + flight.max(0.001), Event::Beam(beam));
        Ok(())
    }

    /// A laser beam's light has had time to reach its target: did the ray pass close
    /// enough to where the target really was when the light got there?
    fn resolve_beam(&mut self, beam: Beam) {
        let traj = self.bodies[beam.target.0 as usize].trajectory.clone();
        if std::env::var("LUMINAL_DEBUG").is_ok() { eprintln!("  beam resolve at {:.2} emitted {:.2} target end {:?}", self.time, beam.front.t_emit, traj.end()); }
        let Some(t_arr) = beam.front.arrival(&traj, beam.front.t_emit, self.time) else {
            if traj.end().is_none() {
                // The target can move after the shot. Keep the emitted pulse alive
                // until its light reaches the target; never steer it after emission.
                let range = (traj.state_at(self.time).unwrap().pos - beam.front.origin).length();
                let wait = ((range - beam.front.radius_at(self.time)) / crate::units::C).max(0.001);
                self.scheduler.schedule(self.time + wait, Event::Beam(beam));
            }
            return;
        };
        let Some(s) = traj.state_at(t_arr) else { return };
        let rel = s.pos - beam.front.origin;
        let along = rel.dot(beam.direction);
        let miss = (rel - beam.direction * along).length();
        let blocked = self.system.occluder(beam.front.origin, beam.front.t_emit, s.pos, t_arr).is_some();
        self.debug_note("BEAM_RESULT",format!("shooter={:?} target={:?} arrival={t_arr:.6} miss_km={miss} along_km={along} blocked={blocked}",beam.missile,beam.target));
        {
            let energy=beam.ship_energy_j;
            if along > 0.0 && !blocked {
                let spot = (along * SHIP_BEAM_DIVERGENCE.value).max(SHIP_RADIUS_KM.value);
                // Gaussian fluence sampled over the target's projected area. The
                // intercepted fraction is bounded, so widening a beam cannot create energy.
                let fraction = (SHIP_RADIUS_KM.value / spot).powi(2) * (-miss * miss / (spot * spot)).exp();
                let coupled = energy * fraction;
                if coupled > 0.0 {
                    self.deliver(beam.target, t_arr, coupled, Payload::Beam, beam.missile);
                }
            }
        }
    }

    fn live_body_mut(&mut self, id: BodyId) -> Result<&mut Body, OrderError> {
        let t = self.time;
        match self.bodies.get_mut(id.0 as usize) {
            Some(b) if b.alive_at(t) => Ok(b),
            Some(_) => Err(OrderError::Destroyed),
            None => Err(OrderError::InvalidTarget),
        }
    }

    /// Recompute a body's thrust from its orders and the collision check, using only
    /// what its faction knows, and commit it if it changed.
    fn guide(&mut self, id: BodyId) {
        let t = self.time;
        let i = id.0 as usize;
        let b = &self.bodies[i];
        if b.missile.is_some() || b.interceptor.is_some() || matches!(b.kind,BodyKind::Probe|BodyKind::Station) {
            return;
        }
        let Some(s) = b.trajectory.state_at(t) else { return };
        let max_accel = b.max_accel();
        let limit = max_accel.min(b.drive_limit);
        // No drive authority means ballistic drift, including collision avoidance.
        // Keep the standing order so guidance can resume after repairs.
        if limit<=0.0 {
            let b=&mut self.bodies[i];
            b.avoidance=Avoidance {thrust:Vec2::ZERO,active:false,impossible:false};
            if b.trajectory.last().thrust!=Vec2::ZERO {b.trajectory.set_thrust(t,Vec2::ZERO).unwrap();}
            return;
        }
        let (desired, status) = match b.autopilot.map(|a| a.order) {
            None => {
                let nominal=max_accel/if b.controls.boost_active {1.2} else {1.0};
                let commanded=if b.controls.boost_active && b.commanded.length()>=nominal*(1.0-1e-9) {b.commanded*1.2} else {b.commanded};
                (if commanded.length()>limit {commanded.normalized()*limit} else {commanded},None)
            },
            Some(Order::Orbit { celestial, radius, sense }) => {
                let thrust = autopilot::orbit_thrust(&self.system, celestial, s, t, radius, sense, limit);
                let c = self.system.state(celestial, t);
                let rel = s.pos - c.pos;
                let settled = ((rel.length() - radius) / radius).abs() < 0.02 && thrust.length() < 0.01 * crate::units::G0;
                (thrust, Some(if settled { AutopilotStatus::Holding } else { AutopilotStatus::Manoeuvring }))
            }
            Some(Order::Intercept(target) | Order::Flyby(target) | Order::KeepRange(target,_) | Order::Evade(target)) => {
                let flyby = matches!(b.autopilot.map(|a| a.order), Some(Order::Flyby(_)));
                let evade=matches!(b.autopilot.map(|a|a.order),Some(Order::Evade(_)));
                let keep=match b.autopilot.map(|a|a.order) {Some(Order::KeepRange(_,r))=>Some(r),_=>None};
                let known = match target {
                    InterceptTarget::Own(o) => self.known_body(b.faction,o).and_then(|known|
                        known.trajectory.state_at(t).map(|ts| (ts,known.trajectory.thrust_at(t).unwrap_or(Vec2::ZERO)))),
                    InterceptTarget::Contact(c) => self
                        .received_picture(id)
                        .and_then(|p| p.contacts.get(&c))
                        .and_then(|c| c.estimate(t,&self.system))
                        .map(|tr| {
                            let now = tr.at(t, &self.system);
                            (State { pos: now.pos(), vel: now.vel() }, now.accel())
                        }),
                };
                match known {
                    None => {
                        let bearing=if let InterceptTarget::Contact(c)=target {
                            self.received_picture(id).and_then(|p|p.contacts.get(&c))
                                .filter(|c|t-c.last.decider_received_at<=TRACK_STALE_S.value)
                                .map(|c|match c.last.measurement {Measurement::Bearing {bearing,..}|Measurement::BearingRange {bearing,..}=>bearing})
                        } else {None};
                        if !flyby && let Some(bearing)=bearing {
                            let direction=Vec2::new(bearing.cos(),bearing.sin());
                            (direction*if evade {-limit} else {limit},Some(AutopilotStatus::Manoeuvring))
                        }
                        else {(Vec2::ZERO,Some(AutopilotStatus::NoTrack))}
                    },
                    Some((ts, ta)) => {
                        let range = (ts.pos - s.pos).length();
                        let passed = b.autopilot.is_some_and(|ap| match ap.status {
                            AutopilotStatus::Passed => true,
                            AutopilotStatus::Closing { range: previous, .. } =>
                                range > previous && previous < (ts.vel - s.vel).length() * SENSOR_FRAME_S.value * 2.0,
                            _ => false,
                        });
                        if evade {((s.pos-ts.pos).normalized()*limit,Some(AutopilotStatus::Manoeuvring))}
                        else if flyby && passed {
                            (Vec2::ZERO, Some(AutopilotStatus::Passed))
                        } else {
                            let r = if flyby { autopilot::flyby(s, ts, ta, limit) } else if let Some(range)=keep {autopilot::keep_range(s,ts,ta-self.system.gravity(s.pos,t),range,limit)} else { autopilot::rendezvous(s, ts, ta-self.system.gravity(s.pos,t),limit) };
                            let holding = r.gap.abs() < keep.map_or(0.5*autopilot::STANDOFF_KM,|r|(r*0.01).max(500.0)) && r.rel_speed < 1.0;
                            let status = if holding && !flyby { AutopilotStatus::Holding } else { AutopilotStatus::Closing { eta: r.eta, range: r.gap } };
                            (r.thrust, Some(status))
                        }
                    }
                }
            }
            Some(Order::MoveTo { frame, offset }) => {
                let f = self.system.state(frame, t);
                let dest = State { pos: f.pos + offset, vel: f.vel };
                // Keep pace with the frame and cancel local gravity.
                let ff = self.system.accel(frame, t) - self.system.gravity(s.pos, t);
                let m = autopilot::move_to(s, dest, ff, limit);
                // A location order finishes once, rather than station-keeping
                // forever and chasing tiny residual errors in alternating directions.
                let holding = b.autopilot.is_some_and(|a| a.status == AutopilotStatus::Holding)
                    || (m.gap < 1.0 && m.rel_speed < 0.01);
                let status = if holding { AutopilotStatus::Holding } else { AutopilotStatus::Closing { eta: m.eta, range: m.gap } };
                (if holding {Vec2::ZERO} else {m.thrust}, Some(status))
            }
        };
        // The orbit law steers to a safe radius by construction; everything else is
        // checked against every surface.
        let orbiting = matches!(b.autopilot.map(|a| a.order), Some(Order::Orbit { .. }));
        let arrived = matches!(b.autopilot.map(|a| a.order), Some(Order::MoveTo { .. }))
            && status == Some(AutopilotStatus::Holding);
        let avoidance = if orbiting || arrived {
            Avoidance { thrust: desired, active: false, impossible: false }
        } else {
            autopilot::avoid(&self.system, s, t, desired, max_accel)
        };
        let b = &mut self.bodies[i];
        if arrived {b.commanded = Vec2::ZERO;}
        let faction = b.faction;
        let mut raised = vec![];
        if let (Some(a), Some(st)) = (&mut b.autopilot, status) {
            if matches!(st, AutopilotStatus::Holding | AutopilotStatus::Passed) && a.status != st {
                raised.push(AlertKind::OrderComplete(id));
            }
            a.status = st;
        }
        if avoidance.active && !b.avoidance.active {
            raised.push(if avoidance.impossible { AlertKind::CollisionUnavoidable(id) } else { AlertKind::CollisionWarning(id) });
        } else if avoidance.impossible && !b.avoidance.impossible {
            raised.push(AlertKind::CollisionUnavoidable(id));
        }
        b.avoidance = avoidance;
        if b.trajectory.last().thrust != avoidance.thrust {
            b.trajectory.set_thrust(t, avoidance.thrust).expect("orders apply at current time");
        }
        for kind in raised {
            self.alert(Some(faction), kind);
        }
    }

    pub fn ping(&mut self, id: BodyId) -> bool {
        let t = self.time;
        let Some(b) = self.bodies.get(id.0 as usize).filter(|b| b.alive_at(t)) else { return false };
        let active=b.operating_effectiveness(crate::damage::System::Active);
        if !b.sensors.active || active==0.0 { return false; }
        let Some(s) = b.trajectory.state_at(t) else { return false };
        let signature_w = (emission_w(b.kind,b.baseline_emission_factor,b.trajectory.thrust_at(t).unwrap_or(Vec2::ZERO))+b.thermal.emission(b.screen_j))
            * ACTIVE_EXPOSURE_RANGE.value.powi(2);
        let front = Front { origin: s.pos, t_emit: t };
        let pending = (0..self.bodies.len()).filter(|&j| self.bodies[j].faction!=b.faction && self.bodies[j].alive_at(t)).map(|j| BodyId(j as u32)).collect();
        let power_w=ACTIVE_PING_POWER_W.value * active * if matches!(b.kind,BodyKind::Probe|BodyKind::Missile) { PROBE_SENSOR_FACTOR.value } else { 1.0 };
        self.pings.push(Ping { emitter: id, signature_w, front, pending, power_w,search_heading:b.missile.map(|m|m.search_heading) });
        self.ping_emissions.push((id, front));
        self.debug_note("PING",format!("emitter={id:?} origin={:?} power_w={power_w}",front.origin));
        true
    }

    /// Process every event up to `t`, then set the clock to `t`.
    pub fn advance_to(&mut self, t: f64) {
        self.advance_until_alert(t, None);
    }

    /// As `advance_to`, but stop at the first event that raises an alert for `watch`
    /// (or for everyone). Returns that alert; the clock then stands at its time.
    pub fn advance_until_alert(&mut self, t: f64, watch: Option<FactionId>) -> Option<Alert> {
        self.advance_until_alert_budgeted(t,watch,None)
    }
    /// GUI work budget: yield between events without skipping simulation time or
    /// removing queued events. Headless deterministic runs use no deadline.
    pub fn advance_until_alert_budgeted(&mut self,t:f64,watch:Option<FactionId>,deadline:Option<std::time::Instant>)->Option<Alert> {
        let mut count=0usize;
        loop {
            if count.is_multiple_of(32) && deadline.is_some_and(|d|std::time::Instant::now()>=d) {return None;}
            let Some((te,ev))=self.scheduler.pop_due(t) else {break};
            count+=1;
            let timer=self.profile.as_ref().map(|_|std::time::Instant::now());
            let category=match ev {Event::InterceptorGuide(_)=>0,Event::TacticalFrame=>1,Event::SensorFrame=>2,Event::MissileGuide(_)=>3,Event::PointDefence(_)=>4,_=>5};
            self.time = te.max(self.time);
            let seen = self.alerts.len();
            match ev {
                Event::InterceptorGuide(id)=>self.guide_interceptor(id),
                Event::PointDefence(id)=>self.point_defence_cycle(id),
                Event::PointDefencePulse(pulse)=>self.resolve_point_defence(pulse),
                Event::TacticalFrame => {
                    self.tactical_frame();
                    self.scheduler.schedule(self.time + TACTICAL_FRAME_S.value, Event::TacticalFrame);
                }
                Event::Step(id) => self.step(id),
                Event::BeamControl(id, order) => self.control_beam(id, order),
                Event::SensorFrame => {
                    self.sensor_frame();
                    self.scheduler.schedule(self.time + SENSOR_FRAME_S.value, Event::SensorFrame);
                }
                Event::MissileGuide(id) => self.guide_missile(id),
                Event::Beam(beam) => self.resolve_beam(beam),
                Event::QueuedLaunch(id, target, payload, generation) => {
                    let b = &mut self.bodies[id.0 as usize];
                    if b.launch_generation != generation { continue; }
                    if b.alive_at(self.time) && self.time<b.missile_ready_at[payload.index()] {
                        self.scheduler.schedule(b.missile_ready_at[payload.index()],Event::QueuedLaunch(id,target,payload,generation));
                        continue;
                    }
                    b.missile_queued[payload.index()] -= 1;
                    let faction = b.faction;
                    if b.alive_at(self.time) && self.launch(id, target, payload).is_err() {
                        self.alert(Some(faction), AlertKind::LaunchCancelled(id));
                    }
                }
            }
            if let (Some(profile),Some(timer))=(&mut self.profile,timer) {
                profile.seconds[category]+=timer.elapsed().as_secs_f64();
                profile.events[category]+=1;
                if profile.since.elapsed().as_secs_f64()>5.0 {
                    eprintln!("PROFILE sim={:.0}s wall={:.2}s seconds={:?} events={:?} [interceptor,tactical,sensors,missile,PD,other] sensor_parts={:?} [ping,echo,passive,relay,ingest]",self.time,profile.total.elapsed().as_secs_f64(),profile.seconds,profile.events,profile.sensor_parts);
                    profile.since=std::time::Instant::now();
                }
            }
            if let Some(w) = watch
                && let Some(a) = self.alerts[seen..].iter().filter(|a| a.faction.is_none_or(|f| f == w))
                    .min_by_key(|a|matches!(a.kind,AlertKind::NewContact(_)|AlertKind::ContactLost(_)))
            {
                return Some(a.clone());
            }
        }
        if t > self.time {
            self.time = t;
        }
        None
    }

    fn alert(&mut self, faction: Option<FactionId>, kind: AlertKind) {
        self.alerts.push(Alert { t: self.time, faction, kind });
    }

    fn decide(&mut self, winner: FactionId, reason: String) {
        if self.outcome.is_none() {
            self.outcome = Some(Outcome { winner, t: self.time, reason });
            self.alert(None, AlertKind::GameOver);
        }
    }

    /// Energy lands on a body: a raised screen stores what it can, the hull takes the
    /// rest, and the ship is lost once the hull has taken too much.
    fn deliver(&mut self, id: BodyId, t: f64, energy_j: f64, payload: Payload, missile: BodyId) {
        self.debug_note("DAMAGE",format!("event_time={t:.6} target={id:?} source={missile:?} payload={payload:?} energy_j={energy_j}"));
        let b = &mut self.bodies[id.0 as usize];
        if !b.alive_at(t) {
            return;
        }
        b.advance_thermal(t);
        let room = if b.has_screen { (SCREEN_CAPACITY_J.value * b.thermal.field*b.operating_effectiveness(crate::damage::System::Screens) - b.screen_j).max(0.0) } else {0.0};
        let missile_hit=payload!=Payload::Beam;
        let leak_chance=if missile_hit {crate::damage::MISSILE_SCREEN_LEAK_CHANCE} else {crate::damage::SCREEN_LEAK_CHANCE};
        let leak=if room>0.0 && energy_j>=1e6 && self.rng.uniform()<leak_chance {energy_j*crate::damage::SCREEN_LEAK_FRACTION} else {0.0};
        // Missile energy couples less efficiently into screen storage. The
        // rejected portion dissipates outside the field, not into the hull.
        let coupling=if payload==Payload::Beam {1.0} else {crate::damage::MISSILE_SCREEN_COUPLING};
        let overload=b.has_screen && b.thermal.field>0.0 && b.operating_effectiveness(crate::damage::System::Screens)>0.0
            && b.screen_j+(energy_j-leak)*coupling>SCREEN_CAPACITY_J.value;
        let screened_j = (energy_j-leak).min(room/coupling);
        b.screen_j += screened_j*coupling;
        b.thermal.captured_j += screened_j*coupling;
        let installed=b.installed_systems();
        let before=b.damage;
        let mut casualty=b.damage.penetrate(energy_j-screened_j,&installed,&mut self.rng);
        // A missile's rare screen puncture also shocks one installed component.
        if missile_hit && leak>0.0 && casualty.is_none() {casualty=b.damage.hit_system(&installed,&mut self.rng);}
        if overload {
            b.damage.screen_overload(&installed,&mut self.rng);
            b.controls.screens_latched=false;b.screen_up=false;b.thermal.field=0.0;
            // The failed field vents its reservoir; it cannot absorb another hit.
            b.thermal.radiated_j+=b.screen_j;b.screen_j=0.0;
        }
        b.hull_j=(b.damage.hull_max-b.damage.hull)*crate::damage::JOULES_PER_HP;
        let (faction, lost) = (b.faction, b.damage.hull<=0.0 || b.damage.state(crate::damage::System::Power)==crate::damage::Condition::Destroyed);
        let pos = b.trajectory.state_at(t).unwrap().pos;
        let damage=b.damage;
        let mut parts=vec![];
        if overload {parts.push("SCREEN OVERLOAD".into());}
        if screened_j>0.0 {parts.push(format!("SCREEN +{:.2} TJ",screened_j*coupling/1e12));}
        if before.armour>damage.armour {parts.push(format!("ARMOUR -{:.2}",before.armour-damage.armour));}
        if before.hull>damage.hull {parts.push(format!("HULL -{:.2}",before.hull-damage.hull));}
        for system in crate::damage::System::ALL {
            if before.state(system)!=damage.state(system) {parts.push(format!("{} {}",system.code(),if damage.state(system)==crate::damage::Condition::Destroyed {"DESTROYED"} else {"DAMAGED"}));}
        }
        let summary=parts.join(" · ");
        self.debug_note("DAMAGE_STATE",format!("target={id:?} hull={} armour={} leak_j={leak} system_hit={casualty:?}",damage.hull,damage.armour));
        self.debug_note("DAMAGE_DETAIL",format!("event_time={t:.6} target={id:?} {summary}"));
        self.record_combat_damage(t, pos, CombatKind::Impact, Some(id), Some(faction),Some((summary,refinements::impact_strength(&before,&damage))));
        if self.bodies[missile.0 as usize].missile.is_some_and(|m|m.target_body==id) {
            self.record_combat(t,pos,CombatKind::MissileHit,Some(missile),None);
        }
        self.hits.push(Hit { body: id, t, payload, missile, energy_j, screened_j });
        if lost {
            let cause = if self.bodies[missile.0 as usize].kind == BodyKind::Ship {
                LossCause::ShipBeam { shooter: missile }
            } else { LossCause::Missile { payload, missile } };
            self.destroy(id, t, cause);
        } else {
            self.guide(id);
            self.delay_alert(id, t, AlertKind::Hit(id));
        }
    }


    /// Request buildup or collapse; thermal integration preserves stored energy.
    pub fn set_screen(&mut self, id: BodyId, up: bool) -> Result<(), OrderError> {
        if !self.live_body_mut(id)?.has_screen { return Err(OrderError::Unarmed); }
        if up && self.live_body_mut(id)?.operating_effectiveness(crate::damage::System::Screens)==0.0 {return Err(OrderError::PowerOrHeat);}
        let t=self.time;
        let b=self.live_body_mut(id)?;
        b.advance_thermal(t);
        b.controls.screens=if up {controls::Mode::On} else {controls::Mode::Off};
        b.controls.screens_latched=up;
        b.screen_up = up;
        Ok(())
    }

    /// A body stops existing at `t`.
    fn destroy(&mut self, id: BodyId, t: f64, cause: LossCause) {
        if self.bodies[id.0 as usize].trajectory.end().is_some_and(|end|end<=t) {return;}
        self.probability_flights.remove(&id);
        self.debug_note("LOSS",format!("event_time={t:.6} body={id:?} cause={cause:?}"));
        let b = &mut self.bodies[id.0 as usize];
        let at = b.trajectory.state_at(t).map(|s| s.pos);
        // Catastrophic loss releases remaining stored field/electrical/thermal
        // energy into the destruction flash, rather than deleting the reservoirs.
        b.thermal.radiated_j += b.screen_j + b.thermal.capacitor_j + b.thermal.heat_j;
        b.screen_j = 0.0;
        b.thermal.capacitor_j = 0.0;
        b.thermal.heat_j = 0.0;
        b.thermal.field = 0.0;
        b.trajectory.terminate(t);
        b.autopilot = None;
        let (faction, name) = (b.faction, b.name.clone());
        // An interceptor has one mission, not a second attack or an orphaned
        // coast. Retire it with its target; observers still see causal telemetry.
        let orphaned:Vec<_>=self.bodies.iter().enumerate().filter(|(_,b)|
            b.alive_at(t) && b.trajectory.end().is_none_or(|end|end>t) && b.interceptor.is_some_and(|i|i.target==id))
            .map(|(i,_)|BodyId(i as u32)).collect();
        for interceptor in orphaned {
            self.debug_note("INTERCEPT_RETIRE",format!("missile={interceptor:?} target={id:?} reason=target_destroyed"));
            self.destroy(interceptor,t,LossCause::Expended);
        }
        self.losses.push(Loss { body: id, t, cause });
        if let Some(at) = at {
            self.record_combat(t, at, if cause == LossCause::Expended { CombatKind::Expended } else { CombatKind::Destroyed }, Some(id), Some(faction));
        }
        if cause != LossCause::Expended {
            self.delay_alert(id, t, AlertKind::ShipLost(id));
        }
        if let Some(o) = &self.objective
            && o.protect == id
        {
            let attacker = o.attacker;
            self.decide(attacker, format!("{name} was destroyed"));
        }
    }

    /// Has the protected body reached the objective?
    fn check_objective(&mut self, id: BodyId) {
        let Some(o) = &self.objective else { return };
        if o.protect != id || self.outcome.is_some() {
            return;
        }
        let Some(s) = self.bodies[id.0 as usize].trajectory.state_at(self.time) else { return };
        if (s.pos - o.center).length() < o.radius {
            let (winner, reason) = (o.defender, format!("{} reached the {}", self.bodies[id.0 as usize].name, o.name));
            self.decide(winner, reason);
        }
    }

    /// Check the last step for surface contact, then commit the next gravity sample.
    fn step(&mut self, id: BodyId) {
        let t = self.time;
        let i = id.0 as usize;
        let t_prev = self.last_step[i];
        let b = &mut self.bodies[i];
        let (Some(a), Some(s)) = (b.trajectory.state_at(t_prev), b.trajectory.state_at(t)) else { return };
        if t > t_prev
            && let Some((celestial, ti)) = self.system.impact(a.pos, s.pos, t_prev, t)
        {
            self.destroy(id, ti, LossCause::Impact(celestial));
            return;
        }
        self.check_objective(id);
        let b = &mut self.bodies[i];
        let dt = self.system.step_size(s.pos, t);
        let thrust = b.trajectory.last().thrust;
        let g = self.system.step_gravity(s, thrust, t, dt);
        b.trajectory.push(t, thrust, g).expect("steps advance in time");
        self.last_step[i] = t;
        self.scheduler.schedule(t + dt, Event::Step(id));
    }

    fn contact_id(&mut self, f: FactionId, body: BodyId) -> ContactId {
        if let Some(c)=self.association.get(&(f,body)) {return *c;}
        let next = ContactId(self.association.keys().filter(|(ff, _)| *ff == f).count() as u32 + 1);
        self.association.insert((f,body),next);
        self.reverse_association.insert((f,next),body);
        next
    }

    /// Scenario briefing only: a supplied initial solution from historical light,
    /// not an ongoing truth feed. Ordinary sensing takes over after setup.
    pub(crate) fn seed_debug_contact(&mut self, observer:BodyId,target:BodyId)->ContactId {
        assert_eq!(self.time,0.0,"briefings belong to initial scenario setup");
        let faction=self.bodies[observer.0 as usize].faction;
        let contact=self.contact_id(faction,target);
        {
            let received=0.0;
            let origin=self.state(observer,received).unwrap().pos;
            let trajectory=&self.bodies[target.0 as usize].trajectory;
            let (emitted,seen)=retarded_state(trajectory,origin,received).unwrap();
            let rel=seen.pos-origin;
            let picture=self.perceptions.get_mut(&faction).unwrap();
            picture.ingest(Observation {detection:crate::sensors::DetectionLevel::Resolved,contact,sensor:observer,origin,emitted_at:emitted,
                sensor_received_at:received,decider_received_at:received,source:Source::Emission,snr:9.0,
                measurement:Measurement::Bearing {bearing:bearing_of(rel),sigma:0.002}},&self.system);
        }
        contact
    }

    fn state(&self, id: BodyId, t: f64) -> Option<State> {
        self.bodies[id.0 as usize].trajectory.state_at(t)
    }

    fn sensor_frame(&mut self) {
        let mut timer=self.profile.as_ref().map(|_|std::time::Instant::now());
        let t = self.time;
        let t_prev = self.last_frame;
        self.last_frame = t;
        let mut reports: Vec<Observation> = vec![];

        // Pings in flight reaching bodies: echoes start back, targets may see the ping.
        let mut pings = std::mem::take(&mut self.pings);
        for ping in &mut pings {
            let emitter_faction = self.bodies[ping.emitter.0 as usize].faction;
            let mut still = vec![];
            for &target in &ping.pending {
                let traj = &self.bodies[target.0 as usize].trajectory;
                if traj.end().is_some_and(|e| e < t_prev) {
                    continue;
                }
                let Some(t_hit) = ping.front.arrival(traj, t_prev, t) else {
                    still.push(target);
                    continue;
                };
                let rx = traj.state_at(t_hit).expect("alive at arrival").pos;
                if self.system.occluder(ping.front.origin, ping.front.t_emit, rx, t_hit).is_some() {
                    continue;
                }
                let target_faction = self.bodies[target.0 as usize].faction;
                if target_faction == emitter_faction {
                    continue;
                }
                let out_km = ping.front.radius_at(t_hit);
                if ping.search_heading.is_none_or(|heading|out_km<=MISSILE_ACTIVE_RANGE_LS*crate::units::LIGHT_SECOND && missile::in_search_cone(heading,rx-ping.front.origin)) {
                    self.echoes.push(Echo { target,emitter: ping.emitter, front: Front { origin: rx, t_emit: t_hit }, out_km, power_w: ping.power_w });
                }
                if let Some((_, snr)) = sensors::receive_measurement_scaled(
                        self.bodies[target.0 as usize].sensors,
                        ping.signature_w / (1.0 + self.bodies[target.0 as usize].thermal.emission(self.bodies[target.0 as usize].screen_j) / SCREEN_GLARE_W.value), out_km, bearing_of(ping.front.origin - rx),self.bodies[target.0 as usize].sensor_effectiveness(), &mut self.rng,
                    ) {
                    let listener=&self.bodies[target.0 as usize];
                    if self.bodies[ping.emitter.0 as usize].interceptor.is_some()
                        || !listener.sensors.direction_finding || listener.sensor_effectiveness()[1]<=0.0
                        || !self.historical_signature(ping.emitter,ping.front.t_emit).is_some_and(|s|s.direction_active()) {continue;}
                    reports.push(Observation {detection:crate::sensors::DetectionLevel::Bearing,
                        contact: self.contact_id(target_faction, ping.emitter),
                        sensor: target,
                        origin: rx,
                        emitted_at: ping.front.t_emit,
                        sensor_received_at: t_hit,
                        decider_received_at: f64::NAN,
                        measurement:Measurement::Bearing {bearing:bearing_of(ping.front.origin-rx),sigma:0.002},
                        snr,
                        source: Source::Ping,
                    });
                }
            }
            ping.pending = still;
        }
        pings.retain(|p| !p.pending.is_empty() && p.front.radius_at(t) < MAX_FRONT_RADIUS_KM);
        self.pings = pings;
        self.profile_sensor_part(&mut timer,0);

        // Echoes returning to their pinger.
        let echoes = std::mem::take(&mut self.echoes);
        for echo in echoes {
            let emitter = &self.bodies[echo.emitter.0 as usize];
            if emitter.trajectory.end().is_some_and(|e| e < t_prev) {
                continue;
            }
            let Some(t_rx) = echo.front.arrival(&emitter.trajectory, t_prev, t) else {
                if echo.front.radius_at(t) < MAX_FRONT_RADIUS_KM {
                    self.echoes.push(echo);
                }
                continue;
            };
            let rx = self.state(echo.emitter, t_rx).expect("alive at arrival").pos;
            if self.system.occluder(echo.front.origin, echo.front.t_emit, rx, t_rx).is_some() {
                continue;
            }
            let back_km = echo.front.radius_at(t_rx);
            let receiver = &self.bodies[echo.emitter.0 as usize];
            if !receiver.sensors.active || receiver.operating_effectiveness(crate::damage::System::Active)==0.0 { continue; }
            let snr = sensors::echo_intensity(echo.power_w, echo.out_km, back_km) / ACTIVE_NOISE_FLOOR.value
                / (1.0 + receiver.thermal.emission(receiver.screen_j) / SCREEN_GLARE_W.value)
                * if matches!(receiver.kind,BodyKind::Probe|BodyKind::Missile) { PROBE_SENSOR_FACTOR.value } else { 1.0 };
            let ship_target=matches!(self.bodies[echo.target.0 as usize].kind,BodyKind::Ship|BodyKind::Station);
            let detection=if ship_target {
                self.detect_ship(echo.emitter,echo.target,echo.front.t_emit,echo.out_km.max(back_km),true)
            } else if sensors::echo_resolvable(snr) {sensors::DetectionLevel::Resolved} else {sensors::DetectionLevel::Bearing};
            let resolved=detection>=sensors::DetectionLevel::Resolved;
            if self.bodies[echo.target.0 as usize].interceptor.is_some() && !resolved {continue;}
            if detection==sensors::DetectionLevel::None || (!ship_target && (snr<9.0 || (!resolved && !sensors::detected(snr, &mut self.rng)))) {
                continue;
            }
            // Which body reflected it is truth; the report carries only the contact id.
            let faction = self.bodies[echo.emitter.0 as usize].faction;
            let target=echo.target;
            let m = sensors::measure_echo(bearing_of(echo.front.origin - rx), back_km, if ship_target {1e12} else {snr}, &mut self.rng);
            if !resolved {
                reports.push(Observation {detection:crate::sensors::DetectionLevel::Resolved,
                    contact:self.contact_id(faction,target),sensor:echo.emitter,origin:rx,
                    emitted_at:echo.front.t_emit,sensor_received_at:t_rx,decider_received_at:f64::NAN,
                    measurement:Measurement::Bearing {bearing:m.bearing,sigma:m.sigma_bearing},snr,source:Source::Echo,
                });
                continue;
            }
            // A returned echo is available locally before its report reaches the
            // frigate. Never substitute a current target truth state for the fix.
            if let Some(ms)=self.bodies[echo.emitter.0 as usize].missile.as_mut()
                && ms.target_body==target {
                let pos=rx+Vec2::new(m.bearing.cos(),m.bearing.sin())*m.range;
                if ms.local_fix.is_none_or(|fix|echo.front.t_emit>fix.t) {
                    let prior=ms.local_fix.map_or(Vec2::ZERO,|fix|fix.vel);
                    ms.local_fix=Some(sensors::SeekerFix::update(ms.local_fix,echo.front.t_emit,pos,prior));
                    ms.seeker=Some(Seeker::update(ms.seeker,t_rx,m.bearing));
                }
            }
            reports.push(Observation {detection,
                contact: self.contact_id(faction, target),
                sensor: echo.emitter,
                origin: rx,
                emitted_at: echo.front.t_emit,
                sensor_received_at: t_rx,
                decider_received_at: f64::NAN,
                measurement: Measurement::BearingRange {
                    bearing: m.bearing,
                    sigma_bearing: m.sigma_bearing,
                    range: m.range,
                    sigma_range: m.sigma_range,
                },
                snr,
                source: Source::Echo,
            });
        }

        // Passive: each live ship looks for the light now arriving from foreign bodies.
        self.profile_sensor_part(&mut timer,1);
        for si in 0..self.bodies.len() {
            let sensor = BodyId(si as u32);
            let s = &self.bodies[si];
            if !s.alive_at(t) || s.missile.is_some() {
                continue;
            }
            let Some(me) = s.trajectory.state_at(t) else { continue };
            let faction = s.faction;
            let suite=s.sensors;
            let glare = 1.0 + s.thermal.emission(s.screen_j) / SCREEN_GLARE_W.value;
            let sensitivity = if matches!(s.kind,BodyKind::Probe|BodyKind::Missile) { PROBE_SENSOR_FACTOR.value } else { 1.0 };
            let effectiveness=s.sensor_effectiveness();
            let eccm=s.operating_effectiveness(crate::damage::System::Eccm);
            for ti in 0..self.bodies.len() {
                let b = &self.bodies[ti];
                if b.faction == faction {
                    continue;
                }
                let Some((t_e, src)) = retarded_state(&b.trajectory, me.pos, t) else { continue };
                if self.system.occluder(src.pos, t_e, me.pos, t).is_some() {
                    continue;
                }
                let thrust = b.trajectory.thrust_at(t_e).unwrap_or(Vec2::ZERO);
                let range = (src.pos - me.pos).length();
                let countermeasures=(1.0+eccm)/(1.0+self.historical_ecm(BodyId(ti as u32),t_e));
                let measured=if matches!(b.kind,BodyKind::Ship|BodyKind::Station) {
                    let ef=self.historical_ef(BodyId(ti as u32),t_e);
                    let level=self.detect_ship(BodyId(si as u32),BodyId(ti as u32),t_e,range,false);
                    if level==sensors::DetectionLevel::None && !self.reverse_association.values().any(|id|*id==BodyId(ti as u32)) {continue;}
                    Some((sensors::ship_measurement(level,range,bearing_of(src.pos-me.pos),ef),100.0,level))
                } else {sensors::receive_measurement_scaled(suite,
                    (emission_w(b.kind,b.baseline_emission_factor,thrust) + self.thermal_emission(BodyId(ti as u32), t_e))
                        * sensitivity * countermeasures / glare, range, bearing_of(src.pos - me.pos),effectiveness, &mut self.rng,
                ).map(|(m,snr)|(m,snr,if matches!(m,Measurement::BearingRange {..}) {sensors::DetectionLevel::Resolved} else {sensors::DetectionLevel::Bearing}))};
                let Some((measurement,snr,detection))=measured else {continue;};
                if b.interceptor.is_some() && matches!(measurement,Measurement::Bearing {..}) {continue;}
                reports.push(Observation {detection,
                    contact: self.contact_id(faction, BodyId(ti as u32)),
                    sensor,
                    origin: me.pos,
                    emitted_at: t_e,
                    sensor_received_at: t,
                    decider_received_at: f64::NAN,
                    measurement,
                    snr,
                    source: Source::Emission,
                });
            }
        }

        self.ping_emissions.retain(|(_, front)| (t - front.t_emit) * crate::units::C / 2.0 < AU);
        self.hidden_ping_circles.retain(|(_,bits)| (t-f64::from_bits(*bits))*crate::units::C/2.0<AU);
        self.profile_sensor_part(&mut timer,2);

        // Route reports to each faction's decider: directly, or by laser relay at c.
        for obs in reports {
            let obs = self.bias_observation(obs);
            let faction = self.bodies[obs.sensor.0 as usize].faction;
            let front = Front { origin: self.state(obs.sensor, obs.sensor_received_at).unwrap().pos, t_emit: obs.sensor_received_at };
            self.relays.push(Relay { faction, front, obs });
        }
        let relays = std::mem::take(&mut self.relays);
        let mut delivered = vec![];
        for mut r in relays {
            let Some(decider) = self.decider(r.faction, t) else { continue };
            if decider == r.obs.sensor {
                r.obs.decider_received_at = r.obs.sensor_received_at;
                delivered.push(r.obs);
                continue;
            }
            let traj = &self.bodies[decider.0 as usize].trajectory;
            let lo = t_prev.max(r.front.t_emit);
            match r.front.arrival(traj, lo, t) {
                Some(t_arr) => {
                    let rx = traj.state_at(t_arr).unwrap().pos;
                    // A blocked laser link loses the report.
                    if self.system.occluder(r.front.origin, r.front.t_emit, rx, t_arr).is_none() {
                        r.obs.decider_received_at = t_arr;
                        delivered.push(r.obs);
                    }
                }
                None => self.relays.push(r),
            }
        }
        delivered.sort_by(|a, b| a.decider_received_at.total_cmp(&b.decider_received_at));
        self.profile_sensor_part(&mut timer,3);
        for obs in delivered {
            let faction = self.bodies[obs.sensor.0 as usize].faction;
            if self.decider(faction,t).is_none_or(|id|self.bodies[id.0 as usize].operating_effectiveness(crate::damage::System::Mind)==0.0) {continue;}
            self.observe_damage(&obs);
            let sys = &self.system;
            if let Some(p) = self.perceptions.get_mut(&faction) {
                let new = !p.contacts.contains_key(&obs.contact);
                p.ingest(obs, sys);
                if obs.source == Source::Ping {
                    let tr = p.contacts.get(&obs.contact).and_then(|c| c.track.as_ref());
                    let (bearing,pos,radius) = match obs.measurement {
                        Measurement::Bearing { bearing, .. } => (bearing, tr.map(|tr| tr.at(obs.emitted_at,sys).pos()),
                            tr.map_or(0.0,|tr| 2.0*tr.pos_cov()[0][0].max(tr.pos_cov()[1][1]).sqrt())),
                        Measurement::BearingRange { bearing, range, sigma_range, sigma_bearing } =>
                            (bearing,Some(obs.origin+Vec2::new(bearing.cos(),bearing.sin())*range),2.0*sigma_range.max(range*sigma_bearing)),
                    };
                    let sighting=crate::session::PingSighting { contact:obs.contact,emitted_at:obs.emitted_at,
                        received_at:obs.decider_received_at,pos,vel:tr.map_or(Vec2::ZERO,|tr| tr.vel()),initial_radius:radius,
                        observer:obs.origin,bearing };
                    self.refinement.record_ping(faction,sighting);
                }
                if new {
                    self.alerts.push(Alert { t, faction: Some(faction), kind: AlertKind::NewContact(obs.contact) });
                }
            }
        }

        // Each ship re-plans using its own delivered fire-control picture.
        for i in 0..self.bodies.len() {
            self.guide(BodyId(i as u32));
        }
        self.profile_sensor_part(&mut timer,4);
    }

    fn profile_sensor_part(&mut self,timer:&mut Option<std::time::Instant>,part:usize) {
        if let (Some(profile),Some(start))=(&mut self.profile,timer.as_mut()) {
            profile.sensor_parts[part]+=start.elapsed().as_secs_f64();
            *start=std::time::Instant::now();
        }
    }

}

struct SimulationProfile {seconds:[f64;6],events:[u64;6],sensor_parts:[f64;5],since:std::time::Instant,total:std::time::Instant}
impl Default for SimulationProfile {fn default()->Self {Self {seconds:[0.0;6],events:[0;6],sensor_parts:[0.0;5],since:std::time::Instant::now(),total:std::time::Instant::now()}}}

/// Isotropic emission of a body of `kind` thrusting at `thrust`.
fn emission_w(kind: BodyKind, baseline_factor:f64, thrust: Vec2) -> f64 {
    sensors::platform_emission_w(kind==BodyKind::Missile,baseline_factor,thrust,0.0)
}

/// A trajectory that has held `thrust` under gravity for `history_s` and arrives at
/// `state` at `t = 0`. History is integrated backwards, then committed forwards with
/// the same stepping the world uses, so it is self-consistent.
fn ballistic_history(sys: &System, state: State, thrust: Vec2, history_s: f64) -> Trajectory {
    let mut s = state;
    let mut t = 0.0;
    while t > -history_s {
        let dt = sys.step_size(s.pos, t).min(t + history_s);
        // Midpoint rule, stepping backwards in time.
        let a = thrust + sys.gravity(s.pos - s.vel * (0.5 * dt), t - 0.5 * dt);
        s = crate::kinematics::advance(s, a, -dt);
        t -= dt;
    }
    let mut traj = Trajectory::new(t, s);
    let mut tt = t;
    while tt < 0.0 {
        let cur = traj.state_at(tt).unwrap();
        let dt = sys.step_size(cur.pos, tt).min(-tt);
        let g = sys.step_gravity(cur, thrust, tt, dt);
        traj.push(tt, thrust, g).unwrap();
        tt += dt;
    }
    traj
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::celestial::{Celestial, CelestialKind, Orbit};
    use crate::missile::{Payload, Phase};
    use crate::units::{G0, LIGHT_SECOND};

    fn sun() -> System {
        System {
            bodies: vec![Celestial {
                name: "Sun".into(),
                kind: CelestialKind::Star,
                gm: 1.327_124_4e11,
                radius: 696_000.0,
                orbit: Orbit::Fixed(Vec2::ZERO),
            }],
        }
    }

    fn ship(name: &str, f: u8, pos: Vec2, vel: Vec2, thrust: Vec2) -> BodySpec {
        BodySpec { name: name.into(), kind: BodyKind::Ship, faction: FactionId(f), state: State { pos, vel }, thrust, magazine: 4 }
    }

    #[test]
    fn avoidance_pulls_a_diving_ship_out() {
        let dive = ship("Icarus", 0, Vec2::new(0.05 * AU, 0.0), Vec2::ZERO, Vec2::new(-10.0 * G0, 0.0));
        let mut w = World::new(sun(), vec![dive], 600.0, 1);
        w.advance_to(3.0 * 86_400.0);
        assert!(w.losses.is_empty(), "{:?}", w.losses);
        assert!(w.bodies[0].trajectory.state_at(3.0 * 86_400.0).is_some());
    }

    #[test]
    fn an_unavoidable_impact_is_fatal() {
        // 2,000 km above the surface, falling at 2,000 km/s: no 100 g burn can save it.
        let pos = Vec2::new(698_000.0, 0.0);
        let doomed = ship("Doomed", 0, pos, Vec2::new(-2_000.0, 0.0), Vec2::ZERO);
        let mut w = World::new(sun(), vec![doomed], 0.0, 1);
        assert!(w.bodies[0].avoidance.impossible);
        w.advance_to(60.0);
        assert_eq!(w.losses.len(), 1);
    }

    #[test]
    fn orbit_order_settles_into_a_circular_orbit() {
        let sys = crate::scenario::home_system();
        let p = sys.state(1, 0.0);
        let s = ship("Parker", 0, p.pos + Vec2::new(60_000.0, 0.0), p.vel + Vec2::new(4.0, 0.0), Vec2::ZERO);
        let mut w = World::new(sys, vec![s], 0.0, 1);
        w.set_orbit(BodyId(0), 1).unwrap();
        w.advance_to(86_400.0);
        let pl = w.system.state(1, w.time());
        let st = w.bodies[0].trajectory.state_at(w.time()).unwrap();
        let r = (st.pos - pl.pos).length();
        let vc = (w.system.bodies[1].gm / r).sqrt();
        assert!((r / 60_000.0 - 1.0).abs() < 0.03, "radius {r}");
        assert!(((st.vel - pl.vel).length() / vc - 1.0).abs() < 0.03, "circular speed");
        assert_eq!(w.bodies[0].autopilot.unwrap().status, AutopilotStatus::Holding);
    }

    #[test]
    fn intercept_order_joins_a_friendly_ship() {
        let base = Vec2::new(2.0 * AU, 0.0);
        let specs = vec![
            ship("Chaser", 0, base, Vec2::ZERO, Vec2::ZERO),
            ship("Runner", 0, base + Vec2::new(5.0 * LIGHT_SECOND, 0.0), Vec2::new(0.0, 30.0), Vec2::new(0.0, 1.0 * G0)),
        ];
        let mut w = World::new(sun(), specs, 0.0, 1);
        w.set_intercept(BodyId(0), InterceptTarget::Own(BodyId(1))).unwrap();
        w.advance_to(6.0 * 3600.0);
        let t = w.time();
        let (a, b) = (w.bodies[0].trajectory.state_at(t).unwrap(), w.bodies[1].trajectory.state_at(t).unwrap());
        assert!((a.pos - b.pos).length() < 2.0 * autopilot::STANDOFF_KM, "{}", (a.pos - b.pos).length());
        assert!(w.bodies[0].trajectory.thrust_at(t).unwrap().length() < 1.5 * G0, "throttled to match");
    }

    #[test]
    fn move_order_finishes_at_a_point_and_keeps_the_drive_off() {
        let sys = crate::scenario::home_system();
        let p = sys.state(1, 0.0);
        let s = ship("Mover", 0, p.pos + Vec2::new(60_000.0, 0.0), p.vel + Vec2::new(4.0, 0.0), Vec2::ZERO);
        let mut w = World::new(sys, vec![s], 0.0, 1);
        let target = p.pos + Vec2::new(-200_000.0, 150_000.0);
        w.set_move(BodyId(0), target).unwrap();
        assert!(matches!(w.bodies[0].autopilot.unwrap().order, Order::MoveTo { frame: 1, .. }));
        while w.time() < 3.0 * 3600.0 && w.bodies[0].autopilot.unwrap().status != AutopilotStatus::Holding {
            w.advance_to(w.time()+1.0);
        }
        let t = w.time();
        let pl = w.system.state(1, t);
        let st = w.bodies[0].trajectory.state_at(t).unwrap();
        let want = pl.pos + Vec2::new(-200_000.0, 150_000.0);
        assert!((st.pos - want).length() < 20.0, "off by {} km", (st.pos - want).length());
        assert!((st.vel - pl.vel).length() < 0.2, "at rest in the planet frame");
        assert_eq!(w.bodies[0].autopilot.unwrap().status, AutopilotStatus::Holding);
        for _ in 0..60 {
            w.advance_to(w.time()+60.0);
            assert_eq!(w.bodies[0].trajectory.last().thrust, Vec2::ZERO);
            assert_eq!(w.bodies[0].commanded, Vec2::ZERO);
            assert_eq!(w.bodies[0].autopilot.unwrap().status, AutopilotStatus::Holding);
        }
        // Gravity can move the ship after completion, but cannot restart its order.
        assert!((w.state(BodyId(0),w.time()).unwrap().pos-target).length()>10.0);
        w.set_move(BodyId(0),target+Vec2::new(10_000.0,0.0)).unwrap();
        w.advance_to(w.time()+2.0);
        assert!(w.bodies[0].trajectory.last().thrust.length()>0.0,"new orders resume the drive");
        assert!(w.losses.is_empty());
    }

    #[test]
    fn move_order_goes_around_the_planet_not_through_it() {
        let sys = crate::scenario::home_system();
        let p = sys.state(1, 0.0);
        let s = ship("Mover", 0, p.pos + Vec2::new(80_000.0, 0.0), p.vel, Vec2::ZERO);
        let mut w = World::new(sys, vec![s], 0.0, 1);
        w.set_move(BodyId(0), p.pos + Vec2::new(-80_000.0, 0.0)).unwrap();
        while w.time() < 3.0 * 3600.0 && w.bodies[0].autopilot.unwrap().status != AutopilotStatus::Holding {
            w.advance_to(w.time()+1.0);
        }
        assert_eq!(w.bodies[0].autopilot.unwrap().status, AutopilotStatus::Holding);
        assert!(w.losses.is_empty(), "{:?}", w.losses);
        let t = w.time();
        let st = w.bodies[0].trajectory.state_at(t).unwrap();
        let want = w.system.state(1, t).pos + Vec2::new(-80_000.0, 0.0);
        assert!((st.pos - want).length() < 50.0, "off by {} km", (st.pos - want).length());
    }

    #[test]
    fn cannot_move_into_a_planet() {
        let sys = crate::scenario::home_system();
        let p = sys.state(1, 0.0);
        let s = ship("Mover", 0, p.pos + Vec2::new(80_000.0, 0.0), p.vel, Vec2::ZERO);
        let mut w = World::new(sys, vec![s], 0.0, 1);
        assert_eq!(w.set_move(BodyId(0), p.pos), Err(OrderError::InvalidTarget));
    }

    #[test]
    fn reaching_the_region_wins_and_losing_the_ship_loses() {
        let base = Vec2::new(2.0 * AU, 0.0);
        let make = |thrust: Vec2| {
            let specs = vec![
                ship("Runner", 0, base, Vec2::ZERO, thrust),
                ship("Hunter", 1, base + Vec2::new(0.0, 0.05 * AU), Vec2::ZERO, Vec2::ZERO),
            ];
            let mut w = World::new(sun(), specs, 0.0, 1);
            w.objective = Some(Objective {
                name: "gate".into(),
                center: base + Vec2::new(1e6, 0.0),
                radius: 1e5,
                protect: BodyId(0),
                defender: FactionId(0),
                attacker: FactionId(1),
            });
            w
        };
        let mut w = make(Vec2::new(G0, 0.0));
        let mut kinds = vec![];
        while let Some(a) = w.advance_until_alert(86_400.0, Some(FactionId(1))) {
            kinds.push(a.kind.clone());
            if a.kind == AlertKind::GameOver {
                break;
            }
        }
        assert!(matches!(kinds[0], AlertKind::NewContact(_)), "the hunter sees the runner's drive first");
        assert_eq!(kinds.last(), Some(&AlertKind::GameOver), "everyone hears the result");
        assert_eq!(w.outcome.as_ref().unwrap().winner, FactionId(0));
        assert!(w.time() < 86_400.0, "stopped at the alert");

        let mut w = make(Vec2::ZERO);
        w.destroy(BodyId(0), 0.0, LossCause::Impact(0));
        assert_eq!(w.outcome.as_ref().unwrap().winner, FactionId(1));
    }

    #[test]
    fn a_new_contact_stops_the_clock_for_its_faction_only() {
        let base = Vec2::new(2.0 * AU, 0.0);
        let specs = vec![
            ship("Watcher", 0, base, Vec2::ZERO, Vec2::ZERO),
            // A cold ship that lights its drive at T+600; its light arrives ~30 s later.
            ship("Sleeper", 1, base + Vec2::new(30.0 * LIGHT_SECOND, 0.0), Vec2::ZERO, Vec2::ZERO),
        ];
        let mut w = World::new(sun(), specs, 3600.0, 1);
        w.bodies[0].sensors.passive=false; // Isolate DF: cold is silent, drive ignition is visible.
        assert!(w.advance_until_alert(600.0, Some(FactionId(0))).is_none());
        w.set_thrust(BodyId(1),Vec2::new(0.0,20.0*G0)).unwrap();
        let a = w.advance_until_alert(3600.0, Some(FactionId(0))).expect("noticed");
        assert!(matches!(a.kind, AlertKind::NewContact(_)));
        assert!(a.t >= 630.0 && a.t <= 650.0, "when the light arrived: {}", a.t);
    }

    /// Two shooters 3 ls apart and a target 4 ls away burning at `accel_g` across their
    /// line of sight, so it is tracked. Returns the world at T+60 and the contact.
    fn range(accel_g: f64) -> (World, ContactId) {
        let base = Vec2::new(2.0 * AU, 0.0);
        let specs = vec![
            ship("Shooter", 0, base, Vec2::ZERO, Vec2::ZERO),
            ship("Spotter", 0, base + Vec2::new(0.0, 3.0 * LIGHT_SECOND), Vec2::ZERO, Vec2::ZERO),
            ship("Target", 1, base + Vec2::new(4.0 * LIGHT_SECOND, 1.5 * LIGHT_SECOND), Vec2::new(0.0, 5.0), Vec2::new(0.0, accel_g * G0)),
        ];
        let mut w = World::new(sun(), specs, 3600.0, 11);
        for b in &mut w.bodies {b.controls.ecm=controls::Mode::Off;b.controls.screens=controls::Mode::Off;b.controls.boost=controls::Mode::Off;}
        w.ping(BodyId(0));
        w.advance_to(60.0);
        let c = *w.contact_truth(FactionId(0)).keys().next().expect("target tracked");
        (w, c)
    }

    fn delta_v_used(w: &World, id: BodyId) -> f64 {
        let traj = &w.bodies[id.0 as usize].trajectory;
        let segs = traj.segments();
        let end = traj.end().unwrap_or(w.time());
        segs.iter()
            .enumerate()
            .map(|(k, s)| {
                let t1 = segs.get(k + 1).map_or(end, |n| n.t0).min(end);
                s.thrust.length() * (t1 - s.t0).max(0.0)
            })
            .sum()
    }

    /// Controlled, accurate fire-control solution for testing physical beam delivery.
    fn beam_trial() -> (World, ContactId) {
        let base = Vec2::new(20.0 * AU, 0.0);
        let mut w = World::new(sun(), vec![
            ship("Emitter", 0, base, Vec2::ZERO, Vec2::ZERO),
            ship("Target", 1, base + Vec2::new(LIGHT_SECOND, 0.0), Vec2::ZERO, Vec2::ZERO),
        ], 10.0, 13);
        // Isolate weapon physics from the separately tested automation policy.
        for b in &mut w.bodies {b.controls.ecm=controls::Mode::Off;b.controls.screens=controls::Mode::Off;b.controls.boost=controls::Mode::Off;}
        let c = w.contact_id(FactionId(0), BodyId(1));
        w.perceptions.get_mut(&FactionId(0)).unwrap().ingest(Observation {detection:crate::sensors::DetectionLevel::Resolved,
            contact: c, sensor: BodyId(0), origin: base, emitted_at: 0.0,
            sensor_received_at: 0.0, decider_received_at: 0.0,
            measurement: Measurement::BearingRange { bearing: 0.0, range: LIGHT_SECOND, sigma_bearing: 1e-10, sigma_range: 1e-5 },
            snr: 1e9, source: Source::Echo,
        }, &w.system);
        (w, c)
    }

    #[test]
    fn screen_overload_is_a_single_logged_generator_failure() {
        use crate::damage::{Condition,System as S};
        let (mut w,_)=beam_trial();
        let b=&mut w.bodies[1];
        b.damage.hull=1000.0;b.damage.hull_max=1000.0;
        b.screen_up=true;b.thermal.field=1.0;b.screen_j=SCREEN_CAPACITY_J.value;
        w.deliver(BodyId(1),0.0,1e9,Payload::Beam,BodyId(0));
        let b=&w.bodies[1];
        assert_eq!(b.damage.state(S::Screens),Condition::Destroyed);
        assert_eq!(b.thermal.field,0.0);assert_eq!(b.screen_j,0.0);
        assert!(b.damage.hull<=800.0);
        let hit=b.damage.systems.iter().filter(|s|**s!=Condition::Intact).count();
        assert!((2..=5).contains(&hit));
        assert!(w.combat_events(None).iter().any(|e|e.damage.as_ref().is_some_and(|d|d.contains("SCREEN OVERLOAD") && d.contains("SCRN DESTROYED"))));
        let before=b.damage.hull;
        w.deliver(BodyId(1),0.0,1e9,Payload::Beam,BodyId(0));
        assert!(before-w.bodies[1].damage.hull<1.0,"failed generators cannot overload twice");
    }

    #[test]
    fn automatic_main_beam_leaves_missiles_to_dedicated_defences() {
        let (mut w,c)=beam_trial();
        w.bodies[1].kind=BodyKind::Missile;
        w.arm_beams(BodyId(0)).unwrap();
        assert_eq!(w.bodies[0].beam_target,None);
        assert_eq!(w.bodies[0].beam_emitted_j,0.0);
        w.engage_beam(BodyId(0),Some(c)).unwrap();
        assert_eq!(w.bodies[0].beam_emitted_j,SHIP_BEAM_ENERGY_J.value,"explicit manual orders still work");
    }

    #[test]
    fn armed_beams_acquire_without_target_orders_and_obey_hold_fire() {
        let (mut w, c) = beam_trial();
        w.arm_beams(BodyId(0)).unwrap();
        assert!(w.bodies[0].beam_auto);
        assert_eq!(w.bodies[0].beam_target, Some(c));
        assert_eq!(w.bodies[0].beam_emitted_j, SHIP_BEAM_ENERGY_J.value);
        w.advance_to(SHIP_BEAM_RECHARGE_S.value);
        assert_eq!(w.bodies[0].beam_emitted_j, 2.0 * SHIP_BEAM_ENERGY_J.value);
        w.engage_beam(BodyId(0), None).unwrap();
        w.advance_to(40.0);
        assert!(!w.bodies[0].beam_auto);
        assert_eq!(w.bodies[0].beam_emitted_j, 2.0 * SHIP_BEAM_ENERGY_J.value);

        let (mut w, c) = beam_trial();
        w.perceptions.get_mut(&FactionId(0)).unwrap().contacts.get_mut(&c).unwrap().track.as_mut().unwrap().x[0] += 10.0 * LIGHT_SECOND;
        w.arm_beams(BodyId(0)).unwrap();
        assert_eq!(w.bodies[0].beam_target, None);
        assert_eq!(w.bodies[0].beam_emitted_j, 0.0);
        w.perceptions.get_mut(&FactionId(0)).unwrap().contacts.get_mut(&c).unwrap().track.as_mut().unwrap().x[0] -= 10.0 * LIGHT_SECOND;
        w.advance_to(1.0);
        assert_eq!(w.bodies[0].beam_target, Some(c));
        assert_eq!(w.bodies[0].beam_emitted_j, SHIP_BEAM_ENERGY_J.value);
        let scenario = crate::scenario::transport_intercept();
        assert!(scenario.bodies[1].beam_auto && scenario.bodies[2].beam_auto);
        assert!(!scenario.bodies[0].beam_auto && !scenario.bodies[3].beam_auto);
    }

    #[test]
    fn automatic_beams_wait_for_estimated_range_and_a_track() {
        let (mut w, c) = beam_trial();
        let p = w.perceptions.get_mut(&FactionId(0)).unwrap();
        p.contacts.get_mut(&c).unwrap().track.as_mut().unwrap().x[0] += 10.0 * LIGHT_SECOND;
        w.arm_beams(BodyId(0)).unwrap();
        assert_eq!(w.bodies[0].beam_emitted_j, 0.0);
        w.perceptions.get_mut(&FactionId(0)).unwrap().contacts.get_mut(&c).unwrap().track.as_mut().unwrap().x[0] -= 10.0 * LIGHT_SECOND;
        w.control_beam(BodyId(0), w.bodies[0].beam_order);
        assert_eq!(w.bodies[0].beam_emitted_j, SHIP_BEAM_ENERGY_J.value);
        w.perceptions.get_mut(&FactionId(0)).unwrap().contacts.get_mut(&c).unwrap().track = None;
        w.time = 10.0;
        w.control_beam(BodyId(0), w.bodies[0].beam_order);
        assert_eq!(w.bodies[0].beam_emitted_j, SHIP_BEAM_ENERGY_J.value);
    }

    #[test]
    fn long_range_beams_use_received_confidence_not_a_hard_cutoff() {
        let (mut w,c)=beam_trial();
        let tr=w.perceptions.get_mut(&FactionId(0)).unwrap().contacts.get_mut(&c).unwrap().track.as_mut().unwrap();
        tr.x[0]+=29.0*LIGHT_SECOND;
        tr.p=[[0.0;6];6];
        assert!(w.beam_worth_firing(BodyId(0),c));
        w.arm_beams(BodyId(0)).unwrap();
        assert_eq!(w.bodies[0].beam_emitted_j,SHIP_BEAM_ENERGY_J.value);
        let (mut w,c)=beam_trial();
        w.perceptions.get_mut(&FactionId(0)).unwrap().contacts.get_mut(&c).unwrap().track.as_mut().unwrap().x[0]+=29.0*LIGHT_SECOND;
        assert!(!w.beam_worth_firing(BodyId(0),c));
        w.engage_beam(BodyId(0),Some(c)).unwrap();
        assert_eq!(w.bodies[0].beam_emitted_j,SHIP_BEAM_ENERGY_J.value,"directed shots accept the player's risk");
    }
    #[test]
    fn damaged_systems_change_capability_and_destroyed_weapons_stop() {
        use crate::damage::{System as S,Condition as D};
        let (mut w,c)=beam_trial();
        let max=w.bodies[0].max_accel();
        w.bodies[0].damage.systems[S::Propulsion as usize]=D::Damaged;
        assert_eq!(w.bodies[0].max_accel(),max*0.5);
        w.set_thrust(BodyId(0),Vec2::new(max,0.0)).unwrap();
        assert!((w.bodies[0].trajectory.last().thrust.length()-max*0.5).abs()<1e-8);
        w.bodies[0].damage.systems[S::Propulsion as usize]=D::Destroyed;
        w.set_thrust(BodyId(0),Vec2::new(max,0.0)).unwrap();
        assert_eq!(w.bodies[0].trajectory.last().thrust,Vec2::ZERO);
        w.bodies[0].damage.systems[S::Beam as usize]=D::Damaged;
        w.fire_beam(BodyId(0),c).unwrap();
        assert_eq!(w.bodies[0].beam_ready_at,2.0*SHIP_BEAM_RECHARGE_S.value);
        w.bodies[0].damage.systems[S::Beam as usize]=D::Destroyed;
        assert_eq!(w.fire_beam(BodyId(0),c),Err(OrderError::PowerOrHeat));
        w.bodies[0].damage.systems[S::Launcher as usize]=D::Destroyed;
        assert_eq!(w.queue_launch(BodyId(0),c,Payload::Nuclear),Err(OrderError::PowerOrHeat));
        w.bodies[0].damage.systems[S::Active as usize]=D::Destroyed;
        assert!(!w.ping(BodyId(0)));
        w.bodies[0].damage.systems[S::Passive as usize]=D::Damaged;
        assert_eq!(w.bodies[0].sensor_effectiveness(),[0.5,1.0]);
    }
    #[test]
    fn hull_damage_limits_thrust_at_inclusive_thresholds_and_stacks_with_system_damage() {
        use crate::damage::{System as S,Condition as D};
        let (mut w,_)=beam_trial();
        let base=w.bodies[0].max_accel();
        w.bodies[0].damage.hull_max=1000.0;
        for (hull,factor) in [(1000.0,1.0),(501.0,1.0),(500.0,0.75),(251.0,0.75),(250.0,0.5),(100.0,0.5)] {
            w.bodies[0].damage.hull=hull;
            assert!((w.bodies[0].max_accel()-base*factor).abs()<1e-12);
            w.set_thrust(BodyId(0),Vec2::new(base,0.0)).unwrap();
            assert!((w.bodies[0].trajectory.last().thrust.length()-base*factor).abs()<1e-12);
        }
        w.bodies[0].damage.systems[S::Propulsion as usize]=D::Damaged;
        assert_eq!(w.bodies[0].max_accel(),base*0.25);
        w.bodies[0].damage.systems[S::Power as usize]=D::Damaged;
        assert_eq!(w.bodies[0].max_accel(),0.0);
        w.bodies[0].damage.systems[S::Power as usize]=D::Intact;
        w.bodies[0].damage.systems[S::Propulsion as usize]=D::Intact;
        w.bodies[0].damage.hull=501.0;
        assert_eq!(w.bodies[0].max_accel(),base,"hull recovery restores thrust capacity");
    }
    #[test]
    fn power_failure_drifts_with_backup_sensors_and_power_destruction_kills_with_hull_remaining() {
        use crate::damage::{System as S,Condition as D};
        let (mut w,c)=beam_trial();
        w.set_thrust(BodyId(0),Vec2::new(G0,0.0)).unwrap();
        w.bodies[0].damage.systems[S::Power as usize]=D::Damaged;
        w.guide(BodyId(0));
        assert_eq!(w.bodies[0].trajectory.last().thrust,Vec2::ZERO);
        assert_eq!(w.bodies[0].sensor_effectiveness(),[1.0,1.0]);
        assert_eq!(w.bodies[0].operating_effectiveness(S::Crew),1.0);
        assert_eq!(w.bodies[0].operating_effectiveness(S::Repair),1.0);
        assert!(!w.ping(BodyId(0)));
        assert_eq!(w.fire_beam(BodyId(0),c),Err(OrderError::PowerOrHeat));
        assert_eq!(w.queue_launch(BodyId(0),c,Payload::Nuclear),Err(OrderError::PowerOrHeat));
        w.bodies[0].damage.systems[S::Power as usize]=D::Destroyed;
        w.deliver(BodyId(0),0.0,1.0,Payload::Beam,BodyId(1));
        assert!(w.bodies[0].damage.hull>0.0);
        assert!(!w.bodies[0].alive_at(0.001));
    }
    #[test]
    fn automatic_beams_and_thrust_stay_off_until_power_repair() {
        use crate::damage::{System as S,Condition as D};
        let (mut w,c)=beam_trial();
        w.set_thrust(BodyId(0),Vec2::new(G0,0.0)).unwrap();
        w.bodies[0].damage.systems[S::Power as usize]=D::Damaged;
        w.guide(BodyId(0));
        w.engage_beam(BodyId(0),Some(c)).unwrap();
        let shots=w.bodies[0].beam_emitted_j;
        w.advance_to(100.0);
        assert_eq!(w.bodies[0].trajectory.last().thrust,Vec2::ZERO);
        assert_eq!(w.bodies[0].beam_emitted_j,shots);
        for s in [S::Active,S::Ecm,S::Eccm,S::Propulsion,S::Screens,S::PdMissiles,S::PdLaser,S::Beam,S::Launcher] {
            assert_eq!(w.bodies[0].operating_effectiveness(s),0.0,"{s:?}");
        }
        w.advance_to(121.0);
        assert_eq!(w.bodies[0].damage.state(S::Power),D::Intact);
        assert!(w.bodies[0].trajectory.last().thrust.length()>0.0);
    }
    #[test]
    fn exhausted_ui_budget_keeps_pending_simulation_events() {
        let (mut w,c)=beam_trial();
        w.fire_beam(BodyId(0),c).unwrap();
        w.advance_until_alert_budgeted(5.0,None,Some(std::time::Instant::now()));
        assert_eq!(w.time(),0.0);
        assert!(w.hits.is_empty());
        w.advance_to(5.0);
        assert!(!w.hits.is_empty(),"budget yield must not drop the pending beam arrival");
    }

    #[test]
    fn screens_can_leak_small_hits_without_losing_energy() {
        let (mut w,_)=beam_trial();
        w.bodies[1].screen_up=true;w.bodies[1].thermal.field=1.0;
        let mut leaks=0;
        for _ in 0..400 {
            w.bodies[1].damage=crate::damage::Damage::default();
            w.deliver(BodyId(1),0.0,1e10,Payload::Beam,BodyId(0));
            let hit=w.hits.last().unwrap();
            let d=w.bodies[1].damage;
            let absorbed=(d.hull_max-d.hull+d.armour_max-d.armour)*crate::damage::JOULES_PER_HP;
            assert!((hit.screened_j+absorbed-hit.energy_j).abs()<1.0);
            if d.hull<d.hull_max {leaks+=1;assert!((hit.energy_j-hit.screened_j-1e8).abs()<1.0);}
        }
        assert!((5..40).contains(&leaks),"small leak probability: {leaks}/400");
    }

    #[test]
    fn automatic_beams_repeat_and_cease_fire_cancels_pending_shots() {
        let (mut w, c) = beam_trial();
        w.engage_beam(BodyId(0), Some(c)).unwrap();
        assert_eq!(w.bodies[0].beam_emitted_j, SHIP_BEAM_ENERGY_J.value);
        w.advance_to(SHIP_BEAM_RECHARGE_S.value - 0.01);
        assert_eq!(w.bodies[0].beam_emitted_j, SHIP_BEAM_ENERGY_J.value);
        w.advance_to(SHIP_BEAM_RECHARGE_S.value);
        assert_eq!(w.bodies[0].beam_emitted_j, 2.0 * SHIP_BEAM_ENERGY_J.value);
        w.engage_beam(BodyId(0), None).unwrap();
        w.advance_to(40.0);
        assert_eq!(w.bodies[0].beam_emitted_j, 2.0 * SHIP_BEAM_ENERGY_J.value);
    }

    #[test]
    fn automatic_beams_hold_on_stale_tracks_and_power_limits() {
        let (mut w,c)=beam_trial();
        w.time=TRACK_STALE_S.value+1.0;
        w.engage_beam(BodyId(0),Some(c)).unwrap();
        assert_eq!(w.bodies[0].beam_emitted_j,0.0);
        let (mut w,c)=beam_trial();
        w.bodies[0].thermal.capacitor_j=0.0;
        assert_eq!(w.fire_beam(BodyId(0),c),Err(OrderError::PowerOrHeat));
        assert_eq!(w.bodies[0].beam_emitted_j,0.0);
        w.bodies[0].thermal.capacitor_j=BEAM_CAPACITOR_J.value;
        w.bodies[0].thermal.heat_j=BEAM_HEAT_LIMIT_J.value;
        assert_eq!(w.fire_beam(BodyId(0),c),Err(OrderError::PowerOrHeat));
    }

    #[test]
    fn ship_beams_travel_at_c_and_survive_the_emitter() {
        let (mut w, c) = beam_trial();
        w.fire_beam(BodyId(0), c).unwrap();
        w.advance_to(0.9);
        assert!(w.hits.is_empty());
        w.destroy(BodyId(0), 0.9, LossCause::Impact(0));
        w.advance_to(2.0);
        let hit = w.hits.first().expect("emitted light still hits");
        assert!((hit.t - 1.0).abs() < 0.001);
        assert!(hit.energy_j > 1e12 && hit.energy_j <= SHIP_BEAM_ENERGY_J.value);
        assert!(w.bodies[1].damage.hull<100.0,"surviving target still takes damage from the emitted pulse");
    }

    #[test]
    fn ship_beams_use_tracks_and_require_a_ready_emitter() {
        let (mut w, c) = beam_trial();
        assert_eq!(w.fire_beam(BodyId(0), ContactId(99)), Err(OrderError::NoTrack));
        assert_eq!(w.bodies[0].beam_emitted_j, 0.0);
        w.bodies[0].armed = false;
        assert_eq!(w.fire_beam(BodyId(0), c), Err(OrderError::Unarmed));
        w.bodies[0].armed = true;
        w.bodies[0].magazine = [0; 2];
        // False track: the pulse must follow the estimate, not snap onto truth.
        w.perceptions.get_mut(&FactionId(0)).unwrap().contacts.get_mut(&c).unwrap().track.as_mut().unwrap().x[1] += 10_000.0;
        w.fire_beam(BodyId(0), c).unwrap();
        assert_eq!(w.fire_beam(BodyId(0), c), Err(OrderError::BeamRecharging));
        w.advance_to(SHIP_BEAM_RECHARGE_S.value);
        assert!(w.hits.is_empty(), "bad fire-control solution misses");
        w.fire_beam(BodyId(0), c).unwrap();
        assert_eq!(w.bodies[0].beam_emitted_j, 2.0 * SHIP_BEAM_ENERGY_J.value);
        assert_eq!(w.bodies[0].magazine, [0; 2]);
    }

    #[test]
    fn ship_beam_energy_enters_screens_and_obeys_occlusion() {
        let (mut w, c) = beam_trial();
        w.bodies[1].screen_up = true;
        w.bodies[1].thermal.field = 1.0; // This fixture starts with an established field.
        w.fire_beam(BodyId(0), c).unwrap();
        w.advance_to(2.0);
        assert!(w.bodies[1].screen_j > 0.0);
        assert_eq!(w.bodies[1].hull_j, 0.0);
        assert_eq!(w.bodies[1].thermal.captured_j, w.hits[0].energy_j);
        assert!(w.bodies[1].thermal.balance_error(w.bodies[1].screen_j).abs() < 100.0);

        let (mut w, c) = beam_trial();
        // Insert a star directly in the shot path after establishing the test track.
        let midpoint = (w.state(BodyId(0), 0.0).unwrap().pos + w.state(BodyId(1), 0.0).unwrap().pos) * 0.5;
        w.system.bodies[0].orbit = crate::celestial::Orbit::Fixed(midpoint);
        w.system.bodies[0].radius = 1000.0;
        w.system.bodies[0].gm = 0.0;
        w.fire_beam(BodyId(0), c).unwrap();
        w.advance_to(2.0);
        assert!(w.hits.is_empty(), "celestial body blocks the beam");
    }

    #[test]
    fn missiles_deposit_reduced_heat_but_keep_unscreened_damage() {
        for payload in Payload::ALL {
            let (mut w, _) = beam_trial();
            w.bodies[1].screen_up=true;
            w.bodies[1].thermal.field=1.0;
            w.deliver(BodyId(1),0.0,1e5,payload,BodyId(0));
            assert_eq!(w.bodies[1].screen_j,4e4);
            assert_eq!(w.bodies[1].thermal.captured_j,4e4);
            assert_eq!(w.bodies[1].hull_j,0.0);
            let (mut bare, _) = beam_trial();
            bare.bodies[1].has_screen=false;
            bare.deliver(BodyId(1),0.0,1e12,payload,BodyId(0));
            assert_eq!(bare.hits[0].screened_j,0.0);
            assert!(bare.bodies[1].hull_j>0.0);
        }
    }

    #[test]
    fn ship_beam_spreading_reduces_coupled_energy() {
        let energy = |distance: f64| {
            let (mut w, c) = beam_trial();
            let origin = w.state(BodyId(0), 0.0).unwrap().pos;
            let pos = origin + Vec2::new(distance * LIGHT_SECOND, 0.0);
            w.bodies[1].trajectory = Trajectory::new(0.0, State { pos, vel: Vec2::ZERO });
            w.bodies[1].screen_up = true;
            let tr = w.perceptions.get_mut(&FactionId(0)).unwrap().contacts.get_mut(&c).unwrap().track.as_mut().unwrap();
            tr.x[0] = pos.x;
            tr.x[1] = pos.y;
            w.fire_beam(BodyId(0), c).unwrap();
            w.advance_to(distance + 1.0);
            w.hits[0].energy_j
        };
        let (near, far) = (energy(2.0), energy(10.0));
        assert!(near > far * 20.0 && near < far * 30.0, "{near} vs {far}");
    }

    #[test]
    fn only_two_offensive_missile_types_are_launchable() {
        let (mut w,c)=beam_trial();
        assert_eq!(Payload::ALL.len(),2);
        assert_eq!(w.launch(BodyId(0),c,Payload::Beam),Err(OrderError::InvalidTarget));
        assert_eq!(w.queue_launch(BodyId(0),c,Payload::Beam),Err(OrderError::InvalidTarget));
    }

    #[test]
    fn nuclear_round_bursts_once_at_scheduled_attack_and_is_removed() {
        let (mut w,c)=beam_trial();
        w.bodies[1].damage.hull=1e6;w.bodies[1].damage.hull_max=1e6;
        let m=w.launch(BodyId(0),c,Payload::Nuclear).unwrap();
        let due=w.probability_flights[&m].due;
        w.advance_to(due-0.01);
        assert!(w.bodies[m.0 as usize].alive_at(w.time()));
        w.advance_to(due+1.0);
        assert_eq!(w.bodies[m.0 as usize].trajectory.end(),Some(due));
        let count=|w:&World|w.refinement.truth_events.iter().filter(|e|e.kind==CombatKind::NuclearBurst && e.own_body==Some(m)).count();
        assert_eq!(count(&w),1);
        w.advance_to(due+100.0);assert_eq!(count(&w),1);
    }

    #[test]
    fn nuclear_fuse_does_not_fire_while_still_closing() {
        let (mut w, c) = beam_trial();
        let target = w.state(BodyId(1), 0.0).unwrap().pos;
        let m = w.launch(BodyId(0), c, Payload::Nuclear).unwrap();
        w.bodies[m.0 as usize].trajectory = Trajectory::new(0.0, State {
            pos: target + Vec2::new(-1000.0, 1000.0), vel: Vec2::new(1000.0, 0.0),
        });
        w.time = 0.5;
        w.guide_missile(m);
        assert!(w.bodies[m.0 as usize].trajectory.end().is_none());
        assert!(w.hits.is_empty());
    }

    #[test]
    fn nuclear_salvo_finishes_its_pass_after_the_target_is_destroyed() {
        let (mut w, c) = beam_trial();
        w.bodies[0].magazine = [10; 2];
        for _ in 0..10 { w.queue_launch(BodyId(0), c, Payload::Nuclear).unwrap(); }
        w.advance_to(4000.0);
        let alive: Vec<_> = w.bodies.iter().filter(|b| b.kind == BodyKind::Missile && b.alive_at(w.time()))
            .map(|b| (&b.name, b.missile.unwrap().phase, b.missile.unwrap().dv_left)).collect();
        assert!(w.bodies[1].trajectory.end().is_some(), "lead warhead hits");
        assert!(alive.is_empty(), "missiles should burst after their pass, not fly on: {alive:?}");
    }

    #[test]
    fn kinetic_attack_retires_at_the_scheduled_deadline() {
        let (mut w,c)=beam_trial();
        let m=w.launch(BodyId(0),c,Payload::Kinetic).unwrap();
        let due=w.probability_flights[&m].due;
        w.advance_to(due+1.0);
        assert_eq!(w.bodies[m.0 as usize].trajectory.end(),Some(due));
        let results=w.refinement.truth_events.iter().filter(|e|e.own_body==Some(m)
            && matches!(e.kind,CombatKind::MissileHit|CombatKind::MissileMiss)).count();
        assert_eq!(results,1,"exactly one attack result, no second pass");
    }

    #[test]
    fn missed_missiles_retire_even_with_fuel_and_a_dead_target() {
        {
            let payload=Payload::Kinetic;
            let (mut w, c) = beam_trial();
            let m = w.launch(BodyId(0), c, payload).unwrap();
            w.advance_to(0.1);
            w.destroy(BodyId(1), 0.1, LossCause::Impact(0));
            w.advance_to(4000.0);
            assert!(w.bodies[m.0 as usize].trajectory.end().is_some(), "{payload:?} should leave play after its pass");
            assert!(w.bodies[m.0 as usize].missile.unwrap().dv_left > 0.0, "removal must not require exhausting fuel");
        }
    }

    #[test]
    fn active_sensors_do_not_resolve_the_out_of_range_scenario_target() {
        let mut w = crate::scenario::transport_intercept();
        // Place the raider beyond the newly normalized envelopes, not the old
        // opening distance which now legitimately admits some ranged contacts.
        let pos=w.state(BodyId(1),0.0).unwrap().pos+Vec2::new(3.0*AU,0.0);
        w.bodies[2].trajectory=Trajectory::new(-3600.0,State {pos,vel:Vec2::ZERO});
        w.bodies[2].commanded=Vec2::ZERO;
        w.ping(BodyId(2));
        w.advance_to(4000.0);
        let p = w.perception(crate::scenario::RAIDER).unwrap();
        let echoes = p.log.iter().filter(|o| o.source == Source::Echo).count();
        assert_eq!(echoes,0,"out-of-range returns must not produce range fixes");
        // Passive approximate tracks of conspicuous platforms are independent
        // of whether this active pulse can identify them.
    }

    #[test]
    fn evade_burns_away_using_the_received_target() {
        let (mut w,c)=beam_trial();
        let delta=w.state(BodyId(1),0.0).unwrap().pos-w.state(BodyId(0),0.0).unwrap().pos;
        w.set_tactical_range(BodyId(0),InterceptTarget::Contact(c),None).unwrap();
        let thrust=w.bodies[0].trajectory.last().thrust;
        assert!(thrust.dot(delta)<0.0);
        assert!((thrust.length()-w.bodies[0].max_accel().min(w.bodies[0].drive_limit)).abs()<1e-8);
    }

    #[test]
    fn all_range_orders_close_on_bearings_then_hold_the_received_range() {
        for range in [Payload::Nuclear,Payload::Kinetic,Payload::Beam].map(autopilot::weapon_standoff) {
            let (mut w,c)=beam_trial();
            let origin=w.state(BodyId(0),0.0).unwrap().pos;
            let mut obs=w.perceptions[&FactionId(0)].contacts[&c].last;
            let original=obs;
            obs.detection=sensors::DetectionLevel::Bearing;
            obs.measurement=Measurement::Bearing {bearing:0.0,sigma:0.01};
            w.perceptions.get_mut(&FactionId(0)).unwrap().contacts.remove(&c);
            w.perceptions.get_mut(&FactionId(0)).unwrap().ingest(obs,&w.system);
            w.set_tactical_range(BodyId(0),InterceptTarget::Contact(c),Some(range)).unwrap();
            assert!(w.bodies[0].trajectory.last().thrust.x>0.0,"close on bearing: {range}");
            // The first fix places the ship inside its selected separation: withdraw.
            w.perceptions.get_mut(&FactionId(0)).unwrap().ingest(original,&w.system);
            let contact=w.perceptions.get_mut(&FactionId(0)).unwrap().contacts.get_mut(&c).unwrap();
            let tr=contact.track.as_mut().unwrap();
            tr.x[0]=origin.x+range*0.5;tr.x[1]=origin.y;
            tr.x[2]=0.0;tr.x[3]=0.0;tr.x[4]=0.0;tr.x[5]=0.0;
            w.guide(BodyId(0));
            assert!(w.bodies[0].trajectory.last().thrust.x<0.0,"withdraw after fix: {range}");
            w.time=TRACK_STALE_S.value+1.0;
            w.guide(BodyId(0));
            assert_eq!(w.bodies[0].trajectory.last().thrust,Vec2::ZERO,"coast on stale evidence");
        }
    }

    #[test]
    fn each_payload_kills_an_unaware_target() {
        for payload in Payload::ALL {
            let (mut w, c) = beam_trial();
            // A fragile unarmoured target isolates payload delivery; combat
            // frigates intentionally survive individual penetrating hits.
            w.bodies[1].damage.hull=0.001;
            w.bodies[1].damage.hull_max=0.001;
            w.bodies[1].damage.armour=0.0;
            let m = w.launch(BodyId(0), c, payload).unwrap();
            // A perfect shot: this checks the payloads, not the spread.
            assert_eq!(w.bodies[0].magazine[payload.index()], 3);
            w.advance_to(8.0 * 3600.0);
            let loss = w.losses.iter().find(|l| l.body == BodyId(1));
            assert!(
                matches!(loss, Some(Loss { cause: LossCause::Missile { payload: p, .. }, .. }) if *p == payload),
                "{payload:?}: {:?}",
                w.losses
            );
            let used = delta_v_used(&w, m);
            assert!(used <= MISSILE_DELTA_V_KMS.value * 1.001, "{payload:?} used {used} km/s");
            assert!(w.outcome.is_none());
        }
    }

    /// One engagement at a fixed closing speed `vc` (km/s) against a target jinking at
    /// 100 g in a random direction every 2 s. Returns whether the target died.
    fn closing_trial(vc: f64, payload: Payload, seed: u64) -> bool {
        let base = Vec2::new(2.0 * AU, 0.0);
        let target = base + Vec2::new(0.0, 0.0);
        let flight = 150.0;
        // The spotters come first so one of them is the flagship that fuses reports.
        let specs = vec![
            ship("SpotterA", 0, target + Vec2::new(0.0, 3.0 * LIGHT_SECOND), Vec2::ZERO, Vec2::ZERO),
            ship("SpotterB", 0, target + Vec2::new(-2.0 * LIGHT_SECOND, -2.0 * LIGHT_SECOND), Vec2::ZERO, Vec2::ZERO),
            ship("Shooter", 0, target - Vec2::new(vc * (flight + 30.0), 0.0), Vec2::new(vc, 0.0), Vec2::ZERO),
            ship("Target", 1, target, Vec2::ZERO, Vec2::new(0.0, 100.0 * G0)),
        ];
        let mut w = World::new(sun(), specs, 600.0, seed);
        let mut rng = crate::rng::Rng::stream(seed, 99);
        let jink = |w: &mut World, rng: &mut crate::rng::Rng| {
            let a = std::f64::consts::TAU * rng.uniform();
            w.set_thrust(BodyId(3), Vec2::new(a.cos(), a.sin()) * (100.0 * G0)).unwrap();
        };
        while w.time() < 30.0 {
            let t = w.time() + 2.0;
            w.advance_to(t);
            jink(&mut w, &mut rng);
        }
        let c = *w.contact_truth(FactionId(0)).iter().find(|(_, b)| **b == BodyId(3)).expect("tracked").0;
        let m = w.launch(BodyId(2), c, payload).unwrap_or_else(|e| panic!("launch at {vc} km/s: {e:?}"));
        // No launch burn: the missile keeps the shooter's closing speed.
        if let Some(ms) = w.bodies[m.0 as usize].missile.as_mut() {
            ms.burn_left = 0.0;
            ms.phase = Phase::Cruise;
        }
        while w.time() < 30.0 + 2.0 * flight {
            let t = w.time() + 2.0;
            w.advance_to(t);
            if w.bodies[3].trajectory.end().is_some() {
                return true;
            }
            jink(&mut w, &mut rng);
        }
        false
    }

    /// Calibration: a missile launched from `range_km` at 0.01c closing, against a target
    /// jinking at 10 g in a random direction every 60 s with its screen down. Spotters
    /// fly the same jinks in formation 3 ls from the target, so the track stays fresh and
    /// the spread comes from the shot itself.
    /// Returns whether the payload hit on its first pass.
    fn range_trial(payload: Payload, range_km: f64, seed: u64) -> bool {
        let vc = 0.01 * crate::units::C;
        let target = Vec2::new(40.0 * AU, 0.0);
        let specs = vec![
            ship("SpotterA", 0, target + Vec2::new(0.0, 3.0 * LIGHT_SECOND), Vec2::ZERO, Vec2::new(10.0 * G0, 0.0)),
            ship("SpotterB", 0, target + Vec2::new(-2.0 * LIGHT_SECOND, -2.0 * LIGHT_SECOND), Vec2::ZERO, Vec2::new(10.0 * G0, 0.0)),
            ship("Shooter", 0, target - Vec2::new(0.0, range_km), Vec2::ZERO, Vec2::ZERO),
            ship("Target", 1, target, Vec2::ZERO, Vec2::new(10.0 * G0, 0.0)),
        ];
        let mut w = World::new(sun(), specs, 600.0, seed);
        let mut rng = crate::rng::Rng::stream(seed, 99);
        let jink = |w: &mut World, rng: &mut crate::rng::Rng| {
            let a = std::f64::consts::TAU * rng.uniform();
            let t = w.time();
            let thrust = Vec2::new(a.cos(), a.sin()) * (10.0 * G0);
            for k in [0, 1, 3] {
                if w.bodies[k].alive_at(t) {
                    w.set_thrust(BodyId(k as u32), thrust).unwrap();
                }
            }
        };
        w.advance_to(60.0);
        let c = *w.contact_truth(FactionId(0)).iter().find(|(_, b)| **b == BodyId(3)).expect("tracked").0;
        let m = w.launch(BodyId(2), c, payload).unwrap();
        // Already coasting at the closing speed: no launch burn.
        let t0 = w.time();
        let start = w.bodies[2].trajectory.state_at(t0).unwrap();
        let aim = (w.bodies[3].trajectory.state_at(t0).unwrap().pos - start.pos).normalized();
        let mb = &mut w.bodies[m.0 as usize];
        mb.trajectory = Trajectory::new(t0, State { pos: start.pos, vel: start.vel + aim * vc });
        if let Some(ms) = mb.missile.as_mut() {
            ms.burn_left = 0.0;
            ms.phase = Phase::Cruise;
            if std::env::var("LUMINAL_NO_DRIFT").is_ok() {
            }
        }
        let end = t0 + range_km / vc + 120.0;
        let mut closest = f64::INFINITY;
        while w.time() < end {
            let t_prev = w.time();
            let t = (t_prev + 60.0).min(end);
            w.advance_to(t);
            let (mt, tt) = (&w.bodies[m.0 as usize].trajectory, &w.bodies[3].trajectory);
            if let Some((_, d)) = missile::closest_approach(t_prev, t, |tau| Some(tt.state_at(tau)?.pos - mt.state_at(tau)?.pos)) {
                closest = closest.min(d);
            }
            if w.hits.iter().any(|h| h.body == BodyId(3) && h.missile == m) {
                return true;
            }
            jink(&mut w, &mut rng);
        }
        if std::env::var("LUMINAL_DEBUG").is_ok() {
            eprintln!("  {payload:?} seed {seed}: missed, closest {closest:.0} km");
        }
        false
    }

    /// The design's 50 % hit ranges at 0.01c.
    fn half_range(payload: Payload) -> f64 {
        match payload {
            Payload::Kinetic => 0.01 * AU,
            Payload::Nuclear => AU,
            Payload::Beam => 10.0 * AU,
        }
    }

    /// `LUMINAL_SURVEY_N` trials per point (default 40); `LUMINAL_SURVEY_AT` a comma list
    /// of range factors (default 0.3,1,3); `LUMINAL_SURVEY_PAYLOAD` one payload name.
    #[test]
    #[ignore]
    fn range_survey() {
        let n: u64 = std::env::var("LUMINAL_SURVEY_N").ok().and_then(|v| v.parse().ok()).unwrap_or(40);
        let at: Vec<f64> = std::env::var("LUMINAL_SURVEY_AT")
            .ok()
            .map(|v| v.split(',').filter_map(|x| x.parse().ok()).collect())
            .unwrap_or(vec![0.3, 1.0, 3.0]);
        let only = std::env::var("LUMINAL_SURVEY_PAYLOAD").ok();
        for payload in Payload::ALL.into_iter().filter(|p| only.as_deref().is_none_or(|o| o == p.name())) {
            for &f in &at {
                let r = f * half_range(payload);
                let clock = std::time::Instant::now();
                let hits = (0..n).filter(|&s| range_trial(payload, r, 1000 + s)).count();
                println!("{payload:?} at {:.3} AU: {hits}/{n} hits ({:.1} s)", r / AU, clock.elapsed().as_secs_f64());
            }
        }
    }

    #[test]
    #[ignore]
    fn closing_debug() {
        for (vc, p) in [(100.0, Payload::Kinetic)] {
            for seed in 100..101 {
                let hit = closing_trial(vc, p, seed);
                eprintln!("{p:?} {vc} seed {seed}: hit {hit}");
            }
        }
    }

    #[test]
    #[ignore]
    fn closing_speed_survey() {
        for payload in Payload::ALL {
            for vc in [100.0, 1_000.0, 3_000.0, 10_000.0, 30_000.0, 100_000.0] {
                let hits = (0..10).filter(|&s| closing_trial(vc, payload, 100 + s)).count();
                println!("{payload:?} closing {vc:>8.0} km/s: {hits}/10");
            }
        }
    }

    #[test]
    fn missile_active_search_starts_before_terminal_steering() {
        let (mut w,c)=beam_trial();
        let pos=w.state(BodyId(0),0.0).unwrap().pos+Vec2::new(4.0*LIGHT_SECOND,0.0);
        w.bodies[1].trajectory=Trajectory::new(0.0,State {pos,vel:Vec2::ZERO});
        let tr=w.perceptions.get_mut(&FactionId(0)).unwrap().contacts.get_mut(&c).unwrap().track.as_mut().unwrap();
        tr.x[0]=pos.x;tr.x[1]=pos.y;
        let m=w.launch(BodyId(0),c,Payload::Kinetic).unwrap();
        w.bodies[m.0 as usize].sensors.passive=false;
        w.advance_to(12.0);
        assert!(w.bodies[m.0 as usize].missile.unwrap().active_seeker);
        assert_eq!(w.bodies[m.0 as usize].missile.unwrap().phase,Phase::Burn);
        assert!(w.ping_emissions.iter().any(|(id,_)|*id==m));
    }

    #[test]
    fn missile_early_active_search_relays_target_reports() {
        let (mut w,c)=range(1.0);
        // A known initial solution isolates seeker/relay timing from search-cone
        // misses caused by the separately tested Approximate-contact ellipse.
        let target=w.state(BodyId(2),w.time).unwrap();
        let origin=w.state(BodyId(0),w.time).unwrap().pos;
        w.perceptions.get_mut(&FactionId(0)).unwrap().ingest(Observation {detection:sensors::DetectionLevel::Resolved,
            contact:c,sensor:BodyId(0),origin,emitted_at:w.time,sensor_received_at:w.time,decider_received_at:w.time,
            source:Source::Echo,snr:1e12,measurement:Measurement::BearingRange {bearing:bearing_of(target.pos-origin),range:(target.pos-origin).length(),sigma_range:0.01,sigma_bearing:1e-10}},&w.system);
        let tr=w.perceptions.get_mut(&FactionId(0)).unwrap().contacts.get_mut(&c).unwrap().track.as_mut().unwrap();
        tr.x[2]=target.vel.x;tr.x[3]=target.vel.y;tr.x[4]=0.0;tr.x[5]=G0;
        let m=w.launch(BodyId(0),c,Payload::Kinetic).unwrap();
        assert_eq!(w.bodies[m.0 as usize].sensors,sensors::SensorSuite::MISSILE);
        w.bodies[m.0 as usize].sensors.passive=false;
        w.advance_to(61.0);
        assert!(w.ping_emissions.iter().any(|(id,_)|*id==m));
        w.bodies[m.0 as usize].missile.as_mut().unwrap().phase=Phase::Terminal;
        // Disable passive sensing so the next local fix must come from an echo.
        w.bodies[m.0 as usize].sensors.passive=false;
        w.guide_missile(m); // Run the phase change now, not at the previous burn cadence.
        w.advance_to(64.0);
        assert!(w.ping_emissions.iter().any(|(id,_)|*id==m));
        assert!(w.bodies[m.0 as usize].missile.unwrap().local_fix.is_none());
        w.advance_to(100.0);
        assert!(w.bodies[m.0 as usize].missile.unwrap().local_fix.is_some());
        let reports=&w.perception(FactionId(0)).unwrap().log;
        assert!(reports.iter().any(|o|o.sensor==m && o.contact==c && o.source==Source::Echo
            && o.decider_received_at>o.sensor_received_at && o.sensor_received_at>o.emitted_at));
        assert!(reports.iter().filter(|o|o.sensor==m).all(|o|matches!(o.measurement,Measurement::BearingRange {..})));
        w.bodies[m.0 as usize].sensors.active=false;
        assert!(!w.ping(m));
    }

    #[test]
    fn the_target_sees_the_missile_burn() {
        let (mut w, c) = range(1.0);
        let m = w.launch(BodyId(0), c, Payload::Kinetic).unwrap();
        w.advance_to(120.0);
        assert!(w.contact_truth(FactionId(1)).values().any(|b| *b == m), "missile drive detected");
    }

    #[test]
    fn uncertain_contacts_allow_missile_launch_and_bearing_search() {
        let (mut w,c)=beam_trial();
        w.time=TRACK_STALE_S.value+1.0;
        let m=w.launch(BodyId(0),c,Payload::Kinetic).expect("stale estimate is still launchable");
        assert!(w.bodies[m.0 as usize].missile.is_some());

        let (mut w,c)=beam_trial();
        w.perceptions.get_mut(&FactionId(0)).unwrap().contacts.get_mut(&c).unwrap().track=None;
        w.bodies[1].ship_class=Some(ShipClass::Battleship);
        w.snapshot_platform(BodyId(1),0.0);
        w.queue_launch(BodyId(0),c,Payload::Nuclear).expect("bearing-only contact is launchable");
        let m=BodyId((w.bodies.len()-1) as u32);
        w.bodies[m.0 as usize].sensors.passive=false;
        w.guide_missile(m);
        w.advance_to(MISSILE_RESPONSE_S.value+0.001);
        assert!(w.bodies[m.0 as usize].trajectory.last().thrust.length()>0.0,"search must fly down the received bearing");
        assert!(w.bodies[m.0 as usize].missile.unwrap().local_fix.is_none());
        w.advance_to(1.1); // Target is one light-second away; its first light must arrive.
        w.bodies[m.0 as usize].sensors.passive=true;
        w.guide_missile(m);
        assert!(w.bodies[m.0 as usize].missile.unwrap().local_fix.is_some(),"local seeker acquires without a datalink range");
        assert_eq!(w.bodies[m.0 as usize].missile.unwrap().phase,Phase::Burn,"acquisition must not cancel the departure boost");
    }

    #[test]
    fn nuclear_round_is_safe_beside_its_launcher() {
        let (mut w,c)=beam_trial();
        let m=w.launch(BodyId(0),c,Payload::Nuclear).unwrap();
        w.probability_flights.get_mut(&m).unwrap().due=0.0;
        w.guide_missile(m);
        assert!(w.hits.is_empty(),"unarmed warhead must not destroy its own launcher");
    }

    #[test]
    fn launching_needs_a_track_and_missiles() {
        let (mut w, c) = range(1.0);
        assert_eq!(w.launch(BodyId(0), ContactId(99), Payload::Kinetic), Err(OrderError::NoTrack));
        w.bodies[0].magazine = [0; 2];
        assert_eq!(w.launch(BodyId(0), c, Payload::Kinetic), Err(OrderError::EmptyMagazine));
    }

    #[test]
    fn rapid_clicks_queue_independent_srm_and_lrm_launchers() {
        let (mut w, c) = beam_trial();
        w.bodies[0].magazine = [10; 2];
        for p in [Payload::Kinetic, Payload::Nuclear, Payload::Nuclear, Payload::Kinetic] {
            w.queue_launch(BodyId(0), c, p).unwrap();
        }
        assert_eq!(w.bodies[0].magazine, [9, 9]);
        assert_eq!(w.bodies[0].missile_queued, [1, 1]);
        w.advance_to(4.999);
        assert_eq!(w.bodies.iter().filter(|b| b.kind == BodyKind::Missile).count(), 2);
        w.advance_to(60.0);
        let shots: Vec<_> = w.bodies.iter().filter_map(|b| b.missile.map(|m| (b.trajectory.start(), m.payload))).collect();
        assert_eq!(shots, vec![(0.0, Payload::Kinetic), (0.0, Payload::Nuclear), (5.0, Payload::Kinetic), (60.0, Payload::Nuclear)]);
        assert_eq!(w.bodies[0].magazine, [8, 8]);
        assert_eq!(w.bodies[0].missile_queued, [0; 2]);
    }

    #[test]
    fn queued_shots_reserve_ammo_without_consuming_other_types() {
        let (mut w, c) = beam_trial();
        w.bodies[0].magazine = [10; 2];
        assert_eq!(w.queue_launch(BodyId(0), ContactId(999), Payload::Nuclear), Err(OrderError::NoTrack));
        for _ in 0..10 { w.queue_launch(BodyId(0), c, Payload::Nuclear).unwrap(); }
        assert_eq!(w.queue_launch(BodyId(0), c, Payload::Nuclear), Err(OrderError::EmptyMagazine));
        assert_eq!(w.bodies[0].magazine, [10, 9]);
        assert_eq!(w.bodies[0].missile_queued, [0, 9]);
        w.queue_launch(BodyId(0), c, Payload::Kinetic).unwrap();
        assert_eq!(w.bodies[0].missile_queued, [0, 9]);
        // Lose the track before queued shots fire: reserved ammunition is released.
        for b in &mut w.bodies {b.sensors=sensors::SensorSuite {passive:false,active:false,direction_finding:false};}
        w.relays.clear();
        w.perceptions.get_mut(&FactionId(0)).unwrap().contacts.remove(&c);
        w.advance_to(MISSILE_LAUNCH_INTERVAL_S.value);
        assert_eq!(w.bodies[0].magazine, [9, 9]);
        assert_eq!(w.bodies[0].missile_queued, [0, 8]);
    }

    #[test]
    fn match_approaches_a_bearing_only_contact_but_flyby_requires_range() {
        let base = Vec2::new(2.0 * AU, 0.0);
        let specs = vec![
            ship("Alone", 0, base, Vec2::ZERO, Vec2::ZERO),
            ship("Burner", 1, base + Vec2::new(2.0*AU, 0.0), Vec2::ZERO, Vec2::new(0.0, 5.0 * G0)),
        ];
        let mut w = World::new(sun(), specs, 3600.0, 1);
        w.advance_to(1200.0); // Allow the 2 AU light-travel delay for drive emission.
        let c = *w.contact_truth(FactionId(0)).keys().next().unwrap();
        assert_eq!(w.set_intercept(BodyId(0), InterceptTarget::Contact(c)), Ok(()));
        assert!(w.bodies[0].trajectory.last().thrust.x>0.0,"MATCH approaches the bearing");
        assert_eq!(w.set_flyby(BodyId(0), InterceptTarget::Contact(c)), Err(OrderError::NoTrack));
    }

    #[test]
    fn a_lone_passive_ship_localizes_within_ef_scaled_range() {
        let base = Vec2::new(20.0 * AU, 0.0);
        let mut w = World::new(sun(), vec![
            ship("Observer", 0, base, Vec2::ZERO, Vec2::ZERO),
            ship("Burner", 1, base + Vec2::new(0.01 * AU, 0.0), Vec2::ZERO, Vec2::new(0.0, 10.0 * G0)),
        ], 600.0, 42);
        w.advance_to(600.0);
        let p = w.perception(FactionId(0)).unwrap();
        let track = p.contacts.values().next().unwrap().track.as_ref().unwrap();
        assert!(track.updates > 10);
        let truth = w.bodies[1].trajectory.state_at(track.t).unwrap();
        assert!((track.pos() - truth.pos).length() < 5.0 * track.pos_cov()[0][0].max(track.pos_cov()[1][1]).sqrt());
        assert!(p.log.iter().all(|o| o.source == Source::Emission));
    }

    #[test]
    fn active_ranging_and_exposure_arrive_at_light_speed() {
        let base = Vec2::new(20.0 * AU, 0.0);
        let mut w = World::new(sun(), vec![
            ship("Pinger", 0, base, Vec2::ZERO, Vec2::new(0.0, 10.0 * G0)),
            ship("Observer", 1, base + Vec2::new(0.01*AU, 0.0), Vec2::ZERO, Vec2::ZERO),
        ], 1200.0, 42);
        // Multiple returns avoid depending on a single probabilistic detection.
        for _ in 0..10 { w.ping(BodyId(0)); }
        w.advance_to(4.0);
        assert!(w.perception(FactionId(1)).unwrap().log.iter().all(|o| o.source != Source::Ping));
        w.advance_to(20.0);
        let observer = w.perception(FactionId(1)).unwrap();
        assert!(observer.log.iter().any(|o| o.source == Source::Ping && matches!(o.measurement, Measurement::Bearing { .. })),"signature {:?}, log {:?}",w.historical_signature(BodyId(0),0.0),observer.log);
        let ping=observer.log.iter().find(|o|o.source==Source::Ping).unwrap();
        let echo=w.perception(FactionId(0)).unwrap().log.iter().find(|o|o.source==Source::Echo).unwrap();
        assert!(ping.sensor_received_at>=0.01*AU/crate::units::C-0.01);
        assert!(echo.sensor_received_at>ping.sensor_received_at);
    }

    #[test]
    fn flyby_retains_closing_speed_and_can_switch_to_matching() {
        let base = Vec2::new(20.0 * AU, 0.0);
        let specs = vec![
            ship("Chaser", 0, base, Vec2::ZERO, Vec2::ZERO),
            ship("Target", 0, base + Vec2::new(1e6, 0.0), Vec2::ZERO, Vec2::ZERO),
        ];
        let mut w = World::new(sun(), specs, 60.0, 1);
        w.set_flyby(BodyId(0), InterceptTarget::Own(BodyId(1))).unwrap();
        w.advance_to(1600.0);
        assert_eq!(w.bodies[0].autopilot.unwrap().status, AutopilotStatus::Passed);
        let speed = w.state(BodyId(0), w.time()).unwrap().vel.length();
        assert!(speed > 1000.0, "flyby must not brake: {speed}");
        w.set_intercept(BodyId(0), InterceptTarget::Own(BodyId(1))).unwrap();
        w.advance_to(12000.0);
        assert_eq!(w.bodies[0].autopilot.unwrap().status, AutopilotStatus::Holding);
        assert!((w.state(BodyId(0), w.time()).unwrap().vel - w.state(BodyId(1), w.time()).unwrap().vel).length() < 1.0);
    }

    #[test]
    fn prehistory_arrives_near_the_requested_state() {
        let sys = sun();
        let pos = Vec2::new(AU, 0.0);
        let vel = Vec2::new(0.0, 29.78);
        let traj = ballistic_history(&sys, State { pos, vel }, Vec2::ZERO, 3600.0);
        let s = traj.state_at(0.0).unwrap();
        assert!((s.pos - pos).length() < 1.0, "{}", (s.pos - pos).length());
        assert!(traj.state_at(-3500.0).is_some());
    }

    #[test]
    fn burning_ship_is_triangulated_and_cold_ship_is_not_seen() {
        let base = Vec2::new(2.0 * AU, 0.0);
        let specs = vec![
            ship("A", 0, base, Vec2::ZERO, Vec2::ZERO),
            ship("B", 0, base + Vec2::new(0.0, 3.0 * LIGHT_SECOND), Vec2::ZERO, Vec2::ZERO),
            ship("Burner", 1, base + Vec2::new(30.0 * LIGHT_SECOND, 10.0 * LIGHT_SECOND), Vec2::ZERO, Vec2::new(0.0, 5.0 * G0)),
            ship("Cold", 1, base + Vec2::new(-2.0*AU, 0.0), Vec2::ZERO, Vec2::ZERO),
        ];
        let mut w = World::new(sun(), specs, 3600.0, 7);
        w.advance_to(600.0);
        let p = w.perception(FactionId(0)).unwrap();
        let truth = w.contact_truth(FactionId(0));
        assert_eq!(truth.len(), 1, "only the burner is detected");
        let (cid, _) = truth.iter().next().unwrap();
        let track = p.contacts[cid].track.as_ref().expect("two sensors triangulate");
        let t_e = track.t;
        let real = w.bodies[2].trajectory.state_at(t_e).unwrap().pos;
        let err = (track.pos() - real).length();
        let sigma = track.pos_cov()[0][0].max(track.pos_cov()[1][1]).sqrt();
        assert!(err < 5.0 * sigma + 1000.0, "err {err} sigma {sigma}");
        assert!(p.contacts[cid].last.emitted_at < 600.0 - 25.0, "seen light-delayed");
    }

    #[test]
    fn active_ping_ranges_a_cold_target_and_reveals_the_pinger() {
        let base = Vec2::new(2.0 * AU, 0.0);
        let specs = vec![
            ship("Pinger", 0, base, Vec2::ZERO, Vec2::ZERO),
            ship("Cold", 1, base + Vec2::new(3.0 * LIGHT_SECOND, 0.0), Vec2::ZERO, Vec2::ZERO),
        ];
        let mut w = World::new(sun(), specs, 3600.0, 3);
        // Cold ships at 3 ls are visible passively too; that is fine. Check echo use.
        w.ping(BodyId(0));
        w.advance_to(60.0);
        let echo = w.perception(FactionId(0)).unwrap().log.iter().any(|o| o.source == Source::Echo);
        let seen = w.perception(FactionId(1)).unwrap().log.iter().any(|o| o.source == Source::Ping);
        assert!(echo && !seen,"a cold pinger does not satisfy the direction activity rule");
        let p = w.perception(FactionId(0)).unwrap();
        let t = p.contacts.values().next().unwrap().track.as_ref().unwrap();
        let truth = w.state(BodyId(1), t.t).unwrap().pos;
        let sigma = t.pos_cov()[0][0].max(t.pos_cov()[1][1]).sqrt();
        assert!((t.pos() - truth).length() < 5.0 * sigma, "one pulse must yield a statistically consistent fix");
    }

    #[test]
    fn one_ping_resolves_inside_ef_envelope_and_not_outside() {
        for seed in 0..12 {
            // Cold unstealthed size-7 target: EF 0.7, active range 0.5 AU.
            for (range,resolves) in [(0.45*AU,true),(0.55*AU,false)] {
                let base=Vec2::new(20.0*AU,0.0);
                let mut w=World::new(sun(),vec![
                    ship("Pinger",0,base,Vec2::ZERO,Vec2::ZERO),
                    ship("Target",1,base+Vec2::new(range,0.0),Vec2::ZERO,Vec2::ZERO),
                ],1500.0,seed);
                w.bodies[0].sensors.passive=false;
                w.bodies[0].sensors.direction_finding=false;
                assert!(w.ping(BodyId(0)));
                w.advance_to(900.0);
                let p=w.perception(FactionId(0)).unwrap();
                let echoes:Vec<_>=p.log.iter().filter(|o|o.source==Source::Echo).collect();
                if resolves {
                    assert!(!echoes.is_empty(),"one inner-range ping must resolve, seed {seed}");
                    assert!(p.contacts.values().any(|c|c.resolved));
                    assert!(echoes.iter().all(|o|matches!(o.measurement,Measurement::BearingRange {..})));
                } else {
                    assert!(p.contacts.values().all(|c|!c.resolved));
                    assert!(echoes.iter().all(|o|matches!(o.measurement,Measurement::Bearing {..})));
                }
            }
        }
    }

    #[test]
    fn the_sun_blocks_the_view() {
        let specs = vec![
            ship("Watcher", 0, Vec2::new(-AU, 0.0), Vec2::ZERO, Vec2::ZERO),
            ship("Burner", 1, Vec2::new(AU, 0.0), Vec2::ZERO, Vec2::new(0.0, 50.0 * G0)),
        ];
        let mut w = World::new(sun(), specs, 3600.0, 3);
        w.advance_to(100.0);
        assert!(w.contact_truth(FactionId(0)).is_empty());
    }

    #[test]
    fn deterministic_across_warp() {
        let run = |chunks: usize| {
            let base = Vec2::new(2.0 * AU, 0.0);
            let specs = vec![
                ship("A", 0, base, Vec2::ZERO, Vec2::ZERO),
                ship("B", 0, base + Vec2::new(0.0, 3.0 * LIGHT_SECOND), Vec2::ZERO, Vec2::ZERO),
                ship("Burner", 1, base + Vec2::new(60.0 * LIGHT_SECOND, 20.0 * LIGHT_SECOND), Vec2::ZERO, Vec2::new(0.0, 5.0 * G0)),
            ];
            let mut w = World::new(sun(), specs, 3600.0, 9);
            for i in 1..=chunks {
                w.advance_to(1200.0 * i as f64 / chunks as f64);
            }
            let p = w.perception(FactionId(0)).unwrap();
            p.contacts.values().next().and_then(|c| c.track.as_ref()).map(|t| t.x)
        };
        assert_eq!(run(1), run(97));
    }
}
