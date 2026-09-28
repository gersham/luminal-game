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
    let delta=target.pos-own.pos;
    let range=delta.length();
    let relative=target.vel-own.vel;
    if range>nominal_range() || hit_probability(relative.length())<=0.0 {return None;}
    let closing=-relative.dot(delta.normalized());
    let speed=0.1*C;
    if speed+closing<=0.0 {return None;}
    let seconds=(range/(speed+closing)).max(range/C).max(0.1);
    (seconds<=INTERCEPTOR_LIFETIME_S.value).then_some(seconds)
}

impl World {
    /// Launch and end-of-flight reports propagate at c. A remote commitment
    /// cannot suppress local defence before its report is actually received.
    pub(super) fn interceptor_committed(&self, receiver:BodyId, target:BodyId, before:Option<BodyId>)->bool {
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
        carrier.last_missile_launch=Some(t);
        let mut body=carrier.clone();
        body.last_missile_launch=None;
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
        self.start_probability_interceptor(id,solution);
        self.scheduler.schedule(t,Event::InterceptorGuide(id));
        id
    }

    pub(super) fn guide_interceptor(&mut self,id:BodyId) {
        self.guide_probability_weapon(id);
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
    fn interceptors_make_no_bearing_contacts_but_still_allow_resolved_points() {
        let mut w=fixture(123);
        let target=w.state(BodyId(1),0.0).unwrap();
        let fix=sensors::SeekerFix::update(None,0.0,target.pos,target.vel);
        let id=w.launch_interceptor(BodyId(0),BodyId(1),fix);
        w.bodies[id.0 as usize].faction=FactionId(1);
        w.bodies[id.0 as usize].trajectory=Trajectory::new(-10.0,target);
        w.bodies[0].sensors=sensors::SensorSuite {passive:false,active:false,direction_finding:true};
        w.sensor_frame();
        assert!(!w.contact_truth(FactionId(0)).values().any(|body|*body==id),"DF must not allocate an interceptor track");
        assert!(w.contact_truth(FactionId(0)).values().any(|body|*body==BodyId(1)),"ordinary missiles retain bearings");
        w.bodies[0].sensors=sensors::SensorSuite::FULL;
        w.sensor_frame();
        let contact=w.contact_truth(FactionId(0)).into_iter().find_map(|(c,b)|(b==id).then_some(c)).expect("resolved interceptor remains visible");
        assert!(w.perception(FactionId(0)).unwrap().contacts[&contact].resolved);
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
