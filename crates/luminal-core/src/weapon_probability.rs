//! Abstract missile flights: sensed aim, bounded flight time, one terminal roll.
//! Course animation is not a physical fuel/closest-approach solver. Enemy truth
//! is used only for local sensor observations and adjudicating an impact.
use super::*;
use crate::units::{C,G0,LIGHT_SECOND};

/// Direct shotgun strike: enough penetrating energy to punish exhausted defences.
pub const SRM_HIT_ENERGY_J:f64=2.0e15;

#[derive(Clone,Copy,Debug)]
pub(super) struct Flight {
    pub due:f64, start:f64, target:BodyId, aim:State, quality:f64,
    range:f64, sigma:f64, interceptor:bool, chance:f64,
}

pub fn flight_seconds(payload:Payload,range:f64,closing:f64)->f64 {
    let speed=(payload.delta_v()+closing.max(0.0)).clamp(100.0,0.3*C);
    let accel=payload.acceleration_g()*G0;
    let boost_distance=speed*speed/(2.0*accel);
    (if range<boost_distance {(2.0*range/accel).sqrt()} else {range/speed+speed/(2.0*accel)})
        .min(payload.endurance()).max(range/C).max(1.0)
}
pub fn quality(level:sensors::DetectionLevel)->f64 {
    use sensors::DetectionLevel::*;
    match level {Identity=>1.0,Resolved=>0.95,Approximate=>0.65,Bearing=>0.2,None=>0.0}
}
/// Before point defence; acquisition can improve this after launch.
pub fn hit_chance(payload:Payload,range:f64,quality:f64,sigma:f64,evasion:f64,ecm_factor:f64)->f64 {
    if range > payload.engagement_range() { return 0.0; }
    let preferred=autopilot::weapon_standoff(payload).max(1.0);
    let range_factor=1.0/(1.0+0.3*(range/preferred).powi(2));
    let uncertainty=1.0/(1.0+(sigma/(5.0*LIGHT_SECOND)).powi(2));
    // The SRM shotgun gets a modest accuracy edge, not extra damage or range.
    let (accuracy,ceiling)=if payload==Payload::Kinetic {(1.1,0.95)} else {(1.0,0.9)};
    (0.9*accuracy*range_factor*quality.clamp(0.0,1.0)*uncertainty
        *(1.0-0.25*evasion.clamp(0.0,1.0))*ecm_factor.clamp(0.5,1.0)).clamp(0.0,ceiling)
}

impl World {
    pub(super) fn start_probability_missile(&mut self,id:BodyId) {
        let m=self.bodies[id.0 as usize].missile.unwrap();
        let own=self.state(id,self.time).unwrap();
        let contact=self.received_picture(m.launcher).and_then(|p|p.contacts.get(&m.target));
        let track=contact.and_then(|c|c.estimate(self.time,&self.system));
        let aim=track.as_ref().map(|t|State {pos:t.pos(),vel:t.vel()}).unwrap_or(State {
            pos:own.pos+m.search_heading*autopilot::weapon_standoff(m.payload),vel:Vec2::ZERO});
        let range=(aim.pos-own.pos).length();
        let closing=(own.vel-aim.vel).dot((aim.pos-own.pos).normalized());
        let seconds=flight_seconds(m.payload,range,closing);
        let quality=contact.map_or(0.0,|c|quality(c.detection(self.time)));
        let sigma=track.as_ref().map_or(5.0*LIGHT_SECOND,|t|(t.pos_cov()[0][0]+t.pos_cov()[1][1]).max(0.0).sqrt());
        self.probability_flights.insert(id,Flight {due:self.time+seconds,start:self.time,target:m.target_body,
            aim,quality,range,sigma,interceptor:false,chance:0.0});
        self.debug_note("MISSILE_PLAN",format!("missile={id:?} target={:?} payload={:?} range_km={range} flight_s={seconds} quality={quality} sigma_km={sigma}",m.target_body,m.payload));
    }

    pub(super) fn start_probability_interceptor(&mut self,id:BodyId,fix:sensors::SeekerFix) {
        let defence=self.bodies[id.0 as usize].interceptor.unwrap();
        let own=self.state(id,self.time).unwrap();
        let aim=State {pos:fix.pos+fix.vel*(self.time-fix.t),vel:fix.vel};
        let range=(aim.pos-own.pos).length();
        let closing=(own.vel-aim.vel).dot((aim.pos-own.pos).normalized());
        let speed=0.1*C;
        let seconds=(range/(speed+closing).max(speed*0.25)).max(range/C).max(0.1);
        let chance=interceptor::hit_probability((aim.vel-own.vel).length());
        self.probability_flights.insert(id,Flight {due:self.time+seconds,start:self.time,target:defence.target,
            aim,quality:1.0,range,sigma:0.0,interceptor:true,chance});
        self.debug_note("INTERCEPT_PLAN",format!("missile={id:?} target={:?} flight_s={seconds} chance={chance}",defence.target));
    }

