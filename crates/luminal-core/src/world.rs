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

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FactionId(pub u8);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BodyId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyKind {
    Ship,
    Probe,
    Missile,
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
    /// Fly to a point in minimal time and stop there. The point is `offset` from
    /// celestial `frame` and moves with it.
    MoveTo { frame: usize, offset: Vec2 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AutopilotStatus {
    Manoeuvring,
    Closing { eta: f64, range: f64 },
    Holding,
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
    pub name: String,
    pub kind: BodyKind,
    pub faction: FactionId,
    pub trajectory: Trajectory,
    /// Pinging once per sensor frame.
    pub active_sensor: bool,
    /// Manual thrust order, used when no autopilot order is active.
    pub commanded: Vec2,
    pub autopilot: Option<Autopilot>,
    /// Result of the last collision check.
    pub avoidance: Avoidance,
    /// Cap on autopilot thrust, km/s². Lower is quieter: drive emission scales with it.
    pub drive_limit: f64,
    /// Missiles left to launch.
    pub magazine: u32,
    /// Present on missiles.
    pub missile: Option<MissileState>,
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
    pub seeker: Option<Seeker>,
    pub last_guide: f64,
    pub(crate) target_body: BodyId,
}

impl Body {
    pub fn alive_at(&self, t: f64) -> bool {
        self.trajectory.state_at(t).is_some()
    }

    pub fn max_accel(&self) -> f64 {
        crate::units::G0
            * match self.kind {
                BodyKind::Ship => SHIP_MAX_ACCEL_G.value,
                BodyKind::Probe => PROBE_MAX_ACCEL_G.value,
                BodyKind::Missile => MISSILE_MAX_ACCEL_G.value,
            }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderError {
    Destroyed,
    NoTrack,
    InvalidTarget,
    EmptyMagazine,
}

/// How a body came to an end. Truth only.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LossCause {
    /// Hit the surface of a celestial body (index into `System::bodies`).
    Impact(usize),
    /// Killed by a missile's payload.
    Missile { payload: Payload, missile: BodyId },
    /// A missile that fired, detonated or missed and is spent.
    Expended,
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
/// from that faction's own information (own losses are a PLACEHOLDER: known at once).
#[derive(Clone, Debug, PartialEq)]
pub enum AlertKind {
    NewContact(ContactId),
    ShipLost(BodyId),
    CollisionWarning(BodyId),
    CollisionUnavoidable(BodyId),
    OrderComplete(BodyId),
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
    pub magazine: u32,
}

#[derive(Clone, Copy, Debug)]
enum Event {
    Step(BodyId),
    SensorFrame,
    MissileGuide(BodyId),
    /// A laser missile's beam, resolved when its light reaches the target.
    Beam(Beam),
}

#[derive(Clone, Copy, Debug)]
struct Beam {
    missile: BodyId,
    target: BodyId,
    front: Front,
    direction: Vec2,
}

#[derive(Clone, Debug)]
struct Ping {
    emitter: BodyId,
    front: Front,
    pending: Vec<BodyId>,
}

#[derive(Clone, Debug)]
struct Echo {
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
    time: f64,
    pub system: System,
    pub bodies: Vec<Body>,
    pub losses: Vec<Loss>,
    pub objective: Option<Objective>,
    pub outcome: Option<Outcome>,
    pub alerts: Vec<Alert>,
    scheduler: Scheduler<Event>,
    last_step: Vec<f64>,
    last_frame: f64,
    pings: Vec<Ping>,
    echoes: Vec<Echo>,
    relays: Vec<Relay>,
    perceptions: BTreeMap<FactionId, Perception>,
    /// Truth-side association of bodies to each faction's contact ids. PLACEHOLDER:
    /// perfect association; real data association is not modelled.
    association: BTreeMap<(FactionId, BodyId), ContactId>,
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
                    name: spec.name,
                    kind: spec.kind,
                    faction: spec.faction,
                    trajectory,
                    active_sensor: false,
                    commanded: spec.thrust,
                    autopilot: None,
                    avoidance: Avoidance { thrust: spec.thrust, active: false, impossible: false },
                    drive_limit: f64::INFINITY,
                    magazine: spec.magazine,
                    missile: None,
                }
            })
            .collect();
        let mut scheduler = Scheduler::default();
        for i in 0..bodies.len() {
            scheduler.schedule(0.0, Event::Step(BodyId(i as u32)));
        }
        scheduler.schedule(0.0, Event::SensorFrame);
        let perceptions = bodies.iter().map(|b| (b.faction, Perception::new(b.faction))).collect();
        let mut world = Self {
            time: 0.0,
            system,
            last_step: vec![0.0; bodies.len()],
            bodies,
            losses: vec![],
            objective: None,
            outcome: None,
            alerts: vec![],
            scheduler,
            last_frame: -SENSOR_FRAME_S.value,
            pings: vec![],
            echoes: vec![],
            relays: vec![],
            perceptions,
            association: BTreeMap::new(),
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

    /// The ship that receives and decides for a faction: its lowest-numbered live ship.
    pub fn decider(&self, f: FactionId, t: f64) -> Option<BodyId> {
        self.bodies
            .iter()
            .enumerate()
            .find(|(_, b)| b.faction == f && b.kind == BodyKind::Ship && b.alive_at(t))
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
                let has_track = self.perceptions.get(&faction).and_then(|p| p.contacts.get(&c)).is_some_and(|c| c.track.is_some());
                if !has_track {
                    return Err(OrderError::NoTrack);
                }
            }
        }
        let order = Order::Intercept(target);
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
    pub fn launch(&mut self, id: BodyId, target: ContactId, payload: Payload) -> Result<BodyId, OrderError> {
        let t = self.time;
        let b = self.live_body_mut(id)?;
        if b.kind != BodyKind::Ship {
            return Err(OrderError::InvalidTarget);
        }
        if b.magazine == 0 {
            return Err(OrderError::EmptyMagazine);
        }
        let faction = b.faction;
        let has_track = self.perceptions.get(&faction).and_then(|p| p.contacts.get(&target)).is_some_and(|c| c.track.is_some());
        if !has_track {
            return Err(OrderError::NoTrack);
        }
        let target_body = *self.contact_truth(faction).get(&target).ok_or(OrderError::InvalidTarget)?;

        let fired = self.bodies.iter().filter(|m| m.missile.is_some_and(|ms| ms.launcher == id)).count() + 1;
        let b = &mut self.bodies[id.0 as usize];
        b.magazine -= 1;
        let name = format!("{} M{fired}", b.name);
        let start = b.trajectory.state_at(t).expect("alive");
        let dv = MISSILE_DELTA_V_KMS.value;
        let mid = BodyId(self.bodies.len() as u32);
        self.bodies.push(Body {
            name,
            kind: BodyKind::Missile,
            faction,
            trajectory: Trajectory::new(t, start),
            active_sensor: false,
            commanded: Vec2::ZERO,
            autopilot: None,
            avoidance: Avoidance { thrust: Vec2::ZERO, active: false, impossible: false },
            drive_limit: f64::INFINITY,
            magazine: 0,
            missile: Some(MissileState {
                payload,
                target,
                launcher: id,
                phase: Phase::Burn,
                dv_left: dv,
                burn_left: dv * MISSILE_BURN_FRACTION.value,
                seeker: None,
                last_guide: t,
                target_body,
            }),
        });
        self.last_step.push(t);
        self.scheduler.schedule(t, Event::Step(mid));
        self.scheduler.schedule(t, Event::MissileGuide(mid));
        Ok(mid)
    }

    /// One missile guidance cycle: account fuel, check for a hit since the last cycle,
    /// sense, pick the phase, fire or steer, and schedule the next cycle.
    fn guide_missile(&mut self, id: BodyId) {
        let t = self.time;
        let i = id.0 as usize;
        let Some(me) = self.bodies[i].trajectory.state_at(t) else { return };
        let Some(mut ms) = self.bodies[i].missile else { return };
        let faction = self.bodies[i].faction;
        let max_accel = self.bodies[i].max_accel();
        let target_traj = self.bodies[ms.target_body.0 as usize].trajectory.clone();

        // Fuel spent since the last cycle.
        let spent = self.bodies[i].trajectory.last().thrust.length() * (t - ms.last_guide);
        ms.dv_left = (ms.dv_left - spent).max(0.0);
        if ms.phase == Phase::Burn {
            ms.burn_left = (ms.burn_left - spent).max(0.0);
        }

        // Did the last interval bring us close enough? Resolved against truth.
        let missile_traj = &self.bodies[i].trajectory;
        let sep = |tau: f64| Some(target_traj.state_at(tau)?.pos - missile_traj.state_at(tau)?.pos);
        if t > ms.last_guide
            && let Some((tc, d)) = missile::closest_approach(ms.last_guide, t, sep)
        {
            let lethal = match ms.payload {
                Payload::Kinetic => SHIP_RADIUS_KM.value,
                Payload::Nuclear => NUCLEAR_LETHAL_KM.value,
                Payload::Laser => 0.0,
            };
            if d < lethal {
                self.destroy(ms.target_body, tc, LossCause::Missile { payload: ms.payload, missile: id });
                self.destroy(id, tc, LossCause::Expended);
                return;
            }
        }
        ms.last_guide = t;

        // What the missile knows: the faction track (datalink) and its own seeker.
        let track = self
            .perceptions
            .get(&faction)
            .and_then(|p| p.contacts.get(&ms.target))
            .and_then(|c| c.track.as_ref())
            .map(|tr| {
                let now = tr.at(t, &self.system);
                (State { pos: now.pos(), vel: now.vel() }, now.accel())
            });
        if let Some((t_e, src)) = retarded_state(&target_traj, me.pos, t)
            && self.system.occluder(src.pos, t_e, me.pos, t).is_none()
        {
            let kind = self.bodies[ms.target_body.0 as usize].kind;
            let thrust = target_traj.thrust_at(t_e).unwrap_or(Vec2::ZERO);
            let snr = sensors::intensity(emission_w(kind, thrust), (src.pos - me.pos).length()) / MISSILE_SEEKER_NOISE_FLOOR.value;
            if snr >= PASSIVE_DETECT_SNR.value {
                let (bearing, _) = sensors::measure_bearing(bearing_of(src.pos - me.pos), snr, &mut self.rng);
                ms.seeker = Some(Seeker::update(ms.seeker, t, bearing));
            }
        }

        let Some((est, est_accel)) = track else {
            // Nothing to steer by: coast and look again soon.
            self.set_missile(i, ms, Vec2::ZERO, 1.0);
            return;
        };
        let (t_go, zem) = missile::zero_effort_miss(me, est, est_accel);
        let rel = est.pos - me.pos;
        let range = rel.length();
        let closing = -(est.vel - me.vel).dot(rel.normalized());

        // Passed the target: spent.
        if t_go <= 0.0 && closing < 0.0 && ms.phase != Phase::Burn {
            self.destroy(id, t, LossCause::Expended);
            return;
        }

        if ms.phase == Phase::Burn && ms.burn_left <= 1e-9 {
            ms.phase = Phase::Cruise;
        }
        if ms.phase != Phase::Terminal && t_go < MISSILE_TERMINAL_S.value && (ms.phase == Phase::Cruise || closing > 0.0) {
            ms.phase = Phase::Terminal;
        }

        if ms.payload == Payload::Laser && range < LASER_STANDOFF_KM.value && closing > 0.0 {
            // Aim where the target will be when the beam arrives. The seeker's image is
            // one light-time old and the beam takes another; once emitted the beam does
            // not share the missile's motion, so lead by the target's own velocity.
            let delay = 2.0 * range / crate::units::C;
            let image = match ms.seeker.filter(|s| s.t == t) {
                Some(s) => me.pos + Vec2::new(s.bearing.cos(), s.bearing.sin()) * range,
                None => est.pos - est.vel * (0.5 * delay),
            };
            let aim = image + est.vel * delay + est_accel * (0.5 * delay * delay);
            let direction = (aim - me.pos).normalized();
            let front = Front { origin: me.pos, t_emit: t };
            self.scheduler.schedule(t + delay, Event::Beam(Beam { missile: id, target: ms.target_body, front, direction }));
            self.destroy(id, t, LossCause::Expended);
            return;
        }

        let reserve = MISSILE_DELTA_V_KMS.value * MISSILE_RESERVE_FRACTION.value;
        let (cmd, budget, dt_next) = match ms.phase {
            Phase::Burn => {
                let dir = missile::intercept_aim(me, est, est_accel, max_accel, 1e6).map_or(rel.normalized(), |(d, _)| d);
                (dir * max_accel, ms.burn_left, (ms.burn_left / max_accel).clamp(0.01, SENSOR_FRAME_S.value))
            }
            Phase::Cruise => {
                let a = missile::cruise_correction(t_go, zem);
                let a = if a.length() < 0.1 * crate::units::G0 { Vec2::ZERO } else { a };
                (a, (ms.dv_left - reserve).max(0.0), ((t_go - MISSILE_TERMINAL_S.value) / 2.0).clamp(0.5, SENSOR_FRAME_S.value))
            }
            Phase::Terminal => {
                let a = match ms.seeker.filter(|s| s.t == t && s.samples >= 2) {
                    Some(s) => missile::pn_accel(&s, closing),
                    None => missile::cruise_correction(t_go, zem),
                };
                (a, ms.dv_left, (t_go / 40.0).clamp(0.002, 1.0))
            }
        };
        // Never command more than the drive or the remaining budget allows.
        let mut thrust = cmd;
        let cap = max_accel.min(budget / dt_next);
        if thrust.length() > cap {
            thrust = thrust.normalized() * cap;
        }
        self.set_missile(i, ms, thrust, dt_next);
    }

    fn set_missile(&mut self, i: usize, ms: MissileState, thrust: Vec2, dt_next: f64) {
        let t = self.time;
        let b = &mut self.bodies[i];
        b.missile = Some(ms);
        if b.trajectory.last().thrust != thrust {
            b.trajectory.set_thrust(t, thrust).expect("guidance applies at current time");
        }
        self.scheduler.schedule(t + dt_next, Event::MissileGuide(BodyId(i as u32)));
    }

    /// A laser beam's light has had time to reach its target: did the ray pass close
    /// enough to where the target really was when the light got there?
    fn resolve_beam(&mut self, beam: Beam) {
        let traj = self.bodies[beam.target.0 as usize].trajectory.clone();
        let Some(t_arr) = beam.front.arrival(&traj, beam.front.t_emit, self.time) else { return };
        let Some(s) = traj.state_at(t_arr) else { return };
        let rel = s.pos - beam.front.origin;
        let along = rel.dot(beam.direction);
        let miss = (rel - beam.direction * along).length();
        let blocked = self.system.occluder(beam.front.origin, beam.front.t_emit, s.pos, t_arr).is_some();
        if along > 0.0 && miss < SHIP_RADIUS_KM.value && !blocked {
            self.destroy(beam.target, t_arr, LossCause::Missile { payload: Payload::Laser, missile: beam.missile });
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
        if b.missile.is_some() {
            return;
        }
        let Some(s) = b.trajectory.state_at(t) else { return };
        let max_accel = b.max_accel();
        let limit = max_accel.min(b.drive_limit);
        let (desired, status) = match b.autopilot.map(|a| a.order) {
            None => (b.commanded, None),
            Some(Order::Orbit { celestial, radius, sense }) => {
                let thrust = autopilot::orbit_thrust(&self.system, celestial, s, t, radius, sense, limit);
                let c = self.system.state(celestial, t);
                let rel = s.pos - c.pos;
                let settled = ((rel.length() - radius) / radius).abs() < 0.02 && thrust.length() < 0.01 * crate::units::G0;
                (thrust, Some(if settled { AutopilotStatus::Holding } else { AutopilotStatus::Manoeuvring }))
            }
            Some(Order::Intercept(target)) => {
                let known = match target {
                    InterceptTarget::Own(o) => self.bodies[o.0 as usize]
                        .trajectory
                        .state_at(t)
                        .map(|ts| (ts, self.bodies[o.0 as usize].trajectory.thrust_at(t).unwrap_or(Vec2::ZERO))),
                    InterceptTarget::Contact(c) => self
                        .perceptions
                        .get(&b.faction)
                        .and_then(|p| p.contacts.get(&c))
                        .and_then(|c| c.track.as_ref())
                        .map(|tr| {
                            let now = tr.at(t, &self.system);
                            (State { pos: now.pos(), vel: now.vel() }, now.accel())
                        }),
                };
                match known {
                    None => (Vec2::ZERO, Some(AutopilotStatus::NoTrack)),
                    Some((ts, ta)) => {
                        let r = autopilot::rendezvous(s, ts, ta, limit);
                        let holding = r.gap < 0.5 * autopilot::STANDOFF_KM && r.rel_speed < 1.0;
                        let status = if holding { AutopilotStatus::Holding } else { AutopilotStatus::Closing { eta: r.eta, range: r.gap } };
                        (r.thrust, Some(status))
                    }
                }
            }
            Some(Order::MoveTo { frame, offset }) => {
                let f = self.system.state(frame, t);
                let dest = State { pos: f.pos + offset, vel: f.vel };
                // Keep pace with the frame and cancel local gravity.
                let ff = self.system.accel(frame, t) - self.system.gravity(s.pos, t);
                let m = autopilot::move_to(s, dest, ff, limit);
                let holding = m.gap < 10.0 && m.rel_speed < 0.1;
                let status = if holding { AutopilotStatus::Holding } else { AutopilotStatus::Closing { eta: m.eta, range: m.gap } };
                (m.thrust, Some(status))
            }
        };
        // The orbit law steers to a safe radius by construction; everything else is
        // checked against every surface.
        let orbiting = matches!(b.autopilot.map(|a| a.order), Some(Order::Orbit { .. }));
        let avoidance = if orbiting {
            Avoidance { thrust: desired, active: false, impossible: false }
        } else {
            autopilot::avoid(&self.system, s, t, desired, max_accel)
        };
        let b = &mut self.bodies[i];
        let faction = b.faction;
        let mut raised = vec![];
        if let (Some(a), Some(st)) = (&mut b.autopilot, status) {
            if st == AutopilotStatus::Holding && a.status != AutopilotStatus::Holding {
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

    pub fn set_active_sensor(&mut self, id: BodyId, on: bool) -> bool {
        let t = self.time;
        match self.bodies.get_mut(id.0 as usize) {
            Some(b) if b.alive_at(t) => {
                b.active_sensor = on;
                true
            }
            _ => false,
        }
    }

    /// Process every event up to `t`, then set the clock to `t`.
    pub fn advance_to(&mut self, t: f64) {
        self.advance_until_alert(t, None);
    }

    /// As `advance_to`, but stop at the first event that raises an alert for `watch`
    /// (or for everyone). Returns that alert; the clock then stands at its time.
    pub fn advance_until_alert(&mut self, t: f64, watch: Option<FactionId>) -> Option<Alert> {
        while let Some((te, ev)) = self.scheduler.pop_due(t) {
            self.time = te.max(self.time);
            let seen = self.alerts.len();
            match ev {
                Event::Step(id) => self.step(id),
                Event::SensorFrame => {
                    self.sensor_frame();
                    self.scheduler.schedule(self.time + SENSOR_FRAME_S.value, Event::SensorFrame);
                }
                Event::MissileGuide(id) => self.guide_missile(id),
                Event::Beam(beam) => self.resolve_beam(beam),
            }
            if let Some(w) = watch
                && let Some(a) = self.alerts[seen..].iter().find(|a| a.faction.is_none_or(|f| f == w))
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

    /// A body stops existing at `t`.
    fn destroy(&mut self, id: BodyId, t: f64, cause: LossCause) {
        let b = &mut self.bodies[id.0 as usize];
        b.trajectory.terminate(t);
        b.active_sensor = false;
        b.autopilot = None;
        let (faction, name) = (b.faction, b.name.clone());
        self.losses.push(Loss { body: id, t, cause });
        if cause != LossCause::Expended {
            self.alert(Some(faction), AlertKind::ShipLost(id));
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
        let next = ContactId(self.association.keys().filter(|(ff, _)| *ff == f).count() as u32 + 1);
        *self.association.entry((f, body)).or_insert(next)
    }

    fn state(&self, id: BodyId, t: f64) -> Option<State> {
        self.bodies[id.0 as usize].trajectory.state_at(t)
    }

    fn sensor_frame(&mut self) {
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
                self.echoes.push(Echo { emitter: ping.emitter, front: Front { origin: rx, t_emit: t_hit }, out_km });
                if self.bodies[target.0 as usize].kind == BodyKind::Ship {
                    let snr = sensors::intensity(ACTIVE_PING_POWER_W.value, out_km) / PASSIVE_NOISE_FLOOR.value;
                    if snr >= PASSIVE_DETECT_SNR.value {
                        let (bearing, sigma) =
                            sensors::measure_bearing(bearing_of(ping.front.origin - rx), snr, &mut self.rng);
                        reports.push(Observation {
                            contact: self.contact_id(target_faction, ping.emitter),
                            sensor: target,
                            origin: rx,
                            emitted_at: ping.front.t_emit,
                            sensor_received_at: t_hit,
                            decider_received_at: f64::NAN,
                            measurement: Measurement::Bearing { bearing, sigma },
                            snr,
                            source: Source::Ping,
                        });
                    }
                }
            }
            ping.pending = still;
        }
        pings.retain(|p| !p.pending.is_empty() && p.front.radius_at(t) < MAX_FRONT_RADIUS_KM);
        self.pings = pings;

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
            let snr = sensors::echo_intensity(ACTIVE_PING_POWER_W.value, echo.out_km, back_km) / ACTIVE_NOISE_FLOOR.value;
            if snr < PASSIVE_DETECT_SNR.value {
                continue;
            }
            // Which body reflected it is truth; the report carries only the contact id.
            let faction = self.bodies[echo.emitter.0 as usize].faction;
            let Some(target) = self.body_at(echo.front.origin, echo.front.t_emit) else { continue };
            let m = sensors::measure_echo(bearing_of(echo.front.origin - rx), back_km, snr, &mut self.rng);
            reports.push(Observation {
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
        for si in 0..self.bodies.len() {
            let sensor = BodyId(si as u32);
            let s = &self.bodies[si];
            if s.kind != BodyKind::Ship {
                continue;
            }
            let Some(me) = s.trajectory.state_at(t) else { continue };
            let faction = s.faction;
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
                let snr = sensors::intensity(emission_w(b.kind, thrust), range) / PASSIVE_NOISE_FLOOR.value;
                if snr < PASSIVE_DETECT_SNR.value {
                    continue;
                }
                let (bearing, sigma) = sensors::measure_bearing(bearing_of(src.pos - me.pos), snr, &mut self.rng);
                reports.push(Observation {
                    contact: self.contact_id(faction, BodyId(ti as u32)),
                    sensor,
                    origin: me.pos,
                    emitted_at: t_e,
                    sensor_received_at: t,
                    decider_received_at: f64::NAN,
                    measurement: Measurement::Bearing { bearing, sigma },
                    snr,
                    source: Source::Emission,
                });
            }
        }

        // New pings leave active ships now.
        for i in 0..self.bodies.len() {
            let b = &self.bodies[i];
            if !b.active_sensor {
                continue;
            }
            let Some(s) = b.trajectory.state_at(t) else { continue };
            let pending = (0..self.bodies.len()).filter(|&j| j != i).map(|j| BodyId(j as u32)).collect();
            self.pings.push(Ping { emitter: BodyId(i as u32), front: Front { origin: s.pos, t_emit: t }, pending });
        }

        // Route reports to each faction's decider: directly, or by laser relay at c.
        for obs in reports {
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
        for obs in delivered {
            let faction = self.bodies[obs.sensor.0 as usize].faction;
            let sys = &self.system;
            if let Some(p) = self.perceptions.get_mut(&faction) {
                let new = !p.contacts.contains_key(&obs.contact);
                p.ingest(obs, sys);
                if new {
                    self.alerts.push(Alert { t, faction: Some(faction), kind: AlertKind::NewContact(obs.contact) });
                }
            }
        }

        // Orders and collision checks re-plan on the fresh picture. PLACEHOLDER: every
        // ship uses its faction's flagship perception without relay delay.
        for i in 0..self.bodies.len() {
            self.guide(BodyId(i as u32));
        }
    }

    /// The body at `pos` at time `t` (the reflector of an echo).
    fn body_at(&self, pos: Vec2, t: f64) -> Option<BodyId> {
        self.bodies
            .iter()
            .enumerate()
            .filter_map(|(i, b)| b.trajectory.state_at(t).map(|s| (i, (s.pos - pos).length())))
            .filter(|(_, d)| *d < 1.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| BodyId(i as u32))
    }
}

/// Isotropic emission of a body of `kind` thrusting at `thrust`.
fn emission_w(kind: BodyKind, thrust: Vec2) -> f64 {
    match kind {
        BodyKind::Missile => sensors::missile_emission_w(thrust),
        BodyKind::Ship | BodyKind::Probe => sensors::ship_emission_w(thrust),
    }
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
        s = State { pos: s.pos - s.vel * dt + a * (0.5 * dt * dt), vel: s.vel - a * dt };
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
    use crate::missile::Payload;
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
    fn move_order_stops_at_a_point_near_the_planet_and_holds() {
        let sys = crate::scenario::home_system();
        let p = sys.state(1, 0.0);
        let s = ship("Mover", 0, p.pos + Vec2::new(60_000.0, 0.0), p.vel + Vec2::new(4.0, 0.0), Vec2::ZERO);
        let mut w = World::new(sys, vec![s], 0.0, 1);
        let target = p.pos + Vec2::new(-200_000.0, 150_000.0);
        w.set_move(BodyId(0), target).unwrap();
        assert!(matches!(w.bodies[0].autopilot.unwrap().order, Order::MoveTo { frame: 1, .. }));
        w.advance_to(3.0 * 3600.0);
        let t = w.time();
        let pl = w.system.state(1, t);
        let st = w.bodies[0].trajectory.state_at(t).unwrap();
        let want = pl.pos + Vec2::new(-200_000.0, 150_000.0);
        assert!((st.pos - want).length() < 20.0, "off by {} km", (st.pos - want).length());
        assert!((st.vel - pl.vel).length() < 0.2, "at rest in the planet frame");
        assert_eq!(w.bodies[0].autopilot.unwrap().status, AutopilotStatus::Holding);
        assert!(w.losses.is_empty());
    }

    #[test]
    fn move_order_goes_around_the_planet_not_through_it() {
        let sys = crate::scenario::home_system();
        let p = sys.state(1, 0.0);
        let s = ship("Mover", 0, p.pos + Vec2::new(80_000.0, 0.0), p.vel, Vec2::ZERO);
        let mut w = World::new(sys, vec![s], 0.0, 1);
        w.set_move(BodyId(0), p.pos + Vec2::new(-80_000.0, 0.0)).unwrap();
        w.advance_to(3.0 * 3600.0);
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
                ship("Hunter", 1, base + Vec2::new(0.0, 0.2 * AU), Vec2::ZERO, Vec2::ZERO),
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
            // A cold ship that lights its drive at T+600; its light arrives ~100 s later.
            ship("Sleeper", 1, base + Vec2::new(100.0 * LIGHT_SECOND, 0.0), Vec2::ZERO, Vec2::ZERO),
        ];
        let mut w = World::new(sun(), specs, 3600.0, 1);
        assert!(w.advance_until_alert(600.0, Some(FactionId(0))).is_none());
        w.bodies[1].trajectory.set_thrust(600.0, Vec2::new(0.0, 20.0 * G0)).unwrap();
        let a = w.advance_until_alert(3600.0, Some(FactionId(0))).expect("noticed");
        assert!(matches!(a.kind, AlertKind::NewContact(_)));
        assert!(a.t >= 700.0 && a.t < 720.0, "when the light arrived: {}", a.t);
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

    #[test]
    fn each_payload_kills_an_unaware_target() {
        for payload in Payload::ALL {
            let (mut w, c) = range(1.0);
            let m = w.launch(BodyId(0), c, payload).unwrap();
            assert_eq!(w.bodies[0].magazine, 3);
            w.advance_to(8.0 * 3600.0);
            let loss = w.losses.iter().find(|l| l.body == BodyId(2));
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

    #[test]
    fn the_target_sees_the_missile_burn() {
        let (mut w, c) = range(1.0);
        let m = w.launch(BodyId(0), c, Payload::Kinetic).unwrap();
        w.advance_to(120.0);
        assert!(w.contact_truth(FactionId(1)).values().any(|b| *b == m), "missile drive detected");
    }

    #[test]
    fn launching_needs_a_track_and_missiles() {
        let (mut w, c) = range(1.0);
        assert_eq!(w.launch(BodyId(0), ContactId(99), Payload::Kinetic), Err(OrderError::NoTrack));
        w.bodies[0].magazine = 0;
        assert_eq!(w.launch(BodyId(0), c, Payload::Kinetic), Err(OrderError::EmptyMagazine));
    }

    #[test]
    fn cannot_intercept_a_bearing_only_contact() {
        let base = Vec2::new(2.0 * AU, 0.0);
        let specs = vec![
            ship("Alone", 0, base, Vec2::ZERO, Vec2::ZERO),
            ship("Burner", 1, base + Vec2::new(60.0 * LIGHT_SECOND, 0.0), Vec2::ZERO, Vec2::new(0.0, 5.0 * G0)),
        ];
        let mut w = World::new(sun(), specs, 3600.0, 1);
        w.advance_to(100.0);
        let c = *w.contact_truth(FactionId(0)).keys().next().unwrap();
        assert_eq!(w.set_intercept(BodyId(0), InterceptTarget::Contact(c)), Err(OrderError::NoTrack));
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
            ship("Burner", 1, base + Vec2::new(60.0 * LIGHT_SECOND, 20.0 * LIGHT_SECOND), Vec2::ZERO, Vec2::new(0.0, 5.0 * G0)),
            ship("Cold", 1, base + Vec2::new(-80.0 * LIGHT_SECOND, 0.0), Vec2::ZERO, Vec2::ZERO),
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
        assert!(p.contacts[cid].last.emitted_at < 600.0 - 50.0, "seen light-delayed");
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
        w.set_active_sensor(BodyId(0), true);
        w.advance_to(60.0);
        let echo = w.perception(FactionId(0)).unwrap().log.iter().any(|o| o.source == Source::Echo);
        let seen = w.perception(FactionId(1)).unwrap().log.iter().any(|o| o.source == Source::Ping);
        assert!(echo && seen);
        let p = w.perception(FactionId(0)).unwrap();
        let t = p.contacts.values().next().unwrap().track.as_ref().unwrap();
        assert!((t.pos() - (base + Vec2::new(3.0 * LIGHT_SECOND, 0.0))).length() < 50.0);
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
