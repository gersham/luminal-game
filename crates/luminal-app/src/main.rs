//! Luminal desktop client. Talks to the simulation only through `session`.
mod audio;
mod weapon_effects;
mod jump_effects;
mod theme;
mod star_systems;
mod celestial_art;
mod startup;
mod roster;
mod ship_art;

const BUILD_VERSION:&str=env!("LUMINAL_VERSION");
const BUILD_COMMIT:&str=env!("LUMINAL_COMMIT");

fn build_dirty()->bool { env!("LUMINAL_DIRTY")=="1" }
fn build_hover()->String {
    let mut hover=format!("commit {BUILD_COMMIT}");
    if build_dirty() {hover.push_str(" · uncommitted changes");}
    hover
}

use luminal_core::world::jump::{JumpState, MAX_SOL_RADIUS_AU};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Vec2 as EVec2};
use luminal_core::celestial::{CelestialKind, Orbit};
use luminal_core::damage::{Condition, Report, System};
use luminal_core::kinematics::{State, Vec2};
use luminal_core::mind::{ContactId, Source};
use luminal_core::sensors::{self, wrap_angle};
use luminal_core::params;
use luminal_core::scenario::{self, Scenario, ESCORT, RAIDER};
use luminal_core::session::{
    AutopilotStatus, BodyId, BodyView, Command, ContactView, InterceptTarget, LocalSession, Order, Payload, Phase, Role, View,
};
use luminal_core::units::{AU, C, G0, LIGHT_SECOND};
use luminal_core::world::{BodyKind, FactionId};
use std::collections::{BTreeMap,BTreeSet,VecDeque};
use luminal_core::world::CombatKind;

#[derive(Clone)]
struct LogLine { key:String, text:String, color:Color32, at:f64, sim:f64, count:u32 }
type CombatLogKey=(u64,u64,u8,Option<u32>,Option<u32>);
#[derive(Default)]
struct TacticalLog {
    lines:VecDeque<LogLine>, contacts:BTreeSet<ContactId>,
    combat:BTreeSet<CombatLogKey>, raider_at:f64,
    hull:BTreeMap<BodyId,(f64,f64)>, now:f64,
}
impl TacticalLog {
    fn opacity(age:f64)->f32 {((40.0-age)/32.0).clamp(0.0,1.0) as f32}
    fn push(&mut self,key:String,text:String,color:Color32,sim:f64) {
        if let Some(i)=self.lines.iter().position(|line|line.key==key && self.now-line.at<4.0) {
            let old=self.lines.remove(i).unwrap();
            self.lines.push_front(LogLine {key,text,color,at:self.now,sim,count:old.count+1});
        } else {self.lines.push_front(LogLine {key,text,color,at:self.now,sim,count:1});}
        self.lines.truncate(8);
    }
    fn observe(&mut self,view:&View,selected:Option<BodyId>,now:f64) {
        self.now=now;
        self.lines.retain(|l|Self::opacity(now-l.at)>0.0);
        let new:Vec<_>=view.contacts.iter().filter(|c|self.contacts.insert(c.id) && !c.resolved_missile).collect();
        if !new.is_empty() {
            let text=if new.len()==1 {format!("NEW TARGET · {}",contact_label(new[0]))} else {format!("{} NEW TARGETS",new.len())};
            self.push("contacts".into(),text,ACCENT,view.time);
        }
        // Scan the received picture only: no truth events or unreceived damage.
        let key=|e:&luminal_core::world::CombatEvent|(e.received_at.to_bits(),e.emitted_at.to_bits(),e.kind as u8,e.own_body.map(|b|b.0),e.contact.map(|c|c.0));
        let fresh:Vec<_>=view.combat.iter().filter(|e|!self.combat.contains(&key(e))).collect();
        self.combat=view.combat.iter().map(key).collect();
        for b in view.bodies.iter().filter(|b|b.kind!=BodyKind::Missile) {
            let previous=self.hull.insert(b.id,(b.damage.damage.hull,b.damage.damage.armour));
            let damaged=previous.is_some_and(|(h,a)|b.damage.damage.hull<h-1e-6 || b.damage.damage.armour<a-1e-6);
            let hits=fresh.iter().filter(|e|e.kind==CombatKind::Impact && e.own_body==Some(b.id)).count();
            if damaged && hits==0 {
                let prefix=if selected==Some(b.id) {"OWN SHIP"} else {&b.name};
                let text=if damaged {format!("{prefix} DAMAGE · HULL {:.0}%",100.0*b.damage.damage.hull/b.damage.damage.hull_max)}
                    else {format!("{prefix} HIT · SCREENS ABSORBING")};
                self.push(format!("damage-{}",b.id.0),text,SYS_DAMAGED,view.time);
            }
        }
        for event in fresh.iter().rev() {
            match event.kind {
                CombatKind::Destroyed if event.subject_kind==Some(BodyKind::Ship)=>{
                    let name=if event.own_body==selected && selected.is_some() {"OWN SHIP".into()}
                        else if let Some(body)=event.own_body {view.bodies.iter().find(|b|b.id==body).map_or("FRIENDLY SHIP".into(),|b|b.name.clone())}
                        else if let Some(contact)=event.contact {view.contacts.iter().find(|c|c.id==contact).map(contact_label).unwrap_or_else(||format!("T{}",contact.0))}
                        else {"SHIP".into()};
                    self.push(format!("ship-destroyed-{:?}",key(event)),format!("{name} · DESTROYED"),DANGER,event.received_at);
                },
                CombatKind::InterferencePulse|CombatKind::FireControlDisrupted|CombatKind::WithdrawalStarted|CombatKind::WithdrawalCancelled|CombatKind::Withdrawn|CombatKind::Surrendered=>{
                    let name=event.own_body.and_then(|id|view.bodies.iter().find(|b|b.id==id)).map(|b|b.name.clone())
                        .or_else(||event.contact.map(|c|format!("T{}",c.0))).unwrap_or_else(||"SHIP".into());
                    self.push(format!("exit-{:?}",key(event)),format!("{name} · {}",event.kind.label().to_uppercase()),ACCENT,event.received_at);
                },
                CombatKind::Impact=>{
                    let source=if event.own_body==selected && selected.is_some() {"OWN SHIP".into()}
                        else if let Some(b)=event.own_body.and_then(|id|view.bodies.iter().find(|b|b.id==id)) {b.name.clone()}
                        else if let Some(c)=event.contact {view.contacts.iter().find(|t|t.id==c).map(contact_label).unwrap_or_else(||format!("T{}",c.0))}
                        else {"CONTACT".into()};
                    let text=event.damage.as_ref().map_or_else(||format!("{source} · HIT OBSERVED"),|d|format!("{source} DAMAGE · {d}"));
                    self.push(format!("impact-{:?}",key(event)),text,SYS_DAMAGED,event.received_at);
                },
                CombatKind::MissileHit|CombatKind::MissileMiss=>{
                    let hit=event.kind==CombatKind::MissileHit;
                    self.push(if hit {"missile-hit"} else {"missile-miss"}.into(),
                        format!("MISSILE {}{}",if hit {"HIT"} else {"MISSED"},event.own_body.map_or(String::new(),|id|format!(" · ROUND {}",id.0))),
                        if hit {WARM} else {TEXT_MUTED},event.received_at);
                },
                CombatKind::BeamPulse|CombatKind::SpinalPulse|CombatKind::PointDefence=>{
                    let weapon=match event.kind {CombatKind::PointDefence=>"PD LASER",CombatKind::SpinalPulse=>"SPINAL MOUNT",_=>"MAIN BEAM"};
                    let source=event.own_body.and_then(|id|view.bodies.iter().find(|b|b.id==id)).map_or(
                        if event.own_body.is_some() {"FRIENDLY"} else {"HOSTILE"},|b|b.name.as_str());
                    self.push(format!("beam-{weapon}-{source}"),format!("{source} · {weapon} FIRED"),ACCENT,event.received_at);
                },
                _=>{}, // Do not flood the display with beams, PD or expended rounds.
            }
        }
    }
}

fn main() -> eframe::Result {
    if std::env::args().skip(1).any(|arg| arg=="--version" || arg=="-V") {
        println!("luminal-app {BUILD_VERSION} commit={BUILD_COMMIT} dirty={}",u8::from(build_dirty()));
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1400.0, 900.0])
            .with_title(format!("Luminal {BUILD_VERSION}")).with_app_id("luminal").with_fullscreen(true)
            .with_icon(eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/icons/luminal.png"))
                .expect("bundled Luminal icon is valid PNG")),
        ..Default::default()
    };
    eframe::run_native("Luminal", options, Box::new(|_cc| Ok(Box::new(LuminalApp::new()))))
}

const WARPS: &[f64] = &[1.0, 10.0, 50.0, 100.0, 1_000.0];
const AUTO_MIN_WARP:f64=5.0;

