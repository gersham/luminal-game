//! The only interface a client (local UI, remote player, bot) has to a running game.
//!
//! A session owns world truth. Clients send `Command`s and receive `View`s built for
//! their `Role`. A faction view is built from that faction's `Perception` only, so it
//! is safe to send over a network; only the spectator role ever receives truth.

#[path = "event_wait.rs"]
mod event_wait;
use crate::celestial::{CelestialKind, System};
use crate::kinematics::Vec2;
use crate::mind::{ContactId, Measurement, Source};
pub use crate::missile::{Payload, Phase};
use crate::params;
use crate::units::G0;
use crate::autopilot::Avoidance;
use crate::world::{Alert, AlertKind, Autopilot, BodyKind, FactionId, LossCause, Objective, OrderError, Outcome, World};
use std::collections::{BTreeMap, VecDeque};

pub use crate::world::{AutopilotStatus, BodyId, InterceptTarget, Order};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Faction(FactionId),
    /// Omniscient. For local exploration, AI-versus-AI and replays; never granted to a
    /// competitive network player.
    Spectator,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    SetBeamMode {body:BodyId,mode:crate::world::weapon_fit::BeamMode},
    Withdraw {body:BodyId},
    Surrender {body:BodyId},
    SetRepairGoal {body:BodyId,goal:crate::damage::RepairGoal},
    Jump {body:BodyId,destination:Vec2},
    CancelJump {body:BodyId},
    /// Constant thrust from now on, km/s². Gravity acts in addition. Cancels any
    /// autopilot order.
    SetThrust { body: BodyId, thrust: Vec2 },
    /// Settle into a convenient orbit about celestial body `celestial` (index).
    Orbit { body: BodyId, celestial: usize },
    /// Close on a target, match velocity and hold station.
    Intercept { body: BodyId, target: InterceptTarget },
    Follow { body: BodyId, target: BodyId },
    Alongside { body: BodyId, target: InterceptTarget },
    /// Close at maximum thrust and fly through, retaining velocity.
    Flyby { body: BodyId, target: InterceptTarget },
    KeepRange {body:BodyId,target:InterceptTarget,range:f64},
    CombatRange {body:BodyId,target:InterceptTarget,standoff:bool},
    Evade {body:BodyId,target:InterceptTarget},
    /// Fly to a point in minimal time (burn, flip, brake) and stop there.
    MoveTo { body: BodyId, point: Vec2 },
    AppendWaypoint {body:BodyId,point:Vec2},
    /// Come to rest in the local frame as fast as the drive allows.
    AllStop { body: BodyId },
    /// Cap autopilot thrust, in g. Lower thrust means a fainter drive signature.
    SetDriveLimit { body: BodyId, g: f64 },
    /// Launch a missile at a tracked contact.
    Launch { body: BodyId, target: ContactId, payload: Payload },
    CancelLaunches { body: BodyId },
    DeployProbe { body: BodyId, direction: Vec2 },
    FireBeam { body: BodyId, target: ContactId },
    /// Emit one pulse. Pings are visible far beyond their echo range.
    Ping { body: BodyId },
    EngageBeam { body: BodyId, target: Option<ContactId> },
    ArmBeams { body: BodyId },
    /// Request gradual screen buildup or energy-conserving collapse.
    SetScreen { body: BodyId, up: bool },
    SetHeatDump {body:BodyId,enabled:bool},
    SetSystemMode {body:BodyId,system:crate::world::controls::ControlledSystem,mode:crate::world::controls::Mode},
    SetWarp(f64),
    SetPaused(bool),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Rejection {
    JumpUnavailable,
    JumpRecovering,
    JumpBusy,
    PowerOrHeat,
    UnknownBody,
    NotYourBody,
    NotControllable,
    Destroyed,
    ExceedsMaxAccel { requested_g: f64, max_g: f64 },
    SpectatorCannotCommand,
    /// Intercept needs a track; a bearing alone gives no range.
    NoTrack,
    InvalidTarget,
    EmptyMagazine,
    LauncherRecharging,
    BeamRecharging,
    Unarmed,
}

impl Command {
    pub fn body(&self) -> Option<BodyId> {
        match *self {
            Self::SetBeamMode {body,..} | Self::Withdraw {body} | Self::Surrender {body} | Self::SetRepairGoal {body,..} | Self::Jump {body,..} | Self::CancelJump {body} | Self::Alongside {body,..} | Self::Follow {body,..} | Self::SetThrust { body, .. } | Self::Orbit { body, .. } | Self::Intercept { body, .. }
            | Self::AppendWaypoint {body,..} | Self::Flyby { body, .. } | Self::CombatRange {body,..} | Self::KeepRange {body,..} | Self::Evade {body,..} | Self::MoveTo { body, .. } | Self::AllStop { body }
            | Self::SetDriveLimit { body, .. } | Self::Launch { body, .. } | Self::FireBeam { body, .. }
            | Self::SetHeatDump {body,..} | Self::Ping { body } | Self::EngageBeam { body, .. } | Self::SetScreen { body, .. } | Self::SetSystemMode {body,..}
            | Self::CancelLaunches { body } | Self::DeployProbe {body,..} | Self::ArmBeams { body } => Some(body),
            Self::SetWarp(_) | Self::SetPaused(_) => None,
        }
    }
}

impl From<OrderError> for Rejection {
    fn from(e: OrderError) -> Self {
        match e {
            OrderError::JumpUnavailable=>Rejection::JumpUnavailable,
            OrderError::JumpRecovering=>Rejection::JumpRecovering,
            OrderError::JumpBusy=>Rejection::JumpBusy,
            OrderError::PowerOrHeat => Rejection::PowerOrHeat,
            OrderError::Destroyed => Rejection::Destroyed,
            OrderError::NoTrack => Rejection::NoTrack,
            OrderError::InvalidTarget => Rejection::InvalidTarget,
            OrderError::EmptyMagazine => Rejection::EmptyMagazine,
            OrderError::LauncherRecharging => Rejection::LauncherRecharging,
            OrderError::BeamRecharging => Rejection::BeamRecharging,
            OrderError::Unarmed => Rejection::Unarmed,
        }
    }
}

/// Command-ship state, delayed friendly telemetry, or truth for the spectator.
#[derive(Clone, Debug)]
pub struct BodyView {
    pub beam_mode:crate::world::weapon_fit::BeamMode,
    pub interference_remaining:f64,
    pub display_class:Option<String>,
    pub withdrawing:bool,
    pub jump:Option<crate::world::jump::JumpState>,
    pub jump_ready_at:f64,
    pub ship_class:Option<crate::world::ShipClass>,
    pub heading:Vec2,
    pub spinal_ready_at:f64,
    pub controls:crate::world::controls::Controls,
    pub emissivity:crate::sensors::EmissivityFactors,
    pub damage:crate::damage::Report,
    pub interceptor_battery:Option<crate::world::interceptor::Battery>,
    pub interceptor:Option<(f64,f64)>, // Remaining delta-v and expiry, not truth target IDs.
    pub point_defence: Option<crate::world::point_defence::PointDefence>,
    pub has_screen: bool,
    pub baseline_emission_factor: f64,
    pub sensors: crate::sensors::SensorSuite,
    pub probes: u32,
    pub thermal: crate::thermal::Thermal,
    pub thermal_rated_accel:f64,
    pub id: BodyId,
    pub name: String,
    pub kind: BodyKind,
    pub faction: FactionId,
    pub pos: Vec2,
    pub vel: Vec2,
    /// Thrust actually applied now, km/s².
    pub thrust: Vec2,
    /// Manual thrust order (used when no autopilot order is active).
    pub commanded: Vec2,
    pub autopilot: Option<Autopilot>,
    pub route:Option<crate::route::FlightRoute>,
    pub avoidance: Avoidance,
    /// Cap on autopilot thrust, km/s² (infinite when unset).
    pub drive_limit: f64,
    pub magazine: [u32; 2],
    pub missile_ready_at: [f64; 2],
    pub missile_queued: [u32; 2],
    pub missile: Option<MissileView>,
    pub screen_up: bool,
    /// Energy stored in the screen and taken by the hull, J.
    pub hull_j: f64,
    /// A combatant; unarmed ships (transports) are shown differently.
    pub armed: bool,
    pub controllable: bool,
    pub beam_ready_at: f64,
    pub beam_target: Option<ContactId>,
    pub beam_auto: bool,
    pub beam_solutions: BTreeMap<ContactId,crate::world::BeamSolution>,
    pub beam_emitted_j: f64,
    /// Our emitted shot's aim line; carries no enemy hit result.
    pub last_beam: Option<(f64, Vec2, Vec2)>,
}

