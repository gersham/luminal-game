//! Physical offensive missile guidance with a terminal probability roll.
//! Proper acceleration consumes fuel; reactor lifetime bounds the encounter.
//! Enemy truth is used only for local sensing and swept proximity adjudication.
use super::*;
use crate::units::{C,G0,LIGHT_SECOND};

/// Direct shotgun strike: enough penetrating energy to punish exhausted defences.
pub const SRM_HIT_ENERGY_J:f64=2.5e14;

#[derive(Clone,Copy,Debug)]
pub(super) struct Flight {
    pub due:f64, start:f64, target:BodyId, aim:State, quality:f64,
    center_launch:bool, last_course:f64, correction_left:f64, closest:f64,
    range:f64, sigma:f64, interceptor:bool, chance:f64, aim_accel:Vec2,
}

pub fn flight_seconds(payload:Payload,range:f64,closing:f64)->f64 {
    missile::remaining_flight_seconds(payload,range,closing,payload.boost_budget(),payload.delta_v(),Phase::Burn).max(1.0)
}

pub fn quality(level:sensors::DetectionLevel)->f64 {
    use sensors::DetectionLevel::*;
    match level {Identity=>1.0,Resolved=>0.95,Approximate=>0.65,Bearing=>0.2,None=>0.0}
}
/// Difficulty of tracking across the missile's approach: lateral acceleration and
/// crossing speed matter; simply accelerating along its flight line does not.
pub fn evasion_score(thrust:Vec2,relative_velocity:Vec2,approach:Vec2)->f64 {
    let axis=approach.normalized();
    if axis==Vec2::ZERO {return 0.0;}
    let lateral_accel=(thrust-axis*thrust.dot(axis)).length()/(120.0*G0);
    let crossing=(relative_velocity-axis*relative_velocity.dot(axis)).length()/1000.0;
    (0.7*lateral_accel.clamp(0.0,1.0)+0.3*crossing.clamp(0.0,1.0)).clamp(0.0,1.0)
}

/// Before point defence; acquisition can improve this after launch.
pub fn hit_chance(payload:Payload,range:f64,quality:f64,sigma:f64,evasion:f64,ecm_factor:f64)->f64 {
    let nominal=payload.engagement_range();
    let range_factor=1.0/(1.0+(range/nominal).powi(2)/3.0);
    let footprint=if quality<0.9 {payload.kill_radius()+missile::lateral_reach(payload.lateral_accel(),
        payload.correction_budget(),payload.seeker_range()/payload.delta_v().max(1.0))} else {5.0*LIGHT_SECOND};
    let uncertainty=1.0/(1.0+(sigma/footprint).powi(2));
    // The SRM shotgun gets a modest accuracy edge, not extra damage or range.
    let (accuracy,ceiling)=if payload==Payload::Kinetic {(1.1,0.97)} else {(1.0,0.95)};
    (accuracy*range_factor*quality.clamp(0.0,1.0)*uncertainty
        *(1.0-0.65*evasion.clamp(0.0,1.0)*(range/nominal).powi(2).min(1.0))*ecm_factor.clamp(0.5,1.0)).clamp(0.0,ceiling)
}

/// A fresh ship echo closes 90% of the residual fire-control miss. A 75% shot
/// becomes 97.5% for the minute `active_fire_control` stays up.
/// Physical interception is checked separately before this is allowed to apply.
pub fn ping_supported_chance(base:f64,support:f64)->f64 {
    (base+(1.0-base)*0.9*support.clamp(0.0,1.0)).min(0.995)
}

impl World {
    pub(super) fn start_probability_missile(&mut self,id:BodyId) {
        let m=self.bodies[id.0 as usize].missile.unwrap();
        let own=self.state(id,self.time).unwrap();
        let contact=self.received_picture(m.launcher).and_then(|p|p.contacts.get(&m.target));
        let track=contact.and_then(|c|c.estimate(self.time,&self.system));
        let center_launch=contact.is_some_and(|c|c.detection(self.time)==sensors::DetectionLevel::Approximate);
        let aim=track.as_ref().map(|t|State {pos:t.pos(),vel:if center_launch {Vec2::ZERO} else {t.vel()}}).unwrap_or(State {
            pos:own.pos+m.search_heading*autopilot::weapon_standoff(m.payload),vel:Vec2::ZERO});
        let range=(aim.pos-own.pos).length();
        let closing=(own.vel-aim.vel).dot((aim.pos-own.pos).normalized());
        let seconds=flight_seconds(m.payload,range,closing);
        let quality=contact.map_or(0.0,|c|quality(c.detection(self.time)));
        let sigma=track.as_ref().map_or(5.0*LIGHT_SECOND,|t|(t.pos_cov()[0][0]+t.pos_cov()[1][1]).max(0.0).sqrt());
        self.probability_flights.insert(id,Flight {due:self.time+m.payload.endurance(),start:self.time,target:m.target_body,
            center_launch,last_course:self.time,closest:f64::INFINITY,correction_left:m.payload.correction_budget(),
            aim,quality,range,sigma,interceptor:false,chance:0.0,aim_accel:track.as_ref().filter(|_|!center_launch).map_or(Vec2::ZERO,|t|t.accel())});
        self.debug_note("MISSILE_PLAN",format!("missile={id:?} target={:?} payload={:?} range_km={range} closing_kms={closing} flight_s={seconds} quality={quality} sigma_km={sigma}",m.target_body,m.payload));
    }

    pub(super) fn start_probability_interceptor(&mut self,id:BodyId,fix:sensors::SeekerFix) {
        let defence=self.bodies[id.0 as usize].interceptor.unwrap();
        let own=self.state(id,self.time).unwrap();
        let fix=fix.accelerating();
        self.bodies[id.0 as usize].interceptor.as_mut().unwrap().solution=fix;
        let age=(self.time-fix.t).max(0.0);
        let aim=State {pos:fix.pos+fix.vel*age+fix.accel*(0.5*age*age),vel:fix.vel+fix.accel*age};
        let range=(aim.pos-own.pos).length();
        let closing=(own.vel-aim.vel).dot((aim.pos-own.pos).normalized());
        let speed=0.1*C;
        let seconds=interceptor::flight_seconds(own,aim,fix.accel)
            .unwrap_or((range/(speed+closing).max(speed*0.25)).max(range/C).max(0.1));
        let payload=self.bodies[defence.target.0 as usize].missile.map(|m|m.payload);
        let chance=interceptor::hit_probability_against((aim.vel-own.vel).length(),payload);
        self.probability_flights.insert(id,Flight {due:self.time+seconds,start:self.time,target:defence.target,
            center_launch:false,last_course:self.time,closest:f64::INFINITY,correction_left:INTERCEPTOR_ACCEL_G.value*G0*INTERCEPTOR_BURN_S.value,
            aim,quality:1.0,range,sigma:0.0,interceptor:true,chance,aim_accel:fix.accel});
        self.debug_note("INTERCEPT_PLAN",format!("missile={id:?} target={:?} flight_s={seconds} chance={chance} aim={aim:?} acceleration={:?}",defence.target,fix.accel));
    }