fn range_warp(range:f64)->f64 {
    let anchors=[(luminal_core::units::LIGHT_SECOND,AUTO_MIN_WARP),
        (10.0*luminal_core::units::LIGHT_SECOND,10.0),(0.1*AU,50.0),(AU,300.0),(2.0*AU,1000.0)];
    if range<=anchors[0].0 {return AUTO_MIN_WARP;}
    for pair in anchors.windows(2) {
        let [(a,x),(b,y)]=[pair[0],pair[1]];
        if range<=b {
            let f=(range/a).ln()/(b/a).ln();
            return (x.ln()+(y.ln()-x.ln())*f).exp().clamp(AUTO_MIN_WARP,1000.0);
        }
    }
    1000.0
}
fn smooth_warp(current:f64,target:f64,dt:f64)->f64 {
    let current=current.clamp(AUTO_MIN_WARP,1000.0);
    let target=target.clamp(AUTO_MIN_WARP,1000.0);
    let tau=if target<current {0.35} else {2.0};
    let alpha=1.0-(-dt/tau).exp();
    (current.ln()+(target.ln()-current.ln())*alpha).exp().clamp(AUTO_MIN_WARP,1000.0)
}
fn desired_auto_warp(view:&View)->f64 {
    let Some(own)=view.bodies.iter().find(|b|b.controllable && b.kind==BodyKind::Ship) else {return AUTO_MIN_WARP;};
    let nearest=view.contacts.iter().filter_map(|c|c.track.as_ref()).map(|track| {
        let relative=track.pos-own.pos;
        let velocity=track.vel-own.vel;
        // Look two real-time seconds ahead at the current warp, so a fast
        // closing missile can slow AUTO before entering knife-fight range.
        let closest=(-relative.dot(velocity)/velocity.dot(velocity).max(1e-12)).clamp(0.0,2.0*view.warp);
        let uncertainty=3.0*(track.cov[0][0]+track.cov[1][1]).max(0.0).sqrt();
        ((relative+velocity*closest).length()-uncertainty).max(0.0)
    }).min_by(f64::total_cmp);
    nearest.map_or(if view.contacts.is_empty() {1000.0} else {100.0},range_warp)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn energy_uses_integer_si_units() {
        for (value,expected) in [(0.0,"0 J"),(999.0,"999 J"),(999.9e9,"1 TJ"),(420e9,"420 GJ"),(12e15,"12 PJ"),(1e18,"1 EJ"),(2e24,"2 YJ")] {
            assert_eq!(fmt_energy(value),expected);
        }
    }
    #[test]
    fn speeds_switch_to_light_speed_units_above_threshold() {
        assert_eq!(fmt_speed(0.5*C),"0.500c");
        assert_eq!(fmt_speed(-0.02*C),"-0.020c");
        assert!(fmt_speed(0.01*C).ends_with("km/s"));
        assert!(fmt_speed(0.0101*C).ends_with("c"));
    }
    #[test]
    fn tactical_keys_map_orders_and_respect_missile_gates() {
        use egui::Key;
        let app=LuminalApp::new();
        let view=app.session.view(Role::Faction(ESCORT));
        let mut ship=view.bodies.iter().find(|b|b.controllable).unwrap().clone();
        let contact=&view.contacts[0];
        use luminal_core::world::controls::{ControlledSystem as C,Mode};
        for (key,system) in [(Key::E,C::Ecm),(Key::R,C::Screens)] {
            assert!(matches!(tactical_shortcut(key,&ship,None),Some(Command::SetSystemMode {system:s,mode:Mode::On,..}) if s==system));
        }
        assert!(matches!(tactical_shortcut(Key::A,&ship,None),Some(Command::SetSystemMode {system:C::Active,mode:Mode::Auto,..})));
        ship.controls.active=Mode::Auto;
        assert!(matches!(tactical_shortcut(Key::A,&ship,None),Some(Command::SetSystemMode {system:C::Active,mode:Mode::Off,..})));
        assert!(matches!(tactical_shortcut(Key::Num1,&ship,Some(contact)),Some(Command::CombatRange {standoff:false,..})));
        assert!(matches!(tactical_shortcut(Key::Num2,&ship,Some(contact)),Some(Command::CombatRange {standoff:true,..})));
        assert!(matches!(tactical_shortcut(Key::Num3,&ship,Some(contact)),Some(Command::Flyby {..})));
        assert!(matches!(tactical_shortcut(Key::Num0,&ship,Some(contact)),Some(Command::Alongside {..})));
        assert!(matches!(tactical_shortcut(Key::P,&ship,None),Some(Command::Ping {..})));
        assert!(matches!(tactical_shortcut(Key::L,&ship,Some(contact)),Some(Command::Launch {payload:Payload::Nuclear,..})));
        assert!(tactical_shortcut(Key::S,&ship,Some(contact)).is_none());
        assert!(tactical_shortcut(Key::L,&ship,None).is_none());
        assert!(tactical_shortcut(Key::Num4,&ship,Some(contact)).is_none());
        ship.missile_queued[Payload::Nuclear.index()]=ship.magazine[Payload::Nuclear.index()];
        assert!(tactical_shortcut(Key::L,&ship,Some(contact)).is_none());
        ship.missile_queued[Payload::Nuclear.index()]=0;
        ship.damage.damage.systems[System::Power as usize]=Condition::Damaged;
        assert!(tactical_shortcut(Key::L,&ship,Some(contact)).is_none());
    }
    #[test]
    fn shift_launch_queues_only_unreserved_rounds_and_respects_gates() {
        let app=LuminalApp::new();let view=app.session.view(app.role);
        let mut ship=view.bodies.iter().find(|b|b.controllable).unwrap().clone();
        let mut target=view.contacts[0].clone();
        let mut track=test_track();track.pos=ship.pos+Vec2::new(LIGHT_SECOND,0.0);
        track.cov=[[0.0;2];2];track.velocity_sigma=0.0;
        target.track=Some(track);target.stale=false;target.detection=luminal_core::sensors::DetectionLevel::Resolved;
        for (key,payload) in [(egui::Key::L,Payload::Nuclear),(egui::Key::S,Payload::Kinetic)] {
            ship.magazine[payload.index()]=20;ship.missile_queued[payload.index()]=3;
            let commands=tactical_shortcut_commands(key,true,&ship,Some(&target));
            assert_eq!(commands.len(),17);
            assert!(commands.iter().all(|c|matches!(c,Command::Launch {target:id,payload:p,..} if *id==target.id && *p==payload)));
            assert_eq!(tactical_shortcut_commands(key,false,&ship,Some(&target)).len(),1);
            assert!(tactical_shortcut_commands(key,true,&ship,None).is_empty());
            ship.missile_queued[payload.index()]=20;
            assert!(tactical_shortcut_commands(key,true,&ship,Some(&target)).is_empty());
        }
        ship.missile_queued=[0;2];ship.damage.damage.systems[System::Launcher as usize]=Condition::Destroyed;
        assert!(tactical_shortcut_commands(egui::Key::L,true,&ship,Some(&target)).is_empty());
    }

    #[test]
    fn player_tracking_follows_own_ship_without_changing_zoom_or_target() {
        let mut app=LuminalApp::new();
        assert!(app.track_player,"tracking is on by default");
        let mut view=app.session.view(Role::Faction(ESCORT));
        let zoom=app.camera.km_per_px;
        let target=app.inspected;
        app.track_player=true;
        let position=Vec2::new(123.0,456.0);
        view.bodies.iter_mut().find(|b|b.controllable).unwrap().pos=position;
        app.update_player_tracking(&view);
        assert_eq!(app.camera.center,position);
        assert_eq!(app.camera.km_per_px,zoom);
        assert!(app.inspected==target);
        app.track_player=false;
        view.bodies.iter_mut().find(|b|b.controllable).unwrap().pos=Vec2::ZERO;
        app.update_player_tracking(&view);
        assert_eq!(app.camera.center,position);
    }
    #[test]
    fn manual_ping_holds_zoom_until_sweep_finishes() {
        let mut app=LuminalApp::new();
        app.command(Command::Ping {body:BodyId(1)});
        let mut view=app.session.view(app.role);
        let rect=Rect::from_min_size(Pos2::ZERO,EVec2::new(1000.0,600.0));
        app.camera.km_per_px=1.0;
        app.update_tracking_zoom(&view,rect,0.1);
        assert_eq!(app.camera.km_per_px,1.0);
        assert!(app.manual_ping_zoom_until>view.time);
        view.time=app.manual_ping_zoom_until+1.0;
        app.update_tracking_zoom(&view,rect,0.1);
        assert!(app.camera.km_per_px>1.0,"automatic zoom resumes after the sweep");
    }

    #[test]
    fn tracking_zoom_smoothly_frames_target_and_preserves_bearing_only_scale() {
        let mut app=LuminalApp::new();
        let mut view=app.session.view(app.role);
        let rect=Rect::from_min_size(Pos2::ZERO,EVec2::new(1000.0,600.0));
        let own=view.bodies.iter().find(|b|b.controllable).unwrap().pos;
        app.inspected=Some(Selection::Contact(view.contacts[0].id));
        let initial=app.camera.km_per_px;
        app.update_tracking_zoom(&view,rect,0.1);
        assert_eq!(app.camera.km_per_px,initial,"no invented range for bearings");
        let mut track=test_track();
        track.pos=own+Vec2::new(AU,AU);track.cov=[[0.0;2];2];
        view.contacts[0].track=Some(track);
        app.inspected=Some(Selection::Contact(view.contacts[0].id));
        app.camera.km_per_px=1000.0;
        app.update_player_tracking(&view);
        app.update_tracking_zoom(&view,rect,0.1);
        assert!(app.camera.km_per_px>1000.0 && app.camera.km_per_px<AU/180.0,"zoom eases instead of snapping");
        for _ in 0..100 {app.update_tracking_zoom(&view,rect,0.1);}
        let p=to_screen(&app.camera,rect,view.contacts[0].track.as_ref().unwrap().pos);
        assert!(rect.shrink(50.0).contains(p));
        assert_eq!(app.camera.center,own);
        let wide=app.camera.km_per_px;
        view.contacts[0].track.as_mut().unwrap().pos=own+Vec2::new(LIGHT_SECOND,0.0);
        app.update_tracking_zoom(&view,rect,0.1);
        assert!(app.camera.km_per_px<wide && app.camera.km_per_px>1000.0,"zoom in is gradual too");
        app.track_player=false;
        let manual=app.camera.km_per_px;
        app.update_tracking_zoom(&view,rect,0.1);
        assert_eq!(app.camera.km_per_px,manual);
        app.track_player=true;app.tracking_zoom_hold=2.0;
        app.update_tracking_zoom(&view,rect,0.1);
        assert_eq!(app.camera.km_per_px,manual,"mouse wheel gets a brief override");
    }

    #[test]
    fn edge_pan_is_time_scaled_and_only_near_map_boundaries() {
        let r=Rect::from_min_size(Pos2::ZERO,EVec2::new(1000.0,600.0));
        assert_eq!(edge_pan_delta(r,r.center(),0.1),Vec2::ZERO);
        assert_eq!(edge_pan_delta(r,Pos2::new(-1.0,100.0),0.1),Vec2::ZERO);
        assert_eq!(edge_pan_delta(r,r.left_top(),0.1),Vec2::new(-70.0,70.0));
        assert_eq!(edge_pan_delta(r,r.right_bottom(),0.05),Vec2::new(35.0,-35.0));
    }

    #[test]
    fn tracking_zoom_includes_new_enemy_while_inspecting_charge() {
        let mut app=LuminalApp::new();let mut view=app.session.view(app.role);
        let rect=Rect::from_min_size(Pos2::ZERO,EVec2::new(1000.0,600.0));
        let own=view.bodies.iter().find(|b|b.controllable).unwrap().pos;
        app.inspected=Some(Selection::Body(BodyId(0)));app.camera.km_per_px=1000.0;
        let mut track=test_track();track.pos=own+Vec2::new(AU,AU);track.cov=[[0.0;2];2];
        view.contacts[0].track=Some(track);view.contacts[0].stale=false;
        for _ in 0..50 {app.update_player_tracking(&view);app.update_tracking_zoom(&view,rect,0.1);}
        assert!(rect.shrink(50.0).contains(to_screen(&app.camera,rect,view.contacts[0].track.as_ref().unwrap().pos)));
        assert!(app.inspected==Some(Selection::Body(BodyId(0))),"framing must not change orders or selection");
        let wide=app.camera.km_per_px;view.contacts[0].stale=true;
        app.update_tracking_zoom(&view,rect,0.1);assert!(app.camera.km_per_px<wide);
    }

    #[test]
    fn tracking_zoom_includes_targets_target_using_visible_positions() {
        let app=LuminalApp::new();
        let mut view=app.session.view(app.role);
        let rect=Rect::from_min_size(Pos2::ZERO,EVec2::new(1000.0,600.0));
        let own=view.bodies.iter().find(|b|b.controllable).unwrap().pos;
        let contact=&mut view.contacts[0];
        let mut track=test_track();track.pos=own+Vec2::new(LIGHT_SECOND,0.0);track.cov=[[0.0;2];2];
        contact.track=Some(track);contact.stale=false;
        let selected=InterceptTarget::Contact(contact.id);
        let transport=view.bodies.iter_mut().find(|b|b.id==BodyId(0)).unwrap();
        transport.pos=own+Vec2::new(-20.0*LIGHT_SECOND,30.0*LIGHT_SECOND);
        let transport_pos=transport.pos;
        let primary=tracking_zoom_scale(&view,own,rect,selected,None).unwrap();
        let expanded=tracking_zoom_scale(&view,own,rect,selected,Some(InterceptTarget::Own(BodyId(0)))).unwrap();
        assert!(expanded>primary);
        let cam=Camera {center:own,km_per_px:expanded};
        assert!(rect.shrink(50.0).contains(to_screen(&cam,rect,transport_pos)));
        assert_eq!(tracking_zoom_scale(&view,own,rect,selected,Some(InterceptTarget::Contact(ContactId(999)))),Some(primary));
    }

    #[test]
    fn old_target_outages_expire_instead_of_claiming_current_disablement() {
        let app=LuminalApp::new();
        let mut view=app.session.view(Role::Faction(ESCORT));
        let mut contact=view.contacts[0].clone();
        let mut report=Report {damage:Default::default(),installed:[true;System::COUNT],observed_at:0.0,screen_available:1.0};
        report.damage.systems[System::Power as usize]=Condition::Damaged;
        contact.damage=Some(report);contact.last_emitted_at=0.0;contact.last_received_at=600.0;
        view.time=600.0;
        assert!(target_system_report(&contact,&view).is_some(),"light travel does not expire a newly received snapshot");
        view.time=600.0+luminal_core::damage::SYSTEM_REPAIR_SECONDS;
        assert!(target_system_report(&contact,&view).is_none());
        assert!(Chip::Unknown.color()!=Chip::Inoperative.color());
    }
    fn test_track()->luminal_core::session::TrackView {
        luminal_core::session::TrackView {velocity_sigma:1.0,pos:Vec2::new(2.0*AU,0.0),vel:Vec2::new(1.0,0.0),accel:Vec2::ZERO,cov:[[1.0,0.0],[0.0,1.0]],updated_at:0.0,updates:4}
    }

    #[test]
    fn hover_details_use_received_objects_and_preserve_unknown_information() {
        let app=LuminalApp::new();
        let mut view=app.session.view(Role::Faction(ESCORT));
        let rect=Rect::from_min_size(Pos2::ZERO,EVec2::new(1000.0,700.0));
        let c=&mut view.contacts[0];
        c.track=Some(test_track());
        c.damage=None;
        c.resolved_kind=None;
        c.resolved_class=None;
        c.quality="position resolution";
        let cam=Camera {center:c.track.as_ref().unwrap().pos,km_per_px:1000.0};
        let lines=hover_details(&view,&cam,rect,rect.center()).unwrap();
        assert!(lines[0].starts_with("T"));
        assert!(lines[1].contains("UNKNOWN CLASS"));
        assert_eq!(lines[2],"SPEED UNKNOWN");
        assert!(lines.iter().any(|s|s=="HULL / SYSTEMS UNKNOWN"));
        let ranged=hover_details_for_target(&view,&cam,rect,rect.center(),Some(Selection::Body(BodyId(1))),Some(Selection::Contact(view.contacts[0].id))).unwrap();
        assert!(ranged.iter().any(|line|line.starts_with("FROM OWN SHIP")));
        assert!(ranged.iter().any(|line|line==&format!("TO TARGET  {} · EST",fmt_distance(0.0))));
        view.bodies.clear();
        view.contacts.clear();
        view.celestials.clear();
        assert!(hover_details(&view,&cam,rect,rect.center()).is_none());
    }

    #[test]
    fn combat_log_reports_missile_results_and_beams_not_missile_destruction() {
        let app=LuminalApp::new();
        let mut view=app.session.view(Role::Faction(ESCORT));
        view.contacts.clear();
        view.combat=[CombatKind::Destroyed,CombatKind::MissileHit,CombatKind::MissileMiss,CombatKind::BeamPulse].into_iter().enumerate().map(|(i,kind)|
            luminal_core::world::CombatEvent {weapon_visual:luminal_core::world::weapon_fit::WeaponVisual::Standard,target:None,velocity:None,subject_kind:Some(BodyKind::Missile),impact_strength:0.0,damage:None,contact:None,aim:None,pos:None,kind,own_body:Some(BodyId(1)),emitted_at:i as f64,received_at:i as f64}).collect();
        let mut log=TacticalLog::default();
        log.observe(&view,Some(BodyId(1)),0.0);
        assert_eq!(log.lines.len(),3);
        for expected in ["MISSILE HIT","MISSILE MISSED","MAIN BEAM FIRED"] {
            assert!(log.lines.iter().any(|line|line.text.contains(expected)));
        }
        log.observe(&view,Some(BodyId(1)),1.0);
        assert_eq!(log.lines.len(),3);
        assert!(log.lines.iter().all(|line|line.count==1));
    }

    #[test]
    fn tactical_log_is_bounded_coalesced_and_fades_in_real_time() {
        let mut log=TacticalLog::default();
        for i in 0..12 {log.push(i.to_string(),"event".into(),ACCENT,0.0);}
        assert_eq!(log.lines.len(),8);
        log.now=2.0;
        log.push("11".into(),"updated".into(),ACCENT,1000.0);
        assert_eq!(log.lines.len(),8);
        assert_eq!(log.lines[0].count,2);
        assert_eq!(TacticalLog::opacity(8.0),1.0);
        assert_eq!(TacticalLog::opacity(24.0),0.5);
        assert_eq!(TacticalLog::opacity(40.0),0.0);
    }

    #[test] fn ship_destruction_logs_survive_removed_objects_and_ignore_expendables() {
        let app=LuminalApp::new();let mut view=app.session.view(Role::Faction(ESCORT));
        view.bodies.clear();view.contacts.clear();
        view.combat=[Some(BodyKind::Ship),Some(BodyKind::Ship),Some(BodyKind::Missile),Some(BodyKind::Missile),None].into_iter().enumerate().map(|(i,subject_kind)|
            luminal_core::world::CombatEvent {weapon_visual:luminal_core::world::weapon_fit::WeaponVisual::Standard,target:None,velocity:None,subject_kind,impact_strength:0.0,damage:None,
                contact:if i==1 {Some(ContactId(1))} else {None},own_body:if i==0 {Some(BodyId(1))} else {None},
                aim:None,pos:None,kind:CombatKind::Destroyed,emitted_at:i as f64,received_at:10.0+i as f64}).collect();
        let mut log=TacticalLog::default();log.observe(&view,Some(BodyId(1)),0.0);
        assert_eq!(log.lines.len(),2);
        assert!(log.lines.iter().any(|l|l.text=="OWN SHIP · DESTROYED"));
        assert!(log.lines.iter().any(|l|l.text=="T1 · DESTROYED"));
        log.observe(&view,Some(BodyId(1)),1.0);
        assert_eq!(log.lines.len(),2);assert!(log.lines.iter().all(|l|l.count==1));
    }

    #[test]
    fn tactical_log_reports_contacts_and_own_damage_once() {
        let app=LuminalApp::new();
        let mut view=app.session.view(Role::Faction(ESCORT));
        let mut log=TacticalLog::default();
        log.observe(&view,Some(BodyId(1)),0.0);
        assert!(log.lines.iter().any(|l|l.text.contains("NEW TARGET")));
        let n=log.lines.len();
        log.observe(&view,Some(BodyId(1)),1.0);
        assert_eq!(log.lines.len(),n);
        view.bodies.iter_mut().find(|b|b.id==BodyId(1)).unwrap().damage.damage.hull*=0.9;
        log.observe(&view,Some(BodyId(1)),2.0);
        assert!(log.lines[0].text.contains("OWN SHIP DAMAGE"));
        log.observe(&view,Some(BodyId(1)),3.0);
        assert_eq!(log.lines[0].count,1);
        log.observe(&view,Some(BodyId(1)),43.0);
        assert!(log.lines.is_empty());
    }
    #[test]
    fn tactical_log_keeps_each_damage_event_even_in_one_frame() {
        let app=LuminalApp::new();
        let mut view=app.session.view(Role::Faction(ESCORT));
        let mut log=TacticalLog::default();
        log.observe(&view,Some(BodyId(1)),0.0);
        view.combat=(1..=2).map(|i|luminal_core::world::CombatEvent {weapon_visual:luminal_core::world::weapon_fit::WeaponVisual::Standard,target:None,velocity:None,
            subject_kind:Some(BodyKind::Ship),
            impact_strength:0.65,
            damage:Some(if i==1 {"SCREEN +3.00 TJ"} else {"HULL -2.00 · PROP DAMAGED"}.into()),
            contact:None,aim:None,pos:None,kind:CombatKind::Impact,own_body:Some(BodyId(1)),
            emitted_at:i as f64,received_at:i as f64,
        }).collect();
        log.observe(&view,Some(BodyId(1)),1.0);
        assert_eq!(log.lines.iter().filter(|l|l.text.contains("OWN SHIP DAMAGE")).count(),2);
        assert!(log.lines.iter().any(|l|l.text.contains("PROP DAMAGED")));
        log.observe(&view,Some(BodyId(1)),2.0);
        assert!(log.lines.iter().all(|l|l.count==1));
    }

    #[test]
    fn restart_restores_transport_follow_and_historical_enemy_bearing() {
        let mut app=LuminalApp::new();
        app.session.tick(2.0);
        app.command(Command::SetWarp(1.0));
        app.command(Command::SetPaused(true));
        app.inspected=None;
        app.opening_fit=false;
        app.bearing_display.insert((ContactId(99),BodyId(99)),(1.0,1.0));
        app.restart_scenario();
        assert!(app.track_player,"restart restores tracking");
        let view=app.session.view(Role::Faction(ESCORT));
        assert_eq!(view.time,0.0);
        assert_eq!(view.warp,AUTO_MIN_WARP);
        assert!(app.auto_speed);
        assert!(!view.paused);
        assert!(app.fit_pending && app.opening_fit && app.bearing_display.is_empty());
        assert!(app.tactical_log.lines.is_empty());
        for id in [BodyId(1), BodyId(2)] {
            let truth = app.session.view(Role::Spectator);
            let b = truth.bodies.iter().find(|b| b.id == id).unwrap();
            assert!(b.has_screen && b.screen_up);
            assert_eq!(b.thermal.field, 1.0);
        }
        assert!(app.inspected==Some(Selection::Body(BodyId(0))));
        let ship=view.bodies.iter().find(|b|b.id==BodyId(1)).unwrap();
        let enemy=&view.contacts[0];
        assert!(enemy.last_emitted_at<0.0,"briefing must use historical light");
        assert!(!contact_has_course(enemy));
        assert!(enemy.track.is_none() && enemy.resolved_kind.is_none());
        assert!(matches!(ship.autopilot.map(|a|a.order),Some(Order::Follow {target:BodyId(0),..})));
        assert!(enemy.bearings[0].received_at-enemy.bearings[0].emitted_at>600.0);
        assert_eq!(bearing_opacity(&enemy.bearings[0],view.time),1.0);
        assert_eq!(contact_label(enemy),"T1");
        let rect=Rect::from_min_size(Pos2::ZERO,EVec2::new(1000.0,700.0));
        app.fit(&view,rect);
        let target_pos=view.bodies.iter().find(|b|b.id==BodyId(0)).unwrap().pos;
        let midpoint=(ship.pos+target_pos)*0.5;
        assert!((app.camera.center-midpoint).length()<1e-6);
        for pos in [ship.pos,target_pos] {
            let delta=pos-app.camera.center;
            assert!(delta.x.abs()/app.camera.km_per_px<rect.width() as f64*0.5);
            assert!(delta.y.abs()/app.camera.km_per_px<rect.height() as f64*0.5);
        }
    }

    #[test]
    fn chevrons_require_a_fresh_resolved_course() {
        let mut c=ContactView {identified_name:None,display_class:None,detection:sensors::DetectionLevel::Resolved,ping_remaining:0.0,active_fire_control:0.0,reporting_sensor:None,resolved_class:None,resolved_interceptor:false,damage:None,id:ContactId(1),resolved_kind:Some(BodyKind::Ship),resolved_missile:false,
            quality:"position resolution",stale:false,bearings:vec![],
            track:Some(luminal_core::session::TrackView {velocity_sigma:1.0,pos:Vec2::ZERO,vel:Vec2::new(1.0,0.0),accel:Vec2::ZERO,
                cov:[[1.0,0.0],[0.0,1.0]],updated_at:0.0,updates:4}),
            last_emitted_at:0.0,last_received_at:0.0,last_source:Source::Echo,last_snr:10.0,last_range:Some(1.0)};
        c.detection=sensors::DetectionLevel::Approximate;
        assert!(!contact_has_course(&c));
        assert_eq!(contact_label(&c),"T1");
        c.resolved_class=Some(luminal_core::world::ShipClass::Frigate);
        assert_eq!(contact_label(&c),"FF1");
        c.quality="Resolved";c.detection=sensors::DetectionLevel::Resolved;
        assert!(contact_has_course(&c));
        c.stale=true;
        assert!(!contact_has_course(&c));
        c.stale=false;
        c.track.as_mut().unwrap().vel=Vec2::ZERO;
        assert!(!contact_has_course(&c));
        c.track=None;
        assert!(!contact_has_course(&c));
    }

    #[test]
    fn auto_speed_is_smooth_bounded_and_manual_selection_disables_it() {
        assert_eq!(range_warp(0.0),AUTO_MIN_WARP);
        assert_eq!(smooth_warp(1.0,1.0,0.1),AUTO_MIN_WARP);
        assert!((range_warp(AU)-300.0).abs()<1e-9);
        assert_eq!(range_warp(3.0*AU),1000.0);
        let mut previous=1.0;
        for i in 0..100 {
            let next=range_warp(2.0*AU*i as f64/99.0);
            assert!(next>=previous && next<=1000.0);previous=next;
        }
        let mut speed=1.0;
        for _ in 0..100 {let next=smooth_warp(speed,1000.0,0.1);assert!(next>=speed && next<1000.0);speed=next;}
        assert!(speed>950.0);
        assert!((smooth_warp(1000.0,1.0,0.1)-1.0).abs()<999.0);
        let mut app=LuminalApp::new();
        app.command(Command::SetWarp(50.0));
        app.update_auto_speed(1.0);
        assert!(!app.auto_speed);
        assert_eq!(app.session.view(app.role).warp,50.0);
        app.auto_speed=true;
        app.command(Command::SetPaused(true));
        app.update_auto_speed(1.0);
        assert_eq!(app.session.view(app.role).warp,50.0);
    }

    #[test]
    fn auto_speed_uses_nearest_received_track_and_anticipates_closure() {
        let app=LuminalApp::new();
        let mut view=app.session.view(app.role);
        assert_eq!(desired_auto_warp(&view),100.0,"bearings do not invent range");
        let own=view.bodies.iter().find(|b|b.controllable).unwrap().clone();
        let mut track=test_track();
        track.pos=own.pos+Vec2::new(AU,0.0);track.vel=own.vel;track.cov=[[0.0;2];2];
        view.contacts[0].track=Some(track.clone());
        assert!((desired_auto_warp(&view)-300.0).abs()<1e-6);
        let mut nearby=view.contacts[0].clone();
        nearby.track.as_mut().unwrap().pos=own.pos+Vec2::new(luminal_core::units::LIGHT_SECOND*0.5,0.0);
        view.contacts.push(nearby);
        assert_eq!(desired_auto_warp(&view),AUTO_MIN_WARP,"nearest contact wins, not selected target");
        view.contacts.pop();view.warp=1000.0;
        view.contacts[0].track.as_mut().unwrap().vel=own.vel+Vec2::new(-AU/1000.0,0.0);
        assert_eq!(desired_auto_warp(&view),AUTO_MIN_WARP,"fast closure slows before the pass");
        view.contacts.clear();assert_eq!(desired_auto_warp(&view),1000.0);
    }

    #[test]
    fn auto_target_waits_for_resolved_ship_and_preserves_orders_and_selection() {
        let mut app=LuminalApp::new();let mut view=app.session.view(Role::Faction(ESCORT));
        app.acquire_first_target(&view);
        assert!(matches!(app.inspected,Some(Selection::Body(BodyId(0)))));
        view.contacts[0].detection=sensors::DetectionLevel::Resolved;view.contacts[0].stale=false;
        view.contacts[0].resolved_kind=Some(BodyKind::Missile);
        app.acquire_first_target(&view);assert!(matches!(app.inspected,Some(Selection::Body(_))));
        view.contacts[0].resolved_kind=Some(BodyKind::Ship);
        app.acquire_first_target(&view);assert!(app.inspected==Some(Selection::Contact(view.contacts[0].id)));
        let mut other=view.contacts[0].clone();other.id=ContactId(999);view.contacts.insert(0,other);
        app.acquire_first_target(&view);assert!(app.inspected==Some(Selection::Contact(view.contacts[1].id)));
        let own=app.session.view(Role::Faction(ESCORT));
        assert!(matches!(own.bodies.iter().find(|b|b.id==BodyId(1)).unwrap().autopilot.map(|a|a.order),Some(Order::Follow {target:BodyId(0),..})));
    }

    #[test]
    fn hostile_ping_does_not_replace_a_resolved_ship_chevron() {
        let app=LuminalApp::new();
        let mut view=app.session.view(Role::Faction(ESCORT));
        let c=&mut view.contacts[0];
        c.track=Some(test_track());c.quality="Resolved";c.detection=sensors::DetectionLevel::Resolved;c.stale=false;
        c.resolved_kind=Some(BodyKind::Ship);
        let c=c.clone();
        view.hostile_pings=vec![luminal_core::session::PingSighting {contact:c.id,emitted_at:-600.0,received_at:0.0,pos:Some(c.track.as_ref().unwrap().pos),vel:Vec2::ZERO,initial_radius:1.0,observer:Vec2::ZERO,bearing:0.0}];
        let ctx=egui::Context::default();
        let mut output=ctx.run_ui(Default::default(),|ui| {
            let ctx=ui.ctx();
            let painter=ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground,egui::Id::new("test")));
            let rect=Rect::from_min_size(Pos2::ZERO,EVec2::new(1000.0,700.0));
            let cam=Camera {center:c.track.as_ref().unwrap().pos,km_per_px:AU/100.0};
            draw_contact(&painter,&cam,rect,&view,&c,CONTACT,true,&mut Labels::default());
        });
        output.textures_delta.clear();
        assert!(output.shapes.iter().any(|s|matches!(&s.shape,Shape::Path(p) if p.points.len()==4 && p.fill==CONTACT)),"resolved ship must remain a filled red chevron while pinging");
    }

    #[test]
    fn missile_chance_uses_received_uncertainty_and_endurance() {
        let app=LuminalApp::new();
        let view=app.session.view(Role::Faction(ESCORT));
        let ship=view.bodies.iter().find(|b|b.controllable).unwrap();
        let mut contact=view.contacts[0].clone();
        assert!(missile_solution_launchable(Some(&contact),Payload::Nuclear,0.0),"bearing-only LRM search launches are permitted");
        assert!(!missile_solution_launchable(None,Payload::Nuclear,1.0));
        assert!(!missile_solution_launchable(Some(&contact),Payload::Kinetic,0.0));
        contact.track=Some(test_track());
        contact.detection=luminal_core::sensors::DetectionLevel::Resolved;
        assert_eq!(missile_hit_estimate(ship,None,Payload::Nuclear),0.0);
        let tr=contact.track.as_mut().unwrap();
        tr.pos=ship.pos+Vec2::new(0.01*AU,0.0);
        tr.cov=[[1.0,0.0],[0.0,1.0]];tr.velocity_sigma=0.1;
        let good=missile_hit_estimate(ship,Some(&contact),Payload::Kinetic);
        assert!(good>0.5);
        for range in [0.07,0.14] {
            contact.track.as_mut().unwrap().pos=ship.pos+Vec2::new(range*AU,0.0);
            let chance=missile_hit_estimate(ship,Some(&contact),Payload::Kinetic);
            assert!(missile_solution_launchable(Some(&contact),Payload::Kinetic,chance),"SRM enabled at {range} AU");
            assert!(tactical_shortcut(egui::Key::S,ship,Some(&contact)).is_some());
        }
        contact.track.as_mut().unwrap().pos=ship.pos+Vec2::new(0.141*AU,0.0);
        assert!(missile_hit_estimate(ship,Some(&contact),Payload::Kinetic)>0.0,"nominal range is not a hard wall");
        contact.track.as_mut().unwrap().cov=[[AU*AU,0.0],[0.0,AU*AU]];
        assert!(missile_hit_estimate(ship,Some(&contact),Payload::Kinetic)<0.01);
        contact.track.as_mut().unwrap().pos=ship.pos+Vec2::new(5.0*AU,0.0);
        assert_eq!(missile_hit_estimate(ship,Some(&contact),Payload::Kinetic),0.0);
    }

    #[test]
    fn lrm_readout_rejects_a_target_beyond_reactor_lifetime() {
        let app=LuminalApp::new();let view=app.session.view(app.role);
        let ship=view.bodies.iter().find(|b|b.controllable).unwrap();
        let mut target=view.contacts[0].clone();let mut track=test_track();
        track.pos=ship.pos+Vec2::new(AU,0.0);track.vel=ship.vel+Vec2::new(40_000.0,0.0);
        target.track=Some(track);target.detection=sensors::DetectionLevel::Resolved;
        assert_eq!(missile_hit_estimate(ship,Some(&target),Payload::Nuclear),0.0);
    }

    #[test] fn weapon_rings_follow_magazine_depletion_independently() {
        let app=LuminalApp::new();let view=app.session.view(Role::Faction(ESCORT));
        let mut ship=view.bodies.iter().find(|b|b.controllable).unwrap().clone();
        assert_eq!(weapon_ranges(&ship).len(),3);
        ship.magazine[Payload::Nuclear.index()]=0;
        assert_eq!(weapon_ranges(&ship).iter().map(|r|r.0).collect::<Vec<_>>(),vec!["SRM","BEAM"]);
        ship.magazine[Payload::Kinetic.index()]=0;
        assert_eq!(weapon_ranges(&ship).iter().map(|r|r.0).collect::<Vec<_>>(),vec!["BEAM"]);
        ship.magazine[Payload::Nuclear.index()]=1;
        ship.missile_queued[Payload::Nuclear.index()]=1;
        let ranges=weapon_ranges(&ship);
        assert_eq!(ranges[0].0,"LRM");assert_eq!(ranges[0].1,Payload::Nuclear.engagement_range());
        assert_eq!(ranges[1].1,params::SHIP_BEAM_AUTO_RANGE_LS.value*LIGHT_SECOND);
    }

    #[test] fn heat_gauge_shows_sub_gigawatt_net_rates() {
        assert_eq!(heat_gauge_fill(0.0),0.0);
        assert!(heat_gauge_fill(230e6)>0.25);
        assert!(heat_gauge_fill(1.0)>=1.0/28.0);
        assert_eq!(heat_gauge_fill(1e15),1.0);
    }
    #[test] fn any_arrow_cancels_navigation_but_keeps_weapon_target() {
        for key in 0..4 {
            let mut app=LuminalApp::new();let target=app.inspected;
            let mut pressed=[false;4];pressed[key]=true;
            app.free_flight_input(pressed,pressed,false,0.016);
            let view=app.session.view(Role::Faction(ESCORT));
            let ship=view.bodies.iter().find(|b|b.controllable).unwrap();
            assert!(ship.autopilot.is_none());assert!(app.inspected==target);
            assert!(app.manual_flight.is_some());
        }
    }
    #[test]
    fn jump_button_map_selection_spool_and_cancel_work_in_headless_ui() {
        fn frame(app:&mut LuminalApp,ctx:&egui::Context,map:bool,events:Vec<egui::Event>)->egui::FullOutput {
            let view=app.session.view(app.role);
            let mut output=ctx.run_ui(egui::RawInput {screen_rect:Some(Rect::from_min_size(Pos2::ZERO,EVec2::new(900.0,700.0))),events,..Default::default()},|ui| {
                if map {app.map(ui,&view,None);} else {
                    let ship=view.bodies.iter().find(|b|b.controllable).unwrap();ui.columns(2,|cols| {app.central_controls(&mut cols[0],Some(ship),&view);app.movement_panel(&mut cols[1],&view,ship);});
                }
            });
            output.textures_delta.clear();output
        }
        fn text_position(output:&egui::FullOutput,label:&str)->Option<Pos2> {
            output.shapes.iter().find_map(|s|match &s.shape {Shape::Text(t) if t.galley.text()==label=>Some(t.pos+EVec2::new(5.0,5.0)),_=>None})
        }
        fn click(app:&mut LuminalApp,ctx:&egui::Context,map:bool,pos:Pos2) {
            for pressed in [true,false] {frame(app,ctx,map,vec![egui::Event::PointerMoved(pos),egui::Event::PointerButton {pos,button:egui::PointerButton::Primary,pressed,modifiers:Default::default()}]);}
        }
        use luminal_core::world::ShipClass;
        for class in [ShipClass::Picket,ShipClass::Frigate] {
            let mut app=LuminalApp::new_with_theme(class,theme::Theme::Luminal);let ctx=egui::Context::default();
            assert!(text_position(&frame(&mut app,&ctx,false,vec![]),"JUMP DRIVE").is_none());
        }
        let mut app=LuminalApp::new_with_theme(ShipClass::Destroyer,theme::Theme::Luminal);let ctx=egui::Context::default();
        let output=frame(&mut app,&ctx,false,vec![]);let button=text_position(&output,"JUMP DRIVE").unwrap();
        click(&mut app,&ctx,false,button);assert_eq!(app.jump_select,Some(BodyId(1)));
        app.fit_pending=false;app.track_player=false;app.camera=Camera {center:Vec2::new(2.0*AU,0.0),km_per_px:AU/100.0};
        frame(&mut app,&ctx,true,vec![]);
        let hover=frame(&mut app,&ctx,true,vec![egui::Event::PointerMoved(Pos2::new(500.0,350.0))]);
        assert_eq!(hover.platform_output.cursor_icon,egui::CursorIcon::Crosshair);
        click(&mut app,&ctx,true,Pos2::new(500.0,350.0));
        assert!(app.jump_select.is_none());
        for key in 0..4 {let mut pressed=[false;4];pressed[key]=true;app.free_flight_input(pressed,pressed,false,0.1);}
        let view=app.session.view(app.role);let ship=view.bodies.iter().find(|b|b.controllable).unwrap();
        assert!(matches!(ship.jump,Some(JumpState::Spooling {..})));assert_eq!(ship.thrust,Vec2::ZERO);assert!(app.manual_flight.is_none());
        let output=frame(&mut app,&ctx,false,vec![]);
        assert!(text_position(&output,"Spooling for jump").is_some());
        let cancel=text_position(&output,"CANCEL JUMP").unwrap();click(&mut app,&ctx,false,cancel);
        let view=app.session.view(app.role);let ship=view.bodies.iter().find(|b|b.controllable).unwrap();
        assert!(ship.jump.is_none());assert_eq!(ship.thermal.field,0.0);
        let output=frame(&mut app,&ctx,false,vec![]);
        click(&mut app,&ctx,false,text_position(&output,"REPAIR · AUTOMATIC").unwrap());
        let output=frame(&mut app,&ctx,false,vec![]);
        click(&mut app,&ctx,false,text_position(&output,"REPAIR · FIGHT").unwrap());
        assert_eq!(app.session.view(app.role).bodies.iter().find(|b|b.controllable).unwrap().damage.damage.repair_goal,luminal_core::damage::RepairGoal::Escape);
        let output=frame(&mut app,&ctx,false,vec![]);
        assert!(text_position(&output,"WITHDRAW (JUMP)").is_none());
        assert!(app.session.command(app.role,Command::Withdraw {body:BodyId(1)}).is_err());
        click(&mut app,&ctx,false,text_position(&output,"SURRENDER").unwrap());
        let output=frame(&mut app,&ctx,false,vec![]);
        click(&mut app,&ctx,false,text_position(&output,"Confirm surrender").unwrap());
        assert!(app.session.view(app.role).outcome.unwrap().reason.contains("surrendered"));
    }

    #[test]
    fn planet_name_sits_once_inside_its_orbit_with_star_distance() {
        assert_eq!(star_distance_label("Earth", AU), "Earth - 1 AU");
        assert_eq!(star_distance_label("Mercury", 0.3871 * AU), "Mercury - 0.39 AU");
        assert_eq!(star_distance_label("Jupiter", 5.2029 * AU), "Jupiter - 5.2 AU");
        assert_eq!(primary_distance_label("Moon", 384_400.0), "Moon - 1.3 LS");
        assert_eq!(primary_distance_label("Scourge", 44_000.0), "Scourge - 0.15 LS");
        fn frame(app:&mut LuminalApp,ctx:&egui::Context)->egui::FullOutput {
            let view=app.session.view(app.role);
            let mut output=ctx.run_ui(egui::RawInput {screen_rect:Some(Rect::from_min_size(Pos2::ZERO,EVec2::new(900.0,700.0))),..Default::default()},|ui| app.map(ui,&view,None));
            output.textures_delta.clear();output
        }
        fn walk<'a>(shape:&'a Shape,out:&mut Vec<&'a egui::epaint::TextShape>) {
            match shape {Shape::Text(t)=>out.push(t),Shape::Vec(v)=>for s in v {walk(s,out);},_=>{}}
        }
        fn texts(output:&egui::FullOutput)->Vec<&egui::epaint::TextShape> {
            let mut out=Vec::new();
            for clipped in &output.shapes {walk(&clipped.shape,&mut out);}
            out
        }
        fn glyph_anchor(t:&egui::epaint::TextShape)->Pos2 {
            let a0=egui::Align2::CENTER_CENTER.pos_in_rect(&t.galley.rect).to_vec2();
            let (s,c)=t.angle.sin_cos();
            t.pos+EVec2::new(c*a0.x-s*a0.y,s*a0.x+c*a0.y)
        }
        fn arc_label(output:&egui::FullOutput,center:Pos2,body:Pos2,min_r:f32,max_r:f32,near:f32,color:Color32)->String {
            let mut glyphs=Vec::new();
            for t in texts(output) {
                if t.fallback_color!=color || t.galley.text().chars().count()!=1 {continue;}
                let at=glyph_anchor(t);
                let rel=at-center;
                if rel.length()<min_r || rel.length()>max_r || at.distance(body)>near {continue;}
                glyphs.push((rel.y.atan2(rel.x),t.galley.text().to_string()));
            }
            let body_angle=(body-center).y.atan2((body-center).x);
            let delta=|angle:f32| {let mut d=angle-body_angle;while d>std::f32::consts::PI {d-=std::f32::consts::TAU;}while d<=-std::f32::consts::PI {d+=std::f32::consts::TAU;}d};
            glyphs.sort_by(|a,b|delta(b.0).total_cmp(&delta(a.0)));
            glyphs.into_iter().map(|(_,s)|s).collect()
        }
        let label_color=|kind| {let c=celestial_color(kind);Color32::from_rgba_unmultiplied(c.r(),c.g(),c.b(),128)};
        let mut app=LuminalApp::new_with_theme(luminal_core::world::ShipClass::Frigate,theme::Theme::Luminal);
        app.fit_pending=false;app.track_player=false;app.opening_fit=false;
        let ctx=egui::Context::default();
        let rect=Rect::from_min_size(Pos2::ZERO,EVec2::new(900.0,700.0));
        let view=app.session.view(app.role);
        let earth=view.celestials[1].pos;
        let earth_orbit=match view.system.bodies[1].orbit {Orbit::Frozen {radius,..} | Orbit::Circular {radius,..}=>radius,_=>panic!("earth orbit")};
        app.camera=Camera {center:earth,km_per_px:earth_orbit/400.0};
        let output=frame(&mut app,&ctx);
        let sun_s=to_screen(&app.camera,rect,view.celestials[0].pos);
        let earth_s=to_screen(&app.camera,rect,earth);
        let joined=arc_label(&output,sun_s,earth_s,370.0,400.0,140.0,label_color(CelestialKind::Planet));
        assert_eq!(joined,"Earth - 1 AU");
        for forbidden in ["Earth","0.5 AU","1 AU","1.5 AU","2 AU"] {
            assert!(!texts(&output).iter().any(|t|t.galley.text()==forbidden),"{forbidden} still painted");
        }
        let moon=view.celestials[2].pos;
        let (moon_orbit,parent)=match view.system.bodies[2].orbit {Orbit::Frozen {radius,parent,..} | Orbit::Circular {radius,parent,..}=>(radius,parent),_=>panic!("moon orbit")};
        assert_eq!(parent,1);
        app.camera=Camera {center:moon,km_per_px:moon_orbit/280.0};
        let output=frame(&mut app,&ctx);
        let earth_s=to_screen(&app.camera,rect,view.celestials[parent].pos);
        let moon_s=to_screen(&app.camera,rect,moon);
        let expected=primary_distance_label("Moon",(moon-view.celestials[parent].pos).length());
        let joined=arc_label(&output,earth_s,moon_s,250.0,278.0,140.0,label_color(CelestialKind::Moon));
        assert_eq!(joined,expected);
        assert!(!texts(&output).iter().any(|t|t.galley.text()=="Moon"));
        app.camera=Camera {center:earth,km_per_px:earth_orbit/24.0};
        let output=frame(&mut app,&ctx);
        let straight=texts(&output).iter().filter(|t|t.galley.text()=="Earth - 1 AU").count();
        assert_eq!(straight,1);
        let sun_s=to_screen(&app.camera,rect,view.celestials[0].pos);
        assert!(arc_label(&output,sun_s,to_screen(&app.camera,rect,earth),4.0,20.0,80.0,label_color(CelestialKind::Planet)).is_empty());
    }

    #[test]
    fn point_grid_is_one_au_and_switches_to_one_light_second() {
        assert_eq!(grid_spacing(AU / 80.0), Some(AU));
        assert_eq!(grid_spacing(AU / 400.0), Some(AU));
        assert_eq!(grid_spacing(LIGHT_SECOND / 40.0), Some(LIGHT_SECOND));
        assert_eq!(grid_spacing(AU / 4.0), None, "dots closer than the minimum pitch would clot");
        fn frame(app:&mut LuminalApp,ctx:&egui::Context)->egui::FullOutput {
            let view=app.session.view(app.role);
            let mut output=ctx.run_ui(egui::RawInput {screen_rect:Some(Rect::from_min_size(Pos2::ZERO,EVec2::new(900.0,700.0))),..Default::default()},|ui| app.map(ui,&view,None));
            output.textures_delta.clear();output
        }
        fn points(output:&egui::FullOutput)->Vec<Pos2> {
            let mut found=Vec::new();
            fn walk(shape:&Shape,found:&mut Vec<Pos2>) {
                match shape {
                    Shape::Mesh(mesh)=>{
                        let v=&mesh.vertices;
                        let mut i=0;
                        while i+3<v.len() {
                            if v[i].color==GRID_DOT && v[i+1].color==GRID_DOT && v[i+2].color==GRID_DOT && v[i+3].color==GRID_DOT {
                                found.push(Pos2::new((v[i].pos.x+v[i+1].pos.x+v[i+2].pos.x+v[i+3].pos.x)*0.25,(v[i].pos.y+v[i+1].pos.y+v[i+2].pos.y+v[i+3].pos.y)*0.25));
                                i+=4;
                            } else {i+=1;}
                        }
                    }
                    Shape::Vec(list)=>for shape in list {walk(shape,found);},
                    _=>{}
                }
            }
            for clipped in &output.shapes {walk(&clipped.shape,&mut found);}
            found
        }
        fn near(points:&[Pos2],at:Pos2)->bool {points.iter().any(|p|p.distance(at)<1.5)}
        let mut app=LuminalApp::new_with_theme(luminal_core::world::ShipClass::Frigate,theme::Theme::Luminal);
        app.fit_pending=false;app.track_player=false;app.opening_fit=false;
        let ctx=egui::Context::default();
        let rect=Rect::from_min_size(Pos2::ZERO,EVec2::new(900.0,700.0));
        app.camera=Camera {center:Vec2::new(0.3*AU,-0.4*AU),km_per_px:AU/100.0};
        let output=frame(&mut app,&ctx);
        let dots=points(&output);
        let origin=to_screen(&app.camera,rect,Vec2::ZERO);
        assert!(near(&dots,origin),"the star sits on an AU point");
        assert!(near(&dots,to_screen(&app.camera,rect,Vec2::new(AU,0.0))));
        assert!(near(&dots,to_screen(&app.camera,rect,Vec2::new(0.0,AU))));
        assert!(!near(&dots,to_screen(&app.camera,rect,Vec2::new(0.5*AU,0.0))),"half an AU is not a grid point");
        assert_eq!(dots.len(),63);
        app.camera=Camera {center:Vec2::ZERO,km_per_px:LIGHT_SECOND/40.0};
        let output=frame(&mut app,&ctx);
        let dots=points(&output);
        assert!(near(&dots,rect.center()));
        assert!(near(&dots,rect.center()+EVec2::new(40.0,0.0)));
        assert!(!near(&dots,rect.center()+EVec2::new(20.0,0.0)));
        assert_eq!(dots.len(),23*17);
    }

    #[test] fn manual_turns_and_throttle_are_bounded_and_frame_independent() {
        let initial=ManualFlight {body:BodyId(1),angle:0.0,throttle:0.0};
        let mut one=initial;let mut many=initial;
        one.adjust([true,false,true,false],[false;4],1.0);
        for _ in 0..100 {many.adjust([true,false,true,false],[false;4],0.01);}
        assert!((one.angle-many.angle).abs()<1e-10);assert!((one.throttle-many.throttle).abs()<1e-10);
        assert!(one.direction().y>0.99);assert_eq!(one.throttle,0.5);
        one.adjust([false,true,true,false],[false;4],10.0);assert_eq!(one.throttle,1.0);
        one.adjust([false,false,false,true],[false;4],10.0);assert_eq!(one.throttle,0.0);
        let angle=one.angle;one.adjust([false,true,false,false],[false;4],0.1);
        assert_ne!(one.angle,angle);assert_eq!(one.throttle,0.0,"turning at idle must not light the drive");
    }

    #[test]
    fn power_outage_chips_preserve_backup_and_crew_conditions() {
        let mut report=Report {damage:Default::default(),installed:[true;System::COUNT],observed_at:0.0,screen_available:1.0};
        report.damage.systems[System::Power as usize]=Condition::Damaged;
        for system in System::ALL {
            let chip=Chip::of(Some(report),system);
            assert!(chip==if system==System::Power {Chip::PowerOffline}
                else if system.independent_power() || system==System::Screens {Chip::Intact} else {Chip::Inoperative},"{}",system.code());
        }
        report.damage.systems[System::Passive as usize]=Condition::Damaged;
        assert!(Chip::of(Some(report),System::Passive)==Chip::Damaged);
        report.damage.systems[System::Beam as usize]=Condition::Destroyed;
        assert!(Chip::of(Some(report),System::Beam)==Chip::Inoperative);
        report.damage.systems[System::Power as usize]=Condition::Intact;
        assert!(Chip::of(Some(report),System::Beam)==Chip::Destroyed);
        assert!(Chip::of(Some(report),System::Active)==Chip::Intact);
    }

    #[test]
    fn dead_mind_greys_dependent_systems_and_lifeless_hulk_greys_every_chip() {
        let mut report=Report {damage:Default::default(),installed:[true;System::COUNT],observed_at:0.0,screen_available:1.0};
        report.damage.systems[System::Mind as usize]=Condition::Destroyed;
        for system in [System::Repair,System::Ecm,System::Eccm,System::Propulsion,System::Beam,System::Active] {
            assert!(Chip::of(Some(report),system)==Chip::Inoperative,"{}",system.code());
        }
        assert!(Chip::of(Some(report),System::Passive)==Chip::Intact);
        report.damage.systems[System::Crew as usize]=Condition::Destroyed;
        report.installed[System::SrmLauncher as usize]=false;
        for system in System::ALL {assert!(Chip::of(Some(report),system)==Chip::Lifeless);}
    }

    #[test]
    fn targeting_is_sticky_and_never_replaces_movement() {
        let mut app=LuminalApp::new();let view=app.session.view(Role::Faction(ESCORT));
        let ship=view.bodies.iter().find(|b|b.id==BodyId(1)).unwrap();
        let before=ship.autopilot;
        let target=Selection::Contact(view.contacts[0].id);
        app.select_object(target,&view);
        assert!(app.session.view(Role::Faction(ESCORT)).bodies.iter().find(|b|b.id==ship.id).unwrap().autopilot==before);
        let mut missing=view.clone();missing.contacts.clear();app.acquire_first_target(&missing);
        assert!(app.inspected==Some(target));
        app.move_to_ship(ship,InterceptTarget::Own(BodyId(0)));
        assert!(app.inspected==Some(target));
        assert!(app.movement_mode==MovementMode::Alongside);
        app.select_object(Selection::Body(BodyId(0)),&view);
        app.acquire_first_target(&view);
        assert!(app.inspected==Some(Selection::Body(BodyId(0))));
    }

    #[test]
    fn inspecting_targets_and_automatic_ships_preserves_command_selection() {
        let mut app = LuminalApp::new();
        let view = app.session.view(Role::Faction(ESCORT));
        app.select_object(Selection::Contact(ContactId(1)), &view);
        assert!(app.selected == Some(Selection::Body(BodyId(1))));
        app.select_object(Selection::Body(BodyId(0)), &view);
        assert!(app.selected == Some(Selection::Body(BodyId(1))));
        let truth = app.session.view(Role::Spectator);
        app.select_object(Selection::Body(BodyId(2)), &truth);
        assert!(app.selected == Some(Selection::Body(BodyId(1))));
        app.select_object(Selection::Body(BodyId(1)), &view);
        assert!(app.inspected==Some(Selection::Body(BodyId(1))));
    }
}
/// How far ahead to forecast committed motion, seconds.
const FORECAST_S: f64 = 8.0 * 3600.0;

/// Velocity tail length, px per km/s of Sun-frame speed, and its cap.
const TAIL_PX_PER_KMS: f64 = 0.01;
const TAIL_MAX_PX: f64 = 2.4;
/// A bearing-only detection fades out over this much game time unless refreshed, seconds.
const BEARING_FADE_S: f64 = 300.0;

const BACKGROUND: Color32 = Color32::from_rgb(6, 8, 14);
/// Relative to the viewer: our combatants, our non-combatants, enemies (and every
/// sensed contact), and neutral or unidentified.
const FRIEND: Color32 = Color32::from_rgb(100, 220, 120);
const NONCOMBAT: Color32 = Color32::from_rgb(90, 170, 255);
const ENEMY: Color32 = Color32::from_rgb(255, 80, 80);
#[allow(dead_code)]
const UNKNOWN: Color32 = Color32::from_rgb(240, 210, 80);
const CONTACT: Color32 = ENEMY;
const BELIEF: Color32 = Color32::from_rgb(255, 220, 90);
const DANGER: Color32 = Color32::from_rgb(255, 70, 70);

#[derive(Clone, Copy)]
struct Camera {
    /// World km at the screen centre.
    center: Vec2,
    km_per_px: f64,
}

#[derive(Clone, Copy, PartialEq)]
enum Selection {
    Body(BodyId),
    Contact(ContactId),
}

#[derive(Clone,Copy)]
struct ManualFlight {body:BodyId,angle:f64,throttle:f64}
impl ManualFlight {
    fn direction(self)->Vec2 {Vec2::new(self.angle.cos(),self.angle.sin())}
    fn adjust(&mut self,held:[bool;4],pressed:[bool;4],dt:f64) {
        // Input is wall-clock based, independent of simulation warp. A tap
        // gives 3 degrees / 5% throttle; holding gives 90 degrees / 50% per second.
        let amount=|i:usize,tap:f64,rate:f64|if pressed[i] {tap} else if held[i] {rate*dt} else {0.0};
        self.angle=wrap_angle(self.angle+amount(0,3f64.to_radians(),std::f64::consts::FRAC_PI_2)-amount(1,3f64.to_radians(),std::f64::consts::FRAC_PI_2));
        self.throttle=(self.throttle+amount(2,0.05,0.5)-amount(3,0.05,0.5)).clamp(0.0,1.0);
    }
}

#[derive(Clone,Copy,PartialEq)]
enum MovementMode {Alongside,Flyby,Standoff,Close}
impl MovementMode {
    fn label(self)->&'static str {match self {Self::Alongside=>"ALONGSIDE",Self::Flyby=>"FLYBY",Self::Standoff=>"STANDOFF",Self::Close=>"CLOSE"}}
    fn help(self)->&'static str {match self {Self::Alongside=>"Escort and screen threats; otherwise match alongside",Self::Flyby=>"Maximum thrust toward the ship; no braking",Self::Standoff=>"Hold outside SRM range with LRMs; otherwise outside beam range",Self::Close=>"Brake into beam range and match motion"}}
    fn command(self,body:BodyId,target:InterceptTarget)->Command {match self {
        Self::Alongside=>Command::Alongside {body,target},Self::Flyby=>Command::Flyby {body,target},
        Self::Standoff|Self::Close=>Command::CombatRange {body,target,standoff:self==Self::Standoff},
    }}
    fn of(order:Order)->Option<Self> {match order {
        Order::Follow {..}|Order::Alongside {..}=>Some(Self::Alongside),Order::Flyby(_)=>Some(Self::Flyby),
        Order::CombatRange(_,true)=>Some(Self::Standoff),Order::CombatRange(_,false)|Order::Intercept(_)=>Some(Self::Close),_=>None,
    }}
}
fn movement_target(order:Order)->Option<InterceptTarget> {match order {
    Order::Follow {target,..}=>Some(InterceptTarget::Own(target)),Order::Alongside {target,..}=>Some(InterceptTarget::Contact(target)),
    Order::Intercept(t)|Order::Flyby(t)|Order::KeepRange(t,_)|Order::CombatRange(t,_)|Order::Evade(t)=>Some(t),_=>None,
}}

struct LuminalApp {
    manual_flight:Option<ManualFlight>,
    manual_send_elapsed:f64,
    audio:audio::Audio,
    auto_speed:bool,
    auto_speed_elapsed:f64,
    session: LocalSession,
    role: Role,
    /// In spectator mode, whose picture to overlay on truth.
    overlay: Option<FactionId>,
    camera: Camera,
    track_player: bool,
    tracking_zoom_hold:f64,
    manual_ping_zoom_until:f64,
    /// Frame the scene on the next map draw.
    fit_pending: bool,
    opening_fit: bool,
    selected: Option<Selection>,
    inspected: Option<Selection>,
    movement_mode: MovementMode,
    jump_select:Option<BodyId>,
    target_chosen: bool,
    last_message: Option<String>,
    payload: Payload,
    dev: DevHooks,
    /// Displayed bearing per (contact, sensor): a running average of the noisy
    /// measurements, and the emission time of the last one folded in.
    bearing_display: BTreeMap<(ContactId, BodyId), (f64, f64)>,
    ship_headings: BTreeMap<BodyId, Vec2>,
    selection_pending:bool,
    chosen_class:luminal_core::world::ShipClass,
    chosen_scenario:Scenario,
    theme:theme::Theme,
    ship_art:ship_art::ShipArt,
    tactical_log:TacticalLog,
    weapon_effects:weapon_effects::WeaponEffects,
    jump_effects:jump_effects::JumpEffects,
    celestial_art:celestial_art::CelestialArt,
}

/// Development hooks driven by environment variables, used for visual checks.
/// `LUMINAL_ADVANCE=<sim seconds>` pre-runs the scenario; `LUMINAL_ROLE=spectator|raider`
/// picks the starting view; `LUMINAL_SCENARIO=escort|raid|hide|armada` picks the situation;
/// `LUMINAL_SCREENSHOT=<file.ppm>` saves a frame and exits.
#[derive(Default)]
struct DevHooks {
    screenshot: Option<std::path::PathBuf>,
    frames: u32,
}