/// A missile's own status, as its faction knows it.
#[derive(Clone, Copy, Debug)]
pub struct MissileView {
    pub launcher:BodyId,
    pub correction_possible: Option<bool>,
    pub locally_resolved: bool,
    pub payload: Payload,
    pub target: ContactId,
    pub phase: Phase,
    pub dv_left: f64,
}

#[derive(Clone, Debug)]
pub struct TrackView {
    pub velocity_sigma:f64,
    /// Estimate propagated to view time.
    pub pos: Vec2,
    pub vel: Vec2,
    /// Estimated thrust, km/s².
    pub accel: Vec2,
    /// Position covariance at view time, km².
    pub cov: [[f64; 2]; 2],
    /// Emission time of the newest measurement folded in.
    pub updated_at: f64,
    pub updates: u32,
}

#[derive(Clone, Debug)]
pub struct BearingView {
    /// Nominal effective DF reach; a bearing alone does not reveal source signature.
    pub max_range:f64,
    pub received_at:f64,
    pub sensor: BodyId,
    pub origin: Vec2,
    pub bearing: f64,
    pub sigma: f64,
    pub emitted_at: f64,
}

/// Something the faction has sensed. Only received identity is included; no truth IDs.
#[derive(Clone, Debug)]
pub struct ContactView {
    /// Remembered name only after an identity-level observation arrives.
    pub identified_name:Option<String>,
    pub display_class:Option<String>,
    pub detection:crate::sensors::DetectionLevel,
    pub reporting_sensor:Option<BodyId>,
    pub ping_remaining:f64,
    pub active_fire_control:f64,
    pub resolved_class:Option<crate::world::ShipClass>,
    pub resolved_interceptor:bool,
    pub damage:Option<crate::damage::Report>,
    pub resolved_kind:Option<BodyKind>,
    /// Gameplay assumption: a position resolution identifies missile class,
    /// but bearing-only indications reveal no platform class or physical identity.
    pub resolved_missile: bool,
    pub quality: &'static str,
    pub stale: bool,
    pub id: ContactId,
    pub track: Option<TrackView>,
    pub bearings: Vec<BearingView>,
    pub last_emitted_at: f64,
    pub last_received_at: f64,
    pub last_source: Source,
    pub last_snr: f64,
    pub last_range: Option<f64>,
}

#[derive(Clone, Debug)]
pub struct CelestialView {
    pub name: String,
    pub kind: CelestialKind,
    pub pos: Vec2,
    pub radius: f64,
}

/// An observed hostile pulse, not the opponent's outbound wavefront or true origin.
#[derive(Clone, Copy, Debug)]
pub struct PingSighting {
    pub contact: ContactId,
    pub emitted_at: f64,
    pub received_at: f64,
    pub pos: Option<Vec2>,
    pub vel: Vec2,
    pub initial_radius: f64,
    pub observer: Vec2,
    pub bearing: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct OwnPing {
    pub origin: Vec2,
    pub t_emit: f64,
    pub useful_range: f64,
}
impl PingSighting {
    pub fn opacity(&self, now: f64) -> f32 {
        let lifetime=(1.0-(now-self.received_at).max(0.0)/params::HOSTILE_PING_LIFETIME_S.value).clamp(0.0,1.0);
        lifetime as f32
    }
    pub fn radius(&self, now: f64) -> f64 {
        self.initial_radius + 0.5 * params::SHIP_MAX_ACCEL_G.value * G0 * (now-self.emitted_at).max(0.0).powi(2)
    }
    pub fn center(&self, now: f64) -> Option<Vec2> { self.pos.map(|p| p+self.vel*(now-self.emitted_at).max(0.0)) }
}

#[derive(Clone, Debug)]
pub struct LossView {
    pub body: BodyId,
    pub name: String,
    pub t: f64,
    pub cause: String,
}

#[derive(Clone, Debug)]
pub struct View {
    pub jump_events:Vec<crate::world::CombatEvent>,
    pub withdrawals:BTreeMap<ContactId,f64>,
    pub hostile_pings: Vec<PingSighting>,
    pub combat: Vec<crate::world::CombatEvent>,
    pub pending_orders: usize,
    pub time: f64,
    pub role: Role,
    pub warp: f64,
    pub paused: bool,
    pub bodies: Vec<BodyView>,
    /// Own emitted pulses only (all pulses for the spectator).
    pub pings: Vec<OwnPing>,
    pub contacts: Vec<ContactView>,
    pub celestials: Vec<CelestialView>,
    /// Own losses whose reports have arrived; the spectator sees truth.
    pub losses: Vec<LossView>,
    /// Public ephemeris, for display forecasts.
    pub system: System,
    /// The scenario goal, known to every side.
    pub objective: Option<Objective>,
    /// Referee's verdict once the game is decided.
    pub outcome: Option<Outcome>,
}

pub struct LocalSession {
    event_wait:Option<event_wait::EventWait>,
    bot_debug: VecDeque<(f64, FactionId, String)>,
    bots: BTreeMap<FactionId, crate::doctrine::Doctrine>,
    next_doctrine: f64,
    world: World,
    warp: f64,
    paused: bool,
    /// Whose alerts drop the warp (the local player's faction).
    watch: Option<FactionId>,
    last_alert: Option<(f64, String)>,
}

impl LocalSession {
    pub fn new(world: World) -> Self {
        Self { event_wait:None,bot_debug: VecDeque::new(), world, warp: 1.0, paused: true, watch: None, last_alert: None, bots: BTreeMap::new(), next_doctrine: 0.0 }
    }

    /// Explicit omniscient local debug feed, separate from faction sensor views.
    pub fn bot_debug(&self, faction: FactionId) -> impl Iterator<Item = &(f64, FactionId, String)> {
        self.bot_debug.iter().filter(move |(_, f, _)| *f == faction)
    }

