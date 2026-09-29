//! Combat exits are explicit outcomes, not destruction. Intent and removal reports
//! travel through the same causal command/telemetry paths as other ship actions.
use super::*;
use crate::damage::{Condition,Damage,RepairGoal,System as Subsystem};

pub fn recoverable(d:&Damage,system:Subsystem)->bool {
    let repair=d.state(Subsystem::Repair)!=Condition::Destroyed
        && d.state(Subsystem::Crew)!=Condition::Destroyed && d.state(Subsystem::Mind)!=Condition::Destroyed
        && d.state(Subsystem::Power)!=Condition::Destroyed;
    if d.state(Subsystem::Mind)==Condition::Destroyed {return false;}
    let power=d.state(Subsystem::Power)==Condition::Intact || repair;
    power && (d.state(system)==Condition::Intact || (d.state(system)==Condition::Damaged && repair))
}
pub fn can_fight_again(class:ShipClass,d:&Damage,magazine:[u32;2])->bool {
    (class!=ShipClass::Picket && recoverable(d,Subsystem::Beam))
        || (magazine[0]>0 && recoverable(d,Subsystem::SrmLauncher))
        || (magazine[1]>0 && recoverable(d,Subsystem::Launcher))
}
impl World {
    pub(super) fn announce_ship_event(&mut self,id:BodyId,kind:CombatKind) {
        if let Some(state)=self.state(id,self.time) {self.record_combat(self.time,state.pos,kind,Some(id),Some(self.bodies[id.0 as usize].faction));}
    }
    pub fn withdraw(&mut self,id:BodyId)->Result<(),OrderError> {
        self.start_jump(id,Vec2::ZERO)?;
        let b=&mut self.bodies[id.0 as usize];
        b.withdrawing=true;b.damage.repair_goal=RepairGoal::Escape;
        b.missile_queued=[0,0];b.beam_auto=false;b.beam_target=None;
        self.announce_ship_event(id,CombatKind::WithdrawalStarted);
        self.snapshot_platform(id,self.time);
        Ok(())
    }
    pub fn surrender(&mut self,id:BodyId)->Result<(),OrderError> {
        let b=self.live_body_mut(id)?;
        if b.kind!=BodyKind::Ship {return Err(OrderError::InvalidTarget);}
        b.missile_queued=[0,0];b.beam_auto=false;b.beam_target=None;
        self.destroy(id,self.time,LossCause::Surrendered);
        Ok(())
    }
    pub fn set_repair_goal(&mut self,id:BodyId,goal:RepairGoal)->Result<(),OrderError> {
        let b=self.live_body_mut(id)?;
        if b.kind!=BodyKind::Ship {return Err(OrderError::InvalidTarget);}
        b.damage.repair_goal=goal;
        // Preserve accumulated work on the current repair; apply the goal next.
        self.snapshot_platform(id,self.time);
        Ok(())
    }
    /// A remote concession must arrive before it reveals the mission result.
    pub fn received_outcome(&self,faction:FactionId)->Option<Outcome> {
        let o=self.outcome.as_ref()?;
        if let Some(id)=self.outcome_exit {
            if self.bodies[id.0 as usize].faction==faction {return Some(o.clone());}
            let contact=self.association.get(&(faction,id));
            if !self.refinement_received_exit(faction,contact.copied(),o.t) {return None;}
        }
        Some(o.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::LIGHT_SECOND;
    use crate::session::{Command,LocalSession,Role};
    fn fixture()->World {
        let specs=(0..2).map(|i|BodySpec {name:format!("Ship {i}"),kind:BodyKind::Ship,faction:FactionId(i),
            state:State {pos:Vec2::new(i as f64*30.0*LIGHT_SECOND,0.0),vel:Vec2::ZERO},thrust:Vec2::ZERO,magazine:0}).collect();
        let mut w=World::new(System {bodies:vec![]},specs,0.0,42);
        for b in &mut w.bodies {b.ship_class=Some(ShipClass::Destroyer);b.controllable=true;}
        w.objective=Some(Objective {sensor_site:None,name:"Defeat raider".into(),center:Vec2::ZERO,radius:1.0,
            protect:BodyId(0),player:Some(BodyId(0)),defeat:Some(BodyId(1)),defender:FactionId(0),attacker:FactionId(1)});
        w
    }
    #[test]
    fn combat_incapable_bot_finishes_a_stalled_fight_by_withdrawing() {
        let mut w=fixture();
        w.bodies[1].armed=true;
        w.bodies[1].damage.systems[Subsystem::Beam as usize]=Condition::Destroyed;
        let mut session=LocalSession::new(w);session.enable_bot(FactionId(1),true);
        session.command(Role::Spectator,Command::SetPaused(false)).unwrap();
        session.tick(700.0);
        let view=session.view(Role::Spectator);
        assert!(view.outcome.as_ref().is_some_and(|o|o.reason.contains("withdrew")),"{:?}",view.outcome);
        assert!(!view.bodies.iter().any(|b|b.id==BodyId(1)));
    }
    #[test]
    fn jump_visual_reports_are_causal_and_have_distinct_endpoints() {
        let mut w=fixture();let id=BodyId(1);let observer=FactionId(0);
        w.start_jump(id,Vec2::new(60.0*LIGHT_SECOND,0.0)).unwrap();
        assert!(w.jump_events(Some(observer)).is_empty());
        assert!(w.jump_events(Some(FactionId(1))).iter().any(|e|e.kind==CombatKind::JumpSpool));
        w.advance_to(35.0);
        assert!(w.jump_events(Some(observer)).iter().any(|e|e.kind==CombatKind::JumpSpool && e.received_at>=30.0));
        w.advance_to(610.0);
        assert!(!w.jump_events(Some(observer)).iter().any(|e|matches!(e.kind,CombatKind::JumpDeparture|CombatKind::JumpArrival)));
        let own=w.jump_events(Some(FactionId(1)));
        let departure=own.iter().find(|e|e.kind==CombatKind::JumpDeparture).unwrap();
        let arrival=own.iter().find(|e|e.kind==CombatKind::JumpArrival).unwrap();
        assert_eq!(departure.pos,Some(Vec2::new(30.0*LIGHT_SECOND,0.0)));
        assert_eq!(arrival.pos,Some(Vec2::new(60.0*LIGHT_SECOND,0.0)));
        w.advance_to(670.0);
        let observed=w.jump_events(Some(observer));
        assert!(observed.iter().any(|e|e.kind==CombatKind::JumpDeparture && e.received_at>=630.0));
        assert!(observed.iter().any(|e|e.kind==CombatKind::JumpArrival && e.received_at>=660.0));
    }
    #[test]
    fn withdrawal_is_vulnerable_for_full_spool_then_removes_without_destruction() {
        let mut w=fixture();w.withdraw(BodyId(1)).unwrap();
        w.advance_to(599.0);assert!(w.bodies[1].alive_at(w.time()));assert!(w.outcome.is_none());
        w.advance_to(600.0);assert!(!w.bodies[1].alive_at(w.time()));
        assert_eq!(w.losses.last().unwrap().cause,LossCause::Withdrawn);
        assert!(w.jump_events(Some(FactionId(1))).iter().any(|e|e.kind==CombatKind::JumpDeparture));
        assert!(!w.jump_events(None).iter().any(|e|e.kind==CombatKind::JumpArrival));
        assert!(w.bodies[1].damage.hull>0.0);assert!(w.bodies[1].jump.is_none());
        assert!(w.combat_events(None).iter().all(|e|e.kind!=CombatKind::Destroyed));
        assert!(w.received_outcome(FactionId(0)).is_none(),"remote concession travels at c");
        w.advance_to(640.0);
        assert_eq!(w.received_outcome(FactionId(0)).unwrap().winner,FactionId(0));
        assert!(w.received_outcome(FactionId(0)).unwrap().reason.contains("withdrew"));
        let c=w.contact_id(FactionId(0),BodyId(1));assert!(w.contact_retired(FactionId(0),c));
        assert_eq!(w.start_jump(BodyId(1),Vec2::ZERO),Err(OrderError::Destroyed));
        w.advance_to(2000.0);assert_eq!(w.losses.len(),1);
    }
    #[test]
    fn cancel_or_damage_aborts_withdrawal_and_stale_timer_cannot_remove_ship() {
        for cancel in [false,true] {
            let mut w=fixture();w.withdraw(BodyId(1)).unwrap();w.advance_to(100.0);
            if cancel {w.cancel_jump(BodyId(1)).unwrap();}
            else {w.bodies[1].damage.systems[Subsystem::Jump as usize]=Condition::Damaged;w.update_system_controls(BodyId(1));}
            assert!(!w.bodies[1].withdrawing);w.advance_to(610.0);
            assert!(w.bodies[1].alive_at(w.time()));assert!(w.outcome.is_none());
        }
    }
    #[test]
    fn destruction_during_spool_is_not_reported_as_successful_escape() {
        let mut w=fixture();w.withdraw(BodyId(1)).unwrap();w.advance_to(100.0);
        w.destroy(BodyId(1),100.0,LossCause::ShipBeam {shooter:BodyId(0)});w.advance_to(700.0);
        assert_eq!(w.losses.len(),1);assert!(matches!(w.losses[0].cause,LossCause::ShipBeam {..}));
    }
    #[test]
    fn surrender_concedes_objective_and_only_owner_can_order_it() {
        let mut s=LocalSession::new(fixture());
        assert!(s.command(Role::Faction(FactionId(0)),Command::Surrender {body:BodyId(1)}).is_err());
        s.command(Role::Faction(FactionId(1)),Command::Surrender {body:BodyId(1)}).unwrap();
        assert!(s.view(Role::Faction(FactionId(0))).outcome.is_none());
        s.command(Role::Faction(FactionId(0)),Command::SetPaused(false)).unwrap();s.tick(40.0);
        assert!(s.view(Role::Faction(FactionId(0))).outcome.unwrap().reason.contains("surrendered"));
        let mut w=fixture();w.surrender(BodyId(0)).unwrap();
        assert_eq!(w.outcome.unwrap().winner,FactionId(1),"escort departure concedes rather than winning");
    }
    #[test]
    fn destroyed_weapons_and_failed_repair_are_not_future_combat_capability() {
        let mut d=Damage::default();d.systems[Subsystem::Beam as usize]=Condition::Destroyed;
        assert!(!can_fight_again(ShipClass::Destroyer,&d,[0,0]));
        assert!(can_fight_again(ShipClass::Destroyer,&d,[1,0]));
        d.systems[Subsystem::Power as usize]=Condition::Damaged;
        assert!(can_fight_again(ShipClass::Destroyer,&d,[1,0]));
        d.systems[Subsystem::Repair as usize]=Condition::Destroyed;
        assert!(!can_fight_again(ShipClass::Destroyer,&d,[1,0]));
    }
}