    pub(super) fn guide_probability_weapon(&mut self,id:BodyId) {
        let Some(mut flight)=self.probability_flights.get(&id).copied() else {return;};
        let t=self.time;
        let Some(me)=self.state(id,t) else {return;};
        if flight.interceptor && self.interceptor_committed(id,flight.target,Some(id)) {
            self.destroy(id,t,LossCause::Expended);return;
        }
        if !self.bodies[flight.target.0 as usize].alive_at(t) {
            if !flight.interceptor {self.record_combat(t,me.pos,CombatKind::MissileMiss,Some(id),None);}
            self.destroy(id,t,LossCause::Expended);return;
        }
        let remaining=(flight.due-t).max(0.0);
        let missile=self.bodies[id.0 as usize].missile;
        // Prefer the weapon's own, light-delayed terminal observation. Prior to
        // acquisition, its animation follows only the datalink's received track.
        if !flight.interceptor && let Some(m)=missile {
            if let Some(tr)=self.received_picture(id).and_then(|p|p.contacts.get(&m.target))
                .and_then(|c|c.estimate(t,&self.system)) {
                flight.aim=State {pos:tr.pos(),vel:tr.vel()};
            } else {flight.aim.pos=flight.aim.pos+flight.aim.vel*(t-self.bodies[id.0 as usize].missile.unwrap().last_guide);}
            let terminal=remaining<=MISSILE_ACTIVE_LEAD_S || (flight.aim.pos-me.pos).length()<=MISSILE_ACTIVE_RANGE_LS*LIGHT_SECOND;
            if terminal {
                self.bodies[id.0 as usize].missile.as_mut().unwrap().active_seeker=true;
                if t>=self.bodies[id.0 as usize].probe_ping_at {
                    self.ping(id);self.bodies[id.0 as usize].probe_ping_at=t+5.0;
                }
            }
            if let Some((emitted,seen))=retarded_state(&self.bodies[flight.target.0 as usize].trajectory,me.pos,t) {
                let range=(seen.pos-me.pos).length();
                let level=self.detect_ship(id,flight.target,emitted,range,false);
                if range<=MISSILE_ACTIVE_RANGE_LS*LIGHT_SECOND && level>=sensors::DetectionLevel::Resolved
                    && self.system.occluder(seen.pos,emitted,me.pos,t).is_none() {
                    let prior=m.local_fix;
                    let fix=sensors::SeekerFix::update(prior,emitted,seen.pos,flight.aim.vel);
                    self.bodies[id.0 as usize].missile.as_mut().unwrap().local_fix=Some(fix);
                    flight.aim=State {pos:fix.pos+fix.vel*(t-fix.t),vel:fix.vel};
                    flight.quality=1.0;flight.sigma=0.0;
                    let faction=self.bodies[id.0 as usize].faction;
                    self.relays.push(Relay {faction,front:Front {origin:me.pos,t_emit:t},obs:Observation {
                        detection:level,contact:m.target,sensor:id,origin:me.pos,emitted_at:emitted,
                        sensor_received_at:t,decider_received_at:f64::NAN,source:Source::Emission,snr:1e9,
                        measurement:Measurement::BearingRange {bearing:bearing_of(seen.pos-me.pos),range,sigma_range:1.0,sigma_bearing:1e-7}}});
                }
            }
            if let Some(fix)=self.bodies[id.0 as usize].missile.unwrap().local_fix
                && t-fix.t<=MISSILE_ACTIVE_RANGE_LS+10.0 {
                flight.aim=State {pos:fix.pos+fix.vel*(t-fix.t),vel:fix.vel};
                flight.quality=1.0;flight.sigma=0.0;
            }
            let ms=self.bodies[id.0 as usize].missile.as_mut().unwrap();
            ms.search_heading=(flight.aim.pos-me.pos).normalized();
            ms.last_guide=t;ms.phase=if remaining<20.0 {Phase::Terminal} else if t-flight.start<0.6*(flight.due-flight.start) {Phase::Burn} else {Phase::Cruise};
            ms.dv_left=m.payload.delta_v()*(remaining/(flight.due-flight.start).max(1.0)).clamp(0.0,1.0);
        } else if let Some((_,seen))=retarded_state(&self.bodies[flight.target.0 as usize].trajectory,me.pos,t) {
            flight.aim=seen;
        }
        self.probability_flights.insert(id,flight);
        if remaining<=1e-6 {self.resolve_probability_weapon(id,flight);return;}
        let destination=flight.aim.pos+flight.aim.vel*remaining;
        let velocity=(destination-me.pos)*(1.0/remaining);
        let velocity=velocity.normalized()*velocity.length().min(0.3*C);
        let thrust=velocity.normalized()*self.bodies[id.0 as usize].max_accel();
        self.bodies[id.0 as usize].trajectory.weapon_course(t,velocity,thrust);
        let dt=remaining.min(if remaining<30.0 {0.25} else {5.0});
        self.scheduler.schedule(t+dt,if flight.interceptor {Event::InterceptorGuide(id)} else {Event::MissileGuide(id)});
    }