    /// Advance by elapsed wall-clock seconds, scaled by the player's chosen warp.
    /// Alerts are notifications only: they never slow or pause the simulation.
    pub fn tick(&mut self, wall_dt: f64) {
        self.tick_with_deadline(wall_dt,None);
    }
    /// Keep input/rendering responsive under load. Requested warp is retained;
    /// achieved simulation speed is lower when the event budget is exhausted.
    pub fn tick_realtime(&mut self,wall_dt:f64) {
        self.tick_with_deadline(wall_dt,Some(std::time::Instant::now()+std::time::Duration::from_millis(8)));
    }
    fn tick_with_deadline(&mut self,wall_dt:f64,deadline:Option<std::time::Instant>) {
        if self.paused {
            return;
        }
        let t = (self.world.time() + wall_dt * self.warp).min(self.event_wait_until());
        loop {
            if deadline.is_some_and(|d|std::time::Instant::now()>=d) {break;}
            if !self.bots.is_empty() && self.world.time() >= self.next_doctrine {
                for f in self.bots.keys().copied().collect::<Vec<_>>() {
                    let v = self.view(Role::Faction(f));
                    let orders = self.bots.get_mut(&f).unwrap().orders(&v);
                    for cmd in orders {
                        let description=match &cmd {
                            Command::Ping {..} => "Active ping".into(),
                            Command::Flyby {target:InterceptTarget::Contact(c),..} => format!("Flyby ordered on {c}"),
                            Command::EngageBeam {target:Some(c),..} => format!("Main beam assigned to {c}"),
                            Command::Launch {target,payload,..} => format!("Queue {} volley → {target}",payload.name()),
                            Command::SetScreen {up,..} => format!("Screen {}",if *up {"raised"} else {"lowered"}),
                            _ => format!("{cmd:?}"),
                        };
                        let result=self.command(Role::Faction(f),cmd);
                        let note=match result {Ok(())=>description,Err(e)=>format!("{description} · REJECTED: {e:?}")};
                        if self.bot_debug.back().is_none_or(|(_,last_f,last)|*last_f!=f || *last!=note) {
                            self.bot_debug.push_back((self.world.time(),f,note));
                            if self.bot_debug.len()>128 {self.bot_debug.pop_front();}
                        }
                    }
                }
                self.next_doctrine = self.world.time() + params::SENSOR_FRAME_S.value;
            }
            let mut until = if self.bots.is_empty() { t } else { t.min(self.next_doctrine) };
            if self.waiting_for_event() {until=until.min(self.world.time()+5.0);}
            if let Some(alert) = self.world.advance_until_alert_budgeted(until, self.event_wait_watch().or(self.watch),deadline) {
                self.last_alert = Some((alert.t, self.describe(&alert)));
                if self.finish_event_wait(true) {break;}
                if self.world.time()>=t { break; }
                continue;
            }
            if self.finish_event_wait(false) {break;}
            if self.world.time() >= t { break; }
        }
    }

    pub fn enable_bot(&mut self, faction: FactionId, enabled: bool) {
        if enabled { self.bots.entry(faction).or_default(); } else { self.bots.remove(&faction); }
    }

    /// The faction whose perceived events should drop the warp; `None` for none.
    pub fn set_watch(&mut self, f: Option<FactionId>) {
        self.watch = f;
    }

    /// The most recent alert that dropped the warp, with its time.
    pub fn last_alert(&self) -> Option<&(f64, String)> {
        self.last_alert.as_ref()
    }

    fn describe(&self, a: &Alert) -> String {
        let name = |id: BodyId| self.world.body(id).map_or("?".to_string(), |b| b.name.clone());
        match &a.kind {
            AlertKind::ContactLost(c) => format!("Contact lost: {c}"),
            AlertKind::NewContact(c) => format!("New contact: {c}"),
            AlertKind::ShipLost(b) => format!("{} lost", name(*b)),
            AlertKind::CollisionWarning(b) => format!("{}: collision avoidance engaged", name(*b)),
            AlertKind::CollisionUnavoidable(b) => format!("{}: collision unavoidable", name(*b)),
            AlertKind::OrderComplete(b) => format!("{}: order complete", name(*b)),
            AlertKind::Hit(b) => format!("{} hit", name(*b)),
            AlertKind::LaunchCancelled(b) => format!("{}: queued launch cancelled; target track unavailable", name(*b)),
            AlertKind::GameOver => match &self.world.outcome {
                Some(o) => format!("Game over: {}", o.reason),
                None => "Game over".into(),
            },
        }
    }

    pub fn command(&mut self, role: Role, cmd: Command) -> Result<(), Rejection> {
        let description=format!("role={role:?} command={cmd:?}");
        let result=self.execute_command(role,cmd);
        self.world.debug_note("ORDER",format!("{description} result={result:?}"));
        result
    }

    pub fn enable_debug_log(&mut self,path:&std::path::Path)->std::io::Result<()> {self.world.enable_debug_log(path)}
    pub fn debug_log_path(&self)->Option<&std::path::Path> {self.world.debug_log_path()}
    pub fn debug_log_error(&self)->Option<&str> {self.world.debug_log_error()}

    fn execute_command(&mut self, role: Role, cmd: Command) -> Result<(), Rejection> {
        if let Some(body) = cmd.body() {
            let kind = self.owned(role, body)?;
            if matches!(cmd,Command::Withdraw {..}) && matches!(role,Role::Faction(_)) && (self.world.objective.as_ref().is_some_and(|o|o.player==Some(body)) || self.world.body(body).is_some_and(|b|b.controllable && !self.bots.contains_key(&b.faction))) {return Err(Rejection::InvalidTarget);}
            if let Command::SetThrust { thrust, .. } = &cmd {
                let max_g = if kind == BodyKind::Ship { self.world.body(body).and_then(|b|b.ship_class).map_or(120.0,|c|c.max_g()) } else { params::PROBE_MAX_ACCEL_G.value };
                if thrust.length() / G0 > max_g * (1.0 + 1e-9) { return Err(Rejection::ExceedsMaxAccel { requested_g: thrust.length()/G0, max_g }); }
            }
            self.world.log_command(&cmd);
            if self.world.transmit_order(body, cmd.clone()) { return Ok(()); }
        }
        match cmd {
            Command::SetBeamMode {body,mode}=>self.world.set_beam_mode(body,mode)?,
            Command::Withdraw {body}=>self.world.withdraw(body)?,
            Command::Surrender {body}=>self.world.surrender(body)?,
            Command::SetRepairGoal {body,goal}=>self.world.set_repair_goal(body,goal)?,
            Command::Jump {body,destination}=>self.world.start_jump(body,destination)?,
            Command::CancelJump {body}=>self.world.cancel_jump(body)?,
            Command::DeployProbe {body,direction} => { self.owned(role,body)?; self.world.deploy_probe(body,direction)?; }
            Command::CancelLaunches { body } => { self.owned(role, body)?; self.world.cancel_launches(body)?; }
            Command::SetWarp(w) => {self.cancel_event_wait();self.warp = w.clamp(0.0, 1e6);},
            Command::SetPaused(p) => {self.cancel_event_wait();self.paused = p;},
            Command::SetThrust { body, thrust } => {
                let kind = self.owned(role, body)?;
            if matches!(cmd,Command::Withdraw {..}) && matches!(role,Role::Faction(_)) && (self.world.objective.as_ref().is_some_and(|o|o.player==Some(body)) || self.world.body(body).is_some_and(|b|b.controllable && !self.bots.contains_key(&b.faction))) {return Err(Rejection::InvalidTarget);}
                let max_g = match kind {
                    BodyKind::Ship => self.world.body(body).and_then(|b|b.ship_class).map_or(120.0,|c|c.max_g()),
                    BodyKind::Station => 0.0,
                    BodyKind::Probe => params::PROBE_MAX_ACCEL_G.value,
                    BodyKind::Missile => params::MISSILE_MAX_ACCEL_G.value,
                };
                let requested_g = thrust.length() / G0;
                if requested_g > max_g * (1.0 + 1e-9) {
                    return Err(Rejection::ExceedsMaxAccel { requested_g, max_g });
                }
                self.world.set_thrust(body, thrust)?;
            }
            Command::Orbit { body, celestial } => {
                self.owned(role, body)?;
                self.world.set_orbit(body, celestial)?;
            }
            Command::Alongside {body,target}=>{self.owned(role,body)?;self.world.set_alongside(body,target)?;}
            Command::Follow {body,target}=>{self.owned(role,body)?;self.world.set_follow(body,target)?;}
            Command::Intercept { body, target } => {
                self.owned(role, body)?;
                self.world.set_intercept(body, target)?;
            }
            Command::Flyby { body, target } => {
                self.owned(role, body)?;
                self.world.set_flyby(body, target)?;
            }
            Command::CombatRange {body,target,standoff}=>self.world.set_combat_range(body,target,standoff)?,
            Command::KeepRange {body,target,range}=>{self.owned(role,body)?;self.world.set_tactical_range(body,target,Some(range))?;}
            Command::Evade {body,target}=>{self.owned(role,body)?;self.world.set_tactical_range(body,target,None)?;}
            Command::AppendWaypoint {body,point}=>{self.owned(role,body)?;self.world.append_waypoint(body,point)?;}
            Command::MoveTo { body, point } => {
                self.owned(role, body)?;
                self.world.set_move(body, point)?;
            }
            Command::AllStop { body } => {
                self.owned(role, body)?;
                self.world.set_all_stop(body)?;
            }
            Command::SetDriveLimit { body, g } => {
                self.owned(role, body)?;
                self.world.set_drive_limit(body, g * G0)?;
            }
            Command::Launch { body, target, payload } => {
                self.owned(role, body)?;
                self.world.queue_launch(body, target, payload)?;
            }
            Command::FireBeam { body, target } => {
                self.owned(role, body)?;
                self.world.fire_beam(body, target)?;
            }
            Command::Ping { body } => {
                self.owned(role, body)?;
                if !self.world.ping(body) {
                    return Err(Rejection::Destroyed);
                }
            }
            Command::EngageBeam { body, target } => {
                self.owned(role, body)?;
                self.world.engage_beam(body, target)?;
            }
            Command::ArmBeams { body } => {
                self.owned(role, body)?;
                self.world.arm_beams(body)?;
            }
            Command::SetHeatDump {body,enabled}=>{self.owned(role,body)?;self.world.set_heat_dump(body,enabled)?;}
            Command::SetScreen { body, up } => {
                self.owned(role, body)?;
                self.world.set_screen(body, up)?;
            }
            Command::SetSystemMode {body,system,mode}=>{
                self.owned(role,body)?;
                self.world.set_system_mode(body,system,mode)?;
            }
        }
        Ok(())
    }