impl LuminalApp {
    fn free_flight_input(&mut self,held:[bool;4],pressed:[bool;4],released:bool,dt:f64) {
        if !held.iter().any(|v|*v) && !pressed.iter().any(|v|*v) && !released {return;}
        if self.session.waiting_for_event() {self.command(Command::SetPaused(true));}
        let view=self.session.view(self.role);
        let Some(ship)=view.bodies.iter().find(|b|b.controllable && b.kind==BodyKind::Ship) else {return;};
        if ship.jump.is_some() {return;}
        let rated_g=ship.ship_class.map_or(params::SHIP_MAX_ACCEL_G.value,|class|class.max_g());
        let entering=self.manual_flight.is_none_or(|m|m.body!=ship.id) || ship.autopilot.is_some();
        if entering {
            let direction=self.ship_headings.get(&ship.id).copied().unwrap_or_else(||if ship.thrust.length()>0.0 {ship.thrust.normalized()} else if ship.vel.length()>0.0 {ship.vel.normalized()} else {Vec2::new(0.0,1.0)});
            let thrust=if ship.autopilot.is_some() {ship.thrust.length()} else {ship.commanded.length()};
            self.manual_flight=Some(ManualFlight {body:ship.id,angle:direction.y.atan2(direction.x),throttle:(thrust/(rated_g*G0)).clamp(0.0,1.0)});
        }
        let manual=self.manual_flight.as_mut().unwrap();manual.adjust(held,pressed,dt);
        self.ship_headings.insert(ship.id,manual.direction());
        self.manual_send_elapsed+=dt;
        if entering || pressed.iter().any(|v|*v) || released || self.manual_send_elapsed>=0.1 {
            let command=Command::SetThrust {body:ship.id,thrust:manual.direction()*(manual.throttle*rated_g*G0)};
            self.manual_send_elapsed=0.0;
            if let Err(error)=self.session.command(self.role,command) {self.last_message=Some(format!("Manual flight rejected: {error:?}"));}
            else if entering {self.tactical_log.push("helm".into(),"FREE FLIGHT · ARROW KEYS".into(),ACCENT,view.time);self.audio.play(audio::Cue::Click);}
        }
    }
    fn command_deck(&mut self,ui:&mut egui::Ui,view:&View) {
        let own=view.bodies.iter().find(|b|b.controllable && Some(b.faction)==self.own_faction());
        let target=match self.inspected {Some(Selection::Contact(id))=>view.contacts.iter().find(|c|c.id==id),_=>None};
        command_columns(ui,|columns| {
            for ui in columns.iter_mut().skip(1) {
                let rect=ui.available_rect_before_wrap();
                ui.painter().line_segment([rect.left_top()-EVec2::new(4.0,0.0),rect.left_bottom()-EVec2::new(4.0,0.0)],Stroke::new(1.0,EDGE));
            }
            let ui=&mut columns[0];
            sub_header(ui,&format!("FIRE CONTROL · {}",self.theme.name().to_uppercase()),Some(("PRE-DEFENCE",TEXT_MUTED)));
            if let Some(ship)=own {self.compact_weapons(ui,view,ship);}
            let ui=&mut columns[1];
            let mind_offline=format!("{} OFFLINE",self.theme.system(System::Mind).to_uppercase());
            sub_header(ui,"OWN SHIP",own.and_then(|b|if b.damage.damage.lifeless() {Some(("LIFELESS HULK",TEXT_MUTED))} else if b.damage.damage.state(System::Mind)==Condition::Destroyed {Some((mind_offline.as_str(),TEXT_MUTED))} else {None}));
            if let Some(b)=own {ui.label(egui::RichText::new(&b.name).strong().color(FRIEND));ui.small(b.display_class.as_deref().unwrap_or("Ship"));}
            compact_status(ui,own.map(|b|&b.damage),own.map(|b|b.thrust.length()/G0),own.and_then(|b|b.ship_class).map(|c|c.max_g()),false);
            compact_systems(ui,"own_deck",own.map(|b|b.damage),self.theme);
            if let Some(ship)=own {if ship.interference_remaining>0.0 {ui.small(egui::RichText::new(format!("FIRE CONTROL -15% · {:.0}s",ship.interference_remaining)).color(WARM));}}
            self.central_controls(&mut columns[2],own,view);
            let ui=&mut columns[3];
            let systems=target.and_then(|c|target_system_report(c,view));
            let label=if systems.is_some_and(|r|r.damage.lifeless()) {"LIFELESS HULK"} else if target.is_some_and(|c|c.damage.is_some()) && systems.is_none() {"STALE ECHO"} else {"LAST ECHO"};
            sub_header(ui,"TARGET",Some((label,CONTACT)));
            if let Some(c)=target {ui.label(egui::RichText::new(contact_label(c)).strong().color(CONTACT));ui.small(contact_class(c));}
            let thrust=target.filter(|c|contact_has_course(c)).and_then(|c|c.track.as_ref()).map(|t|t.accel.length()/G0)
                .filter(|_|systems.is_some_and(|r|r.operating_effectiveness(System::Propulsion)>0.0));
            compact_status(ui,target.and_then(|c|c.damage.as_ref()),thrust,target.and_then(|c|c.resolved_class).map(|c|c.max_g()),true);
            compact_systems(ui,"target_deck",systems,self.theme);
            let ui=&mut columns[4];
            if let Some(ship)=own {self.movement_panel(ui,view,ship);}
        });
    }

