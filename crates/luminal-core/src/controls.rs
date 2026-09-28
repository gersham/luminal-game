//! Platform automation uses only the platform's delivered tactical picture.
use super::*;

#[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
pub enum Mode {On,Off,#[default] Auto}
impl Mode {
    pub fn next(self)->Self {match self {Self::Auto=>Self::On,Self::On=>Self::Off,Self::Off=>Self::Auto}}
    pub fn label(self)->&'static str {match self {Self::On=>"ON",Self::Off=>"OFF",Self::Auto=>"AUTO"}}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::damage::{Condition,System as Subsystem};
    use crate::units::{G0,LIGHT_SECOND};
    fn world()->World {
        let specs=(0..2).map(|i|BodySpec {name:format!("Ship {i}"),kind:BodyKind::Ship,faction:FactionId(i),
            state:State {pos:Vec2::new(i as f64*20.0*LIGHT_SECOND,0.0),vel:Vec2::ZERO},thrust:Vec2::ZERO,magazine:10}).collect();
        World::new(System {bodies:vec![]},specs,0.0,42)
    }
    fn report(w:&mut World,range:f64,level:sensors::DetectionLevel) {
        let contact=w.contact_id(FactionId(0),BodyId(1));
        w.perceptions.get_mut(&FactionId(0)).unwrap().ingest(Observation {contact,sensor:BodyId(0),origin:Vec2::ZERO,
            emitted_at:w.time,sensor_received_at:w.time,decider_received_at:w.time,source:Source::Echo,detection:level,snr:100.0,
            measurement:Measurement::BearingRange {bearing:0.0,range,sigma_range:0.1,sigma_bearing:1e-7}},&w.system);
    }
    #[test] fn auto_uses_resolution_screens_latch_and_boost_yields_to_nearby_contacts() {
        let mut w=world();
        w.update_system_controls(BodyId(0));
        assert_eq!(w.bodies[0].controls.ecm,Mode::Auto);
        assert!(!w.bodies[0].controls.ecm_active && !w.bodies[0].screen_up && w.bodies[0].controls.boost_active);
        report(&mut w,20.0*LIGHT_SECOND,sensors::DetectionLevel::Approximate);
        w.update_system_controls(BodyId(0));assert!(!w.bodies[0].screen_up);
        report(&mut w,20.0*LIGHT_SECOND,sensors::DetectionLevel::Resolved);
        w.update_system_controls(BodyId(0));assert!(w.bodies[0].screen_up && w.bodies[0].controls.ecm_active);
        w.perceptions.get_mut(&FactionId(0)).unwrap().contacts.clear();
        report(&mut w,5.0*LIGHT_SECOND,sensors::DetectionLevel::Resolved);
        w.update_system_controls(BodyId(0));assert!(!w.bodies[0].controls.boost_active);
        w.perceptions.get_mut(&FactionId(0)).unwrap().contacts.clear();
        w.update_system_controls(BodyId(0));
        assert!(w.bodies[0].screen_up && !w.bodies[0].controls.ecm_active);
        w.set_system_mode(BodyId(0),ControlledSystem::Screens,Mode::Off).unwrap();
        assert!(!w.bodies[0].controls.screens_latched && !w.bodies[0].screen_up);
    }
    #[test] fn boost_adds_twenty_percent_and_pauses_both_laser_recharges() {
        let mut w=world();w.fit_point_defence(BodyId(0));
        w.set_system_mode(BodyId(0),ControlledSystem::Boost,Mode::Off).unwrap();
        let nominal=w.bodies[0].max_accel();
        w.set_thrust(BodyId(0),Vec2::new(nominal,0.0)).unwrap();
        w.set_system_mode(BodyId(0),ControlledSystem::Boost,Mode::On).unwrap();
        assert!((w.bodies[0].trajectory.last().thrust.length()/nominal-1.2).abs()<1e-9);
        let b=&mut w.bodies[0];
        b.thermal.capacitor_j=0.0;b.point_defence.as_mut().unwrap().next_shot_at=0.5;
        b.advance_thermal(20.0);
        assert_eq!(b.thermal.capacitor_j,0.0);
        assert_eq!(b.point_defence.unwrap().next_shot_at,20.5);
        w.time=20.0;w.set_system_mode(BodyId(0),ControlledSystem::Boost,Mode::Off).unwrap();
        w.bodies[0].advance_thermal(21.0);
        assert!(w.bodies[0].thermal.capacitor_j>0.0);
        assert_eq!(w.bodies[0].point_defence.unwrap().next_shot_at,20.5);
        w.set_thrust(BodyId(0),Vec2::new(10.0*G0,0.0)).unwrap();
        w.set_system_mode(BodyId(0),ControlledSystem::Boost,Mode::On).unwrap();
        assert!((w.bodies[0].trajectory.last().thrust.length()-10.0*G0).abs()<1e-9,"partial thrust is not boosted");
    }
    #[test] fn disabled_ecm_and_drive_cannot_be_forced_on() {
        let mut w=world();w.bodies[0].damage.systems[Subsystem::Power as usize]=Condition::Damaged;
        for system in [ControlledSystem::Ecm,ControlledSystem::Boost] {w.set_system_mode(BodyId(0),system,Mode::On).unwrap();}
        assert!(!w.bodies[0].controls.ecm_active && !w.bodies[0].controls.boost_active);
        assert_eq!(w.bodies[0].ecm_strength(),0.0);assert_eq!(w.bodies[0].max_accel(),0.0);
    }
    #[test] fn active_auto_repeats_without_contacts_until_explicitly_disabled() {
        let mut w=world();assert_eq!(w.bodies[0].controls.active,Mode::Off);
        w.set_system_mode(BodyId(0),ControlledSystem::Active,Mode::Auto).unwrap();
        for t in [59.0,60.0,119.0,120.0] {w.time=t;w.update_system_controls(BodyId(0));}
        let times:Vec<_>=w.ping_emissions.iter().filter(|(id,_)|*id==BodyId(0)).map(|(_,f)|f.t_emit).collect();
        assert_eq!(times,vec![0.0,60.0,120.0]);
        assert!(!w.hidden_ping_circles.contains(&(BodyId(0),0.0_f64.to_bits())));
        assert!(w.hidden_ping_circles.contains(&(BodyId(0),60.0_f64.to_bits())));
        assert!(w.hidden_ping_circles.contains(&(BodyId(0),120.0_f64.to_bits())));
        assert_eq!(w.bodies[0].controls.active,Mode::Auto);
        w.set_system_mode(BodyId(0),ControlledSystem::Active,Mode::Off).unwrap();
        assert!(!w.hidden_ping_circles.contains(&(BodyId(0),120.0_f64.to_bits())),"final actual pulse becomes visible");
        assert!(w.hidden_ping_circles.contains(&(BodyId(0),60.0_f64.to_bits())),"intermediate pulses stay hidden");
        w.time=180.0;w.update_system_controls(BodyId(0));
        assert_eq!(w.ping_emissions.iter().filter(|(id,_)|*id==BodyId(0)).count(),3);
        w.set_system_mode(BodyId(0),ControlledSystem::Active,Mode::Auto).unwrap();
        assert!(!w.hidden_ping_circles.contains(&(BodyId(0),180.0_f64.to_bits())),"new AUTO cycle shows its first pulse");
        w.time=190.0;assert!(w.ping(BodyId(0)));
        assert!(!w.hidden_ping_circles.contains(&(BodyId(0),190.0_f64.to_bits())),"manual pings stay visible during AUTO");
    }
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum ControlledSystem {Ecm,Screens,Boost,Active}
#[derive(Clone,Copy,Debug)]
pub struct Controls {
    pub active:Mode,pub next_ping_at:f64,
    pub last_auto_ping:Option<f64>,
    pub ecm:Mode,pub screens:Mode,pub boost:Mode,
    pub ecm_active:bool,pub boost_active:bool,pub screens_latched:bool,
    pub ecm_rating:f64,pub eccm_rating:f64,
}
impl Default for Controls {
    fn default()->Self {Self {active:Mode::Off,next_ping_at:0.0,last_auto_ping:None,ecm:Mode::Auto,screens:Mode::Auto,boost:Mode::Auto,
        ecm_active:false,boost_active:false,screens_latched:false,ecm_rating:100.0,eccm_rating:50.0}}
}
impl Body {
    pub fn ecm_strength(&self)->f64 {
        if self.controls.ecm_active {self.controls.ecm_rating*self.operating_effectiveness(crate::damage::System::Ecm)} else {0.0}
    }
}
impl World {
    pub(super) fn detect_ship(&self,sensor:BodyId,target:BodyId,emitted:f64,range:f64,ping:bool)->sensors::DetectionLevel {
        let b=&self.bodies[sensor.0 as usize];
        let ef=self.historical_ef(target,emitted);
        let active=self.historical_signature(target,emitted).is_some_and(|s|s.direction_active());
        let eccm=b.controls.eccm_rating*b.operating_effectiveness(crate::damage::System::Eccm);
        let factor=sensors::resolution_factor(self.historical_ecm(target,emitted)*100.0,eccm);
        let suite=if ping {sensors::SensorSuite {passive:false,direction_finding:false,..b.sensors}} else {b.sensors};
        sensors::ship_detection_ew(suite,b.sensor_effectiveness(),if ping {b.operating_effectiveness(crate::damage::System::Active)} else {0.0},ef,range,active,factor)
    }
    pub fn set_system_mode(&mut self,id:BodyId,system:ControlledSystem,mode:Mode)->Result<(),OrderError> {
        let t=self.time;
        if system==ControlledSystem::Active && mode==Mode::On {return Err(OrderError::InvalidTarget);}
        let b=self.live_body_mut(id)?;
        if b.kind!=BodyKind::Ship || (system==ControlledSystem::Screens && !b.has_screen) {return Err(OrderError::Unarmed);}
        b.advance_thermal(t);
        match system {ControlledSystem::Ecm=>b.controls.ecm=mode,
            ControlledSystem::Screens=>{b.controls.screens=mode;if mode==Mode::Off {b.controls.screens_latched=false;}},
            ControlledSystem::Boost=>b.controls.boost=mode,
            ControlledSystem::Active=>{
                if b.controls.active!=mode {
                    let last=b.controls.last_auto_ping;
                    b.controls.last_auto_ping=None;
                    b.controls.active=mode;b.controls.next_ping_at=t;
                    if mode==Mode::Off && let Some(at)=last {
                        // Reveal the actual final pulse; do not emit another one
                        // or restart its light-travel animation when switching off.
                        self.hidden_ping_circles.remove(&(id,at.to_bits()));
                    }
                }
            }}
        self.update_system_controls(id);
        Ok(())
    }
    pub(super) fn update_system_controls(&mut self,id:BodyId) {
        let b=&self.bodies[id.0 as usize];
        if b.kind!=BodyKind::Ship || !b.alive_at(self.time) {return;}
        let origin=b.trajectory.state_at(self.time).unwrap().pos;
        let mut resolved_ship=false;let mut nearby=false;
        if let Some(p)=self.received_picture(id) {for c in p.contacts.values() {
            if self.contact_retired(b.faction,c.id) {continue;}
            let level=c.detection(self.time);
            let kind=if c.resolved {self.body_for_contact(b.faction,c.id).map(|id|self.bodies[id.0 as usize].kind)} else {None};
            resolved_ship|=level>=sensors::DetectionLevel::Resolved && kind==Some(BodyKind::Ship);
            if level>=sensors::DetectionLevel::Approximate && kind.is_none_or(|k|matches!(k,BodyKind::Ship|BodyKind::Missile))
                && let Some(tr)=c.estimate(self.time,&self.system) {
                nearby|=(tr.pos()-origin).length()<=10.0*crate::units::LIGHT_SECOND;
            }
        }}
        let b=&mut self.bodies[id.0 as usize];
        b.advance_thermal(self.time);
        let previous=b.controls.boost_active;
        let auto=|mode,condition|match mode {Mode::On=>true,Mode::Off=>false,Mode::Auto=>condition};
        b.controls.screens_latched|=resolved_ship && b.controls.screens==Mode::Auto;
        b.controls.ecm_active=auto(b.controls.ecm,resolved_ship) && b.operating_effectiveness(crate::damage::System::Ecm)>0.0;
        b.controls.boost_active=auto(b.controls.boost,!nearby) && b.operating_effectiveness(crate::damage::System::Propulsion)>0.0;
        b.screen_up=b.has_screen && b.damage.state(crate::damage::System::Screens)!=crate::damage::Condition::Destroyed
            && auto(b.controls.screens,b.controls.screens_latched);
        let ping=b.controls.active==Mode::Auto && self.time>=b.controls.next_ping_at;
        if previous!=b.controls.boost_active {self.guide(id);}
        if ping && self.ping(id) {
            let controls=&mut self.bodies[id.0 as usize].controls;
            if controls.last_auto_ping.is_some() {self.hidden_ping_circles.insert((id,self.time.to_bits()));}
            controls.last_auto_ping=Some(self.time);
            controls.next_ping_at=self.time+60.0;
        }
    }
}
