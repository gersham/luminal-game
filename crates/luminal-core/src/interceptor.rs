//! Short-endurance, autonomous point-defence missiles. Launch decisions use local
//! observations; world truth is used only for physical pass/kill resolution.
use super::*;
use crate::units::{C,G0,LIGHT_SECOND};

#[derive(Clone,Copy,Debug)]
pub struct Battery {pub rounds:u32,pub launched:u32,pub ready_at:f64,pub status:&'static str}
#[derive(Clone,Copy,Debug)]
pub struct Interceptor {
    pub launcher:BodyId,pub target:BodyId,pub expires:f64,pub dv_left:f64,
    pub last_update:f64,pub solution:sensors::SeekerFix,pub last_range:f64,
}
pub fn hit_probability(relative_speed:f64)->f64 {
    if relative_speed>=INTERCEPTOR_MAX_SPEED_C.value*C {return 0.0;}
    0.60/(1.0+(relative_speed.max(0.0)/(INTERCEPTOR_HALF_SPEED_C.value*C)).powi(4))
}
/// Outer kinematic envelope against a zero-relative-velocity target.
pub fn nominal_range()->f64 {reach(INTERCEPTOR_LIFETIME_S.value).min(INTERCEPTOR_RANGE_LS.value*LIGHT_SECOND)}
fn reach(t:f64)->f64 {
    let burn=t.min(INTERCEPTOR_BURN_S.value);
    INTERCEPTOR_ACCEL_G.value*G0*burn*(t-0.5*burn)
}
/// Earliest reachable coasting solution, capped by endurance, range and encounter
/// speed. No current target truth state is supplied to this calculation.
pub fn engagement(own:State,target:State)->Option<f64> {
    let r=target.pos-own.pos; let v=target.vel-own.vel;
    if r.length()>INTERCEPTOR_RANGE_LS.value*LIGHT_SECOND {return None;}
    for step in 1..=(INTERCEPTOR_BURN_S.value.min(INTERCEPTOR_LIFETIME_S.value)*4.0) as usize {
        let t=step as f64*0.25;
        let aim=r+v*t;
        if aim.length()<=reach(t) {
            let velocity=aim.normalized()*(INTERCEPTOR_ACCEL_G.value*G0*t.min(INTERCEPTOR_BURN_S.value));
            if hit_probability((velocity-v).length())==0.0 {return None;}
            return Some(t);
        }
    }
    // After burnout reach grows linearly. Solve the coast intersection directly
    // rather than scanning hours of endurance every fire-control cycle.
    let burn=INTERCEPTOR_BURN_S.value;
    let speed=INTERCEPTOR_ACCEL_G.value*G0*burn;
    let a=v.dot(v)-speed*speed;
    let b=2.0*r.dot(v)+speed*speed*burn;
    let c=r.dot(r)-0.25*speed*speed*burn*burn;
    let mut roots=if a.abs()<1e-9 {
        vec![if b.abs()>1e-9 {-c/b} else {f64::INFINITY}]
    } else {
        let disc=b*b-4.0*a*c;
        if disc<0.0 {return None;}
        vec![(-b-disc.sqrt())/(2.0*a),(-b+disc.sqrt())/(2.0*a)]
    };
    roots.sort_by(f64::total_cmp);
    roots.into_iter().find(|&t| {
        if !t.is_finite() || t<burn || t>INTERCEPTOR_LIFETIME_S.value {return false;}
        let aim=r+v*t;
        aim.length()<=reach(t)+1e-3 && hit_probability((aim.normalized()*speed-v).length())>0.0
    })
}

impl World {
    /// Launch and end-of-flight reports propagate at c. A remote commitment
    /// cannot suppress local defence before its report is actually received.
    fn interceptor_committed(&self, receiver:BodyId, target:BodyId, before:Option<BodyId>)->bool {
        let rx=&self.bodies[receiver.0 as usize];
        let heard=|trajectory:&Trajectory, time:f64| {
            let Some(segment)=trajectory.segments().iter().rev().find(|s|s.t0<=time) else {return false};
            let origin=segment.state_at(time).pos;
            let front=Front {origin,t_emit:time};
            front.arrival(&rx.trajectory,time.max(rx.trajectory.start()),self.time).is_some_and(|arrival| {
                rx.trajectory.state_at(arrival).is_some_and(|at|
                    self.system.occluder(origin,time,at.pos,arrival).is_none())
            })
        };
        self.bodies.iter().enumerate().any(|(index,b)| {
            let id=BodyId(index as u32);
            let Some(i)=b.interceptor else {return false};
            if before.is_some_and(|limit|id>=limit) || b.faction!=rx.faction || i.target!=target {return false;}
            let launched=i.launcher==receiver || heard(&b.trajectory,b.trajectory.start());
            let ended=b.trajectory.end().is_some_and(|end|heard(&b.trajectory,end));
            launched && !ended && self.time<i.expires
        })
    }

