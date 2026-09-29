//! Bounded fast-forward based only on the caller's received tactical picture.
use super::*;
use crate::damage::{Condition,System};
use crate::sensors::DetectionLevel;
use crate::world::jump::JumpState;

#[derive(PartialEq)]
struct Picture {
    contacts:Vec<(ContactId,DetectionLevel)>,
    ships:Vec<(BodyId,[Condition;System::COUNT],u8)>,
    combat_at:f64,
    beam_contacts:Vec<ContactId>,
}
impl Picture {
    fn from(v:&View)->Self {
        Self {
            contacts:v.contacts.iter().map(|c|(c.id,c.detection)).collect(),
            ships:v.bodies.iter().filter(|b|b.controllable).map(|b|(b.id,b.damage.damage.systems,match b.jump {None=>0,Some(JumpState::Spooling {..})=>1,Some(JumpState::Transit {..})=>2})).collect(),
            combat_at:v.combat.iter().map(|e|e.received_at).max_by(f64::total_cmp).unwrap_or(f64::NEG_INFINITY),
            beam_contacts:v.contacts.iter().filter(|c|c.track.as_ref().is_some_and(|tr|v.bodies.iter().any(|b|b.controllable && (tr.pos-b.pos).length()<=params::SHIP_BEAM_AUTO_RANGE_LS.value*crate::units::LIGHT_SECOND))).map(|c|c.id).collect(),
        }
    }
}
pub(super) struct EventWait {role:Role,until:f64,warp:f64,picture:Picture}
impl LocalSession {
    pub fn waiting_for_event(&self)->bool {self.event_wait.is_some()}
    pub fn advance_to_next_event(&mut self,role:Role) {
        if self.waiting_for_event() || self.view(role).outcome.is_some() {return;}
        let picture=Picture::from(&self.view(role));
        self.event_wait=Some(EventWait {role,until:self.world.time()+24.0*3600.0,warp:self.warp,picture});
        self.paused=false;self.warp=10000.0;
        self.last_alert=Some((self.world.time(),"Advancing to next received tactical event (24 h limit)".into()));
    }
    pub(super) fn cancel_event_wait(&mut self) {
        if let Some(wait)=self.event_wait.take() {self.warp=wait.warp;}
    }
    pub(super) fn event_wait_watch(&self)->Option<FactionId> {
        self.event_wait.as_ref().and_then(|w|match w.role {Role::Faction(f)=>Some(f),Role::Spectator=>None})
    }
    pub(super) fn event_wait_until(&self)->f64 {self.event_wait.as_ref().map_or(f64::INFINITY,|w|w.until)}
    pub(super) fn finish_event_wait(&mut self,alert:bool)->bool {
        let Some(wait)=self.event_wait.as_ref() else {return false;};
        let v=self.view(wait.role);
        let reason=if v.outcome.is_some() {Some("Mission resolved")}
            else if alert {Some("Tactical alert received")}
            else if Picture::from(&v)!=wait.picture {Some("Tactical event received: contact, weapon, repair or jump status changed")}
            else if self.world.time()>=wait.until {Some("No tactical event within 24 hours")}
            else {None};
        if let Some(reason)=reason {
            self.last_alert=Some((self.world.time(),reason.into()));
            self.cancel_event_wait();self.paused=true;true
        } else {false}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{BodySpec,BodyKind,FactionId,ShipClass};
    use crate::kinematics::State;
    fn fixture()->LocalSession {
        let mut w=World::new(crate::celestial::System {bodies:vec![]},vec![BodySpec {name:"Ship".into(),kind:BodyKind::Ship,faction:FactionId(0),
            state:State {pos:Vec2::ZERO,vel:Vec2::ZERO},thrust:Vec2::ZERO,magazine:0}],0.0,42);
        w.bodies[0].controllable=true;w.bodies[0].ship_class=Some(ShipClass::Destroyer);
        LocalSession::new(w)
    }
    #[test]
    fn next_event_stops_on_jump_transition_and_restores_manual_warp() {
        let mut s=fixture();let role=Role::Faction(FactionId(0));
        s.command(role,Command::SetWarp(20.0)).unwrap();
        s.command(role,Command::Jump {body:BodyId(0),destination:Vec2::new(crate::units::AU,0.0)}).unwrap();
        s.advance_to_next_event(role);s.tick(1.0);
        assert!(s.paused);assert!(!s.waiting_for_event());assert_eq!(s.warp,20.0);
        assert!(s.world.time()>=600.0 && s.world.time()<=605.0,"stop on departure rather than skip whole jump");
    }
    #[test]
    fn next_event_stops_when_repair_completes_not_on_every_progress_tick() {
        let mut s=fixture();s.world.bodies[0].damage.systems[System::Propulsion as usize]=Condition::Damaged;
        s.advance_to_next_event(Role::Faction(FactionId(0)));s.tick(1.0);
        assert!(s.paused);assert!(s.world.time()>=1190.0 && s.world.time()<=1220.0,"t={}",s.world.time());
        assert_eq!(s.world.bodies[0].damage.state(System::Propulsion),Condition::Intact);
    }
    #[test]
    fn no_event_wait_is_bounded_and_can_be_cancelled() {
        let mut s=fixture();let role=Role::Faction(FactionId(0));
        s.advance_to_next_event(role);s.command(role,Command::SetPaused(true)).unwrap();assert!(!s.waiting_for_event());
        s.advance_to_next_event(role);s.tick(100.0);assert!(s.paused);assert_eq!(s.world.time(),86400.0);
    }
    #[test]
    fn withdrawal_notice_and_cancellation_wait_for_light_before_stopping() {
        let specs=(0..2).map(|i|BodySpec {name:format!("Ship {i}"),kind:BodyKind::Ship,faction:FactionId(i),
            state:State {pos:Vec2::new(i as f64*30.0*crate::units::LIGHT_SECOND,0.0),vel:Vec2::ZERO},thrust:Vec2::ZERO,magazine:0}).collect();
        let mut w=World::new(crate::celestial::System {bodies:vec![]},specs,0.0,42);
        for b in &mut w.bodies {b.controllable=true;b.ship_class=Some(ShipClass::Destroyer);}
        w.advance_to(100.0);w.withdraw(BodyId(1)).unwrap();
        let mut s=LocalSession::new(w);let role=Role::Faction(FactionId(0));
        s.advance_to_next_event(role);s.tick(0.001);
        assert!(s.view(role).withdrawals.is_empty());assert!(s.waiting_for_event());
        s.tick(1.0);assert!(s.paused);assert!(s.world.time()>=130.0 && s.world.time()<=135.0);
        assert_eq!(s.view(role).withdrawals.len(),1);
        let cancelled=s.world.time();s.world.cancel_jump(BodyId(1)).unwrap();
        s.advance_to_next_event(role);s.tick(0.001);assert_eq!(s.view(role).withdrawals.len(),1);
        s.tick(1.0);assert!(s.paused);assert!(s.world.time()>=cancelled+30.0);
        assert!(s.view(role).withdrawals.is_empty());
    }

}