    fn repair_controls(&mut self,ui:&mut egui::Ui,ship:&BodyView) {
        if let Some(target)=ship.damage.damage.repair_target {
            let rate=ship.damage.damage.system_repair_rate();
            if rate>0.0 {ui.small(format!("Repairing {} · {}",self.theme.system(target),fmt_time((luminal_core::damage::SYSTEM_REPAIR_SECONDS-ship.damage.damage.repair_progress).max(0.0)/rate)));}
            else {ui.small("Repairs unavailable");}
        }
    }
    fn movement_panel(&mut self,ui:&mut egui::Ui,view:&View,ship:&BodyView) {
        if ship.jump.is_none() {
            ui.horizontal(|ui| {
                ui.menu_button("SURRENDER",|ui| {
                    ui.label("Concede this battle and remove your ship from combat.");
                    if ui.button("Confirm surrender").clicked() {self.command(Command::Surrender {body:ship.id});ui.close();}
                });
            });
        }
        if let Some(jump)=ship.jump {
            sub_header(ui,&format!("HELM / {}",self.theme.jump()),None);
            match jump {
                JumpState::Spooling {depart_at,..}=>{
                    ui.label(egui::RichText::new(if ship.withdrawing {"Withdrawing — spooling for jump"} else {"Spooling for jump"}).strong().color(ACCENT));
                    ui.label(format!("{} remaining",fmt_time((depart_at-view.time).max(0.0))));
                    ui.small("Thrust, evasion, screens, beams and PD lasers offline.");
                    if ui.button(if self.theme==theme::Theme::Luminal {"CANCEL JUMP".into()} else {format!("CANCEL {}",self.theme.jump())}).clicked() {self.command(Command::CancelJump {body:ship.id});}
                    ui.small("Cancel: screens recharge from zero.");
                }
                JumpState::Transit {arrive_at,..}=>{
                    ui.label(egui::RichText::new("JUMP IN TRANSIT").strong().color(ACCENT));
                    ui.label(format!("Arrival in {:.1}s · 1 AU/s",(arrive_at-view.time).max(0.0)));
                }
            }
            return;
        }
        if ship.ship_class.is_some_and(|c|c.has_jump_drive()) {
            let selecting=self.jump_select==Some(ship.id);
            let recovery=(ship.jump_ready_at-view.time).max(0.0);
            sub_header(ui,&format!("NAVIGATION / {}",self.theme.jump()),None);
            let label=if selecting {"CANCEL DESTINATION".into()} else if recovery>0.0 {format!("RECOVERING · {}",fmt_time(recovery))} else {self.theme.jump().to_string()};
            if tac_button(ui,&label,EVec2::new(ui.available_width(),40.0),ACCENT,selecting,
                recovery<=0.0 && ship.damage.operating_effectiveness(System::Jump)>0.0)
                .on_hover_text("Choose a point within 50 AU of the central star. One-hour spool, then 1 AU/s; preserves velocity. Six-hour drive recovery after arrival. Thrust, screens and lasers are offline while spooling.").clicked() {
                self.jump_select=if selecting {None} else {Some(ship.id)};
                self.manual_flight=None;
            }
            ui.label(egui::RichText::new("1 H SPOOL  /  6 H RECOVERY  /  50 AU").monospace().size(9.0).color(TEXT_MUTED));
            if selecting {ui.small("Left-click destination · Esc / right-click cancels");}
        }
        let order=ship.autopilot.map(|a|a.order);
        let active=order.and_then(MovementMode::of);
        let target=order.and_then(movement_target);
        sub_header(ui,"HELM / MOVEMENT",Some(("RIGHT CLICK",ACCENT)));
        let name=|t:InterceptTarget|match t {
            InterceptTarget::Own(id)=>view.bodies.iter().find(|b|b.id==id).map_or("Unavailable ship".into(),|b|b.name.clone()),
            InterceptTarget::Contact(id)=>view.contacts.iter().find(|c|c.id==id).map_or("Lost contact".into(),contact_label),
        };
        let title=target.map(name).unwrap_or_else(||match order {
            Some(Order::Route)=>"WAYPOINT ROUTE".into(),Some(Order::MoveTo {..})=>"MAP DESTINATION".into(),
            Some(Order::Orbit {celestial,..})=>format!("ORBIT · {}",view.celestials[celestial].name),
            _=>if ship.thrust.length()>0.001 {"MANUAL THRUST".into()} else {"COASTING".into()},
        });
        egui::Frame::new().fill(Color32::from_rgb(12,23,35)).inner_margin(8.0).show(ui,|ui| {
            ui.set_width(ui.available_width());
            ui.label(egui::RichText::new(title).monospace().strong().size(14.0).color(TEXT_HI));
            let status=if ship.controls.evading {"AUTO EVADE · RESUMES ORDER".into()} else {ship.autopilot.map_or("Right-click a ship or destination".into(),|ap|match ap.status {
                AutopilotStatus::Closing {eta,..}=>format!("CLOSING · ETA {}",fmt_time(eta)),AutopilotStatus::Holding=>"ON STATION · MATCHING".into(),
                AutopilotStatus::NoTrack=>"TRACK LOST · COASTING".into(),AutopilotStatus::Passed=>"COMPLETE · COASTING".into(),AutopilotStatus::Manoeuvring=>"MANOEUVRING".into(),
            })};
            ui.label(egui::RichText::new(status).monospace().size(9.0).color(ACCENT));
            let pos=target.and_then(|t|match t {InterceptTarget::Own(id)=>view.bodies.iter().find(|b|b.id==id).map(|b|b.pos),InterceptTarget::Contact(id)=>view.contacts.iter().find(|c|c.id==id).and_then(|c|c.track.as_ref()).map(|t|t.pos)})
                .or_else(||match order {Some(Order::MoveTo {frame,offset})=>Some(view.celestials[frame].pos+offset),_=>None});
            if let Some(pos)=pos {ui.label(egui::RichText::new(format!("{}  SEPARATION",fmt_distance((pos-ship.pos).length()))).monospace().size(11.0).color(TEXT_MUTED));}
            if let Some(route)=ship.route.as_ref().filter(|_|matches!(order,Some(Order::Route))) {ui.small(format!("{} waypoints remaining",route.points.len().saturating_sub(route.progress.floor() as usize+1)));}
        });
        ui.add_space(5.0);
        let modes=[MovementMode::Alongside,MovementMode::Flyby,MovementMode::Standoff,MovementMode::Close];
        for pair in modes.chunks(2) {ui.horizontal(|ui| {
            let width=(ui.available_width()-ui.spacing().item_spacing.x)/2.0;
            for &mode in pair {
                let selected=active==Some(mode);
                if tac_button(ui,&self.theme.range(mode),EVec2::new(width,27.0),ACCENT,selected,ship.damage.operating_effectiveness(System::Propulsion)>0.0).on_hover_text(mode.help()).clicked() {
                    self.movement_mode=mode;
                    if let Some(target)=target {self.command(mode.command(ship.id,target));}
                }
            }
            if pair.len()==1 && tac_button(ui,"COAST",EVec2::new(width,27.0),ACCENT,order.is_none() && ship.thrust.length()<0.001,true).clicked() {self.command(Command::SetThrust {body:ship.id,thrust:Vec2::ZERO});}
        });}
        ui.add_space(4.0);
        ui.label(egui::RichText::new(active.unwrap_or(self.movement_mode).help()).size(10.0).color(TEXT_MUTED));
        ui.label(egui::RichText::new("Left click: target   Right click: move
Shift + right click: extend route").monospace().size(9.0).color(TEXT_MUTED));
    }

    fn move_to_ship(&mut self,ship:&BodyView,target:InterceptTarget) {
        let mode=if matches!(target,InterceptTarget::Own(_)) {MovementMode::Alongside}
            else {ship.autopilot.and_then(|a|MovementMode::of(a.order)).unwrap_or(self.movement_mode)};
        self.movement_mode=mode;
        self.command(mode.command(ship.id,target));
    }

    fn central_controls(&mut self,ui:&mut egui::Ui,ship:Option<&BodyView>,view:&View) {
        use luminal_core::world::controls::{ControlledSystem as C,Mode};
        let Some(ship)=ship else {return;};
        ui.spacing_mut().item_spacing=EVec2::new(2.0,1.0);
        let thermal=ship.thermal;
        let ef=ship.emissivity;
        let shaded=ship.thermal.heat_fraction()<0.25 && ship.thrust.length()<1e-9 && view.system.stellar_visibility(ship.pos,view.time,None)<=1e-6;
        let (r,_)=ui.allocate_exact_size(EVec2::new(ui.available_width(),157.0),Sense::hover());
        let cx=r.center().x;let center=Pos2::new(cx,r.top()+70.0);
        let radius=(r.width()*0.115).clamp(35.0,52.0);
        let cool=Color32::from_rgb(64,214,128);let hot=Color32::from_rgb(238,74,56);
        let net=thermal.net_heat_flow();let heating=net.max(0.0);let cooling=(-net).max(0.0);
        ui.painter().rect_filled(r.expand2(EVec2::new(5.0,0.0)),8.0,Color32::from_rgb(9,15,27));
        ui.painter().line_segment([r.left_top(),r.right_top()],Stroke::new(2.0,ACCENT.gamma_multiply(0.45)));
        for (rate,color,start,end) in [(cooling,cool,0.0_f32,-160.0_f32),(heating,hot,0.0,160.0)] {
            let fill=heat_gauge_fill(rate);
            for i in 0..28 {
                let angle=|u:f32| {let a=(start+(end-start)*u).to_radians();center+EVec2::new(a.sin(),-a.cos())*radius};
                ui.painter().line_segment([angle(if i==0 {0.0} else {i as f32/28.0+0.006}),angle((i+1) as f32/28.0-0.006)],
                    Stroke::new(5.0,if (i as f64)<fill*28.0 {color} else {EDGE}));
            }
        }
        let energy=fmt_energy(thermal.heat_j);let (number,unit)=energy.split_once(' ').unwrap_or((&energy,"J"));
        let heat_color=if thermal.heat_fraction()>=1.0 {hot} else if thermal.heat_fraction()>=0.7 {WARM} else {Color32::from_rgb(246,232,208)};
        ui.painter().text(center-EVec2::new(0.0,28.0),egui::Align2::CENTER_CENTER,"HEAT",mono(8.0),TEXT_MUTED);
        ui.painter().text(center-EVec2::new(0.0,2.0),egui::Align2::CENTER_CENTER,number,mono(radius*0.65),heat_color);
        ui.painter().text(center+EVec2::new(0.0,24.0),egui::Align2::CENTER_CENTER,unit,mono(11.0),heat_color);
        let flank=radius+22.0;let size=(r.width()*0.047).clamp(14.0,24.0);
        for (x,align,label,value,color) in [(cx-flank,egui::Align2::RIGHT_CENTER,if shaded {"EF · SHADE ×0.5"} else {"EMISSIVITY"},format!("{:.2}×",ef.value()),ACCENT),
            (cx+flank,egui::Align2::LEFT_CENTER,self.theme.screens(),if ship.has_screen {format!("{:.0}%",ship.damage.screen_available*100.0)} else {"N/F".into()},ARMOUR)] {
            ui.painter().text(Pos2::new(x,r.top()+45.0),align,label,mono(8.0),TEXT_MUTED);
            ui.painter().text(Pos2::new(x,r.top()+72.0),align,value,mono(size),color);
        }
        let rate_label=|w:f64|fmt_energy(w).replace('J',"W");
        ui.painter().text(Pos2::new(cx-radius-10.0,r.top()+112.0),egui::Align2::RIGHT_CENTER,format!("−{}",rate_label(cooling)),mono(9.0),cool);
        ui.painter().text(Pos2::new(cx+radius+10.0,r.top()+112.0),egui::Align2::LEFT_CENTER,format!("+{}",rate_label(heating)),mono(9.0),hot);
        let status=if thermal.dumping {"RADIATORS OPEN · DRIVE / LASERS OFF"} else if thermal.thrust_factor()<1.0 {"THERMAL THRUST LIMIT"} else if heating>cooling {"HEAT RISING"} else if cooling>0.0 {"HEAT FALLING"} else {"HEAT BALANCED"};
        ui.painter().text(Pos2::new(cx,r.top()+12.0),egui::Align2::CENTER_CENTER,status,mono(8.0),if thermal.dumping {WARM} else {TEXT_MUTED});
        ui.interact(r,ui.id().with("heat_rates"),Sense::hover()).on_hover_text("Net heat flow only: left cooling, right heating. Heat input is averaged over five seconds. Gauge scale: 1 MW to 1 PW; smaller nonzero rates retain one visible segment. Cruising at up to 50% rated thrust with screens enabled is heat balanced. Higher burns, weapons and absorbed hits build heat. Enabled screens add a small heat load; absorbed hits heat the ship immediately.");
        let button=Rect::from_center_size(Pos2::new(cx,r.top()+140.0),EVec2::new(150.0_f32.min(r.width()),26.0));
        let mut child=ui.new_child(egui::UiBuilder::new().max_rect(button));
        if tac_button(&mut child,"PING  [P]",button.size(),ACCENT,false,ship.damage.operating_effectiveness(System::Active)>0.0)
            .on_hover_text("Resolve ships earlier, reveal cruising LRMs and improve missile accuracy. Echoes arrive at light speed.").clicked() {self.command(Command::Ping {body:ship.id});}
        ui.add_space(5.0);
        sub_header(ui,"SYSTEM MODES",None);
        let screen=if ship.damage.operating_effectiveness(System::Screens)<=0.0 {"DISABLED".into()}
            else if !ship.screen_up {"OFF".into()}
            else if ship.thermal.field+0.001<ship.damage.operating_effectiveness(System::Screens) {format!("CHARGING {:.0}%",ship.damage.screen_available*100.0)}
            else {format!("{:.0}% AVAILABLE",ship.damage.screen_available*100.0)};
        let controls=[
            ("ECM",C::Ecm,ship.controls.ecm,ship.controls.ecm_active,if ship.controls.ecm_active {"EMITTING".into()} else {"SILENT".into()},ship.damage.operating_effectiveness(System::Ecm)>0.0,"Auto emits while a resolved enemy ship is known. Class-rated ECM/ECCM; maximum 50% resolution reduction."),
            (self.theme.screens(),C::Screens,ship.controls.screens,ship.screen_up,screen,ship.damage.operating_effectiveness(System::Screens)>0.0,"Auto latches on after resolving an enemy ship. Charges 0.2% per minute. Off disables absorption immediately. Hits and idle operation heat the ship."),
            ("EVADE",C::Evade,ship.controls.evade,ship.controls.evading,if ship.jump.is_some() {"OFF: JUMP".into()} else if ship.controls.evading {"EVADING".into()} else if ship.controls.evade==Mode::Auto {"WATCHING".into()} else {"OFF".into()},ship.damage.operating_effectiveness(System::Propulsion)>0.0,"Auto temporarily evades incoming damaging missiles, then resumes your prior movement order. Off disables automatic evasion."),
            ("ACTIVE",C::Active,ship.controls.active,ship.controls.active==Mode::Auto,if ship.controls.active==Mode::Auto {format!("PING IN {:.0}s",(ship.controls.next_ping_at-view.time).max(0.0))} else {"SILENT".into()},ship.damage.operating_effectiveness(System::Active)>0.0,"Auto pings every 60 seconds until switched Off, even without contacts.")
        ];
        for row in [[Some(0),Some(2),Some(3)],[Some(1),None,Some(4)]] {ui.columns(3,|columns| {
            for (ui,index) in columns.iter_mut().zip(row) {
                let Some(index)=index else {
                    use luminal_core::damage::RepairGoal;
                    let goal=ship.damage.damage.repair_goal;
                    ui.add_space(7.0);
                    if tac_button(ui,&format!("REPAIR · {}",goal.label().to_uppercase()),EVec2::new(ui.available_width(),28.0),ACCENT,true,true)
                        .on_hover_text("Cycle repair priority: Automatic, Fight, Escape. Current repair progress is preserved.").clicked() {
                        let goal=match goal {RepairGoal::Automatic=>RepairGoal::Fight,RepairGoal::Fight=>RepairGoal::Escape,RepairGoal::Escape=>RepairGoal::Automatic};
                        self.command(Command::SetRepairGoal {body:ship.id,goal});
                    }
                    continue;
                };
                if index>=controls.len() {
                    ui.add_space(7.0);
                    if tac_button(ui,if thermal.dumping {"RADIATORS · OPEN"} else {"RADIATORS · CLOSED"},EVec2::new(ui.available_width(),28.0),WARM,thermal.dumping,!ship.damage.damage.lifeless())
                        .on_hover_text("Open radiators: 5× cooling, 10× heat signature; thrust and beams are disabled.").clicked() {self.command(Command::SetHeatDump {body:ship.id,enabled:!thermal.dumping});}
                    continue;
                }
                let (label,system,mode,active,ref status,enabled,help)=controls[index];
                let inhibited=ship.jump.is_some() && matches!(system,C::Evade|C::Screens);
                let enabled=enabled && !inhibited;
                let status=if inhibited {"OFF: JUMP"} else {status.as_str()};
                ui.add_space(7.0);
                let response=tac_button(ui,&format!("{label} · {}",mode.label()),EVec2::new(ui.available_width(),28.0),ACCENT,active,enabled).on_hover_text(help);
                if response.clicked() {
                    let next=if matches!(system,C::Active|C::Evade) {if mode==Mode::Auto {Mode::Off} else {Mode::Auto}} else {mode.next()};
                    self.command(Command::SetSystemMode {body:ship.id,system,mode:next});
                }
                ui.label(egui::RichText::new(status).monospace().size(7.5).color(if active {ACCENT} else {TEXT_MUTED}));
            }
        });}
        self.repair_controls(ui,ship);
    }


    fn compact_weapons(&mut self,ui:&mut egui::Ui,view:&View,b:&BodyView) {
        let target=match self.inspected {Some(Selection::Contact(id))=>view.contacts.iter().find(|c|c.id==id),_=>None};
        let area=ui.available_rect_before_wrap();
        let title=target.map_or_else(||"SELECT AN ENEMY · NO SOLUTION".into(),|c|format!("{} · {}{}",contact_label(c),c.quality.to_uppercase(),c.track.as_ref().map_or(String::new(),|t|format!(" · {}",fmt_distance((t.pos-b.pos).length())))));
        ui.painter().text(area.left_top(),egui::Align2::LEFT_TOP,title,mono(10.0),ACCENT);
        let weapons:Vec<_>=[Payload::Nuclear,Payload::Kinetic,Payload::Beam].into_iter().filter(|p|*p==Payload::Beam || b.magazine[p.index()]>0).collect();
        let footer=19.0;let top=area.top()+18.0;let height=((area.height()-18.0-footer-8.0)/weapons.len() as f32).min(110.0);
        for (row,p) in weapons.into_iter().enumerate() {
            let r=Rect::from_min_size(Pos2::new(area.left(),top+row as f32*(height+4.0)),EVec2::new(area.width(),height));
            ui.painter().rect_filled(r,3.0,Color32::from_rgb(12,22,33));
            let left=r.left()+7.0;let right=r.right()-7.0;
            let class=b.ship_class.unwrap_or(luminal_core::world::ShipClass::Frigate);
            let label=if p==Payload::Beam && b.beam_mode==luminal_core::world::weapon_fit::BeamMode::Interference {self.theme.projector()} else {self.theme.fitted_weapon(p,class)};
            ui.painter().text(Pos2::new(left,r.top()+5.0),egui::Align2::LEFT_TOP,label,mono(12.0),TEXT);
            if p!=Payload::Beam {
                let chance=missile_hit_estimate(b,target,p);
                let tracked=target.is_some_and(|c|c.track.is_some() && !c.stale);
                let count=b.magazine[p.index()].saturating_sub(b.missile_queued[p.index()]);
                let mounts=b.ship_class.unwrap_or(luminal_core::world::ShipClass::Frigate).launchers(p);
                let mounts=if b.damage.operating_effectiveness(p.launcher_system())<1.0 {mounts.div_ceil(2)} else {mounts};
                let volley=mounts.min(count);
                let reload=(b.missile_ready_at[p.index()]-view.time).max(0.0);
                let ready=count>0 && mounts>0 && missile_solution_launchable(target,p,chance) && b.damage.operating_effectiveness(p.launcher_system())>0.0;
                let status=if mounts==0 {"NOT FITTED".into()} else if b.damage.operating_effectiveness(p.launcher_system())<=0.0 {"DISABLED".into()}
                    else if count==0 {"EMPTY".into()} else if target.is_none() {"NO TARGET".into()}
                    else if p==Payload::Kinetic && !target.is_some_and(|c|!c.stale && c.detection>=sensors::DetectionLevel::Resolved) {"NEEDS RESOLUTION".into()}
                    else if tracked && chance<=0.0 {"OUT OF RANGE".into()} else if reload>0.0 {format!("RELOAD {reload:.0}s")} else {"READY".into()};
                let score=if tracked {format!("{}HIT ≈{:.0}%",if target.is_some_and(|c|c.active_fire_control>0.0) {"PING · "} else {""},chance*100.0)} else {"HIT UNKNOWN".into()};
                ui.painter().text(Pos2::new(right,r.top()+5.0),egui::Align2::RIGHT_TOP,score,mono(12.0),if chance>=0.5 {SYS_OK} else {WARM});
                let flight=target.and_then(|c|c.track.as_ref()).map(|t| {
                    let rel=t.pos-b.pos;let closing=(b.vel-t.vel).dot(rel.normalized());
                    luminal_core::world::weapon_probability::flight_seconds(p,rel.length(),closing)
                });
                let energy=if p==Payload::Nuclear {params::NUCLEAR_ENERGY_J.value} else {luminal_core::world::weapon_probability::SRM_HIT_ENERGY_J};
                let detail=if tracked {format!("T+{} · {} / hit",fmt_time(flight.unwrap_or(0.0)),fmt_energy(energy))} else if target.is_none() {"Select a target for a firing solution".into()} else if p==Payload::Kinetic {"Resolved contact required".into()} else {"Range / arrival unknown · speculative".into()};
                ui.painter().text(Pos2::new(left,r.top()+20.0),egui::Align2::LEFT_TOP,detail,mono(9.0),TEXT_MUTED);
                ui.painter().text(Pos2::new(left,r.top()+31.0),egui::Align2::LEFT_TOP,format!("AMMO {count} · VOLLEY {volley} · QUEUED {}",b.missile_queued[p.index()]),mono(9.0),TEXT_MUTED);
                let button=Rect::from_min_max(Pos2::new(left,r.bottom()-24.0),Pos2::new(left+r.width()*0.48,r.bottom()-4.0));
                let mut child=ui.new_child(egui::UiBuilder::new().id_salt(("launch",row)).max_rect(button));
                let text=format!("{} ×{volley}",if reload>0.0 {"QUEUE"} else {"LAUNCH"});
                if tac_button(&mut child,&text,button.size(),WARM,false,ready).on_hover_text("Estimated hit chance before enemy defence and ECM. Hit energy before screens and armour. Arrival assumes the current received course. One click orders one volley.").clicked() && let Some(c)=target {
                    self.command(Command::Launch {body:b.id,target:c.id,payload:p});
                }
                ui.painter().text(Pos2::new(right,button.center().y),egui::Align2::RIGHT_CENTER,status,mono(8.0),if ready {ACCENT} else {TEXT_MUTED});
            } else {
                let fitted=b.ship_class!=Some(luminal_core::world::ShipClass::Picket) && b.armed;
                let solution=target.and_then(|c|b.beam_solutions.get(&c.id));
                let energy=params::SHIP_BEAM_ENERGY_J.value*b.ship_class.map_or(1.0,|c|c.beam_power());
                let reload=(b.beam_ready_at-view.time).max(0.0);
                let status=if b.jump.is_some() {"OFF: JUMP".into()} else if !fitted {"NOT FITTED".into()} else if b.damage.operating_effectiveness(System::Beam)<=0.0 {"DISABLED".into()}
                    else if b.thermal.dumping {"DUMPING".into()} else if !b.thermal.can_fire_energy(energy) {"POWER / HEAT".into()}
                    else if b.beam_mode==luminal_core::world::weapon_fit::BeamMode::Interference && b.damage.operating_effectiveness(System::Ecm)<=0.0 {"ECM DISABLED".into()}
                    else if b.beam_mode==luminal_core::world::weapon_fit::BeamMode::Interference && solution.is_some_and(|s|s.range_km>luminal_core::world::weapon_fit::INTERFERENCE_RANGE_LS*luminal_core::units::LIGHT_SECOND) {"OUT OF RANGE".into()}
                    else if solution.is_none() {"NO SOLUTION".into()} else if reload>0.0 {format!("RELOAD {reload:.0}s")}
                    else if solution.is_some_and(|s|!s.worth_firing) {"AUTO HOLDS".into()} else {"READY".into()};
                ui.painter().text(Pos2::new(right,r.top()+5.0),egui::Align2::RIGHT_TOP,&status,mono(10.0),if status=="READY" {SYS_OK} else {WARM});
                let detail=if b.beam_mode==luminal_core::world::weapon_fit::BeamMode::Interference {"6 LS · -15% offensive beam coupling · 8s".into()} else {solution.filter(|_|fitted).map_or_else(||"Expected energy — · coupling —".into(),|s|format!("EXP {} / pulse · {:.1}% coupled",fmt_energy(s.expected_j),100.0*s.coupled_fraction))};
                ui.painter().text(Pos2::new(left,r.top()+20.0),egui::Align2::LEFT_TOP,detail,mono(9.0),TEXT_MUTED);
                let aim=solution.map_or_else(||"Aim uncertainty — · beam radius —".into(),|s|format!("Aim ±{:.2} km · radius {:.2} km",s.aim_sigma_km,s.spot_km));
                ui.painter().text(Pos2::new(left,r.top()+31.0),egui::Align2::LEFT_TOP,aim,mono(9.0),TEXT_MUTED);
                let buttons=Rect::from_min_max(Pos2::new(left,r.bottom()-24.0),Pos2::new(right,r.bottom()-4.0));
                let mut child=ui.new_child(egui::UiBuilder::new().id_salt("beam_modes").max_rect(buttons));
                child.horizontal(|ui| {
                    let support=class.has_projector();
                    let n=if support {4.0} else {3.0};
                    let w=(buttons.width()-(n-1.0)*ui.spacing().item_spacing.x)/n;
                    if tac_button(ui,"AUTO",EVec2::new(w,20.0),ACCENT,b.beam_auto,fitted).clicked() {self.command(Command::ArmBeams {body:b.id});}
                    if tac_button(ui,"DIRECT",EVec2::new(w,20.0),ACCENT,!b.beam_auto && b.beam_target.is_some(),fitted && target.is_some_and(|c|c.track.is_some() && !c.stale)).on_hover_text("Assign this target; permits risky shots that AUTO would hold.").clicked() {self.command(Command::EngageBeam {body:b.id,target:target.map(|c|c.id)});}
                    if tac_button(ui,"HOLD",EVec2::new(w,20.0),ACCENT,!b.beam_auto && b.beam_target.is_none(),fitted).clicked() {self.command(Command::EngageBeam {body:b.id,target:None});}
                    if support {
                        use luminal_core::world::weapon_fit::BeamMode;
                        let active=b.beam_mode==BeamMode::Interference;
                        if tac_button(ui,if active {"DISRUPT"} else {"DAMAGE"},EVec2::new(w,20.0),ACCENT,active,fitted).on_hover_text(format!("{}: toggle the main beam between damage and electronic attack. Uses the same energy, heat and recharge; needs functioning beam and ECM systems. Requires a ship within 6 LS. Eight seconds of 15% weaker offensive beam coupling, no stacking, no hull damage. PD and missiles are unaffected.",self.theme.projector())).clicked() {
                            self.command(Command::SetBeamMode {body:b.id,mode:if active {BeamMode::Damage} else {BeamMode::Interference}});
                        }
                    }
                });
                ui.interact(r,ui.id().with("beam_prediction"),Sense::hover()).on_hover_text("Expected energy uses the same received-track prediction as AUTO fire control, before screens and armour. Aim ± is one standard deviation at pulse arrival, including pointing error. Coupling is expected energy delivered, not hit probability.");
            }
        }
        let queued=b.missile_queued.iter().sum::<u32>();
        let bottom=Rect::from_min_max(Pos2::new(area.left(),area.bottom()-footer),area.right_bottom());
        let mut child=ui.new_child(egui::UiBuilder::new().id_salt("defence_footer").max_rect(bottom));
        child.horizontal(|ui| {
            let pd_status=if b.jump.is_some() {"OFF: JUMP"} else if b.thermal.dumping {"OFF: DUMP"} else if b.damage.operating_effectiveness(System::PdLaser)<=0.0 {"DISABLED"}
                else if b.thermal.heat_j+params::PD_WASTE_HEAT_J>params::BEAM_HEAT_LIMIT_J.value*b.thermal.capacity_scale {"OFF: HEAT"} else {"AUTO"};
            ui.label(egui::RichText::new(format!("PD {pd_status} · {} INT",b.interceptor_battery.map_or(0,|x|x.rounds))).monospace().size(9.0).color(if pd_status=="AUTO" {TEXT_MUTED} else {WARM}))
                .on_hover_text(format!("{} laser mounts. Interceptors remain independent of laser heat limits.",b.point_defence.map_or(0,|pd|pd.lasers)));
            if b.ship_class==Some(luminal_core::world::ShipClass::Battleship) {
                let left=(b.spinal_ready_at-view.time).max(0.0);
                ui.label(egui::RichText::new(if b.jump.is_some() {format!("{} OFF: JUMP",self.theme.spinal())} else if left>0.0 {format!("{} {left:.0}s",self.theme.spinal())} else {format!("{} READY",self.theme.spinal())}).monospace().size(9.0).color(WARM)).on_hover_text("Forward mount: requires target alignment within 2°; shares heat and power with beams.");
            }
            if queued>0 && ui.small_button(format!("CANCEL {queued}")).clicked() {self.command(Command::CancelLaunches {body:b.id});}
        });
        ui.allocate_space(area.size());
    }

    fn acquire_first_target(&mut self,view:&View) {
        if self.own_faction().is_some() && !self.target_chosen && !matches!(self.inspected,Some(Selection::Contact(_)))
            && let Some(contact)=view.contacts.iter().find(|c|!c.stale && c.detection>=sensors::DetectionLevel::Resolved && c.resolved_kind==Some(BodyKind::Ship)) {
            // Acquiring a weapon target must not replace follow/route/movement orders.
            self.inspected=Some(Selection::Contact(contact.id));
        }
    }

    fn select_object(&mut self, selection: Selection, view: &View) {
        if let Selection::Body(id)=selection && view.bodies.iter().any(|b|b.id==id && b.controllable && b.kind==BodyKind::Ship
            && (self.role==Role::Spectator || self.own_faction()==Some(b.faction))) {self.selected=Some(selection);}
        self.inspected=Some(selection);
        self.target_chosen=true;
    }

    fn new() -> Self {
        // Presentation-only stream: startup choices never consume combat RNG.
        let seed=std::env::var("LUMINAL_SEED").ok().and_then(|s|s.parse().ok()).unwrap_or_else(|| {
            if cfg!(test) || std::env::var_os("LUMINAL_SCREENSHOT").is_some() {42} else {
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos() as u64
            }
        });
        let mut rng=luminal_core::rng::Rng::stream(seed,0x53544152545550);
        let classes=luminal_core::world::ShipClass::COMBAT;
        let random_class=classes[(rng.next_u64()%classes.len() as u64) as usize];
        let random_theme=theme::Theme::ALL[(rng.next_u64()%theme::Theme::ALL.len() as u64) as usize];
        let chosen=std::env::var("LUMINAL_SHIP").ok().and_then(|name|classes.into_iter().find(|c|c.name().eq_ignore_ascii_case(&name)))
            .unwrap_or(if cfg!(test) {luminal_core::world::ShipClass::Frigate} else {random_class});
        let theme=std::env::var("LUMINAL_THEME").ok().and_then(|name|theme::Theme::ALL.into_iter().find(|t|t.name().eq_ignore_ascii_case(&name)))
            .unwrap_or(if cfg!(test) {theme::Theme::Culture} else {random_theme});
        let scenario=std::env::var("LUMINAL_SCENARIO").ok().and_then(|name|Scenario::parse(&name)).unwrap_or(Scenario::Escort);
        Self::new_with_scenario(scenario,chosen,theme)
    }
    #[cfg(test)]
    fn new_with_class(chosen:luminal_core::world::ShipClass) -> Self {
        Self::new_with_scenario(Scenario::Escort,chosen,theme::Theme::Culture)
    }
    #[cfg(test)]
    fn new_with_theme(chosen:luminal_core::world::ShipClass,theme:theme::Theme) -> Self {
        Self::new_with_scenario(Scenario::Escort,chosen,theme)
    }
    fn new_with_scenario(scenario:Scenario,chosen:luminal_core::world::ShipClass,theme:theme::Theme) -> Self {
        let seed = std::env::var("LUMINAL_SEED").ok().and_then(|s|s.parse().ok()).unwrap_or_else(|| {
            if cfg!(test) || std::env::var_os("LUMINAL_SCREENSHOT").is_some() {42} else {
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos() as u64
            }
        });
        let system=theme.star_system(seed);
        let mut world=if scenario==Scenario::Escort {scenario::transport_intercept_class_in_system(seed,chosen,system)} else {scenario.build(seed,system)};
        theme.name_scenario(&mut world,seed);
        let player=world.objective.as_ref().and_then(|o|o.player);
        let inspected=if scenario==Scenario::Escort {world.objective.as_ref().map(|o|Selection::Body(o.protect))} else {None};
        let mut session=LocalSession::new(world);
        session.set_build_identity(BUILD_VERSION,BUILD_COMMIT,build_dirty());
        let log_path=std::path::PathBuf::from("logs/latest.log");
        let log_error=if cfg!(test) || std::env::var_os("LUMINAL_SCREENSHOT").is_some() {None} else {
            std::fs::create_dir_all("logs").and_then(|_|session.enable_debug_log(&log_path)).err()
                .map(|e|format!("Debug logfile unavailable: {e}"))
        };
        let mut app = Self {
            auto_speed:true,auto_speed_elapsed:0.0,
            session,
            role: Role::Faction(ESCORT),
            overlay: Some(ESCORT),
            camera: Camera { center: Vec2::ZERO, km_per_px: AU / 500.0 },
            track_player:true,
            tracking_zoom_hold:0.0,
            manual_ping_zoom_until:0.0,
            fit_pending: true,
            opening_fit: true,
            selected: player.map(Selection::Body),
            inspected,
            movement_mode: MovementMode::Flyby,
            jump_select:None,
            target_chosen: false,
            last_message: log_error,
            payload: Payload::Kinetic,
            dev: DevHooks { screenshot: std::env::var_os("LUMINAL_SCREENSHOT").map(Into::into), frames: 0 },
            bearing_display: BTreeMap::new(),
            ship_headings: BTreeMap::new(),
            selection_pending:!cfg!(test) && (std::env::var_os("LUMINAL_SCREENSHOT").is_none() || std::env::var_os("LUMINAL_SHIP_SELECT").is_some()),
            chosen_class:chosen,
            chosen_scenario:scenario,
            theme,ship_art:ship_art::ShipArt::default(),
            tactical_log:TacticalLog::default(),
            weapon_effects:weapon_effects::WeaponEffects::default(),
            jump_effects:jump_effects::JumpEffects::default(),
            celestial_art:celestial_art::CelestialArt::default(),
            audio:audio::Audio::default(),
            manual_flight:None,manual_send_elapsed:0.0,
        }
        .with_env_setup();
        let bots: &[FactionId] = if app.chosen_scenario==Scenario::Escort && app.role==Role::Faction(RAIDER) {&[ESCORT]} else {app.chosen_scenario.bots()};
        for faction in bots {app.session.enable_bot(*faction, true);}
        let _ = app.session.command(app.role, Command::SetWarp(AUTO_MIN_WARP));
        // AUTO ramps up from 5×; screenshot fixtures remain paused.
        if app.dev.screenshot.is_none() && !app.selection_pending {
            let _ = app.session.command(app.role, Command::SetPaused(false));
        }
        app
    }

    fn restart_scenario(&mut self) {
        let theme=self.theme;
        let scenario=self.chosen_scenario;
        let class=self.chosen_class;
        let pending=self.selection_pending;
        let volume=self.audio.volume;
        let music_volume=self.audio.music_volume;
        let muted=self.audio.muted;
        *self=Self::new_with_scenario(scenario,class,theme);
        self.theme=theme;
        self.selection_pending=pending;
        if !pending {let _=self.session.command(self.role,Command::SetPaused(false));}
        self.audio.volume=volume;self.audio.music_volume=music_volume;self.audio.muted=muted;self.audio.settings_changed();
    }

    fn with_env_setup(mut self) -> Self {
        // Screenshot pre-runs should exercise the same hostile AI as live play.
        if self.dev.screenshot.is_some() {
            self.session.enable_bot(RAIDER,true);
            self.jump_effects.observe(&self.session.view(self.role),0.0);
        }
        // `LUMINAL_ORDERS=orbit:<body>:<celestial>;intercept:<body>:own:<body>;intercept:<body>:contact:<n>;move:<body>:objective;launch:<body>:<contact>:<payload>`
        // `LUMINAL_ORDERS_AT=<sim seconds>` runs the scenario that far before issuing them.
        let orders_at = std::env::var("LUMINAL_ORDERS_AT").ok().and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
        let _ = self.session.command(Role::Spectator, Command::SetPaused(false));
        self.session.tick(orders_at);
        let _ = self.session.command(Role::Spectator, Command::SetPaused(true));
        if let Ok(orders) = std::env::var("LUMINAL_ORDERS") {
            let view = self.session.view(Role::Spectator);
            for o in orders.split(';').filter(|o| !o.is_empty()) {
                let f: Vec<&str> = o.split(':').collect();
                let num = |i: usize| f.get(i).and_then(|v| v.parse::<u32>().ok());
                let Some(body) = num(1).map(BodyId) else { continue };
                let Some(faction) = view.bodies.iter().find(|b| b.id == body).map(|b| b.faction) else { continue };
                let cmd = match (f[0], f.get(2).copied()) {
                    ("jump", _) => f.get(2).and_then(|x|x.parse::<f64>().ok()).zip(f.get(3).and_then(|y|y.parse::<f64>().ok())).map(|(x,y)|Command::Jump {body,destination:Vec2::new(x*AU,y*AU)}),
                    ("withdraw", _) => Some(Command::Withdraw {body}),
                    ("orbit", _) => num(2).map(|c| Command::Orbit { body, celestial: c as usize }),
                    ("beam", _) => num(2).map(|c| Command::FireBeam { body, target: ContactId(c) }),
                    ("ping", _) => Some(Command::Ping { body }),
                    ("dump", _) => Some(Command::SetHeatDump {body,enabled:true}),
                    ("launch", _) => num(2).zip(f.get(3)).map(|(c, p)| Command::Launch {
                        body,
                        target: ContactId(c),
                        payload: Payload::ALL.into_iter().find(|x| x.name() == *p).unwrap_or(Payload::Kinetic),
                    }),
                    ("move", Some("objective")) => view.objective.as_ref().map(|o| Command::MoveTo { body, point: o.center }),
                    ("intercept", Some("own")) => num(3).map(|t| Command::Intercept { body, target: InterceptTarget::Own(BodyId(t)) }),
                    ("intercept", Some("contact")) => {
                        num(3).map(|t| Command::Intercept { body, target: InterceptTarget::Contact(ContactId(t)) })
                    }
                    ("flyby", Some("own")) => num(3).map(|t| Command::Flyby { body, target: InterceptTarget::Own(BodyId(t)) }),
                    ("flyby", Some("contact")) => num(3).map(|t| Command::Flyby { body, target: InterceptTarget::Contact(ContactId(t)) }),
                    _ => None,
                };
                if let Some(cmd) = cmd
                    && let Err(e) = self.session.command(Role::Faction(faction), cmd)
                {
                    eprintln!("LUMINAL_ORDERS {o}: {e:?}");
                }
            }
        }
        if let Some(t) = std::env::var("LUMINAL_ADVANCE").ok().and_then(|v| v.parse::<f64>().ok()) {
            let _ = self.session.command(Role::Spectator, Command::SetPaused(false));
            self.session.tick(t);
            let _ = self.session.command(Role::Spectator, Command::SetPaused(true));
        }
        match std::env::var("LUMINAL_ROLE").as_deref() {
            Ok("spectator") => self.role = Role::Spectator,
            Ok("raider") => {
                self.role = Role::Faction(RAIDER);
                if self.chosen_scenario==Scenario::Escort {self.selected = Some(Selection::Body(BodyId(2)));}
            }
            _ => {}
        }
        if self.dev.screenshot.is_some() && let Some(index)=std::env::var("LUMINAL_CELESTIAL").ok().and_then(|v|v.parse::<usize>().ok()) {
            let view=self.session.view(self.role);
            if let Some(body)=view.celestials.get(index) {
                self.camera=Camera {center:body.pos,km_per_px:body.radius/130.0};
                self.track_player=false;self.fit_pending=false;self.opening_fit=false;
            }
        }
        self
    }

    fn dev_screenshot(&mut self, ui: &egui::Ui) {
        let Some(path) = self.dev.screenshot.clone() else { return };
        self.dev.frames += 1;
        if self.dev.frames == 20 {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        let image = ui.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = image {
            let [w, h] = img.size;
            let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
            out.extend(img.pixels.iter().flat_map(|c| [c.r(), c.g(), c.b()]));
            if let Err(e) = std::fs::write(&path, out) {
                eprintln!("screenshot failed: {e}");
            }
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn command(&mut self, cmd: Command) {
        if cmd.body().is_some() && self.session.waiting_for_event() {let _=self.session.command(self.role,Command::SetPaused(true));}
        if matches!(cmd,Command::SetWarp(_)) {self.auto_speed=false;}
        let note=match &cmd {
            Command::AppendWaypoint {..}=>Some(("helm","ROUTE POINT ADDED".into())),
            Command::Launch {payload,..}=>Some(("launch",format!("MISSILE QUEUED · {}",self.theme.weapon(*payload)))),
            Command::Alongside {..}|Command::Follow {..}=>Some(("helm","FOLLOW · 1 LS ALONGSIDE".into())),
            Command::Intercept {..}=>Some(("helm","MATCH ORDERED".into())),
            Command::Flyby {..}=>Some(("helm","FLYBY ORDERED".into())),
            Command::CombatRange {standoff,..}=>Some(("helm",if *standoff {"STANDOFF ORDERED".into()} else {"CLOSE ORDERED".into()})),
            Command::KeepRange {range,..}=>Some(("helm",format!("HOLD {}",fmt_distance(*range)))),
            Command::SetHeatDump {enabled,..}=>Some(("thermal",if *enabled {"HEAT DUMP · RADIATORS OPEN".into()} else {"HEAT DUMP STOPPED".into()})),
            Command::Evade {..}=>Some(("helm","EVADE ORDERED".into())),
            Command::AllStop {..}=>Some(("helm","ALL STOP ORDERED".into())),
            Command::SetScreen {up,..}=>Some(("screen",format!("{} {}",self.theme.screens(),if *up {"RAISING"} else {"LOWERING"}))),
            _=>None,
        };
        let ping_duration=if let Command::Ping {body}=cmd {
            self.session.view(self.role).bodies.iter().find(|b|b.id==body).map(|b|
                2.0*sensors::ping_range(sensors::REFERENCE_EF)*b.ship_class.map_or(1.0,|c|c.sensor_rating()/100.0)*b.damage.operating_effectiveness(System::Active)/LIGHT_SECOND)
        } else {None};
        self.last_message = match self.session.command(self.role, cmd) {
            Ok(()) => {
                if let Some(duration)=ping_duration {self.manual_ping_zoom_until=self.manual_ping_zoom_until.max(self.session.view(self.role).time+duration);}
                self.audio.play(audio::Cue::Click);
                if let Some((key,text))=note {self.tactical_log.push(key.into(),text,ACCENT,self.session.view(self.role).time);}
                None
            },
            Err(r) => {
                self.audio.play(audio::Cue::Alert);
                let text=format!("Rejected: {r:?}");
                self.tactical_log.push("rejected".into(),text.clone(),SYS_DAMAGED,self.session.view(self.role).time);
                Some(text)
            },
        };
    }

    fn own_faction(&self) -> Option<FactionId> {
        match self.role {
            Role::Faction(f) => Some(f),
            Role::Spectator => None,
        }
    }
}

/// A body's colour as `viewer` sees it; the spectator takes the escort's side.
fn body_color(b: &BodyView, viewer: Option<FactionId>) -> Color32 {
    if b.faction != viewer.unwrap_or(ESCORT) {
        ENEMY
    } else if b.kind == BodyKind::Ship && !b.armed {
        NONCOMBAT
    } else {
        FRIEND
    }
}

fn faction_name(f: FactionId) -> &'static str {
    match f {
        ESCORT => "Escort",
        RAIDER => "Raider",
        _ => "Unknown",
    }
}

fn celestial_color(k: CelestialKind) -> Color32 {
    match k {
        CelestialKind::Star => Color32::from_rgb(255, 214, 120),
        CelestialKind::Planet => Color32::from_rgb(90, 180, 160),
        CelestialKind::Moon => Color32::from_rgb(170, 170, 180),
    }
}

fn fmt_time(t: f64) -> String {
    if !t.is_finite() {return "UNREACHABLE".into();}
    let sign = if t < 0.0 { "-" } else { "" };
    let s = t.abs() as u64;
    format!("{sign}{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

fn fmt_age(s: f64) -> String {
    if s < 120.0 { format!("{s:.0} s") } else if s < 7200.0 { format!("{:.1} min", s / 60.0) } else { format!("{:.1} h", s / 3600.0) }
}

/// Frame the selected contact and its selected opponent using received positions.
fn edge_pan_delta(rect:Rect,pointer:Pos2,dt:f64)->Vec2 {
    if !rect.contains(pointer) {return Vec2::ZERO;}
    let edge=24.0_f32;
    let axis=|p:f32,lo:f32,hi:f32|((p-(hi-edge))/edge).clamp(0.0,1.0)-(((lo+edge)-p)/edge).clamp(0.0,1.0);
    Vec2::new(axis(pointer.x,rect.left(),rect.right()) as f64,-axis(pointer.y,rect.top(),rect.bottom()) as f64)*700.0*dt.clamp(0.0,0.1)
}

fn tracking_zoom_scale(view:&View,origin:Vec2,rect:Rect,selected:InterceptTarget,secondary:Option<InterceptTarget>)->Option<f64> {
    let point=|target|match target {
        InterceptTarget::Own(id)=>view.bodies.iter().find(|b|b.id==id).map(|b|(b.pos,Vec2::ZERO)),
        InterceptTarget::Contact(id)=>view.contacts.iter().find(|c|c.id==id && !c.stale)
            .and_then(|c|c.track.as_ref()).map(|t|(t.pos,Vec2::new(
                3.0*t.cov[0][0].max(0.0).sqrt(),3.0*t.cov[1][1].max(0.0).sqrt()))),
    };
    let primary=point(selected)?;
    let desired=std::iter::once(primary).chain(secondary.and_then(point)).map(|(pos,uncertainty)| {
        let delta=pos-origin;
        ((delta.x.abs()+uncertainty.x)/(rect.width() as f64*0.35).max(1.0))
            .max((delta.y.abs()+uncertainty.y)/(rect.height() as f64*0.30).max(1.0))
    }).fold(0.0_f64,f64::max);
    Some(desired.max(2.0*LIGHT_SECOND/(rect.width().min(rect.height()) as f64).max(1.0)).clamp(1e-3,1e8))
}

fn sparse_distance(value:f64)->String {
    if value>=10.0 {format!("{value:.0}")}
    else if value>0.0 {
        let decimals=(1.0-value.log10().floor()).clamp(0.0,8.0) as usize;
        let text=format!("{value:.decimals$}");
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {"0".into()}
}

fn draw_target_link(painter:&egui::Painter,cam:&Camera,rect:Rect,from:Vec2,to:Vec2) {
    let pts=[to_screen(cam,rect,from),to_screen(cam,rect,to)];
    let color=Color32::from_rgb(235,80,80);
    painter.extend(Shape::dashed_line(&pts,Stroke::new(1.0,color.gamma_multiply(0.7)),6.0,4.0));
    let distance=(to-from).length();
    let text=if distance>=0.1*AU {format!("{} AU",sparse_distance(distance/AU))}
        else {format!("{} LS",sparse_distance(distance/LIGHT_SECOND))};
    let galley=painter.layout_no_wrap(text,mono(10.0),color);
    if pts[0].distance(pts[1])<140.0_f32.max(galley.size().x*3.0) {return;}
    let mut direction=(pts[1]-pts[0]).normalized();
    // Keep text upright when the target is left of the ship.
    if direction.x<0.0 {direction = -direction;}
    let above=EVec2::new(direction.y,-direction.x);
    let center=pts[0].lerp(pts[1],0.5)+above*(galley.size().y*0.5+4.0);
    let shape=egui::epaint::TextShape::new(center-galley.size()*0.5,galley,color)
        .with_angle_and_anchor(direction.y.atan2(direction.x),egui::Align2::CENTER_CENTER);
    if rect.shrink(8.0).contains_rect(shape.visual_bounding_rect()) {painter.add(shape);}
}

fn fmt_energy(joules:f64)->String {
    let units=["J","kJ","MJ","GJ","TJ","PJ","EJ","ZJ","YJ","RJ","QJ"];
    let mut value=joules.max(0.0);
    let mut unit=0;
    while value.round()>=1000.0 && unit+1<units.len() {value/=1000.0;unit+=1;}
    format!("{value:.0} {}",units[unit])
}

fn fmt_speed(speed: f64) -> String {
    if speed.abs()>0.01*C {format!("{}{:.3}c",if speed<0.0 {"-"} else {""},speed.abs()/C)}
    else {format!("{speed:.0} km/s")}
}

fn fmt_distance(km: f64) -> String {
    if km >= 0.1 * AU {
        format!("{} AU", sparse_distance(km / AU))
    } else if km >= 0.5 * LIGHT_SECOND {
        format!("{} ls", sparse_distance(km / LIGHT_SECOND))
    } else {
        format!("{km:.0} km")
    }
}

fn contact_label(c: &ContactView) -> String {
    if c.resolved_interceptor {format!("Interceptor {}",c.id.0)}
    else if c.resolved_missile {format!("Missile {}",c.id.0)}
    else if c.resolved_kind==Some(BodyKind::Probe) {format!("Probe {}",c.id.0)}
    else if let Some(name)=&c.identified_name {name.clone()}
    else if c.resolved_kind==Some(BodyKind::Station) {format!("Station {}",c.id.0)}
    else if let Some(class)=c.resolved_class {format!("{}{}",class.designator(),c.id.0)}
    else {format!("T{}",c.id.0)}
}

/// LRM seekers support speculative bearing searches even without a range fix.
fn command_columns(ui:&mut egui::Ui,content:impl FnOnce(&mut [egui::Ui])) {
    let rect=ui.available_rect_before_wrap();let mut x=rect.left();
    let mut columns=Vec::new();
    for (i,weight) in [0.175,0.20,0.25,0.20,0.175].into_iter().enumerate() {
        let width=rect.width()*weight;
        let r=Rect::from_min_max(Pos2::new(x+8.0,rect.top()),Pos2::new(x+width-8.0,rect.bottom()));
        columns.push(ui.new_child(egui::UiBuilder::new().id_salt(i).max_rect(r)));x+=width;
    }
    content(&mut columns);
    ui.allocate_space(rect.size());
}

fn missile_solution_launchable(target:Option<&ContactView>,payload:Payload,chance:f64)->bool {
    target.is_some_and(|c|payload==Payload::Nuclear || (!c.stale && c.detection>=luminal_core::sensors::DetectionLevel::Resolved && chance>=0.01))
}

fn tactical_shortcut_commands(key:egui::Key,shift:bool,ship:&BodyView,target:Option<&ContactView>)->Vec<Command> {
    let Some(command)=tactical_shortcut(key,ship,target) else {return vec![];};
    let count=if shift && let Command::Launch {payload,..}=&command {
        ship.magazine[payload.index()].saturating_sub(ship.missile_queued[payload.index()]).div_ceil(ship.ship_class.unwrap_or(luminal_core::world::ShipClass::Frigate).launchers(*payload).max(1))
    } else {1};
    vec![command;count as usize]
}

fn tactical_shortcut(key:egui::Key,ship:&BodyView,target:Option<&ContactView>)->Option<Command> {
    use egui::Key;
    use luminal_core::world::controls::{ControlledSystem as C,Mode};
    let control=match key {
        Key::E=>Some((C::Ecm,ship.controls.ecm)),
        Key::R if ship.has_screen=>Some((C::Screens,ship.controls.screens)),
        Key::V=>Some((C::Evade,ship.controls.evade)),
        Key::A if ship.sensors.active=>Some((C::Active,ship.controls.active)),_=>None,
    };
    if let Some((system,mode))=control {
        let mode=if matches!(system,C::Active|C::Evade) {if mode==Mode::Auto {Mode::Off} else {Mode::Auto}} else {mode.next()};
        return Some(Command::SetSystemMode {body:ship.id,system,mode});
    }
    if key==Key::P {return Some(Command::Ping {body:ship.id});}
    if matches!(key,Key::Num0|Key::Num1|Key::Num2|Key::Num3) {
        let target=ship.autopilot.and_then(|a|movement_target(a.order))?;
        let mode=match key {Key::Num0=>MovementMode::Alongside,Key::Num1=>MovementMode::Close,Key::Num2=>MovementMode::Standoff,_=>MovementMode::Flyby};
        return Some(mode.command(ship.id,target));
    }
    let contact=target?;
    match key {
        Key::L|Key::S=>{
            let payload=if key==Key::L {Payload::Nuclear} else {Payload::Kinetic};
            let available=ship.magazine[payload.index()].saturating_sub(ship.missile_queued[payload.index()]);
            let chance=missile_hit_estimate(ship,Some(contact),payload);
            (available>0 && ship.damage.operating_effectiveness(payload.launcher_system())>0.0
                && missile_solution_launchable(Some(contact),payload,chance))
                .then_some(Command::Launch {body:ship.id,target:contact.id,payload})
        },
        _=>None,
    }
}

/// Conservative UI estimate from received information only, before defence.
fn missile_hit_estimate(ship:&BodyView,target:Option<&ContactView>,payload:Payload)->f64 {
    let Some(contact)=target else {return 0.0;};
    let Some(track)=contact.track.as_ref() else {return 0.0;};
    let range=(track.pos-ship.pos).length();
    {
        let closing=(ship.vel-track.vel).dot((track.pos-ship.pos).normalized());
        if luminal_core::world::weapon_probability::flight_seconds(payload,range,closing)>=payload.endurance() {return 0.0;}
    }
    let sigma=(track.cov[0][0]+track.cov[1][1]).max(0.0).sqrt();
    let base=luminal_core::world::weapon_probability::hit_chance(payload,range,
        luminal_core::world::weapon_probability::quality(contact.detection),sigma,
        luminal_core::world::weapon_probability::evasion_score(track.accel,track.vel-ship.vel,track.pos-ship.pos),1.0);
    luminal_core::world::weapon_probability::ping_supported_chance(base,contact.active_fire_control)
}

fn contact_has_course(c:&ContactView)->bool {
    !c.stale && c.detection>=sensors::DetectionLevel::Resolved && c.resolved_kind==Some(BodyKind::Ship)
        && c.track.as_ref().is_some_and(|t|t.vel.length().is_finite() && t.vel.length()>1e-6)
}

fn target_system_report(c:&ContactView,view:&View)->Option<Report> {
    let report=c.damage?;
    // Reports are historical, not a promise that the enemy remains disabled.
    // Expire at the fastest repair interval, accounting for observed light delay.
    let visible_time=c.last_emitted_at+(view.time-c.last_received_at).max(0.0);
    if visible_time-report.observed_at>=luminal_core::damage::SYSTEM_REPAIR_SECONDS {return None;}
    if report.operating_effectiveness(System::Beam)==0.0 && view.combat.iter().any(|e|
        e.contact==Some(c.id) && matches!(e.kind,CombatKind::BeamPulse|CombatKind::SpinalPulse) && e.emitted_at>report.observed_at) {return None;}
    Some(report)
}
fn bearing_opacity(b:&luminal_core::session::BearingView,now:f64)->f32 {
    (1.0-((now-b.received_at)/BEARING_FADE_S).clamp(0.0,1.0)) as f32
}

impl eframe::App for LuminalApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.selection_pending {
            let start=self.startup(ui);
            if start {self.deploy_selected();}
            self.dev_screenshot(ui);ui.ctx().request_repaint();return;
        }
        let dt = ui.input(|i| i.stable_dt) as f64;
        self.session.set_watch(self.own_faction());
        if self.jump_effects.holds_auto_speed(ui.input(|i|i.time)) {self.auto_speed_elapsed=0.0;}
        else {self.update_auto_speed(dt.min(0.1));}
        self.session.tick_realtime(dt.min(0.1));
        ui.ctx().request_repaint();

        if !ui.ctx().egui_wants_keyboard_input() {
            let keys=[egui::Key::ArrowLeft,egui::Key::ArrowRight,egui::Key::ArrowUp,egui::Key::ArrowDown];
            let (held,pressed,released)=ui.input(|i|(keys.map(|k|i.key_down(k)),keys.map(|k|i.key_pressed(k)),keys.iter().any(|k|i.key_released(*k))));
            self.free_flight_input(held,pressed,released,dt.min(0.1));
            let (space, f, t) = ui.input(|i| (i.key_pressed(egui::Key::Space), i.key_pressed(egui::Key::F),i.key_pressed(egui::Key::T)));
            if t {self.track_player = !self.track_player;self.fit_pending=false;}
            if space {
                let paused = self.session.view(Role::Spectator).paused;
                self.command(Command::SetPaused(!paused));
            }
            if f {
                self.fit_pending = true;
            }
            for key in [egui::Key::E,egui::Key::R,egui::Key::B,egui::Key::A,egui::Key::V,egui::Key::L,egui::Key::S,egui::Key::P,egui::Key::Num1,egui::Key::Num2,egui::Key::Num3,egui::Key::Num0] {
                if ui.input(|i|i.key_pressed(key)) {
                    let view=self.session.view(self.role);
                    if let Some(ship)=view.bodies.iter().find(|b|b.controllable && b.kind==BodyKind::Ship) {
                        let target=match self.inspected {Some(Selection::Contact(id))=>view.contacts.iter().find(|c|c.id==id),_=>None};
                        for command in tactical_shortcut_commands(key,ui.input(|i|i.modifiers.shift),ship,target) {self.command(command);}
                    }
                }
            }
        }

        let mut view = self.session.view(self.role);
        if !view.bodies.iter().any(|b| self.selected == Some(Selection::Body(b.id)) && b.controllable && b.kind == BodyKind::Ship) {
            self.selected = view.bodies.iter().find(|b| b.controllable && b.kind == BodyKind::Ship).map(|b| Selection::Body(b.id));
        }
        self.acquire_first_target(&view);
        self.smooth_bearings(&mut view,dt);
        let overlay = match (self.role, self.overlay) {
            (Role::Spectator, Some(f)) => Some((
                self.session.view(Role::Faction(f)),
                self.session.contact_truth(Role::Spectator, f).unwrap_or_default(),
            )),
            _ => None,
        };

        self.update_tactical_log(&view,ui.input(|i|i.time));
        self.jump_effects.observe(&view,ui.input(|i|i.time));
        self.weapon_effects.observe(&view,self.own_faction(),ui.input(|i|i.time));
        self.audio.observe(&view,match self.selected {Some(Selection::Body(id))=>Some(id),_=>None});
        if ui.input(|i|i.pointer.button_clicked(egui::PointerButton::Primary)) {self.audio.play(audio::Cue::Click);}
        let deck_height=(ui.available_height()*0.26).clamp(332.0,348.0);
        let frame=panel_frame().inner_margin(egui::Margin {left:0,right:0,top:5,bottom:4});
        egui::Panel::bottom("command_deck").exact_size(deck_height).resizable(false).frame(frame).show(ui, |ui| {
            panel_style(ui);
            ui.spacing_mut().item_spacing=EVec2::new(4.0,3.0);
            self.command_deck(ui,&view);
        });
        let map=egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| self.map(ui, &view, overlay.as_ref())).response.rect;
        let restart=egui::Area::new(egui::Id::new("tactical_controls")).fixed_pos(map.left_top()+EVec2::splat(10.0)).movable(false).order(egui::Order::Foreground).show(ui.ctx(),|ui| {
            egui::Frame::new().fill(PANEL_BG.gamma_multiply(0.94)).stroke(Stroke::new(1.0,EDGE)).inner_margin(8.0)
                .show(ui,|ui|self.time_bar(ui,&view)).inner
        }).inner;
        if restart {self.restart_scenario();ui.ctx().request_repaint();return;}
        self.dev_screenshot(ui);
    }
}

impl LuminalApp {
    fn update_auto_speed(&mut self,dt:f64) {
        if !self.auto_speed {self.auto_speed_elapsed=0.0;return;}
        self.auto_speed_elapsed+=dt;
        if self.auto_speed_elapsed<0.1 {return;}
        let elapsed=std::mem::take(&mut self.auto_speed_elapsed);
        let view=self.session.view(self.role);
        if view.paused {return;}
        let target=desired_auto_warp(&view);
        let next=smooth_warp(view.warp,target,elapsed);
        let next=if (next-target).abs()<0.01 {target} else {next};
        if (next-view.warp).abs()>0.001 {
            let _=self.session.command(self.role,Command::SetWarp(next));
        }
    }
    fn time_bar(&mut self, ui: &mut egui::Ui, view: &View)->bool {
        ui.set_width(206.0);
        panel_style(ui);
        ui.spacing_mut().item_spacing=EVec2::new(4.0,4.0);
        sub_header(ui,"LUMINAL / TACTICAL",Some((if view.paused {"PAUSED"} else {"LIVE"},ACCENT)));
        ui.label(egui::RichText::new(format!("T+ {}",fmt_time(view.time))).monospace().size(18.0).color(TEXT_HI));
        let mut restart=false;
        ui.horizontal(|ui| {
            if tac_button(ui,if view.paused {"RUN"} else {"PAUSE"},EVec2::new(66.0,23.0),ACCENT,!view.paused,true).clicked() {
                self.command(Command::SetPaused(!view.paused));
            }
            if tac_button(ui,"FIT",EVec2::new(66.0,23.0),ACCENT,false,true).on_hover_text("Fit map (F)").clicked() {self.fit_pending=true;}
            restart=tac_button(ui,"RESTART",EVec2::new(66.0,23.0),WARM,false,true).on_hover_text("Fresh scenario on AUTO speed; replaces the latest log").clicked();
        });
        let speeds=[None].into_iter().chain(WARPS.iter().copied().map(Some)).collect::<Vec<_>>();
        for row in speeds.chunks(3) {
            ui.horizontal(|ui| {for &warp in row {
                if let Some(warp)=warp {
                    if tac_button(ui,&format!("{warp}×"),EVec2::new(66.0,20.0),ACCENT,!self.auto_speed && view.warp==warp,true).clicked() {self.command(Command::SetWarp(warp));}
                } else if tac_button(ui,"AUTO",EVec2::new(66.0,20.0),ACCENT,self.auto_speed,true)
                    .on_hover_text("Smooth range-based speed, 5–1000×; bearing-only search 100×").clicked() {
                        self.auto_speed=true;self.auto_speed_elapsed=0.0;
                        if view.warp<AUTO_MIN_WARP {let _=self.session.command(self.role,Command::SetWarp(AUTO_MIN_WARP));}
                    }
            }});
        }
        ui.label(egui::RichText::new(format!("{} · {:.1}×",if self.auto_speed {"AUTO"} else {"MANUAL"},view.warp)).monospace().size(10.0).color(ACCENT));
        ui.label(egui::RichText::new(BUILD_VERSION).monospace().size(9.0).color(TEXT_MUTED)).on_hover_text(build_hover());
        if ui.button(if self.session.waiting_for_event() {"STOP ADVANCING"} else {"NEXT TACTICAL EVENT"}).on_hover_text("Advance until a received threat, combat report, repair, jump, useful beam range or mission outcome; pauses automatically. Maximum 24 simulated hours.").clicked() {
            self.auto_speed=false;
            if self.session.waiting_for_event() {self.command(Command::SetPaused(true));}
            else {self.session.advance_to_next_event(self.role);}
        }
        if tac_button(ui,"TRACK OWN SHIP",EVec2::new(206.0,23.0),ACCENT,self.track_player,true).clicked() {
            self.track_player = !self.track_player;self.fit_pending=false;
        }
        ui.horizontal(|ui| {
            if tac_button(ui,if self.audio.muted {"MUTED"} else {"SOUND"},EVec2::new(66.0,20.0),ACCENT,!self.audio.muted,true).clicked() {
                self.audio.muted = !self.audio.muted;self.audio.settings_changed();
            }
            ui.spacing_mut().slider_width=110.0;
            if ui.add(egui::Slider::new(&mut self.audio.volume,0.0..=1.0).show_value(false)).on_hover_text("Effects volume · default 35%").changed() {self.audio.settings_changed();}
        });
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("MUSIC").monospace().size(10.0).color(TEXT_MUTED));
            ui.spacing_mut().slider_width=130.0;
            if ui.add(egui::Slider::new(&mut self.audio.music_volume,0.0..=1.0).show_value(false)).on_hover_text("Ambient music · default 25% · slide to zero to disable").changed() {self.audio.settings_changed();}
        });
        restart
    }
    #[allow(dead_code)] // Legacy detailed inspector retained while the compact deck settles.
    fn side_panel(&mut self, ui: &mut egui::Ui, view: &View) {
        let ship = view.bodies.iter().find(|b| b.controllable && Some(b.faction) == self.own_faction());
        if let Some(b) = ship {
            let side = faction_name(b.faction).to_uppercase();
            section(ui, "OWN SHIP", Some((side.as_str(), TEXT_MUTED)));
            card(ui, FRIEND, |ui| {
                let (state, state_color) = if b.avoidance.active { ("AVOIDING COLLISION", DANGER) }
                    else if b.autopilot.is_some() { ("UNDER WAY", ACCENT) } else { ("COASTING", TEXT_MUTED) };
                card_title(ui, Glyph::Own, FRIEND, &b.name,
                    &format!("{:.0} KM/S  ·  {:.2} G", b.vel.length(), b.thrust.length() / G0), Some((state, state_color)));
                damage_bars(ui, Some(&b.damage));
                let power = (b.thermal.capacitor_j / params::BEAM_CAPACITOR_J.value).clamp(0.0, 1.0);
                let heat = b.damage.screen_available;
                meter(ui, "POWER", Some(power), &format!("{:.0}%", 100.0 * power), ACCENT).on_hover_text("Beam capacitor charge");
                meter(ui, "SCREENS", Some(heat), &format!("{:.0}%", 100.0 * heat), HEAT)
                    .on_hover_text("Remaining shield capacity. Absorbed hits heat the ship.");
                ui.add_space(2.0);
                system_matrix(ui, "own", Some(b.damage),self.theme);
                system_legend(ui);
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    let w = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
                    let ping = b.damage.operating_effectiveness(System::Active) > 0.0;
                    if tac_button(ui, "ACTIVE PING", EVec2::new(w, 24.0), ACCENT, false, ping)
                        .on_hover_text("Send one active pulse. Reveals your emissions to other platforms.").clicked()
                    {
                        self.command(Command::Ping { body: b.id });
                    }
                    let screen = b.damage.operating_effectiveness(System::Screens) > 0.0;
                    if tac_button(ui, if b.screen_up { "SCREEN  UP" } else { "SCREEN  DOWN" }, EVec2::new(w, 24.0), ACCENT, b.screen_up, screen)
                        .on_hover_text("Raise rechargeable shields. Absorbed hits heat the ship.").clicked()
                    {
                        self.command(Command::SetScreen { body: b.id, up: !b.screen_up });
                    }
                });
            });
            if let Some(message) = &self.last_message {
                ui.label(egui::RichText::new(message).monospace().size(10.0).color(SYS_DAMAGED));
            }
            self.weapons_panel(ui, view);
            ui.add_space(4.0);
            egui::CollapsingHeader::new(header_text("HELM")).id_salt("helm").show(ui, |ui| {
                hint(ui, "Right-click the plot to order movement");
                if let Some(ap) = b.autopilot {
                    status_line(ui, &match ap.status {
                        AutopilotStatus::Manoeuvring => "MANOEUVRING".into(),
                        AutopilotStatus::Closing { eta, range } => format!("{}  ·  ETA {}", fmt_distance(range), fmt_age(eta)),
                        AutopilotStatus::Holding => "HOLDING STATION".into(),
                        AutopilotStatus::Passed => "FLYBY COMPLETE".into(),
                        AutopilotStatus::NoTrack => "TRACK LOST  ·  COASTING".into(),
                    }, ACCENT);
                    if let Order::Intercept(target) | Order::Flyby(target) = ap.order {
                        let mut match_velocity = matches!(ap.order, Order::Intercept(_));
                        if ui.checkbox(&mut match_velocity, "Match target velocity").changed() {
                            self.command(if match_velocity { Command::Intercept { body: b.id, target } } else { Command::Flyby { body: b.id, target } });
                        }
                    }
                }
                let mut limit = (b.drive_limit / G0).min(params::SHIP_MAX_ACCEL_G.value);
                if ui.add(egui::Slider::new(&mut limit, 0.01..=params::SHIP_MAX_ACCEL_G.value).logarithmic(true).text("g limit")).changed() {
                    self.command(Command::SetDriveLimit { body: b.id, g: limit });
                }
                ui.horizontal(|ui| {
                    let w = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
                    if tac_button(ui, "ALL STOP", EVec2::new(w, 22.0), ACCENT, false, true).clicked() {
                        self.command(Command::AllStop { body: b.id });
                    }
                    if tac_button(ui, "COAST", EVec2::new(w, 22.0), ACCENT, false, true).clicked() {
                        self.command(Command::SetThrust { body: b.id, thrust: Vec2::ZERO });
                    }
                });
            });
        } else {
            section(ui, if self.own_faction().is_some() { "COMMAND LOST" } else { "SPECTATOR" }, None);
            if let Some(message) = &self.last_message {
                ui.label(egui::RichText::new(message).monospace().size(10.0).color(SYS_DAMAGED));
            }
        }