    pub(super) fn consider_interceptors(&mut self,id:BodyId,sightings:&[point_defence::Sighting]) {
        let t=self.time;
        let Some(battery)=self.bodies[id.0 as usize].interceptor_battery else {return};
        if self.bodies[id.0 as usize].operating_effectiveness(crate::damage::System::PdMissiles)==0.0 {
            self.bodies[id.0 as usize].interceptor_battery.as_mut().unwrap().status="Launcher disabled";
            return;
        }
        // Keep observing even while reloading or empty; later evidence improves
        // velocity rather than treating a single direction as a firing solution.
        for &(_,target,pos,emitted,_,_) in sightings {
            let key=(id,target);
            let previous=self.interceptor_solutions.get(&key).copied();
            if previous.is_none_or(|p|emitted>p.t) {
                self.interceptor_solutions.insert(key,sensors::SeekerFix::update(previous,emitted,pos,Vec2::ZERO));
            }
        }
        let mut status=if battery.rounds==0 {"Magazine empty"} else if t<battery.ready_at {"Reloading"} else {"No local missile contacts"};
        if battery.rounds==0 || t<battery.ready_at {
            self.bodies[id.0 as usize].interceptor_battery.as_mut().unwrap().status=status;
            return;
        }
        let own=self.state(id,t).unwrap();
        let candidate=sightings.iter().filter_map(|(_,target,_,_,_,_)| {
            if self.bodies[target.0 as usize].interceptor.is_some() {return None;}
            let fix=*self.interceptor_solutions.get(&(id,*target))?;
            if fix.samples<4 {status="Acquiring local firing solution";return None;}
            if self.interceptor_committed(id,*target,None) {status="Allied interceptor committed";return None;}
            let state=State {pos:fix.pos+fix.vel*(t-fix.t),vel:fix.vel};
            if (state.pos-own.pos).length()<5.0*LIGHT_SECOND {
                status="Inside 5 ls - laser defence";
                return None;
            }
            status="No reachable intercept";
            Some((engagement(own,state)?,*target,fix))
        }).min_by(|a,b|a.0.total_cmp(&b.0));
        self.bodies[id.0 as usize].interceptor_battery.as_mut().unwrap().status=status;
        if status!=battery.status {self.debug_note("PD_DECISION",format!("platform={id:?} status={status} contacts={} rounds={}",sightings.len(),battery.rounds));}
        if let Some((_,target,fix))=candidate {self.launch_interceptor(id,target,fix);}
    }

