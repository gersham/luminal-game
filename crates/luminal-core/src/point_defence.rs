//! Autonomous missile defence. Detection and reporting are causal; lethality is
//! an explicit range-calibrated probability, not the offensive laser damage model.
use super::*;
use crate::units::{C,LIGHT_SECOND};
pub(super) type Sighting=(f64,BodyId,Vec2,f64,Measurement,f64);

#[derive(Clone,Copy,Debug)]
pub struct PointDefence {
    pub rate_hz:f64,
    pub shots:u64,
    pub lasers:u8,
    /// Independent recharge clocks for up to eight emplacements.
    pub ready_at:[f64;8],
    /// One tracked threat per mount; larger batteries defend more simultaneous arrivals.
    pub targets:[Option<BodyId>;8],
    pub last_shot:Option<(f64,Vec2,Vec2)>,
}
impl Default for PointDefence {
    fn default()->Self { Self {rate_hz:PD_RATE_HZ.value,shots:0,lasers:1,ready_at:[0.0;8],targets:[None;8],last_shot:None} }
}

/// Per-mount pulse odds calibrated to a 50% complete, unsaturated approach.
/// Independent low-probability pulses accumulate over the received closing time.
/// Each mount stays assigned to one threat, including during recharge.
pub fn pulse_probability(closing_kms:f64,burst_radius_km:f64,rate_hz:f64)->f64 {
    let width=(PD_LASER_MAX_RANGE_LS.value*LIGHT_SECOND-burst_radius_km).max(1.0);
    let opportunities=width/closing_kms.max(1000.0)*rate_hz;
    (1.0-(1.0-PD_PASS_KILL_CHANCE.value).powf(1.0/opportunities.max(0.001))).min(0.85)
}

pub fn defence_ring_radius(laser:bool,interceptors:bool)->f64 {
    let beam=if laser {PD_LASER_MAX_RANGE_LS.value*LIGHT_SECOND} else {0.0};
    beam.max(if interceptors {interceptor::nominal_range()} else {0.0})
}

#[derive(Clone,Copy,Debug)]
pub(super) struct Pulse {
    shooter:BodyId,target:BodyId,front:Front,hit:bool,
}

impl World {
    pub fn fit_point_defence(&mut self,id:BodyId) {
        if self.bodies[id.0 as usize].point_defence.is_some() {return;}
        let lasers=self.bodies[id.0 as usize].ship_class.map_or(1,ShipClass::pd_lasers);
        self.bodies[id.0 as usize].point_defence=Some(PointDefence {lasers,..Default::default()});
        self.scheduler.schedule(self.time,Event::PointDefence(id));
    }

