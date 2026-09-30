//! Jump is the sole FTL exception. Transit has no normal-space trajectory.
use super::*;
use crate::session::Command;

pub const SPOOL_SECONDS:f64=3600.0;
pub const RECOVERY_SECONDS:f64=6.0*3600.0;
pub const MAX_SOL_RADIUS_AU:f64=50.0;
pub const SPEED_AU_PER_SECOND:f64=1.0;
/// Sustained spool input is four times class-rated full-thrust heat.
pub const SPOOL_FULL_DRIVE_MULTIPLIER:f64=4.0;
/// Arrival adds one full class-scaled thermal limit, in addition to spool heat.
pub const ARRIVAL_HEAT_FRACTION:f64=1.0;
pub fn spool_power(scale:f64)->f64 {crate::thermal::Thermal::drive_power(1.0)*SPOOL_FULL_DRIVE_MULTIPLIER*scale}

#[derive(Clone,Copy,Debug,PartialEq)]
pub enum JumpState {
    Spooling {destination:Vec2,depart_at:f64},
    Transit {origin:Vec2,destination:Vec2,velocity:Vec2,depart_at:f64,arrive_at:f64},
}
impl JumpState {
    /// A UI-only transit marker, never used for physical sensing or weapon hits.
    pub fn display_state(self,t:f64)->Option<State> {
        match self {
            Self::Transit {origin,destination,velocity,depart_at,arrive_at}=>Some(State {
                pos:origin+(destination-origin)*((t-depart_at)/(arrive_at-depart_at).max(f64::MIN_POSITIVE)).clamp(0.0,1.0),vel:velocity}),
            _=>None,
        }
    }
}
impl World {
    pub(super) fn ensure_not_jumping(&self,id:BodyId)->Result<(),OrderError> {
        if self.bodies.get(id.0 as usize).is_some_and(|b|b.jump.is_some()) {Err(OrderError::JumpBusy)} else {Ok(())}
    }
    pub fn start_jump(&mut self,id:BodyId,destination:Vec2)->Result<(),OrderError> {
        if !self.jumps_enabled {return Err(OrderError::JumpUnavailable);}
        self.ensure_not_jumping(id)?;
        // System origin is Sol; only a finite point within its 50 AU sphere is valid.
        if !destination.x.is_finite() || !destination.y.is_finite() || destination.length()>MAX_SOL_RADIUS_AU*AU {return Err(OrderError::InvalidTarget);}
        let t=self.time;
        let b=self.live_body_mut(id)?;
        if b.kind!=BodyKind::Ship || !b.ship_class.is_some_and(ShipClass::has_jump_drive) {return Err(OrderError::JumpUnavailable);}
        if b.operating_effectiveness(crate::damage::System::Jump)<=0.0 {return Err(OrderError::JumpUnavailable);}
        if t<b.jump_ready_at {return Err(OrderError::JumpRecovering);}
        b.advance_thermal(t);
        b.jump=Some(JumpState::Spooling {destination,depart_at:t+SPOOL_SECONDS});
        b.commanded=Vec2::ZERO;b.autopilot=None;b.route=None;
        b.controls.evading=false;b.controls.evasion=None;b.screen_up=false;b.thermal.field=0.0;
        b.thermal.dumping=false;
        b.spinal_tracking=None;
        b.trajectory.set_thrust(t,Vec2::ZERO).unwrap();
        b.avoidance=Avoidance {thrust:Vec2::ZERO,active:false,impossible:false};
        self.announce_ship_event(id,CombatKind::JumpSpool);
        self.scheduler.schedule(t+SPOOL_SECONDS,Event::JumpDepart(id,t+SPOOL_SECONDS));
        self.snapshot_platform(id,t);
        Ok(())
    }
    pub fn cancel_jump(&mut self,id:BodyId)->Result<(),OrderError> {
        let t=self.time;
        let b=self.bodies.get_mut(id.0 as usize).ok_or(OrderError::InvalidTarget)?;
        if !b.alive_at(t) {return Err(if b.jump.is_some() {OrderError::JumpBusy} else {OrderError::Destroyed});}
        if !matches!(b.jump,Some(JumpState::Spooling {..})) {return Err(OrderError::InvalidTarget);}
        let withdrawal=b.withdrawing;
        b.advance_thermal(t);b.jump=None;b.withdrawing=false;b.thermal.field=0.0;
        self.announce_ship_event(id,CombatKind::JumpCancelled);
        if withdrawal {self.announce_ship_event(id,CombatKind::WithdrawalCancelled);}
        self.update_system_controls(id);
        self.snapshot_platform(id,t);
        Ok(())
    }
    pub(super) fn jump_depart(&mut self,id:BodyId,at:f64) {
        if !matches!(self.bodies[id.0 as usize].jump,Some(JumpState::Spooling {depart_at,..}) if depart_at==at) {return;}
        if self.bodies[id.0 as usize].operating_effectiveness(crate::damage::System::Jump)<=0.0 {let _=self.cancel_jump(id);return;}
        // Resolve the final normal-space coast before allowing departure.
        self.step(id);
        if !self.bodies[id.0 as usize].alive_at(at)
            || !matches!(self.bodies[id.0 as usize].jump,Some(JumpState::Spooling {depart_at,..}) if depart_at==at) {return;}
        self.announce_ship_event(id,CombatKind::JumpDeparture);
        if self.bodies[id.0 as usize].withdrawing && self.bodies[id.0 as usize].alive_at(at) {
            self.destroy(id,at,LossCause::Withdrawn);return;
        }
        let b=&mut self.bodies[id.0 as usize];
        let Some(JumpState::Spooling {destination,depart_at})=b.jump else {return};
        if depart_at!=at || !b.alive_at(at) {return;}
        b.advance_thermal(at);
        let state=b.trajectory.state_at(at).unwrap();
        let arrive_at=at+(destination-state.pos).length()/(SPEED_AU_PER_SECOND*AU);
        b.jump=Some(JumpState::Transit {origin:state.pos,destination,velocity:state.vel,depart_at,arrive_at});
        b.trajectory.jump_departure(at);
        b.step_generation+=1;
        self.scheduler.schedule(arrive_at,Event::JumpArrive(id,arrive_at));
    }
    pub(super) fn jump_arrive(&mut self,id:BodyId,at:f64) {
        let b=&mut self.bodies[id.0 as usize];
        let Some(JumpState::Transit {destination,velocity,arrive_at,..})=b.jump else {return};
        if arrive_at!=at || b.trajectory.end().is_some() {return;}
        b.advance_thermal(at);
        b.trajectory.jump_arrival(at,State {pos:destination,vel:velocity});
        b.jump=None;
        b.jump_ready_at=at+RECOVERY_SECONDS;
        b.thermal.add_waste_heat(SHIP_HEAT_LIMIT_J*b.thermal.capacity_scale*ARRIVAL_HEAT_FRACTION);
        b.thermal.field=0.0;
        self.announce_ship_event(id,CombatKind::JumpArrival);
        // Never sweep a collision segment across the FTL path.
        self.last_step[id.0 as usize]=at;
        if let Some((celestial,_))=self.system.impact(destination,destination,at,at) {
            self.destroy(id,at,LossCause::Impact(celestial));return;
        }
        self.step(id);
        self.update_system_controls(id);
        self.snapshot_platform(id,at);

    }