    fn owned(&self, role: Role, body: BodyId) -> Result<BodyKind, Rejection> {
        let Role::Faction(faction) = role else {
            return Err(Rejection::SpectatorCannotCommand);
        };
        let b = self.world.body(body).ok_or(Rejection::UnknownBody)?;
        if b.faction != faction {
            return Err(Rejection::NotYourBody);
        }
        if !b.controllable || self.world.decider(faction,self.world.time()) != Some(body) {
            return Err(Rejection::NotControllable);
        }
        Ok(b.kind)
    }

    /// Camera-only target relationship. This intentionally reveals selected
    /// targeting intent, but returns identities only within the viewer's picture.
    /// Position and uncertainty must still come from `view`, never world truth.
    pub fn camera_target_of(&self,role:Role,selected:InterceptTarget)->Option<InterceptTarget> {
        let w=&self.world;
        let id=match selected {
            InterceptTarget::Own(id)=>id,
            InterceptTarget::Contact(contact)=>match role {
                Role::Faction(f)=>w.body_for_contact(f,contact)?,
                Role::Spectator=>return None,
            },
        };
        let b=w.body(id)?;
        let target=b.missile.map(|m|InterceptTarget::Contact(m.target))
            .or(b.beam_target.map(InterceptTarget::Contact))
            .or_else(||b.autopilot.and_then(|ap|match ap.order {
                Order::Intercept(t)|Order::Flyby(t)|Order::KeepRange(t,_)|Order::CombatRange(t,_)|Order::Evade(t)=>Some(t),
                Order::Alongside {target,..}=>Some(InterceptTarget::Contact(target)),
                Order::Follow {target,..}=>Some(InterceptTarget::Own(target)),
                _=>None,
            }));
        let target_id=if let Some(interceptor)=b.interceptor {interceptor.target} else {match target? {
            InterceptTarget::Own(id)=>id,
            InterceptTarget::Contact(c)=>w.body_for_contact(b.faction,c)?,
        }};
        let target=w.body(target_id)?;
        match role {
            Role::Spectator=>Some(InterceptTarget::Own(target_id)),
            Role::Faction(f) if target.faction==f=>Some(InterceptTarget::Own(target_id)),
            Role::Faction(f)=>w.contact_truth(f).into_iter()
                .find_map(|(c,id)|(id==target_id).then_some(InterceptTarget::Contact(c))),
        }
    }