    fn launch_interceptor(&mut self,launcher:BodyId,target:BodyId,solution:sensors::SeekerFix)->BodyId {
        let t=self.time;
        let id=BodyId(self.bodies.len() as u32);
        let carrier=&mut self.bodies[launcher.0 as usize];
        let effectiveness=carrier.operating_effectiveness(crate::damage::System::PdMissiles);
        let battery=carrier.interceptor_battery.as_mut().unwrap();
        battery.rounds-=1; battery.launched+=1; battery.ready_at=t+INTERCEPTOR_LAUNCH_INTERVAL_S.value/effectiveness;
        battery.status="Interceptor away";
        let number=battery.launched;
        let mut body=carrier.clone();
        body.damage=crate::damage::Damage::default();
        body.name=format!("{} interceptor {number}",carrier.name);
        body.kind=BodyKind::Missile; body.controllable=false; body.armed=false;
        body.ship_class=None;
        body.has_screen=false; body.screen_up=false; body.screen_j=0.0; body.hull_j=0.0;
        body.point_defence=None; body.interceptor_battery=None; body.missile=None;
        body.probes=0; body.probe_burn_until=None; body.probe_ping_at=t;
        body.magazine=[0; 2]; body.missile_queued=[0; 2]; body.beam_target=None; body.last_beam=None;
        body.autopilot=None; body.commanded=Vec2::ZERO; body.baseline_emission_factor=1.0;
        body.sensors=sensors::SensorSuite::MISSILE;
        body.thermal=crate::thermal::Thermal {capacitor_j:0.0,last_t:t,..Default::default()};
        body.avoidance=Avoidance {thrust:Vec2::ZERO,active:false,impossible:false};
        body.trajectory=Trajectory::new(t,carrier.trajectory.state_at(t).unwrap());
        body.interceptor=Some(Interceptor {launcher,target,expires:t+INTERCEPTOR_LIFETIME_S.value,
            dv_left:INTERCEPTOR_ACCEL_G.value*G0*INTERCEPTOR_BURN_S.value,last_update:t,solution,last_range:f64::INFINITY});
        self.bodies.push(body); self.last_step.push(t);
        self.report_launch(id);
        self.scheduler.schedule(t,Event::Step(id));
        self.scheduler.schedule(t,Event::InterceptorGuide(id));
        id
    }

