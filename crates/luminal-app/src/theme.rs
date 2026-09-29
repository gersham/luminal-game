//! Presentation vocabulary only. Never passed to the simulation or command layer.
use super::*;
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
pub(super) enum Theme {Luminal,GrimDark,Imperium,#[default] Culture}
impl Theme {
    pub const ALL:[Self;4]=[Self::Luminal,Self::GrimDark,Self::Imperium,Self::Culture];
    pub fn name(self)->&'static str {match self {Self::Luminal=>"Luminal",Self::GrimDark=>"Grim Dark",Self::Imperium=>"Imperium",Self::Culture=>"Culture"}}
    pub fn description(self)->&'static str {match self {
        Self::Luminal=>"Original naval terminology. Precise, spare, practical.",
        Self::GrimDark=>"Gothic battlefleet · ancient machinery, ritual and overwhelming firepower.",
        Self::Imperium=>"Traveller-inspired navy · professional crews, frontier patrols and jump routes.",
        Self::Culture=>"Mind-led ships · elegant machinery, dry wit and politely excessive force.",
    }}
    pub fn weapon(self,p:Payload)->&'static str {match (self,p) {
        (Self::GrimDark,Payload::Nuclear)=>"VOID TORPEDO",(Self::GrimDark,Payload::Kinetic)=>"STRIKE SALVO",(Self::GrimDark,Payload::Beam)=>"LANCE",
        (Self::Imperium,Payload::Nuclear)=>"SMART MISSILE",(Self::Imperium,Payload::Kinetic)=>"BURST MISSILE",(Self::Imperium,Payload::Beam)=>"BEAM LASER",
        (Self::Culture,Payload::Nuclear)=>"GUIDED DART",(Self::Culture,Payload::Kinetic)=>"SHARD SWARM",(Self::Culture,Payload::Beam)=>"COHERENT BEAM",
        (_,Payload::Nuclear)=>"LRM",(_,Payload::Kinetic)=>"SRM",(_,Payload::Beam)=>"BEAM",
    }}
    pub fn jump(self)->&'static str {match self {Self::GrimDark=>"WARP ENGINE",Self::Culture=>"DISPLACER",_=>"JUMP DRIVE"}}
    pub fn screens(self)->&'static str {match self {Self::GrimDark=>"VOID SHIELDS",Self::Culture=>"FIELDS",_=>"SCREENS"}}
    pub fn spinal(self)->&'static str {match self {Self::GrimDark=>"NOVA LANCE",Self::Imperium=>"SPINAL LASER",Self::Culture=>"GRID LANCE",_=>"SPINAL"}}
    pub fn system(self,s:System)->&'static str {match s {
        System::Jump=>self.jump(),System::Screens=>self.screens(),System::Beam=>self.weapon(Payload::Beam),System::Launcher=>self.weapon(Payload::Nuclear),System::SrmLauncher=>self.weapon(Payload::Kinetic),
        System::Power=>match self {Self::GrimDark=>"Plasma shrine",Self::Culture=>"Energy bank",_=>s.name()},
        System::Mind=>match self {Self::GrimDark=>"Servitor",Self::Imperium=>"AI",Self::Culture=>"Mind",_=>s.name()},
        System::Repair=>match self {Self::GrimDark=>"Enginseer crews",Self::Culture=>"Repair drones",_=>s.name()},_=>s.name(),
    }}
    pub fn system_code(self,s:System)->&'static str {
        if s==System::Mind {match self {Self::GrimDark=>"SERV",Self::Imperium=>"AI",_=>s.code()}} else {s.code()}
    }
    pub fn projector(self)->&'static str {match self {Self::Culture=>"EFFECTOR",Self::GrimDark=>"VOX-SCOURGE",Self::Imperium=>"ELECTRONIC ATTACK",Self::Luminal=>"DIRECTED JAMMER"}}
    pub fn fitted_weapon(self,p:Payload,class:luminal_core::world::ShipClass)->&'static str {
        if p==Payload::Beam && class.beam_pulses()>1 {match self {Self::Culture=>"COHERENT BURST",Self::GrimDark=>"LANCE BATTERY",Self::Imperium=>"PULSE LASERS",Self::Luminal=>"PULSE BATTERY"}}
        else if p==Payload::Kinetic {match self {Self::Culture=>"SHARD SWARM",Self::GrimDark=>"FRAG TORPEDO",Self::Imperium=>"CANISTER MISSILE",Self::Luminal=>"FLECHETTE BUS"}}
        else {self.weapon(p)}
    }
    pub fn range(self,mode:MovementMode)->String {mode.label().into()}
}