    pub(super) fn point_defence_cycle(&mut self,id:BodyId) {
        let t=self.time;
        self.bodies[id.0 as usize].advance_thermal(t);
        let b=&self.bodies[id.0 as usize];
        let Some(pd)=b.point_defence else {return};
        if b.trajectory.end().is_none() && matches!(b.jump,Some(jump::JumpState::Transit {..})) {
            self.scheduler.schedule(t+1.0,Event::PointDefence(id));return;
        }
        if !b.alive_at(t) || !pd.rate_hz.is_finite() || pd.rate_hz<=0.0 {return;}
        let origin=b.trajectory.state_at(t).unwrap().pos;
        let faction=b.faction;
        let suite=b.sensors;
        let sensor_effectiveness=b.sensor_effectiveness();
        let laser_effectiveness=b.operating_effectiveness(crate::damage::System::PdLaser);
        let mut candidates=Vec::new();
        // Dedicated local fire control must detect a missile before engaging it.
        // Use light-delayed emission, never its current truth position/velocity.
        for (i,target) in self.bodies.iter().enumerate() {
            if target.faction==faction || target.kind!=BodyKind::Missile || !target.alive_at(t) {continue;}
            // Retained boost tracks and mandatory SRM/terminal resolution are
            // valid fire-control information, even when a plume is now dark.
            if sensor_effectiveness.iter().any(|v|*v>0.0)
                && let Some(contact)=self.association.get(&(faction,BodyId(i as u32)))
                && let Some(track)=self.received_picture(id).and_then(|p|p.contacts.get(contact))
                    .filter(|c|c.detection(t)>=sensors::DetectionLevel::Resolved)
                    .and_then(|c|c.estimate(t,&self.system)) {
                let rel=track.pos()-origin;let range=rel.length();
                if range<=PD_MAX_RANGE_LS.value*LIGHT_SECOND && self.system.occluder(track.pos(),t,origin,t).is_none() {
                    let measurement=Measurement::BearingRange {range,bearing:bearing_of(rel),sigma_range:1.0,sigma_bearing:1e-7};
                    candidates.push((range,BodyId(i as u32),track.pos(),t,measurement,1e12));
                    continue;
                }
            }
            let Some((emitted,seen))=retarded_state(&target.trajectory,origin,t) else {continue};
            let rel=seen.pos-origin;
            let range=rel.length();
            if range>PD_MAX_RANGE_LS.value*LIGHT_SECOND || self.system.occluder(seen.pos,emitted,origin,t).is_some() {continue;}
            let thrust=target.trajectory.thrust_at(emitted).unwrap_or(Vec2::ZERO);
            let power=emission_w(target.kind,target.baseline_emission_factor,thrust)+self.thermal_emission(BodyId(i as u32),emitted);
            let Some((measurement,snr))=sensors::receive_measurement_scaled(sensors::SensorSuite {direction_finding:false,..suite},
                power*PASSIVE_NOISE_FLOOR.value/PD_SENSOR_NOISE_FLOOR.value,range,bearing_of(rel),sensor_effectiveness,&mut self.rng) else {continue};
            if let Measurement::BearingRange {range,bearing,..}=measurement {
                candidates.push((range,BodyId(i as u32),origin+Vec2::new(bearing.cos(),bearing.sin())*range,emitted,measurement,snr));
            }
        }
        candidates.sort_by(|a,b|a.0.total_cmp(&b.0));
        self.consider_interceptors(id,&candidates);
        let targets:Vec<_>=candidates.iter().copied().filter(|c|c.0<=PD_LASER_MAX_RANGE_LS.value*LIGHT_SECOND).collect();
        let mut assignments=pd.targets;
        for assigned in &mut assignments {if assigned.is_some_and(|id|!targets.iter().any(|c|c.1==id)) {*assigned=None;}}
        for candidate in &targets {
            if assignments.contains(&Some(candidate.1)) {continue;}
            if let Some(slot)=(0..usize::from(pd.lasers.min(8))).filter(|i|assignments[*i].is_none())
                .min_by(|a,b|pd.ready_at[*a].total_cmp(&pd.ready_at[*b])) {assignments[slot]=Some(candidate.1);}
        }
        self.bodies[id.0 as usize].point_defence.as_mut().unwrap().targets=assignments;
        for (laser,&assigned) in assignments.iter().enumerate().take(usize::from(pd.lasers.min(8))) {
        if let Some((range,target,aim,emitted,measurement,snr))=assigned.and_then(|id|targets.iter().find(|c|c.1==id)).copied()
            && !self.bodies[id.0 as usize].thermal.dumping
            && self.bodies[id.0 as usize].thermal.heat_j+PD_WASTE_HEAT_J<=BEAM_HEAT_LIMIT_J.value*self.bodies[id.0 as usize].thermal.capacity_scale
            && laser_effectiveness>0.0 && t>=pd.ready_at[laser] && range<=PD_LASER_MAX_RANGE_LS.value*LIGHT_SECOND {
            let contact=self.contact_id(faction,target);
            self.relays.push(Relay {faction,front:Front {origin,t_emit:t},obs:Observation {detection:crate::sensors::DetectionLevel::Resolved,
                contact,sensor:id,origin,emitted_at:emitted,sensor_received_at:t,decider_received_at:f64::NAN,
                measurement,snr,source:Source::Emission}});
            self.bodies[id.0 as usize].advance_thermal(t);
            self.bodies[id.0 as usize].thermal.add_waste_heat(PD_WASTE_HEAT_J);
            let pd=self.bodies[id.0 as usize].point_defence.as_mut().unwrap();
            pd.shots+=1; pd.ready_at[laser]=t+1.0/(pd.rate_hz*laser_effectiveness);
            pd.last_shot=Some((t,origin,aim));
            let rate=pd.rate_hz;
            let own_velocity=self.state(id,t).unwrap().vel;
            let seen_velocity=retarded_state(&self.bodies[target.0 as usize].trajectory,origin,t).map_or(Vec2::ZERO,|(_,s)|s.vel);
            let closing=(own_velocity-seen_velocity).dot((aim-origin).normalized()).max(0.0);
            let burst=self.bodies[target.0 as usize].missile.map_or(0.0,|m|m.payload.kill_radius());
            let chance=pulse_probability(closing,burst,rate);
            let pulse=Pulse {shooter:id,target,front:Front {origin,t_emit:t},hit:self.rng.uniform()<chance};
            self.debug_note("PD_SHOT",format!("shooter={id:?} target={target:?} range_km={range} chance={} hit_roll={}",chance,pulse.hit));
            self.scheduler.schedule(t+(range/C).max(0.001),Event::PointDefencePulse(pulse));
            self.record_beam(t,origin,CombatKind::PointDefence,id,target,faction);
        }
        }
        // Laser reload must not slow sensor acquisition or interceptor launches.
        let tracking_step=if candidates.first().is_some_and(|c|c.0<0.1*LIGHT_SECOND) {0.1} else {1.0};
        self.scheduler.schedule(t+(1.0/pd.rate_hz).min(tracking_step),Event::PointDefence(id));
    }

