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
    #[test]
    fn transport_cruises_at_25g_then_latches_50g_on_received_resolution() {
        let mut w=world();let id=BodyId(0);
        w.system=crate::scenario::home_system();
        w.bodies[0].trajectory=Trajectory::new(0.0,State {pos:Vec2::new(20.0*crate::units::AU,0.0),vel:Vec2::ZERO});
        w.bodies[0].ship_class=Some(ShipClass::Transport);
        w.set_move(id,Vec2::new(30.0*crate::units::AU,0.0)).unwrap();
        w.update_system_controls(id);
        assert!((w.bodies[0].max_accel()/G0-25.0).abs()<1e-9);
        assert!((w.bodies[0].trajectory.last().thrust.length()/G0-25.0).abs()<1e-9);
        report(&mut w,20.0*LIGHT_SECOND,sensors::DetectionLevel::Approximate);
        w.update_system_controls(id);assert!(!w.bodies[0].controls.transport_alerted);
        report(&mut w,20.0*LIGHT_SECOND,sensors::DetectionLevel::Resolved);
        w.update_system_controls(id);
        assert!(w.bodies[0].controls.transport_alerted);
        assert!((w.bodies[0].trajectory.last().thrust.length()/G0-50.0).abs()<1e-9);
        w.perceptions.get_mut(&FactionId(0)).unwrap().contacts.clear();
        w.update_system_controls(id);
        assert_eq!(w.bodies[0].drive_limit,50.0*G0);
        w.bodies[0].thermal.add_waste_heat(SHIP_HEAT_LIMIT_J*1.25);
        w.guide(id);
        assert!((w.bodies[0].trajectory.last().thrust.length()/G0-25.0).abs()<1e-9);
    }

    #[test]
    fn transport_leaves_battleship_catchup_headroom_and_releases_it_after_follow() {
        let mut w=world();
        w.bodies[0].ship_class=Some(ShipClass::Transport);
        w.bodies[0].controls.transport_alerted=true;
        w.bodies[1].faction=FactionId(0);w.bodies[1].ship_class=Some(ShipClass::Battleship);
        w.bodies[1].trajectory=Trajectory::new(0.0,State {pos:Vec2::new(0.0,LIGHT_SECOND),vel:Vec2::ZERO});
        w.bodies[1].autopilot=Some(Autopilot {order:Order::Follow {target:BodyId(0),offset:Vec2::new(0.0,LIGHT_SECOND)},status:AutopilotStatus::Holding});
        w.reset_platform_history(BodyId(1));w.update_system_controls(BodyId(0));
        assert!((w.bodies[0].drive_limit/G0-37.5).abs()<1e-8);
        w.bodies[1].autopilot=None;w.reset_platform_history(BodyId(1));
        w.update_system_controls(BodyId(0));assert!((w.bodies[0].drive_limit/G0-50.0).abs()<1e-8);
    }

    #[test]
    fn battleship_keeps_up_with_an_alerted_transport_for_an_hour() {
        let origin=Vec2::new(20.0*crate::units::AU,0.0);
        let specs=(0..2).map(|i|BodySpec {name:format!("Convoy {i}"),kind:BodyKind::Ship,faction:FactionId(0),
            state:State {pos:origin+Vec2::new(0.0,i as f64*LIGHT_SECOND),vel:Vec2::ZERO},thrust:Vec2::ZERO,magazine:0}).collect();
        let mut w=World::new(crate::scenario::home_system(),specs,0.0,42);
        w.bodies[0].ship_class=Some(ShipClass::Transport);w.bodies[0].controls.transport_alerted=true;
        w.bodies[1].ship_class=Some(ShipClass::Battleship);w.bodies[1].thermal.capacity_scale=8.0;
        w.set_move(BodyId(0),Vec2::new(30.0*crate::units::AU,0.0)).unwrap();
        w.set_follow(BodyId(1),BodyId(0)).unwrap();w.reset_platform_history(BodyId(1));
        let Order::Follow {offset,..}=w.bodies[1].autopilot.unwrap().order else {panic!("follow");};
        w.advance_to(3600.0);
        let transport=w.state(BodyId(0),w.time).unwrap();let escort=w.state(BodyId(1),w.time).unwrap();
        let gap=(escort.pos-transport.pos-offset).length();let speed=(escort.vel-transport.vel).length();
        assert!(gap<0.1*LIGHT_SECOND,"formation error {gap} km");
        assert!(speed<1.0,"relative speed {speed} km/s");
    }

    #[test] fn auto_uses_resolution_and_screens_latch() {
        let mut w=world();
        w.update_system_controls(BodyId(0));
        assert_eq!(w.bodies[0].controls.ecm,Mode::Auto);
        assert!(!w.bodies[0].controls.ecm_active && !w.bodies[0].screen_up);
        report(&mut w,20.0*LIGHT_SECOND,sensors::DetectionLevel::Approximate);
        w.update_system_controls(BodyId(0));assert!(!w.bodies[0].screen_up);
        report(&mut w,20.0*LIGHT_SECOND,sensors::DetectionLevel::Resolved);
        w.update_system_controls(BodyId(0));assert!(w.bodies[0].screen_up && w.bodies[0].controls.ecm_active);
        w.perceptions.get_mut(&FactionId(0)).unwrap().contacts.clear();
        report(&mut w,5.0*LIGHT_SECOND,sensors::DetectionLevel::Resolved);
        w.update_system_controls(BodyId(0));
        w.perceptions.get_mut(&FactionId(0)).unwrap().contacts.clear();
        w.update_system_controls(BodyId(0));
        assert!(w.bodies[0].screen_up && !w.bodies[0].controls.ecm_active);
        w.set_system_mode(BodyId(0),ControlledSystem::Screens,Mode::Off).unwrap();
        assert!(!w.bodies[0].controls.screens_latched && !w.bodies[0].screen_up);
    }
    #[test] fn rated_thrust_allows_laser_recharging() {
        let mut w=world();w.fit_point_defence(BodyId(0));
        let rated=120.0*G0;
        assert!((w.bodies[0].max_accel()-rated).abs()<1e-9);
        w.set_thrust(BodyId(0),Vec2::new(rated,0.0)).unwrap();
        let b=&mut w.bodies[0];b.thermal.capacitor_j=0.0;
        b.point_defence.as_mut().unwrap().ready_at[0]=0.5;
        b.advance_thermal(20.0);
        assert!(b.thermal.capacitor_j>0.0);
        assert_eq!(b.point_defence.unwrap().ready_at[0],0.5);
        assert!(b.thermal.heat_j>0.0);
    }
    #[test] fn disabled_ecm_and_drive_cannot_be_forced_on() {
        let mut w=world();w.bodies[0].damage.systems[Subsystem::Power as usize]=Condition::Damaged;
        w.set_system_mode(BodyId(0),ControlledSystem::Ecm,Mode::On).unwrap();
        assert!(!w.bodies[0].controls.ecm_active);
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
pub enum ControlledSystem {Ecm,Screens,Active,Evade}
#[derive(Clone,Copy,Debug)]
pub struct Controls {
    pub active:Mode,pub next_ping_at:f64,
    pub transport_alerted:bool,
    pub evade:Mode,pub evading:bool,
    pub evasion:Option<(ContactId,autopilot::EvasionBurn)>,
    /// Locked lateral sign for an Evade with no missile inbound. The key is the contact or own-ship id.
    pub drift_side:Option<(u64,f64)>,
    pub last_auto_ping:Option<f64>,
    pub ecm:Mode,pub screens:Mode,
    pub ecm_active:bool,pub screens_latched:bool,
    pub ecm_rating:f64,pub eccm_rating:f64,
}
impl Default for Controls {
    fn default()->Self {Self {active:Mode::Off,next_ping_at:0.0,transport_alerted:false,evade:Mode::Auto,evading:false,evasion:None,drift_side:None,last_auto_ping:None,ecm:Mode::Auto,screens:Mode::Auto,
        ecm_active:false,screens_latched:false,ecm_rating:100.0,eccm_rating:50.0}}
}
impl Body {
    pub fn ecm_strength(&self)->f64 {
        if self.controls.ecm_active {self.controls.ecm_rating*self.operating_effectiveness(crate::damage::System::Ecm)} else {0.0}
    }
}
impl World {
    pub(super) fn detect_ship(&self,sensor:BodyId,target:BodyId,emitted:f64,range:f64,ping:bool)->sensors::DetectionLevel {
        let b=&self.bodies[sensor.0 as usize];
        if let Some(m)=b.missile {
            let effectiveness=if ping {if b.sensors.active {b.operating_effectiveness(crate::damage::System::Active)} else {0.0}}
                else if b.sensors.passive {b.sensor_effectiveness()[0]} else {0.0};
            return if effectiveness>0.0 && range<=m.payload.seeker_range()*effectiveness {
                sensors::DetectionLevel::Resolved
            } else {sensors::DetectionLevel::None};
        }
        let ef=self.historical_ef(target,emitted);
        let active=self.historical_signature(target,emitted).is_some_and(|s|s.direction_active());
        let eccm=b.controls.eccm_rating*b.operating_effectiveness(crate::damage::System::Eccm);
        let factor=sensors::resolution_factor(self.historical_ecm(target,emitted)*100.0,eccm);
        let suite=if ping {sensors::SensorSuite {passive:false,direction_finding:false,..b.sensors}} else {b.sensors};
        sensors::ship_detection_ew(suite,b.sensor_effectiveness(),if ping {b.operating_effectiveness(crate::damage::System::Active)*b.sensor_rating()/100.0} else {0.0},ef,range,active,factor)
    }
    pub fn set_system_mode(&mut self,id:BodyId,system:ControlledSystem,mode:Mode)->Result<(),OrderError> {
        let t=self.time;
        if matches!(system,ControlledSystem::Active|ControlledSystem::Evade) && mode==Mode::On {return Err(OrderError::InvalidTarget);}
        let b=self.live_body_mut(id)?;
        if b.kind!=BodyKind::Ship || (system==ControlledSystem::Screens && !b.has_screen) {return Err(OrderError::Unarmed);}
        b.advance_thermal(t);
        match system {ControlledSystem::Evade=>b.controls.evade=mode,ControlledSystem::Ecm=>b.controls.ecm=mode,
            ControlledSystem::Screens=>{b.controls.screens=mode;if mode==Mode::Off {b.controls.screens_latched=false;}},
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
        if system==ControlledSystem::Evade {self.guide(id);}
        Ok(())
    }
    pub(super) fn update_system_controls(&mut self,id:BodyId) {
        let b=&self.bodies[id.0 as usize];
        if b.kind!=BodyKind::Ship || !b.alive_at(self.time) {return;}
        let origin=b.trajectory.state_at(self.time).unwrap().pos;
        let mut resolved_ship=false;let mut missile_watch=false;
        if let Some(p)=self.received_picture(id) {for c in p.contacts.values() {
            if self.contact_retired(b.faction,c.id) {continue;}
            let level=c.detection(self.time);
            let kind=if c.resolved {self.body_for_contact(b.faction,c.id).map(|id|self.bodies[id.0 as usize].kind)} else {None};
            resolved_ship|=level>=sensors::DetectionLevel::Resolved && kind==Some(BodyKind::Ship);
            missile_watch|=level>=sensors::DetectionLevel::Resolved && kind==Some(BodyKind::Missile);

        }}
        // Convoy pacing uses delivered friendly telemetry. Leave thrust headroom
        // for the escort to catch up; release this limit when it leaves Follow.
        let convoy_limit=if b.ship_class==Some(ShipClass::Transport) {
            self.bodies.iter().enumerate().filter(|(_,other)|other.faction==b.faction && other.kind==BodyKind::Ship)
                .filter_map(|(i,_)|self.known_body(b.faction,BodyId(i as u32)))
                .filter_map(|escort| {
                    let Some(Autopilot {order:Order::Follow {target,offset},..})=escort.autopilot else {return None;};
                    if target!=id {return None;}
                    let at=escort.trajectory.state_at(self.time)?;
                    let gap=(at.pos-origin-offset).length();
                    let reserve=if gap>2.0*crate::units::LIGHT_SECOND {0.5} else {0.75};
                    Some(escort.max_accel()*reserve)
                }).reduce(f64::min)
        } else {None};
        let b=&mut self.bodies[id.0 as usize];
        b.advance_thermal(self.time);
        let previous_limit=b.drive_limit;
        if b.ship_class==Some(ShipClass::Transport) {
            b.controls.transport_alerted|=resolved_ship;
            b.drive_limit=(crate::units::G0*if b.controls.transport_alerted {50.0} else {25.0}).min(convoy_limit.unwrap_or(f64::INFINITY));
        }
        let auto=|mode,condition|match mode {Mode::On=>true,Mode::Off=>false,Mode::Auto=>condition};
        b.controls.screens_latched|=resolved_ship && b.controls.screens==Mode::Auto;
        let aborted=matches!(b.jump,Some(super::jump::JumpState::Spooling {..})) && b.operating_effectiveness(crate::damage::System::Jump)<=0.0;
        let announce_abort=aborted && b.withdrawing;
        if aborted {
            b.jump=None;b.withdrawing=false;b.thermal.field=0.0;
        }
        b.controls.ecm_active=auto(b.controls.ecm,resolved_ship) && b.operating_effectiveness(crate::damage::System::Ecm)>0.0;
        b.screen_up=b.jump.is_none() && b.has_screen && b.damage.state(crate::damage::System::Screens)!=crate::damage::Condition::Destroyed
            && auto(b.controls.screens,b.controls.screens_latched);
        if !b.screen_up {b.thermal.field=0.0;} else {b.thermal.field=b.thermal.field.min(b.operating_effectiveness(crate::damage::System::Screens));}
        let ping=b.controls.active==Mode::Auto && self.time>=b.controls.next_ping_at;
        let guide=previous_limit!=b.drive_limit || (b.controls.evade==Mode::Auto && missile_watch) || b.controls.evading;
        if aborted {self.announce_ship_event(id,CombatKind::JumpCancelled);}
        if announce_abort {self.announce_ship_event(id,CombatKind::WithdrawalCancelled);}
        if guide {self.guide(id);}
        if ping && self.ping(id) {
            let controls=&mut self.bodies[id.0 as usize].controls;
            if controls.last_auto_ping.is_some() {self.hidden_ping_circles.insert((id,self.time.to_bits()));}
            controls.last_auto_ping=Some(self.time);
            controls.next_ping_at=self.time+60.0;
        }
    }
}