    pub fn view(&self, role: Role) -> View {
        let w = &self.world;
        let t = w.time();
        let visible = |f: FactionId| match role {
            Role::Spectator => true,
            Role::Faction(me) => me == f,
        };
        let bodies = w
            .bodies
            .iter()
            .enumerate()
            .filter(|(_, b)| visible(b.faction))
            .filter_map(|(i, b)| {
                let known;
                let b = if let Role::Faction(f) = role { known = w.known_body(f, BodyId(i as u32))?; &known } else { b };
                let s = b.trajectory.state_at(t).or_else(||b.jump.and_then(|jump|jump.display_state(t)))?;
                Some(BodyView {withdrawing:b.withdrawing,jump:b.jump,jump_ready_at:b.jump_ready_at,beam_solutions:if b.controllable && b.kind==BodyKind::Ship {
                    w.received_picture(BodyId(i as u32)).into_iter().flat_map(|p|p.contacts.keys())
                        .filter_map(|c|w.beam_solution(BodyId(i as u32),*c).map(|s|(*c,s))).collect()
                } else {BTreeMap::new()},ship_class:b.ship_class,heading:b.heading_at(t),spinal_ready_at:b.spinal_ready_at,
                    controls:b.controls,
                    emissivity:b.emissivity_factors(t),
                    damage:crate::damage::Report {damage:b.damage,installed:b.installed_systems(),observed_at:b.trajectory.start(),screen_available:b.screen_available()},
                    interceptor_battery:b.interceptor_battery,interceptor:b.interceptor.map(|i|(i.dv_left,i.expires)),
                    point_defence: b.point_defence.map(|mut pd| {pd.targets=[None;8];pd.rate_hz*=b.operating_effectiveness(crate::damage::System::PdLaser);pd}),
                    has_screen: b.has_screen,
                    baseline_emission_factor: b.baseline_emission_factor,
                    sensors: b.sensors,
                    probes: b.probes,
                    thermal: b.thermal,
                    thermal_rated_accel:b.heat_rated_accel(),
                    id: BodyId(i as u32),
                    beam_mode:b.beam_mode,interference_remaining:(b.disrupted_until-t).max(0.0),
                    name: b.name.clone(),display_class:b.display_class.clone(),
                    kind: b.kind,
                    faction: b.faction,
                    pos: s.pos,
                    vel: s.vel,
                    thrust: b.trajectory.thrust_at(t).unwrap_or(Vec2::ZERO),
                    commanded: b.commanded,
                    autopilot: b.autopilot,
                    route:if b.autopilot.is_some_and(|a|a.order==Order::Route) {b.route.clone()} else {None},
                    avoidance: b.avoidance,
                    drive_limit: b.drive_limit,
                    magazine: b.magazine,
                    missile_ready_at: b.missile_ready_at,
                    missile_queued: b.missile_queued,
                    missile: b.missile.map(|m| MissileView { launcher:m.launcher,payload: m.payload, target: m.target, phase: m.phase, dv_left: m.dv_left, locally_resolved: m.local_fix.is_some(),
                        correction_possible:m.local_fix.map(|fix| {
                            let target=crate::kinematics::State {pos:fix.pos+fix.vel*(t-fix.t),vel:fix.vel};
                            let (left,miss)=crate::missile::zero_effort_miss(s,target,Vec2::ZERO);
                            miss.length()<=crate::missile::lateral_reach(b.max_accel(),m.dv_left,left)
                        }),
                    }),
                    screen_up: b.screen_up,

                    hull_j: b.hull_j,
                    armed: b.armed,
                    controllable: b.controllable && w.decider(b.faction,t)==Some(BodyId(i as u32)),
                    beam_ready_at: b.beam_ready_at,
                    beam_target: b.beam_target,
                    beam_auto: b.beam_auto,
                    beam_emitted_j: b.beam_emitted_j,
                    last_beam: b.last_beam,
                })
            })
            .collect();

        let contacts = match role {
            Role::Spectator => vec![],
            Role::Faction(f) => w.perception(f).map(|p| {
                p.contacts
                    .values()
                    .filter(|c| !w.contact_retired(f, c.id) && c.detection(t)!=crate::sensors::DetectionLevel::None)
                    .filter(|c| c.detection(t)>=crate::sensors::DetectionLevel::Resolved
                        || !w.body_for_contact(f,c.id).is_some_and(|id|w.bodies[id.0 as usize].interceptor.is_some()))
                    .map(|c| {
                        let detection=c.detection(t);
                        let track = c.estimate(t,&w.system).map(|now| {
                            TrackView { velocity_sigma:now.p[2][2].max(now.p[3][3]).max(0.0).sqrt(),pos: now.pos(), vel: now.vel(), accel: now.accel(), cov: now.pos_cov(), updated_at: c.track.as_ref().unwrap().t, updates: now.updates }
                        });
                        let bearings = c
                            .bearings
                            .values()
                            .filter_map(|o| match o.measurement {
                                Measurement::Bearing { bearing, sigma } => Some(BearingView {
                                    max_range:crate::sensors::detection_ranges(crate::sensors::REFERENCE_EF)[3]
                                        * w.bodies[o.sensor.0 as usize].sensor_effectiveness()[1],
                                    received_at:o.decider_received_at,
                                    sensor: o.sensor,
                                    origin: o.origin,
                                    bearing,
                                    sigma,
                                    emitted_at: o.emitted_at,
                                }),
                                Measurement::BearingRange { .. } => None,
                            })
                            .collect();
                        let last_range = match c.last.measurement {
                            Measurement::BearingRange { range, .. } => Some(range),
                            Measurement::Bearing { .. } => None,
                        };
                        ContactView {
                            identified_name:if c.identified {w.body_for_contact(f,c.id).and_then(|id|w.body(id)).filter(|b|matches!(b.kind,BodyKind::Ship|BodyKind::Station)).map(|b|b.name.clone())} else {None},
                            display_class:if c.resolved {w.body_for_contact(f,c.id).and_then(|id|w.body(id)).and_then(|b|b.display_class.clone())} else {None},
                            detection,reporting_sensor:c.best_evidence(t).map(|o|o.sensor),ping_remaining:c.ping_remaining(t),active_fire_control:c.active_fire_control(t,|sensor|w.body(sensor).is_some_and(|b|matches!(b.kind,BodyKind::Ship|BodyKind::Station))),
                            resolved_class:if c.resolved {w.body_for_contact(f,c.id).and_then(|id|w.bodies[id.0 as usize].ship_class)} else {None},
                            resolved_interceptor:c.resolved && w.body_for_contact(f,c.id).is_some_and(|id|w.bodies[id.0 as usize].interceptor.is_some()),
                            damage:w.known_damage(f,c.id),
                            resolved_kind:if c.resolved {w.body_for_contact(f,c.id).map(|id|w.bodies[id.0 as usize].kind)} else {None},
                            resolved_missile: c.resolved && w.body_for_contact(f,c.id)
                                .is_some_and(|id|w.bodies[id.0 as usize].kind==BodyKind::Missile),
                            quality: if t - c.last.decider_received_at > params::TRACK_LOST_S.value { "lost" }
                                else if t - c.last.decider_received_at > params::TRACK_STALE_S.value { "stale" }
                                else {detection.label()},
                            stale: t - c.last.decider_received_at > params::TRACK_STALE_S.value,
                            id: c.id,
                            track,
                            bearings,
                            last_emitted_at: c.last.emitted_at,
                            last_received_at: c.last.decider_received_at,
                            last_source: c.last.source,
                            last_snr: c.last.snr,
                            last_range,
                        }
                    })
                    .collect()
            }).unwrap_or_default(),
        };

        let celestials = w
            .system
            .bodies
            .iter()
            .enumerate()
            .map(|(i, c)| CelestialView { name: c.name.clone(), kind: c.kind, pos: w.system.state(i, t).pos, radius: c.radius })
            .collect();

        let losses = w
            .losses
            .iter()
            .filter(|l| l.cause != LossCause::Expended && w.body(l.body).is_some_and(|b| visible(b.faction))
                && match role { Role::Spectator => true, Role::Faction(f) => w.loss_known(f,l.body) })
            .map(|l| LossView {
                body: l.body,
                name: w.body(l.body).map(|b| b.name.clone()).unwrap_or_default(),
                t: l.t,
                cause: match l.cause {
                    LossCause::Impact(i) => format!("hit {}", w.system.bodies[i].name),
                    LossCause::Missile { payload, .. } => format!("{} missile", payload.name()),
                    LossCause::Withdrawn=>"withdrew from combat".into(),
                    LossCause::Surrendered=>"surrendered".into(),
                    LossCause::Expended => "expended".into(),
                    LossCause::ShipBeam { .. } => "ship beam".into(),
                    LossCause::PointDefence { .. } => "point-defence laser".into(),
                    LossCause::Interceptor { .. } => "point-defence missile".into(),
                },
            })
            .collect();

        View {
            jump_events:w.jump_events(match role {Role::Faction(f)=>Some(f),Role::Spectator=>None}),
            withdrawals:match role {Role::Faction(f)=>w.withdrawal_notices(f),Role::Spectator=>BTreeMap::new()},
            hostile_pings: w.hostile_pings(match role { Role::Faction(f) => Some(f), Role::Spectator => None }),
            combat: w.combat_events(match role { Role::Spectator => None, Role::Faction(f) => Some(f) }),
            pending_orders: match role { Role::Spectator => 0, Role::Faction(f) => w.pending_orders(f) },
            time: t,
            role,
            warp: self.warp,
            paused: self.paused,
            bodies,
            pings: w.ping_emissions.iter().filter(|(id, front)| visible(w.bodies[id.0 as usize].faction)
                && !w.hidden_ping_circles.contains(&(*id,front.t_emit.to_bits()))
                && !matches!(w.bodies[id.0 as usize].kind, BodyKind::Station | BodyKind::Missile)).map(|(id, front)| OwnPing {
                origin:front.origin,t_emit:front.t_emit,useful_range:crate::sensors::ping_range(crate::sensors::REFERENCE_EF) * w.bodies[id.0 as usize].sensor_rating()/100.0 * w.bodies[id.0 as usize].operating_effectiveness(crate::damage::System::Active) * if matches!(w.bodies[id.0 as usize].kind,BodyKind::Probe|BodyKind::Missile) { params::PROBE_SENSOR_FACTOR.value.sqrt() } else {1.0}
            }).collect(),
            contacts,
            celestials,
            losses,
            system: w.system.clone(),
            objective: w.objective.clone(),
            outcome: match role {Role::Spectator=>w.outcome.clone(),Role::Faction(f)=>w.received_outcome(f)},
        }
    }