        egui::CollapsingHeader::new(header_text(&format!("CONTACTS  ·  {}", view.contacts.len()))).id_salt("contacts").show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("contact_list").max_height(180.0).show(ui, |ui| {
                for c in &view.contacts {
                    let (status, color) = if c.stale { ("STALE", SYS_DAMAGED) } else if c.track.is_some() { ("TRACK", ACCENT) } else { ("BEARING", TEXT_MUTED) };
                    let pinging = view.hostile_pings.iter().any(|p| p.contact == c.id);
                    let tag = if pinging { format!("PING · {status}") } else { status.to_string() };
                    let glyph = if c.resolved_missile { Glyph::HostileMissile } else { Glyph::Hostile };
                    if list_row(ui, self.inspected == Some(Selection::Contact(c.id)), Some((glyph, CONTACT)), &contact_label(c),
                        Some((&tag, if pinging { DANGER } else { color }))).clicked()
                    {
                        self.inspected = Some(Selection::Contact(c.id));
                    }
                }
                if view.contacts.is_empty() { hint(ui, "No contacts"); }
            });
        });
        egui::CollapsingHeader::new(header_text("ALLIED PLATFORMS")).id_salt("allied").show(ui, |ui| {
            for b in view.bodies.iter().filter(|b| b.kind != BodyKind::Missile) {
                if list_row(ui, self.inspected == Some(Selection::Body(b.id)), Some((Glyph::Own, body_color(b, self.own_faction()))), &b.name,
                    Some(if b.controllable { ("COMMAND", FRIEND) } else { ("AUTOMATIC", TEXT_MUTED) })).clicked()
                {
                    self.select_object(Selection::Body(b.id), view);
                }
            }
            for l in &view.losses { status_line(ui, &format!("{}  ·  LOST", l.name.to_uppercase()), DANGER); }
        });
        let flights = view.bodies.iter().filter(|b| b.kind == BodyKind::Missile).count();
        egui::CollapsingHeader::new(header_text(&format!("MISSILES IN FLIGHT  ·  {flights}"))).id_salt("flights").show(ui, |ui| {
            for b in view.bodies.iter().filter(|b| b.kind == BodyKind::Missile) {
                if let Some(m) = b.missile {
                    status_line(ui, &format!("{} → {}  ·  {:?}", m.payload.name().to_uppercase(), m.target, m.phase), TEXT)
                        .on_hover_text(format!("{}\nFuel {:.0}% · {}", b.name, 100.0 * m.dv_left / params::MISSILE_DELTA_V_KMS.value,
                            if m.locally_resolved { "Local seeker track" } else { "Datalink / search" }));
                    if m.correction_possible == Some(false) { status_line(ui, "UNABLE TO CORRECT", DANGER); }
                } else {
                    status_line(ui, &format!("{}  ·  INTERCEPTING", b.name), TEXT_MUTED);
                }
            }
            if flights == 0 { hint(ui, "None"); }
        });
        egui::CollapsingHeader::new(header_text("ACTION LOG")).id_salt("action_log").show(ui, |ui| {
            for e in view.combat.iter().take(10) {
                ui.label(egui::RichText::new(format!("{}  {}", fmt_time(e.received_at), e.kind.label())).monospace().size(9.5).color(TEXT));
            }
        });
        egui::CollapsingHeader::new(header_text("DETAILS & HELP")).id_salt("details").show(ui, |ui| {
            if let Some(b) = ship { self.body_details(ui, view, b); }
            if let Some(Selection::Contact(id)) = self.inspected
                && let Some(c) = view.contacts.iter().find(|c| c.id == id) { contact_details(ui, view, c); }
            ui.separator();
            ui.small("Space: pause · F: fit · Drag: pan · Scroll: zoom");
            ui.small("Sensor reports travel at light speed. All target positions are estimates.");
            ui.collapsing("Simulation parameters", |ui| {
                for p in params::ALL { ui.small(format!("{} = {} {}", p.key, p.value, p.unit)).on_hover_text(p.note); }
            });
        });
    }

    fn weapons_panel(&mut self, ui: &mut egui::Ui, view: &View) {
        let Some(b) = view.bodies.iter().find(|b| b.controllable && b.armed && Some(b.faction) == self.own_faction()) else { return };
        // Selection is explicit and shared by the map, list and weapons. No silent
        // fallback to a different contact when the selected track disappears.
        let mut target = match self.inspected { Some(Selection::Contact(id)) => Some(id), _ => None };
        let contact = target.and_then(|id| view.contacts.iter().find(|c| c.id == id));
        let fireable = contact.is_some_and(|c| c.track.is_some() && !c.stale);
        let launchable = contact.is_some() && b.damage.operating_effectiveness(System::Launcher) > 0.0;

        let count = format!("{} HELD", view.contacts.len());
        section(ui, "TARGET", Some((count.as_str(), TEXT_MUTED)));
        card(ui, if contact.is_some() { CONTACT } else { EDGE }, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(EVec2::splat(18.0), Sense::hover());
                let glyph = if contact.is_some_and(|c| c.resolved_missile) { Glyph::HostileMissile } else { Glyph::Hostile };
                paint_glyph(ui.painter(), rect.center(), glyph, if contact.is_some() { CONTACT } else { TEXT_DIM });
                let solution = contact.map(|c| if fireable { ("SOLUTION", SYS_OK) } else if c.stale { ("STALE", SYS_DAMAGED) } else { ("BEARING", SYS_DAMAGED) });
                let tag_w = if solution.is_some() { 84.0 } else { 0.0 };
                let selected = contact.map(contact_label)
                    .unwrap_or_else(|| if target.is_some() { "Track lost".into() } else { "Designate target…".into() });
                egui::ComboBox::from_id_salt("weapon_target").width((ui.available_width() - tag_w).max(90.0))
                    .selected_text(egui::RichText::new(selected).monospace().size(12.0).color(if contact.is_some() { TEXT_HI } else { TEXT_MUTED }))
                    .show_ui(ui, |ui| {
                        for c in &view.contacts {
                            if ui.selectable_value(&mut target, Some(c.id), contact_label(c)).clicked() {
                                self.inspected = Some(Selection::Contact(c.id));
                            }
                        }
                    });
                if let Some((text, color)) = solution {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| { tag(ui, text, color); });
                }
            });
            let Some(c) = contact else {
                hint(ui, "Designate on the plot or from Contacts");
                return;
            };
            let (quality, quality_color) = track_quality(c);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(contact_class(c)).monospace().size(9.5).color(TEXT));
                ui.label(egui::RichText::new(quality).monospace().size(9.5).color(quality_color));
                if view.hostile_pings.iter().any(|p| p.contact == c.id) {
                    ui.label(egui::RichText::new("PINGING").monospace().size(9.5).color(DANGER));
                }
            });
            if let Some(tr) = &c.track {
                let rel = tr.pos - b.pos;
                let closure = -(tr.vel - b.vel).dot(rel.normalized());
                readouts(ui, &[
                    ("RANGE", fmt_distance(rel.length()), TEXT_HI),
                    (if closure >= 0.0 { "CLOSING" } else { "OPENING" }, fmt_speed(closure.abs()), if closure >= 0.0 { SYS_DAMAGED } else { TEXT_HI }),
                    ("ERROR 2σ", format!("±{}", fmt_distance(2.0 * sigma_major(tr.cov))), TEXT),
                    ("AGE", fmt_age((view.time - tr.updated_at).max(0.0)), TEXT),
                ]);
            } else {
                hint(ui, "No range  ·  bearing launch, seeker must acquire");
            }
            let order = b.autopilot.map(|ap| ap.order);
            ui.horizontal(|ui| {
                let w = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
                let flying_by = matches!(order, Some(Order::Flyby(InterceptTarget::Contact(id))) if id == c.id);
                let matching = matches!(order, Some(Order::Intercept(InterceptTarget::Contact(id))) if id == c.id);
                if tac_button(ui, "FLYBY", EVec2::new(w, 22.0), ACCENT, flying_by, fireable)
                    .on_hover_text("Full thrust toward the target, without approach braking").clicked()
                {
                    self.command(Command::Flyby { body: b.id, target: InterceptTarget::Contact(c.id) });
                }
                if tac_button(ui, "MATCH VELOCITY", EVec2::new(w, 22.0), ACCENT, matching, fireable)
                    .on_hover_text("Intercept and match the target's velocity").clicked()
                {
                    self.command(Command::Intercept { body: b.id, target: InterceptTarget::Contact(c.id) });
                }
            });
            rule(ui);
            let stamp = c.damage.map(|r| format!("ECHO T+ {}  ·  {}", fmt_time(r.observed_at), fmt_age((view.time - r.observed_at).max(0.0))));
            sub_header(ui, "DAMAGE ASSESSMENT", Some((stamp.as_deref().unwrap_or("NO ECHO"), if stamp.is_some() { TEXT_MUTED } else { SYS_UNKNOWN })))
                .on_hover_text("Latest confirmed active echo, not live truth. Ping to refresh.");
            damage_bars(ui, c.damage.as_ref());
            system_matrix(ui, "target", c.damage,self.theme);
        });

        section(ui, "WEAPONS", None);
        card(ui, ACCENT, |ui| {
            // Main beam.
            let directed = !b.beam_auto && b.beam_target.is_some();
            let (mode, mode_color) = if b.beam_auto { ("WEAPONS FREE", SYS_OK) } else if directed { ("DIRECTED", ACCENT) } else { ("HOLD FIRE", TEXT_MUTED) };
            sub_header(ui, "MAIN BEAM", Some((mode, mode_color)));
            ui.horizontal(|ui| {
                let w = (ui.available_width() - 2.0 * ui.spacing().item_spacing.x) / 3.0;
                if tac_button(ui, "AUTO", EVec2::new(w, 22.0), ACCENT, b.beam_auto, true)
                    .on_hover_text("Engage any track with a useful firing solution").clicked() && !b.beam_auto
                {
                    self.command(Command::ArmBeams { body: b.id });
                }
                if tac_button(ui, "DIRECT", EVec2::new(w, 22.0), ACCENT, directed && b.beam_target == target, fireable)
                    .on_hover_text("Direct the beam at the designated target").clicked()
                {
                    self.command(Command::EngageBeam { body: b.id, target: contact.map(|c| c.id) });
                }
                if tac_button(ui, "HOLD", EVec2::new(w, 22.0), ACCENT, !b.beam_auto && !directed, true)
                    .on_hover_text("Hold fire").clicked() && (b.beam_auto || directed)
                {
                    self.command(Command::EngageBeam { body: b.id, target: None });
                }
            });
            let cycle = params::SHIP_BEAM_RECHARGE_S.value;
            let recharge = (b.beam_ready_at - view.time).max(0.0);
            let can_fire = b.thermal.can_fire();
            let (charge, charge_text, charge_color) = if !can_fire {
                ((b.thermal.capacitor_j / params::BEAM_CAPACITOR_J.value).clamp(0.0, 1.0), "PWR/HEAT".to_string(), SYS_DAMAGED)
            } else if recharge > 0.0 {
                (1.0 - (recharge / cycle).clamp(0.0, 1.0), format!("{recharge:.1} s"), ACCENT.gamma_multiply(0.7))
            } else { (1.0, "READY".to_string(), ACCENT) };
            meter(ui, "CYCLE", Some(charge), &charge_text, charge_color);
            let beam_contact = b.beam_target.and_then(|id| view.contacts.iter().find(|c| c.id == id));
            let (status, status_color) = if !can_fire { ("POWER / HEAT LIMIT".to_string(), SYS_DAMAGED) }
                else if recharge > 0.0 { ("RECHARGING".into(), TEXT) }
                else if let Some(c) = beam_contact {
                    if c.stale || c.track.is_none() { ("TRACK UNAVAILABLE".into(), SYS_DAMAGED) }
                    else { (format!("ENGAGING {}", contact_label(c).to_uppercase()), SYS_OK) }
                } else if b.beam_auto { ("READY  ·  SEEKING SOLUTION".into(), TEXT) } else { ("WEAPONS HELD".into(), TEXT_MUTED) };
            status_line(ui, &status, status_color);
            let band = params::SHIP_BEAM_AUTO_RANGE_LS.value;
            let range_ls = contact.and_then(|c| c.track.as_ref()).map(|tr| (tr.pos - b.pos).length() / LIGHT_SECOND);
            readouts(ui, &[
                ("PULSE", format!("{:.0} TJ", params::SHIP_BEAM_ENERGY_J.value / 1e12), TEXT),
                ("CYCLE", format!("{cycle:.0} s"), TEXT),
                ("CLOSE BAND", format!("{band:.0} ls"), TEXT),
                match range_ls {
                    Some(r) if r <= band => ("TARGET · CLOSE", format!("{r:.1} ls"), SYS_OK),
                    Some(r) => ("TARGET · EXT", format!("{r:.1} ls"), ACCENT),
                    None => ("TARGET", "—".into(), TEXT_DIM),
                },
            ]).on_hover_text(format!("Automatic fire engages freely inside {band:.0} ls. Beyond it the beam still fires whenever predicted \
                energy on target is useful, so predictable, well-tracked targets can be hit much farther out. \
                Directed fire has no range cutoff. Repeats every {cycle:.0} s with a fresh track and available power."));

            // Offensive missiles.
            rule(ui);
            let queued = b.missile_queued.iter().sum::<u32>();
            let reload = b.missile_ready_at[self.payload.index()] - view.time;
            let (state, state_color) = if b.damage.operating_effectiveness(System::Launcher) <= 0.0 { ("LAUNCHER OFFLINE", SYS_DESTROYED) }
                else if queued > 0 { ("SALVO QUEUED", WARM) } else if reload > 0.0 { ("RELOADING", TEXT_MUTED) } else { ("LAUNCHER READY", SYS_OK) };
            sub_header(ui, "MISSILES", Some((state, state_color)));
            ui.horizontal(|ui| {
                let w = (ui.available_width() - 2.0 * ui.spacing().item_spacing.x) / 3.0;
                for p in Payload::ALL {
                    let i = p.index();
                    let available = b.magazine[i].saturating_sub(b.missile_queued[i]);
                    if payload_tile(ui, w, self.theme.weapon(p), available, b.missile_queued[i], self.payload == p)
                        .on_hover_text(match p { Payload::Kinetic => "SRM: short-range kinetic shotgun", Payload::Nuclear => "LRM: long-range nuclear-pumped laser", Payload::Beam => "Ship beam" })
                        .clicked()
                    {
                        self.payload = p;
                    }
                }
            });
            let ammo = b.magazine[self.payload.index()].saturating_sub(b.missile_queued[self.payload.index()]);
            let label = if ammo == 0 { "MAGAZINE EMPTY".into() } else { format!("LAUNCH  ·  {}", self.theme.weapon(self.payload)) };
            let armed = launchable && ammo > 0;
            if tac_button(ui, &label, EVec2::new(ui.available_width(), 30.0), WARM, armed, armed)
                .on_hover_text("Each click queues one missile. Shared launch rate: one every sixty seconds.").clicked()
                && let Some(c) = contact
            {
                self.command(Command::Launch { body: b.id, target: c.id, payload: self.payload });
            }
            if queued > 0 {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("{queued} QUEUED  ·  1/60s")).monospace().size(9.5).color(WARM));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if tac_button(ui, "CANCEL QUEUE", EVec2::new(96.0, 18.0), WARM, false, true).clicked() {
                            self.command(Command::CancelLaunches { body: b.id });
                        }
                    });
                });
            } else if contact.is_none() { hint(ui, "Designate a target to launch"); }
            else if !launchable { status_line(ui, "LAUNCHER OFFLINE", SYS_DESTROYED); }
            else if !fireable { status_line(ui, "UNCERTAIN SHOT  ·  SEEKER MUST ACQUIRE", SYS_DAMAGED); }
            else if reload > 0.0 { status_line(ui, &format!("RELOADING  ·  {reload:.1} s"), TEXT); }
            else { status_line(ui, "READY  ·  1 PER SECOND", SYS_OK); }

            // Point defence.
            rule(ui);
            sub_header(ui, "POINT DEFENCE", Some(("AUTO", SYS_OK)));
            let battery = b.interceptor_battery;
            let rounds = battery.map_or(0, |battery| battery.rounds);
            readouts(ui, &[
                ("INTERCEPTORS", rounds.to_string(), if rounds > 0 { TEXT_HI } else { SYS_DESTROYED }),
                ("LAUNCHED", battery.map_or("—".into(), |battery| battery.launched.to_string()), TEXT),
                ("LASER", format!("{} ls", params::PD_LASER_MAX_RANGE_LS.value), TEXT),
            ]).on_hover_text("Grey ring: the larger weapon envelope. Interceptors: 60% at low relative speed, reduced at high closure; \
                actual reach depends on motion. When empty, shows the laser's 75% per-shot radius. Laser is last ditch only.");
            if rounds == 0 { status_line(ui, "INTERCEPTORS EMPTY  ·  LASER ONLY", SYS_DAMAGED); }
            else if let Some(battery) = battery { status_line(ui, &battery.status.to_uppercase(), TEXT); }
        });
    }

    fn body_details(&mut self, ui: &mut egui::Ui, view: &View, b: &BodyView) {
        ui.separator();
        ui.heading(&b.name);
        if b.controllable && b.kind==BodyKind::Ship && b.probes>0 {
            ui.label(format!("Reconnaissance probes: {}",b.probes));
            for c in &view.contacts {
                let direction=c.track.as_ref().map(|tr|(tr.pos-b.pos).normalized()).or_else(||
                    c.bearings.iter().max_by(|a,b| a.emitted_at.total_cmp(&b.emitted_at)).map(|bearing|Vec2::new(bearing.bearing.cos(),bearing.bearing.sin())));
                if let Some(direction)=direction
                    && ui.button(format!("Send probe toward {}",c.id)).on_hover_text("Launch a weaker sensor platform: 500g fixed-heading burn, then coast. Reports take light time to reach you. No automatic target identity.").clicked() {
                    self.command(Command::DeployProbe {body:b.id,direction});
                }
            }
        }
        ui.weak(format!("Power {:.0}% · heat {} · screens {:.0}%",100.0*b.thermal.capacitor_j/b.thermal.capacitor_capacity(),fmt_energy(b.thermal.heat_j),100.0*b.damage.screen_available));
        ui.weak(format!("Thermal emission {:.2} TW · emissivity {:.2}×",b.thermal.emission()/1e12,b.emissivity.value()));
        if let Some(pd)=b.point_defence {
            ui.label(format!("Point defence: {} lasers · {:.1}/s each · {} shots",pd.lasers,pd.rate_hz,pd.shots));
            ui.weak("PD lasers: 3 LS range; approximately 50% stopped over a full unsaturated approach, across repeated shots.");
            if b.controllable {
                let interceptors=b.interceptor_battery.is_some_and(|battery|battery.rounds>0) && b.damage.operating_effectiveness(luminal_core::damage::System::PdMissiles)>0.0;
                let radius=luminal_core::world::point_defence::defence_ring_radius(pd.rate_hz>0.0,interceptors);
                ui.weak(format!("Grey circle: {} · {}",fmt_distance(radius),
                    if interceptors {"interceptor envelope"} else {"laser 75% radius"}))
                    .on_hover_text("Best available weapon envelope. Interceptor odds start at 60% and degrade with encounter speed; reach depends on target motion.");
            }
        }
        if let Some(battery)=b.interceptor_battery {
            ui.label(format!("Defensive missiles: {} remaining · {} launched · automatic",battery.rounds,battery.launched));
            ui.weak("Launches only inside calculated short-range intercept envelope.");
        }
        ui.label(format!("{} · {:?}", faction_name(b.faction), b.kind));
        if let Some(m) = b.missile {
            let phase = match m.phase {
                Phase::Burn => "burn",
                Phase::Cruise => "cruise",
                Phase::Terminal => "terminal (own seeker)",
            };
            ui.label(format!("{} missile → {} · {phase} · {:.0} km/s delta-v left", m.payload.name(), m.target, m.dv_left));
        }
        if let Some(nearest) = view
            .celestials
            .iter()
            .map(|c| (c, (c.pos - b.pos).length() - c.radius))
            .min_by(|a, b| a.1.total_cmp(&b.1))
        {
            let rel = b.vel - view.system.state(view.celestials.iter().position(|c| c.name == nearest.0.name).unwrap(), view.time).vel;
            ui.label(format!("{} relative to {}, {} above surface", fmt_speed(rel.length()), nearest.0.name, fmt_distance(nearest.1)));
        }
        ui.label(format!("Speed {} (system frame)", fmt_speed(b.vel.length())));
        ui.label(format!("Thrust {:.2} g", b.thrust.length() / G0));
        let forecast = view.system.predict(State { pos: b.pos, vel: b.vel }, b.thrust, view.time, FORECAST_S, 2);
        if let Some((i, t)) = forecast.impact {
            ui.colored_label(DANGER, format!("⚠ On course to hit {} in {}", view.celestials[i].name, fmt_age(t - view.time)));
        }
        if b.avoidance.impossible {
            ui.colored_label(DANGER, "⚠ Collision unavoidable: burning to delay it");
        } else if b.avoidance.active {
            ui.colored_label(Color32::from_rgb(255, 150, 60), "⚠ Collision avoidance overriding orders");
        }
        if let Some(ap) = b.autopilot {
            let what = match ap.order {
                Order::Alongside {target,..}=>format!("alongside {target} · 1 LS"),
                Order::Follow {target,..}=>format!("escort {} · 1 LS alongside / 10 LS screen",view.bodies.iter().find(|b|b.id==target).map_or("?",|b|b.name.as_str())),
                Order::Route=>format!("fly-through route · {} points remaining",b.route.as_ref().map_or(0,|r|r.points.len().saturating_sub(r.progress.floor() as usize+1))),
                Order::Orbit { celestial, radius, .. } => {
                    format!("orbit {} at {} altitude", view.celestials[celestial].name, fmt_distance(radius - view.celestials[celestial].radius))
                }
                Order::Intercept(InterceptTarget::Own(o)) => {
                    format!("join {}", view.bodies.iter().find(|x| x.id == o).map_or("?".into(), |x| x.name.clone()))
                }
                Order::Intercept(InterceptTarget::Contact(c)) => format!("intercept {c}"),
                Order::Flyby(InterceptTarget::Own(o)) => format!("fly by {}", view.bodies.iter().find(|x| x.id == o).map_or("?", |x| x.name.as_str())),
                Order::Flyby(InterceptTarget::Contact(c)) => format!("fly by {c}"),
                Order::CombatRange(_,standoff)=>if standoff {"standoff".into()} else {"close to beam range".into()},
                Order::KeepRange(_,range)=>format!("hold range {}",fmt_distance(range)),
                Order::Evade(_)=>"evade · incoming missiles / coast when clear".into(),
                Order::MoveTo { frame, .. } => format!("move and stop (frame: {})", view.celestials[frame].name),
            };
            let status = match ap.status {
                AutopilotStatus::Manoeuvring => "manoeuvring".to_string(),
                AutopilotStatus::Closing { eta, range } => format!("{} to go, ETA ~{}", fmt_distance(range), fmt_age(eta)),
                AutopilotStatus::Holding => "holding".into(),
                AutopilotStatus::Passed => "flyby complete, coasting".into(),
                AutopilotStatus::NoTrack => "target track lost, coasting".into(),
            };
            ui.label(format!("Autopilot: {what} ({status})"));
            if self.own_faction() == Some(b.faction) && b.controllable
                && let Order::Intercept(target) | Order::Flyby(target) = ap.order
            {
                let mut match_velocity = matches!(ap.order, Order::Intercept(_));
                if ui.checkbox(&mut match_velocity, "Intercept and match velocity").changed() {
                    self.command(if match_velocity { Command::Intercept { body: b.id, target } } else { Command::Flyby { body: b.id, target } });
                }
            }
        }
        if self.own_faction() == Some(b.faction) && b.controllable {
            let max_g = match b.kind {
                BodyKind::Ship => params::SHIP_MAX_ACCEL_G.value,
                BodyKind::Station => 0.0,
                BodyKind::Probe => params::PROBE_MAX_ACCEL_G.value,
                BodyKind::Missile => params::MISSILE_MAX_ACCEL_G.value,
            };
            let mut limit_g = if b.drive_limit.is_finite() { b.drive_limit / G0 } else { max_g };
            if ui
                .add(egui::Slider::new(&mut limit_g, 0.01..=max_g).logarithmic(true).text("g drive limit"))
                .on_hover_text("Caps thrust for all orders. Drive emission scales with thrust, so lower is quieter.")
                .changed()
            {
                self.command(Command::SetDriveLimit { body: b.id, g: limit_g });
            }
            ui.label("Right-click: space to move and stop; a celestial body to orbit; a ship or tracked contact for a full-thrust flyby. Toggle velocity matching above to rendezvous.");
            ui.horizontal(|ui| {
                if ui.button("All stop").on_hover_text("Brake to rest in the local frame").clicked() {
                    self.command(Command::AllStop { body: b.id });
                }
                if ui.button(if b.autopilot.is_some() { "Cancel order" } else { "Cut thrust" }).on_hover_text("Stop thrusting and coast").clicked() {
                    self.command(Command::SetThrust { body: b.id, thrust: Vec2::ZERO });
                }
            });
            if b.kind == BodyKind::Ship {
                ui.label(if b.armed {
                    format!("Missiles: SRM {} · LRM {}", b.magazine[0], b.magazine[1])
                } else if b.point_defence.is_some() { "Point defence only".into() } else { "Unarmed".into() });
            }
            ui.weak(format!("Sensors: passive {} · active {} · direction finding {}",if b.sensors.passive {"fitted"} else {"absent"},if b.sensors.active {"fitted"} else {"absent"},if b.sensors.direction_finding {"fitted"} else {"absent"}));
            if ui.add_enabled(b.sensors.active,egui::Button::new("Ping")).on_hover_text("Send one active pulse. White ring shows round-trip detection range, fading at 5 AU (reduced by sensor damage). The pulse exposes you at 10× passive/direction-finding range after light travel time.").clicked() {
                self.command(Command::Ping { body: b.id });
            }
            if b.kind == BodyKind::Ship {
                let mut up = b.screen_up;
                if ui.add_enabled(b.has_screen,egui::Checkbox::new(&mut up, "Screen up")).on_hover_text("Charges 0.2% per minute. Hits reduce capacity and heat the ship. Off disables absorption immediately.").changed() {
                    self.command(Command::SetScreen { body: b.id, up });
                }
                ui.label(format!(
                    "Screen {:.0} % full · hull {:.0} % damaged",
                    100.0 * b.damage.screen_available,
                    100.0 * b.hull_j / params::HULL_INTEGRITY_J.value
                ));
            }
        }
    }

    fn draw_navigation_readout(&self, painter: &egui::Painter, rect: Rect, view: &View) {
        let Some(Selection::Body(id)) = self.selected else { return };
        let Some(ship) = view.bodies.iter().find(|b| b.id == id) else { return };
        let mut lines = vec![format!("SPEED  {}    THRUST  {:.0}g", fmt_speed(ship.vel.length()), ship.thrust.length()/G0)];
        if let Some(Selection::Contact(id)) = self.inspected
            && let Some(contact) = view.contacts.iter().find(|c| c.id == id) {
            if let Some(track) = &contact.track {
                let rel = track.pos-ship.pos;
                let closing = -(track.vel-ship.vel).dot(rel.normalized());
                lines.push(format!("RANGE  {}    {}", fmt_distance(rel.length()), contact_label(contact)));
                lines.push(if contact_has_course(contact) {
                    format!("{}  {} · ESTIMATED", if closing>=0.0 {"CLOSING"} else {"OPENING"}, fmt_speed(closing.abs()))
                } else {"CLOSURE UNKNOWN · BEARING / POSITION ONLY".into()});
            } else { lines.push("RANGE / CLOSURE UNKNOWN · BEARING ONLY".into()); }
        } else { lines.push("NO TARGET DESIGNATED".into()); }
        let panel = Rect::from_min_size(Pos2::new(rect.left()+10.0, rect.bottom()-85.0), EVec2::new(310.0,75.0));
        painter.rect_filled(panel,0.0,PANEL_BG.gamma_multiply(0.9));
        painter.line_segment([panel.left_top(),panel.left_bottom()],Stroke::new(2.0,ACCENT));
        if let Some(jump)=ship.jump {
            lines=match jump {
                JumpState::Spooling {depart_at,..}=>vec![format!("Spooling for jump · {}",fmt_time((depart_at-view.time).max(0.0))),"THRUST / SCREENS / LASERS OFFLINE".into(),"Cancel jump in helm controls".into()],
                JumpState::Transit {arrive_at,..}=>vec![format!("JUMP TRANSIT · {:.1}s to arrival",(arrive_at-view.time).max(0.0)),"1 AU/s · normal-space velocity preserved".into()],
            };
        }
        let heading=self.manual_flight.filter(|m|m.body==ship.id && ship.autopilot.is_none())
            .map_or("NAVIGATION / FIRING SOLUTION".into(),|m|format!("FREE FLIGHT · {:.0}% · ← → TURN / ↑ ↓ THROTTLE",m.throttle*100.0));
        painter.text(panel.min+EVec2::new(10.0,7.0),egui::Align2::LEFT_TOP,heading,egui::FontId::monospace(9.0),ACCENT);
        for (i,line) in lines.iter().enumerate() {
            painter.text(panel.min+EVec2::new(10.0,24.0+i as f32*15.0),egui::Align2::LEFT_TOP,line,egui::FontId::monospace(10.0),TEXT_HI);
        }
    }

    fn update_player_tracking(&mut self,view:&View) {
        if self.track_player && let Some(ship)=view.bodies.iter().find(|b|b.controllable && b.kind==BodyKind::Ship) {
            self.camera.center=ship.pos;
        }
    }

    fn update_tracking_zoom(&mut self,view:&View,rect:Rect,dt:f64) {
        self.tracking_zoom_hold=(self.tracking_zoom_hold-dt).max(0.0);
        if !self.track_player || self.tracking_zoom_hold>0.0 || view.time<self.manual_ping_zoom_until {return;}
        let Some(ship)=view.bodies.iter().find(|b|b.controllable && b.kind==BodyKind::Ship) else {return;};
        let selected=self.inspected.map(|s|match s {
            Selection::Body(id)=>InterceptTarget::Own(id),Selection::Contact(id)=>InterceptTarget::Contact(id),
        });
        let primary=selected.and_then(|target|tracking_zoom_scale(view,ship.pos,rect,target,self.session.camera_target_of(self.role,target)));
        // New localized enemies must fit even while inspecting our charge or own
        // ship. Use received estimates; a bearing alone has no drawable range.
        let enemies=view.contacts.iter().filter(|c|!c.stale && !c.resolved_missile
            && !matches!(c.resolved_kind,Some(BodyKind::Station|BodyKind::Probe)))
            .filter_map(|c|tracking_zoom_scale(view,ship.pos,rect,InterceptTarget::Contact(c.id),None));
        let Some(desired)=primary.into_iter().chain(enemies).reduce(f64::max) else {return;};
        let current=self.camera.km_per_px;
        // Hysteresis prevents sensor noise from making the camera breathe.
        if desired<=current && desired>=current*0.8 {return;}
        // Reveal threats promptly; zoom back in more gently.
        let tau=if desired>current {1.0} else {10.0/3.0};
        let alpha=1.0-(-dt.min(0.1)/tau).exp();
        self.camera.km_per_px=(current.ln()+(desired.ln()-current.ln())*alpha).exp().clamp(1e-3,1e8);
    }

    fn map(&mut self, ui: &mut egui::Ui, view: &View, overlay: Option<&(View, BTreeMap<ContactId, BodyId>)>) {
        let rect = ui.available_rect_before_wrap();
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, BACKGROUND);

        if self.fit_pending {
            if !self.opening_fit {self.track_player=false;}
            let safe = Rect::from_min_max(rect.min + EVec2::new(0.0, 150.0_f32.min(rect.height()*0.25)), rect.max - EVec2::new(0.0, 85.0));
            self.fit(view, safe);
            self.camera.center.y += (safe.center().y-rect.center().y) as f64*self.camera.km_per_px;
            self.fit_pending = false;
            self.opening_fit = false;
        }

        // Pan and zoom about the pointer.
        if resp.dragged_by(egui::PointerButton::Primary) || resp.dragged_by(egui::PointerButton::Middle) {
            self.track_player=false;
            let d = resp.drag_delta();
            self.camera.center = self.camera.center - Vec2::new(d.x as f64, -d.y as f64) * self.camera.km_per_px;
        }
        if let Some(hover) = resp.hover_pos() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y) as f64;
            if scroll != 0.0 {
                self.tracking_zoom_hold=10.0;
                let before = to_world(&self.camera, rect, hover);
                self.camera.km_per_px = (self.camera.km_per_px * (-scroll * 0.002).exp()).clamp(1e-3, 1e8);
                let after = to_world(&self.camera, rect, hover);
                self.camera.center = self.camera.center + (before - after);
            }
        }

        if !self.track_player && !resp.dragged() {
            if let Some(pointer)=resp.hover_pos() {
                let delta=edge_pan_delta(rect,pointer,ui.input(|i|i.stable_dt) as f64);
                self.camera.center=self.camera.center+delta*self.camera.km_per_px;
            }
        }
        self.update_player_tracking(view);
        self.update_tracking_zoom(view,rect,ui.input(|i|i.stable_dt) as f64);
        let cam = self.camera;
        let mut labels = Labels::default();
        if self.track_player {painter.text(rect.center_top()+EVec2::new(0.0,12.0),egui::Align2::CENTER_TOP,"TRACKING OWN SHIP · T",mono(10.0),ACCENT);}
        draw_point_grid(&painter, &cam, rect);
        draw_orbits(&painter, &cam, rect, view);

        // Stellar shadows are cosmetic. Sensor occlusion remains in the core.
        if let Some(star)=view.celestials.first() {
            for c in view.celestials.iter().skip(1) {draw_shadow(&painter,&cam,rect,star.pos,star.radius,c.pos,c.radius);}
        }

        if let Some(o) = &view.objective {
            let center = to_screen(&cam, rect, o.center);
            let r = ((o.radius / cam.km_per_px) as f32).max(6.0);
            let col = Color32::from_rgb(120, 220, 140);
            painter.circle_filled(center, r, col.gamma_multiply(0.06));
            let pts: Vec<Pos2> = (0..=96)
                .map(|k| {
                    let a = std::f32::consts::TAU * k as f32 / 96.0;
                    center + EVec2::new(a.cos(), a.sin()) * r
                })
                .collect();
            painter.extend(Shape::dashed_line(&pts, Stroke::new(1.0, col.gamma_multiply(0.7)), 8.0, 5.0));
            let label = if o.prize.is_some() || o.wipe {o.name.clone()} else {
                format!("{} (goal: {})", o.name, view.bodies.iter().find(|b| b.id == o.protect).map_or("the transport".into(), |b| b.name.clone()))
            };
            if rect.contains(center) {
                labels.add(center + EVec2::new(r * 0.7 + 4.0, -r * 0.7), label, col);
            } else {
                let origin=view.bodies.iter().find(|b|b.controllable && Some(b.faction)==self.own_faction()).map_or(cam.center,|b|b.pos);
                let label=edge_distance_label(&label,(o.center-origin).length());
                draw_edge_marker(&painter, rect, center, &label, col, &mut labels);
            }
        }

        // Ping is a property of the fused contact, not a second plotted estimate.

        self.jump_effects.draw(&painter,&cam,rect,view,ui.input(|i|i.time));

        // Observed combat flashes only: enemy effects arrive after light travel.
        for e in &view.combat {
            use luminal_core::world::CombatKind;
            if matches!(e.kind,CombatKind::FireControlDisrupted|CombatKind::JumpSpool|CombatKind::JumpCancelled|CombatKind::JumpDeparture|CombatKind::JumpArrival|CombatKind::WithdrawalStarted|CombatKind::WithdrawalCancelled|CombatKind::Withdrawn|CombatKind::Surrendered|CombatKind::Destroyed|CombatKind::Expended|CombatKind::Impact|CombatKind::MissileHit|CombatKind::MissileMiss|CombatKind::NuclearBurst) {continue;}
            let age=(view.time-e.received_at).max(0.0);
            let Some(pos)=e.pos else {continue};
            let p=to_screen(&cam,rect,pos);
            if matches!(e.kind,CombatKind::BeamPulse|CombatKind::SpinalPulse|CombatKind::PointDefence|CombatKind::InterferencePulse) {
                let spinal=e.kind==CombatKind::SpinalPulse;
                let burst=matches!(e.weapon_visual,luminal_core::world::weapon_fit::WeaponVisual::Pulse(_));
                let duration=if spinal {1.0_f64.max(view.warp*0.45)} else if burst {0.75_f64.max(view.warp*0.6)} else {0.5_f64.max(view.warp*0.2)};
                if age>=duration {continue;}
                if let Some(aim)=e.aim {
                    let start=e.own_body.and_then(|id|view.bodies.iter().find(|b|b.id==id).map(|b|b.pos))
                        .or_else(||e.contact.and_then(|id|view.contacts.iter().find(|c|c.id==id).and_then(|c|c.track.as_ref()).map(|t|t.pos))).unwrap_or(pos);
                    let end=e.target.and_then(|target|match target {
                        InterceptTarget::Own(id)=>view.bodies.iter().find(|b|b.id==id).map(|b|b.pos),
                        InterceptTarget::Contact(id)=>view.contacts.iter().find(|c|c.id==id).and_then(|c|c.track.as_ref()).map(|t|t.pos),
                    }).unwrap_or(aim);
                    let alpha=(1.0-age/duration) as f32;
                    let line=[to_screen(&cam,rect,start),to_screen(&cam,rect,end)];
                    if spinal {
                        for (width,strength) in [(18.0,0.08),(11.0,0.18),(5.0,0.95)] {
                            painter.line_segment(line,Stroke::new(width,Color32::from_rgb(255,215,35).gamma_multiply(alpha*strength)));
                        }
                        painter.line_segment(line,Stroke::new(1.8,Color32::from_rgb(255,250,185).gamma_multiply(alpha)));
                        painter.circle_filled(line[0],7.0*alpha,Color32::from_rgb(255,235,95).gamma_multiply(alpha));
                    } else {
                        if e.kind==CombatKind::InterferencePulse {
                            painter.line_segment(line,Stroke::new(3.0,Color32::from_rgb(185,110,255).gamma_multiply(alpha*0.7)));
                            painter.circle_stroke(line[0],6.0+12.0*(1.0-alpha),Stroke::new(1.5,Color32::from_rgb(185,110,255).gamma_multiply(alpha)));
                        } else {
                            let pulses=match e.weapon_visual {luminal_core::world::weapon_fit::WeaponVisual::Pulse(n)=>n,_=>1};
                            let phase=(age/duration*pulses as f64).fract() as f32;
                            let brightness=if pulses>1 {if phase<0.65 {1.0} else {0.12}} else {1.0};
                            painter.line_segment(line,Stroke::new(if pulses>1 {2.3} else {1.5},Color32::from_rgb(150,225,255).gamma_multiply(alpha*brightness)));
                        }
                    }
                }
            } else {
                let duration=20.0_f64.max(view.warp*0.3);
                if age>=duration {continue;}
                let progress=(age/duration) as f32;
                let color=if e.kind==CombatKind::NuclearBurst {Color32::from_rgb(255,230,40)} else {Color32::from_rgb(255,120,80)};
                painter.circle_stroke(p,6.0+40.0*progress,Stroke::new(2.0,color.gamma_multiply(1.0-progress)));
            }
        }

        self.weapon_effects.draw(&painter,&cam,rect,ui.input(|i|i.time));

        // Round-trip range, not the outbound light front. Anchor at emission.
        for front in &view.pings {
            let range = (view.time - front.t_emit).max(0.0) * LIGHT_SECOND / 2.0;
            let opacity = ((1.0 - range / front.useful_range) / 0.3).clamp(0.0, 1.0) as f32;
            if opacity > 0.0 {
                painter.circle_stroke(to_screen(&cam, rect, front.origin), (range / cam.km_per_px) as f32,
                    Stroke::new(1.5, Color32::WHITE.gamma_multiply(opacity)));
            }
        }

        if let Some((_, pos)) = resp.hover_pos().and_then(|pointer| selectable_at(view, &cam, rect, pointer)) {
            let p = to_screen(&cam, rect, pos);
            let yellow = Color32::from_rgb(255, 220, 60);
            painter.circle_filled(p, 14.0, yellow.gamma_multiply(0.24));
            painter.circle_stroke(p, 14.0, Stroke::new(1.0, yellow));
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        for (i, c) in view.celestials.iter().enumerate() {
            // Reveal satellite labels when zoomed into their parent system,
            // rather than stacking every moon on the planet's edge marker.
            if c.kind == CelestialKind::Moon
                && let Orbit::Frozen {radius, ..} | Orbit::Circular {radius, ..} = view.system.bodies[i].orbit
                && radius / cam.km_per_px < 12.0 {continue;}
            let p = to_screen(&cam, rect, c.pos);
            let min_px = if c.kind == CelestialKind::Star { 6.0 } else { 3.0 };
            let r = ((c.radius / cam.km_per_px) as f32).max(min_px);
            let col=if c.kind==CelestialKind::Star {self.theme.star_color()} else {celestial_color(c.kind)};
            if rect.expand(r*2.2).contains(p) {
                self.celestial_art.draw(ui.ctx(),&painter,view,i,self.theme,p,r,ui.input(|i|i.time));
            }
            if rect.contains(p) {
                let labeled=c.kind!=CelestialKind::Star && draw_orbit_label(&painter,&cam,rect,view,i,p,col);
                if !labeled {
                    let text=if c.kind==CelestialKind::Star {c.name.clone()} else {celestial_distance_label(view,i)};
                    labels.add(p + EVec2::new(r + 4.0, -r - 2.0), text, col.gamma_multiply(0.8));
                }
            } else {
                let origin=view.bodies.iter().find(|b|b.controllable && Some(b.faction)==self.own_faction()).map_or(cam.center,|b|b.pos);
                let label=edge_distance_label(&c.name,(c.pos-origin).length());
                draw_edge_marker(&painter, rect, p, &label, col, &mut labels);
            }
        }

        // Contacts: estimates with uncertainty, or bare bearing lines.
        for c in &view.contacts {
            let selected = self.inspected == Some(Selection::Contact(c.id));
            if let Some(track)=&c.track {
                draw_hit_bloom(&painter,to_screen(&cam,rect,track.pos),CONTACT,view,None,Some(c.id));
            }
            draw_contact(&painter, &cam, rect, view, c, CONTACT, selected, &mut labels);
        }

        // Standing orders: target orbits and intercept lines.
        for b in &view.bodies {
            if self.own_faction()==Some(b.faction) && !b.controllable { continue; }
            let Some(ap) = b.autopilot else { continue };
            let c = body_color(b, self.own_faction()).gamma_multiply(0.5);
            match ap.order {
                Order::Route=>{
                    if let Some(route)=&b.route {
                        let end=(route.points.len()-1) as f64;
                        let samples=((end-route.progress)*32.0).ceil().max(1.0) as usize;
                        let points=(0..=samples).map(|i|to_screen(&cam,rect,
                            route.sample(route.progress+(end-route.progress)*i as f64/samples as f64))).collect();
                        painter.add(Shape::line(points,Stroke::new(1.0,ACCENT.gamma_multiply(0.55))));
                        for (i,point) in route.points.iter().enumerate().skip(route.progress.floor() as usize+1) {
                            let at=to_screen(&cam,rect,*point);
                            painter.circle_stroke(at,4.0,Stroke::new(1.0,ACCENT));
                            painter.text(at+EVec2::new(6.0,-6.0),egui::Align2::LEFT_BOTTOM,i.to_string(),mono(9.0),ACCENT);
                        }
                    }
                }
                Order::Orbit { celestial, radius, .. } => {
                    let center = view.celestials[celestial].pos;
                    let pts: Vec<Pos2> = (0..=96)
                        .map(|k| {
                            let a = std::f64::consts::TAU * k as f64 / 96.0;
                            to_screen(&cam, rect, center + Vec2::new(a.cos(), a.sin()) * radius)
                        })
                        .collect();
                    painter.extend(Shape::dashed_line(&pts, Stroke::new(1.0, c), 6.0, 4.0));
                }
                Order::MoveTo { frame, offset } => {
                    let to = view.celestials[frame].pos + offset;
                    let (ps, pt) = (to_screen(&cam, rect, b.pos), to_screen(&cam, rect, to));
                    painter.extend(Shape::dashed_line(&[ps, pt], Stroke::new(1.0, c), 6.0, 4.0));
                    painter.circle_stroke(pt, 5.0, Stroke::new(1.0, c));
                    painter.line_segment([pt - EVec2::new(8.0, 0.0), pt + EVec2::new(8.0, 0.0)], Stroke::new(1.0, c));
                    painter.line_segment([pt - EVec2::new(0.0, 8.0), pt + EVec2::new(0.0, 8.0)], Stroke::new(1.0, c));
                }
                Order::Alongside {target,..}=>{
                    if let Some(pos)=view.contacts.iter().find(|c|c.id==target).and_then(|c|c.track.as_ref()).map(|t|t.pos) {draw_target_link(&painter,&cam,rect,b.pos,pos);}
                }
                Order::Follow {target:target_id,..} => {
                    if let Some(target)=view.bodies.iter().find(|b|b.id==target_id) {
                        draw_target_link(&painter,&cam,rect,b.pos,target.pos);
                    }
                }
                Order::Intercept(target) | Order::Flyby(target) | Order::KeepRange(target,_) | Order::CombatRange(target,_) | Order::Evade(target) => {
                    let to = match target {
                        InterceptTarget::Own(o) => view.bodies.iter().find(|x| x.id == o).map(|x| x.pos),
                        InterceptTarget::Contact(ci) => view.contacts.iter().find(|x| x.id == ci).and_then(|x| x.track.as_ref()).map(|t| t.pos),
                    };
                    if let Some(to) = to {
                        draw_target_link(&painter,&cam,rect,b.pos,to);
                    }
                }
            }
        }

        // Only the player's ship gets weapon envelopes; no emission or PD discs.
        for b in view.bodies.iter().filter(|b| b.controllable && Some(b.faction)==self.own_faction()) {
            draw_weapon_ranges(&painter,&cam,rect,b,self.theme);
        }

        // Own (or, for the spectator, all) ships.
        for b in &view.bodies {
            let c = body_color(b, self.own_faction());
            let selected = self.selected == Some(Selection::Body(b.id));
            let launch_pos=self.weapon_effects.launch_position(b,view,ui.input(|i|i.time));
            let p = to_screen(&cam, rect, launch_pos.unwrap_or(b.pos));
            if launch_pos.is_some() {
                painter.circle_filled(p,7.0,c.gamma_multiply(0.12));
                painter.circle_filled(p,3.5,Color32::from_rgb(255,215,140).gamma_multiply(0.65));
            }
            if b.kind!=BodyKind::Missile {draw_hit_bloom(&painter,p,c,view,Some(b.id),None);}
            if b.kind == BodyKind::Missile {
                if b.interceptor.is_some() {painter.circle_filled(p,2.0,c);}
                else {draw_missile(&painter, p, c, selected);}
            } else if b.kind == BodyKind::Station {
                painter.rect_filled(Rect::from_center_size(p,EVec2::splat(7.0)),0.0,c);
            } else {
                // Only the player's command ship needs a route forecast. Allied
                // autonomous platforms remain visible without map-spanning trails.
                if !matches!(b.jump,Some(JumpState::Transit {..})) && (self.own_faction()!=Some(b.faction) || b.controllable) {
                    let forecast = view.system.predict(State { pos: b.pos, vel: b.vel }, b.thrust, view.time, FORECAST_S, 480);
                    let pts: Vec<Pos2> = forecast.points.iter().map(|&p| to_screen(&cam, rect, p)).collect();
                    painter.extend(fading_path(&pts,c.gamma_multiply(0.3),6.0));
                    if forecast.impact.is_some()
                        && let Some(&end) = pts.last()
                    {
                        draw_cross(&painter, end, DANGER);
                    }
                }
                let heading = self.ship_headings.entry(b.id).or_insert_with(|| {
                    if b.vel.length() > 0.01 {b.vel.normalized()} else {Vec2::new(0.0,1.0)}
                });
                *heading=b.heading;
                if b.kind==BodyKind::Ship {draw_burn_vector(&painter,p,*heading,b.thrust.length()/G0,1.0);}
                draw_ship(&painter, p, b.vel, *heading, c, selected);
                if matches!(b.jump,Some(JumpState::Spooling {..})) && ui.input(|i|i.time).rem_euclid(1.0)<0.6 {
                    let triangle=vec![p+EVec2::new(0.0,-24.0),p+EVec2::new(22.0,17.0),p+EVec2::new(-22.0,17.0)];
                    painter.add(Shape::closed_line(triangle,Stroke::new(2.0,ACCENT)));
                }
                if matches!(b.jump,Some(JumpState::Transit {..})) {painter.text(p+EVec2::new(0.0,22.0),egui::Align2::CENTER_TOP,"JUMP TRANSIT",egui::FontId::monospace(10.0),ACCENT);}

            }
            if b.avoidance.active {
                let col = if b.avoidance.impossible { DANGER } else { Color32::from_rgb(255, 150, 60) };
                painter.circle_stroke(p, 16.0, Stroke::new(1.5, col));
            }
            if b.kind != BodyKind::Missile && !(b.controllable && self.own_faction()==Some(b.faction)) {
                labels.add(p + EVec2::new(10.0, -10.0), b.name.clone(), c);
            }
        }

        // Spectator overlay: the chosen faction's belief, linked to truth.
        if let Some((ov, truth)) = overlay {
            for c in &ov.contacts {
                let real = truth.get(&c.id).and_then(|id| view.bodies.iter().find(|b| b.id == *id));
                draw_contact(&painter, &cam, rect, ov, c, BELIEF, false, &mut labels);
                if let (Some(t), Some(r)) = (&c.track, real) {
                    let pb = to_screen(&cam, rect, t.pos);
                    let pt = to_screen(&cam, rect, r.pos);
                    painter.line_segment([pb, pt], Stroke::new(1.0, BELIEF.gamma_multiply(0.5)));
                    painter.rect_stroke(Rect::from_center_size(pb, EVec2::splat(9.0)), 0.0, Stroke::new(1.0, BELIEF), StrokeKind::Middle);
                }
            }
        }

        labels.paint(&painter);
        if let Some(lines) = resp.hover_pos().and_then(|pointer| hover_details_for_target(view, &cam, rect, pointer, self.selected, self.inspected)) {
            let width = 280.0_f32.min(rect.width()*0.48);
            let height=35.0+lines.len() as f32*17.0;
            let panel = Rect::from_min_size(Pos2::new(rect.right()-width-10.0,rect.bottom()-height-10.0),EVec2::new(width,height));
            painter.rect_filled(panel,0.0,PANEL_BG.gamma_multiply(0.95));
            painter.line_segment([panel.right_top(),panel.right_bottom()],Stroke::new(2.0,ACCENT));
            let clipped=painter.with_clip_rect(panel.shrink(8.0));
            clipped.text(panel.min+EVec2::new(10.0,8.0),egui::Align2::LEFT_TOP,"OBJECT / SENSOR PICTURE",mono(9.0),ACCENT);
            for (i,line) in lines.iter().enumerate() {
                clipped.text(panel.min+EVec2::new(10.0,26.0+i as f32*17.0),egui::Align2::LEFT_TOP,line,mono(10.0),if i==0 {TEXT_HI} else {TEXT_MUTED});
            }
        } else {
            draw_scale_bar(&painter, &cam, rect);
        }
        self.draw_navigation_readout(&painter, rect, view);

        if let Some(o) = &view.outcome {
            let mine = self.own_faction().map(|f| f == o.winner);
            let (title, col) = match mine {
                Some(true) => ("VICTORY", Color32::from_rgb(120, 220, 140)),
                Some(false) => ("DEFEAT", DANGER),
                None => ("GAME OVER", Color32::WHITE),
            };
            let at = rect.center_top() + EVec2::new(0.0, 40.0);
            painter.text(at, egui::Align2::CENTER_TOP, title, egui::FontId::proportional(32.0), col);
            painter.text(
                at + EVec2::new(0.0, 40.0),
                egui::Align2::CENTER_TOP,
                format!("{} wins at T+ {}: {}", faction_name(o.winner), fmt_time(o.t), o.reason),
                egui::FontId::proportional(14.0),
                Color32::LIGHT_GRAY,
            );
        }

        if let Some(body)=self.jump_select {
            if !view.bodies.iter().any(|b|b.id==body && b.jump.is_none()) {self.jump_select=None;}
            else {
                painter.circle_stroke(to_screen(&cam,rect,Vec2::ZERO),(MAX_SOL_RADIUS_AU*AU/cam.km_per_px) as f32,Stroke::new(1.0,ACCENT));
                if let Some(pointer)=resp.hover_pos() {
                    let destination=to_world(&cam,rect,pointer);
                    let valid=destination.length()<=MAX_SOL_RADIUS_AU*AU;
                    ui.ctx().set_cursor_icon(if valid {egui::CursorIcon::Crosshair} else {egui::CursorIcon::NotAllowed});
                    painter.text(pointer+EVec2::new(14.0,14.0),egui::Align2::LEFT_TOP,format!("JUMP · {:.2} AU from {}",destination.length()/AU,self.theme.star_name()),egui::FontId::monospace(11.0),if valid {ACCENT} else {DANGER});
                    if resp.clicked() {
                        if valid {self.command(Command::Jump {body,destination});self.jump_select=None;}
                        else {self.last_message=Some(format!("Jump destination must be within 50 AU of {}",self.theme.star_name()));}
                    }
                }
                if resp.secondary_clicked() || ui.input(|i|i.key_pressed(egui::Key::Escape)) {self.jump_select=None;}
                self.combat_overlay(ui,view,rect);
                return;
            }
        }
        // Orders. Right-click a ship or contact to intercept it, a celestial body to
        // orbit it, or empty space to fly there and stop.
        if let (Some(Selection::Body(id)), Some(click)) =
            (self.selected, resp.secondary_clicked().then(|| resp.interact_pointer_pos()).flatten())
            && let Some(b) = view.bodies.iter().find(|b| b.id == id)
            && self.own_faction() == Some(b.faction)
            && b.controllable && b.jump.is_none() && b.kind != BodyKind::Missile
        {
            let near = |p: Vec2, r: f32| to_screen(&cam, rect, p).distance(click) < r;
            let own = view.bodies.iter().find(|o| o.id != id && o.faction==b.faction && o.kind!=BodyKind::Missile && near(o.pos, 14.0)).map(|o| InterceptTarget::Own(o.id));
            let contact = view
                .contacts
                .iter()
                .find(|c| c.track.as_ref().is_some_and(|t| near(t.pos, 14.0)))
                .map(|c| InterceptTarget::Contact(c.id));
            let celestial = view.celestials.iter().position(|c| {
                let r_px = (c.radius / cam.km_per_px) as f32;
                near(c.pos, r_px.max(8.0) + 4.0)
            });
            if ui.input(|i|i.modifiers.shift) {
                self.command(Command::AppendWaypoint {body:id,point:to_world(&cam,rect,click)});
            } else if let Some(target) = own.or(contact) {
                self.move_to_ship(b,target);
            } else if let Some(celestial) = celestial {
                self.command(Command::Orbit { body: id, celestial });
            } else {
                self.command(Command::MoveTo { body: id, point: to_world(&cam, rect, click) });
            }
        }
        if resp.clicked()
            && let Some(click) = resp.interact_pointer_pos()
            && let Some((s, _)) = selectable_at(view, &cam, rect, click)
        {
            self.select_object(s, view);
        }
        self.combat_overlay(ui,view,rect);
    }

    fn update_tactical_log(&mut self,view:&View,now:f64) {
        let own=match self.selected {Some(Selection::Body(id))=>Some(id),_=>None};
        self.tactical_log.observe(view,own,now);
        let actions:Vec<_>=self.session.bot_debug(RAIDER).filter(|(at,_,_)|*at>self.tactical_log.raider_at).cloned().collect();
        for (at,_,note) in actions {
            self.tactical_log.raider_at=self.tactical_log.raider_at.max(at);
            if note.to_ascii_lowercase().contains("ping") {continue;}
            self.tactical_log.push("raider".into(),format!("RAIDER [DEBUG] · {note}"),TEXT_MUTED,at);
        }
    }

    fn combat_overlay(&self,ui:&egui::Ui,_view:&View,map:Rect) {
        let width=(map.width()*0.31).clamp(230.0,360.0);
        let lines:Vec<_>=self.tactical_log.lines.iter().filter(|l|TacticalLog::opacity(self.tactical_log.now-l.at)>0.0).take(8).collect();
        let rect=Rect::from_min_size(Pos2::new(map.right()-width-4.0,map.top()+10.0),EVec2::new(width,27.0+lines.len().max(1) as f32*16.0));
        let p=ui.painter();
        p.rect_filled(rect,2.0,Color32::from_rgba_unmultiplied(6,12,22,85));
        p.line_segment([rect.right_top(),rect.right_top()+EVec2::new(0.0,17.0)],Stroke::new(2.0,ACCENT));
        p.text(rect.right_top()+EVec2::new(-9.0,6.0),egui::Align2::RIGHT_TOP,"COMBAT LOG",mono(10.0),ACCENT);
        let max_chars=((width-18.0)/5.8) as usize;
        for (i,line) in lines.iter().enumerate() {
            let text=format!("{}  {}{}",fmt_time(line.sim),line.text,if line.count>1 {format!(" · {} reports",line.count)} else {String::new()});
            let text=if text.chars().count()>max_chars {format!("{}…",text.chars().take(max_chars.saturating_sub(1)).collect::<String>())} else {text};
            let alpha=TacticalLog::opacity(self.tactical_log.now-line.at);
            p.text(rect.left_top()+EVec2::new(9.0,25.0+i as f32*16.0),egui::Align2::LEFT_TOP,text,mono(9.5),line.color.gamma_multiply(alpha));
        }
        if lines.is_empty() {p.text(rect.right_top()+EVec2::new(-9.0,25.0),egui::Align2::RIGHT_TOP,"NO RECENT ACTIVITY",mono(8.0),TEXT_MUTED);}
    }
    /// Ease the displayed angle every frame, rather than stepping once per report.
    fn smooth_bearings(&mut self, view: &mut View, dt:f64) {
        let alpha=if view.paused {0.0} else {1.0-(-dt.min(0.1)/0.35).exp()};
        for c in &mut view.contacts {
            for b in &mut c.bearings {
                let entry=self.bearing_display.entry((c.id,b.sensor)).or_insert((b.emitted_at,b.bearing));
                entry.0=b.emitted_at;
                entry.1=wrap_angle(entry.1+wrap_angle(b.bearing-entry.1)*alpha);
                b.bearing=entry.1;
            }
        }
    }

    /// Frame own ships and contacts (or everything, for the spectator).
    fn fit(&mut self, view: &View, rect: Rect) {
        let mut pts: Vec<Vec2> = view.bodies.iter().map(|b| b.pos).collect();
        pts.extend(view.contacts.iter().filter_map(|c| c.track.as_ref().map(|t| t.pos)));
        if self.opening_fit {
            let own=view.bodies.iter().find(|b|self.selected==Some(Selection::Body(b.id)));
            let target=match self.inspected {
                Some(Selection::Body(id))=>view.bodies.iter().find(|b|b.id==id).map(|b|b.pos),
                Some(Selection::Contact(id))=>view.contacts.iter().find(|c|c.id==id).and_then(|c|c.track.as_ref()).map(|t|t.pos),
                None=>None,
            };
            if let (Some(own),Some(target))=(own,target) {pts=vec![own.pos,target];}
            else if let Some(own)=own
                && let Some(b)=view.contacts.iter().find(|c|self.inspected==Some(Selection::Contact(c.id))).and_then(|c|c.bearings.first()) {
                    pts=vec![own.pos,own.pos+Vec2::new(b.bearing.cos(),b.bearing.sin())*AU];
            }
        }
        if pts.is_empty() {
            pts.push(Vec2::ZERO);
        }
        let (mut lo, mut hi) = (pts[0], pts[0]);
        for p in &pts {
            lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
        }
        let size = hi - lo;
        let km_per_px = (size.x / rect.width() as f64).max(size.y / rect.height() as f64) * 1.3;
        self.camera = Camera { center: (lo + hi) * 0.5, km_per_px: km_per_px.max(2.0 * LIGHT_SECOND / rect.width() as f64) };
    }
}

