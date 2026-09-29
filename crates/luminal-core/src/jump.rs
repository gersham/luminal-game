//! Jump is the sole FTL exception. Transit has no normal-space trajectory.
use super::*;

pub const SPOOL_SECONDS:f64=600.0;
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
        self.ensure_not_jumping(id)?;
        // System origin is Sol; only a finite point within its 50 AU sphere is valid.
        if !destination.x.is_finite() || !destination.y.is_finite() || destination.length()>MAX_SOL_RADIUS_AU*AU {return Err(OrderError::InvalidTarget);}
        let t=self.time;
        let b=self.live_body_mut(id)?;
        if b.kind!=BodyKind::Ship || !b.ship_class.is_some_and(ShipClass::has_jump_drive) {return Err(OrderError::JumpUnavailable);}
        if b.operating_effectiveness(crate::damage::System::Jump)<=0.0 {return Err(OrderError::JumpUnavailable);}
        b.advance_thermal(t);
        b.jump=Some(JumpState::Spooling {destination,depart_at:t+SPOOL_SECONDS});
        b.commanded=Vec2::ZERO;b.autopilot=None;b.route=None;
        b.controls.evading=false;b.controls.evasion=None;b.screen_up=false;b.thermal.field=0.0;
        b.thermal.dumping=false;
        b.spinal_tracking=None;
        b.trajectory.set_thrust(t,Vec2::ZERO).unwrap();
        b.avoidance=Avoidance {thrust:Vec2::ZERO,active:false,impossible:false};
        self.scheduler.schedule(t+SPOOL_SECONDS,Event::JumpDepart(id,t+SPOOL_SECONDS));
        self.snapshot_platform(id,t);
        Ok(())
    }
    pub fn cancel_jump(&mut self,id:BodyId)->Result<(),OrderError> {
        let t=self.time;
        let b=self.bodies.get_mut(id.0 as usize).ok_or(OrderError::InvalidTarget)?;
        if !b.alive_at(t) {return Err(if b.jump.is_some() {OrderError::JumpBusy} else {OrderError::Destroyed});}
        if !matches!(b.jump,Some(JumpState::Spooling {..})) {return Err(OrderError::InvalidTarget);}
        b.advance_thermal(t);b.jump=None;b.thermal.field=0.0;
        self.update_system_controls(id);
        self.snapshot_platform(id,t);
        Ok(())
    }
    pub(super) fn jump_depart(&mut self,id:BodyId,at:f64) {
        if !matches!(self.bodies[id.0 as usize].jump,Some(JumpState::Spooling {depart_at,..}) if depart_at==at) {return;}
        if self.bodies[id.0 as usize].operating_effectiveness(crate::damage::System::Jump)<=0.0 {let _=self.cancel_jump(id);return;}
        // Resolve the final normal-space coast before allowing departure.
        self.step(id);
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
        b.thermal.add_waste_heat(SHIP_HEAT_LIMIT_J*b.thermal.capacity_scale*ARRIVAL_HEAT_FRACTION);
        b.thermal.field=0.0;
        // Never sweep a collision segment across the FTL path.
        self.last_step[id.0 as usize]=at;
        if let Some((celestial,_))=self.system.impact(destination,destination,at,at) {
            self.destroy(id,at,LossCause::Impact(celestial));return;
        }
        self.step(id);
        self.update_system_controls(id);
        self.snapshot_platform(id,at);

    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::celestial::{Celestial,CelestialKind,Orbit};
    use crate::damage::System as ShipSystem;
    use crate::session::{Command,LocalSession,Rejection,Role};

    fn fixture(class:ShipClass)->World {
        let system=System {bodies:vec![Celestial {name:"Sol".into(),kind:CelestialKind::Star,gm:0.0,radius:100.0,orbit:Orbit::Fixed(Vec2::ZERO)}]};
        let mut world=World::new(system,vec![BodySpec {name:"Jumper".into(),kind:BodyKind::Ship,faction:FactionId(0),state:State {pos:Vec2::new(AU,0.0),vel:Vec2::new(2.0,3.0)},thrust:Vec2::ZERO,magazine:4}],0.0,42);
        let b=&mut world.bodies[0];b.ship_class=Some(class);b.controllable=true;b.thermal.capacity_scale=class.scale();
        world
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
            w.advance_to(610.0);
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
        w.advance_to(610.0);assert!(w.bodies[0].jump.is_none());assert!(w.bodies[0].trajectory.jump_gaps().is_empty());
    }
    #[test]
    fn exact_spool_transit_velocity_heat_and_history_survive_large_warp() {
        for class in [ShipClass::Destroyer,ShipClass::Cruiser,ShipClass::Battleship] {
            let mut w=fixture(class);let id=BodyId(0);let destination=Vec2::new(-4.0*AU,0.0);
            let velocity=w.state(id,0.0).unwrap().vel;
            w.start_jump(id,destination).unwrap();w.advance_to(599.999);
            assert!(matches!(w.bodies[0].jump,Some(JumpState::Spooling {..})));
            w.advance_to(600.0);
            let Some(JumpState::Transit {origin,arrive_at,..})=w.bodies[0].jump else {panic!("expected transit")};
            assert!((arrive_at-600.0-(destination-origin).length()/AU).abs()<1e-9);
            assert!(w.state(id,600.0).is_none());assert_eq!(w.cancel_jump(id),Err(OrderError::JumpBusy));
            w.advance_to(arrive_at);
            let state=w.state(id,arrive_at).unwrap();assert_eq!(state.pos,destination);assert_eq!(state.vel,velocity);
            assert!(w.bodies[0].thermal.heat_fraction()>=ARRIVAL_HEAT_FRACTION);
            assert!(w.losses.is_empty(),"jump path must not collide with Sol");
            assert!(w.state(id,599.0).is_some());assert!(w.state(id,601.0).is_none());
            w.advance_to(arrive_at+1000.0);
            assert!(w.losses.is_empty());assert_eq!(w.state(id,w.time).unwrap().vel,velocity);
        }
    }
    #[test]
    fn cancelled_spool_timer_cannot_fire_a_later_jump_early() {
        let mut w=fixture(ShipClass::Destroyer);let id=BodyId(0);
        w.start_jump(id,Vec2::new(3.0*AU,0.0)).unwrap();w.advance_to(100.0);w.cancel_jump(id).unwrap();
        w.start_jump(id,Vec2::new(-3.0*AU,0.0)).unwrap();w.advance_to(600.0);
        assert!(matches!(w.bodies[0].jump,Some(JumpState::Spooling {depart_at:700.0,..})));
        assert!(w.bodies[0].trajectory.jump_gaps().is_empty());
        w.advance_to(700.0);assert!(matches!(w.bodies[0].jump,Some(JumpState::Transit {..})));
    }

    #[test]
    fn session_retains_command_ship_and_transit_readout() {
        let mut session=LocalSession::new(fixture(ShipClass::Destroyer));
        let role=Role::Faction(FactionId(0));let body=BodyId(0);
        session.command(role,Command::Jump {body,destination:Vec2::new(40.0*AU,0.0)}).unwrap();
        session.command(role,Command::SetPaused(false)).unwrap();session.tick(601.0);
        let view=session.view(role);
        assert!(matches!(view.bodies[0].jump,Some(JumpState::Transit {..})));
        assert_eq!(view.bodies[0].vel,Vec2::new(2.0,3.0));
        assert_eq!(session.command(role,Command::CancelJump {body}),Err(Rejection::JumpBusy));
        assert_eq!(session.command(role,Command::SetThrust {body,thrust:Vec2::ZERO}),Err(Rejection::JumpBusy));
        session.tick(39.0);
        assert!(session.view(role).bodies[0].jump.is_none());
    }
}
