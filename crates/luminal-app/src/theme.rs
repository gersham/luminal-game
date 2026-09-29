//! Presentation vocabulary only. Never passed to the simulation or command layer.
use super::*;
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
pub(super) enum Theme {#[default] Luminal,Starfleet,GrimDark,Imperium,Culture}
impl Theme {
    pub const ALL:[Self;5]=[Self::Luminal,Self::Starfleet,Self::GrimDark,Self::Imperium,Self::Culture];
    pub fn name(self)->&'static str {match self {Self::Luminal=>"Luminal",Self::Starfleet=>"Starfleet",Self::GrimDark=>"Grim Dark",Self::Imperium=>"Imperium",Self::Culture=>"Culture"}}
    pub fn description(self)->&'static str {match self {
        Self::Luminal=>"Original naval terminology. Precise, spare, practical.",
        Self::Starfleet=>"Exploration fleet · scientific language and disciplined bridge crews.",
        Self::GrimDark=>"Gothic battlefleet · ancient machinery, ritual and overwhelming firepower.",
        Self::Imperium=>"Traveller-inspired navy · professional crews, frontier patrols and jump routes.",
        Self::Culture=>"Mind-led ships · elegant machinery, dry wit and politely excessive force.",
    }}
    pub fn weapon(self,p:Payload)->&'static str {match (self,p) {
        (Self::Starfleet,Payload::Nuclear)=>"PHOTON TORPEDO",(Self::Starfleet,Payload::Kinetic)=>"MICRO TORPEDO",(Self::Starfleet,Payload::Beam)=>"PHASER",
        (Self::GrimDark,Payload::Nuclear)=>"VOID TORPEDO",(Self::GrimDark,Payload::Kinetic)=>"STRIKE SALVO",(Self::GrimDark,Payload::Beam)=>"LANCE",
        (Self::Imperium,Payload::Nuclear)=>"SMART MISSILE",(Self::Imperium,Payload::Kinetic)=>"BURST MISSILE",(Self::Imperium,Payload::Beam)=>"BEAM LASER",
        (Self::Culture,Payload::Nuclear)=>"GUIDED DART",(Self::Culture,Payload::Kinetic)=>"SHARD SWARM",(Self::Culture,Payload::Beam)=>"COHERENT BEAM",
        (_,Payload::Nuclear)=>"LRM",(_,Payload::Kinetic)=>"SRM",(_,Payload::Beam)=>"BEAM",
    }}
    pub fn jump(self)->&'static str {match self {Self::Starfleet=>"WARP DRIVE",Self::GrimDark=>"WARP ENGINE",Self::Culture=>"DISPLACER",_=>"JUMP DRIVE"}}
    pub fn screens(self)->&'static str {match self {Self::Starfleet=>"DEFLECTORS",Self::GrimDark=>"VOID SHIELDS",Self::Culture=>"FIELDS",_=>"SCREENS"}}
    pub fn spinal(self)->&'static str {match self {Self::Starfleet=>"PHASER LANCE",Self::GrimDark=>"NOVA LANCE",Self::Imperium=>"SPINAL LASER",Self::Culture=>"GRID LANCE",_=>"SPINAL"}}
    pub fn system(self,s:System)->&'static str {match s {
        System::Jump=>self.jump(),System::Screens=>self.screens(),System::Beam=>self.weapon(Payload::Beam),System::Launcher=>self.weapon(Payload::Nuclear),System::SrmLauncher=>self.weapon(Payload::Kinetic),
        System::Power=>match self {Self::Starfleet=>"Warp core",Self::GrimDark=>"Plasma shrine",Self::Culture=>"Energy bank",_=>s.name()},
        System::Mind=>match self {Self::Starfleet=>"Main computer",Self::GrimDark=>"Machine spirit",Self::Culture=>"Mind",_=>s.name()},
        System::Repair=>match self {Self::GrimDark=>"Enginseer crews",Self::Culture=>"Repair drones",_=>s.name()},_=>s.name(),
    }}
    pub fn range(self,mode:MovementMode)->String {match mode {MovementMode::Long=>format!("{} RANGE",self.weapon(Payload::Nuclear)),MovementMode::Medium=>format!("{} RANGE",self.weapon(Payload::Kinetic)),MovementMode::Short=>format!("{} RANGE",self.weapon(Payload::Beam)),_=>mode.label().into()}}
}
