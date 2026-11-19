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

/// The interface colours that follow the theme. Friend, enemy and system-status colours do
/// not: green, red, orange and grey mean the same thing in every theme.
#[derive(Clone,Copy,Debug,PartialEq)]
pub(super) struct Palette {
    pub background:Color32,pub panel_bg:Color32,pub card_bg:Color32,pub card_raised:Color32,pub well_bg:Color32,
    pub edge:Color32,pub edge_dim:Color32,pub accent:Color32,pub warm:Color32,
    pub text_hi:Color32,pub text:Color32,pub text_muted:Color32,pub text_dim:Color32,
    pub frame:FrameShape,
}
/// How a button or card is cut. Every shape fills the same rectangle, so no layout moves.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(super) enum FrameShape {
    /// Two opposite corners cut. The original.
    Chamfer,
    /// All four corners cut, with an inner rule: engraved plate.
    Plate,
    /// Square, with ticks at the corners: a registry card.
    Card,
    /// Rounded.
    Soft,
}
const fn rgb(r:u8,g:u8,b:u8)->Color32 {Color32::from_rgb(r,g,b)}
impl Theme {
    pub(super) fn palette(self)->Palette {match self {
        Self::Luminal=>Palette {background:rgb(6,8,14),panel_bg:rgb(7,11,20),card_bg:rgb(11,18,31),card_raised:rgb(15,25,42),well_bg:rgb(5,9,16),
            edge:rgb(36,58,88),edge_dim:rgb(22,34,52),accent:rgb(84,206,236),warm:rgb(236,128,70),
            text_hi:rgb(226,238,250),text:rgb(174,194,216),text_muted:rgb(106,128,156),text_dim:rgb(62,78,100),frame:FrameShape::Chamfer},
        // Brass on soot. The warm tone is a burnt orange, kept well away from the red that means hostile or destroyed.
        Self::GrimDark=>Palette {background:rgb(9,6,6),panel_bg:rgb(14,10,8),card_bg:rgb(22,16,12),card_raised:rgb(33,24,16),well_bg:rgb(9,6,5),
            edge:rgb(96,74,42),edge_dim:rgb(54,42,27),accent:rgb(214,176,98),warm:rgb(214,112,52),
            text_hi:rgb(244,232,208),text:rgb(208,192,162),text_muted:rgb(144,128,102),text_dim:rgb(86,74,58),frame:FrameShape::Plate},
        // Navy registry: cream type and pale-gold rules on deep blue. Pale, so it is not the yellow of an unknown contact.
        Self::Imperium=>Palette {background:rgb(5,7,16),panel_bg:rgb(8,12,25),card_bg:rgb(12,18,37),card_raised:rgb(18,27,54),well_bg:rgb(5,8,18),
            edge:rgb(60,76,120),edge_dim:rgb(31,41,70),accent:rgb(236,216,164),warm:rgb(226,122,74),
            text_hi:rgb(242,238,224),text:rgb(198,198,190),text_muted:rgb(126,132,148),text_dim:rgb(72,80,100),frame:FrameShape::Card},
        // Cool, light and spare: pale type, cyan accents, soft corners.
        Self::Culture=>Palette {background:rgb(8,11,15),panel_bg:rgb(14,20,26),card_bg:rgb(20,28,36),card_raised:rgb(28,40,51),well_bg:rgb(9,13,18),
            edge:rgb(70,110,128),edge_dim:rgb(38,58,70),accent:rgb(120,236,240),warm:rgb(255,172,112),
            text_hi:rgb(246,252,255),text:rgb(206,224,232),text_muted:rgb(130,152,164),text_dim:rgb(76,94,104),frame:FrameShape::Soft},
    }}
}
thread_local! {
    /// The theme being drawn on this thread. Per thread, so tests that draw different themes at once do not collide.
    static CURRENT:std::cell::Cell<Theme>=const {std::cell::Cell::new(Theme::Luminal)};
}
/// Draw with this theme's palette from here on.
pub(super) fn set_current(theme:Theme) {CURRENT.with(|current|current.set(theme));}
/// The palette of the theme being drawn. Luminal until a frame says otherwise.
pub(super) fn pal()->Palette {CURRENT.with(|current|current.get()).palette()}

#[cfg(test)]
mod tests {
    use super::*;
    fn luminance(c:Color32)->f32 {
        let channel=|v:u8| {let v=v as f32/255.0;if v<=0.03928 {v/12.92} else {((v+0.055)/1.055).powf(2.4)}};
        0.2126*channel(c.r())+0.7152*channel(c.g())+0.0722*channel(c.b())
    }
    fn contrast(a:Color32,b:Color32)->f32 {let (hi,lo)=(luminance(a).max(luminance(b)),luminance(a).min(luminance(b)));(hi+0.05)/(lo+0.05)}
    fn distance(a:Color32,b:Color32)->f32 {
        ((a.r() as f32-b.r() as f32).powi(2)+(a.g() as f32-b.g() as f32).powi(2)+(a.b() as f32-b.b() as f32).powi(2)).sqrt()
    }
    #[test]
    fn every_theme_is_readable_and_keeps_status_colours_unambiguous() {
        for theme in Theme::ALL {
            let p=theme.palette();
            for surface in [p.panel_bg,p.card_bg,p.card_raised,p.well_bg] {
                assert!(contrast(p.text,surface)>=7.0,"{theme:?}: body text on a panel surface");
                assert!(contrast(p.text_muted,surface)>=3.5,"{theme:?}: muted text");
                assert!(contrast(p.accent,surface)>=6.0,"{theme:?}: accent");
            }
            assert!(luminance(p.background)<0.01,"{theme:?}: the plot stays dark, or additive light would wash out");
            // Hostile, destroyed and damaged keep their meaning. No theme tone may sit near them.
            for tone in [p.accent,p.warm] {
                assert!(distance(tone,ENEMY)>45.0 && distance(tone,SYS_DESTROYED)>45.0,"{theme:?}: {tone:?} reads as hostile");
                assert!(distance(tone,FRIEND)>70.0 && distance(tone,SYS_OK)>70.0,"{theme:?}: {tone:?} reads as friendly");
                assert!(distance(tone,UNKNOWN)>45.0 && distance(tone,BELIEF)>45.0,"{theme:?}: {tone:?} reads as an unknown contact");
            }
            assert!(distance(p.accent,p.warm)>60.0,"{theme:?}: the two tones are distinct");
        }
        let shapes:std::collections::BTreeSet<String>=Theme::ALL.iter().map(|t|format!("{:?}",t.palette().frame)).collect();
        assert_eq!(shapes.len(),4,"each theme cuts its frames its own way");
    }
    #[test]
    fn the_palette_follows_the_theme_set_on_this_thread_only() {
        assert_eq!(pal(),Theme::Luminal.palette(),"nothing set: the original colours");
        set_current(Theme::GrimDark);
        assert_eq!(pal(),Theme::GrimDark.palette());
        let elsewhere=std::thread::spawn(pal).join().unwrap();
        assert_eq!(elsewhere,Theme::Luminal.palette(),"another thread is not affected");
        set_current(Theme::Luminal);
    }
}
