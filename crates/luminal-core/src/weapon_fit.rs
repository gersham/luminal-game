//! Class roles and bounded support weapon. Visual sub-pulses/fragments never
//! create extra simulation shots, hit rolls, defensive targets or damage.
use super::*;
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
pub enum BeamMode {#[default] Damage,Interference}
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
pub enum WeaponVisual {#[default] Standard,Pulse(u8),Canister(u8)}
pub const INTERFERENCE_RANGE_LS:f64=6.0;
pub const INTERFERENCE_SECONDS:f64=8.0;
pub const DISRUPTED_COUPLING:f64=0.85;
impl ShipClass {
    pub fn has_projector(self)->bool {self==Self::Frigate}
    pub fn beam_pulses(self)->u8 {match self {Self::Picket|Self::Transport=>0,Self::Frigate=>3,Self::Destroyer=>5,_=>1}}
    pub fn canister_fragments(self)->u8 {match self {Self::Picket=>3,Self::Frigate=>5,Self::Destroyer=>7,Self::Cruiser=>5,Self::Battleship=>11,Self::Transport=>0}}
    pub fn fleet_role(self)->&'static str {match self {
        Self::Picket=>"Fast scout / pursuit screen",Self::Frigate=>"Escort / electronic support",Self::Destroyer=>"Close-assault line combatant",
        Self::Cruiser=>"Long-range missile artillery",Self::Battleship=>"Heavy line / spinal finisher",Self::Transport=>"Civilian / convoy objective",
    }}
}
impl Body {
    pub fn beam_coherence(&self,t:f64)->f64 {if t<self.disrupted_until {DISRUPTED_COUPLING} else {1.0}}
}
impl World {
    pub fn set_beam_mode(&mut self,id:BodyId,mode:BeamMode)->Result<(),OrderError> {
        let b=self.live_body_mut(id)?;
        if b.kind!=BodyKind::Ship || !b.armed || !b.ship_class.is_some_and(ShipClass::has_projector) {return Err(OrderError::Unarmed);}
        b.beam_mode=mode;self.snapshot_platform(id,self.time);Ok(())
    }
    pub(super) fn weapon_visual(&self,kind:CombatKind,id:Option<BodyId>)->WeaponVisual {
        let Some(b)=id.and_then(|id|self.body(id)) else {return WeaponVisual::Standard;};
        if kind==CombatKind::BeamPulse && let Some(class)=b.ship_class && class.beam_pulses()>1 {return WeaponVisual::Pulse(class.beam_pulses());}
        if kind==CombatKind::MissileHit && let Some(m)=b.missile && m.payload==Payload::Kinetic {
            let count=self.body(m.launcher).and_then(|b|b.ship_class).map_or(5,ShipClass::canister_fragments);
            return WeaponVisual::Canister(count);
        }
        WeaponVisual::Standard
    }
}

