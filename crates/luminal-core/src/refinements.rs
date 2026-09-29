//! Causal communications, combat reports and retained telemetry. Child of world.
use super::*;
use crate::session::Command;
use std::collections::{BTreeSet, VecDeque};
use std::io::Write;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CombatKind { JumpSpool, JumpCancelled, JumpDeparture, JumpArrival, WithdrawalStarted,WithdrawalCancelled,Withdrawn, Surrendered, NuclearBurst, BeamPulse, PointDefence, Impact, Destroyed, Expended, MissileHit, MissileMiss, SpinalPulse }
impl CombatKind {
    pub fn label(self) -> &'static str { match self {
        Self::JumpSpool=>"Jump spooling",Self::JumpCancelled=>"Jump cancelled",Self::JumpDeparture=>"Jump departure",Self::JumpArrival=>"Jump arrival",
        Self::WithdrawalStarted=>"Withdrawal jump spooling",Self::WithdrawalCancelled=>"Withdrawal interrupted",
        Self::Withdrawn=>"Withdrew from combat",Self::Surrendered=>"Surrendered",
        Self::SpinalPulse => "Spinal pulse", Self::NuclearBurst => "Nuclear burst", Self::BeamPulse => "Beam pulse",
        Self::Impact => "Impact flash", Self::Destroyed => "Loss reported", Self::Expended => "Missile expended / pass complete",
        Self::PointDefence => "Point-defence laser",
        Self::MissileHit => "Missile hit", Self::MissileMiss => "Missile miss",
    } }
}
#[derive(Clone, Debug)]
pub struct CombatEvent {
    /// Terminal motion from own telemetry; foreign events never expose true velocity.
    pub velocity:Option<Vec2>,
    /// Known platform category, retained after its map object disappears.
    pub subject_kind:Option<BodyKind>,
    /// Coarse visible impact intensity, not enemy subsystem telemetry (0..1).
    pub impact_strength:f32,
    /// Exact damage telemetry is shared with allies only.
    pub damage: Option<String>,
    /// Observer-local association; never a foreign truth body ID.
    pub contact: Option<ContactId>,
    pub target: Option<InterceptTarget>,
    pub aim: Option<Vec2>,
    pub emitted_at: f64, pub received_at: f64, pub pos: Option<Vec2>, pub kind: CombatKind,
    /// Own identity only; foreign events never reveal body IDs.
    pub own_body: Option<BodyId>,
}
#[derive(Clone)]
struct Flash { target:Option<BodyId>, velocity:Option<Vec2>, front: Front, kind: CombatKind, body: Option<BodyId>, owner: Option<FactionId>, pending: Vec<FactionId>, aim: Option<Vec2>, damage:Option<String>, impact_strength:f32 }

pub(super) fn impact_strength(before:&crate::damage::Damage,after:&crate::damage::Damage)->f32 {
    use crate::damage::{Condition,System};
    let hull=(before.hull-after.hull).max(0.0)/before.hull_max.max(1.0);
    if hull>=0.1 || System::ALL.into_iter().any(|s|before.state(s)!=after.state(s) && after.state(s)==Condition::Destroyed) {1.0}
    else if hull>0.0 || before.systems!=after.systems {0.65}
    else if after.armour<before.armour {0.35}
    else {0.0}
}

#[test]
fn impact_intensity_distinguishes_absorption_damage_and_severe_hits() {
    use crate::damage::{Damage,Condition,System};
    let before=Damage::default();
    assert_eq!(impact_strength(&before,&before),0.0);
    let mut after=before;after.armour-=1.0;
    assert_eq!(impact_strength(&before,&after),0.35);
    after.hull-=1.0;
    assert_eq!(impact_strength(&before,&after),0.65);
    after=before;after.systems[System::Propulsion as usize]=Condition::Damaged;
    assert_eq!(impact_strength(&before,&after),0.65);
    after.systems[System::Propulsion as usize]=Condition::Destroyed;
    assert_eq!(impact_strength(&before,&after),1.0);
    after=before;after.hull-=before.hull_max*0.2;
    assert_eq!(impact_strength(&before,&after),1.0);
}
struct OrderPacket { front: Front, body: BodyId, cmd: Command }
struct AlertPacket { front: Front, faction: FactionId, kind: AlertKind }

#[derive(Default)]
pub(super) struct Refinements {
    logfile: Option<std::fs::File>,
    log_path: Option<std::path::PathBuf>,
    log_error: Option<String>,
    bias_seed: u64,
    flashes: Vec<Flash>, orders: Vec<OrderPacket>, alerts: Vec<AlertPacket>,
    pub received: BTreeMap<FactionId, Vec<CombatEvent>>,
    pub truth_events: Vec<CombatEvent>,
    pub retired_contacts: BTreeSet<(FactionId, ContactId)>,
    pictures: BTreeMap<FactionId, VecDeque<(f64, Arc<Perception>)>>,
    cached_pictures: BTreeMap<BodyId, Arc<Perception>>,
    telemetry: BTreeMap<BodyId, VecDeque<(f64, Body)>>,
    damage_reports:BTreeMap<(FactionId,ContactId),crate::damage::Report>,
    emission: BTreeMap<BodyId, VecDeque<(f64, f64)>>,
    pub command_log: Vec<(f64, String)>,
    last_t: f64,
    lost_tracks: std::collections::BTreeSet<(FactionId,ContactId)>,
    pub(super) hostile_pings: BTreeMap<(FactionId,ContactId), crate::session::PingSighting>,
    biases: BTreeMap<(BodyId,ContactId,u8), (f64,f64)>,
}

impl Refinements {
    pub(super) fn with_seed(seed:u64)->Self { Self {bias_seed:seed,..Default::default()} }
    pub(super) fn record_ping(&mut self,faction:FactionId,sighting:crate::session::PingSighting) {
        let key=(faction,sighting.contact);
        if let Some(old)=self.hostile_pings.get(&key) {
            // Multiple allied sensors may report the same pulse at different times.
            // Neither duplicate nor older delayed reports replace a newer pulse.
            if sighting.emitted_at<=old.emitted_at { return; }
        }
        self.hostile_pings.insert(key,sighting);
    }
}

impl World {
    /// Cosmetic identity must not resample heat/signature or alter sensing history.
    pub(super) fn rename_telemetry(&mut self,id:BodyId,name:&str,class_title:&str) {
        if let Some(history)=self.refinement.telemetry.get_mut(&id) {
            for (_,body) in history {body.name=name.into();body.display_class=Some(class_title.into());}
        }
    }

    pub fn enable_debug_log(&mut self, path:&std::path::Path)->std::io::Result<()> {
        let file=std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(path)?;
        self.refinement.logfile=Some(file);
        self.refinement.log_path=Some(path.to_path_buf());
        self.debug_note("SESSION",format!("seed={} start_time={} format=1",self.refinement.bias_seed,self.time));
        for p in crate::params::ALL {self.debug_note("PARAM",format!("{}={} {}",p.key,p.value,p.unit));}
        for i in 0..self.bodies.len() {
            let b=&self.bodies[i];
            self.debug_note("PLATFORM",format!("id={i} name={:?} faction={:?} kind={:?} state={:?}",b.name,b.faction,b.kind,b.trajectory.state_at(self.time)));
        }
        Ok(())
    }
    pub fn debug_log_path(&self)->Option<&std::path::Path> {self.refinement.log_path.as_deref()}
    pub fn debug_log_error(&self)->Option<&str> {self.refinement.log_error.as_deref()}
    pub(crate) fn debug_note(&mut self, kind:&str, text:String) {
        if let Some(file)=self.refinement.logfile.as_mut()
            && let Err(error)=writeln!(file,"{:.6}\t{}\t{}",self.time,kind,text.replace(['\n','\r']," ")) {
            self.refinement.log_error=Some(error.to_string());
            self.refinement.logfile=None;
        }
    }
    pub(super) fn report_launch(&mut self, id: BodyId) {
        self.refinement.telemetry.entry(id).or_default().push_back((self.time,self.bodies[id.0 as usize].clone()));
        let b=&self.bodies[id.0 as usize];
        self.debug_note("LAUNCH",format!("id={id:?} name={:?} faction={:?} missile={:?} interceptor={:?}",b.name,b.faction,b.missile,b.interceptor));
        // Gameplay exception: watching a resolved launcher reveals its launch now.
        let round=&self.bodies[id.0 as usize];
        if let Some(launcher)=round.missile.map(|m|m.launcher).or_else(||round.interceptor.map(|i|i.launcher)) {
            let viewers:Vec<_>=self.perceptions.keys().copied().filter(|f| {
                *f!=self.bodies[id.0 as usize].faction && (round.missile.is_some_and(|m|m.payload==Payload::Kinetic)
                    || self.association.get(&(*f,launcher))
                    .and_then(|c|self.perceptions.get(f)?.contacts.get(c))
                    .is_some_and(|c|c.detection(self.time)>=sensors::DetectionLevel::Resolved))
            }).collect();
            for f in viewers {self.refresh_missile_contact(f,id);}
        }
    }