fn contact_details(ui: &mut egui::Ui, view: &View, c: &ContactView) {
    ui.separator();
    ui.heading(contact_label(c));
    ui.label(format!("{} · {} · {}",contact_class(c),track_quality(c).0,if c.identified_name.is_some() {"Identity confirmed"} else {"Identity unknown"}));
    ui.label(format!(
        "Last seen by {} · light emitted T+ {}, reached flagship T+ {}",
        match c.last_source {
            Source::Emission => "its own emission",
            Source::Echo => "our ping's echo",
            Source::Ping => "its active ping",
        },
        fmt_time(c.last_emitted_at),
        fmt_time(c.last_received_at),
    ));
    ui.label(format!("SNR {:.1}", c.last_snr));
    if let Some(r) = c.last_range {
        ui.label(format!("Echo range {}", fmt_distance(r)));
    }
    match &c.track {
        Some(t) => {
            ui.label(format!("Estimated speed {}, thrust {:.1} g", fmt_speed(t.vel.length()), t.accel.length() / G0));
            if let Some(own) = view.bodies.iter().find(|b| b.controllable && b.kind == BodyKind::Ship) {
                let rel = t.pos-own.pos;
                let vel = t.vel-own.vel;
                let cpa_t = (-rel.dot(vel)/vel.dot(vel).max(1e-12)).max(0.0);
                ui.label(format!("Range {} · closure {}",fmt_distance(rel.length()),fmt_speed(-vel.dot(rel.normalized()))));
                ui.label(format!("Coasting closest approach {} in {}",fmt_distance((rel+vel*cpa_t).length()),fmt_age(cpa_t)));
                ui.weak("Closest approach assumes unchanged velocity; manoeuvres alter it.");
            }
            ui.label(format!("Position ±{} (2σ, now)", fmt_distance(2.0 * sigma_major(t.cov))));
            ui.label(format!("{} measurements; newest emitted {} ago", t.updates, fmt_age(view.time - t.updated_at)));
        }
        None => {
            ui.label("Direction finding only. Close for passive localization, send a ping, or triangulate with another ship.");
        }
    }
}

/// Largest 1σ axis of a 2×2 covariance, km.
fn sigma_major(cov: [[f64; 2]; 2]) -> f64 {
    let (a, b, d) = (cov[0][0], cov[0][1], cov[1][1]);
    let tr = 0.5 * (a + d);
    let disc = (0.25 * (a - d).powi(2) + b * b).sqrt();
    (tr + disc).max(0.0).sqrt()
}