    pub(super) fn guide_probability_weapon(&mut self,id:BodyId) {
        let Some(mut flight)=self.probability_flights.get(&id).copied() else {return;};
        let t=self.time;
        let Some(me)=self.state(id,t) else {return;};
        let reactor_expires=self.bodies[id.0 as usize].missile
            .map(|m|flight.start+m.payload.endurance());
        if reactor_expires.is_some_and(|expires|t>=expires) {
            self.debug_note("MISSILE_REACTOR_EXHAUSTED",format!("missile={id:?} lifetime_s={}",t-flight.start));
            self.record_combat(t,me.pos,CombatKind::MissileMiss,Some(id),None);
            self.destroy(id,t,LossCause::Expended);
            return;
        }
        if flight.interceptor && self.interceptor_committed(id,flight.target,Some(id)) {
            self.destroy(id,t,LossCause::Expended);return;
        }
        if self.bodies[flight.target.0 as usize].trajectory.end().is_some_and(|end|end<=t) {
            if !flight.interceptor {self.record_combat(t,me.pos,CombatKind::MissileMiss,Some(id),None);}
            self.destroy(id,t,LossCause::Expended);return;
        }
        let remaining=(flight.due-t).max(0.0);
        if let Some(m)=self.bodies[id.0 as usize].missile {
            let spent=self.bodies[id.0 as usize].trajectory.thrust_impulse(m.last_guide,t);
            let ms=self.bodies[id.0 as usize].missile.as_mut().unwrap();
            ms.dv_left=(ms.dv_left-spent).max(0.0);
            if ms.phase==Phase::Burn {ms.burn_left=(ms.burn_left-spent).max(0.0);}
            if ms.phase==Phase::Cruise {flight.correction_left=(flight.correction_left-spent).max(0.0);}
        }
        let missile=self.bodies[id.0 as usize].missile;
        // Prefer the weapon's own, light-delayed terminal observation. Prior to
        // acquisition, its animation follows only the datalink's received track.
        if !flight.interceptor && let Some(m)=missile {
            let epoch=self.received_picture_epoch(id).unwrap_or(t);
            if !flight.center_launch && let Some(tr)=self.received_picture(id).and_then(|p|p.contacts.get(&m.target))
                .filter(|c|c.detection(epoch)>=sensors::DetectionLevel::Resolved).and_then(|c|c.estimate(epoch,&self.system)).map(|tr|tr.at(t,&self.system)) {
                flight.aim=State {pos:tr.pos(),vel:tr.vel()};flight.aim_accel=tr.accel();
            } else {flight.aim.pos=flight.aim.pos+flight.aim.vel*(t-self.bodies[id.0 as usize].missile.unwrap().last_guide);}
            let terminal=(flight.aim.pos-me.pos).length()<=m.payload.terminal_range()*1.25;
            if terminal {
                self.bodies[id.0 as usize].missile.as_mut().unwrap().active_seeker=true;
                if t>=self.bodies[id.0 as usize].probe_ping_at {
                    self.ping(id);self.bodies[id.0 as usize].probe_ping_at=t+5.0;
                }
            }
            if let Some((emitted,seen))=retarded_state(&self.bodies[flight.target.0 as usize].trajectory,me.pos,t) {
                let range=(seen.pos-me.pos).length();
                let level=self.detect_ship(id,flight.target,emitted,range,false);
                if range<=m.payload.seeker_range() && level>=sensors::DetectionLevel::Resolved
                    && missile::in_search_cone(m.search_heading,seen.pos-me.pos)
                    && self.system.occluder(seen.pos,emitted,me.pos,t).is_none() {
                    let prior=m.local_fix;
                    let fix=sensors::SeekerFix::update(prior,emitted,seen.pos,flight.aim.vel).accelerating();
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
                && t-fix.t<=m.payload.seeker_range()/C+1.0 {
                flight.aim=State {pos:fix.pos+fix.vel*(t-fix.t),vel:fix.vel};
                flight.quality=1.0;flight.sigma=0.0;
            }
            let ms=self.bodies[id.0 as usize].missile.as_mut().unwrap();
            ms.search_heading=(flight.aim.pos-me.pos).normalized();
            ms.last_guide=t;
            if ms.phase!=Phase::Terminal {
                ms.phase=if (flight.aim.pos-me.pos).length()<=m.payload.terminal_range() {Phase::Terminal}
                    else if m.payload==Payload::Kinetic || ms.burn_left>1e-6 {Phase::Burn} else {Phase::Cruise};
            }
        } else if let Some((emitted,seen))=retarded_state(&self.bodies[flight.target.0 as usize].trajectory,me.pos,t) {
            let prior=self.bodies[id.0 as usize].interceptor.unwrap().solution;
            if emitted>prior.t+1e-6 {
                // Acceleration comes from successive light-delayed velocity samples,
                // never the target's current thrust or future trajectory.
                let measured=(seen.vel-prior.vel)*(1.0/(emitted-prior.t));
                flight.aim_accel=measured.normalized()*measured.length().min(6000.0*G0);
                let solution=&mut self.bodies[id.0 as usize].interceptor.as_mut().unwrap().solution;
                solution.t=emitted;solution.pos=seen.pos;solution.vel=seen.vel;
            }
            let age=(t-emitted).max(0.0);
            flight.aim=State {pos:seen.pos+seen.vel*age+flight.aim_accel*(0.5*age*age),vel:seen.vel+flight.aim_accel*age};
        }
        // Sweep only continuous normal-space intervals: jumping targets leave
        // no physical trail between departure and arrival to collide with.
        for (start, end) in crate::lightcone::normal_intervals(
            &self.bodies[flight.target.0 as usize].trajectory, flight.last_course, t) {
            let distance=|at:f64| (self.state(id,at).unwrap().pos-self.state(flight.target,at).unwrap().pos).length();
            let (mut lo,mut hi)=(start,end);
            for _ in 0..32 {
                let a=lo+(hi-lo)/3.0;let b=hi-(hi-lo)/3.0;
                if distance(a)<distance(b) {hi=b;} else {lo=a;}
            }
            flight.closest=flight.closest.min(distance(lo)).min(distance(end)).min(distance(start));
            let radius=missile.map_or(INTERCEPTOR_KILL_RADIUS_KM.value,|m|m.payload.kill_radius());
            if flight.closest<=radius {
                let at=if distance(start)<=radius {start} else {lo};
                self.resolve_probability_weapon_at(id,flight,at);return;
            }
        }
        if remaining<=1e-6 {self.resolve_probability_weapon(id,flight);return;}
        if !flight.interceptor {
            self.steer_powered_missile(id,flight,me,reactor_expires.unwrap());
            return;
        }
        let horizon=remaining;
        let destination=flight.aim.pos+flight.aim.vel*remaining+flight.aim_accel*(0.5*horizon*horizon);
        let velocity=(destination-me.pos)*(1.0/remaining);
        let mut velocity=velocity.normalized()*velocity.length().min(0.3*C);
        if t>flight.start && (flight.interceptor || remaining<=MISSILE_TERMINAL_S.value || flight.center_launch) {
            let correction=velocity-me.vel;
            let accel=missile.map_or(INTERCEPTOR_ACCEL_G.value*G0,|m|m.payload.lateral_accel());
            let allowed=(accel*(t-flight.last_course).max(0.0)).min(flight.correction_left);
            let used=correction.length().min(allowed);
            velocity=me.vel+correction.normalized()*used;
            flight.correction_left=(flight.correction_left-used).max(0.0);
        }
        flight.last_course=t;
        self.probability_flights.insert(id,flight);
        let thrust=velocity.normalized()*self.bodies[id.0 as usize].max_accel();
        self.bodies[id.0 as usize].trajectory.weapon_course(t,velocity,thrust);
        let sensor_step=missile.map_or(5.0,|m|m.payload.seeker_range()/(2.0*(velocity.length()+C*0.01)).max(1.0));
        let dt=remaining.min(sensor_step.min(if remaining<30.0 {0.25} else {5.0}).max(0.001));
        let dt=reactor_expires.map_or(dt,|expires|dt.min(expires-t));
        self.scheduler.schedule(t+dt,if flight.interceptor {Event::InterceptorGuide(id)} else {Event::MissileGuide(id)});
    }

    fn steer_powered_missile(&mut self,id:BodyId,mut flight:Flight,me:State,expires:f64) {
        let t=self.time;
        let m=self.bodies[id.0 as usize].missile.unwrap();
        if m.dv_left<=1e-6 {
            self.record_combat(t,me.pos,CombatKind::MissileMiss,Some(id),None);
            self.destroy(id,t,LossCause::Expended);return;
        }
        let rel=flight.aim.pos-me.pos;
        let closing=(me.vel-flight.aim.vel).dot(rel.normalized());
        let estimate=missile::remaining_flight_seconds(m.payload,rel.length(),closing,m.burn_left,m.dv_left,m.phase);
        let eta=estimate.min(expires-t).max(0.1);
        let horizon=eta.min(missile::ACCEL_PERSIST_S);
        let aim=flight.aim.pos+flight.aim.vel*eta+flight.aim_accel*(horizon*(eta-0.5*horizon));
        let error=aim-me.pos-me.vel*eta;
        let accel=m.payload.acceleration_g()*G0;
        let thrust=if m.phase==Phase::Cruise {
            // Midcourse corrections spend a separate finite reserve; a correct
            // stationary shot coasts with the main engine off for 0.9 AU.
            if flight.correction_left>1e-6 {
                let forward=(me.vel-flight.aim.vel).normalized();
                let lateral_error=error-forward*error.dot(forward);
                let correction=lateral_error*(2.0/(eta*eta));
                correction.normalized()*correction.length().min(m.payload.lateral_accel())
            } else {Vec2::ZERO}
        } else {
            // Continuous boost/terminal acceleration. Steering cannot teleport
            // velocity: acceleration and the total fuel budget bound every turn.
            error.normalized()*accel
        };
        flight.last_course=t;
        self.probability_flights.insert(id,flight);
        self.bodies[id.0 as usize].trajectory.push(t,thrust,Vec2::ZERO).unwrap();
        let relative_speed=(me.vel-flight.aim.vel).length().max(1.0);
        let mut dt=5.0_f64.min((rel.length()/relative_speed*0.1).max(0.05));
        if thrust.length()>0.0 {
            dt=dt.min(m.dv_left/thrust.length());
            if m.phase==Phase::Burn && m.payload==Payload::Nuclear {dt=dt.min(m.burn_left/thrust.length());}
            if m.phase==Phase::Cruise {dt=dt.min(flight.correction_left/thrust.length());}
        }
        self.scheduler.schedule(t+dt.max(1e-6).min(expires-t),Event::MissileGuide(id));
    }

    fn resolve_probability_weapon(&mut self,id:BodyId,flight:Flight) {
        self.resolve_probability_weapon_at(id,flight,self.time);
    }
    fn resolve_probability_weapon_at(&mut self,id:BodyId,flight:Flight,t:f64) {
        let me=self.state(id,t).unwrap();
        let Some(target)=self.state(flight.target,t) else {
            self.record_combat(t,me.pos,CombatKind::MissileMiss,Some(id),None);
            self.destroy(id,t,LossCause::Expended);
            return;
        };
        let blocked=self.system.occluder(me.pos,t,target.pos,t).is_some();
        if flight.interceptor {
            let kill=!blocked && (target.pos-me.pos).length()<=INTERCEPTOR_KILL_RADIUS_KM.value && self.rng.uniform()<flight.chance;
            self.debug_note("INTERCEPT",format!("missile={id:?} target={:?} model=probability chance={} kill={kill} miss_km={} closest_km={}",flight.target,flight.chance,(target.pos-me.pos).length(),flight.closest));
            self.record_interception(id,flight.target,t,if kill {CombatKind::MissileHit} else {CombatKind::MissileMiss});
            self.destroy(id,t,LossCause::Expended);
            if kill {self.destroy(flight.target,t,LossCause::Interceptor {missile:id});}
        } else {
            let m=self.bodies[id.0 as usize].missile.unwrap();
            let target_body=&self.bodies[flight.target.0 as usize];
            let launch_velocity=self.state(m.launcher,flight.start).map_or(Vec2::ZERO,|s|s.vel);
            let evasion=[1.0,5.0,10.0,20.0,30.0].into_iter().filter_map(|ago| {
                let at=(t-ago).max(flight.start);
                let missile=self.state(id,at)?;let target=self.state(flight.target,at)?;
                Some(evasion_score(target_body.trajectory.thrust_at(at).unwrap_or(Vec2::ZERO),
                    target.vel-launch_velocity,target.pos-missile.pos))
            }).fold(0.0_f64,f64::max);
            let launcher=&self.bodies[m.launcher.0 as usize];
            let ecm=sensors::resolution_factor(target_body.ecm_strength(),launcher.controls.eccm_rating*launcher.operating_effectiveness(crate::damage::System::Eccm));
            let armed=(me.pos-m.launched_at).length()>2.0*NUCLEAR_AOE_KM.value || m.payload==Payload::Kinetic;
            let hit_radius=if m.payload==Payload::Nuclear {NUCLEAR_AOE_KM.value} else {KINETIC_PATTERN_KM.value};
            let missed_center_pass=flight.closest.min((target.pos-me.pos).length())>hit_radius;
            let epoch=self.received_picture_epoch(id).unwrap_or(t);
            let active=self.received_picture(id).and_then(|p|p.contacts.get(&m.target)).map_or(0.0,|c|c.active_fire_control(epoch,|sensor|self.body(sensor).is_some_and(|b|matches!(b.kind,BodyKind::Ship|BodyKind::Station))));
            let chance=if blocked || !armed || missed_center_pass {0.0} else {
                ping_supported_chance(hit_chance(m.payload,flight.range,1.0,0.0,0.0,0.6+0.4*ecm),active)
            };
            let hit=self.rng.uniform()<chance;
            self.debug_note("MISSILE_RESULT",format!("missile={id:?} target={:?} payload={:?} model=probability chance={chance} evasion={evasion} hit={hit} quality={} range_km={} closest_km={} correction_left={}",flight.target,m.payload,flight.quality,flight.range,flight.closest,flight.correction_left));
            if m.payload==Payload::Nuclear && armed {self.record_combat(t,if hit {target.pos} else {me.pos},CombatKind::NuclearBurst,Some(id),None);}
            if hit {
                let energy=if m.payload==Payload::Nuclear {NUCLEAR_ENERGY_J.value} else {SRM_HIT_ENERGY_J};
                self.deliver(flight.target,t,energy,m.payload,id);
            } else {self.record_combat(t,me.pos,CombatKind::MissileMiss,Some(id),None);}
            // Publish the outcome before retirement flushes tracked-round events.
            // Otherwise the marker disappears now but its hit/miss arrives later.
            self.destroy(id,t,LossCause::Expended);
        }
    }
}

#[cfg(test)] mod tests {
    use super::*;
    #[test]
    fn close_missile_shots_are_hard_to_evade() {
        fn trial(seed:u64,payload:Payload,evade:bool)->(bool,bool) {
            let range=5.0*LIGHT_SECOND;
            let specs=vec![
                BodySpec {name:"Launcher".into(),kind:BodyKind::Ship,faction:FactionId(0),state:State {pos:Vec2::ZERO,vel:Vec2::ZERO},thrust:Vec2::ZERO,magazine:10},
                BodySpec {name:"Target".into(),kind:BodyKind::Ship,faction:FactionId(1),state:State {pos:Vec2::new(range,0.0),vel:Vec2::ZERO},thrust:Vec2::ZERO,magazine:0},
            ];
            let mut w=World::new(crate::celestial::System {bodies:vec![]},specs,100.0,seed);
            for b in &mut w.bodies {
                b.controls.ecm=controls::Mode::Off;b.controls.screens=controls::Mode::Off;
                b.controls.evade=controls::Mode::Off;
            }
            w.bodies[1].controls.evade=if evade {controls::Mode::Auto} else {controls::Mode::Off};
            let c=w.contact_id(FactionId(0),BodyId(1));
            w.perceptions.get_mut(&FactionId(0)).unwrap().ingest(Observation {
                detection:sensors::DetectionLevel::Resolved,contact:c,sensor:BodyId(0),origin:Vec2::ZERO,
                emitted_at:0.0,sensor_received_at:0.0,decider_received_at:0.0,source:Source::Echo,snr:1e9,
                measurement:Measurement::BearingRange {bearing:0.0,range,sigma_range:1.0,sigma_bearing:1e-7}},&w.system);
            let missile=w.launch(BodyId(0),c,payload).unwrap();
            let due=w.probability_flights[&missile].due;
            let mut maneuvered=false;
            while w.time<due+2.0 {
                w.advance_to((w.time+5.0).min(due+2.0));
                maneuvered|=w.bodies[1].controls.evading;
            }
            (w.hits.iter().any(|h|h.missile==missile),maneuvered)
        }
        for payload in [Payload::Nuclear,Payload::Kinetic] {
            let (mut straight,mut evasive,mut activated)=(0,0,0);
            for seed in 0..64 {
                straight+=u32::from(trial(seed,payload,false).0);
                let (hit,active)=trial(seed,payload,true);evasive+=u32::from(hit);activated+=u32::from(active);
            }
            println!("{payload:?}: straight {straight}/64 hits; AUTO evade {evasive}/64 hits; activated {activated}/64");
            assert!(straight>=48,"accurate against a coasting target: {straight}");
            assert_eq!(activated,0,"close shots do not justify spending heat on evasion");
            assert!(evasive+8>=straight,"fresh close-range missiles should retain a strong intercept: {straight} vs {evasive}");
        }
    }

    #[test]
    fn lrm_potshots_depend_on_fixed_offset_and_ellipse_size() {
        let mut totals=Vec::new();
        for radius in [15_000.0,60_000.0,300_000.0,3_000_000.0] {
            let mut hits=0;
            for seed in 0..64 {
                let mut random=crate::rng::Rng::stream(seed,1701);
                let angle=random.uniform()*std::f64::consts::TAU;
                let offset=Vec2::new(angle.cos(),angle.sin())*(radius*random.uniform().sqrt());
                let range=5.0*LIGHT_SECOND;
                let specs=(0..2).map(|i|BodySpec {name:format!("Ship {i}"),kind:BodyKind::Ship,faction:FactionId(i),
                    state:State {pos:Vec2::new(i as f64*range,0.0),vel:Vec2::ZERO},thrust:Vec2::ZERO,magazine:10}).collect();
                let mut w=World::new(crate::celestial::System {bodies:vec![]},specs,100.0,seed);
                for b in &mut w.bodies {b.controls.evade=controls::Mode::Off;b.controls.ecm=controls::Mode::Off;
                    b.controls.screens=controls::Mode::Off;b.sensors=sensors::SensorSuite {passive:false,active:false,direction_finding:false};}
                let contact=w.contact_id(FactionId(0),BodyId(1));
                w.perceptions.get_mut(&FactionId(0)).unwrap().ingest(Observation {detection:sensors::DetectionLevel::Approximate,
                    contact,sensor:BodyId(0),origin:Vec2::ZERO,emitted_at:0.0,sensor_received_at:0.0,decider_received_at:0.0,
                    source:Source::Echo,snr:100.0,measurement:Measurement::BearingRange {bearing:0.0,range,sigma_range:radius/2.0,sigma_bearing:radius/(2.0*range)}},&w.system);
                let center=w.perceptions[&FactionId(0)].contacts[&contact].estimate(0.0,&w.system).unwrap().pos();
                w.bodies[1].trajectory=Trajectory::new(0.0,State {pos:center+offset,vel:Vec2::ZERO});
                assert_eq!(w.launch(BodyId(0),contact,Payload::Kinetic),Err(OrderError::NoTrack));
                assert_eq!(w.queue_launch(BodyId(0),contact,Payload::Kinetic),Err(OrderError::NoTrack));
                let id=w.launch(BodyId(0),contact,Payload::Nuclear).unwrap();
                let due=w.probability_flights[&id].due;
                w.advance_to(due+10.0);
                hits+=usize::from(w.hits.iter().any(|h|h.missile==id));
            }
            println!("LRM ellipse radius {radius:.0} km: {hits}/64 hits");totals.push(hits);
        }
        assert!(totals[0]>=48);
        assert!(totals[2]>=48,"new terminal seeker can correct a modest initial error");
        assert!(totals[3]+20<totals[0],"large fixed errors must still cause physical misses");
    }

    #[test]
    fn missile_sensor_resolution_is_close_only_for_passive_and_echoes() {
        let (mut w,c)=super::super::tests::beam_trial();
        for payload in Payload::ALL {
            let id=w.launch(BodyId(0),c,payload).unwrap();
            for ping in [false,true] {
                assert_eq!(w.detect_ship(id,BodyId(1),0.0,payload.seeker_range()+1.0,ping),sensors::DetectionLevel::None);
                assert_eq!(w.detect_ship(id,BodyId(1),0.0,payload.seeker_range()-1.0,ping),sensors::DetectionLevel::Resolved);
            }
        }
        assert_eq!(Payload::Kinetic.seeker_range(),0.04*AU);
        assert_eq!(Payload::Nuclear.seeker_range(),0.1*AU);
    }

    #[test]
    fn missiles_keep_flying_when_target_jumps_and_miss_if_it_is_absent_at_deadline() {
        let specs=vec![
            BodySpec {name:"Launcher".into(),kind:BodyKind::Ship,faction:FactionId(0),state:State {pos:Vec2::ZERO,vel:Vec2::ZERO},thrust:Vec2::ZERO,magazine:10},
            BodySpec {name:"Target".into(),kind:BodyKind::Ship,faction:FactionId(1),state:State {pos:Vec2::new(AU,0.0),vel:Vec2::ZERO},thrust:Vec2::ZERO,magazine:0},
        ];
        let mut w=World::new(crate::celestial::System {bodies:vec![]},specs,3000.0,42);
        let contact=w.contact_id(FactionId(0),BodyId(1));
        w.perceptions.get_mut(&FactionId(0)).unwrap().ingest(Observation {
            detection:sensors::DetectionLevel::Resolved,contact,sensor:BodyId(0),origin:Vec2::ZERO,
            emitted_at:0.0,sensor_received_at:0.0,decider_received_at:0.0,source:Source::Echo,snr:1e9,
            measurement:Measurement::BearingRange {bearing:0.0,range:AU,sigma_range:1.0,sigma_bearing:1e-7}},&w.system);
        let id=w.launch(BodyId(0),contact,Payload::Nuclear).unwrap();
        w.guide_probability_weapon(id);
        let due=w.probability_flights[&id].due;
        w.bodies[1].trajectory.jump_departure(1.0);
        w.time=5.0;
        w.guide_probability_weapon(id);
        assert!(w.bodies[id.0 as usize].alive_at(5.0),"hidden jump must not retire missile");
        assert!(w.hits.is_empty());
        w.time=due;
        w.guide_probability_weapon(id);
        assert!(w.bodies[id.0 as usize].trajectory.end().is_some());
        assert!(w.hits.is_empty());
    }

    #[test]
    fn lrm_reactor_expires_after_120_minutes_without_shortening_unreachable_flights() {
        assert_eq!(Payload::Nuclear.endurance(),7200.0);
        assert_eq!(Payload::Kinetic.endurance(),1320.0);
        assert!(flight_seconds(Payload::Nuclear,AU,-45_000.0)>7200.0);
        let specs=vec![
            BodySpec {name:"Launcher".into(),kind:BodyKind::Ship,faction:FactionId(0),state:State {pos:Vec2::ZERO,vel:Vec2::ZERO},thrust:Vec2::ZERO,magazine:10},
            BodySpec {name:"Receding target".into(),kind:BodyKind::Ship,faction:FactionId(1),state:State {pos:Vec2::new(AU,0.0),vel:Vec2::new(30_000.0,0.0)},thrust:Vec2::ZERO,magazine:0},
        ];
        let mut w=World::new(crate::celestial::System {bodies:vec![]},specs,3000.0,42);
        let contact=w.contact_id(FactionId(0),BodyId(1));
        w.perceptions.get_mut(&FactionId(0)).unwrap().ingest(Observation {
            detection:sensors::DetectionLevel::Resolved,contact,sensor:BodyId(0),origin:Vec2::ZERO,
            emitted_at:0.0,sensor_received_at:0.0,decider_received_at:0.0,source:Source::Echo,snr:1e9,
            measurement:Measurement::BearingRange {bearing:0.0,range:AU,sigma_range:1.0,sigma_bearing:1e-7}},&w.system);
        let id=w.launch(BodyId(0),contact,Payload::Nuclear).unwrap();
        w.probability_flights.get_mut(&id).unwrap().due=8000.0;
        w.time=7199.0;w.guide_probability_weapon(id);
        assert!(w.bodies[id.0 as usize].alive_at(w.time));
        w.time=7200.0;w.guide_probability_weapon(id);
        assert_eq!(w.bodies[id.0 as usize].trajectory.end(),Some(7200.0));
        assert!(!w.probability_flights.contains_key(&id));
        assert!(w.hits.is_empty());
    }

    #[test]
    fn crossing_motion_hurts_accuracy_more_than_radial_motion() {
        let approach=Vec2::new(1.0,0.0);
        let crossing=evasion_score(Vec2::new(0.0,120.0*G0),Vec2::new(0.0,1000.0),approach);
        let radial=evasion_score(Vec2::new(120.0*G0,0.0),Vec2::new(1000.0,0.0),approach);
        assert_eq!(radial,0.0);assert_eq!(crossing,1.0);
        for p in Payload::ALL {
            let range=p.engagement_range();
            assert!(hit_chance(p,range,1.0,0.0,crossing,1.0)<0.4*hit_chance(p,range,1.0,0.0,radial,1.0));
        }
    }

    #[test]
    fn approximate_launch_aims_at_center_and_seeker_corrections_are_bounded() {
        let target_pos=Vec2::new(AU,3.0*LIGHT_SECOND);
        let specs=vec![
            BodySpec {name:"Launcher".into(),kind:BodyKind::Ship,faction:FactionId(0),
                state:State {pos:Vec2::ZERO,vel:Vec2::ZERO},thrust:Vec2::ZERO,magazine:10},
            BodySpec {name:"Target".into(),kind:BodyKind::Ship,faction:FactionId(1),
                state:State {pos:target_pos,vel:Vec2::ZERO},thrust:Vec2::ZERO,magazine:0},
        ];
        let mut w=World::new(crate::celestial::System {bodies:vec![]},specs,3000.0,42);
        let contact=w.contact_id(FactionId(0),BodyId(1));
        w.perceptions.get_mut(&FactionId(0)).unwrap().ingest(Observation {
            detection:sensors::DetectionLevel::Approximate,contact,sensor:BodyId(0),origin:Vec2::ZERO,
            emitted_at:-500.0,sensor_received_at:0.0,decider_received_at:0.0,source:Source::Emission,snr:100.0,
            measurement:Measurement::BearingRange {bearing:0.0,range:AU,sigma_range:LIGHT_SECOND,sigma_bearing:LIGHT_SECOND/AU}},&w.system);
        let center=w.perceptions[&FactionId(0)].contacts[&contact].estimate(0.0,&w.system).unwrap().pos();
        let id=w.launch(BodyId(0),contact,Payload::Nuclear).unwrap();
        let flight=w.probability_flights[&id];
        assert!(flight.center_launch);
        assert_eq!(flight.aim.pos,center);
        assert_eq!(flight.aim.vel,Vec2::ZERO);
        w.guide_probability_weapon(id);
        let velocity=w.state(id,0.0).unwrap().vel;
        assert_eq!(velocity,Vec2::ZERO,"launch inherits velocity; thrust must not teleport it");
        assert!((w.bodies[id.0 as usize].trajectory.last().thrust.normalized()-center.normalized()).length()<1e-10);
        assert!(w.bodies[id.0 as usize].missile.unwrap().local_fix.is_none());
        // Put a terminal seeker within resolution range, crossing too fast to snap onto the new aim.
        w.time=5.0;
        let velocity=Vec2::new(0.0,10_000.0);
        w.bodies[id.0 as usize].trajectory=Trajectory::new(0.0,State {pos:target_pos-Vec2::new(50_000.0,0.0)-velocity*5.0,vel:velocity});
        w.bodies[id.0 as usize].missile.as_mut().unwrap().search_heading=Vec2::new(1.0,0.0);
        w.guide_probability_weapon(id);
        assert!(w.bodies[id.0 as usize].missile.unwrap().local_fix.is_some());
        let changed=(w.state(id,5.0).unwrap().vel-velocity).length();
        assert_eq!(changed,0.0,"guidance changes acceleration, not instantaneous velocity");
        assert!(w.bodies[id.0 as usize].trajectory.last().thrust.length()<=w.bodies[id.0 as usize].max_accel()+1e-8);
        assert_eq!(w.bodies[id.0 as usize].missile.unwrap().dv_left,Payload::Nuclear.delta_v(),"synthetic coasting history spends no fuel");
        // A probability roll must never teleport a distant miss onto the hidden target.
        let flight=w.probability_flights[&id];
        w.time=flight.due;
        w.resolve_probability_weapon(id,flight);
        assert!(w.hits.is_empty());
        assert!(w.combat_events(None).iter().any(|e|e.own_body==Some(id) && e.kind==CombatKind::MissileMiss));
    }

    #[test] fn interceptor_outcomes_report_hit_or_miss_before_disappearing() {
        for (chance,kind) in [(1.0,CombatKind::MissileHit),(0.0,CombatKind::MissileMiss)] {
            let specs=(0..2).map(|i|BodySpec {name:format!("Round {i}"),kind:BodyKind::Missile,
                faction:FactionId(i),state:State {pos:Vec2::new(i as f64*100.0,0.0),vel:Vec2::new(5.0,2.0)},
                thrust:Vec2::ZERO,magazine:0}).collect();
            let mut world=World::new(crate::celestial::System {bodies:vec![]},specs,0.0,42);
            let aim=world.state(BodyId(1),0.0).unwrap();
            world.resolve_probability_weapon(BodyId(0),Flight {due:0.0,start:0.0,target:BodyId(1),
                center_launch:false,last_course:0.0,closest:f64::INFINITY,correction_left:0.0,
                aim,quality:1.0,range:100.0,sigma:0.0,interceptor:true,chance,aim_accel:Vec2::ZERO});
            let events=world.combat_events(None);
            let result=events.iter().find(|e|e.own_body==Some(BodyId(0)) && e.kind==kind).unwrap();
            assert_eq!(result.pos,Some(if chance==1.0 {aim.pos} else {Vec2::ZERO}));
            assert_eq!(result.velocity,Some(Vec2::new(5.0,2.0)));
            assert!(world.bodies[0].trajectory.end().is_some());
            assert_eq!(world.bodies[1].trajectory.end().is_some(),chance==1.0);
            assert!(world.combat_events(Some(FactionId(0))).iter().any(|e|e.own_body==Some(BodyId(0)) && e.kind==kind),
                "the outcome must arrive with retirement, not after a light-delay gap");
            let losses=world.losses.len();
            let outcomes=world.combat_events(None).iter().filter(|e|e.own_body==Some(BodyId(0)) && e.kind==kind).count();
            for t in [1.0,10.0,100.0] {world.time=t;world.guide_probability_weapon(BodyId(0));}
            assert_eq!(world.losses.len(),losses,"a spent interceptor gets no second attempt");
            assert_eq!(world.combat_events(None).iter().filter(|e|e.own_body==Some(BodyId(0)) && e.kind==kind).count(),outcomes);
            assert_eq!(world.bodies[1].alive_at(world.time),chance==0.0);
        }
    }
    #[test] fn offensive_retirement_publishes_the_terminal_result_immediately() {
        let mut hits=0;
        for seed in 0..16 {
            let (mut w,c)=super::super::tests::beam_trial();
            w.rng=Rng::new(seed);
            let id=w.launch(BodyId(0),c,Payload::Kinetic).unwrap();
            let flight=w.probability_flights[&id];
            let target=w.state(BodyId(1),0.0).unwrap();
            w.bodies[id.0 as usize].trajectory=Trajectory::new(0.0,State {
                pos:target.pos-Vec2::new(if seed<8 {100.0} else {1e6},0.0),vel:Vec2::ZERO});
            w.resolve_probability_weapon(id,flight);
            let hit=w.hits.iter().any(|h|h.missile==id);
            hits+=usize::from(hit);
            let kind=if hit {CombatKind::MissileHit} else {CombatKind::MissileMiss};
            let events=w.combat_events(Some(FactionId(0)));
            assert_eq!(events.iter().filter(|e|e.own_body==Some(id) && e.kind==kind).count(),1);
            assert!(!w.bodies[id.0 as usize].alive_at(w.time));
        }
        assert!(hits>0 && hits<16,"exercise both terminal outcomes");
    }
    #[test] fn a_fresh_datalink_picture_survives_more_than_two_minutes_in_transit() {
        let (mut w,c)=super::super::tests::beam_trial();
        let id=w.launch(BodyId(0),c,Payload::Nuclear).unwrap();
        let origin=w.state(BodyId(0),0.0).unwrap().pos;
        w.time=800.0;
        w.perceptions.get_mut(&FactionId(0)).unwrap().ingest(Observation {
            detection:sensors::DetectionLevel::Resolved,contact:c,sensor:BodyId(0),origin,
            emitted_at:799.0,sensor_received_at:800.0,decider_received_at:800.0,source:Source::Echo,snr:1e12,
            measurement:Measurement::BearingRange {bearing:0.1,range:LIGHT_SECOND,sigma_range:1.0,sigma_bearing:1e-7}},&w.system);
        w.tactical_frame();
        w.time=1001.0;
        w.bodies[id.0 as usize].trajectory=Trajectory::new(0.0,State {pos:origin+Vec2::new(200.0*C,0.0),vel:Vec2::ZERO});
        let epoch=w.received_picture_epoch(id).unwrap();
        assert!((epoch-801.0).abs()<0.01);
        let contact=&w.received_picture(id).unwrap().contacts[&c];
        assert!(contact.detection(epoch)>=sensors::DetectionLevel::Resolved);
        assert!(contact.detection(w.time)<sensors::DetectionLevel::Resolved,"the old check incorrectly ages a picture during transmission");
        let expected=contact.estimate(epoch,&w.system).unwrap().at(w.time,&w.system).pos();
        let flight=w.probability_flights.get_mut(&id).unwrap();
        flight.due=1200.0;flight.last_course=1001.0;flight.aim.pos=Vec2::ZERO;
        w.guide_probability_weapon(id);
        assert!((w.probability_flights[&id].aim.pos-expected).length()<0.01);
    }
    #[test] fn interception_requires_contact_and_removes_the_incoming_round() {
        for contact in [false,true] {
            let specs=(0..2).map(|i|BodySpec {name:format!("Round {i}"),kind:BodyKind::Missile,faction:FactionId(i),
                state:State {pos:Vec2::new(i as f64*1000.0,0.0),vel:if i==0 && contact {Vec2::new(2000.0,0.0)} else {Vec2::ZERO}},
                thrust:Vec2::ZERO,magazine:0}).collect();
            let mut w=World::new(crate::celestial::System {bodies:vec![]},specs,0.0,42);
            let aim=w.state(BodyId(1),0.0).unwrap();
            w.bodies[0].interceptor=Some(interceptor::Interceptor {launcher:BodyId(0),target:BodyId(1),
                expires:1.0,dv_left:0.0,last_update:0.0,last_range:1000.0,
                solution:sensors::SeekerFix::update(None,0.0,aim.pos,aim.vel)});
            let flight=Flight {due:1.0,start:0.0,target:BodyId(1),center_launch:false,last_course:0.0,closest:f64::INFINITY,
                correction_left:0.0,aim,quality:1.0,range:1000.0,sigma:0.0,interceptor:true,chance:1.0,aim_accel:Vec2::ZERO};
            w.probability_flights.insert(BodyId(0),flight);w.time=1.0;
            w.guide_probability_weapon(BodyId(0));
            assert_eq!(w.bodies[1].trajectory.end().is_some(),contact);
            if contact {
                let end=w.bodies[1].trajectory.end().unwrap();
                assert!((end-0.5).abs()<0.001,"swept contact between guidance ticks");
                assert!(!w.bodies[1].alive_at(end));
                assert!(w.known_body(FactionId(1),BodyId(1)).is_none());
                assert!(!w.probability_flights.contains_key(&BodyId(0)));
                w.time=2.0;w.guide_probability_weapon(BodyId(0));
                assert_eq!(w.losses.iter().filter(|loss|loss.body==BodyId(1)).count(),1);
            }
        }
    }
    #[test] fn srm_launchers_deliver_more_close_range_damage() {
        assert!(NUCLEAR_ENERGY_J.value/MISSILE_LAUNCH_INTERVAL_S.value < SRM_HIT_ENERGY_J/SRM_LAUNCH_INTERVAL_S.value);
        assert!(10.0*NUCLEAR_ENERGY_J.value < 20.0*SRM_HIT_ENERGY_J);
    }
    #[test] fn srm_accuracy_has_nominal_envelope_without_an_arbitrary_distance_wall() {
        let p=Payload::Kinetic;let r=p.engagement_range();
        assert!((hit_chance(p,r,1.0,0.0,0.0,1.0)-0.75*1.1).abs()<1e-10);
        assert_eq!(hit_chance(p,0.0,1.0,0.0,0.0,1.0),0.97);
        assert!(hit_chance(p,p.engagement_range()*1.01,1.0,0.0,0.0,1.0)>0.0);
        assert_eq!(hit_chance(Payload::Nuclear,0.0,1.0,0.0,0.0,1.0),0.95);
    }
    #[test] fn chances_degrade_with_range_uncertainty_evasion_and_ecm() {
        let p=Payload::Nuclear;let r=p.engagement_range();
        let good=hit_chance(p,r,1.0,0.0,0.0,1.0);
        assert!(good>0.6 && good<0.8);
        for bad in [hit_chance(p,r*1.5,1.0,0.0,0.0,1.0),hit_chance(p,r,0.2,0.0,0.0,1.0),
            hit_chance(p,r,1.0,5.0*LIGHT_SECOND,0.0,1.0),hit_chance(p,r,1.0,0.0,1.0,1.0),hit_chance(p,r,1.0,0.0,0.0,0.5)] {assert!(bad<good && bad>=0.0);}
        assert!(hit_chance(p,p.engagement_range()*1.01,1.0,0.0,0.0,1.0)>0.0);
    }
    #[test] fn flight_time_never_precedes_light_and_closure_helps() {
        for p in [Payload::Nuclear,Payload::Kinetic] {for r in [LIGHT_SECOND,crate::units::AU,1000.0*crate::units::AU] {
            let slow=flight_seconds(p,r,0.0);let fast=flight_seconds(p,r,0.01*C);
            assert!(fast>=r/C && slow>=r/C);assert!(fast<=slow);
        }}
    }
    #[test] fn fast_head_on_srm_uses_inherited_closure_in_its_burn_time() {
        let range=2_000_000.0;let closing=19_000.0;let p=Payload::Kinetic;
        assert_eq!(flight_seconds(p,0.0,0.0),1.0);
        let seconds=flight_seconds(p,range,closing);
        assert!(seconds<range/closing,"boost must beat coasting, not wait until after the ships pass");
        assert!((closing*seconds+0.5*p.acceleration_g()*G0*seconds*seconds-range).abs()<1e-6);
        // Its initial straight-course representation must add forward speed,
        // rather than reversing relative to the launch platform.
        assert!(range/seconds-closing>0.0);
        assert!(flight_seconds(p,range,-3000.0)>flight_seconds(p,range,0.0));
    }
    #[test] fn head_on_srm_launches_forward_and_local_lasers_can_stop_it() {
        let mut intercepted=0;let mut hits=0;
        for seed in 0..48 {
            let specs=vec![
                BodySpec {name:"Fast launcher".into(),kind:BodyKind::Ship,faction:FactionId(0),state:State {pos:Vec2::ZERO,vel:Vec2::new(19_000.0,0.0)},thrust:Vec2::ZERO,magazine:10},
                BodySpec {name:"Defender".into(),kind:BodyKind::Ship,faction:FactionId(1),state:State {pos:Vec2::new(2_000_000.0,0.0),vel:Vec2::ZERO},thrust:Vec2::ZERO,magazine:0}];
            let mut w=World::new(crate::celestial::System {bodies:vec![]},specs,100.0,seed);
            for b in &mut w.bodies {b.controls.evade=controls::Mode::Off;b.controls.ecm=controls::Mode::Off;b.controls.screens=controls::Mode::Off;}
            w.bodies[1].ship_class=Some(ShipClass::Frigate);w.fit_point_defence(BodyId(1));
            w.bodies[1].point_defence.as_mut().unwrap().rate_hz=0.2;
            let c=w.contact_id(FactionId(0),BodyId(1));
            w.perceptions.get_mut(&FactionId(0)).unwrap().ingest(Observation {detection:sensors::DetectionLevel::Resolved,
                contact:c,sensor:BodyId(0),origin:Vec2::ZERO,emitted_at:0.0,sensor_received_at:0.0,decider_received_at:0.0,
                source:Source::Echo,snr:1e12,measurement:Measurement::BearingRange {bearing:0.0,range:2_000_000.0,sigma_range:0.1,sigma_bearing:1e-7}},&w.system);
            let id=w.launch(BodyId(0),c,Payload::Kinetic).unwrap();let due=w.probability_flights[&id].due;
            w.advance_to(0.001);
            assert!(w.state(id,w.time).unwrap().vel.x>19_000.0,"missile must gain on its launcher toward the enemy");
            w.advance_to(due+2.0);
            intercepted+=usize::from(w.losses.iter().any(|l|l.body==id && matches!(l.cause,LossCause::PointDefence {..})));
            hits+=usize::from(w.hits.iter().any(|h|h.missile==id));
        }
        println!("Fast SRMs: laser kills {intercepted}/48, hull hits {hits}/48");
        assert!(intercepted>=16,"local lasers must engage before the 5000 km burst: {intercepted}");
        assert!(hits>0,"point defence must still allow occasional leaks");
    }
    #[test]
    #[ignore = "unsaturated two-layer defence survey"]
    fn unsaturated_defence_survey() {
        let trials=std::env::var("LUMINAL_DEFENCE_TRIALS").ok().and_then(|s|s.parse::<usize>().ok()).unwrap_or(96);
        println!("payload,closing_kms,interceptor_stock,shots,int_kills,pd_engaged,pd_kills,warhead_hits,other_misses");
        for payload in [Payload::Kinetic,Payload::Nuclear] {for closing in [0.0,2000.0,19000.0] {for depth in [0,1,40] {
        let mut intercepted=0;let mut hits=0;let mut outer=0;let mut engaged=0;
        for seed in 0..trials {
            let specs=vec![
                BodySpec {name:"Fast launcher".into(),kind:BodyKind::Ship,faction:FactionId(0),state:State {pos:Vec2::ZERO,vel:Vec2::new(closing,0.0)},thrust:Vec2::ZERO,magazine:10},
                BodySpec {name:"Defender".into(),kind:BodyKind::Ship,faction:FactionId(1),state:State {pos:Vec2::new(0.03*crate::units::AU,0.0),vel:Vec2::ZERO},thrust:Vec2::ZERO,magazine:0}];
            let mut w=World::new(crate::celestial::System {bodies:vec![]},specs,100.0,seed as u64);
            for b in &mut w.bodies {b.controls.evade=controls::Mode::Off;b.controls.ecm=controls::Mode::Off;b.controls.screens=controls::Mode::Off;}
            w.bodies[1].ship_class=Some(ShipClass::Frigate);w.fit_point_defence(BodyId(1));
            w.bodies[1].point_defence.as_mut().unwrap().rate_hz=0.2;
            w.bodies[1].interceptor_battery=Some(interceptor::Battery {rounds:depth,launched:0,ready_at:0.0,status:"Ready"});
            let c=w.contact_id(FactionId(0),BodyId(1));
            w.perceptions.get_mut(&FactionId(0)).unwrap().ingest(Observation {detection:sensors::DetectionLevel::Resolved,
                contact:c,sensor:BodyId(0),origin:Vec2::ZERO,emitted_at:0.0,sensor_received_at:0.0,decider_received_at:0.0,
                source:Source::Echo,snr:1e12,measurement:Measurement::BearingRange {bearing:0.0,range:(0.03*crate::units::AU),sigma_range:0.1,sigma_bearing:1e-7}},&w.system);
            let id=w.launch(BodyId(0),c,payload).unwrap();let due=w.probability_flights[&id].due;
            w.advance_to(0.001);

            w.advance_to(due+2.0);
            intercepted+=usize::from(w.losses.iter().any(|l|l.body==id && matches!(l.cause,LossCause::PointDefence {..})));
            hits+=usize::from(w.hits.iter().any(|h|h.missile==id));
            outer+=usize::from(w.losses.iter().any(|l|l.body==id && matches!(l.cause,LossCause::Interceptor {..})));
            engaged+=usize::from(w.bodies[1].point_defence.unwrap().shots>0);
        }
        println!("{payload:?},{closing},{depth},{trials},{outer},{engaged},{intercepted},{hits},{}",trials-outer-intercepted-hits);
        assert!(outer+intercepted+hits<=trials);
        if depth==0 {assert!((0.35..=0.65).contains(&(intercepted as f64/trials as f64)),"unsaturated full-pass laser stop rate should remain near 50%: {payload:?} at {closing}: {intercepted}/{trials}");}
        }}}
    }

}