    /// Once acquired, a missile stays tracked throughout its live flight.
    /// This explicit gameplay exception does not reveal its target or seeker data.
    pub(super) fn refresh_resolved_missiles(&mut self) {
        let mut known=Vec::new();
        for (i,body) in self.bodies.iter().enumerate() {
            if body.kind!=BodyKind::Missile || !body.alive_at(self.time) {continue;}
            let id=BodyId(i as u32);
            let mandatory=body.missile.is_some_and(|m|m.payload==Payload::Kinetic || m.phase==Phase::Terminal);
            for (&f,picture) in &self.perceptions {
                if f==body.faction {continue;}
                let acquired=self.association.get(&(f,id)).and_then(|c|picture.contacts.get(c)).is_some_and(|c|c.resolved);
                if mandatory || acquired {known.push((f,id));}
            }
        }
        for (f,id) in known {self.refresh_missile_contact(f,id);}
    }
    pub(super) fn refresh_missile_contact(&mut self,f:FactionId,id:BodyId) {
        let Some(sensor)=self.decider(f,self.time) else {return;};
        let Some(state)=self.state(id,self.time) else {return;};
        let Some(observer)=self.state(sensor,self.time) else {return;};
        let contact=self.contact_id(f,id);let rel=state.pos-observer.pos;
        let obs=Observation {contact,sensor,origin:observer.pos,emitted_at:self.time,sensor_received_at:self.time,
            decider_received_at:self.time,source:Source::Emission,detection:sensors::DetectionLevel::Resolved,snr:1e12,
            measurement:Measurement::BearingRange {bearing:bearing_of(rel),range:rel.length(),sigma_range:0.1,sigma_bearing:1e-10}};
        let thrust=self.bodies[id.0 as usize].trajectory.last().thrust;
        if let Some(p)=self.perceptions.get_mut(&f) {
            let new=!p.contacts.contains_key(&contact);
            p.ingest(obs,&self.system);
            p.contacts.get_mut(&contact).unwrap().retain_missile_fix(obs,state,thrust);
            if new {self.alerts.push(Alert {t:self.time,faction:Some(f),kind:AlertKind::NewContact(contact)});}
        }
    }

    /// Acquired missiles use continuous gameplay tracking, including their end.
    pub(super) fn retire_tracked_missile(&mut self,id:BodyId,t:f64) {
        if self.bodies[id.0 as usize].kind!=BodyKind::Missile {return;}
        let viewers:Vec<_>=self.perceptions.keys().copied().filter(|f|
            *f==self.bodies[id.0 as usize].faction || self.association.get(&(*f,id))
                .and_then(|c|self.perceptions.get(f)?.contacts.get(c)).is_some_and(|c|c.resolved)).collect();
        for f in viewers {
            if let Some(c)=self.association.get(&(f,id)) {self.refinement.retired_contacts.insert((f,*c));}
            let flashes:Vec<_>=self.refinement.flashes.iter().filter(|flash|flash.body==Some(id) && flash.front.t_emit==t
                && matches!(flash.kind,CombatKind::Destroyed|CombatKind::Expended|CombatKind::MissileHit|CombatKind::MissileMiss|CombatKind::NuclearBurst))
                .cloned().collect();
            for flash in flashes {
                let own=self.bodies[id.0 as usize].faction==f;
                let target=flash.target.and_then(|id|if self.bodies[id.0 as usize].faction==f {Some(InterceptTarget::Own(id))}
                    else {self.association.get(&(f,id)).map(|c|InterceptTarget::Contact(*c))});
                self.refinement.received.entry(f).or_default().push(CombatEvent {target,velocity:flash.velocity,subject_kind:Some(BodyKind::Missile),
                    impact_strength:0.0,damage:None,contact:if own {None} else {self.association.get(&(f,id)).copied()},
                    own_body:own.then_some(id),pos:Some(flash.front.origin),aim:flash.aim,kind:flash.kind,emitted_at:t,received_at:self.time});
            }
            for flash in &mut self.refinement.flashes {if flash.body==Some(id) && flash.front.t_emit==t
                && matches!(flash.kind,CombatKind::Destroyed|CombatKind::Expended|CombatKind::MissileHit|CombatKind::MissileMiss|CombatKind::NuclearBurst) {
                flash.pending.retain(|viewer|*viewer!=f);
            }}
        }
    }

    pub fn contact_retired(&self, faction: FactionId, contact: ContactId) -> bool {
        self.refinement.retired_contacts.contains(&(faction, contact))
            || self.body_for_contact(faction,contact).is_some_and(|id|self.missile_departure_seen(faction,id))
    }

    /// Stop extrapolating an expired missile after its final light has passed the
    /// observer. This is display cleanup, not a new explosion or sensor fix.
    fn missile_departure_seen(&self, faction:FactionId, id:BodyId)->bool {
        let b=&self.bodies[id.0 as usize];
        if b.kind!=BodyKind::Missile {return false;}
        let Some(end)=b.trajectory.end() else {return false;};
        if end<=self.time && (b.faction==faction || self.association.get(&(faction,id)).is_some_and(|c|self.refinement.retired_contacts.contains(&(faction,*c)))) {return true;}
        let Some(receiver)=self.decider(faction,self.time) else {return false;};
        let Some(segment)=b.trajectory.segments().iter().rev().find(|s|s.t0<=end) else {return false;};
        Front {origin:segment.state_at(end).pos,t_emit:end}
            .arrival(&self.bodies[receiver.0 as usize].trajectory,end,self.time).is_some()
    }