fn weapon_ranges(ship:&BodyView)->Vec<(&'static str,f64,Color32)> {
    let mut ranges=Vec::new();
    let color=Color32::from_rgb(255,220,70);
    for (payload,label) in [(Payload::Nuclear,"LRM"),(Payload::Kinetic,"SRM")] {
        // Queued rounds still aboard count until actually launched.
        if ship.magazine[payload.index()]>0 {ranges.push((label,payload.engagement_range(),color));}
    }
    ranges.push(("BEAM",params::SHIP_BEAM_AUTO_RANGE_LS.value*LIGHT_SECOND,color));
    ranges
}
fn draw_weapon_ranges(painter:&egui::Painter,cam:&Camera,rect:Rect,ship:&BodyView,theme:theme::Theme) {
    let center=to_screen(cam,rect,ship.pos);
    for (label,range,color) in weapon_ranges(ship) {
        let label=theme.weapon(match label {"LRM"=>Payload::Nuclear,"SRM"=>Payload::Kinetic,_=>Payload::Beam});
        let radius=(range/cam.km_per_px) as f32;
        if !radius.is_finite() || radius<2.0 || radius>1e6 {continue;}
        let dots=(std::f32::consts::TAU*radius/6.0).ceil() as usize;
        let clip=painter.clip_rect().expand(1.0);
        let nearest=clip.clamp(center).distance(center);
        let farthest=[clip.left_top(),clip.right_top(),clip.left_bottom(),clip.right_bottom()]
            .into_iter().map(|p|p.distance(center)).fold(0.0_f32,f32::max);
        if radius<nearest || radius>farthest {continue;}
        let label_color=Color32::from_rgba_unmultiplied(color.r(),color.g(),color.b(),128);
        let glyphs:Vec<_>=format!("{label} RANGE").chars().map(|c|
            painter.layout_no_wrap(c.to_string(),mono(9.0),label_color)).collect();
        let width:f32=glyphs.iter().map(|g|g.size().x).sum();
        // Letter centers follow the arc. Counterclockwise reading keeps the
        // tops inward, deliberately leaving labels on the far side upside down.
        if radius>=320.0_f32.max(width*6.0) {
            let text_radius=radius-14.0;
            for i in 0..12 {
                paint_inward_arc(painter,center,text_radius,std::f32::consts::TAU*i as f32/12.0,&glyphs,label_color);
            }
        }
        for i in 0..dots {
            let angle=std::f32::consts::TAU*i as f32/dots as f32;
            let at=center+EVec2::new(angle.cos(),angle.sin())*radius;
            if clip.contains(at) {
                painter.circle_filled(at,0.5,color.gamma_multiply(0.5));
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_contact(
    painter: &egui::Painter,
    cam: &Camera,
    rect: Rect,
    view: &View,
    c: &ContactView,
    color: Color32,
    selected: bool,
    labels: &mut Labels,
) {
    let ping=view.hostile_pings.iter().find(|p|p.contact==c.id);
    let resolved_course=contact_has_course(c);
    let color=if resolved_course {color} else {ping.map_or(color,|p|Color32::from_rgb(255,155,45).gamma_multiply(p.opacity(view.time)))};
    let color = if c.stale { color.gamma_multiply(0.4) } else { color };
    match &c.track {
        Some(t) => {
            if c.detection==sensors::DetectionLevel::Approximate {
                let angle=0.5*(2.0*t.cov[0][1]).atan2(t.cov[0][0]-t.cov[1][1]);
                let major=sigma_major(t.cov);
                let minor=(t.cov[0][0]+t.cov[1][1]-major*major).max(0.0).sqrt();
                let points=(0..48).map(|i| {
                    let a=i as f64*std::f64::consts::TAU/48.0;
                    let x=2.0*major*a.cos();let y=2.0*minor*a.sin();
                    to_screen(cam,rect,t.pos+Vec2::new(x*angle.cos()-y*angle.sin(),x*angle.sin()+y*angle.cos()))
                }).collect();
                painter.add(Shape::convex_polygon(points,color.gamma_multiply(0.13),Stroke::NONE));
                let p=to_screen(cam,rect,t.pos);
                painter.line_segment([p,to_screen(cam,rect,t.pos+t.vel*60.0)],Stroke::new(1.0,color.gamma_multiply(0.6)));
                labels.add(p+EVec2::new(10.0,-10.0),contact_label(c),color);
                return;
            }
            if contact_has_course(c) {
                let forecast = view.system.predict(State { pos: t.pos, vel: t.vel }, t.accel, view.time, FORECAST_S, 240);
                let fp: Vec<Pos2> = forecast.points.iter().map(|&p| to_screen(cam, rect, p)).collect();
                painter.extend(fading_path(&fp,color.gamma_multiply(0.24),8.0));
            }
            let p = to_screen(cam, rect, t.pos);
            if let Some(depart)=view.withdrawals.get(&c.id) {
                if (view.time*2.0) as u64%2==0 {painter.add(Shape::closed_line(vec![p+EVec2::new(0.0,-24.0),p+EVec2::new(22.0,17.0),p+EVec2::new(-22.0,17.0)],Stroke::new(2.0,WARM)));}
                let text=if *depart>view.time {format!("WITHDRAWING · {}",fmt_time(*depart-view.time))} else {"WITHDRAWAL · AWAITING CONFIRMATION".into()};
                painter.text(p+EVec2::new(0.0,-29.0),egui::Align2::CENTER_BOTTOM,text,egui::FontId::monospace(10.0),WARM);
            }
            if c.resolved_interceptor {
                painter.circle_filled(p,2.0,color);
            } else if c.resolved_missile {
                draw_missile(painter,p,color,selected);
            } else if c.resolved_kind==Some(BodyKind::Probe) {
                painter.add(Shape::convex_polygon(vec![p+EVec2::new(0.0,-5.0),p+EVec2::new(4.0,0.0),
                    p+EVec2::new(0.0,5.0),p+EVec2::new(-4.0,0.0)],color,Stroke::NONE));
            } else if c.resolved_kind==Some(BodyKind::Station) {
                painter.rect_filled(Rect::from_center_size(p,EVec2::splat(7.0)),0.0,color);
            } else if resolved_course {
                let heading=if t.vel.length()>0.0 {t.vel.normalized()} else {Vec2::new(0.0,1.0)};
                let burn=(t.accel-view.system.gravity(t.pos,view.time)).length()/G0;
                draw_burn_vector(painter,p,heading,burn,if c.stale {0.4} else {1.0});
                draw_contact_marker(painter, p, t.vel, color, selected);
            } else if let Some(ping)=ping {
                painter.circle_filled(p,if selected {7.0} else {5.0},Color32::from_rgb(255,155,45).gamma_multiply(ping.opacity(view.time)));
            } else {
                painter.circle_stroke(p,if selected {6.0} else {4.0},Stroke::new(1.5,color));
            }
            if !c.resolved_missile || selected {
                labels.add(p + EVec2::new(10.0, -10.0), contact_label(c), color);
            }
        }
        None => {
            // Show only bearings measured by the player's command ship.
            // Allied measurements still contribute to contact fusion.
            let score=|b:&luminal_core::session::BearingView| b.sigma /
                (1.0-((view.time-b.received_at)/BEARING_FADE_S).clamp(0.0,1.0)).max(0.001);
            for b in c.bearings.iter().filter(|b|view.time-b.received_at<BEARING_FADE_S
                && view.bodies.iter().any(|ship|ship.controllable && ship.id==b.sensor))
                .min_by(|a,b|score(a).total_cmp(&score(b))).into_iter() {
                let fade = bearing_opacity(b,view.time);
                if fade <= 0.0 {
                    continue;
                }
                // Display follows the current receiver; historical origins remain in sensor fusion.
                let origin=view.bodies.iter().find(|ship|ship.id==b.sensor).map_or(b.origin,|ship|ship.pos);
                let o = to_screen(cam, rect, origin);
                let reach = (b.max_range / cam.km_per_px) as f32;
                let ray = |a: f64, f: f32| o + EVec2::new(a.cos() as f32, -a.sin() as f32) * reach * f;
                let spread = (2.0 * b.sigma).min(0.5);
                for i in 0..64 {
                    let start = i as f32 / 64.0;
                    let end = (i + 1) as f32 / 64.0;
                    let opacity = fade * (1.0 - (start + end) * 0.5).powi(2);
                    painter.add(Shape::convex_polygon(
                        vec![ray(b.bearing-spread,start),ray(b.bearing-spread,end),
                            ray(b.bearing+spread,end),ray(b.bearing+spread,start)],
                        color.gamma_multiply(0.06 * opacity), Stroke::NONE));
                    painter.line_segment([ray(b.bearing,start),ray(b.bearing,end)],
                        Stroke::new(if selected {1.5} else {1.0},color.gamma_multiply(0.45 * opacity)));
                }
                let dir = EVec2::new(b.bearing.cos() as f32, -b.bearing.sin() as f32);
                let tip = clip_to_rect(rect, o, dir).filter(|p|p.distance(o)<reach).unwrap_or(ray(b.bearing,1.0));
                labels.add(tip-dir*30.0,contact_label(c),color.gamma_multiply(fade.max(0.3)));
            }
        }
    }
}

/// Where a ray from `o` along `dir` leaves `rect`, if `o` is inside.
fn clip_to_rect(rect: Rect, o: Pos2, dir: EVec2) -> Option<Pos2> {
    if !rect.contains(o) {
        return None;
    }
    let tx = if dir.x > 0.0 { (rect.right() - o.x) / dir.x } else if dir.x < 0.0 { (rect.left() - o.x) / dir.x } else { f32::INFINITY };
    let ty = if dir.y > 0.0 { (rect.bottom() - o.y) / dir.y } else if dir.y < 0.0 { (rect.top() - o.y) / dir.y } else { f32::INFINITY };
    Some(o + dir * tx.min(ty))
}

/// Greedy label placement: move a label down until it no longer overlaps.
#[derive(Default)]
struct Labels {
    items: Vec<(Pos2, String, Color32)>,
}

impl Labels {
    fn add(&mut self, at: Pos2, text: String, color: Color32) {
        self.items.push((at, text, color));
    }

    fn paint(self, painter: &egui::Painter) {
        let font = egui::FontId::proportional(12.0);
        let bounds = painter.clip_rect().shrink(4.0);
        let mut placed: Vec<Rect> = vec![];
        for (at, text, color) in self.items {
            let galley = painter.layout_no_wrap(text, font.clone(), color);
            let size = galley.size();
            let mut r = Rect::from_min_size(at - EVec2::new(0.0, size.y), size);
            // Keep labels inside the map.
            r = r.translate(EVec2::new((bounds.right() - r.right()).min(0.0) + (bounds.left() - r.left()).max(0.0), 0.0));
            for _ in 0..12 {
                if !placed.iter().any(|p| p.expand(1.0).intersects(r)) {
                    break;
                }
                r = r.translate(EVec2::new(0.0, size.y + 1.0));
            }
            if (r.min - (at - EVec2::new(0.0, size.y))).length() > 1.0 {
                let anchor = if r.center().x < at.x { r.right_center() } else { r.left_center() };
                painter.line_segment([at, anchor], Stroke::new(0.5, color.gamma_multiply(0.4)));
            }
            painter.galley(r.min, galley, color);
            placed.push(r);
        }
    }
}

/// Hit-test only the received map picture, never simulation truth. Hovering does
/// not alter the commanded ship or designated target.
/// Shared hit area for the hover plate and left-click selection.
fn selectable_at(view: &View, cam: &Camera, rect: Rect, pointer: Pos2) -> Option<(Selection, Vec2)> {
    let bodies = view.bodies.iter().filter(|b| b.kind != BodyKind::Missile)
        .map(|b| (Selection::Body(b.id), b.pos));
    let contacts = view.contacts.iter().filter_map(|c| c.track.as_ref().map(|t| (Selection::Contact(c.id), t.pos)));
    bodies.chain(contacts)
        .map(|(s, p)| (s, p, to_screen(cam, rect, p).distance(pointer)))
        .filter(|(_, _, d)| *d < 14.0)
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(s, p, _)| (s, p))
}

#[cfg(test)]
fn hover_details(view: &View, cam: &Camera, rect: Rect, pointer: Pos2) -> Option<Vec<String>> {
    hover_details_for_target(view,cam,rect,pointer,None,None)
}

fn hover_details_for_target(view: &View, cam: &Camera, rect: Rect, pointer: Pos2, own:Option<Selection>, target:Option<Selection>) -> Option<Vec<String>> {
    let bodies=view.bodies.iter().enumerate().map(|(i,b)|(0,i,b.pos,14.0));
    let contacts=view.contacts.iter().enumerate().filter_map(|(i,c)|c.track.as_ref().map(|t|(1,i,t.pos,14.0)));
    let celestials=view.celestials.iter().enumerate().map(|(i,c)|(2,i,c.pos,(c.radius/cam.km_per_px) as f32+8.0));
    let (kind,index,_,_)=bodies.chain(contacts).chain(celestials)
        .map(|(k,i,p,r)|(k,i,to_screen(cam,rect,p).distance(pointer),r))
        .filter(|(_,_,d,r)|d<r)
        .min_by(|a,b|a.2.total_cmp(&b.2))?;
    let pos=match kind {0=>view.bodies[index].pos,1=>view.contacts[index].track.as_ref().unwrap().pos,_=>view.celestials[index].pos};
    let mut lines=match kind {
        0=>{
            let b=&view.bodies[index];
            let d=&b.damage.damage;
            vec![b.name.to_uppercase(),format!("{:?} · {}",b.kind,faction_name(b.faction)),
                format!("SPEED {} · THRUST {:.0}g",fmt_speed(b.vel.length()),b.thrust.length()/G0),
                format!("HULL {:.0}% · SCREEN {}",100.0*d.hull/d.hull_max,if !b.has_screen {"N/A"} else if b.screen_up {"UP"} else {"DOWN"})]
        },
        1=>{
            let c=&view.contacts[index];
            let t=c.track.as_ref().unwrap();
            vec![contact_label(c).to_uppercase(),format!("{} · {}",c.resolved_kind.map_or("UNKNOWN CLASS".into(),|k|format!("{k:?}").to_uppercase()),track_quality(c).0),
                if c.detection>=sensors::DetectionLevel::Approximate {format!("EST SPEED {}",fmt_speed(t.vel.length()))} else {"SPEED UNKNOWN".into()},
                format!("SENSOR {} · AGE {:.0}s · PING {:.0}s",c.reporting_sensor.and_then(|id|view.bodies.iter().find(|b|b.id==id)).map_or("RELAY",|b|b.name.as_str()),(view.time-c.last_emitted_at).max(0.0),c.ping_remaining),
                c.damage.as_ref().map_or("HULL / SYSTEMS UNKNOWN".into(),|r|format!("CONFIRMED HULL {:.0}%",100.0*r.damage.hull/r.damage.hull_max))]
        },
        _=>{
            let c=&view.celestials[index];
            vec![c.name.to_uppercase(),format!("{:?}",c.kind).to_uppercase(),format!("RADIUS {}",fmt_distance(c.radius))]
        }
    };
    if kind==1 {
        let c=&view.contacts[index];
        if view.hostile_pings.iter().any(|p|p.contact==c.id && p.opacity(view.time)>0.0) {
            lines.push("ORANGE · ACTIVE PING OBSERVED".into());
        }
    }
    let position=|selection|match selection {
        Selection::Body(id)=>view.bodies.iter().find(|b|b.id==id).map(|b|b.pos),
        Selection::Contact(id)=>view.contacts.iter().find(|c|c.id==id).and_then(|c|c.track.as_ref()).map(|t|t.pos),
    };
    if let Some(ship)=own.and_then(position) {lines.push(format!("FROM OWN SHIP  {}",fmt_distance((pos-ship).length())));}
    if target.is_some() {
        lines.push(target.and_then(position).map_or("TO TARGET  RANGE UNKNOWN".into(),|p|format!("TO TARGET  {} · EST",fmt_distance((pos-p).length()))));
    }
    Some(lines)
}

fn draw_scale_bar(painter: &egui::Painter, cam: &Camera, rect: Rect) {
    let target_km = 150.0 * cam.km_per_px;
    let unit = if target_km >= 0.1 * AU { AU } else if target_km >= LIGHT_SECOND { LIGHT_SECOND } else { 1.0 };
    let raw = target_km / unit;
    let mag = 10f64.powf(raw.log10().floor());
    let nice = [1.0, 2.0, 5.0, 10.0].into_iter().map(|m| m * mag).rfind(|v| *v <= raw).unwrap_or(mag);
    let km = nice * unit;
    let px = (km / cam.km_per_px) as f32;
    let y = rect.bottom() - 22.0;
    let x = rect.right() - 22.0 - px;
    let label = if unit == AU {
        format!("{nice} AU")
    } else if unit == LIGHT_SECOND {
        format!("{nice} ls")
    } else {
        format!("{nice} km")
    };
    let panel=Rect::from_min_max(Pos2::new(rect.right()-184.0,y-48.0),Pos2::new(rect.right()-10.0,rect.bottom()-10.0));
    painter.rect_filled(panel,0.0,PANEL_BG.gamma_multiply(0.9));
    painter.text(panel.min+EVec2::new(10.0,7.0),egui::Align2::LEFT_TOP,"TACTICAL SCALE",egui::FontId::monospace(9.0),ACCENT);
    painter.line_segment([Pos2::new(x, y), Pos2::new(x + px, y)], Stroke::new(1.0, ACCENT));
    for tick in [x,x+px*0.5,x+px] {painter.line_segment([Pos2::new(tick,y-3.0),Pos2::new(tick,y+3.0)],Stroke::new(1.0,ACCENT));}
    painter.text(Pos2::new(x+px,y-6.0),egui::Align2::RIGHT_BOTTOM,label,egui::FontId::monospace(11.0),TEXT_HI);
}

fn star_distance_label(name:&str,distance:f64)->String {
    format!("{name} - {}",fmt_distance(distance))
}

/// Orbital distance from the moon's primary, always in light-seconds.
fn primary_distance_label(name:&str,distance:f64)->String {
    format!("{name} - {} LS",sparse_distance(distance/LIGHT_SECOND))
}

fn celestial_distance_label(view:&View,index:usize)->String {
    let body=&view.celestials[index];
    match view.system.bodies.get(index).map(|b|&b.orbit) {
        Some(Orbit::Circular {parent,..} | Orbit::Frozen {parent,..}) if body.kind==CelestialKind::Moon =>
            primary_distance_label(&body.name,(body.pos-view.celestials[*parent].pos).length()),
        _ => star_distance_label(&body.name,(body.pos-view.celestials[0].pos).length()),
    }
}

/// Glyphs along an arc, tops facing the center. Reading runs toward decreasing
/// screen angle, so a label on the far side is upside down, matching weapon rings.
fn paint_inward_arc(painter:&egui::Painter,center:Pos2,text_radius:f32,anchor:f32,glyphs:&[std::sync::Arc<egui::Galley>],color:Color32) {
    if text_radius<1.0 {return;}
    let clip=painter.clip_rect().expand(1.0);
    let mut offset=-glyphs.iter().map(|g|g.size().x).sum::<f32>()*0.5;
    for glyph in glyphs {
        let angle=anchor-(offset+glyph.size().x*0.5)/text_radius;
        let at=center+EVec2::new(angle.cos(),angle.sin())*text_radius;
        if clip.expand(glyph.size().length()).contains(at) {
            painter.add(egui::epaint::TextShape::new(at-glyph.size()*0.5,glyph.clone(),color)
                .with_angle_and_anchor(angle-std::f32::consts::FRAC_PI_2,egui::Align2::CENTER_CENTER));
        }
        offset+=glyph.size().x;
    }
}

/// One label inside this body's own orbit, beside the disk. The star keeps a plain name.
fn draw_orbit_label(painter:&egui::Painter,cam:&Camera,rect:Rect,view:&View,index:usize,body:Pos2,color:Color32)->bool {
    let Some(body_def)=view.system.bodies.get(index) else {return false};
    let (parent,radius)=match body_def.orbit {
        Orbit::Circular {parent,radius,..} | Orbit::Frozen {parent,radius,..} => (parent,radius),
        Orbit::Fixed(_) => return false,
    };
    let orbit_px=(radius/cam.km_per_px) as f32;
    if !orbit_px.is_finite() || orbit_px<8.0 {return false;}
    let center=to_screen(cam,rect,view.system.state(parent,view.time).pos);
    let rel=body-center;
    if rel.length()<1.0 {return false;}
    let text=celestial_distance_label(view,index);
    let label_color=Color32::from_rgba_unmultiplied(color.r(),color.g(),color.b(),128);
    let body_px=((view.celestials[index].radius/cam.km_per_px) as f32).max(3.0);
    let glyphs:Vec<_>=text.chars().map(|c|painter.layout_no_wrap(c.to_string(),mono(9.0),label_color)).collect();
    let width:f32=glyphs.iter().map(|g|g.size().x).sum();
    let text_radius=orbit_px-14.0;
    if orbit_px>=32.0 && text_radius>=18.0 && width<=text_radius*std::f32::consts::PI {
        // First glyph sits just clear of the disk; the rest read away from it.
        let body_angle=rel.y.atan2(rel.x);
        let anchor=body_angle-(body_px+6.0)/text_radius-width*0.5/text_radius;
        paint_inward_arc(painter,center,text_radius,anchor,&glyphs,label_color);
        return true;
    }
    let outward=rel.normalized();
    let tangent=EVec2::new(outward.y,-outward.x);
    let inward=14.0_f32.min(orbit_px*0.35).max(4.0);
    let at=body-outward*inward+tangent*(body_px+6.0);
    let align=if tangent.x>=0.0 {egui::Align2::LEFT_CENTER} else {egui::Align2::RIGHT_CENTER};
    painter.text(at,align,text,mono(9.0),label_color);
    true
}

const GRID_DOT: Color32 = Color32::from_rgba_unmultiplied_const(168, 196, 220, 64);
/// Below this pitch the dots clot, so the lattice waits until it reads as points.
const GRID_MIN_PX: f32 = 12.0;

/// 1 AU across the system. 1 light-second once that pitch is wide enough to separate.
fn grid_spacing(km_per_px: f64) -> Option<f64> {
    if !km_per_px.is_finite() || km_per_px <= 0.0 { return None; }
    if LIGHT_SECOND / km_per_px >= GRID_MIN_PX as f64 { return Some(LIGHT_SECOND); }
    if AU / km_per_px >= GRID_MIN_PX as f64 { return Some(AU); }
    None
}

fn draw_point_grid(painter: &egui::Painter, cam: &Camera, rect: Rect) {
    let Some(spacing) = grid_spacing(cam.km_per_px) else { return };
    let corners = [rect.left_top(), rect.right_top(), rect.left_bottom(), rect.right_bottom()];
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for corner in corners {
        let world = to_world(cam, rect, corner);
        min_x = min_x.min(world.x); max_x = max_x.max(world.x);
        min_y = min_y.min(world.y); max_y = max_y.max(world.y);
    }
    let pad = cam.km_per_px;
    let x0 = ((min_x - pad) / spacing).ceil();
    let x1 = ((max_x + pad) / spacing).floor();
    let y0 = ((min_y - pad) / spacing).ceil();
    let y1 = ((max_y + pad) / spacing).floor();
    if ![x0, x1, y0, y1].iter().all(|n| n.is_finite()) { return; }
    let columns = (x1 - x0).round() as i64 + 1;
    let rows = (y1 - y0).round() as i64 + 1;
    if columns <= 0 || rows <= 0 || columns > 800 || rows > 800 { return; }
    let mut mesh = egui::Mesh::default();
    let count = (columns * rows) as usize;
    mesh.reserve_vertices(count * 4);
    mesh.reserve_triangles(count * 2);
    let x0 = x0 as i64;
    let y0 = y0 as i64;
    for row in 0..rows {
        for col in 0..columns {
            let world = Vec2::new((x0 + col) as f64 * spacing, (y0 + row) as f64 * spacing);
            let at = to_screen(cam, rect, world);
            if rect.expand(1.0).contains(at) {
                mesh.add_colored_rect(Rect::from_center_size(at, EVec2::splat(2.0)), GRID_DOT);
            }
        }
    }
    if !mesh.is_empty() { painter.add(Shape::mesh(mesh)); }
}

fn draw_orbits(painter: &egui::Painter, cam: &Camera, rect: Rect, view: &View) {
    for (i, c) in view.system.bodies.iter().enumerate() {
        if let Orbit::Circular { parent, radius, .. } | Orbit::Frozen { parent, radius, .. } = c.orbit {
            let r = (radius / cam.km_per_px) as f32;
            if r < 4.0 {
                continue;
            }
            let center = to_screen(cam, rect, view.system.state(parent, view.time).pos);
            painter.circle_stroke(center, r, Stroke::new(1.0, celestial_color(view.celestials[i].kind).gamma_multiply(0.18)));
        }
    }
}

/// The region a celestial body hides from an observer at `eye`.
fn draw_shadow(painter:&egui::Painter,cam:&Camera,rect:Rect,star:Vec2,star_radius:f64,center:Vec2,radius:f64) {
    let delta=center-star;let distance=delta.length();
    if distance<=star_radius+radius {return;}
    let away=delta.normalized();let side=Vec2::new(-away.y,away.x);
    let screen_reach=(rect.width()+rect.height()) as f64*cam.km_per_px*2.0;
    let reach=if star_radius>radius {(radius*distance/(star_radius-radius)).min(screen_reach)} else {screen_reach};
    let far_width=(radius-reach*(star_radius-radius)/distance).max(0.0);
    let polygon=|reach:f64,width:f64|vec![to_screen(cam,rect,center+side*radius),to_screen(cam,rect,center+away*reach+side*width),
        to_screen(cam,rect,center+away*reach-side*width),to_screen(cam,rect,center-side*radius)];
    // Finite umbra, surrounded by a much fainter widening penumbra.
    painter.add(Shape::convex_polygon(polygon(reach,radius+reach*(star_radius+radius)/distance),Color32::from_black_alpha(8),Stroke::NONE));
    painter.add(Shape::convex_polygon(polygon(reach,far_width),Color32::from_black_alpha(24),Stroke::NONE));
}

fn edge_distance_label(name:&str,distance:f64)->String {
    let au=distance/AU;
    if au>10.0 {format!("{name} · {au:.0} AU")} else if au>0.1 {format!("{name} · {au:.1} AU")} else {name.into()}
}

/// A small arrow at the map edge pointing toward an off-screen object.
fn draw_edge_marker(painter: &egui::Painter, rect: Rect, target: Pos2, name: &str, color: Color32, labels: &mut Labels) {
    let inner = rect.shrink(18.0);
    let dir = (target - inner.center()).normalized();
    let Some(at) = clip_to_rect(inner, inner.center(), dir) else { return };
    let side = EVec2::new(-dir.y, dir.x);
    painter.add(Shape::convex_polygon(vec![at + dir * 8.0, at - dir * 4.0 + side * 5.0, at - dir * 4.0 - side * 5.0], color.gamma_multiply(0.7), Stroke::NONE));
    labels.add(at - dir * 14.0 + EVec2::new(-10.0, 0.0), name.to_string(), color.gamma_multiply(0.7));
}

fn draw_cross(painter: &egui::Painter, p: Pos2, color: Color32) {
    let s = 5.0;
    painter.line_segment([p + EVec2::new(-s, -s), p + EVec2::new(s, s)], Stroke::new(2.0, color));
    painter.line_segment([p + EVec2::new(-s, s), p + EVec2::new(s, -s)], Stroke::new(2.0, color));
}

/// Screen direction (y up in world, y down on screen) of a world vector.
fn screen_dir(v: Vec2) -> EVec2 {
    let n = v.normalized();
    EVec2::new(n.x as f32, -n.y as f32)
}

/// A ship: arrow along thrust (or velocity when coasting), with its velocity vector
/// trailing behind. Tail length is log-scaled so 1 km/s and 0.1c both read.
/// Velocity tail behind a marker, its length proportional to Sun-frame speed.
fn draw_velocity_tail(painter: &egui::Painter, p: Pos2, vel: Vec2, color: Color32) {
    let speed = vel.length();
    if speed > 0.0 {
        let tail_px = (speed * TAIL_PX_PER_KMS).min(TAIL_MAX_PX) as f32;
        painter.line_segment([p, p - screen_dir(vel) * tail_px], Stroke::new(1.5, color.gamma_multiply(0.6)));
    }
}

fn fading_path(points:&[Pos2],color:Color32,spacing:f32)->Vec<Shape> {
    let mut dots=Shape::dotted_line(points,color,spacing,1.0);
    let last=dots.len().saturating_sub(1).max(1) as f32;
    for (i,dot) in dots.iter_mut().enumerate() {
        if let Shape::Circle(circle)=dot {circle.fill=color.gamma_multiply(1.0-i as f32/last);}
    }
    dots
}

fn draw_hit_bloom(painter:&egui::Painter,p:Pos2,color:Color32,view:&View,body:Option<BodyId>,contact:Option<ContactId>) {
    let bloom=view.combat.iter().filter(|e|e.kind==CombatKind::Impact &&
        (body.is_some() && e.own_body==body || contact.is_some() && e.contact==contact))
        .filter_map(|e|hit_bloom_style(e.impact_strength,(view.time-e.received_at).max(0.0)/view.warp.max(1.0)))
        .max_by(|a,b|a.1.total_cmp(&b.1));
    let Some((radius,intensity,strength))=bloom else {return};
    for layer in (1..=8).rev() {
        painter.circle_filled(p,radius*layer as f32/8.0,color.gamma_multiply(intensity*(0.10+0.14*strength)));
    }
    if strength>0.0 {
        painter.circle_filled(p,5.0+9.0*strength,color.gamma_multiply(intensity*0.65));
        // Brief ship-coloured ejecta make penetrating/severe impacts distinct
        // from the softer bloom of a screen absorption.
        for i in 0..8 {
            let angle=i as f32*std::f32::consts::TAU/8.0;
            let dir=EVec2::new(angle.cos(),angle.sin());
            painter.line_segment([p+dir*radius*0.55,p+dir*radius],Stroke::new(1.0+strength,color.gamma_multiply(intensity*0.8)));
        }
    }
}

fn hit_bloom_style(strength:f32,age:f64)->Option<(f32,f32,f32)> {
    let strength=strength.clamp(0.0,1.0);
    let duration=0.35+0.55*strength as f64;
    if age>=duration {return None;}
    let progress=(age/duration).clamp(0.0,1.0) as f32;
    let radius=12.0+18.0*strength+(24.0+48.0*strength)*progress.sqrt();
    Some((radius,(1.0-progress).powi(2)*(1.0+strength),strength))
}

#[test]
fn damaging_hit_blooms_are_larger_brighter_and_longer_lived() {
    let screen=hit_bloom_style(0.0,0.1).unwrap();
    let damage=hit_bloom_style(0.65,0.1).unwrap();
    let severe=hit_bloom_style(1.0,0.1).unwrap();
    assert!(severe.0>damage.0 && damage.0>screen.0);
    assert!(severe.1>damage.1 && damage.1>screen.1);
    assert!(hit_bloom_style(0.0,0.4).is_none());
    assert!(hit_bloom_style(0.65,0.4).is_some());
    assert!(hit_bloom_style(1.0,0.8).is_some());
    assert!(hit_bloom_style(1.0,1.0).is_none());
}

#[cfg(test)]
fn smooth_ship_heading(previous:Vec2,target:Vec2,dt:f64)->Vec2 {
    let angle=previous.y.atan2(previous.x);
    let difference=wrap_angle(target.y.atan2(target.x)-angle);
    let angle=angle+difference*(1.0-(-dt.min(0.1)/0.2).exp());
    Vec2::new(angle.cos(),angle.sin())
}

#[test]
fn heading_easing_is_frame_rate_independent_and_wraps_the_short_way() {
    let start=Vec2::new((-179.0_f64).to_radians().cos(),(-179.0_f64).to_radians().sin());
    let end=Vec2::new(179.0_f64.to_radians().cos(),179.0_f64.to_radians().sin());
    let single=smooth_ship_heading(start,end,0.1);
    let mut frames=start;
    for _ in 0..6 {frames=smooth_ship_heading(frames,end,0.1/6.0);}
    assert!((single-frames).length()<1e-10);
    assert!(single.x< -0.99,"rotation must cross 180 degrees, not zero");
    assert!((smooth_ship_heading(start,end,0.0)-start).length()<1e-10);
}

#[cfg(test)]
fn coast_heading(previous: Vec2, thrust: Vec2) -> Vec2 {
    if thrust.length() > 1e-5 {thrust.normalized()} else {previous}
}

#[test]
fn coasting_keeps_the_last_heading_despite_zero_or_tiny_thrust() {
    let heading=Vec2::new(-1.0,0.0);
    assert_eq!(coast_heading(heading,Vec2::ZERO),heading);
    assert_eq!(coast_heading(heading,Vec2::new(0.0,1e-7)),heading);
    assert_eq!(coast_heading(heading,Vec2::new(0.0,1.0)),Vec2::new(0.0,1.0));
}

/// Ship icons are 15 px nose to stern; full 120g burn extends four icon lengths.
fn draw_burn_vector(painter:&egui::Painter,p:Pos2,heading:Vec2,burn_g:f64,opacity:f32) {
    if !burn_g.is_finite() || burn_g<=0.0 {return;}
    let rear= -screen_dir(heading);
    let start=p+rear*5.0;
    let length=(60.0*burn_g/120.0) as f32;
    let yellow=Color32::from_rgb(255,220,60);
    for i in 0..24 {
        let a=i as f32/24.0;
        let b=(i+1) as f32/24.0;
        painter.line_segment([start+rear*(length*a),start+rear*(length*b)],
            Stroke::new(2.0,yellow.gamma_multiply(opacity*(1.0-(a+b)*0.5))));
    }
}

fn draw_ship(painter: &egui::Painter, p: Pos2, vel: Vec2, facing: Vec2, color: Color32, selected: bool) {
    let f = if facing.length() > 0.0 { screen_dir(facing) } else { EVec2::new(0.0, -1.0) };
    let side = EVec2::new(-f.y, f.x);
    draw_velocity_tail(painter, p, vel, color);
    let (len, half_w) = (10.0, 5.5);
    let nose = p + f * len;
    let pts = vec![nose, p - f * (len * 0.5) + side * half_w, p - f * (len * 0.2), p - f * (len * 0.5) - side * half_w];
    let border = if selected { Stroke::new(1.5, color) } else { Stroke::NONE };
    painter.add(Shape::convex_polygon(pts, color, border));
}

/// A resolved contact: a hollow ship arrow along its estimated velocity, not an
/// assertion about the unobserved hull facing. Nearby tracks need no outer ring.
fn draw_contact_marker(painter: &egui::Painter, p: Pos2, vel: Vec2, color: Color32, selected: bool) {
    let stroke = Stroke::new(if selected {1.5} else {1.2}, color);
    let heading = if vel.length() > 0.0 { vel.normalized() } else { Vec2::new(0.0, 1.0) };
    let forward = EVec2::new(heading.x as f32, -heading.y as f32);
    let side = EVec2::new(-forward.y, forward.x);
    let points=vec![p + forward * 10.0, p - forward * 5.0 + side * 5.5,
        p - forward * 2.0, p - forward * 5.0 - side * 5.5];
    if selected {painter.add(Shape::convex_polygon(points,CONTACT,Stroke::new(1.5,CONTACT)));}
    else {painter.add(Shape::closed_line(points,stroke));}
}

/// A missile: a small diagonal cross, kept legible at every zoom level.
fn draw_missile(painter: &egui::Painter, p: Pos2, color: Color32, selected: bool) {
    let s = 3.0;
    let stroke = Stroke::new(1.25, if selected { Color32::WHITE } else { color });
    painter.line_segment([p + EVec2::new(-s, -s), p + EVec2::new(s, s)], stroke);
    painter.line_segment([p + EVec2::new(-s, s), p + EVec2::new(s, -s)], stroke);
}

fn to_screen(cam: &Camera, rect: Rect, p: Vec2) -> Pos2 {
    let d = (p - cam.center) * (1.0 / cam.km_per_px);
    // Clamp so far-off geometry does not overflow f32 painting.
    let (x, y) = (d.x.clamp(-1e6, 1e6), d.y.clamp(-1e6, 1e6));
    rect.center() + EVec2::new(x as f32, -y as f32)
}

fn to_world(cam: &Camera, rect: Rect, p: Pos2) -> Vec2 {
    let d = p - rect.center();
    cam.center + Vec2::new(d.x as f64, -d.y as f64) * cam.km_per_px
}

// Tactical side panel: palette, scoped style and vector primitives. Everything is
// painted with egui shapes so the panel stays crisp at any scale.

const PANEL_BG: Color32 = Color32::from_rgb(7, 11, 20);
const CARD_BG: Color32 = Color32::from_rgb(11, 18, 31);
const CARD_RAISED: Color32 = Color32::from_rgb(15, 25, 42);
const WELL_BG: Color32 = Color32::from_rgb(5, 9, 16);
const EDGE: Color32 = Color32::from_rgb(36, 58, 88);
const EDGE_DIM: Color32 = Color32::from_rgb(22, 34, 52);
const ACCENT: Color32 = Color32::from_rgb(84, 206, 236);
const WARM: Color32 = Color32::from_rgb(236, 128, 70);
const HEAT: Color32 = Color32::from_rgb(240, 170, 90);
const ARMOUR: Color32 = Color32::from_rgb(128, 164, 204);
const TEXT_HI: Color32 = Color32::from_rgb(226, 238, 250);
const TEXT: Color32 = Color32::from_rgb(174, 194, 216);
const TEXT_MUTED: Color32 = Color32::from_rgb(106, 128, 156);
const TEXT_DIM: Color32 = Color32::from_rgb(62, 78, 100);
const SYS_OK: Color32 = Color32::from_rgb(70, 212, 124);
const SYS_DAMAGED: Color32 = Color32::from_rgb(244, 152, 48);
const SYS_DESTROYED: Color32 = Color32::from_rgb(238, 66, 66);
const SYS_UNKNOWN: Color32 = Color32::from_rgb(126, 134, 148);
const SYS_ABSENT: Color32 = Color32::from_rgb(58, 70, 88);

fn mono(size: f32) -> egui::FontId {
    egui::FontId::monospace(size)
}

fn header_text(text: &str) -> egui::RichText {
    egui::RichText::new(text).monospace().size(11.0).color(TEXT_HI)
}

fn panel_frame() -> egui::Frame {
    egui::Frame::new().fill(PANEL_BG).inner_margin(egui::Margin { left: 10, right: 4, top: 8, bottom: 8 })
}

/// Navy tactical widget style, scoped to the side panel.
fn panel_style(ui: &mut egui::Ui) {
    let s = ui.style_mut();
    s.spacing.item_spacing = EVec2::new(6.0, 6.0);
    s.spacing.button_padding = EVec2::new(8.0, 3.0);
    let v = &mut s.visuals;
    v.extreme_bg_color = WELL_BG;
    v.faint_bg_color = CARD_BG;
    v.selection.bg_fill = ACCENT.gamma_multiply(0.28);
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.hyperlink_color = ACCENT;
    v.warn_fg_color = SYS_DAMAGED;
    v.error_fg_color = SYS_DESTROYED;
    for (w, fill, edge, fg) in [
        (&mut v.widgets.noninteractive, CARD_BG, EDGE_DIM, TEXT),
        (&mut v.widgets.inactive, CARD_RAISED, EDGE, TEXT),
        (&mut v.widgets.hovered, Color32::from_rgb(20, 36, 58), ACCENT.gamma_multiply(0.7), TEXT_HI),
        (&mut v.widgets.active, Color32::from_rgb(24, 48, 74), ACCENT, TEXT_HI),
        (&mut v.widgets.open, CARD_RAISED, EDGE, TEXT_HI),
    ] {
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.bg_stroke = Stroke::new(1.0, edge);
        w.fg_stroke = Stroke::new(1.0, fg);
        w.corner_radius = egui::CornerRadius::same(2);
        w.expansion = 0.0;
    }
}

/// A rectangle with its top-left and bottom-right corners cut.
fn chamfer(painter: &egui::Painter, rect: Rect, cut: f32, fill: Color32, stroke: Stroke) {
    let (l, t, r, b) = (rect.left(), rect.top(), rect.right(), rect.bottom());
    painter.add(Shape::convex_polygon(
        vec![Pos2::new(l + cut, t), Pos2::new(r, t), Pos2::new(r, b - cut), Pos2::new(r - cut, b), Pos2::new(l, b), Pos2::new(l, t + cut)],
        fill,
        stroke,
    ));
}

fn brackets(painter: &egui::Painter, rect: Rect, len: f32, stroke: Stroke) {
    for (corner, dx, dy) in [(rect.left_top(), 1.0, 1.0), (rect.right_top(), -1.0, 1.0), (rect.left_bottom(), 1.0, -1.0), (rect.right_bottom(), -1.0, -1.0)] {
        painter.add(Shape::line(vec![corner + EVec2::new(dx * len, 0.0), corner, corner + EVec2::new(0.0, dy * len)], stroke));
    }
}

/// Framed panel card with corner brackets and an accent tab.
fn card<R>(ui: &mut egui::Ui, accent: Color32, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let inner = egui::Frame::new().fill(CARD_BG).stroke(Stroke::new(1.0, EDGE_DIM)).corner_radius(2)
        .inner_margin(egui::Margin::symmetric(9, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 5.0;
            add(ui)
        });
    let r = inner.response.rect;
    let p = ui.painter();
    brackets(p, r.shrink(0.5), 7.0, Stroke::new(1.0, accent.gamma_multiply(0.8)));
    p.line_segment([Pos2::new(r.left() + 12.0, r.top() + 0.5), Pos2::new(r.left() + 52.0, r.top() + 0.5)], Stroke::new(2.0, accent));
    inner.inner
}

/// Top-level section heading: accent tick, title, rule and optional aside.
fn section(ui: &mut egui::Ui, title: &str, aside: Option<(&str, Color32)>) {
    ui.add_space(4.0);
    let (rect, _) = ui.allocate_exact_size(EVec2::new(ui.available_width(), 16.0), Sense::hover());
    let p = ui.painter();
    let y = rect.center().y;
    p.rect_filled(Rect::from_min_size(Pos2::new(rect.left(), y - 5.0), EVec2::new(3.0, 10.0)), 0.0, ACCENT);
    let t = p.text(Pos2::new(rect.left() + 9.0, y), egui::Align2::LEFT_CENTER, title, mono(11.0), TEXT_HI);
    let mut end = rect.right();
    if let Some((text, color)) = aside {
        end = p.text(Pos2::new(end, y), egui::Align2::RIGHT_CENTER, text, mono(9.0), color).left() - 6.0;
    }
    if end > t.right() + 14.0 {
        p.line_segment([Pos2::new(t.right() + 7.0, y), Pos2::new(end, y)], Stroke::new(1.0, EDGE));
        p.line_segment([Pos2::new(end, y - 3.0), Pos2::new(end, y + 3.0)], Stroke::new(1.0, EDGE));
    }
}

/// Heading inside a card, with an optional status tag at the right.
fn sub_header(ui: &mut egui::Ui, title: &str, tag: Option<(&str, Color32)>) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(EVec2::new(ui.available_width(), 18.0), Sense::hover());
    let p = ui.painter();
    let y = rect.center().y;
    p.rect_filled(Rect::from_center_size(Pos2::new(rect.left() + 2.0, y), EVec2::splat(4.0)), 0.0, ACCENT.gamma_multiply(0.8));
    p.text(Pos2::new(rect.left() + 9.0, y), egui::Align2::LEFT_CENTER, title, mono(10.5), TEXT_HI);
    if let Some((text, color)) = tag {
        paint_tag(p, Pos2::new(rect.right(), y), text, color);
    }
    resp
}

fn paint_tag(p: &egui::Painter, right_center: Pos2, text: &str, color: Color32) -> Rect {
    let galley = p.layout_no_wrap(text.to_owned(), mono(9.0), color);
    let size = galley.size() + EVec2::new(10.0, 4.0);
    let rect = Rect::from_min_size(Pos2::new(right_center.x - size.x, right_center.y - size.y * 0.5), size);
    chamfer(p, rect.shrink(0.5), 3.0, color.gamma_multiply(0.12), Stroke::new(1.0, color.gamma_multiply(0.7)));
    p.galley(rect.center() - galley.size() * 0.5, galley, color);
    rect
}

fn tag(ui: &mut egui::Ui, text: &str, color: Color32) -> egui::Response {
    let size = ui.painter().layout_no_wrap(text.to_owned(), mono(9.0), color).size() + EVec2::new(10.0, 4.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    paint_tag(ui.painter(), rect.right_center(), text, color);
    resp
}

fn rule(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(EVec2::new(ui.available_width(), 5.0), Sense::hover());
    ui.painter().line_segment([rect.left_center(), rect.right_center()], Stroke::new(1.0, EDGE_DIM));
}

fn status_line(ui: &mut egui::Ui, text: &str, color: Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(EVec2::new(ui.available_width(), 14.0), Sense::hover());
    let p = ui.painter();
    let y = rect.center().y;
    p.add(Shape::convex_polygon(vec![Pos2::new(rect.left(), y - 3.5), Pos2::new(rect.left() + 4.0, y), Pos2::new(rect.left(), y + 3.5)], color, Stroke::NONE));
    p.with_clip_rect(rect).text(Pos2::new(rect.left() + 9.0, y), egui::Align2::LEFT_CENTER, text, mono(9.5), color);
    resp
}

fn hint(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(11.0).color(TEXT_MUTED));
}

/// Chamfered command button. Disabled buttons only sense hover so tooltips still show.
fn button_shortcut(text:&str)->Option<&'static str> {
    match text {
        "PING"|"ACTIVE PING"=>Some("P"),
        "SHORT"|"SHORT · BEAM"=>Some("1"),"MEDIUM"|"MEDIUM · SRM"=>Some("2"),"LONG"|"LONG · LRM"=>Some("3"),"EVADE"=>Some("0"),
        "FIT"=>Some("F"),"RUN"|"PAUSE"=>Some("SPACE"),"TRACK OWN SHIP"=>Some("T"),
        _ if text.starts_with("LRM ")=>Some("L"),
        _ if text.starts_with("SRM ")=>Some("S"),
        _=>None,
    }
}
fn tac_button(ui: &mut egui::Ui, text: &str, size: EVec2, tone: Color32, active: bool, enabled: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    if ui.is_rect_visible(rect) {
        let hover = enabled && resp.hovered();
        let pressed = enabled && resp.is_pointer_button_down_on();
        let (fill, stroke, text_color) = if !enabled {
            (WELL_BG, Stroke::new(1.0, EDGE_DIM), TEXT_DIM)
        } else if active {
            (tone.gamma_multiply(if pressed { 0.42 } else if hover { 0.34 } else { 0.24 }), Stroke::new(1.0, tone), TEXT_HI)
        } else if hover {
            (tone.gamma_multiply(if pressed { 0.24 } else { 0.14 }), Stroke::new(1.0, tone), TEXT_HI)
        } else {
            (CARD_RAISED, Stroke::new(1.0, tone.gamma_multiply(0.45)), TEXT)
        };
        chamfer(ui.painter(), rect.shrink(0.5), 5.0, fill, stroke);
        let shortcut=button_shortcut(text);
        let missile=text.starts_with("LRM ") || text.starts_with("SRM ");
        let at=rect.center()-EVec2::new(0.0,if missile {5.5} else if shortcut.is_some() {3.5} else {0.0});
        ui.painter().text(at, egui::Align2::CENTER_CENTER, text, mono(10.5), text_color);
        if let Some(key)=shortcut {
            ui.painter().text(rect.center_bottom()-EVec2::new(0.0,if missile {4.0} else {2.0}),egui::Align2::CENTER_BOTTOM,
                format!("[{key}]"),mono(7.0),if enabled {tone} else {TEXT_DIM});
        }
    }
    if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp }
}