    /// Jump-capable ships of the player's faction spool for the same jump.
    /// A ship already holding station on the flagship keeps that offset. The others
    /// dress ahead along the jump, capitals nearest the admiral. The order travels
    /// at light speed. A ship already jumping, recovering, or without a working drive stays.
    pub fn order_fleet_jump(&mut self, flagship: BodyId, destination: Vec2) {
        if !self.jumps_enabled { return; }
        let t = self.time;
        let Some(flag) = self.body(flagship) else { return };
        let faction = flag.faction;
        let facing = self.state(flagship, t).map(|state| {
            let delta = destination - state.pos;
            if delta.length() > 1.0 { delta.normalized() }
            else if state.vel.length() > 1.0 { state.vel.normalized() }
            else { Vec2::new(1.0, 0.0) }
        }).unwrap_or(Vec2::new(1.0, 0.0));
        let mut stationed = Vec::new();
        let mut loose = Vec::new();
        for (i, b) in self.bodies.iter().enumerate() {
            let id = BodyId(i as u32);
            if id == flagship || b.faction != faction || b.kind != BodyKind::Ship || !b.alive_at(t) { continue; }
            if b.jump.is_some() || t < b.jump_ready_at { continue; }
            if !b.ship_class.is_some_and(ShipClass::has_jump_drive) { continue; }
            if b.operating_effectiveness(crate::damage::System::Jump) <= 0.0 { continue; }
            match b.autopilot.map(|ap| ap.order) {
                Some(Order::Follow { target, offset }) if target == flagship && offset.length() > 5.0 * crate::units::LIGHT_SECOND => {
                    stationed.push((id, offset));
                }
                _ => loose.push((id, b.ship_class.map(|c| c.scale()).unwrap_or(1.0))),
            }
        }
        loose.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.0.cmp(&b.0.0)));
        let scales: Vec<f64> = loose.iter().map(|(_, scale)| *scale).collect();
        let formed = formation_offsets(facing, &scales);
        for (id, offset) in stationed { self.transmit_fleet_jump(id, destination, offset); }
        for ((id, _), offset) in loose.into_iter().zip(formed) { self.transmit_fleet_jump(id, destination, offset); }
    }

    fn transmit_fleet_jump(&mut self, id: BodyId, destination: Vec2, offset: Vec2) {
        let shifted = destination + offset;
        let dest = if shifted.x.is_finite() && shifted.y.is_finite() && shifted.length() <= MAX_SOL_RADIUS_AU * AU {
            shifted
        } else { destination };
        self.transmit_order(id, Command::Jump { body: id, destination: dest });
    }

    /// Stand the fleet down from a jump the player no longer wants.
    /// Packets still in flight are dropped, and ships already spooling are told to cancel.
    /// A withdrawal already in progress is left alone.
    pub fn order_fleet_cancel_jump(&mut self, flagship: BodyId) {
        let t = self.time;
        let Some(flag) = self.body(flagship) else { return };
        let faction = flag.faction;
        self.drop_pending_jumps(faction, flagship);
        let spooling: Vec<BodyId> = self.bodies.iter().enumerate().filter_map(|(i, b)| {
            let id = BodyId(i as u32);
            (id != flagship && b.faction == faction && b.alive_at(t) && !b.withdrawing
                && matches!(b.jump, Some(JumpState::Spooling { .. }))).then_some(id)
        }).collect();
        for id in spooling { self.transmit_order(id, Command::CancelJump { body: id }); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::celestial::{Celestial,CelestialKind,Orbit};
    use crate::damage::System as ShipSystem;
    use crate::kinematics::State;
    use crate::session::{BodyView,Command,LocalSession,Rejection,Role};

    fn fixture(class:ShipClass)->World {
        let system=System {bodies:vec![Celestial {name:"Sol".into(),kind:CelestialKind::Star,gm:0.0,radius:100.0,orbit:Orbit::Fixed(Vec2::ZERO)}]};
        let mut world=World::new(system,vec![BodySpec {name:"Jumper".into(),kind:BodyKind::Ship,faction:FactionId(0),state:State {pos:Vec2::new(AU,0.0),vel:Vec2::new(2.0,3.0)},thrust:Vec2::ZERO,magazine:4}],0.0,42);
        let b=&mut world.bodies[0];b.ship_class=Some(class);b.controllable=true;b.thermal.capacity_scale=class.scale();
        world
    }
    #[test]
    fn recovery_blocks_spooling_until_expiry_and_cancel_does_not_spend_it() {
        let mut w=fixture(ShipClass::Destroyer);let id=BodyId(0);let destination=Vec2::new(2.0*AU,0.0);
        w.bodies[0].jump_ready_at=10.0;
        assert_eq!(w.start_jump(id,destination),Err(OrderError::JumpRecovering));
        assert!(w.bodies[0].jump.is_none());
        w.advance_to(10.0);w.start_jump(id,destination).unwrap();
        w.cancel_jump(id).unwrap();w.start_jump(id,destination).unwrap();
        assert_eq!(w.bodies[0].jump_ready_at,10.0);
    }
    #[test]
    fn eligibility_and_radius_are_enforced_in_core() {
        for class in [ShipClass::Picket,ShipClass::Frigate,ShipClass::Transport] {
            assert_eq!(fixture(class).start_jump(BodyId(0),Vec2::ZERO),Err(OrderError::JumpUnavailable));
        }
        for class in [ShipClass::Destroyer,ShipClass::Cruiser,ShipClass::Battleship] {
            let mut w=fixture(class);
            for p in [Vec2::new(50.01*AU,0.0),Vec2::new(f64::NAN,0.0),Vec2::new(0.0,f64::INFINITY)] {
                assert_eq!(w.start_jump(BodyId(0),p),Err(OrderError::InvalidTarget));
            }
            assert!(w.start_jump(BodyId(0),Vec2::new(-50.0*AU,0.0)).is_ok());
        }
    }
    #[test]
    fn jump_damage_blocks_launch_and_aborts_spooling_until_repaired() {
        use crate::damage::Condition;
        for system in [ShipSystem::Jump,ShipSystem::Power] {
            let mut w=fixture(ShipClass::Destroyer);let id=BodyId(0);
            assert!(w.bodies[0].installed_systems()[ShipSystem::Jump as usize]);
            for condition in [Condition::Damaged,Condition::Destroyed] {
                w.bodies[0].damage.systems[system as usize]=condition;
                assert_eq!(w.start_jump(id,Vec2::new(3.0*AU,0.0)),Err(OrderError::JumpUnavailable));
            }
            w.bodies[0].damage.systems[system as usize]=Condition::Intact;
            w.start_jump(id,Vec2::new(3.0*AU,0.0)).unwrap();
            w.advance_to(30.0);
            w.bodies[0].damage.systems[system as usize]=Condition::Damaged;
            w.update_system_controls(id);
            assert!(w.bodies[0].jump.is_none());
            assert_eq!(w.bodies[0].thermal.field,0.0);
            w.advance_to(SPOOL_SECONDS+10.0);
            assert!(w.bodies[0].trajectory.jump_gaps().is_empty(),"stale departure must not execute");
            w.bodies[0].damage.systems[system as usize]=Condition::Intact;
            assert!(w.start_jump(id,Vec2::new(3.0*AU,0.0)).is_ok());
        }
        assert!(!fixture(ShipClass::Frigate).bodies[0].installed_systems()[ShipSystem::Jump as usize]);
    }

    #[test]
    fn spool_locks_systems_and_cancel_recharges_from_zero() {
        let mut w=fixture(ShipClass::Destroyer);let id=BodyId(0);
        w.set_screen(id,true).unwrap();w.bodies[0].thermal.field=1.0;
        w.set_thrust(id,Vec2::new(0.01,0.0)).unwrap();
        w.start_jump(id,Vec2::new(3.0*AU,0.0)).unwrap();
        assert_eq!(w.set_thrust(id,Vec2::ZERO),Err(OrderError::JumpBusy));
        assert_eq!(w.set_all_stop(id),Err(OrderError::JumpBusy));
        assert_eq!(w.append_waypoint(id,Vec2::ZERO),Err(OrderError::JumpBusy));
        assert_eq!(w.set_screen(id,true),Err(OrderError::JumpBusy));
        assert_eq!(w.fire_beam(id,ContactId(99)),Err(OrderError::JumpBusy));
        assert_eq!(w.fire_spinal(id,ContactId(99)),Err(OrderError::JumpBusy));
        w.advance_to(30.0);
        let b=&w.bodies[0];
        for system in [ShipSystem::Propulsion,ShipSystem::Screens,ShipSystem::Beam,ShipSystem::PdLaser] {assert_eq!(b.operating_effectiveness(system),0.0);}
        assert_eq!(b.trajectory.thrust_at(30.0),Some(Vec2::ZERO));assert!(!b.controls.evading);
        assert_eq!(b.thermal.field,0.0);assert!(b.thermal.heat_j>0.0);
        w.cancel_jump(id).unwrap();
        assert_eq!(w.bodies[0].thermal.field,0.0);assert!(w.bodies[0].screen_up);
        w.advance_to(31.0);w.bodies[0].advance_thermal(31.0);
        assert!(w.bodies[0].thermal.field>0.0 && w.bodies[0].thermal.field<1.0);
        w.advance_to(SPOOL_SECONDS+10.0);assert!(w.bodies[0].jump.is_none());assert!(w.bodies[0].trajectory.jump_gaps().is_empty());
    }
    #[test]
    fn exact_spool_transit_velocity_heat_and_history_survive_large_warp() {
        for class in [ShipClass::Destroyer,ShipClass::Cruiser,ShipClass::Battleship] {
            let mut w=fixture(class);let id=BodyId(0);let destination=Vec2::new(-4.0*AU,0.0);
            let velocity=w.state(id,0.0).unwrap().vel;
            w.start_jump(id,destination).unwrap();w.advance_to(SPOOL_SECONDS-0.001);
            assert!(matches!(w.bodies[0].jump,Some(JumpState::Spooling {..})));
            w.advance_to(SPOOL_SECONDS);
            let Some(JumpState::Transit {origin,arrive_at,..})=w.bodies[0].jump else {panic!("expected transit")};
            assert!((arrive_at-SPOOL_SECONDS-(destination-origin).length()/AU).abs()<1e-9);
            assert!(w.state(id,SPOOL_SECONDS).is_none());assert_eq!(w.cancel_jump(id),Err(OrderError::JumpBusy));
            w.advance_to(arrive_at);
            let state=w.state(id,arrive_at).unwrap();assert_eq!(state.pos,destination);assert_eq!(state.vel,velocity);
            assert!(w.bodies[0].thermal.heat_fraction()>=ARRIVAL_HEAT_FRACTION);
            assert!(w.losses.is_empty(),"jump path must not collide with Sol");
            assert_eq!(w.bodies[0].jump_ready_at,arrive_at+RECOVERY_SECONDS);
            assert_eq!(w.start_jump(id,destination),Err(OrderError::JumpRecovering));
            assert!(w.state(id,599.0).is_some());assert!(w.state(id,SPOOL_SECONDS+1.0).is_none());
            w.advance_to(arrive_at+1000.0);
            assert!(w.losses.is_empty());assert_eq!(w.state(id,w.time).unwrap().vel,velocity);
        }
    }
    #[test]
    fn cancelled_spool_timer_cannot_fire_a_later_jump_early() {
        let mut w=fixture(ShipClass::Destroyer);let id=BodyId(0);
        w.start_jump(id,Vec2::new(3.0*AU,0.0)).unwrap();w.advance_to(100.0);w.cancel_jump(id).unwrap();
        w.start_jump(id,Vec2::new(-3.0*AU,0.0)).unwrap();w.advance_to(SPOOL_SECONDS);
        assert!(matches!(w.bodies[0].jump,Some(JumpState::Spooling {depart_at,..}) if depart_at==SPOOL_SECONDS+100.0));
        assert!(w.bodies[0].trajectory.jump_gaps().is_empty());
        w.advance_to(SPOOL_SECONDS+100.0);assert!(matches!(w.bodies[0].jump,Some(JumpState::Transit {..})));
    }

    #[test]
    fn session_retains_command_ship_and_transit_readout() {
        let mut session=LocalSession::new(fixture(ShipClass::Destroyer));
        let role=Role::Faction(FactionId(0));let body=BodyId(0);
        session.command(role,Command::Jump {body,destination:Vec2::new(40.0*AU,0.0)}).unwrap();
        session.command(role,Command::SetPaused(false)).unwrap();session.tick(SPOOL_SECONDS+1.0);
        let view=session.view(role);
        assert!(matches!(view.bodies[0].jump,Some(JumpState::Transit {..})));
        assert_eq!(view.bodies[0].vel,Vec2::new(2.0,3.0));
        assert_eq!(session.command(role,Command::CancelJump {body}),Err(Rejection::JumpBusy));
        assert_eq!(session.command(role,Command::SetThrust {body,thrust:Vec2::ZERO}),Err(Rejection::JumpBusy));
        session.tick(39.0);
        assert!(session.view(role).bodies[0].jump.is_none());
    }

    fn ship_at(name:&str,faction:u8,pos:Vec2)->BodySpec {
        BodySpec {name:name.into(),kind:BodyKind::Ship,faction:FactionId(faction),state:State {pos,vel:Vec2::ZERO},thrust:Vec2::ZERO,magazine:0}
    }
    fn seen(session:&LocalSession,id:u32)->BodyView {
        session.view(Role::Spectator).bodies.into_iter().find(|b| b.id==BodyId(id)).unwrap()
    }
    fn spool_of(body:&BodyView)->Option<Vec2> {
        match body.jump { Some(JumpState::Spooling {destination,..})=>Some(destination), _=>None }
    }
    /// Admiral, a stationed cruiser, a frigate, a loose cruiser, a loose destroyer,
    /// a recovering destroyer, a damaged destroyer, a destroyer already spooling,
    /// a withdrawing destroyer, and two raiders.
    fn fleet()->World {
        let specs=vec![
            ship_at("Admiral",0,Vec2::ZERO),
            ship_at("Screen",0,Vec2::new(0.02*AU,0.0)),
            ship_at("Frigate",0,Vec2::new(0.0,0.03*AU)),
            ship_at("Loose cruiser",0,Vec2::new(0.01*AU,0.0)),
            ship_at("Loose destroyer",0,Vec2::new(0.0,0.04*AU)),
            ship_at("Recovering",0,Vec2::new(-0.02*AU,0.0)),
            ship_at("Damaged",0,Vec2::new(-0.03*AU,0.0)),
            ship_at("Busy",0,Vec2::new(0.05*AU,0.0)),
            ship_at("Leaving",0,Vec2::new(0.06*AU,0.0)),
            ship_at("Raider",1,Vec2::new(8.0*AU,0.0)),
            ship_at("Raider cruiser",1,Vec2::new(8.02*AU,0.0)),
        ];
        let mut w=World::new(crate::celestial::System {bodies:vec![]},specs,0.0,7);
        let classes=[ShipClass::Battleship,ShipClass::Cruiser,ShipClass::Frigate,ShipClass::Cruiser,ShipClass::Destroyer,
            ShipClass::Destroyer,ShipClass::Destroyer,ShipClass::Destroyer,ShipClass::Destroyer,ShipClass::Battleship,ShipClass::Cruiser];
        for (b,class) in w.bodies.iter_mut().zip(classes) {
            b.ship_class=Some(class);b.controllable=true;b.thermal.capacity_scale=class.scale();
        }
        w.bodies[5].jump_ready_at=1.0e12;
        w.bodies[6].damage.systems[ShipSystem::Jump as usize]=crate::damage::Condition::Damaged;
        w.bodies[7].jump=Some(JumpState::Spooling {destination:Vec2::new(AU,0.0),depart_at:1.0e9});
        w.bodies[8].jump=Some(JumpState::Spooling {destination:Vec2::new(2.0*AU,0.0),depart_at:1.0e9});
        w.bodies[8].withdrawing=true;
        w.objective=Some(Objective {sensor_site:None,name:"jump".into(),center:Vec2::ZERO,radius:AU,protect:BodyId(0),player:Some(BodyId(0)),
            defeat:None,prize:None,wipe:false,defender:FactionId(0),attacker:FactionId(1),stance:Stance::Battle,
            withdrawal_continues:false,extract:false,escape_at_center:false,disengage_wins:false,prize_taken:false,escape_by:None});
        w
    }

    #[test]
    fn the_fleet_jumps_with_the_admiral_when_it_can() {
        let mut w=fleet();
        let dest=Vec2::new(3.0*AU,0.0);
        let station=Vec2::new(0.05*AU,0.01*AU);
        w.set_station(BodyId(1),BodyId(0),station).unwrap();
        w.start_jump(BodyId(0),dest).unwrap();
        assert_eq!(w.pending_orders(FactionId(0)),0,"a direct jump does not order the wing");
        w.cancel_jump(BodyId(0)).unwrap();
        let mut session=LocalSession::new(w);
        let role=Role::Faction(FactionId(0));
        session.command(role,Command::Jump {body:BodyId(0),destination:dest}).unwrap();
        assert_eq!(spool_of(&seen(&session,0)),Some(dest));
        assert_eq!(session.view(role).pending_orders,3);
        session.command(role,Command::SetPaused(false)).unwrap();
        session.tick(3.0);
        assert!(seen(&session,3).jump.is_none(),"loose cruiser is still waiting on the light");
        session.tick(8.0);
        assert_eq!(spool_of(&seen(&session,3)),Some(dest+Vec2::new(0.045*AU,0.0)));
        assert!(seen(&session,4).jump.is_none(),"destroyer light has not arrived");
        assert_eq!(spool_of(&seen(&session,7)),Some(Vec2::new(AU,0.0)),"a ship already spooling is not redirected");
        session.tick(22.0);
        assert_eq!(spool_of(&seen(&session,1)),Some(dest+station));
        assert_eq!(spool_of(&seen(&session,4)),Some(dest+Vec2::new(0.090*AU,0.0)));
        assert!(seen(&session,2).jump.is_none(),"frigate");
        assert!(seen(&session,5).jump.is_none(),"recovering");
        assert!(seen(&session,6).jump.is_none(),"damaged drive");
        assert_eq!(spool_of(&seen(&session,8)),Some(Vec2::new(2.0*AU,0.0)));
        session.command(role,Command::CancelJump {body:BodyId(0)}).unwrap();
        assert!(seen(&session,0).jump.is_none());
        assert!(seen(&session,1).jump.is_some(),"cancel is still in flight");
        session.tick(40.0);
        for id in [1,3,4,7] { assert!(seen(&session,id).jump.is_none(),"body {id} still spooling"); }
        let leaving=seen(&session,8);
        assert_eq!(spool_of(&leaving),Some(Vec2::new(2.0*AU,0.0)),"a withdrawal in progress keeps its jump");
        assert!(leaving.withdrawing);
    }

    #[test]
    fn cancelling_before_the_light_arrives_drops_the_fleet_jump() {
        let mut w=fleet();
        w.set_station(BodyId(1),BodyId(0),Vec2::new(0.05*AU,0.0)).unwrap();
        let mut session=LocalSession::new(w);
        let role=Role::Faction(FactionId(0));
        session.command(role,Command::Jump {body:BodyId(0),destination:Vec2::new(3.0*AU,0.0)}).unwrap();
        assert_eq!(session.view(role).pending_orders,3);
        session.command(role,Command::CancelJump {body:BodyId(0)}).unwrap();
        assert!(seen(&session,0).jump.is_none());
        assert_eq!(session.view(role).pending_orders,1,"in-flight jumps are dropped; the ship already spooling still needs a cancel");
        session.command(role,Command::SetPaused(false)).unwrap();
        session.tick(40.0);
        for id in [1,3,4,7] { assert!(seen(&session,id).jump.is_none(),"body {id}"); }
        assert_eq!(spool_of(&seen(&session,8)),Some(Vec2::new(2.0*AU,0.0)));
        assert!(seen(&session,8).withdrawing);
    }

    #[test]
    fn the_fleet_arrives_dressed_on_the_admiral() {
        let mut w=fleet();
        let dest=Vec2::new(3.0*AU,0.0);
        let station=Vec2::new(0.05*AU,0.01*AU);
        w.set_station(BodyId(1),BodyId(0),station).unwrap();
        let mut session=LocalSession::new(w);
        let role=Role::Faction(FactionId(0));
        session.command(role,Command::Jump {body:BodyId(0),destination:dest}).unwrap();
        session.command(role,Command::SetPaused(false)).unwrap();
        session.tick(SPOOL_SECONDS+30.0);
        let pos=|id| seen(&session,id).pos;
        assert!((pos(0)-dest).length()<1.0,"admiral");
        assert!((pos(1)-(dest+station)).length()<5_000.0,"screen");
        assert!((pos(3)-(dest+Vec2::new(0.045*AU,0.0))).length()<1.0,"loose cruiser");
        assert!((pos(4)-(dest+Vec2::new(0.090*AU,0.0))).length()<1.0,"loose destroyer");
        assert!((pos(2)-Vec2::new(0.0,0.03*AU)).length()<1_000.0,"frigate stayed");
        assert!(seen(&session,5).jump.is_none());
        assert!(seen(&session,6).jump.is_none());
        assert_eq!(spool_of(&seen(&session,8)),Some(Vec2::new(2.0*AU,0.0)));
    }

    #[test]
    fn the_fleet_stays_behind_when_the_admiral_withdraws() {
        let mut w=fleet();
        w.objective.as_mut().unwrap().disengage_wins=true;
        w.set_station(BodyId(1),BodyId(0),Vec2::new(0.05*AU,0.0)).unwrap();
        let mut session=LocalSession::new(w);
        let role=Role::Faction(FactionId(0));
        session.command(role,Command::Withdraw {body:BodyId(0)}).unwrap();
        assert_eq!(spool_of(&seen(&session,0)),Some(Vec2::ZERO));
        assert_eq!(session.view(role).pending_orders,0);
        session.command(role,Command::SetPaused(false)).unwrap();
        session.tick(30.0);
        assert!(seen(&session,0).withdrawing);
        assert!(seen(&session,1).jump.is_none());
        assert!(seen(&session,3).jump.is_none());
        assert!(seen(&session,4).jump.is_none());
    }

    #[test]
    fn another_flagship_jump_does_not_move_its_wing() {
        let w=fleet();
        let mut session=LocalSession::new(w);
        session.command(Role::Faction(FactionId(1)),Command::Jump {body:BodyId(9),destination:Vec2::new(4.0*AU,0.0)}).unwrap();
        session.command(Role::Faction(FactionId(0)),Command::SetPaused(false)).unwrap();
        session.tick(25.0);
        assert_eq!(spool_of(&seen(&session,9)),Some(Vec2::new(4.0*AU,0.0)));
        assert!(seen(&session,10).jump.is_none());
        assert!(seen(&session,1).jump.is_none());
    }

    #[test]
    fn the_fleet_jump_falls_back_inside_the_sphere() {
        let mut w=fleet();
        let outside=Vec2::new(0.2*AU,0.0);
        let inside=Vec2::new(0.0,0.05*AU);
        w.set_station(BodyId(1),BodyId(0),outside).unwrap();
        w.set_station(BodyId(3),BodyId(0),inside).unwrap();
        let mut session=LocalSession::new(w);
        let role=Role::Faction(FactionId(0));
        let dest=Vec2::new(49.9*AU,0.0);
        session.command(role,Command::Jump {body:BodyId(0),destination:dest}).unwrap();
        session.command(role,Command::SetPaused(false)).unwrap();
        session.tick(20.0);
        assert_eq!(spool_of(&seen(&session,1)),Some(dest));
        assert_eq!(spool_of(&seen(&session,3)),Some(dest+inside));
    }
}