    pub(super) fn resolve_point_defence(&mut self,pulse:Pulse) {
        if !self.bodies[pulse.target.0 as usize].alive_at(self.time) {return;}
        let trajectory=&self.bodies[pulse.target.0 as usize].trajectory;
        let Some(arrival)=pulse.front.arrival(trajectory,pulse.front.t_emit,self.time) else {
            if trajectory.end().is_none() && let Some(state)=trajectory.state_at(self.time) {
                let wait=(((state.pos-pulse.front.origin).length()-pulse.front.radius_at(self.time))/C).max(0.001);
                self.scheduler.schedule(self.time+wait,Event::PointDefencePulse(pulse));
            }
            return;
        };
        let Some(target)=trajectory.state_at(arrival) else {return};
        let kill=pulse.hit && self.system.occluder(pulse.front.origin,pulse.front.t_emit,target.pos,arrival).is_none();
        self.debug_note("PD_RESULT",format!("shooter={:?} target={:?} arrival={arrival:.6} kill={kill}",pulse.shooter,pulse.target));
        if kill {
            // A successful PD hit destroys a missile outright, without detonating
            // its warhead or routing through ship hull/screen damage thresholds.
            self.destroy(pulse.target,arrival,LossCause::PointDefence {shooter:pulse.shooter});
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::celestial::{Celestial,CelestialKind,Orbit,System};
    fn fixture(kind:BodyKind,range:f64)->World {
        let system=System {bodies:vec![Celestial {name:"Star".into(),kind:CelestialKind::Star,gm:1.0,radius:1.0,orbit:Orbit::Fixed(Vec2::ZERO)}]};
        let specs=vec![BodySpec {name:"Defender".into(),kind,faction:FactionId(0),state:State {pos:Vec2::new(20.0*AU,0.0),vel:Vec2::ZERO},thrust:Vec2::ZERO,magazine:0},
            BodySpec {name:"Missile".into(),kind:BodyKind::Missile,faction:FactionId(1),state:State {pos:Vec2::new(20.0*AU,range),vel:Vec2::ZERO},thrust:Vec2::ZERO,magazine:0}];
        let mut world=World::new(system,specs,10.0,123);
        // Test a sensor-equipped defender; playtest stations default to DF only.
        world.bodies[0].sensors=sensors::SensorSuite::FULL;
        if kind==BodyKind::Ship {world.bodies[0].ship_class=Some(ShipClass::Picket);}
        world
    }
    #[test]
    fn battery_splits_targets_and_recharges_each_laser_independently() {
        let mut w=fixture(BodyKind::Ship,0.005*LIGHT_SECOND);
        w.bodies[0].ship_class=Some(ShipClass::Battleship);
        // A second local missile, visible to the dedicated PD sensors.
        let mut second=w.bodies[1].clone();
        second.trajectory=Trajectory::new(-10.0,State {pos:Vec2::new(20.0*AU,0.006*LIGHT_SECOND),vel:Vec2::ZERO});
        w.bodies.push(second);w.last_step.push(0.0);
        w.fit_point_defence(BodyId(0));
        w.bodies[0].point_defence.as_mut().unwrap().rate_hz=2.0;
        w.point_defence_cycle(BodyId(0));
        let pd=w.bodies[0].point_defence.unwrap();
        assert_eq!(pd.lasers,8);assert_eq!(pd.shots,2);assert_eq!(pd.ready_at,[0.5,0.5,0.0,0.0,0.0,0.0,0.0,0.0]);
        let events=w.combat_events(None);
        for target in [BodyId(1),BodyId(2)] {
            assert_eq!(events.iter().filter(|e|e.kind==CombatKind::PointDefence && e.target==Some(InterceptTarget::Own(target))).count(),1);
        }
        assert!((w.bodies[0].thermal.heat_j-2.0*PD_WASTE_HEAT_J).abs()<1.0);
        w.point_defence_cycle(BodyId(0));assert_eq!(w.bodies[0].point_defence.unwrap().shots,2);
        // Only one mount is recharged; the other seven cannot fire early.
        w.time=0.25;w.bodies[0].point_defence.as_mut().unwrap().ready_at[1]=0.25;
        w.point_defence_cycle(BodyId(0));
        let pd=w.bodies[0].point_defence.unwrap();assert_eq!(pd.shots,3);assert_eq!(pd.ready_at[1],0.75);
        assert_eq!(pd.ready_at[0],0.5);
        w.bodies[0].damage.systems[crate::damage::System::PdLaser as usize]=crate::damage::Condition::Destroyed;
        w.time=1.0;w.point_defence_cycle(BodyId(0));assert_eq!(w.bodies[0].point_defence.unwrap().shots,3);
    }

    #[test]
    fn interceptors_acquire_and_launch_near_point_three_au() {
        let mut w=fixture(BodyKind::Ship,0.26*crate::units::AU);
        w.bodies[0].interceptor_battery=Some(interceptor::Battery {rounds:20,launched:0,ready_at:0.0,status:"Ready"});
        w.fit_point_defence(BodyId(0));
        w.advance_to(180.0);
        let battery=w.bodies[0].interceptor_battery.unwrap();
        assert!(battery.launched>0,"outer-envelope acquisition and launch: {}",battery.status);
        assert_eq!(w.bodies[0].point_defence.unwrap().shots,0,"lasers remain last ditch");
    }

    #[test]
    fn real_local_sensor_cycle_launches_and_reports_interceptor() {
        let mut w=fixture(BodyKind::Ship,6.0*LIGHT_SECOND);
        w.bodies[0].interceptor_battery=Some(interceptor::Battery {rounds:20,launched:0,ready_at:0.0,status:"Ready"});
        w.fit_point_defence(BodyId(0));
        w.advance_to(10.0);
        let battery=w.bodies[0].interceptor_battery.unwrap();
        assert_eq!(battery.launched,1,"{}",battery.status);
        assert_eq!(w.bodies[0].point_defence.unwrap().shots,0,"outer defence must use interceptors, not laser fire");
        let id=BodyId(w.bodies.iter().position(|b|b.interceptor.is_some()).unwrap() as u32);
        assert!(w.known_body(FactionId(0),id).is_some(),"launch must be visible before next ten-second telemetry sample");
        assert!(defence_ring_radius(true,true)>10.0*defence_ring_radius(true,false));
    }

    #[test]
    fn heat_dump_inhibits_pd_laser_until_switched_off() {
        let mut w=fixture(BodyKind::Ship,0.025*LIGHT_SECOND);
        w.fit_point_defence(BodyId(0));
        w.set_heat_dump(BodyId(0),true).unwrap();
        w.point_defence_cycle(BodyId(0));
        assert_eq!(w.bodies[0].point_defence.unwrap().shots,0);
        w.set_heat_dump(BodyId(0),false).unwrap();
        w.point_defence_cycle(BodyId(0));
        assert_eq!(w.bodies[0].point_defence.unwrap().shots,1);
        assert!(w.bodies[0].thermal.heat_j>=PD_WASTE_HEAT_J);
    }

    #[test]
    fn inner_five_light_seconds_reserve_missiles_and_allow_lasers() {
        for range in [0.025,0.1,2.0,4.5] {
            let mut w=fixture(BodyKind::Ship,range*LIGHT_SECOND);
            w.bodies[0].interceptor_battery=Some(interceptor::Battery {rounds:20,launched:0,ready_at:0.0,status:"Ready"});
            w.fit_point_defence(BodyId(0));
            w.advance_to(12.0);
            assert_eq!(w.bodies[0].interceptor_battery.unwrap().launched,0);
            if range<=3.0 {assert!(w.bodies[0].point_defence.unwrap().shots>0);}
            else {assert_eq!(w.bodies[0].point_defence.unwrap().shots,0,"hold fire outside useful laser range");}
        }
    }

    #[test]
    fn successful_pulse_waits_for_light_and_kills_missile_in_one_hit() {
        let mut w=fixture(BodyKind::Station,LIGHT_SECOND);
        let origin=w.state(BodyId(0),0.0).unwrap().pos;
        let pulse=Pulse {shooter:BodyId(0),target:BodyId(1),front:Front {origin,t_emit:0.0},hit:true};
        w.scheduler.schedule(0.5,Event::PointDefencePulse(pulse));
        w.advance_to(0.99);
        assert!(w.bodies[1].alive_at(w.time()));
        w.advance_to(1.01);
        assert!(!w.bodies[1].alive_at(w.time()));
        assert!(matches!(w.losses[0].cause,LossCause::PointDefence {shooter:BodyId(0)}));
        assert!(!w.refinement.truth_events.iter().any(|e|e.kind==CombatKind::NuclearBurst));
    }
    #[test]
    fn autonomous_unarmed_platforms_engage_but_need_sensors_and_reject_friendlies() {
        for kind in [BodyKind::Ship,BodyKind::Station] {
            let mut w=fixture(kind,0.005*LIGHT_SECOND);
            w.fit_point_defence(BodyId(0));
            w.advance_to(0.001);
            assert_eq!(w.bodies[0].point_defence.unwrap().shots,1);
            assert!(w.combat_events(None).iter().any(|e|e.kind==CombatKind::PointDefence && e.pos.is_some() && e.aim.is_some()),
                "friendly PD shot must carry source-to-target geometry");
            assert!(w.bodies[1].alive_at(w.time()));
            // The weakened laser can miss its first shot; allow repeat fire.
            w.advance_to(6000.0);
            assert!(!w.bodies[1].alive_at(w.time()));
            assert!(!w.bodies[0].armed);
        }
        let mut w=fixture(BodyKind::Ship,0.025*LIGHT_SECOND);
        w.bodies[0].sensors.passive=false;
        w.fit_point_defence(BodyId(0)); w.advance_to(5.0);
        assert_eq!(w.bodies[0].point_defence.unwrap().shots,0);
        w.bodies[0].sensors.passive=true; w.bodies[1].faction=FactionId(0);
        w.advance_to(10.0);
        assert_eq!(w.bodies[0].point_defence.unwrap().shots,0);
    }
    #[test]
    fn fitted_rate_limits_shots_without_bankable_bursts() {
        let mut w=fixture(BodyKind::Ship,0.025*LIGHT_SECOND);
        w.fit_point_defence(BodyId(0));
        // Repeated control calls at one instant cannot unload extra shots.
        w.point_defence_cycle(BodyId(0));
        for _ in 0..10 {w.point_defence_cycle(BodyId(0));}
        assert_eq!(w.bodies[0].point_defence.unwrap().shots,1);
        assert_eq!(w.bodies[0].point_defence.unwrap().ready_at[0],1.0);
    }
    #[test]
    fn blocked_pulse_and_missed_pulse_do_not_kill() {
        for blocked in [false,true] {
            let mut w=fixture(BodyKind::Station,LIGHT_SECOND);
            let origin=w.state(BodyId(0),0.0).unwrap().pos;
            if blocked {w.system.bodies.push(Celestial {name:"Occluder".into(),kind:CelestialKind::Moon,
                gm:1.0,radius:100.0,orbit:Orbit::Fixed(origin+Vec2::new(0.0,0.06*LIGHT_SECOND))});}
            w.scheduler.schedule(1.01,Event::PointDefencePulse(Pulse {shooter:BodyId(0),target:BodyId(1),
                front:Front {origin,t_emit:0.0},hit:blocked}));
            w.advance_to(1.02);
            assert!(w.bodies[1].alive_at(w.time()));
        }
    }
    #[test]
    fn rate_is_per_emplacement_and_configurable() {
        let mut w=fixture(BodyKind::Station,0.025*LIGHT_SECOND);
        w.fit_point_defence(BodyId(0));
        w.bodies[0].point_defence.as_mut().unwrap().rate_hz=2.0;
        w.point_defence_cycle(BodyId(0));
        assert_eq!(w.bodies[0].point_defence.unwrap().ready_at[0],0.5);
        let mut w=fixture(BodyKind::Ship,0.025*LIGHT_SECOND);
        w.fit_point_defence(BodyId(0));
        w.bodies[0].point_defence.as_mut().unwrap().rate_hz=0.5;
        w.bodies[0].interceptor_battery=Some(interceptor::Battery {rounds:20,launched:0,ready_at:0.0,status:"Ready"});
        w.point_defence_cycle(BodyId(0));
        assert_eq!(w.bodies[0].point_defence.unwrap().ready_at[0],2.0);
        w.time=1.0;
        w.point_defence_cycle(BodyId(0));
        assert_eq!(w.bodies[0].point_defence.unwrap().shots,1);
        assert_eq!(w.interceptor_solutions[&(BodyId(0),BodyId(1))].samples,2,"interceptor sensing continues during beam reload");
    }
    #[test]
    fn unsaturated_pulses_accumulate_to_half_across_the_envelope() {
        for closing in [1000.0,10_000.0,30_000.0] {
            let width=3.0*LIGHT_SECOND-5000.0;let rate=0.2;
            let shots=width/closing*rate;
            let p=pulse_probability(closing,5000.0,rate);
            assert!((1.0-(1.0-p).powf(shots)-0.5).abs()<1e-10);
        }
    }
    #[test]
    fn coverage_ring_shrinks_when_interceptors_run_out() {
        assert_eq!(defence_ring_radius(true,false),3.0*LIGHT_SECOND);
        assert!(defence_ring_radius(true,true)>defence_ring_radius(true,false));
        assert_eq!(defence_ring_radius(false,false),0.0);
    }
}