/// Labelled segmented bar; `None` paints an unknown (hatched) value.
fn meter(ui: &mut egui::Ui, label: &str, fraction: Option<f64>, readout: &str, color: Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(EVec2::new(ui.available_width(), 13.0), Sense::hover());
    let p = ui.painter();
    let y = rect.center().y;
    p.text(Pos2::new(rect.left(), y), egui::Align2::LEFT_CENTER, label, mono(9.0), TEXT_MUTED);
    let bar = Rect::from_min_max(Pos2::new(rect.left() + 58.0, rect.top() + 2.0), Pos2::new(rect.right() - 62.0, rect.bottom() - 2.0));
    p.rect_filled(bar, 1.0, WELL_BG);
    match fraction {
        Some(f) => {
            let fill = Rect::from_min_max(bar.min, Pos2::new(bar.left() + bar.width() * f.clamp(0.0, 1.0) as f32, bar.bottom()));
            p.rect_filled(fill, 1.0, color.gamma_multiply(0.8));
            p.line_segment([fill.left_top(), fill.right_top()], Stroke::new(1.0, color));
        }
        None => {
            let hatch = p.with_clip_rect(bar);
            let mut x = bar.left() - bar.height();
            while x < bar.right() {
                hatch.line_segment([Pos2::new(x, bar.bottom()), Pos2::new(x + bar.height(), bar.top())], Stroke::new(1.0, SYS_UNKNOWN.gamma_multiply(0.35)));
                x += 5.0;
            }
        }
    }
    for i in 1..10 {
        let x = bar.left() + bar.width() * i as f32 / 10.0;
        p.line_segment([Pos2::new(x, bar.top()), Pos2::new(x, bar.bottom())], Stroke::new(1.0, WELL_BG));
    }
    p.rect_stroke(bar, 1.0, Stroke::new(1.0, EDGE), StrokeKind::Inside);
    p.text(Pos2::new(rect.right(), y), egui::Align2::RIGHT_CENTER, readout, mono(10.0), if fraction.is_some() { TEXT } else { TEXT_DIM });
    resp
}

/// A row of equal-width label/value cells.
fn readouts(ui: &mut egui::Ui, items: &[(&str, String, Color32)]) -> egui::Response {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(EVec2::new(w, 28.0), Sense::hover());
    let p = ui.painter();
    let cw = w / items.len().max(1) as f32;
    for (i, (label, value, color)) in items.iter().enumerate() {
        let mut x = rect.left() + i as f32 * cw;
        if i > 0 {
            p.line_segment([Pos2::new(x - 3.0, rect.top() + 3.0), Pos2::new(x - 3.0, rect.bottom() - 3.0)], Stroke::new(1.0, EDGE_DIM));
            x += 3.0;
        }
        let cell = p.with_clip_rect(Rect::from_min_max(Pos2::new(x, rect.top()), Pos2::new(rect.left() + (i + 1) as f32 * cw - 4.0, rect.bottom())));
        cell.text(Pos2::new(x, rect.top() + 6.0), egui::Align2::LEFT_CENTER, *label, mono(8.5), TEXT_MUTED);
        cell.text(Pos2::new(x, rect.top() + 19.0), egui::Align2::LEFT_CENTER, value.as_str(), mono(11.5), *color);
    }
    resp
}

#[derive(Clone, Copy)]
enum Glyph {
    Own,
    Hostile,
    HostileMissile,
}

/// Symbology after NATO convention: round friendly, diamond hostile.
fn paint_glyph(p: &egui::Painter, c: Pos2, glyph: Glyph, color: Color32) {
    let stroke = Stroke::new(1.2, color);
    match glyph {
        Glyph::Own => {
            p.circle_filled(c, 6.0, color.gamma_multiply(0.12));
            p.circle_stroke(c, 6.0, stroke);
            p.circle_filled(c, 1.8, color);
        }
        Glyph::Hostile | Glyph::HostileMissile => {
            let d = 7.0;
            let pts = vec![c + EVec2::new(0.0, -d), c + EVec2::new(d, 0.0), c + EVec2::new(0.0, d), c + EVec2::new(-d, 0.0)];
            p.add(Shape::convex_polygon(pts, color.gamma_multiply(0.12), stroke));
            if matches!(glyph, Glyph::HostileMissile) {
                p.line_segment([c + EVec2::new(0.0, -3.5), c + EVec2::new(0.0, 3.5)], stroke);
            } else {
                p.circle_filled(c, 1.6, color);
            }
        }
    }
}

fn card_title(ui: &mut egui::Ui, glyph: Glyph, color: Color32, title: &str, subtitle: &str, tag: Option<(&str, Color32)>) {
    let (rect, _) = ui.allocate_exact_size(EVec2::new(ui.available_width(), 32.0), Sense::hover());
    let p = ui.painter();
    paint_glyph(p, Pos2::new(rect.left() + 8.0, rect.top() + 10.0), glyph, color);
    p.text(Pos2::new(rect.left() + 22.0, rect.top() + 10.0), egui::Align2::LEFT_CENTER, title, egui::FontId::proportional(15.0), TEXT_HI);
    p.text(Pos2::new(rect.left() + 22.0, rect.top() + 26.0), egui::Align2::LEFT_CENTER, subtitle, mono(9.5), TEXT_MUTED);
    if let Some((text, c)) = tag {
        paint_tag(p, Pos2::new(rect.right(), rect.top() + 10.0), text, c);
    }
}

fn list_row(ui: &mut egui::Ui, selected: bool, glyph: Option<(Glyph, Color32)>, text: &str, tag: Option<(&str, Color32)>) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(EVec2::new(ui.available_width(), 20.0), Sense::click());
    let p = ui.painter();
    if selected || resp.hovered() {
        p.rect_filled(rect, 1.0, if selected { ACCENT.gamma_multiply(0.14) } else { CARD_RAISED });
    }
    if selected {
        p.line_segment([rect.left_top() + EVec2::new(1.0, 3.0), rect.left_bottom() + EVec2::new(1.0, -3.0)], Stroke::new(2.0, ACCENT));
    }
    let y = rect.center().y;
    if let Some((g, c)) = glyph {
        paint_glyph(p, Pos2::new(rect.left() + 13.0, y), g, c);
    }
    p.text(Pos2::new(rect.left() + 26.0, y), egui::Align2::LEFT_CENTER, text, mono(10.5), if selected { TEXT_HI } else { TEXT });
    if let Some((t, c)) = tag {
        paint_tag(p, Pos2::new(rect.right() - 2.0, y), t, c);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Magazine tile: payload, rounds available and a pip per round (outlined when queued).
fn payload_tile(ui: &mut egui::Ui, width: f32, name: &str, available: u32, queued: u32, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(EVec2::new(width, 40.0), Sense::click());
    let p = ui.painter();
    let hover = resp.hovered();
    let fill = if selected { WARM.gamma_multiply(0.16) } else if hover { CARD_RAISED } else { WELL_BG };
    let edge = if selected { WARM } else if hover { WARM.gamma_multiply(0.6) } else { EDGE };
    chamfer(p, rect.shrink(0.5), 5.0, fill, Stroke::new(1.0, edge));
    p.text(Pos2::new(rect.left() + 7.0, rect.top() + 11.0), egui::Align2::LEFT_CENTER, name, mono(9.5), if selected { TEXT_HI } else { TEXT });
    p.text(Pos2::new(rect.right() - 7.0, rect.top() + 11.0), egui::Align2::RIGHT_CENTER, available.to_string(), mono(14.0),
        if available > 0 { TEXT_HI } else { SYS_DESTROYED });
    let pips = (available + queued).clamp(10, 12) as usize;
    let pip_w = ((rect.width() - 14.0 - (pips - 1) as f32 * 2.0) / pips as f32).min(6.0);
    for k in 0..pips {
        let r = Rect::from_min_size(Pos2::new(rect.left() + 7.0 + k as f32 * (pip_w + 2.0), rect.bottom() - 11.0), EVec2::new(pip_w, 5.0));
        if k < available as usize {
            p.rect_filled(r, 0.0, if selected { WARM } else { WARM.gamma_multiply(0.7) });
        } else if k < (available + queued) as usize {
            p.rect_stroke(r, 0.0, Stroke::new(1.0, ACCENT), StrokeKind::Inside);
        } else {
            p.rect_filled(r, 0.0, EDGE_DIM);
        }
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn contact_class(c: &ContactView) -> &str {
    if let Some(class)=c.display_class.as_deref() {return class;}
    if c.resolved_missile {
        return "MISSILE";
    }
    match c.resolved_kind {
        Some(BodyKind::Ship) => "SHIP",
        Some(BodyKind::Probe) => "PROBE",
        Some(BodyKind::Station) => "STATION",
        Some(BodyKind::Missile) => "MISSILE",
        None => "UNCLASSIFIED",
    }
}

fn track_quality(c: &ContactView) -> (&'static str, Color32) {
    match c.quality {
        "Identity" => ("IDENTIFIED", SYS_OK),
        "Resolved" | "velocity resolved" => ("FIRM TRACK", SYS_OK),
        "Approximate" | "position resolution" => ("TENTATIVE TRACK", ACCENT),
        "Bearing" | "direction indication" => ("BEARING ONLY", SYS_DAMAGED),
        "stale" => ("STALE TRACK", SYS_DAMAGED),
        "lost" => ("TRACK LOST", SYS_DESTROYED),
        q => (q, TEXT_MUTED),
    }
}

fn integrity_color(f: f64) -> Color32 {
    if f > 0.66 { SYS_OK } else if f > 0.33 { SYS_DAMAGED } else { SYS_DESTROYED }
}

fn damage_bars(ui: &mut egui::Ui, report: Option<&Report>) {
    if let Some(r) = report {
        let d = &r.damage;
        let hull = (d.hull / d.hull_max.max(1e-9)).clamp(0.0, 1.0);
        let armour = (d.armour / d.armour_max.max(1.0)).clamp(0.0, 1.0);
        meter(ui, "HULL", Some(hull), &format!("{:.0}/{:.0}", d.hull, d.hull_max), integrity_color(hull)).on_hover_text("Hull integrity, HP");
        meter(ui, "ARMOUR", Some(armour), &format!("{:.0}/{:.0}", d.armour, d.armour_max), ARMOUR)
            .on_hover_text("Ablative armour: absorbs half of each penetration until exhausted");
    } else {
        meter(ui, "HULL", None, "—", SYS_UNKNOWN).on_hover_text("Unknown · active echo required");
        meter(ui, "ARMOUR", None, "—", SYS_UNKNOWN).on_hover_text("Unknown · active echo required");
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Chip {
    Intact,
    Damaged,
    Destroyed,
    Unknown,
    Absent,
    Inoperative,
    PowerOffline,
    Lifeless,
}

impl Chip {
    fn of(report: Option<Report>, system: System) -> Self {
        match report {
            None => Chip::Unknown,
            Some(r) if r.damage.lifeless() => Chip::Lifeless,
            Some(r) if !r.installed[system as usize] => Chip::Absent,
            Some(r) if system==System::Power && r.damage.state(system)==Condition::Damaged => Chip::PowerOffline,
            Some(r) if !matches!(system,System::Power|System::Screens) && !system.independent_power() && r.damage.state(System::Power)!=Condition::Intact => Chip::Inoperative,
            Some(r) if r.damage.state(system)!=Condition::Destroyed && r.operating_effectiveness(system)==0.0 => Chip::Inoperative,
            Some(r) => match r.damage.state(system) {
                Condition::Intact => Chip::Intact,
                Condition::Damaged => Chip::Damaged,
                Condition::Destroyed => Chip::Destroyed,
            },
        }
    }

    fn color(self) -> Color32 {
        match self {
            Chip::Intact => SYS_OK,
            Chip::Damaged => SYS_DAMAGED,
            Chip::Destroyed => SYS_DESTROYED,
            Chip::Unknown => Color32::from_rgb(80,150,180),
            Chip::Absent => SYS_ABSENT,
            Chip::Inoperative | Chip::Lifeless => Color32::from_rgb(125,132,143),
            Chip::PowerOffline => SYS_DAMAGED,
        }
    }

    fn describe(self) -> &'static str {
        match self {
            Chip::Intact => "Component intact",
            Chip::Damaged => "Damaged · 50%",
            Chip::Destroyed => "Destroyed · offline",
            Chip::Unknown => "Unknown or stale · not confirmed disabled · fresh active echo required",
            Chip::Absent => "Not fitted",
            Chip::Inoperative => "Inoperative · component damage or required power, ship mind or crew unavailable",
            Chip::Lifeless => "Lifeless hulk · crew and ship mind destroyed · all systems inactive",
            Chip::PowerOffline => "Power damaged · first repair priority · passive, direction finding and mind on backup; crew and damage control operational; screens capped at 50%",
        }
    }
}

fn paint_chip(p: &egui::Painter, rect: Rect, code: &str, chip: Chip) {
    let c = chip.color();
    if chip == Chip::Absent {
        let pts = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom(), rect.left_top()];
        p.extend(Shape::dashed_line(&pts, Stroke::new(1.0, c), 3.0, 2.0));
    } else {
        let (fill, edge) = match chip {
            Chip::Intact => (0.10, 0.55),
            Chip::Damaged | Chip::PowerOffline => (0.18, 0.9),
            Chip::Destroyed => (0.30, 1.0),
            _ => (0.06, 0.45),
        };
        p.rect_filled(rect, 1.0, c.gamma_multiply(fill));
        p.rect_stroke(rect, 1.0, Stroke::new(1.0, c.gamma_multiply(edge)), StrokeKind::Inside);
    }
    if matches!(chip, Chip::Intact | Chip::Damaged | Chip::Destroyed) && rect.height() > 10.0 {
        p.rect_filled(Rect::from_min_size(rect.left_top() + EVec2::new(2.0, 3.0), EVec2::new(2.0, rect.height() - 6.0)), 0.0, c);
    }
    let notch = (rect.height() * 0.4).min(5.0);
    if chip == Chip::Damaged {
        let rt = rect.right_top();
        p.add(Shape::convex_polygon(vec![rt + EVec2::new(-notch, 0.0), rt, rt + EVec2::new(0.0, notch)], c, Stroke::NONE));
    }
    if chip == Chip::Destroyed {
        p.line_segment([rect.left_bottom() + EVec2::new(2.0, -2.0), rect.right_top() + EVec2::new(-2.0, 2.0)], Stroke::new(1.0, c.gamma_multiply(0.5)));
    }
    if !code.is_empty() {
        let text = match chip {
            Chip::Destroyed => Color32::from_rgb(255, 130, 130),
            Chip::Absent => Color32::from_rgb(84, 96, 116),
            _ => c,
        };
        p.text(rect.center() + EVec2::new(1.0, 0.0), egui::Align2::CENTER_CENTER, code, mono(9.5), text);
    }
}

/// Subsystem status cards, grouped by department. Unknown stays unknown: a
/// contact's matrix only reflects its latest confirmed active echo.
fn heat_gauge_fill(rate:f64)->f64 {
    if rate<=0.0 {0.0} else {((rate.max(1.0).log10()-6.0)/9.0).clamp(1.0/28.0,1.0)}
}

fn compact_meter(ui:&mut egui::Ui,label:&str,value:Option<f64>,color:Color32) {
    compact_meter_readout(ui,label,value,color,value.map_or("—".into(),|v|format!("{:.0}%",100.0*v)));
}
fn compact_meter_readout(ui:&mut egui::Ui,label:&str,value:Option<f64>,color:Color32,readout:String) {
    let (rect,_)=ui.allocate_exact_size(EVec2::new(ui.available_width(),19.0),Sense::hover());
    let p=ui.painter();
    p.text(rect.left_top(),egui::Align2::LEFT_TOP,label,mono(8.0),TEXT_MUTED);
    p.text(rect.right_top(),egui::Align2::RIGHT_TOP,readout,mono(8.0),color);
    let bar=Rect::from_min_max(rect.left_bottom()-EVec2::new(0.0,7.0),rect.right_bottom());
    p.rect_stroke(bar,0.0,Stroke::new(1.0,EDGE),StrokeKind::Inside);
    if let Some(value)=value {
        let fill=Rect::from_min_size(bar.min,EVec2::new(bar.width()*value.clamp(0.0,1.0) as f32,bar.height()));
        p.rect_filled(fill,0.0,color.gamma_multiply(0.75));
        for i in 1..10 {let x=bar.left()+bar.width()*i as f32/10.0;p.line_segment([Pos2::new(x,bar.top()),Pos2::new(x,bar.bottom())],Stroke::new(1.0,BACKGROUND));}
    }
}

fn compact_damage(ui:&mut egui::Ui,report:Option<&Report>) {
    compact_meter_readout(ui,"HULL",report.map(|r|r.damage.hull/r.damage.hull_max),SYS_OK,report.map_or("—".into(),|r|format!("{:.0}/{:.0}",r.damage.hull,r.damage.hull_max)));
    compact_meter(ui,"ARMOUR",report.map(|r|r.damage.armour/r.damage.armour_max.max(1.0)),TEXT_MUTED);
}

fn compact_status(ui:&mut egui::Ui,report:Option<&Report>,thrust_g:Option<f64>,max_g:Option<f64>,estimated:bool) {
    let (rect,_)=ui.allocate_exact_size(EVec2::new(ui.available_width(),66.0),Sense::hover());
    let bars=Rect::from_min_max(rect.min,rect.max-EVec2::new(23.0,0.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(bars),|ui| {
        compact_damage(ui,report);
        compact_meter(ui,"SCREENS",report.filter(|r|r.installed[System::Screens as usize]).map(|r|r.screen_available),ARMOUR);
    });
    let gauge=Rect::from_min_max(Pos2::new(rect.right()-14.0,rect.top()+9.0),Pos2::new(rect.right()-6.0,rect.bottom()-13.0));
    let p=ui.painter();
    p.rect_filled(gauge,0.0,WELL_BG);
    p.rect_stroke(gauge,0.0,Stroke::new(1.0,EDGE),StrokeKind::Inside);
    if let (Some(g),Some(max_g))=(thrust_g,max_g) {
        let fraction=(g/max_g).clamp(0.0,1.0) as f32;
        p.rect_filled(Rect::from_min_max(Pos2::new(gauge.left(),gauge.bottom()-gauge.height()*fraction),gauge.max),0.0,ACCENT);
    }
    p.text(Pos2::new(gauge.center().x,rect.top()),egui::Align2::CENTER_TOP,"THR",mono(7.0),TEXT_MUTED);
    p.text(Pos2::new(gauge.center().x,rect.bottom()),egui::Align2::CENTER_BOTTOM,thrust_g.map_or("—".into(),|g|format!("{g:.0}")),mono(8.0),ACCENT);
    ui.interact(Rect::from_min_max(Pos2::new(rect.right()-22.0,rect.top()),rect.max),ui.id().with("thrust"),Sense::hover())
        .on_hover_text(thrust_g.map_or("Thrust unknown".into(),|g|format!("{}{g:.1}g thrust · {}",if estimated {"Estimated "} else {""},max_g.map_or("maximum unknown".into(),|max|format!("{:.0}% of {max:.0}g maximum",100.0*g/max)))));
}

fn compact_systems(ui:&mut egui::Ui,salt:&str,report:Option<Report>,theme:theme::Theme) {
    let groups:[(&str,&[System]);5]=[
        ("SENSORS",&[System::Passive,System::Active,System::Direction]),
        ("ELECTRONIC WARFARE",&[System::Ecm,System::Eccm]),
        ("WEAPONS",&[System::Beam,System::Launcher,System::SrmLauncher,System::PdMissiles,System::PdLaser]),
        ("ENGINEERING",&[System::Propulsion,System::Jump,System::Power,System::Screens,System::Repair]),
        ("COMMAND",&[System::Crew,System::Mind]),
    ];
    let width=ui.available_width();
    let (rect,_)=ui.allocate_exact_size(EVec2::new(width,110.0),Sense::hover());
    for (row,(name,systems)) in groups.iter().enumerate() {
        let top=rect.top()+row as f32*22.0;
        ui.painter().text(Pos2::new(rect.left(),top),egui::Align2::LEFT_TOP,*name,mono(8.0),TEXT_MUTED);
        let w=((width-12.0)/5.0).min(48.0);
        for (i,system) in systems.iter().enumerate() {
            let cell=Rect::from_min_size(Pos2::new(rect.left()+i as f32*(w+3.0),top+8.0),EVec2::new(w,13.0));
            let chip=Chip::of(report,*system);
            paint_chip(ui.painter(),cell,theme.system_code(*system),chip);
            let repair=paint_repair_progress(ui.painter(),cell,report,*system);
            ui.interact(cell,ui.id().with((salt,*system as usize)),Sense::hover()).on_hover_text(format!("{} [{}] · {}{repair}",theme.system(*system),system.code(),chip.describe()));
        }
    }
}

fn paint_repair_progress(p:&egui::Painter,cell:Rect,report:Option<Report>,system:System)->String {
    let Some(r)=report else {return String::new();};
    if r.damage.state(system)!=Condition::Damaged {return String::new();}
    if r.damage.repair_target!=Some(system) {return "\nAwaiting damage control".into();}
    let duration=luminal_core::damage::SYSTEM_REPAIR_SECONDS;
    let fraction=(r.damage.repair_progress/duration).clamp(0.0,1.0);
    let start=Pos2::new(cell.left()+2.0,cell.bottom()-1.5);
    let width=(cell.width()-4.0).max(0.0);
    p.line_segment([start,start+EVec2::new(width,0.0)],Stroke::new(2.0,EDGE));
    p.line_segment([start,start+EVec2::new(width*fraction as f32,0.0)],Stroke::new(2.0,ACCENT));
    let rate=r.damage.system_repair_rate();
    if rate<=0.0 {format!("\nRepair stalled · {:.0}%",fraction*100.0)}
    else {format!("\nRepair {:.0}% · {} remaining (at report time)",fraction*100.0,fmt_age((duration-r.damage.repair_progress).max(0.0)/rate))}
}

fn system_matrix(ui: &mut egui::Ui, salt: &str, report: Option<Report>,theme:theme::Theme) {
    const ROWS: [&[(&str, &[System])]; 3] = [
        &[("SENSORS", &[System::Passive, System::Active, System::Direction]), ("EW", &[System::Ecm, System::Eccm])],
        &[("WEAPONS", &[System::Beam, System::Launcher, System::SrmLauncher, System::PdMissiles, System::PdLaser]), ("COMMAND", &[System::Crew, System::Mind])],
        &[("ENGINEERING", &[System::Propulsion, System::Jump, System::Power, System::Screens, System::Repair])],
    ];
    let (gap, group_gap, label_h, chip_h, row_gap) = (3.0, 9.0, 10.0, 17.0, 4.0);
    let w = ui.available_width();
    let chip_w = ((w - group_gap - 4.0 * gap) / 6.0).floor().min(54.0);
    let row_h = label_h + 2.0 + chip_h;
    let (rect, _) = ui.allocate_exact_size(EVec2::new(w, ROWS.len() as f32 * (row_h + row_gap) - row_gap), Sense::hover());
    let p = ui.painter().clone();
    for (r, row) in ROWS.iter().enumerate() {
        let y = rect.top() + r as f32 * (row_h + row_gap);
        let mut x = rect.left();
        for (label, systems) in row.iter() {
            let group_w = systems.len() as f32 * (chip_w + gap) - gap;
            let ly = y + label_h * 0.5;
            let t = p.text(Pos2::new(x, ly), egui::Align2::LEFT_CENTER, *label, mono(8.5), TEXT_MUTED);
            if t.right() + 4.0 < x + group_w {
                p.line_segment([Pos2::new(t.right() + 4.0, ly), Pos2::new(x + group_w, ly)], Stroke::new(1.0, EDGE_DIM));
                p.line_segment([Pos2::new(x + group_w, ly), Pos2::new(x + group_w, ly + 3.0)], Stroke::new(1.0, EDGE_DIM));
            }
            for (i, &system) in systems.iter().enumerate() {
                let cell = Rect::from_min_size(Pos2::new(x + i as f32 * (chip_w + gap), y + label_h + 2.0), EVec2::new(chip_w, chip_h));
                let chip = Chip::of(report, system);
                paint_chip(&p, cell, theme.system_code(system), chip);
                let repair=paint_repair_progress(&p,cell,report,system);
                ui.interact(cell, ui.id().with(("system_chip", salt, system as usize)), Sense::hover()).on_hover_text(format!(
                    "{}\n{}{}{repair}",
                    theme.system(system),
                    chip.describe(),
                    if system == System::Repair { "\nOne damaged component per 20 effective minutes. Power first, then damage control. Hull +1% per hour; no armour regeneration." } else { "" }
                ));
            }
            x += group_w + group_gap;
        }
    }
}

fn system_legend(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(EVec2::new(ui.available_width(), 11.0), Sense::hover());
    let p = ui.painter().with_clip_rect(rect);
    let y = rect.center().y;
    let mut x = rect.left();
    for (chip, label) in [(Chip::Intact, "INTACT"), (Chip::Damaged, "DAMAGED"), (Chip::Destroyed, "DESTROYED"), (Chip::Unknown, "UNKNOWN"), (Chip::Absent, "NOT FITTED")] {
        paint_chip(&p, Rect::from_min_size(Pos2::new(x, y - 3.5), EVec2::new(9.0, 7.0)), "", chip);
        x = p.text(Pos2::new(x + 12.0, y), egui::Align2::LEFT_CENTER, label, mono(8.0), TEXT_MUTED).right() + 7.0;
    }
}

#[cfg(test)]
mod event_wait_ui_tests {
    use super::*;
    fn frame(app:&mut LuminalApp,ctx:&egui::Context,events:Vec<egui::Event>)->egui::FullOutput {
        let view=app.session.view(app.role);
        let mut output=ctx.run_ui(egui::RawInput {screen_rect:Some(Rect::from_min_size(Pos2::ZERO,EVec2::new(400.0,800.0))),events,..Default::default()},|ui| {app.time_bar(ui,&view);});
        output.textures_delta.clear();output
    }
    fn button(output:&egui::FullOutput,label:&str)->Pos2 {
        output.shapes.iter().find_map(|s|match &s.shape {Shape::Text(t) if t.galley.text()==label=>Some(t.pos+EVec2::new(5.0,5.0)),_=>None}).expect(label)
    }
    fn click(app:&mut LuminalApp,ctx:&egui::Context,pos:Pos2) {
        for pressed in [true,false] {frame(app,ctx,vec![egui::Event::PointerMoved(pos),egui::Event::PointerButton {pos,button:egui::PointerButton::Primary,pressed,modifiers:Default::default()}]);}
    }
    #[test]
    fn next_event_button_starts_and_stops_wait_without_auto_warp_override() {
        let mut app=LuminalApp::new_with_class(luminal_core::world::ShipClass::Destroyer);let ctx=egui::Context::default();
        let output=frame(&mut app,&ctx,vec![]);click(&mut app,&ctx,button(&output,"NEXT TACTICAL EVENT"));
        assert!(app.session.waiting_for_event());assert!(!app.auto_speed);
        let output=frame(&mut app,&ctx,vec![]);click(&mut app,&ctx,button(&output,"STOP ADVANCING"));
        assert!(!app.session.waiting_for_event());assert!(app.session.view(app.role).paused);
    }
}