    fn resolve_probability_weapon(&mut self,id:BodyId,flight:Flight) {
        let t=self.time;
        let me=self.state(id,t).unwrap();
        let target=self.state(flight.target,t).unwrap();
        let blocked=self.system.occluder(me.pos,t,target.pos,t).is_some();
        if flight.interceptor {
            let kill=!blocked && self.rng.uniform()<flight.chance;
            self.debug_note("INTERCEPT",format!("missile={id:?} target={:?} model=probability chance={} kill={kill}",flight.target,flight.chance));
            self.destroy(id,t,LossCause::Expended);
            if kill {self.destroy(flight.target,t,LossCause::Interceptor {missile:id});}
        } else {
            let m=self.bodies[id.0 as usize].missile.unwrap();
            let target_body=&self.bodies[flight.target.0 as usize];
            let evasion=target_body.trajectory.thrust_at(t).unwrap_or(Vec2::ZERO).length()/(100.0*G0);
            let launcher=&self.bodies[m.launcher.0 as usize];
            let ecm=sensors::resolution_factor(target_body.ecm_strength(),launcher.controls.eccm_rating*launcher.operating_effectiveness(crate::damage::System::Eccm));
            let armed=(me.pos-m.launched_at).length()>2.0*NUCLEAR_AOE_KM.value || m.payload==Payload::Kinetic;
            let chance=if blocked || !armed {0.0} else {hit_chance(m.payload,flight.range,flight.quality,flight.sigma,evasion,0.6+0.4*ecm)};
            let hit=self.rng.uniform()<chance;
            self.debug_note("MISSILE_RESULT",format!("missile={id:?} target={:?} payload={:?} model=probability chance={chance} hit={hit} quality={} range_km={}",flight.target,m.payload,flight.quality,flight.range));
            self.destroy(id,t,LossCause::Expended);
            if m.payload==Payload::Nuclear && armed {self.record_combat(t,if hit {target.pos} else {me.pos},CombatKind::NuclearBurst,Some(id),None);}
            if hit {
                let energy=if m.payload==Payload::Nuclear {NUCLEAR_ENERGY_J.value} else {SRM_HIT_ENERGY_J};
                self.deliver(flight.target,t,energy,m.payload,id);
            } else {self.record_combat(t,me.pos,CombatKind::MissileMiss,Some(id),None);}
        }
    }
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn lrm_proximity_damage_is_half_srm_direct_damage() {
        assert_eq!(NUCLEAR_ENERGY_J.value*2.0,SRM_HIT_ENERGY_J);
    }
    #[test] fn srm_accuracy_bonus_is_modest_and_does_not_extend_range() {
        let p=Payload::Kinetic;let r=autopilot::weapon_standoff(p);
        assert!((hit_chance(p,r,1.0,0.0,0.0,1.0)-(0.9/1.3)*1.1).abs()<1e-10);
        assert_eq!(hit_chance(p,0.0,1.0,0.0,0.0,1.0),0.95);
        assert_eq!(hit_chance(p,p.engagement_range()*1.01,1.0,0.0,0.0,1.0),0.0);
        assert_eq!(hit_chance(Payload::Nuclear,0.0,1.0,0.0,0.0,1.0),0.9);
    }
    #[test] fn chances_degrade_with_range_uncertainty_evasion_and_ecm() {
        let p=Payload::Nuclear;let r=autopilot::weapon_standoff(p);
        let good=hit_chance(p,r,1.0,0.0,0.0,1.0);
        assert!(good>0.6 && good<0.8);
        for bad in [hit_chance(p,r*1.5,1.0,0.0,0.0,1.0),hit_chance(p,r,0.2,0.0,0.0,1.0),
            hit_chance(p,r,1.0,5.0*LIGHT_SECOND,0.0,1.0),hit_chance(p,r,1.0,0.0,1.0,1.0),hit_chance(p,r,1.0,0.0,0.0,0.5)] {assert!(bad<good && bad>=0.0);}
        assert_eq!(hit_chance(p,p.engagement_range()*1.01,1.0,0.0,0.0,1.0),0.0);
    }
    #[test] fn flight_time_never_precedes_light_and_closure_helps() {
        for p in [Payload::Nuclear,Payload::Kinetic] {for r in [LIGHT_SECOND,crate::units::AU,1000.0*crate::units::AU] {
            let slow=flight_seconds(p,r,0.0);let fast=flight_seconds(p,r,0.01*C);
            assert!(fast>=r/C && slow>=r/C);assert!(fast<=slow);
        }}
    }
}