/// One escort provides support while larger nearby allies supply the damage.
/// Uses only the faction view; solo ships always keep their damage beam.
pub fn support_worthwhile(view:&crate::session::View,ship:&crate::session::BodyView,target:&crate::session::ContactView)->bool {
    use crate::damage::System as S;
    let limit=INTERFERENCE_RANGE_LS*crate::units::LIGHT_SECOND;
    let Some(track)=&target.track else {return false;};
    if ship.ship_class!=Some(ShipClass::Frigate) || target.stale || target.detection<sensors::DetectionLevel::Resolved
        || !target.resolved_class.is_some_and(|c|c.beam_power()>=2.0)
        || (track.pos-ship.pos).length()>limit || ship.damage.operating_effectiveness(S::Ecm)<=0.0 {return false;}
    let friendly=|b:&&crate::session::BodyView|b.faction==ship.faction && b.armed && b.kind==BodyKind::Ship && b.jump.is_none()
        && b.damage.operating_effectiveness(S::Beam)>0.0 && (b.beam_auto || b.beam_target.is_some()) && (b.pos-track.pos).length()<=limit;
    let leader=view.bodies.iter().filter(friendly).filter(|b|b.ship_class==Some(ShipClass::Frigate) && b.damage.operating_effectiveness(S::Ecm)>0.0).map(|b|b.id).min();
    leader==Some(ship.id) && view.bodies.iter().filter(friendly).any(|b|b.id!=ship.id && b.ship_class.is_some_and(|c|c.beam_power()>=2.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::damage::{Condition,System as S};
    use crate::units::LIGHT_SECOND;
    fn fixture(range:f64,seed:u64)->(World,ContactId) {
        let specs=(0..2).map(|i|BodySpec {name:format!("Ship {i}"),kind:BodyKind::Ship,faction:FactionId(i),
            state:State {pos:Vec2::new(i as f64*range*LIGHT_SECOND,0.0),vel:Vec2::ZERO},thrust:Vec2::ZERO,magazine:4}).collect();
        let mut w=World::new(System {bodies:vec![]},specs,0.0,seed);
        for b in &mut w.bodies {b.controls.ecm=controls::Mode::Off;b.controls.screens=controls::Mode::Off;}
        let c=w.contact_id(FactionId(0),BodyId(1));
        w.perceptions.get_mut(&FactionId(0)).unwrap().ingest(Observation {detection:sensors::DetectionLevel::Identity,contact:c,sensor:BodyId(0),origin:Vec2::ZERO,
            emitted_at:0.0,sensor_received_at:0.0,decider_received_at:0.0,measurement:Measurement::BearingRange {bearing:0.0,range:range*LIGHT_SECOND,sigma_bearing:1e-10,sigma_range:1e-5},snr:1e9,source:Source::Echo},&w.system);
        (w,c)
    }
    #[test]
    fn interference_is_causal_costs_a_beam_shot_and_expires_without_damage() {
        let (mut w,c)=fixture(1.0,13);let source=BodyId(0);
        let damage=w.bodies[1].damage;
        w.set_beam_mode(source,BeamMode::Interference).unwrap();w.fire_beam(source,c).unwrap();
        assert_eq!(w.bodies[0].beam_emitted_j,SHIP_BEAM_ENERGY_J.value);
        assert!(w.bodies[0].thermal.heat_j>0.0);
        w.set_beam_mode(source,BeamMode::Damage).unwrap();assert_eq!(w.fire_beam(source,c),Err(OrderError::BeamRecharging));
        w.advance_to(0.9);assert_eq!(w.bodies[1].beam_coherence(w.time),1.0);
        w.advance_to(1.1);assert_eq!(w.bodies[1].beam_coherence(w.time),0.85);assert_eq!(w.bodies[1].damage,damage);assert!(w.hits.is_empty());
        w.advance_to(9.1);assert_eq!(w.bodies[1].beam_coherence(w.time),1.0);
    }
    #[test]
    fn projector_obeys_fit_damage_jump_range_and_occlusion() {
        for class in [ShipClass::Picket,ShipClass::Destroyer,ShipClass::Cruiser,ShipClass::Battleship,ShipClass::Transport] {
            let (mut w,_)=fixture(1.0,13);w.bodies[0].ship_class=Some(class);
            assert_eq!(w.set_beam_mode(BodyId(0),BeamMode::Interference),Err(OrderError::Unarmed));
        }
        for system in [S::Beam,S::Ecm,S::Power] {
            let (mut w,c)=fixture(1.0,13);w.set_beam_mode(BodyId(0),BeamMode::Interference).unwrap();w.bodies[0].damage.systems[system as usize]=Condition::Destroyed;
            assert_eq!(w.fire_beam(BodyId(0),c),Err(OrderError::PowerOrHeat));
        }
        let (mut w,c)=fixture(6.1,13);w.set_beam_mode(BodyId(0),BeamMode::Interference).unwrap();
        assert_eq!(w.fire_beam(BodyId(0),c),Err(OrderError::InvalidTarget));assert_eq!(w.bodies[0].beam_emitted_j,0.0);
        let (mut w,c)=fixture(1.0,13);w.set_beam_mode(BodyId(0),BeamMode::Interference).unwrap();
        w.bodies[0].jump=Some(jump::JumpState::Spooling {destination:Vec2::ZERO,depart_at:600.0});assert_eq!(w.fire_beam(BodyId(0),c),Err(OrderError::JumpBusy));
        let (mut w,c)=fixture(1.0,13);w.set_beam_mode(BodyId(0),BeamMode::Interference).unwrap();w.fire_beam(BodyId(0),c).unwrap();
        w.system.bodies.push(crate::celestial::Celestial {name:"Occluder".into(),kind:crate::celestial::CelestialKind::Planet,gm:0.0,radius:1000.0,orbit:crate::celestial::Orbit::Fixed(Vec2::new(0.5*LIGHT_SECOND,0.0))});
        w.advance_to(1.1);assert_eq!(w.bodies[1].disrupted_until,0.0);
    }
    #[test]
    fn refresh_never_stacks_strength_or_banks_duration() {
        let (mut w,c)=fixture(1.0,13);w.set_beam_mode(BodyId(0),BeamMode::Interference).unwrap();w.fire_beam(BodyId(0),c).unwrap();w.advance_to(5.0);
        w.fire_beam(BodyId(0),c).unwrap();w.advance_to(6.1);
        assert_eq!(w.bodies[1].beam_coherence(w.time),0.85);assert!((w.bodies[1].disrupted_until-14.0).abs()<0.01);
        w.advance_to(14.1);assert_eq!(w.bodies[1].beam_coherence(w.time),1.0);
    }
    #[test]
    fn defocusing_reduces_coupling_not_energy_heat_or_recharge() {
        let run=|disrupted| {
            let (mut w,c)=fixture(1.0,73);if disrupted {w.bodies[0].disrupted_until=10.0;}
            w.fire_beam(BodyId(0),c).unwrap();w.advance_to(1.1);
            (w.hits.iter().map(|h|h.energy_j).sum::<f64>(),w.bodies[0].beam_emitted_j,w.bodies[0].beam_ready_at,w.bodies[0].thermal.heat_j)
        };
        let normal=run(false);let disrupted=run(true);
        assert!(normal.0>0.0);assert!((disrupted.0/normal.0-0.85).abs()<1e-12);
        assert_eq!((normal.1,normal.2,normal.3),(disrupted.1,disrupted.2,disrupted.3));
    }
    #[test]
    fn fleet_support_requires_a_heavy_ally_and_only_one_escort_volunteers() {
        use crate::session::{LocalSession,Role};
        let (w,_)=fixture(1.0,13);let session=LocalSession::new(w);let mut v=session.view(Role::Faction(FactionId(0)));
        let mut target=v.contacts[0].clone();target.resolved_class=Some(ShipClass::Battleship);target.detection=sensors::DetectionLevel::Identity;
        let mut escort=v.bodies[0].clone();escort.beam_auto=true;v.bodies=vec![escort.clone()];
        assert!(!support_worthwhile(&v,&escort,&target));
        let mut heavy=escort.clone();heavy.id=BodyId(2);heavy.ship_class=Some(ShipClass::Battleship);v.bodies.push(heavy);
        assert!(support_worthwhile(&v,&escort,&target));
        let mut other=escort.clone();other.id=BodyId(3);v.bodies.push(other.clone());assert!(!support_worthwhile(&v,&other,&target));
        target.resolved_class=Some(ShipClass::Picket);assert!(!support_worthwhile(&v,&escort,&target));
    }
    #[test]
    fn disruption_reduces_spinal_coupling_and_does_not_change_in_flight_shots() {
        let run=|disrupted:bool,late:bool| {
            let (mut w,c)=fixture(1.0,73);
            w.bodies[0].ship_class=Some(ShipClass::Battleship);
            w.bodies[0].thermal=crate::scenario::transport_intercept_class(73,ShipClass::Battleship).bodies[1].thermal;
            if disrupted {w.bodies[0].disrupted_until=10.0;}
            w.fire_spinal(BodyId(0),c).unwrap();
            if late {w.bodies[0].disrupted_until=10.0;}
            w.advance_to(1.1);
            w.hits.iter().map(|h|h.energy_j).sum::<f64>()
        };
        let normal=run(false,false);
        assert!(normal>0.0);
        assert!((run(true,false)/normal-0.85).abs()<1e-12);
        assert_eq!(run(false,true),normal);
    }
    /// Controlled emissions, not a fleet victory-rate claim.
    #[test]
    #[ignore]
    fn projector_range_survey() {
        println!("range_ls,trials,disruptions");
        for range in [1.0,3.0,5.0,6.0,6.1] {
            let mut hits=0;
            for seed in 60000..60100 {
                let (mut w,c)=fixture(range,seed);
                w.set_beam_mode(BodyId(0),BeamMode::Interference).unwrap();
                let result=w.fire_beam(BodyId(0),c);
                if range>INTERFERENCE_RANGE_LS {assert_eq!(result,Err(OrderError::InvalidTarget));}
                else {result.unwrap();}
                w.advance_to(range+0.01);
                if w.bodies[1].beam_coherence(w.time)<1.0 {hits+=1;}
                assert!(w.hits.is_empty());
            }
            println!("{range},100,{hits}");
            if range>INTERFERENCE_RANGE_LS {assert_eq!(hits,0);}
        }
    }

}