    pub(super) fn guide_interceptor(&mut self,id:BodyId) {
        let t=self.time;
        let Some(me)=self.state(id,t) else {return};
        let Some(mut defence)=self.bodies[id.0 as usize].interceptor else {return};
        // Simultaneous launches are possible before reports arrive. The later
        // interceptor yields when it learns of the earlier allied commitment.
        if self.interceptor_committed(id,defence.target,Some(id)) {
            self.destroy(id,t,LossCause::Expended);
            return;
        }
        let target_trajectory=&self.bodies[defence.target.0 as usize].trajectory;
        if t>defence.last_update {
            let own=&self.bodies[id.0 as usize].trajectory;
            let separation=|at|Some(target_trajectory.state_at(at)?.pos-own.state_at(at)?.pos);
            if let Some((at,distance))=missile::closest_approach(defence.last_update,t,separation) {
                let radial=|tau| {
                    let target=target_trajectory.state_at(tau)?;
                    let missile=own.state_at(tau)?;
                    Some((target.pos-missile.pos).dot(target.vel-missile.vel))
                };
                let passed=(at>defence.last_update+1e-9 && separation(t).is_some_and(|v|v.length()>distance+1e-3))
                    || (radial(defence.last_update).is_some_and(|r|r<0.0) && radial(t).is_some_and(|r|r>=0.0));
                if distance<=INTERCEPTOR_KILL_RADIUS_KM.value || passed {
                    let speed=(target_trajectory.state_at(at).unwrap().vel-own.state_at(at).unwrap().vel).length();
                    let kill=distance<=INTERCEPTOR_KILL_RADIUS_KM.value && self.rng.uniform()<hit_probability(speed);
                    self.debug_note("INTERCEPT",format!("missile={id:?} target={:?} pass_time={at:.6} miss_km={distance} speed_kms={speed} fuel_kms={} chance={} kill={kill}",defence.target,defence.dv_left,hit_probability(speed)));
                    self.destroy(id,at,LossCause::Expended);
                    if kill && self.bodies[defence.target.0 as usize].alive_at(t) {
                        self.destroy(defence.target,at,LossCause::Interceptor {missile:id});
                    }
                    return;
                }
            }
        }
        if t>=defence.expires {self.destroy(id,t,LossCause::Expended);return;}
        defence.dv_left=(defence.dv_left-self.bodies[id.0 as usize].trajectory.thrust_impulse(defence.last_update,t)).max(0.0);
        // Local passive seeker: observe retarded position, fit velocity from samples,
        // and relay the observation home. No truth velocity enters guidance.
        if let Some((emitted,seen))=retarded_state(target_trajectory,me.pos,t)
            && self.system.occluder(seen.pos,emitted,me.pos,t).is_none() {
            let target=&self.bodies[defence.target.0 as usize];
            let power=emission_w(target.kind,target.baseline_emission_factor,target_trajectory.thrust_at(emitted).unwrap_or(Vec2::ZERO));
            let range=(seen.pos-me.pos).length();
            let snr=sensors::intensity(power,range)/PD_SENSOR_NOISE_FLOOR.value;
            if snr>=PASSIVE_DETECT_SNR.value && emitted>defence.solution.t {
                let bearing=bearing_of(seen.pos-me.pos)+LASER_POINTING_RAD.value*self.rng.gaussian();
                let measured=(range+SEEKER_RANGE_SIGMA_KM.value*self.rng.gaussian()).max(0.0);
                let pos=me.pos+Vec2::new(bearing.cos(),bearing.sin())*measured;
                defence.solution=sensors::SeekerFix::update(Some(defence.solution),emitted,pos,defence.solution.vel);
                let faction=self.bodies[id.0 as usize].faction;
                let contact=self.contact_id(faction,defence.target);
                self.relays.push(Relay {faction,front:Front {origin:me.pos,t_emit:t},obs:Observation {contact,sensor:id,origin:me.pos,
                    emitted_at:emitted,sensor_received_at:t,decider_received_at:f64::NAN,source:Source::Emission,snr,
                    measurement:Measurement::BearingRange {bearing,sigma_bearing:LASER_POINTING_RAD.value,range:measured,sigma_range:SEEKER_RANGE_SIGMA_KM.value}}});
            }
        }
        let fix=defence.solution.accelerating();
        let estimate=State {pos:fix.pos+fix.vel*(t-fix.t),vel:fix.vel};
        let range=(estimate.pos-me.pos).length();
        let (time_left,miss)=missile::zero_effort_miss(me,estimate,Vec2::ZERO);
        if t>=self.bodies[id.0 as usize].probe_ping_at {
            self.ping(id); self.bodies[id.0 as usize].probe_ping_at=t+MISSILE_ACTIVE_INTERVAL_S.value;
        }
        // Cruise observations need a useful velocity-fit baseline, not ten noisy
        // fits per second. Keep fine physical pass checks in the final ten seconds.
        let dt=(if time_left>10.0 {1.0} else {INTERCEPTOR_GUIDE_S.value}).min(defence.expires-t);
        let accel=INTERCEPTOR_ACCEL_G.value*G0;
        // A long-range head-on intercept can have zero lateral miss while still
        // needing a departure burn. The old boost-only solver returned None
        // beyond the fuel duration, so ZEM guidance left the interceptor parked.
        // Accelerate toward the reachable boost-and-coast intercept, retaining
        // half of propulsion for terminal corrections after the initial boost.
        let reserve=0.5*accel*INTERCEPTOR_BURN_S.value;
        let coast_aim=if defence.dv_left>reserve && (time_left<=0.0 || time_left>defence.dv_left/accel) {
            engagement(me,estimate).map(|eta|estimate.pos+estimate.vel*eta-me.pos-me.vel*eta)
        } else {None};
        let desired=if let Some(aim)=coast_aim {aim.normalized()*accel}
            else if time_left>0.0 {miss*(3.0/time_left.max(dt).powi(2))}
            else {(estimate.pos-me.pos).normalized()*accel};
        // Do not spend the entire terminal reserve chasing a noisy distant fit.
        // This budget converges toward full authority as closest approach nears.
        let correction_budget=if coast_aim.is_none() && time_left>10.0 {defence.dv_left/(0.5*time_left)} else {defence.dv_left/dt};
        let limit=accel.min(correction_budget);
        let thrust=if desired.length()>limit {desired.normalized()*limit} else {desired};
        if (t/100.0).floor()>(defence.last_update/100.0).floor() {
            self.debug_note("INTERCEPT_GUIDANCE",format!("missile={id:?} target={:?} range_km={range} tgo_s={time_left} fuel_kms={} fit_age_s={} desired_accel={} applied_accel={}",defence.target,defence.dv_left,t-fix.t,desired.length(),thrust.length()));
        }
        defence.last_range=range; defence.last_update=t;
        let body=&mut self.bodies[id.0 as usize]; body.interceptor=Some(defence);
        body.trajectory.set_thrust(t,thrust).unwrap();
        self.scheduler.schedule(t+dt,Event::InterceptorGuide(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::celestial::{Celestial,CelestialKind,Orbit,System};
    fn fixture(seed:u64)->World {
        let system=System {bodies:vec![Celestial {name:"Star".into(),kind:CelestialKind::Star,gm:1.0,radius:1.0,orbit:Orbit::Fixed(Vec2::ZERO)}]};
        let specs=[BodyKind::Ship,BodyKind::Missile].into_iter().enumerate().map(|(i,kind)|BodySpec {
            name:format!("Platform {i}"),kind,faction:FactionId(i as u8),
            state:State {pos:Vec2::new(20.0*AU,i as f64*5000.0),vel:Vec2::ZERO},thrust:Vec2::ZERO,magazine:0}).collect();
        let mut w=World::new(system,specs,10.0,seed);
        w.bodies[0].interceptor_battery=Some(Battery {rounds:20,launched:0,ready_at:0.0,status:"Ready"});
        w
    }
    #[test]
    fn defensive_interceptors_are_not_interceptor_targets() {
        let mut w=fixture(123);
        let target=w.state(BodyId(1),0.0).unwrap();
        let fix=sensors::SeekerFix::update(None,0.0,target.pos,target.vel);
        let other=w.launch_interceptor(BodyId(0),BodyId(1),fix);
        w.bodies[other.0 as usize].faction=FactionId(1);
        let before=w.bodies[0].interceptor_battery.unwrap().launched;
        let pos=w.state(BodyId(0),0.0).unwrap().pos+Vec2::new(10.0*LIGHT_SECOND,0.0);
        for t in 1..10 {
            w.time=t as f64;
            let sighting=(10.0*LIGHT_SECOND,other,pos,w.time,Measurement::BearingRange {bearing:0.0,range:10.0*LIGHT_SECOND,sigma_range:1.0,sigma_bearing:1e-9},1e9);
            w.consider_interceptors(BodyId(0),&[sighting]);
        }
        assert_eq!(w.bodies[0].interceptor_battery.unwrap().launched,before);
    }

    #[test]
    fn destroyed_target_retires_its_interceptors_immediately() {
        let mut w=fixture(123);
        let target=w.state(BodyId(1),0.0).unwrap();
        let fix=sensors::SeekerFix::update(None,0.0,target.pos,target.vel);
        let id=w.launch_interceptor(BodyId(0),BodyId(1),fix);
        assert!(w.bodies[id.0 as usize].alive_at(0.0));
        w.destroy(BodyId(1),0.0,LossCause::Expended);
        assert!(!w.bodies[id.0 as usize].alive_at(1e-6));
        assert_eq!(w.bodies[id.0 as usize].trajectory.end(),Some(0.0));
        let losses=w.losses.len();
        w.advance_to(2.0);
        assert_eq!(w.losses.len(),losses,"queued guidance cannot revive or retire it twice");
        assert!(w.bodies[0].alive_at(w.time()));
    }

    #[test]
    fn reach_and_launch_gate_cover_point_three_au() {
        let current=reach(INTERCEPTOR_LIFETIME_S.value);
        assert!((current/(0.27*AU)-1.0).abs()<1e-10);
        assert!((nominal_range()/AU-0.27).abs()<1e-10);
        assert!(engagement(State {pos:Vec2::ZERO,vel:Vec2::ZERO},
            State {pos:Vec2::new(current*0.999,0.0),vel:Vec2::ZERO}).is_some());
    }
    #[test]
    fn long_range_head_on_interceptor_actually_boosts() {
        let mut w=fixture(123);
        let origin=w.state(BodyId(0),0.0).unwrap().pos;
        let target=State {pos:origin+Vec2::new(0.25*AU,0.0),vel:Vec2::new(-0.06*C,0.0)};
        w.bodies[1].trajectory=Trajectory::new(0.0,target);
        let fix=sensors::SeekerFix::update(None,0.0,target.pos,target.vel);
        let id=w.launch_interceptor(BodyId(0),BodyId(1),fix);
        w.advance_to(10.0);
        assert!(w.state(id,w.time()).unwrap().vel.x>100.0,"must accelerate into the coast intercept, not wait at the carrier");
    }
    #[test]
    fn long_range_coasting_threat_can_be_physically_intercepted() {
        let mut kills=0;
        for seed in 0..4 {
            let mut w=fixture(seed);
            let origin=w.state(BodyId(0),0.0).unwrap().pos;
            let target=State {pos:origin+Vec2::new(0.25*AU,0.0),vel:Vec2::new(-0.06*C,0.0)};
            w.bodies[1].trajectory=Trajectory::new(0.0,target);
            let fix=sensors::SeekerFix::update(None,0.0,target.pos,target.vel);
            let id=w.launch_interceptor(BodyId(0),BodyId(1),fix);
            w.advance_to(2300.0);
            assert!(!w.bodies[id.0 as usize].alive_at(w.time()));
            if w.losses.iter().any(|l|l.body==BodyId(1) && matches!(l.cause,LossCause::Interceptor {..})) {kills+=1;}
        }
        assert!(kills>0,"long-range guidance must actually reach the kill envelope");
    }
    #[test]
    fn long_range_local_fire_control_can_kill_a_coasting_threat() {
        let mut kills=0;
        for seed in 0..16 {
            let mut w=fixture(seed);
            if let Some(path)=std::env::var_os("LUMINAL_INTERCEPT_TEST_LOG") {w.enable_debug_log(std::path::Path::new(&path)).unwrap();}
            let origin=w.state(BodyId(0),0.0).unwrap().pos;
            w.bodies[1].trajectory=Trajectory::new(0.0,State {pos:origin+Vec2::new(0.25*AU,0.0),vel:Vec2::new(-0.06*C,0.0)});
            w.fit_point_defence(BodyId(0));
            w.advance_to(2300.0);
            if w.losses.iter().any(|l|l.body==BodyId(1) && matches!(l.cause,LossCause::Interceptor {..})) {kills+=1;}
        }
        assert!(kills>=6,"received local measurements must support long-range interception: {kills}/16");
    }

    #[test]
    fn allied_commitments_arrive_at_light_speed_and_release_after_loss_report() {
        let mut w=fixture(123);
        let mut ally=w.bodies[0].clone();
        let origin=w.state(BodyId(0),0.0).unwrap().pos;
        ally.trajectory=Trajectory::new(0.0,State {pos:origin+Vec2::new(LIGHT_SECOND,0.0),vel:Vec2::ZERO});
        w.bodies.push(ally); w.last_step.push(0.0);
        let fix=sensors::SeekerFix::update(None,0.0,w.state(BodyId(1),0.0).unwrap().pos,Vec2::ZERO);
        let missile=w.launch_interceptor(BodyId(0),BodyId(1),fix);
        assert!(w.interceptor_committed(BodyId(0),BodyId(1),None));
        assert!(!w.interceptor_committed(BodyId(2),BodyId(1),None));
        w.time=1.1;
        assert!(w.interceptor_committed(BodyId(2),BodyId(1),None));
        let duplicate=w.launch_interceptor(BodyId(2),BodyId(1),fix);
        w.guide_interceptor(duplicate);
        assert!(w.bodies[duplicate.0 as usize].trajectory.end().is_some());
        w.destroy(missile,1.1,LossCause::Expended);
        assert!(w.interceptor_committed(BodyId(2),BodyId(1),None),"unreceived loss must not release commitment");
        w.time=2.2;
        assert!(!w.interceptor_committed(BodyId(2),BodyId(1),None));
    }

    #[test]
    fn envelope_rejects_unreachable_and_high_speed_targets() {
        let own=State {pos:Vec2::ZERO,vel:Vec2::ZERO};
        assert!(engagement(own,State {pos:Vec2::new(5000.0,0.0),vel:Vec2::ZERO}).is_some());
        assert!(engagement(own,State {pos:Vec2::new(0.31*AU,0.0),vel:Vec2::ZERO}).is_none());
        assert!(engagement(own,State {pos:Vec2::new(LIGHT_SECOND,0.0),vel:Vec2::new(0.1*C,0.0)}).is_none());
        assert!(engagement(own,State {pos:Vec2::new(0.27*AU,0.0),vel:Vec2::new(-0.12*C,0.0)}).is_some());
        assert!(engagement(own,State {pos:Vec2::new(LIGHT_SECOND,0.0),vel:Vec2::new(-0.5*C,0.0)}).is_none());
        assert_eq!(hit_probability(0.0),0.60);
        assert!((hit_probability(0.25*C)-0.30).abs()<1e-12);
        assert!(hit_probability(0.12*C)>0.55);
        assert_eq!(hit_probability(0.5*C),0.0);
    }
    #[test]
    fn automatic_launch_reserves_stock_and_never_launches_duplicates() {
        let mut w=fixture(123);
        let range=6.0*LIGHT_SECOND;
        let origin=w.state(BodyId(0),0.0).unwrap().pos;
        w.bodies[1].trajectory=Trajectory::new(0.0,State {pos:origin+Vec2::new(range,0.0),vel:Vec2::ZERO});
        for t in 0..5 {
            w.advance_to(t as f64);
            let pos=w.state(BodyId(1),w.time()).unwrap().pos;
            let obs=(range,BodyId(1),pos,w.time(),Measurement::BearingRange {bearing:0.0,sigma_bearing:0.0,range,sigma_range:0.1},1e9);
            w.consider_interceptors(BodyId(0),&[obs]);
        }
        assert_eq!(w.bodies[0].interceptor_battery.unwrap().rounds,19);
        assert_eq!(w.bodies[0].interceptor_battery.unwrap().launched,1);
        assert_eq!(w.bodies.len(),3);
        assert!(!w.bodies[2].controllable);
        assert_eq!(w.bodies[2].kind,BodyKind::Missile);
        w.advance_to(INTERCEPTOR_LIFETIME_S.value+10.0);
        assert!(!w.bodies[2].alive_at(w.time()),"missed interceptors retire");
        assert!(w.bodies[2].interceptor.unwrap().dv_left>=0.0);
    }
    #[test]
    fn physical_interceptors_can_kill_and_can_miss_at_low_closure() {
        let mut kills=0;
        for seed in 0..40 {
            let mut w=fixture(seed);
            let pos=w.state(BodyId(1),0.0).unwrap().pos;
            let fix=sensors::SeekerFix::update(None,0.0,pos,Vec2::ZERO);
            let id=w.launch_interceptor(BodyId(0),BodyId(1),fix);
            w.advance_to(INTERCEPTOR_LIFETIME_S.value+1.0);
            assert!(!w.bodies[id.0 as usize].alive_at(w.time()));
            if w.losses.iter().any(|l|l.body==BodyId(1) && matches!(l.cause,LossCause::Interceptor {..})) {kills+=1;}
        }
        assert!((10..35).contains(&kills),"physical interception plus a 60% terminal roll must exercise both outcomes, got {kills}/40");
    }
}