    /// Which body each of a faction's contacts really is. Truth: spectator only.
    pub fn contact_truth(&self, role: Role, faction: FactionId) -> Option<BTreeMap<ContactId, BodyId>> {
        (role == Role::Spectator).then(|| self.world.contact_truth(faction))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::{self, ESCORT, RAIDER};

    fn running(secs: f64) -> LocalSession {
        let mut s = LocalSession::new(scenario::transport_intercept());
        s.command(Role::Spectator, Command::SetPaused(false)).unwrap();
        s.tick(secs);
        s
    }
    /// Close geometry for perception/relay tests, independent of opening balance.
    fn close_scenario()->World {
        let mut w=scenario::transport_intercept();
        let own=w.bodies[1].trajectory.state_at(0.0).unwrap();
        w.bodies[2].trajectory=crate::kinematics::Trajectory::new(-2000.0,crate::kinematics::State {pos:own.pos+Vec2::new(3.0*crate::units::LIGHT_SECOND,0.0),vel:Vec2::ZERO});
        for b in &mut w.bodies {b.beam_auto=false;}
        w
    }

    #[test]
    fn ship_names_require_received_identity_without_revealing_remote_truth() {
        use crate::world::BodySpec;
        use crate::kinematics::State;
        let specs=(0..2).map(|i|BodySpec {name:format!("Ship {i}"),kind:BodyKind::Ship,faction:FactionId(i),
            state:State {pos:Vec2::new(i as f64*30.0*crate::units::LIGHT_SECOND,0.0),vel:Vec2::ZERO},thrust:Vec2::ZERO,magazine:0}).collect();
        let mut world=World::new(crate::celestial::System {bodies:vec![]},specs,0.0,42);
        world.set_platform_identity(BodyId(1),"Vow of Iron".into(),"Sword Escort".into());
        let mut session=LocalSession::new(world);let role=Role::Faction(FactionId(0));
        session.world.advance_to(29.0);
        assert!(session.view(role).contacts.iter().all(|c|c.identified_name.is_none()));
        session.world.advance_to(45.0);
        let view=session.view(role);
        let contact=view.contacts.iter().find(|c|c.identified_name.as_deref()==Some("Vow of Iron")).expect("identity report after light delay");
        assert_eq!(contact.display_class.as_deref(),Some("Sword Escort"));
        assert!(view.bodies.iter().all(|b|b.faction==FactionId(0)),"no foreign truth body");
        let id=contact.id;
        let mut old=session.world.perception(FactionId(0)).unwrap().contacts[&id].last;
        old.detection=crate::sensors::DetectionLevel::Approximate;old.emitted_at=50.0;old.sensor_received_at=80.0;old.decider_received_at=80.0;
        let mut perception=session.world.perception(FactionId(0)).unwrap().clone();
        perception.ingest(old,&session.world.system);
        assert!(perception.contacts[&id].identified,"identity is remembered independently of current sensor precision");
    }

    #[test]
    fn camera_maps_enemy_target_into_the_viewers_identity_space() {
        let mut s=LocalSession::new(close_scenario());
        s.world.advance_to(10.0);
        let raider=s.world.contact_truth(ESCORT).into_iter().find_map(|(c,id)|(id==BodyId(2)).then_some(c)).unwrap();
        let victim=s.world.contact_truth(RAIDER).into_iter().find_map(|(c,id)|(id==BodyId(1)).then_some(c)).unwrap();
        s.world.bodies[2].beam_target=Some(victim);
        assert_eq!(s.camera_target_of(Role::Faction(ESCORT),InterceptTarget::Contact(raider)),
            Some(InterceptTarget::Own(BodyId(1))));
        assert_eq!(s.camera_target_of(Role::Faction(ESCORT),InterceptTarget::Contact(ContactId(999))),None);
    }

    #[test]
    fn doctrine_engages_without_truth_and_is_warp_independent() {
        let run=|chunks:usize| {
            let mut s=running(0.0);
            s.enable_bot(RAIDER,true);
            s.enable_bot(ESCORT,true);
            for _ in 0..chunks { s.tick(1800.0/chunks as f64); }
            assert!(!s.world.bodies.iter().any(|b| b.missile.is_some()),"uncertain early tracks must not trigger speculative AI salvos");
            s.world.command_log().to_vec()
        };
        assert_eq!(run(1),run(30));
    }

    #[test]
    fn faction_view_holds_own_ships_and_contacts_only() {
        let mut s = LocalSession::new(close_scenario());s.world.advance_to(60.0);
        let v = s.view(Role::Faction(ESCORT));
        assert!(v.bodies.iter().all(|b| b.faction == ESCORT));
        assert_eq!(v.bodies.len(), 3);
        let c = v.contacts.first().expect("nearby raider is detected");
        assert!(c.last_emitted_at < v.time, "its light is old");
        // Coarse passive bearings from escorts 1 ls apart cannot fix a target ~160 ls
        // away: the opening is bearing-only until they spread out or ping.
        // The lunar station supplies a wider baseline than the two escort ships.
        assert!(!c.bearings.is_empty());
    }

    #[test]
    fn ping_is_one_shot_and_its_display_is_private() {
        let mut s = running(0.0);
        s.command(Role::Faction(ESCORT), Command::Ping { body: BodyId(1) }).unwrap();
        let first = s.view(Role::Faction(ESCORT)).pings[0];
        assert_eq!(first.useful_range,5.0*crate::units::AU);
        assert!(first.useful_range>=Payload::Nuclear.engagement_range());
        assert!(s.view(Role::Faction(RAIDER)).pings.is_empty());
        s.world.advance_to(300.0);
        let pulses = s.view(Role::Faction(ESCORT)).pings;
        assert_eq!(pulses.len(), 1);
        assert_eq!(pulses[0].t_emit, first.t_emit);
        assert_eq!(pulses[0].origin, first.origin);
        s.command(Role::Faction(ESCORT), Command::Ping { body: BodyId(1) }).unwrap();
        assert_eq!(s.view(Role::Faction(ESCORT)).pings.len(), 2);
        s.world.advance_to(300.0+2.0*first.useful_range/crate::units::C+30.0);
        assert!(s.view(Role::Faction(ESCORT)).pings.is_empty());
    }

    #[test]
    fn hostile_ping_is_delayed_then_expires_without_exposing_its_wavefront() {
        let mut s=running(0.0);
        // Isolate direction-only ping reception from passive and station reports.
        s.world.bodies[1].sensors.passive=false;
        for i in [0,3] {s.world.bodies[i].sensors=crate::sensors::SensorSuite {passive:false,active:false,direction_finding:false};}
        // Fixed geometry tests the light-delay boundary independently of random starts.
        let own=s.world.bodies[1].trajectory.state_at(0.0).unwrap().pos;
        s.world.bodies[2].trajectory=crate::kinematics::Trajectory::new(-3600.0,crate::kinematics::State {
            pos:own+Vec2::new(600.0*crate::units::LIGHT_SECOND,0.0),vel:Vec2::ZERO});
        s.world.set_thrust(BodyId(2),Vec2::new(-20.0*crate::units::G0,0.0)).unwrap();
        s.command(Role::Faction(RAIDER),Command::Ping {body:BodyId(2)}).unwrap();
        s.world.advance_to(100.0);
        let v=s.view(Role::Faction(ESCORT));
        assert!(v.hostile_pings.is_empty() && v.pings.is_empty());
        s.world.advance_to(750.0);
        let v=s.view(Role::Faction(ESCORT));
        assert!(v.pings.is_empty());
        let p=v.hostile_pings.first().expect("pinger becomes visible only after light arrives");
        assert!(p.pos.is_none(),"at the doubled separation this pulse is direction-only");
        assert!(p.radius(900.0)>p.radius(750.0));
        assert!(p.opacity(900.0)<p.opacity(750.0));
        assert_eq!(p.opacity(p.received_at+600.0),0.0);
        s.world.advance_to(1400.0);
        assert!(s.view(Role::Faction(ESCORT)).hostile_pings.is_empty());
    }

    #[test]
    fn ping_manoeuvre_radius_uses_100g_and_bearing_only_has_no_position() {
        let p=PingSighting {contact:ContactId(1),emitted_at:0.0,received_at:10.0,pos:None,
            vel:Vec2::ZERO,initial_radius:0.0,observer:Vec2::ZERO,bearing:0.0};
        assert!(p.center(100.0).is_none());
        assert!((p.radius(600.0)-176_519.7).abs()<0.01);
    }

    #[test]
    fn beam_commands_enforce_ownership_and_tracks() {
        let mut s = running(60.0);
        let fire = Command::FireBeam { body: BodyId(1), target: ContactId(999) };
        assert_eq!(s.command(Role::Faction(RAIDER), fire.clone()), Err(Rejection::NotYourBody));
        assert_eq!(s.command(Role::Spectator, fire.clone()), Err(Rejection::SpectatorCannotCommand));
        assert_eq!(s.command(Role::Faction(ESCORT), fire), Err(Rejection::NoTrack));
    }

    #[test]
    fn missiles_cannot_receive_player_commands() {
        let mut s = running(0.0);
        // Use a missile body fixture; every body command shares the ownership gate.
        s.world.bodies[0].kind = BodyKind::Missile;
        let body = BodyId(0);
        for cmd in [
            Command::SetThrust { body, thrust: Vec2::ZERO },
            Command::AllStop { body },
            Command::Ping { body },
            Command::SetScreen { body, up: true },
            Command::SetHeatDump {body,enabled:true},
            Command::Orbit { body, celestial: 1 },
            Command::FireBeam { body, target: ContactId(0) },
            Command::EngageBeam { body, target: Some(ContactId(0)) },
        ] {
            assert_eq!(s.command(Role::Faction(ESCORT), cmd), Err(Rejection::NotControllable));
        }
        assert!(s.view(Role::Faction(ESCORT)).bodies.iter().any(|b| b.id == body), "missiles remain visible on the map");
    }

    #[test]
    fn lunar_station_is_autonomous_and_reports_without_visible_pulses() {
        let mut s=LocalSession::new(close_scenario());
        // Keep this relay fixture clear of lunar occultation for any Sol phase.
        let station_pos=s.world.bodies[3].trajectory.state_at(0.0).unwrap().pos;
        s.world.bodies[1].trajectory=crate::kinematics::Trajectory::new(-2000.0,crate::kinematics::State {
            pos:station_pos+Vec2::new(20_000.0,0.0),vel:Vec2::ZERO});
        s.world.bodies[2].trajectory=crate::kinematics::Trajectory::new(-2000.0,crate::kinematics::State {
            pos:station_pos+Vec2::new(3.0*crate::units::LIGHT_SECOND,0.0),vel:Vec2::ZERO});
        let station=BodyId(3);
        assert_eq!(s.command(Role::Faction(ESCORT),Command::Ping {body:station}),Err(Rejection::NotControllable));
        assert!(s.world.set_screen(station,true).is_err());
        s.world.advance_to(180.0);
        let b=&s.world.bodies[3];
        assert!(!b.armed && !b.screen_up && b.magazine==[0; 2]);
        let pulses:Vec<_>=s.world.ping_emissions.iter().filter(|(id,_)|*id==station).map(|(_,p)|p.t_emit).collect();
        assert!(pulses.len()>=3);
        assert!(pulses.windows(2).all(|p| (p[1]-p[0]-60.0).abs()<1e-9));
        assert!(s.view(Role::Faction(ESCORT)).pings.is_empty());
        assert!(s.world.perception(ESCORT).unwrap().log.iter().any(|o|o.sensor==station && o.decider_received_at>o.sensor_received_at));
        s.world.advance_to(86_400.0);
        let station_pos=s.world.bodies[3].trajectory.state_at(s.world.time()).expect("station survives lunar orbit").pos;
        assert!((station_pos-s.world.system.state(2,s.world.time()).pos).length()<10_000.0);
    }

    #[test]
    fn another_allied_ship_cannot_receive_player_orders() {
        let mut s=running(0.0);
        s.world.bodies[0].controllable=true;
        assert_eq!(s.command(Role::Faction(ESCORT),Command::Ping {body:BodyId(0)}),Err(Rejection::NotControllable));
        assert_eq!(s.view(Role::Faction(ESCORT)).bodies.iter().filter(|b|b.controllable).map(|b|b.id).collect::<Vec<_>>(),vec![BodyId(1)]);
    }

    #[test]
    fn persistent_debug_log_records_orders_rejections_and_combat() {
        let mut s=running(0.0);
        let stamp=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let path=std::env::temp_dir().join(format!("luminal-test-{stamp}-{}.log",std::process::id()));
        s.enable_debug_log(&path).unwrap();
        s.command(Role::Faction(ESCORT),Command::Ping {body:BodyId(1)}).unwrap();
        assert!(s.command(Role::Faction(ESCORT),Command::Ping {body:BodyId(2)}).is_err());
        s.world.debug_note("COMBAT","test event".into());
        let log=std::fs::read_to_string(&path).unwrap();
        assert!(log.contains("SESSION") && log.contains("PARAM") && log.contains("PLATFORM"));
        assert!(log.contains("PING") && log.contains("ORDER") && log.contains("result=Err"));
        assert!(log.contains("COMBAT\ttest event"));
        drop(s);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn missile_pings_remain_physical_but_have_no_map_ring() {
        let mut s=running(0.0);
        assert!(s.world.ping(BodyId(1)));
        let visible=s.view(Role::Faction(ESCORT)).pings.len();
        assert!(visible>0);
        let emitted=s.world.ping_emissions.len();
        s.world.bodies[1].kind=BodyKind::Missile;
        assert_eq!(s.view(Role::Faction(ESCORT)).pings.len(),visible-1);
        assert_eq!(s.world.ping_emissions.len(),emitted,"display filtering must not delete sensor pulses");
    }

    #[test]
    fn transport_orders_are_scenario_controlled() {
        let mut s = running(0.0);
        for cmd in [Command::AllStop { body: BodyId(0) }, Command::SetThrust { body: BodyId(0), thrust: Vec2::ZERO },
            Command::Ping { body: BodyId(0) }] {
            assert_eq!(s.command(Role::Faction(ESCORT), cmd), Err(Rejection::NotControllable));
        }
        let v = s.view(Role::Faction(ESCORT));
        assert!(!v.bodies.iter().find(|b| b.id == BodyId(0)).unwrap().controllable);
        assert!(v.bodies.iter().find(|b| b.id == BodyId(1)).unwrap().controllable);
    }

    #[test]
    fn escort_track_on_cruiser_is_statistically_honest() {
        // The estimate must stay within a few sigma of truth as the engagement runs.
        let mut s = running(0.0);
        for _ in 0..16 {
            s.tick(1800.0);
            let v = s.view(Role::Faction(ESCORT));
            let truth = s.view(Role::Spectator);
            let assoc = s.contact_truth(Role::Spectator, ESCORT).unwrap();
            for c in &v.contacts {
                let (Some(t), Some(id)) = (&c.track, assoc.get(&c.id)) else { continue };
                let Some(real) = truth.bodies.iter().find(|b| b.id == *id) else { continue };
                let err = (t.pos - real.pos).length();
                let sigma = (t.cov[0][0] + t.cov[1][1]).sqrt();
                if std::env::var("LUMINAL_DEBUG").is_ok() {
                    let r = (real.pos - v.bodies[0].pos).length();
                    eprintln!("T+{} err {err:.0} sigma {sigma:.0} range {:.1} ls updates {} speed {:.0}", v.time, r / crate::units::LIGHT_SECOND, t.updates, real.vel.length());
                }
                if std::env::var("LUMINAL_DEBUG").is_err() {
                    assert!(err < 4.0 * sigma + 100.0, "T+{} err {err} km, sigma {sigma} km", v.time);
                }
            }
        }
    }

    #[test]
    fn raider_salvo_has_one_track_per_source_not_duplicate_cruiser_tracks() {
        let mut s=LocalSession::new(close_scenario());
        s.command(Role::Spectator,Command::SetPaused(false)).unwrap();
        s.tick(20.0);
        // Explicit speculative launches exercise association independently of AI doctrine.
        let target=s.view(Role::Faction(RAIDER)).contacts.first().unwrap().id;
        for _ in 0..3 {s.command(Role::Faction(RAIDER),Command::Launch {body:BodyId(2),target,payload:Payload::Nuclear}).unwrap();}
        s.tick(130.0);
        let v=s.view(Role::Faction(ESCORT));
        let association=s.contact_truth(Role::Spectator,ESCORT).unwrap();
        let sources:Vec<_>=v.contacts.iter().map(|c|association[&c.id]).collect();
        let unique:std::collections::BTreeSet<_>=sources.iter().copied().collect();
        assert_eq!(sources.len(),unique.len(),"each source has exactly one displayed contact");
        for contact in &v.contacts {
            let kind=s.world.bodies[association[&contact.id].0 as usize].kind;
            if let Some(resolved)=contact.resolved_kind {assert_eq!(resolved,kind);}
            assert_eq!(contact.resolved_missile,contact.resolved_kind==Some(BodyKind::Missile));
        }
        assert_eq!(sources.iter().filter(|id|**id==BodyId(2)).count(),1,"one cruiser track");
        assert_eq!(sources.iter().filter(|id|s.world.bodies[id.0 as usize].kind==BodyKind::Probe).count(),0,"probes disabled in the scenario");
        assert!(sources.iter().filter(|id|s.world.bodies[id.0 as usize].kind==BodyKind::Missile).count()>1,
            "separate salvo members must remain separate tracks; exact detections depend on scenario geometry");
    }

    #[test]
    fn new_contacts_preserve_selected_warp_and_full_tick() {
        let mut s=LocalSession::new(close_scenario());
        s.set_watch(Some(ESCORT));
        s.command(Role::Spectator,Command::SetWarp(100.0)).unwrap();
        s.command(Role::Spectator,Command::SetPaused(false)).unwrap();
        s.tick(2.0);
        let v=s.view(Role::Faction(ESCORT));
        assert!(!v.contacts.is_empty());
        assert_eq!(v.warp,100.0);
        assert_eq!(v.time,200.0);
        assert!(!s.paused);
    }

    #[test]
    fn order_completion_preserves_warp_and_finishes_tick() {
        let mut s = LocalSession::new(scenario::transport_intercept());
        s.set_watch(Some(ESCORT));
        // The frigate starts parked in orbit, so an orbit order would hold at once;
        // a move takes a while and then completes.
        let planet = s.view(Role::Faction(ESCORT)).celestials[1].pos;
        let point = planet + Vec2::new(0.0, 200_000.0);
        s.command(Role::Faction(ESCORT), Command::MoveTo { body: BodyId(1), point }).unwrap();
        s.command(Role::Spectator, Command::SetWarp(1000.0)).unwrap();
        s.command(Role::Spectator, Command::SetPaused(false)).unwrap();
        s.tick(10.0);
        let v = s.view(Role::Faction(ESCORT));
        assert_eq!(v.warp, 1000.0);
        assert_eq!(v.time, 10_000.0);
        assert!(!v.paused);
        assert!(s.world.alerts.iter().any(|a|matches!(a.kind,AlertKind::OrderComplete(BodyId(1)))));
    }

    #[test]
    fn spectator_sees_truth_and_may_ask_for_associations() {
        let s = running(60.0);
        let v = s.view(Role::Spectator);
        assert_eq!(v.bodies.len(), 4);
        assert_eq!(v.celestials.len(), 22);
        assert!(s.contact_truth(Role::Faction(ESCORT), ESCORT).is_none());
        assert!(s.contact_truth(Role::Spectator, ESCORT).is_some());
    }

    #[test]
    fn route_commands_append_preserve_target_and_restart_after_manual_thrust() {
        let mut s = LocalSession::new(close_scenario());
        let me = Role::Faction(ESCORT);
        let body = BodyId(1);
        let start = s.world.bodies[1].trajectory.state_at(s.world.time()).unwrap().pos;
        let point = start + Vec2::new(1_000_000.0, 1_000_000.0);
        let target = s.world.bodies[1].beam_target;
        s.command(me, Command::AppendWaypoint {body, point}).unwrap();
        s.command(me, Command::AppendWaypoint {body, point:point+Vec2::new(1_000_000.0,0.0)}).unwrap();
        assert_eq!(s.world.bodies[1].route.as_ref().unwrap().points.len(),3);
        assert_eq!(s.world.bodies[1].beam_target,target);
        assert_eq!(s.command(me,Command::AppendWaypoint {body:BodyId(2),point}),Err(Rejection::NotYourBody));
        s.command(me,Command::SetThrust {body,thrust:Vec2::ZERO}).unwrap();
        s.command(me,Command::AppendWaypoint {body,point}).unwrap();
        assert_eq!(s.world.bodies[1].route.as_ref().unwrap().points.len(),2);
    }

    #[test]
    fn cannot_command_enemy_or_exceed_max_accel() {
        let mut s = running(1.0);
        let me = Role::Faction(ESCORT);
        let cruiser = BodyId(2);
        assert_eq!(s.command(me, Command::SetThrust { body: cruiser, thrust: Vec2::ZERO }), Err(Rejection::NotYourBody));
        assert_eq!(s.command(me,Command::SetHeatDump {body:cruiser,enabled:true}),Err(Rejection::NotYourBody));
        assert_eq!(s.command(Role::Spectator,Command::SetHeatDump {body:BodyId(1),enabled:true}),Err(Rejection::SpectatorCannotCommand));
        let too_hard = Vec2::new(121.0 * G0, 0.0);
        assert!(matches!(
            s.command(me, Command::SetThrust { body: BodyId(1), thrust: too_hard }),
            Err(Rejection::ExceedsMaxAccel { .. })
        ));
        let _ = RAIDER;
    }
}