    pub fn deploy_probe(&mut self, id: BodyId, direction: Vec2) -> Result<BodyId, OrderError> {
        if !self.probes_enabled {return Err(OrderError::InvalidTarget);}
        let t=self.time;
        if !direction.length().is_finite() || direction.length()<1e-9 { return Err(OrderError::InvalidTarget); }
        let b=self.live_body_mut(id)?;
        if b.kind!=BodyKind::Ship || b.probes==0 { return Err(OrderError::EmptyMagazine); }
        b.probes-=1;
        let mut probe=b.clone();
        let state=b.trajectory.state_at(t).unwrap();
        let pid=BodyId(self.bodies.len() as u32);
        probe.name=format!("{} Probe {}",probe.name,pid.0);
        probe.kind=BodyKind::Probe; probe.controllable=false; probe.armed=false;
        probe.ship_class=None;probe.jump=None;probe.withdrawing=false;probe.step_generation=0;
        probe.has_screen=false;
        probe.point_defence=None;
        probe.interceptor_battery=None; probe.interceptor=None;
        probe.baseline_emission_factor=1.0;
        probe.probes=0; probe.magazine=[0; 2]; probe.missile_queued=[0; 2]; probe.missile=None;
        probe.autopilot=None; probe.beam_target=None; probe.last_beam=None;
        probe.screen_up=false; probe.hull_j=0.0;
        probe.thermal=crate::thermal::Thermal {capacitor_j:0.0,last_t:t,..Default::default()};
        probe.commanded=direction.normalized()*(PROBE_MAX_ACCEL_G.value*crate::units::G0);
        probe.trajectory=Trajectory::new(t,state);
        probe.trajectory.set_thrust(t,probe.commanded).unwrap();
        probe.probe_burn_until=Some(t+PROBE_BURN_S.value);
        probe.probe_ping_at=t+PROBE_PING_INTERVAL_S.value;
        self.refinement.telemetry.entry(pid).or_default().push_back((t,probe.clone()));
        self.bodies.push(probe);
        self.last_step.push(t);
        self.scheduler.schedule(t,Event::Step(pid,0));
        self.ping(pid);
        Ok(pid)
    }
    pub(super) fn bias_observation(&mut self, mut obs: Observation) -> Observation {
        let key=(obs.sensor,obs.contact,obs.source as u8);
        let seed=self.refinement.bias_seed ^ ((obs.sensor.0 as u64)<<32) ^ obs.contact.0 as u64 ^ ((obs.source as u64)<<56);
        let (radial,angular)=*self.refinement.biases.entry(key).or_insert_with(|| {
            let mut rng=Rng::stream(seed,71); (rng.gaussian(),rng.gaussian())
        });
        match &mut obs.measurement {
            // Approximate position bias is applied once per contact in Cartesian
            // coordinates after filtering, so sensors cannot average it away.
            Measurement::BearingRange {..} if obs.detection==sensors::DetectionLevel::Approximate => {},
            Measurement::BearingRange {bearing,range,..} if obs.detection>=sensors::DetectionLevel::Resolved => {
                *range=(*range+radial.tanh()*0.1).max(0.0);
                *bearing=sensors::wrap_angle(*bearing+angular.tanh()*1e-7);
            }
            Measurement::BearingRange {bearing,range,..} => {
                let sigma=sensors::systematic_range(*range,obs.snr,obs.source);
                *range=(*range+radial*sigma).max(0.0);
                *bearing=sensors::wrap_angle(*bearing+angular*DIRECTION_SYSTEMATIC_RAD.value/obs.snr.sqrt().max(1.0));
            }
            Measurement::Bearing {bearing,..} => {
                *bearing=sensors::wrap_angle(*bearing+angular*DIRECTION_SYSTEMATIC_RAD.value/obs.snr.sqrt().max(1.0));
            }
        }
        obs
    }
    pub fn hostile_pings(&self, faction: Option<FactionId>) -> Vec<crate::session::PingSighting> {
        self.refinement.hostile_pings.iter().filter(|((f,_),p)| faction == Some(*f) && p.opacity(self.time)>0.0).map(|(_,p)| *p).collect()
    }
    pub fn cancel_launches(&mut self, id: BodyId) -> Result<(), OrderError> {
        let b = self.live_body_mut(id)?;
        b.launch_generation += 1;
        b.missile_queued = [0; 2];
        b.missile_queue_ready_at = b.missile_ready_at;
        Ok(())
    }
    pub fn command_log(&self) -> &[(f64, String)] { &self.refinement.command_log }
    pub fn log_command(&mut self, cmd: &Command) { self.refinement.command_log.push((self.time, format!("{cmd:?}"))); }
    /// Returns true when queued for transmission rather than executed locally.
    pub fn transmit_order(&mut self, id: BodyId, cmd: Command) -> bool {
        let Some(b) = self.body(id) else { return false };
        let Some(sender) = self.decider(b.faction, self.time) else { return false };
        if sender == id { return false; }
        let Some(s) = self.state(sender, self.time) else { return false };
        self.refinement.orders.push(OrderPacket { front: Front { origin: s.pos, t_emit: self.time }, body: id, cmd });
        true
    }
    pub fn pending_orders(&self, faction: FactionId) -> usize {
        self.refinement.orders.iter().filter(|o| self.bodies[o.body.0 as usize].faction == faction).count()
    }
    fn execute_transmitted(&mut self, cmd: Command) -> Result<(), OrderError> {
        match cmd {
            Command::Withdraw {body}=>self.withdraw(body),
            Command::Surrender {body}=>self.surrender(body),
            Command::SetRepairGoal {body,goal}=>self.set_repair_goal(body,goal),
            Command::Jump {body,destination}=>self.start_jump(body,destination),
            Command::CancelJump {body}=>self.cancel_jump(body),
            Command::DeployProbe {body,direction} => self.deploy_probe(body,direction).map(|_|()),
            Command::SetThrust { body, thrust } => self.set_thrust(body, thrust),
            Command::Orbit { body, celestial } => self.set_orbit(body, celestial),
            Command::Alongside {body,target}=>self.set_alongside(body,target),
            Command::Follow {body,target}=>self.set_follow(body,target),
            Command::Intercept { body, target } => self.set_intercept(body, target),
            Command::Flyby { body, target } => self.set_flyby(body, target),
            Command::AppendWaypoint {body,point}=>self.append_waypoint(body,point),
            Command::MoveTo { body, point } => self.set_move(body, point),
            Command::AllStop { body } => self.set_all_stop(body),
            Command::SetDriveLimit { body, g } => self.set_drive_limit(body, g * crate::units::G0),
            Command::Launch { body, target, payload } => self.queue_launch(body, target, payload),
            Command::CancelLaunches { body } => self.cancel_launches(body),
            Command::FireBeam { body, target } => self.fire_beam(body, target),
            Command::EngageBeam { body, target } => self.engage_beam(body, target),
            Command::ArmBeams { body } => self.arm_beams(body),
            Command::SetHeatDump {body,enabled}=>self.set_heat_dump(body,enabled),
            Command::SetScreen { body, up } => self.set_screen(body, up),
            Command::SetSystemMode {body,system,mode}=>self.set_system_mode(body,system,mode),
            Command::Ping { body } => if self.ping(body) { Ok(()) } else { Err(OrderError::Destroyed) },
            Command::KeepRange {body,target,range}=>self.set_tactical_range(body,target,Some(range)),
            Command::Evade {body,target}=>self.set_tactical_range(body,target,None),
            Command::SetWarp(_) | Command::SetPaused(_) => Ok(()),
        }
    }
    pub fn combat_events(&self, faction: Option<FactionId>) -> Vec<CombatEvent> {
        let events = faction.and_then(|f| self.refinement.received.get(&f));
        let all = if faction.is_none() { &self.refinement.truth_events } else if let Some(e) = events { e } else { return vec![] };
        all.iter().rev().take(64).cloned().collect()
    }
    /// Retain jump visuals separately so a missile barrage cannot evict them.
    pub fn jump_events(&self,faction:Option<FactionId>)->Vec<CombatEvent> {
        let all=match faction {None=>&self.refinement.truth_events,Some(f)=>match self.refinement.received.get(&f) {Some(e)=>e,None=>return vec![]}};
        all.iter().filter(|e|matches!(e.kind,CombatKind::JumpSpool|CombatKind::JumpCancelled|CombatKind::JumpDeparture|CombatKind::JumpArrival|CombatKind::Withdrawn|CombatKind::Surrendered) || (e.kind==CombatKind::Destroyed && e.subject_kind==Some(BodyKind::Ship))).cloned().collect()
    }
    pub(super) fn record_beam(&mut self,t:f64,pos:Vec2,kind:CombatKind,body:BodyId,target:BodyId,owner:FactionId) {
        self.record_combat(t,pos,kind,Some(body),Some(owner));
        if let Some(flash)=self.refinement.flashes.last_mut() {flash.target=Some(target);}
        if let Some(event)=self.refinement.truth_events.last_mut() {event.target=Some(InterceptTarget::Own(target));}
    }
    pub(super) fn record_interception(&mut self,id:BodyId,target:BodyId,t:f64,kind:CombatKind) {
        let Some(state)=self.state(if kind==CombatKind::MissileHit {target} else {id},t) else {return;};
        self.record_beam(t,state.pos,kind,id,target,self.bodies[id.0 as usize].faction);
        if let Some(flash)=self.refinement.flashes.last_mut() {flash.velocity=Some(state.vel);}
        if let Some(event)=self.refinement.truth_events.last_mut() {event.velocity=Some(state.vel);}
    }
    pub(super) fn record_combat(&mut self, t: f64, pos: Vec2, kind: CombatKind, body: Option<BodyId>, owner: Option<FactionId>) {
        self.record_combat_damage(t,pos,kind,body,owner,None);
    }
    pub(super) fn record_combat_damage(&mut self, t:f64,pos:Vec2,kind:CombatKind,body:Option<BodyId>,owner:Option<FactionId>,damage:Option<(String,f32)>) {
        let (damage,impact_strength)=damage.map_or((None,0.0),|(text,strength)|(Some(text),strength));
        if let Some(id)=body {self.snapshot_platform(id,t);}
        self.debug_note("COMBAT",format!("event_time={t:.6} kind={kind:?} body={body:?} position={pos:?}"));
        let owner = owner.or_else(|| body.map(|id| self.bodies[id.0 as usize].faction));
        let pending = self.perceptions.keys().copied().filter(|f| !matches!(kind,CombatKind::Expended|CombatKind::MissileMiss) || owner == Some(*f)).collect();
        let aim=body.and_then(|id| {
            let b=&self.bodies[id.0 as usize];
            match kind {
                CombatKind::PointDefence=>b.point_defence.and_then(|pd|pd.last_shot),
                CombatKind::BeamPulse|CombatKind::SpinalPulse=>b.last_beam,
                _=>None,
            }.filter(|(fired,_,_)|(*fired-t).abs()<1e-6).map(|(_,_,aim)|aim)
        });
        let velocity=body.and_then(|id|self.state(id,t)).map(|s|s.vel);
        self.refinement.truth_events.push(CombatEvent { target:None, velocity, subject_kind:body.map(|id|self.bodies[id.0 as usize].kind),impact_strength,damage:damage.clone(),contact:None, emitted_at: t, received_at: t, pos:Some(pos), kind, own_body: body, aim });
        let mut pending:Vec<FactionId>=pending;
        if matches!(kind,CombatKind::JumpSpool|CombatKind::JumpCancelled|CombatKind::JumpDeparture|CombatKind::JumpArrival)
            && let (Some(id),Some(f))=(body,owner) && self.decider(f,t)==Some(id) {
            self.refinement.received.entry(f).or_default().push(self.refinement.truth_events.last().unwrap().clone());
            pending.retain(|other|*other!=f);
        }
        self.refinement.flashes.push(Flash { target:None, velocity,impact_strength,damage,front: Front { origin: pos, t_emit: t }, kind, body, owner, pending, aim });
    }
    pub(super) fn delay_alert(&mut self, id: BodyId, t: f64, kind: AlertKind) {
        let b = &self.bodies[id.0 as usize];
        let origin = b.trajectory.segments().iter().rev().find(|s| s.t0 <= t).map(|s| s.state_at(t).pos);
        if let Some(origin) = origin {
            self.refinement.alerts.push(AlertPacket { front: Front { origin, t_emit: t }, faction: b.faction, kind });
        }
    }
    pub fn withdrawal_notices(&self,f:FactionId)->BTreeMap<ContactId,f64> {
        let mut notices=BTreeMap::new();
        if let Some(events)=self.refinement.received.get(&f) {for e in events {if let Some(c)=e.contact {
            match e.kind {
                CombatKind::WithdrawalStarted=>{notices.insert(c,e.emitted_at+super::jump::SPOOL_SECONDS);},
                CombatKind::WithdrawalCancelled|CombatKind::Withdrawn|CombatKind::Surrendered|CombatKind::Destroyed=>{notices.remove(&c);},
                _=>{},
            }
        }}}
        notices
    }
    pub(super) fn refinement_received_exit(&self,f:FactionId,contact:Option<ContactId>,t:f64)->bool {
        self.refinement.received.get(&f).is_some_and(|events|events.iter().any(|e|e.emitted_at==t && e.contact==contact && matches!(e.kind,CombatKind::Withdrawn|CombatKind::Surrendered)))
    }
    fn observe_flash(&mut self, faction: FactionId, flash: &Flash, arrival: f64) -> Option<CombatEvent> {
        let own=flash.owner==Some(faction);
        if matches!(flash.kind,CombatKind::WithdrawalStarted|CombatKind::WithdrawalCancelled|CombatKind::Withdrawn|CombatKind::Surrendered) {
            let contact=if own {None} else {flash.body.map(|id|self.contact_id(faction,id))};
            if let Some(c)=contact && matches!(flash.kind,CombatKind::Withdrawn|CombatKind::Surrendered) {self.refinement.retired_contacts.insert((faction,c));}
            return Some(CombatEvent {velocity:None,subject_kind:Some(BodyKind::Ship),impact_strength:0.0,damage:None,contact,target:None,aim:None,
                emitted_at:flash.front.t_emit,received_at:arrival,pos:own.then_some(flash.front.origin),kind:flash.kind,own_body:if own {flash.body} else {None}});
        }
        let pos=if own { Some(flash.front.origin) } else {
            let observer=self.decider(faction,self.time)?;
            if self.bodies[observer.0 as usize].sensor_effectiveness()==[0.0,0.0] {return None;}
            let rx=self.state(observer,arrival)?.pos;
            let relative=flash.front.origin-rx;
            let power=match flash.kind {CombatKind::NuclearBurst=>NUCLEAR_ENERGY_J.value,
                CombatKind::PointDefence=>PD_FLASH_W.value,_=>SHIP_BEAM_ENERGY_J.value};
            let (measurement,snr)=sensors::receive_measurement(self.bodies[observer.0 as usize].sensors,power,relative.length(),bearing_of(relative),&mut self.rng)?;
            let (measurement,detection)=if let Some(body)=flash.body.filter(|id|matches!(self.bodies[id.0 as usize].kind,BodyKind::Ship|BodyKind::Station)) {
                let ef=self.historical_ef(body,flash.front.t_emit);
                let level=self.detect_ship(observer,body,flash.front.t_emit,relative.length(),false);
                if level==sensors::DetectionLevel::None {return None;}
                (sensors::ship_measurement(level,relative.length(),bearing_of(relative),ef),level)
            } else {(measurement,sensors::DetectionLevel::Resolved)};
            if matches!(measurement,Measurement::Bearing {..})
                && flash.body.is_some_and(|id|self.bodies[id.0 as usize].kind==BodyKind::Missile) {return None;}
            let observation=Observation {detection,contact:flash.body.map_or(ContactId(u32::MAX),|body|self.contact_id(faction,body)),
                sensor:observer,origin:rx,emitted_at:flash.front.t_emit,sensor_received_at:arrival,decider_received_at:arrival,
                measurement,snr,source:Source::Emission};
            let observation=self.bias_observation(observation);
            if flash.body.is_some() { self.perceptions.get_mut(&faction)?.ingest(observation,&self.system); }
            match observation.measurement {
                Measurement::BearingRange {bearing,range,..} => Some(rx+Vec2::new(bearing.cos(),bearing.sin())*range),
                Measurement::Bearing {..} => None,
            }
        };
        // An observed expendable weapon discharge ends its contact, but only
        // once the flash has reached this faction. Delayed old reports must
        // not put the spent missile back on the map.
        if !own && let Some(body) = flash.body
            && self.bodies[body.0 as usize].kind == BodyKind::Missile
            && matches!(flash.kind, CombatKind::NuclearBurst | CombatKind::BeamPulse | CombatKind::Destroyed | CombatKind::MissileHit)
        {
            let contact = self.contact_id(faction, body);
            self.refinement.retired_contacts.insert((faction, contact));
        }
        let contact=if !own && pos.is_some() {flash.body.map(|body|self.contact_id(faction,body))} else {None};
        let classified=own || contact.is_some_and(|c|self.perceptions.get(&faction).and_then(|p|p.contacts.get(&c)).is_some_and(|c|c.resolved));
        let subject_kind=flash.body.filter(|_|classified).map(|id|self.bodies[id.0 as usize].kind);
        let target=flash.target.and_then(|id| {
            if self.bodies[id.0 as usize].faction==faction {Some(InterceptTarget::Own(id))}
            else {let c=self.contact_id(faction,id);
                self.perceptions.get(&faction).and_then(|p|p.contacts.get(&c)).filter(|c|c.resolved)
                    .map(|_|InterceptTarget::Contact(c))}
        });
        Some(CombatEvent {target,velocity:if own {flash.velocity} else {None},subject_kind,impact_strength:flash.impact_strength,damage:if own {flash.damage.clone()} else {None},contact,emitted_at:flash.front.t_emit,received_at:arrival,pos,kind:flash.kind,
            // A visible beam discharge carries its beam direction, not the
            // target's identity or true position. Anchor it at the observed flash.
            aim:if own {flash.aim} else if matches!(flash.kind,CombatKind::BeamPulse|CombatKind::SpinalPulse|CombatKind::PointDefence) {
                pos.zip(flash.aim).map(|(observed,aim)|observed+(aim-flash.front.origin))
            } else {None},
            own_body:if own {flash.body} else {None}})
    }
    fn report_arrival(&self, f: FactionId, front: Front, lo: f64) -> Result<Option<f64>, ()> {
        let Some(receiver) = self.decider(f, self.time) else { return Ok(None) };
        let traj = &self.bodies[receiver.0 as usize].trajectory;
        let Some(arrived) = front.arrival(traj, lo.max(front.t_emit), self.time) else { return Ok(None) };
        let at = traj.state_at(arrived).unwrap().pos;
        if self.system.occluder(front.origin, front.t_emit, at, arrived).is_some() { Err(()) } else { Ok(Some(arrived)) }
    }
    pub fn loss_known(&self, f: FactionId, id: BodyId) -> bool {
        self.refinement.received.get(&f).is_some_and(|events| events.iter().any(|e| e.own_body == Some(id) && matches!(e.kind, CombatKind::Destroyed | CombatKind::Expended | CombatKind::Withdrawn | CombatKind::Surrendered)))
    }
    /// Remote telemetry is extrapolated from the latest packet whose light has arrived.
    pub fn known_body(&self, f: FactionId, id: BodyId) -> Option<Body> {
        if self.loss_known(f, id) || self.missile_departure_seen(f,id) { return None; }
        if self.decider(f, self.time) == Some(id) { return self.body(id).cloned(); }
        let receiver = self.decider(f, self.time)?;
        let at = self.state(receiver, self.time)?.pos;
        self.refinement.telemetry.get(&id)?.iter().rev().find(|(t, b)| {
            let pos = b.trajectory.last().pos;
            (*t + (pos - at).length() / crate::units::C <= self.time)
                && self.system.occluder(pos, *t, at, self.time).is_none()
        }).or_else(|| self.refinement.telemetry.get(&id)?.front().filter(|(at,_)| *at == 0.0)).map(|(_, b)| {
            let mut known = b.clone();
            // Command authority is scenario metadata, not remote telemetry.
            known.controllable = self.bodies[id.0 as usize].controllable;
            known
        })
    }
    /// Delivered fire-control picture; never the current remote flagship picture.
    pub fn received_picture(&self, id: BodyId) -> Option<&Perception> {
        self.uplink_picture(id).or_else(|| self.refinement.cached_pictures.get(&id).map(AsRef::as_ref))
    }
    /// Timestamp of the sending decider's picture currently reaching a platform.
    pub(super) fn received_picture_epoch(&self,id:BodyId)->Option<f64> {
        let b=self.body(id)?;let source=self.decider(b.faction,self.time)?;
        if source==id {return Some(self.time);}
        let me=self.state(id,self.time)?;
        retarded_state(&self.bodies[source.0 as usize].trajectory,me.pos,self.time).map(|(t,_)|t)
    }
    fn uplink_picture(&self, id: BodyId) -> Option<&Perception> {
        let b = self.body(id)?;
        let source = self.decider(b.faction, self.time)?;
        if source == id { return self.perceptions.get(&b.faction); }
        self.uplink_snapshot(id).map(AsRef::as_ref)
    }
    fn uplink_snapshot(&self,id:BodyId)->Option<&Arc<Perception>> {
        let b=self.body(id)?;
        let source=self.decider(b.faction,self.time)?;
        if source==id {return None;}
        let me = b.trajectory.state_at(self.time)?;
        let (emitted, src) = retarded_state(&self.bodies[source.0 as usize].trajectory, me.pos, self.time)?;
        if self.system.occluder(src.pos, emitted, me.pos, self.time).is_some() { return None; }
        self.refinement.pictures.get(&b.faction)?.iter().rev().find(|(t, _)| *t <= emitted).map(|(_, p)| p)
    }
    pub fn track_fresh(&self, id: BodyId, target: ContactId) -> bool {
        self.received_picture(id).and_then(|p| p.contacts.get(&target)).is_some_and(|c|
            c.usable_track(self.time).is_some() && self.time - c.last.decider_received_at <= TRACK_STALE_S.value
                + self.body(id).and_then(|b| self.decider(b.faction, self.time)).and_then(|d| self.state(d, self.time))
                    .zip(self.state(id, self.time)).map_or(0.0, |(a,b)| (a.pos-b.pos).length()/crate::units::C))
    }
    pub fn thermal_emission(&self, id: BodyId, t: f64) -> f64 {
        self.refinement.emission.get(&id).and_then(|h| h.iter().rev().find(|(at,_)| *at <= t)).map_or(0.0, |(_,p)| *p)
    }
    pub(super) fn historical_ecm(&self,id:BodyId,t:f64)->f64 {
        let b=&self.bodies[id.0 as usize];
        if !matches!(b.kind,BodyKind::Ship|BodyKind::Station) {return 0.0;}
        self.refinement.telemetry.get(&id).and_then(|h|h.iter().rev().find(|(at,_)|*at<=t))
            .map_or(0.0,|(_,b)|b.ecm_strength()/100.0)
    }
    pub(super) fn historical_ef(&self,id:BodyId,t:f64)->f64 {
        self.historical_signature(id,t).map_or(0.0,|f|f.value())
    }
    pub(crate) fn reset_platform_history(&mut self,id:BodyId) {
        self.refinement.telemetry.remove(&id);
        self.snapshot_platform(id,self.bodies[id.0 as usize].trajectory.start());
        self.snapshot_platform(id,self.time);
    }
    pub(super) fn snapshot_platform(&mut self,id:BodyId,t:f64) {
        let b=&self.bodies[id.0 as usize];
        if !matches!(b.kind,BodyKind::Ship|BodyKind::Station) {return;}
        let Some(state)=b.trajectory.state_at(t) else {return;};
        let mut snapshot=b.clone();
        snapshot.trajectory=Trajectory::new(t,state);
        snapshot.trajectory.set_thrust(t,b.trajectory.thrust_at(t).unwrap_or(Vec2::ZERO)).unwrap();
        let h=self.refinement.telemetry.entry(id).or_default();
        if h.back().is_some_and(|(at,_)|*at>t) {return;}
        if h.back().is_some_and(|(at,_)|*at==t) {h.pop_back();}
        h.push_back((t,snapshot));
    }
    pub(super) fn historical_signature(&self,id:BodyId,t:f64)->Option<sensors::EmissivityFactors> {
        let history=self.refinement.telemetry.get(&id)?;
        history.iter().rev().find(|(at,_)|*at<=t).or_else(||history.front().filter(|(at,_)|*at==0.0))
            .map(|(_,b)|b.emissivity_factors(t))
    }
    pub fn known_damage(&self,f:FactionId,c:ContactId)->Option<crate::damage::Report> {
        self.refinement.damage_reports.get(&(f,c)).copied()
    }
    pub(super) fn observe_damage(&mut self,obs:&Observation) {
        // Explicit game abstraction: a resolved active echo can inspect damage.
        // The report uses only the target snapshot at reflection time, and is
        // published after both the echo and any allied relay have arrived.
        if obs.detection!=sensors::DetectionLevel::Identity || !obs.decider_received_at.is_finite() || obs.decider_received_at>self.time || !matches!(obs.measurement,Measurement::BearingRange {..}) {return;}
        let f=self.bodies[obs.sensor.0 as usize].faction;
        let Some(id)=self.body_for_contact(f,obs.contact) else {return};
        let Some((at,b))=self.refinement.telemetry.get(&id).and_then(|h|h.iter().rev().find(|(at,_)|*at<=obs.emitted_at)) else {return};
        if !matches!(b.kind,BodyKind::Ship|BodyKind::Station) {return;}
        let report=crate::damage::Report {damage:b.damage,installed:b.installed_systems(),observed_at:*at,screen_available:b.screen_available()};
        let key=(f,obs.contact);
        if self.refinement.damage_reports.get(&key).is_none_or(|old|old.observed_at<report.observed_at) {self.refinement.damage_reports.insert(key,report);}
    }
    pub(super) fn tactical_frame(&mut self) {
        self.refresh_resolved_missiles();
        let t = self.time;
        for i in 0..self.bodies.len() {self.update_system_controls(BodyId(i as u32));}
        let lo = self.refinement.last_t;
        self.refinement.last_t = t;
        self.refinement.hostile_pings.retain(|_,p| t-p.received_at < HOSTILE_PING_LIFETIME_S.value);
        let mut repairs=vec![];
        for (i,b) in self.bodies.iter_mut().enumerate() {
            if matches!(b.kind,BodyKind::Ship|BodyKind::Station) && b.alive_at(t) {
                b.advance_thermal(t);
                if let Some(system)=b.damage.repair(t-lo,&mut self.rng) {repairs.push((BodyId(i as u32),system));}
                b.hull_j=(b.damage.hull_max-b.damage.hull)*crate::damage::JOULES_PER_HP;
            }
        }
        for i in 0..self.bodies.len() {
            if self.bodies[i].kind==BodyKind::Ship && self.bodies[i].alive_at(t) && self.bodies[i].thermal.heat_fraction()>0.5 {self.guide(BodyId(i as u32));}
        }
        for (id,system) in repairs {self.debug_note("REPAIR",format!("platform={id:?} system={system:?}"));self.guide(id);}
        for i in 0..self.bodies.len() {
            let b=&mut self.bodies[i];
            if !matches!(b.kind,BodyKind::Probe|BodyKind::Station) || !b.alive_at(t) { continue; }
            if b.probe_burn_until.is_some_and(|until| t>=until) {
                b.probe_burn_until=None; b.commanded=Vec2::ZERO;
                b.trajectory.set_thrust(t,Vec2::ZERO).unwrap();
            }
            if t>=b.probe_ping_at {
                b.probe_ping_at=t+if b.kind==BodyKind::Station { STATION_PING_INTERVAL_S.value } else { PROBE_PING_INTERVAL_S.value };
                self.ping(BodyId(i as u32));
            }
        }
        for packet in std::mem::take(&mut self.refinement.orders) {
            let traj = &self.bodies[packet.body.0 as usize].trajectory;
            if let Some(arrival) = packet.front.arrival(traj, lo.max(packet.front.t_emit), t) {
                let at = traj.state_at(arrival).unwrap().pos;
                if self.system.occluder(packet.front.origin, packet.front.t_emit, at, arrival).is_none() {
                    if let Err(e) = self.execute_transmitted(packet.cmd) { self.refinement.command_log.push((t, format!("Order rejected on arrival: {e:?}"))); }
                } else { self.refinement.command_log.push((t, "Order link blocked".into())); }
            } else if t - packet.front.t_emit < MAX_FRONT_RADIUS_KM / crate::units::C { self.refinement.orders.push(packet); }
        }
        for mut flash in std::mem::take(&mut self.refinement.flashes) {
            let mut pending=vec![];
            for f in std::mem::take(&mut flash.pending) {
                match self.report_arrival(f,flash.front,lo) {
                    Ok(Some(arrival)) => { if let Some(event)=self.observe_flash(f,&flash,arrival) { self.refinement.received.entry(f).or_default().push(event); } }
                    Ok(None) => pending.push(f),
                    Err(()) => {},
                }
            }
            flash.pending=pending;
            if !flash.pending.is_empty() && t-flash.front.t_emit < MAX_FRONT_RADIUS_KM/crate::units::C { self.refinement.flashes.push(flash); }
        }
        for packet in std::mem::take(&mut self.refinement.alerts) {
            match self.report_arrival(packet.faction, packet.front, lo) {
                Ok(Some(_)) => self.alert(Some(packet.faction), packet.kind),
                Ok(None) if t-packet.front.t_emit < MAX_FRONT_RADIUS_KM/crate::units::C => self.refinement.alerts.push(packet),
                _ => {},
            }
        }
        if (t / SENSOR_FRAME_S.value).fract().abs() < 1e-8 {
            self.refinement.cached_pictures.retain(|id,_|self.bodies[id.0 as usize].alive_at(t));
            for i in 0..self.bodies.len() {
                let id=BodyId(i as u32);
                if self.bodies[i].alive_at(t) && let Some(p)=self.uplink_snapshot(id).cloned() { self.refinement.cached_pictures.insert(id,p); }
            }
            for (&f,p) in &self.perceptions {
                for (&c,contact) in &p.contacts {
                    if t-contact.last.decider_received_at > TRACK_LOST_S.value {
                        if self.refinement.lost_tracks.insert((f,c)) { self.alerts.push(Alert { t, faction: Some(f),kind:AlertKind::ContactLost(c) }); }
                    } else { self.refinement.lost_tracks.remove(&(f,c)); }
                }
            }
            let cutoff = t - MAX_FRONT_RADIUS_KM/crate::units::C - SENSOR_FRAME_S.value;
            for (&f, p) in &self.perceptions {
                let h = self.refinement.pictures.entry(f).or_default();
                // Historical pictures remain immutable and shared by recipients.
                // A known retired round cannot become a firing solution again.
                let snapshot=Perception {faction:f,contacts:p.contacts.iter()
                    .filter(|(c,_)|!self.refinement.retired_contacts.contains(&(f,**c)))
                    .map(|(&c,contact)|(c,contact.clone())).collect(),log:VecDeque::new()};
                h.push_back((t, Arc::new(snapshot)));
                while h.front().is_some_and(|(at,_)| *at < cutoff) { h.pop_front(); }
            }
            for (i,b) in self.bodies.iter().enumerate() {
                if let Some(s) = b.trajectory.state_at(t) {
                    let mut snapshot = b.clone();
                    snapshot.trajectory = Trajectory::new(t,s);
                    snapshot.trajectory.set_thrust(t,b.trajectory.last().thrust).unwrap();
                    let h = self.refinement.telemetry.entry(BodyId(i as u32)).or_default();
                    h.push_back((t,snapshot));
                    while h.front().is_some_and(|(at,_)| *at < cutoff) { h.pop_front(); }
                    let h = self.refinement.emission.entry(BodyId(i as u32)).or_default();
                    h.push_back((t,b.thermal.emission()));
                    while h.front().is_some_and(|(at,_)| *at < cutoff) { h.pop_front(); }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::celestial::{Celestial, CelestialKind, Orbit};
    use crate::units::{LIGHT_SECOND, G0};
    #[test]
    fn delivered_pictures_share_storage_and_new_snapshots_omit_retired_contacts() {
        let mut w=fleet();
        w.bodies.push(w.bodies[1].clone());
        w.time=100.0;w.tactical_frame();
        w.time=120.0;w.tactical_frame();
        let a=&w.refinement.cached_pictures[&BodyId(1)];
        let b=&w.refinement.cached_pictures[&BodyId(2)];
        assert!(Arc::ptr_eq(a,b),"recipients of one picture must not copy it per missile");

        let (mut w,c)=super::super::tests::beam_trial();
        w.tactical_frame();
        let old=w.refinement.pictures[&FactionId(0)].back().unwrap().1.clone();
        assert!(old.contacts.contains_key(&c));
        w.refinement.retired_contacts.insert((FactionId(0),c));
        w.time=10.0;w.tactical_frame();
        assert!(!w.refinement.pictures[&FactionId(0)].back().unwrap().1.contacts.contains_key(&c));
        assert!(old.contacts.contains_key(&c),"retirement must not rewrite a picture already in flight");
    }
    fn fleet() -> World {
        let system = System { bodies: vec![Celestial { name: "Star".into(),kind: CelestialKind::Star,
            gm: 1.0, radius: 1.0, orbit: Orbit::Fixed(Vec2::ZERO) }] };
        let specs = (0..2).map(|i| BodySpec { name: format!("Ship {i}"), kind: BodyKind::Ship,
            faction: FactionId(0), state: State { pos: Vec2::new(AU, i as f64 *10.0*LIGHT_SECOND),vel: Vec2::ZERO },
            thrust: Vec2::ZERO,magazine: 10 }).collect();
        World::new(system,specs,60.0,1)
    }
    #[test]
    fn enemy_damage_reports_wait_for_echo_and_use_reflection_time_state() {
        let mut w=fleet();
        w.bodies[1].faction=FactionId(1);
        let c=w.contact_id(FactionId(0),BodyId(1));
        w.tactical_frame();
        let mut obs=Observation {detection:crate::sensors::DetectionLevel::Identity,contact:c,sensor:BodyId(0),origin:w.state(BodyId(0),0.0).unwrap().pos,
            emitted_at:0.0,sensor_received_at:10.0,decider_received_at:10.0,
            measurement:Measurement::BearingRange {bearing:0.0,range:10.0*LIGHT_SECOND,sigma_range:1.0,sigma_bearing:1e-5},snr:1e6,source:Source::Echo};
        w.bodies[1].damage.systems[crate::damage::System::Passive as usize]=crate::damage::Condition::Destroyed;
        w.bodies[1].screen_up=true;w.bodies[1].controls.screens=controls::Mode::On;w.bodies[1].thermal.field=0.5;
        w.time=5.0;w.observe_damage(&obs);
        assert!(w.known_damage(FactionId(0),c).is_none());
        w.time=10.0;w.observe_damage(&obs);
        assert_eq!(w.known_damage(FactionId(0),c).unwrap().damage.state(crate::damage::System::Passive),crate::damage::Condition::Intact);
        assert_eq!(w.known_damage(FactionId(0),c).unwrap().screen_available,0.0,"no live shield state leaks into an old echo");
        w.tactical_frame();
        obs.emitted_at=10.0;obs.sensor_received_at=20.0;obs.decider_received_at=30.0;
        w.time=20.0;w.observe_damage(&obs);
        assert_eq!(w.known_damage(FactionId(0),c).unwrap().observed_at,0.0);
        w.time=30.0;w.observe_damage(&obs);
        assert_eq!(w.known_damage(FactionId(0),c).unwrap().damage.state(crate::damage::System::Passive),crate::damage::Condition::Destroyed);
        assert!(w.known_damage(FactionId(0),c).unwrap().screen_available>0.0);
    }
    #[test]
    fn fresh_ping_replaces_its_previous_indication_without_duplicates() {
        let mut r=Refinements::default();
        let old=crate::session::PingSighting {contact:ContactId(1),emitted_at:0.0,received_at:10.0,
            pos:Some(Vec2::ZERO),vel:Vec2::ZERO,initial_radius:1.0,observer:Vec2::ZERO,bearing:0.0};
        r.record_ping(FactionId(0),old);
        r.record_ping(FactionId(0),crate::session::PingSighting {contact:ContactId(2),..old});
        let new=crate::session::PingSighting {emitted_at:60.0,received_at:70.0,..old};
        r.record_ping(FactionId(0),new);
        assert_eq!(r.hostile_pings.len(),2);
        assert_eq!(r.hostile_pings[&(FactionId(0),ContactId(1))].emitted_at,60.0);
        assert!(r.hostile_pings[&(FactionId(0),ContactId(1))].opacity(75.0)>0.9);
        assert_eq!(r.hostile_pings[&(FactionId(0),ContactId(2))].emitted_at,0.0);
        r.record_ping(FactionId(0),crate::session::PingSighting {received_at:80.0,..new});
        r.record_ping(FactionId(0),crate::session::PingSighting {received_at:90.0,..old});
        assert_eq!(r.hostile_pings.len(),2,"duplicate and delayed reports never create another indication");
        assert_eq!(r.hostile_pings[&(FactionId(0),ContactId(1))].received_at,70.0);
    }

    #[test]
    fn remote_orders_and_loss_reports_wait_for_light() {
        let mut w = fleet();
        let cmd = Command::SetThrust { body: BodyId(1), thrust: Vec2::new(G0,0.0) };
        assert!(w.transmit_order(BodyId(1),cmd));
        w.advance_to(9.0);
        assert_eq!(w.bodies[1].commanded,Vec2::ZERO);
        w.advance_to(11.0);
        assert_eq!(w.bodies[1].commanded,Vec2::new(G0,0.0));
        w.destroy(BodyId(1),11.0,LossCause::Impact(0));
        w.advance_to(19.0);
        assert!(!w.loss_known(FactionId(0),BodyId(1)));
        assert!(w.known_body(FactionId(0),BodyId(1)).is_some());
        w.advance_to(23.0);
        assert!(w.loss_known(FactionId(0),BodyId(1)));
        assert!(w.known_body(FactionId(0),BodyId(1)).is_none());
    }
    #[test]
    fn remote_fire_control_does_not_read_current_flagship_picture() {
        let mut w = fleet();
        w.advance_to(5.0);
        assert!(w.received_picture(BodyId(1)).is_none());
        w.advance_to(11.0);
        assert!(w.received_picture(BodyId(1)).is_some());
        w.advance_to(20.0);
        w.bodies[0].trajectory.terminate(20.0);
        w.bodies[1].kind=BodyKind::Missile; // No surviving faction command ship.
        assert!(w.received_picture(BodyId(1)).is_some(), "last delivered picture survives lost uplink");
    }
    #[test]
    fn cancellation_invalidates_events_and_refunds_reservations_only() {
        let mut w = fleet();
        w.bodies[0].missile_queued = [2,3];
        w.scheduler.schedule(2.0,Event::QueuedLaunch(BodyId(0),ContactId(999),Payload::Kinetic,0,1));
        w.cancel_launches(BodyId(0)).unwrap();
        w.advance_to(3.0);
        assert_eq!(w.bodies[0].missile_queued,[0; 2]);
        assert_eq!(w.bodies[0].magazine,[10; 2]);
        assert_eq!(w.bodies.len(),2);
    }

    #[test]
    fn station_and_missile_reports_wait_for_the_relay_leg() {
        for kind in [BodyKind::Station,BodyKind::Missile] {
            let mut w=fleet();
            w.bodies[1].kind=kind; w.bodies[1].controllable=false;
            let mut enemy=w.bodies[0].clone();
            enemy.faction=FactionId(1);
            enemy.trajectory=Trajectory::new(0.0,State {pos:w.state(BodyId(1),0.0).unwrap().pos+Vec2::new(100_000.0,0.0),vel:Vec2::ZERO});
            enemy.trajectory.set_thrust(0.0,Vec2::new(10.0*G0,0.0)).unwrap();
            w.bodies.push(enemy); w.last_step.push(0.0);
            w.perceptions.insert(FactionId(1),Perception::new(FactionId(1)));
            w.advance_to(9.0);
            assert!(w.perception(FactionId(0)).unwrap().log.iter().all(|o|o.sensor!=BodyId(1)));
            w.advance_to(40.0);
            assert!(w.perception(FactionId(0)).unwrap().log.iter().any(|o|o.sensor==BodyId(1) && o.decider_received_at-o.sensor_received_at>=9.9),"{kind:?}");
        }
    }

    #[test]
    fn reconnaissance_probe_burns_then_coasts_and_cannot_be_commanded() {
        let mut w=fleet();
        let before=w.bodies[0].probes;
        let p=w.deploy_probe(BodyId(0),Vec2::new(0.0,1.0)).unwrap();
        assert_eq!(w.bodies[0].probes,before-1);
        assert!(!w.bodies[p.0 as usize].controllable);
        w.advance_to(PROBE_BURN_S.value-1.0);
        assert!(w.bodies[p.0 as usize].trajectory.last().thrust.length()>400.0*G0);
        w.advance_to(PROBE_BURN_S.value+1.0);
        assert_eq!(w.bodies[p.0 as usize].trajectory.last().thrust,Vec2::ZERO);
        assert!(w.state(p,w.time()).unwrap().vel.length()>2000.0);
        assert!(w.ping_emissions.iter().any(|(id,_)|*id==p));
    }

    #[test]
    fn probe_observations_relay_home_at_light_speed() {
        let mut w=fleet();
        // An enemy starts beside the distant launching ship, ten light-seconds
        // from the command ship. Its bright drive is visible to the probe.
        let mut enemy=w.bodies[1].clone();
        enemy.faction=FactionId(1); enemy.name="Burner".into();
        enemy.trajectory=Trajectory::new(0.0,State {pos:w.state(BodyId(1),0.0).unwrap().pos+Vec2::new(100_000.0,0.0),vel:Vec2::ZERO});
        enemy.trajectory.set_thrust(0.0,Vec2::new(10.0*G0,0.0)).unwrap();
        w.bodies.push(enemy); w.last_step.push(0.0);
        w.perceptions.insert(FactionId(1),Perception::new(FactionId(1)));
        let p=w.deploy_probe(BodyId(1),Vec2::new(1.0,0.0)).unwrap();
        w.advance_to(19.0);
        assert!(w.perception(FactionId(0)).unwrap().log.iter().all(|o|o.sensor!=p));
        w.advance_to(40.0);
        assert!(w.perception(FactionId(0)).unwrap().log.iter().any(|o|o.sensor==p && o.decider_received_at-o.sensor_received_at>9.0));
    }

    #[test]
    fn fixed_sensor_bias_is_not_drawn_at_truth_or_averaged_away() {
        let mut w=fleet();
        let obs=Observation {detection:crate::sensors::DetectionLevel::Approximate,contact:ContactId(77),sensor:BodyId(0),origin:Vec2::new(AU,0.0),
            emitted_at:0.0,sensor_received_at:0.0,decider_received_at:0.0,
            measurement:Measurement::BearingRange {bearing:0.0,range:0.1*AU,sigma_bearing:0.005,sigma_range:100_000.0},
            snr:9.0,source:Source::Emission};
        let biased=w.bias_observation(obs);
        assert_eq!(biased.measurement,obs.measurement); // bias belongs to the fused position estimate
        assert_eq!(biased.measurement,w.bias_observation(obs).measurement);
        for _ in 0..50 { w.perceptions.get_mut(&FactionId(0)).unwrap().ingest(biased,&w.system); }
        let tr=w.perceptions[&FactionId(0)].contacts[&obs.contact].estimate(0.0,&w.system).unwrap();
        assert!((tr.pos()-(obs.origin+Vec2::new(0.1*AU,0.0))).length()>100.0);
        assert!(tr.pos_cov()[0][0].sqrt()>50_000.0,"repeated frames must not create a precision fix");
    }

    #[test]
    fn missile_discharge_retires_contact_only_after_flash_arrives() {
        for kind in [CombatKind::NuclearBurst, CombatKind::BeamPulse] {
            let mut w = fleet();
            w.bodies[1].faction = FactionId(1);
            w.bodies[1].kind = BodyKind::Missile;
            let contact = w.contact_id(FactionId(0), BodyId(1));
            let pos = w.state(BodyId(1), 0.0).unwrap().pos;
            w.record_combat(0.0, pos, kind, Some(BodyId(1)), None);
            w.advance_to(9.0);
            assert!(!w.contact_retired(FactionId(0), contact));
            w.advance_to(11.0);
            assert!(w.contact_retired(FactionId(0), contact));
            w.advance_to(21.0);
            assert!(w.contact_retired(FactionId(0), contact), "later reports cannot resurrect a spent contact");
        }
        let mut w = fleet();
        w.bodies[1].faction = FactionId(1);
        let contact = w.contact_id(FactionId(0), BodyId(1));
        let pos = w.state(BodyId(1), 0.0).unwrap().pos;
        w.record_combat(0.0, pos, CombatKind::BeamPulse, Some(BodyId(1)), None);
        w.advance_to(11.0);
        assert!(!w.contact_retired(FactionId(0), contact), "ships survive their beam discharge");
    }

    #[test] fn ship_destruction_category_arrives_with_the_flash_not_before() {
        let mut w=fleet();w.bodies[1].faction=FactionId(1);
        let pos=w.state(BodyId(1),0.0).unwrap().pos;
        w.record_combat(0.0,pos,CombatKind::Destroyed,Some(BodyId(1)),None);
        w.advance_to(9.0);
        assert!(!w.refinement.received.get(&FactionId(0)).is_some_and(|events|events.iter().any(|e|e.kind==CombatKind::Destroyed)));
        w.advance_to(11.0);
        let event=w.refinement.received[&FactionId(0)].iter().find(|e|e.kind==CombatKind::Destroyed).unwrap();
        assert_eq!(event.subject_kind,Some(BodyKind::Ship));assert!(event.own_body.is_none());
        assert!(event.received_at>=10.0);assert!(event.contact.is_some());
    }

    #[test]
    fn observed_enemy_beam_keeps_geometry_without_exposing_identity() {
        let mut w=fleet();
        w.bodies[1].faction=FactionId(1);
        let origin=w.state(BodyId(1),0.0).unwrap().pos;
        let aim=w.state(BodyId(0),0.0).unwrap().pos;
        w.bodies[1].last_beam=Some((0.0,origin,aim));
        w.record_beam(0.0,origin,CombatKind::BeamPulse,BodyId(1),BodyId(0),FactionId(1));
        w.advance_to(9.0);
        assert!(w.combat_events(Some(FactionId(0))).is_empty());
        w.advance_to(11.0);
        let events=w.combat_events(Some(FactionId(0)));
        let event=events.iter().find(|e|e.kind==CombatKind::BeamPulse).unwrap();
        assert!(event.received_at>=9.99);
        assert!(event.own_body.is_none());
        let observed=event.pos.expect("visible source");
        let endpoint=event.aim.expect("enemy beam must be drawable");
        assert_eq!(event.target,Some(InterceptTarget::Own(BodyId(0))));
        assert!(((endpoint-observed)-(aim-origin)).length()<1e-6);
    }

    #[test]
    fn unseen_flash_does_not_leak_and_occlusion_discards_it() {
        let mut w=fleet();
        let pos=w.state(BodyId(1),0.0).unwrap().pos;
        w.record_combat(0.0,pos,CombatKind::NuclearBurst,None,None);
        w.advance_to(9.0);
        assert!(w.combat_events(Some(FactionId(0))).is_empty());
        w.advance_to(11.0);
        assert_eq!(w.combat_events(Some(FactionId(0))).len(),1);
        let mut w=fleet();
        let pos=w.state(BodyId(1),0.0).unwrap().pos;
        w.system.bodies[0].orbit=Orbit::Fixed((pos+w.state(BodyId(0),0.0).unwrap().pos)*0.5);
        w.system.bodies[0].radius=1000.0;
        w.record_combat(0.0,pos,CombatKind::NuclearBurst,None,None);
        w.advance_to(11.0);
        w.system.bodies[0].orbit=Orbit::Fixed(Vec2::ZERO);
        w.advance_to(20.0);
        assert!(w.combat_events(Some(FactionId(0))).is_empty(), "blocked pulse cannot reappear after occluder moves");
    }
}
