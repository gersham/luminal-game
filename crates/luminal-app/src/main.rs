//! Luminal desktop client. Talks to the simulation only through `session`.

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Vec2 as EVec2};
use luminal_core::celestial::{CelestialKind, Orbit};
use luminal_core::damage::{Condition, Report, System};
use luminal_core::kinematics::{State, Vec2};
use luminal_core::mind::{ContactId, Source};
use luminal_core::sensors::{self, wrap_angle};
use luminal_core::params;
use luminal_core::scenario::{self, ESCORT, RAIDER};
use luminal_core::session::{
    AutopilotStatus, BodyId, BodyView, Command, ContactView, InterceptTarget, LocalSession, Order, Payload, Phase, Role, View,
};
use luminal_core::units::{AU, G0, LIGHT_SECOND};
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
                CombatKind::BeamPulse|CombatKind::PointDefence=>{
                    let weapon=match event.kind {CombatKind::PointDefence=>"PD LASER",_=>"MAIN BEAM"};
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
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1400.0, 900.0]).with_title("Luminal"),
        ..Default::default()
    };
    eframe::run_native("Luminal", options, Box::new(|_cc| Ok(Box::new(LuminalApp::new()))))
}

const WARPS: &[f64] = &[1.0, 5.0, 10.0, 50.0, 100.0, 1_000.0];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn player_tracking_follows_own_ship_without_changing_zoom_or_target() {
        let mut app=LuminalApp::new();
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
    fn old_target_outages_expire_instead_of_claiming_current_disablement() {
        let app=LuminalApp::new();
        let mut view=app.session.view(Role::Faction(ESCORT));
        let mut contact=view.contacts[0].clone();
        let mut report=Report {damage:Default::default(),installed:[true;15],observed_at:0.0,screen_heat:0.0};
        report.damage.systems[System::Power as usize]=Condition::Damaged;
        contact.damage=Some(report);contact.last_emitted_at=0.0;contact.last_received_at=600.0;
        view.time=600.0;
        assert!(target_system_report(&contact,&view).is_some(),"light travel does not expire a newly received snapshot");
        view.time=720.0;
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
        assert_eq!(lines[3],"HULL / SYSTEMS UNKNOWN");
        let ranged=hover_details_for_target(&view,&cam,rect,rect.center(),Some(Selection::Body(BodyId(1))),Some(Selection::Contact(view.contacts[0].id))).unwrap();
        assert!(ranged.iter().any(|line|line.starts_with("FROM OWN SHIP")));
        assert!(ranged.iter().any(|line|line==&format!("TO TARGET  {} · EST",fmt_distance(0.0))));
        view.bodies.clear();
        view.contacts.clear();
        view.celestials.clear();
        assert!(hover_details(&view,&cam,rect,rect.center()).is_none());
    }

    #[test]
    fn combat_log_reports_missile_results_and_beams_not_destruction() {
        let app=LuminalApp::new();
        let mut view=app.session.view(Role::Faction(ESCORT));
        view.contacts.clear();
        view.combat=[CombatKind::Destroyed,CombatKind::MissileHit,CombatKind::MissileMiss,CombatKind::BeamPulse].into_iter().enumerate().map(|(i,kind)|
            luminal_core::world::CombatEvent {damage:None,contact:None,aim:None,pos:None,kind,own_body:Some(BodyId(1)),emitted_at:i as f64,received_at:i as f64}).collect();
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
        view.combat=(1..=2).map(|i|luminal_core::world::CombatEvent {
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
    fn restart_restores_direction_only_target_without_truth_range() {
        let mut app=LuminalApp::new();
        app.session.tick(2.0);
        app.command(Command::SetWarp(1.0));
        app.command(Command::SetPaused(true));
        app.inspected=None;
        app.opening_fit=false;
        app.bearing_display.insert((ContactId(99),BodyId(99)),(1.0,1.0));
        app.restart_scenario();
        let view=app.session.view(Role::Faction(ESCORT));
        assert_eq!(view.time,0.0);
        assert_eq!(view.warp,50.0);
        assert!(!view.paused);
        assert!(app.fit_pending && app.opening_fit && app.bearing_display.is_empty());
        assert!(app.tactical_log.lines.is_empty());
        for id in [BodyId(1), BodyId(2)] {
            let truth = app.session.view(Role::Spectator);
            let b = truth.bodies.iter().find(|b| b.id == id).unwrap();
            assert!(b.has_screen && b.screen_up);
            assert_eq!(b.thermal.field, 1.0);
            assert_eq!(b.screen_j, 0.0);
        }
        let Some(Selection::Contact(target))=app.inspected else {panic!("enemy must be targeted")};
        let ship=view.bodies.iter().find(|b|b.id==BodyId(1)).unwrap();
        assert!(!matches!(ship.autopilot.as_ref().map(|a|a.order),Some(Order::Intercept(_))));
        let enemy=view.contacts.iter().find(|c|c.id==target).unwrap();
        assert!(enemy.last_emitted_at<0.0,"briefing must use historical light");
        assert!(!contact_has_course(enemy));
        assert!(enemy.track.is_none() && enemy.resolved_kind.is_none());
        assert!(matches!(ship.autopilot.map(|a|a.order),Some(Order::KeepRange(_,r)) if r==3.0*AU));
        assert!(enemy.bearings[0].received_at-enemy.bearings[0].emitted_at>600.0);
        assert_eq!(bearing_opacity(&enemy.bearings[0],view.time),1.0);
        assert_eq!(contact_label(enemy),"T1");
        let rect=Rect::from_min_size(Pos2::ZERO,EVec2::new(1000.0,700.0));
        app.fit(&view,rect);
        let bearing=enemy.bearings[0].bearing;
        let target_pos=ship.pos+Vec2::new(bearing.cos(),bearing.sin())*AU;
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
        let mut c=ContactView {resolved_class:None,resolved_interceptor:false,damage:None,id:ContactId(1),resolved_kind:Some(BodyKind::Ship),resolved_missile:false,
            quality:"position resolution",stale:false,bearings:vec![],
            track:Some(luminal_core::session::TrackView {velocity_sigma:1.0,pos:Vec2::ZERO,vel:Vec2::new(1.0,0.0),accel:Vec2::ZERO,
                cov:[[1.0,0.0],[0.0,1.0]],updated_at:0.0,updates:4}),
            last_emitted_at:0.0,last_received_at:0.0,last_source:Source::Echo,last_snr:10.0,last_range:Some(1.0)};
        assert!(!contact_has_course(&c));
        assert_eq!(contact_label(&c),"T1");
        c.resolved_class=Some(luminal_core::world::ShipClass::Frigate);
        assert_eq!(contact_label(&c),"FF1");
        c.quality="velocity resolved";
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
    fn hostile_ping_does_not_replace_a_resolved_ship_chevron() {
        let app=LuminalApp::new();
        let mut view=app.session.view(Role::Faction(ESCORT));
        let c=&mut view.contacts[0];
        c.track=Some(test_track());c.quality="velocity resolved";c.stale=false;
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
        assert_eq!(missile_hit_estimate(ship,None,Payload::Nuclear),0.0);
        let tr=contact.track.as_mut().unwrap();
        tr.pos=ship.pos+Vec2::new(0.01*AU,0.0);
        tr.cov=[[1.0,0.0],[0.0,1.0]];tr.velocity_sigma=0.1;
        let good=missile_hit_estimate(ship,Some(&contact),Payload::Kinetic);
        assert!(good>0.5);
        contact.track.as_mut().unwrap().cov=[[AU*AU,0.0],[0.0,AU*AU]];
        assert!(missile_hit_estimate(ship,Some(&contact),Payload::Kinetic)<0.01);
        contact.track.as_mut().unwrap().pos=ship.pos+Vec2::new(5.0*AU,0.0);
        assert_eq!(missile_hit_estimate(ship,Some(&contact),Payload::Kinetic),0.0);
    }

    #[test]
    fn power_outage_chips_preserve_backup_and_crew_conditions() {
        let mut report=Report {damage:Default::default(),installed:[true;15],observed_at:0.0,screen_heat:0.0};
        report.damage.systems[System::Power as usize]=Condition::Damaged;
        for system in System::ALL {
            let chip=Chip::of(Some(report),system);
            assert!(chip==if system==System::Power {Chip::PowerOffline}
                else if system.independent_power() {Chip::Intact} else {Chip::Inoperative},"{}",system.code());
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
        assert!(app.inspected.is_none());
    }
}
/// How far ahead to forecast committed motion, seconds.
const FORECAST_S: f64 = 2.0 * 3600.0;
/// Weight of each new bearing in the displayed running average.
const BEARING_SMOOTHING: f64 = 0.15;

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
const EMISSION: Color32 = Color32::from_rgb(255, 50, 50);

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

struct LuminalApp {
    session: LocalSession,
    role: Role,
    /// In spectator mode, whose picture to overlay on truth.
    overlay: Option<FactionId>,
    camera: Camera,
    track_player: bool,
    /// Frame the scene on the next map draw.
    fit_pending: bool,
    opening_fit: bool,
    selected: Option<Selection>,
    inspected: Option<Selection>,
    last_message: Option<String>,
    payload: Payload,
    dev: DevHooks,
    /// Displayed bearing per (contact, sensor): a running average of the noisy
    /// measurements, and the emission time of the last one folded in.
    bearing_display: BTreeMap<(ContactId, BodyId), (f64, f64)>,
    tactical_log:TacticalLog,
}

/// Development hooks driven by environment variables, used for visual checks.
/// `LUMINAL_ADVANCE=<sim seconds>` pre-runs the scenario; `LUMINAL_ROLE=spectator|raider`
/// picks the starting view; `LUMINAL_SCREENSHOT=<file.ppm>` saves a frame and exits.
#[derive(Default)]
struct DevHooks {
    screenshot: Option<std::path::PathBuf>,
    frames: u32,
}

impl LuminalApp {
    fn command_deck(&mut self,ui:&mut egui::Ui,view:&View) {
        let own=view.bodies.iter().find(|b|b.controllable && Some(b.faction)==self.own_faction());
        let target=match self.inspected {Some(Selection::Contact(id))=>view.contacts.iter().find(|c|c.id==id),_=>None};
        ui.columns(4,|columns| {
            for ui in columns.iter_mut().skip(1) {
                let rect=ui.available_rect_before_wrap();
                ui.painter().line_segment([rect.left_top()-EVec2::new(4.0,0.0),rect.left_bottom()-EVec2::new(4.0,0.0)],Stroke::new(1.0,EDGE));
            }
            let ui=&mut columns[0];
            sub_header(ui,"OWN SHIP / COMMAND",None);
            if let Some(ship)=own {
                self.compact_weapons(ui,view,ship);
                ui.horizontal(|ui| {
                    let width=(ui.available_width()-ui.spacing().item_spacing.x)/2.0;
                    if tac_button(ui,"PING",EVec2::new(width,34.0),ACCENT,false,ship.damage.operating_effectiveness(System::Active)>0.0)
                        .on_hover_text("Emit an active sensor pulse; returns arrive at light speed").clicked() {self.command(Command::Ping {body:ship.id});}
                    if tac_button(ui,if ship.screen_up {"SCREEN ON"} else {"SCREEN OFF"},EVec2::new(width,34.0),SYS_OK,ship.screen_up,ship.has_screen)
                        .on_hover_text("Toggle defensive screens").clicked() {self.command(Command::SetScreen {body:ship.id,up:!ship.screen_up});}
                });
            }
            let ui=&mut columns[1];
            sub_header(ui,"OWN SHIP",Some(("FRIENDLY",FRIEND)));
            compact_status(ui,own.map(|b|&b.damage),own.map(|b|b.thrust.length()/G0),false);
            compact_systems(ui,"own_deck",own.map(|b|b.damage));
            let ui=&mut columns[2];
            let systems=target.and_then(|c|target_system_report(c,view));
            let label=if target.is_some_and(|c|c.damage.is_some()) && systems.is_none() {"STALE ECHO"} else {"LAST ECHO"};
            sub_header(ui,"TARGET",Some((label,CONTACT)));
            let thrust=target.filter(|c|contact_has_course(c)).and_then(|c|c.track.as_ref()).map(|t|t.accel.length()/G0)
                .filter(|_|systems.is_some_and(|r|r.operating_effectiveness(System::Propulsion)>0.0));
            compact_status(ui,target.and_then(|c|c.damage.as_ref()),thrust,true);
            compact_systems(ui,"target_deck",systems);
            let ui=&mut columns[3];
            sub_header(ui,"TARGET / ORDERS",None);
            let mut selected=target.map(|c|c.id);
            egui::ComboBox::from_id_salt("deck_target").width(ui.available_width()-10.0)
                .selected_text(target.map(contact_label).unwrap_or("Designate target…".into()))
                .show_ui(ui,|ui| {for c in &view.contacts {ui.selectable_value(&mut selected,Some(c.id),contact_label(c));}});
            if let Some(id)=selected && selected!=target.map(|c|c.id) {self.select_object(Selection::Contact(id),view);}
            let contact=selected.and_then(|id|view.contacts.iter().find(|c|c.id==id));
            if let Some(c)=contact {
                if let (Some(ship),Some(track))=(own,c.track.as_ref()) {
                    ui.label(egui::RichText::new(fmt_distance((track.pos-ship.pos).length())).monospace().size(18.0).color(TEXT_HI));
                    ui.label(egui::RichText::new(track_quality(c).0).monospace().size(10.0).color(track_quality(c).1));
                    let sigma=sigma_major(track.cov);
                    ui.small(format!("Uncertainty ±{}",fmt_distance(2.0*sigma)));
                }
                if let Some(ship)=own {
                    let target=InterceptTarget::Contact(c.id);
                    let buttons=[("MATCH",Command::Intercept {body:ship.id,target},Order::Intercept(target),true),
                        ("FLYBY",Command::Flyby {body:ship.id,target},Order::Flyby(target),true),
                        ("LONG",Command::KeepRange {body:ship.id,target,range:3.0*AU},Order::KeepRange(target,3.0*AU),false),
                        ("MEDIUM",Command::KeepRange {body:ship.id,target,range:AU},Order::KeepRange(target,AU),false),
                        ("SHORT",Command::KeepRange {body:ship.id,target,range:0.03*AU},Order::KeepRange(target,0.03*AU),false),
                        ("EVADE",Command::Evade {body:ship.id,target},Order::Evade(target),false)];
                    for pair in buttons.chunks(2) {ui.horizontal(|ui| {
                        let width=(ui.available_width()-ui.spacing().item_spacing.x)/2.0;
                        for (label,command,order,needs_range) in pair {
                            let enabled=!*needs_range || (c.track.is_some() && !c.stale);
                            if tac_button(ui,label,EVec2::new(width,23.0),ACCENT,ship.autopilot.is_some_and(|a|a.order==*order),enabled).clicked() {self.command(command.clone());}
                        }
                    });}
                    if c.track.is_none() {ui.small("Bearing only · close to acquire range; EVADE burns away");}
                }
            } else {ui.weak("Select a contact on the map");}
        });
    }

    fn compact_weapons(&mut self,ui:&mut egui::Ui,view:&View,b:&BodyView) {
        let target=match self.inspected {Some(Selection::Contact(id))=>view.contacts.iter().find(|c|c.id==id),_=>None};
        ui.label(egui::RichText::new("MAIN BEAM").monospace().size(8.0).color(TEXT_MUTED));
        ui.horizontal(|ui| {
            let w=(ui.available_width()-2.0*ui.spacing().item_spacing.x)/3.0;
            if tac_button(ui,"AUTO",EVec2::new(w,20.0),ACCENT,b.beam_auto,true).clicked() {self.command(Command::ArmBeams {body:b.id});}
            if tac_button(ui,"DIRECT",EVec2::new(w,20.0),ACCENT,!b.beam_auto && b.beam_target.is_some(),target.is_some_and(|c|c.track.is_some() && !c.stale)).clicked() {self.command(Command::EngageBeam {body:b.id,target:target.map(|c|c.id)});}
            if tac_button(ui,"HOLD",EVec2::new(w,20.0),ACCENT,!b.beam_auto && b.beam_target.is_none(),true).clicked() {self.command(Command::EngageBeam {body:b.id,target:None});}
        });
        let left=(b.beam_ready_at-view.time).max(0.0);
        let status=if !b.thermal.can_fire() {"PWR / HEAT".into()} else if left>0.0 {format!("{left:.1}s")} else {"READY".into()};
        compact_meter(ui,&format!("BEAM · {status}"),Some(1.0-(left/params::SHIP_BEAM_RECHARGE_S.value).clamp(0.0,1.0)),ACCENT);
        ui.horizontal(|ui| {
            let w=(ui.available_width()-ui.spacing().item_spacing.x)/2.0;
            for (p,label) in [(Payload::Kinetic,"SRM"),(Payload::Nuclear,"LRM")] {
            let count=b.magazine[p.index()].saturating_sub(b.missile_queued[p.index()]);
            let chance=missile_hit_estimate(b,target,p);
            let ready=count>0 && missile_solution_launchable(target,p,chance) && b.damage.operating_effectiveness(System::Launcher)>0.0;
            let response=tac_button(ui,&format!("{label} {count}"),EVec2::new(w,22.0),WARM,false,ready);
            let bar=Rect::from_min_max(response.rect.left_bottom()+EVec2::new(3.0,-3.0),response.rect.right_bottom()+EVec2::new(-3.0,-1.0));
            ui.painter().rect_filled(bar,0.0,EDGE);
            ui.painter().rect_filled(Rect::from_min_size(bar.min,EVec2::new(bar.width()*chance as f32,bar.height())),0.0,if ready {ACCENT} else {SYS_UNKNOWN});
            if response.on_hover_text(format!("Fire {} · {}\nEach click queues one round; independent launcher fires every {:.0} seconds.\nBefore enemy defence. LRM permits speculative bearing-only shots; its seeker must acquire the target. SRM requires at least 1% estimated chance.",payload_label(p),if target.is_some_and(|c|c.track.is_none()) {"Bearing only · hit chance unknown".into()} else {format!("estimated hit chance {:.0}%",chance*100.0)},p.launch_interval())).clicked() && let Some(c)=target {
                self.command(Command::Launch {body:b.id,target:c.id,payload:p});
            }
        }});
        let queued=b.missile_queued.iter().sum::<u32>();
        ui.horizontal(|ui| {
            let srm=(b.missile_ready_at[Payload::Kinetic.index()]-view.time).max(0.0);
            let lrm=(b.missile_ready_at[Payload::Nuclear.index()]-view.time).max(0.0);
            ui.label(egui::RichText::new(format!("SRM {srm:.0}s · LRM {lrm:.0}s")).monospace().size(8.0).color(TEXT_MUTED));
            if ui.add_enabled(queued>0,egui::Button::new(format!("× {queued}"))).on_hover_text("Cancel queued salvo").clicked() {self.command(Command::CancelLaunches {body:b.id});}
        });
        let rounds=b.interceptor_battery.map_or(0,|x|x.rounds);
        ui.label(egui::RichText::new(format!("PD AUTO · {rounds} ROUNDS")).monospace().size(9.0).color(if rounds>0 {ACCENT} else {SYS_DAMAGED}))
            .on_hover_text(b.interceptor_battery.map_or("No launcher",|x|x.status));
        if let Some(message)=&self.last_message {ui.small(egui::RichText::new(message).color(SYS_DAMAGED));}
    }

    fn select_object(&mut self, selection: Selection, view: &View) {
        if let Selection::Body(id) = selection
            && view.bodies.iter().any(|b| b.id == id && b.controllable && b.kind == BodyKind::Ship
                && (self.role == Role::Spectator || self.own_faction() == Some(b.faction)))
        {
            self.selected = Some(selection);
            self.inspected = None;
        } else {
            self.inspected = Some(selection);
            if let Selection::Contact(contact)=selection
                && let Some(ship)=view.bodies.iter().find(|b|b.controllable && self.selected==Some(Selection::Body(b.id))) {
                self.command(Command::KeepRange {body:ship.id,target:InterceptTarget::Contact(contact),range:3.0*AU});
            }
        }
    }

    fn new() -> Self {
        let mut session=LocalSession::new(scenario::transport_intercept_debug());
        let target=session.contact_truth(Role::Spectator,ESCORT).unwrap().into_iter()
            .find_map(|(contact,body)|(body==BodyId(2)).then_some(contact)).unwrap();
        let log_path=std::path::PathBuf::from("logs/latest.log");
        let log_error=if cfg!(test) || std::env::var_os("LUMINAL_SCREENSHOT").is_some() {None} else {
            std::fs::create_dir_all("logs").and_then(|_|session.enable_debug_log(&log_path)).err()
                .map(|e|format!("Debug logfile unavailable: {e}"))
        };
        let mut app = Self {
            session,
            role: Role::Faction(ESCORT),
            overlay: Some(ESCORT),
            camera: Camera { center: Vec2::ZERO, km_per_px: AU / 500.0 },
            track_player:false,
            fit_pending: true,
            opening_fit: true,
            selected: Some(Selection::Body(BodyId(1))),
            inspected: Some(Selection::Contact(target)),
            last_message: log_error,
            payload: Payload::Kinetic,
            dev: DevHooks { screenshot: std::env::var_os("LUMINAL_SCREENSHOT").map(Into::into), frames: 0 },
            bearing_display: BTreeMap::new(),
            tactical_log:TacticalLog::default(),
        }
        .with_env_setup();
        app.session.enable_bot(if app.role == Role::Faction(RAIDER) { ESCORT } else { RAIDER }, true);
        let _ = app.session.command(app.role, Command::SetWarp(50.0));
        // Keep screenshot fixtures paused; normal play starts immediately at 50×.
        if app.dev.screenshot.is_none() {
            let _ = app.session.command(app.role, Command::SetPaused(false));
        }
        app
    }

    fn restart_scenario(&mut self) {
        *self=Self::new();
    }

    fn with_env_setup(mut self) -> Self {
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
                    ("orbit", _) => num(2).map(|c| Command::Orbit { body, celestial: c as usize }),
                    ("beam", _) => num(2).map(|c| Command::FireBeam { body, target: ContactId(c) }),
                    ("ping", _) => Some(Command::Ping { body }),
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
                self.selected = Some(Selection::Body(BodyId(2)));
            }
            _ => {}
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
        let note=match &cmd {
            Command::Launch {payload,..}=>Some(("launch",format!("MISSILE QUEUED · {}",payload_label(*payload)))),
            Command::Intercept {..}=>Some(("helm","MATCH ORDERED".into())),
            Command::Flyby {..}=>Some(("helm","FLYBY ORDERED".into())),
            Command::KeepRange {range,..}=>Some(("helm",format!("HOLD {}",fmt_distance(*range)))),
            Command::Evade {..}=>Some(("helm","EVADE ORDERED".into())),
            Command::AllStop {..}=>Some(("helm","ALL STOP ORDERED".into())),
            Command::SetScreen {up,..}=>Some(("screen",if *up {"SCREENS RAISING".into()} else {"SCREENS LOWERING".into()})),
            _=>None,
        };
        self.last_message = match self.session.command(self.role, cmd) {
            Ok(()) => {
                if let Some((key,text))=note {self.tactical_log.push(key.into(),text,ACCENT,self.session.view(self.role).time);}
                None
            },
            Err(r) => {
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
    let sign = if t < 0.0 { "-" } else { "" };
    let s = t.abs() as u64;
    format!("{sign}{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

fn fmt_age(s: f64) -> String {
    if s < 120.0 { format!("{s:.0} s") } else if s < 7200.0 { format!("{:.1} min", s / 60.0) } else { format!("{:.1} h", s / 3600.0) }
}

fn fmt_distance(km: f64) -> String {
    if km >= 0.1 * AU {
        format!("{:.2} AU", km / AU)
    } else if km >= 0.5 * LIGHT_SECOND {
        format!("{:.2} ls", km / LIGHT_SECOND)
    } else {
        format!("{km:.0} km")
    }
}

fn contact_label(c: &ContactView) -> String {
    if c.resolved_interceptor {format!("Interceptor {}",c.id.0)}
    else if c.resolved_missile {format!("Missile {}",c.id.0)}
    else if c.resolved_kind==Some(BodyKind::Probe) {format!("Probe {}",c.id.0)}
    else if c.resolved_kind==Some(BodyKind::Station) {format!("Station {}",c.id.0)}
    else if let Some(class)=c.resolved_class {format!("{}{}",class.designator(),c.id.0)}
    else {format!("T{}",c.id.0)}
}

/// LRM seekers support speculative bearing searches even without a range fix.
fn missile_solution_launchable(target:Option<&ContactView>,payload:Payload,chance:f64)->bool {
    target.is_some() && (payload==Payload::Nuclear || chance>=0.01)
}

/// Conservative UI estimate from received information only, before defence.
fn missile_hit_estimate(ship:&BodyView,target:Option<&ContactView>,payload:Payload)->f64 {
    let Some(track)=target.and_then(|c|c.track.as_ref()) else {return 0.0;};
    let range=(track.pos-ship.pos).length();
    let speed=payload.delta_v()*params::MISSILE_BURN_FRACTION.value;
    let flight=range/speed+speed/(2.0*payload.acceleration_g()*G0);
    if flight>payload.endurance() {return 0.0;}
    let sigma=(track.cov[0][0]+track.cov[1][1]).max(0.0).sqrt();
    let score=luminal_core::missile::launch_confidence(range,sigma,track.velocity_sigma,payload);
    (score*score).clamp(0.0,1.0)
}

fn contact_has_course(c:&ContactView)->bool {
    !c.stale && c.quality=="velocity resolved" && c.resolved_kind==Some(BodyKind::Ship)
        && c.track.as_ref().is_some_and(|t|t.vel.length().is_finite() && t.vel.length()>1e-6)
}

fn target_system_report(c:&ContactView,view:&View)->Option<Report> {
    let report=c.damage?;
    // Reports are historical, not a promise that the enemy remains disabled.
    // Expire at the fastest repair interval, accounting for observed light delay.
    let visible_time=c.last_emitted_at+(view.time-c.last_received_at).max(0.0);
    if visible_time-report.observed_at>=luminal_core::damage::SYSTEM_REPAIR_SECONDS {return None;}
    if report.operating_effectiveness(System::Beam)==0.0 && view.combat.iter().any(|e|
        e.contact==Some(c.id) && e.kind==CombatKind::BeamPulse && e.emitted_at>report.observed_at) {return None;}
    Some(report)
}
fn bearing_opacity(b:&luminal_core::session::BearingView,now:f64)->f32 {
    (1.0-((now-b.received_at)/BEARING_FADE_S).clamp(0.0,1.0)) as f32
}

impl eframe::App for LuminalApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let dt = ui.input(|i| i.stable_dt) as f64;
        self.session.set_watch(self.own_faction());
        self.session.tick_realtime(dt.min(0.1));
        ui.ctx().request_repaint();

        if !ui.ctx().egui_wants_keyboard_input() {
            let (space, f, t) = ui.input(|i| (i.key_pressed(egui::Key::Space), i.key_pressed(egui::Key::F),i.key_pressed(egui::Key::T)));
            if t {self.track_player = !self.track_player;self.fit_pending=false;}
            if space {
                let paused = self.session.view(Role::Spectator).paused;
                self.command(Command::SetPaused(!paused));
            }
            if f {
                self.fit_pending = true;
            }
        }

        let mut view = self.session.view(self.role);
        if !view.bodies.iter().any(|b| self.selected == Some(Selection::Body(b.id)) && b.controllable && b.kind == BodyKind::Ship) {
            self.selected = view.bodies.iter().find(|b| b.controllable && b.kind == BodyKind::Ship).map(|b| Selection::Body(b.id));
        }
        self.smooth_bearings(&mut view);
        let overlay = match (self.role, self.overlay) {
            (Role::Spectator, Some(f)) => Some((
                self.session.view(Role::Faction(f)),
                self.session.contact_truth(Role::Spectator, f).unwrap_or_default(),
            )),
            _ => None,
        };

        self.update_tactical_log(&view,ui.input(|i|i.time));
        let deck_height=(ui.available_height()*0.24).clamp(212.0,240.0);
        let frame=panel_frame().inner_margin(egui::Margin {left:8,right:8,top:5,bottom:4});
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
            restart=tac_button(ui,"RESTART",EVec2::new(66.0,23.0),WARM,false,true).on_hover_text("Fresh scenario at 50×; replaces the latest log").clicked();
        });
        for row in WARPS.chunks(3) {
            ui.horizontal(|ui| {for &warp in row {
                if tac_button(ui,&format!("{warp}×"),EVec2::new(66.0,20.0),ACCENT,view.warp==warp,true).clicked() {self.command(Command::SetWarp(warp));}
            }});
        }
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
                let heat = (b.screen_j / params::SCREEN_CAPACITY_J.value).clamp(0.0, 1.0);
                meter(ui, "POWER", Some(power), &format!("{:.0}%", 100.0 * power), ACCENT).on_hover_text("Beam capacitor charge");
                meter(ui, "SCRN HEAT", Some(heat), &format!("{:.0}%", 100.0 * heat), HEAT)
                    .on_hover_text("Energy held in the screen. Hot screens increase your signature.");
                ui.add_space(2.0);
                system_matrix(ui, "own", Some(b.damage));
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
                        .on_hover_text("Raise the protective screen. Hot screens increase your signature.").clicked()
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
                    (if closure >= 0.0 { "CLOSING" } else { "OPENING" }, format!("{:.0} km/s", closure.abs()), if closure >= 0.0 { SYS_DAMAGED } else { TEXT_HI }),
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
                    .on_hover_text("Full-thrust pass through the target's predicted position").clicked()
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
            system_matrix(ui, "target", c.damage);
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
                    if payload_tile(ui, w, payload_label(p), available, b.missile_queued[i], self.payload == p)
                        .on_hover_text(match p { Payload::Kinetic => "SRM: short-range kinetic shotgun", Payload::Nuclear => "LRM: long-range nuclear proximity warhead", Payload::Beam => "Ship beam" })
                        .clicked()
                    {
                        self.payload = p;
                    }
                }
            });
            let ammo = b.magazine[self.payload.index()].saturating_sub(b.missile_queued[self.payload.index()]);
            let label = if ammo == 0 { "MAGAZINE EMPTY".into() } else { format!("LAUNCH  ·  {}", payload_label(self.payload)) };
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
        ui.weak(format!("Power {:.0}% · weapon heat {:.0}% · field {:.0}% · {:.0} K",
            100.0*b.thermal.capacitor_j/params::BEAM_CAPACITOR_J.value,
            100.0*b.thermal.heat_j/params::BEAM_HEAT_LIMIT_J.value,100.0*b.thermal.field,
            luminal_core::thermal::Thermal::temperature(b.screen_j)));
        ui.weak(format!("Thermal emission {:.2} TW · screen {}",
            b.thermal.emission(b.screen_j)/1e12,
            if !b.has_screen { "not fitted" }
            else if b.screen_up { if b.thermal.field < 0.999 { "building" } else { "established" } }
            else if b.thermal.field > 0.001 { "collapsing / cooling" } else { "off" }));
        ui.weak(format!("Baseline signature {:.1}× · screen emissivity {:.1}×",b.baseline_emission_factor,
            luminal_core::thermal::Thermal::screen_emissivity(b.screen_j)));
        if let Some(pd)=b.point_defence {
            ui.label(format!("Point defence: automatic · {:.1}/s · {} shots",pd.rate_hz,pd.shots));
            ui.weak(format!("Laser kill chance: 50% per shot at {} ls; falls sharply beyond.",params::PD_HALF_RANGE_LS.value));
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
            ui.label(format!("{:.1} km/s relative to {}, {} above surface", rel.length(), nearest.0.name, fmt_distance(nearest.1)));
        }
        ui.label(format!("Speed {:.1} km/s (system frame)", b.vel.length()));
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
                Order::Orbit { celestial, radius, .. } => {
                    format!("orbit {} at {} altitude", view.celestials[celestial].name, fmt_distance(radius - view.celestials[celestial].radius))
                }
                Order::Intercept(InterceptTarget::Own(o)) => {
                    format!("join {}", view.bodies.iter().find(|x| x.id == o).map_or("?".into(), |x| x.name.clone()))
                }
                Order::Intercept(InterceptTarget::Contact(c)) => format!("intercept {c}"),
                Order::Flyby(InterceptTarget::Own(o)) => format!("fly by {}", view.bodies.iter().find(|x| x.id == o).map_or("?", |x| x.name.as_str())),
                Order::Flyby(InterceptTarget::Contact(c)) => format!("fly by {c}"),
                Order::KeepRange(_,range)=>format!("hold range {}",fmt_distance(range)),
                Order::Evade(_)=>"evade · maximum separation".into(),
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
            if ui.add_enabled(b.sensors.active,egui::Button::new("Ping")).on_hover_text("Send one active pulse. White ring shows round-trip detection range, fading near 1 AU. The pulse exposes you at 10× passive/direction-finding range after light travel time.").clicked() {
                self.command(Command::Ping { body: b.id });
            }
            if b.kind == BodyKind::Ship {
                let mut up = b.screen_up;
                if ui.add_enabled(b.has_screen,egui::Checkbox::new(&mut up, "Screen up")).on_hover_text("Builds gradually. Absorbed energy heats the field and radiates away; hot screens cannot collapse instantly.").changed() {
                    self.command(Command::SetScreen { body: b.id, up });
                }
                ui.label(format!(
                    "Screen {:.0} % full · hull {:.0} % damaged",
                    100.0 * b.screen_j / params::SCREEN_CAPACITY_J.value,
                    100.0 * b.hull_j / params::HULL_INTEGRITY_J.value
                ));
            }
        }
    }

    fn draw_navigation_readout(&self, painter: &egui::Painter, rect: Rect, view: &View) {
        let Some(Selection::Body(id)) = self.selected else { return };
        let Some(ship) = view.bodies.iter().find(|b| b.id == id) else { return };
        let mut lines = vec![format!("SPEED  {:>8.0} km/s    THRUST  {:.0}g", ship.vel.length(), ship.thrust.length()/G0)];
        if let Some(Selection::Contact(id)) = self.inspected
            && let Some(contact) = view.contacts.iter().find(|c| c.id == id) {
            if let Some(track) = &contact.track {
                let rel = track.pos-ship.pos;
                let closing = -(track.vel-ship.vel).dot(rel.normalized());
                lines.push(format!("RANGE  {}    {}", fmt_distance(rel.length()), contact_label(contact)));
                lines.push(if contact_has_course(contact) {
                    format!("{}  {:.0} km/s · ESTIMATED", if closing>=0.0 {"CLOSING"} else {"OPENING"}, closing.abs())
                } else {"CLOSURE UNKNOWN · BEARING / POSITION ONLY".into()});
            } else { lines.push("RANGE / CLOSURE UNKNOWN · BEARING ONLY".into()); }
        } else { lines.push("NO TARGET DESIGNATED".into()); }
        let panel = Rect::from_min_size(Pos2::new(rect.left()+10.0, rect.bottom()-85.0), EVec2::new(310.0,75.0));
        painter.rect_filled(panel,0.0,PANEL_BG.gamma_multiply(0.9));
        painter.line_segment([panel.left_top(),panel.left_bottom()],Stroke::new(2.0,ACCENT));
        painter.text(panel.min+EVec2::new(10.0,7.0),egui::Align2::LEFT_TOP,"NAVIGATION / FIRING SOLUTION",egui::FontId::monospace(9.0),ACCENT);
        for (i,line) in lines.iter().enumerate() {
            painter.text(panel.min+EVec2::new(10.0,24.0+i as f32*15.0),egui::Align2::LEFT_TOP,line,egui::FontId::monospace(10.0),TEXT_HI);
        }
    }

    fn update_player_tracking(&mut self,view:&View) {
        if self.track_player && let Some(ship)=view.bodies.iter().find(|b|b.controllable && b.kind==BodyKind::Ship) {
            self.camera.center=ship.pos;
        }
    }

    fn map(&mut self, ui: &mut egui::Ui, view: &View, overlay: Option<&(View, BTreeMap<ContactId, BodyId>)>) {
        let rect = ui.available_rect_before_wrap();
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, BACKGROUND);

        if self.fit_pending {
            self.track_player=false;
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
                let before = to_world(&self.camera, rect, hover);
                self.camera.km_per_px = (self.camera.km_per_px * (-scroll * 0.002).exp()).clamp(1e-3, 1e8);
                let after = to_world(&self.camera, rect, hover);
                self.camera.center = self.camera.center + (before - after);
            }
        }

        self.update_player_tracking(view);
        let cam = self.camera;
        let mut labels = Labels::default();
        if self.track_player {painter.text(rect.center_top()+EVec2::new(0.0,12.0),egui::Align2::CENTER_TOP,"TRACKING OWN SHIP · T",mono(10.0),ACCENT);}
        draw_range_rings(&painter, &cam, rect);
        draw_orbits(&painter, &cam, rect, view);

        // Sensor shadows cast from the selected own ship.
        if let Some(Selection::Body(id)) = self.selected
            && let Some(b) = view.bodies.iter().find(|b| b.id == id)
        {
            for c in &view.celestials {
                draw_shadow(&painter, &cam, rect, b.pos, c.pos, c.radius);
            }
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
            let label = format!("{} (goal: {})", o.name, view.bodies.iter().find(|b| b.id == o.protect).map_or("the transport".into(), |b| b.name.clone()));
            if rect.contains(center) {
                labels.add(center + EVec2::new(r * 0.7 + 4.0, -r * 0.7), label, col);
            } else {
                draw_edge_marker(&painter, rect, center, &label, col, &mut labels);
            }
        }

        // Ping is a property of the fused contact, not a second plotted estimate.

        // Observed combat flashes only: enemy effects arrive after light travel.
        for e in &view.combat {
            use luminal_core::world::CombatKind;
            if matches!(e.kind,CombatKind::Destroyed|CombatKind::Expended|CombatKind::Impact|CombatKind::MissileHit|CombatKind::MissileMiss) {continue;}
            let age=(view.time-e.received_at).max(0.0);
            let Some(pos)=e.pos else {continue};
            let p=to_screen(&cam,rect,pos);
            if matches!(e.kind,CombatKind::BeamPulse|CombatKind::PointDefence) {
                let duration=0.5_f64.max(view.warp*0.2);
                if age>=duration {continue;}
                if let Some(aim)=e.aim {
                    let color=Color32::from_rgb(150,225,255).gamma_multiply((1.0-age/duration) as f32);
                    painter.line_segment([p,to_screen(&cam,rect,aim)],Stroke::new(1.5,color));
                }
            } else {
                let duration=20.0_f64.max(view.warp*0.3);
                if age>=duration {continue;}
                let progress=(age/duration) as f32;
                let color=if e.kind==CombatKind::NuclearBurst {Color32::from_rgb(255,230,40)} else {Color32::from_rgb(255,120,80)};
                painter.circle_stroke(p,6.0+40.0*progress,Stroke::new(2.0,color.gamma_multiply(1.0-progress)));
            }
        }

        // Round-trip range, not the outbound light front. Anchor at emission.
        for front in &view.pings {
            let range = (view.time - front.t_emit).max(0.0) * LIGHT_SECOND / 2.0;
            let opacity = ((1.0 - range / front.useful_range) / 0.3).clamp(0.0, 1.0) as f32;
            if opacity > 0.0 {
                painter.circle_stroke(to_screen(&cam, rect, front.origin), (range / cam.km_per_px) as f32,
                    Stroke::new(1.5, Color32::WHITE.gamma_multiply(opacity)));
            }
        }

        for c in &view.celestials {
            let p = to_screen(&cam, rect, c.pos);
            let min_px = if c.kind == CelestialKind::Star { 6.0 } else { 3.0 };
            let r = ((c.radius / cam.km_per_px) as f32).max(min_px);
            let col = celestial_color(c.kind);
            if c.kind == CelestialKind::Star {
                painter.circle_filled(p, r * 2.2, col.gamma_multiply(0.08));
            }
            painter.circle_filled(p, r, col);
            if rect.contains(p) {
                labels.add(p + EVec2::new(r + 4.0, -r - 2.0), c.name.clone(), col.gamma_multiply(0.8));
            } else {
                draw_edge_marker(&painter, rect, p, &c.name, col, &mut labels);
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
                Order::Intercept(target) | Order::Flyby(target) | Order::KeepRange(target,_) | Order::Evade(target) => {
                    let to = match target {
                        InterceptTarget::Own(o) => view.bodies.iter().find(|x| x.id == o).map(|x| x.pos),
                        InterceptTarget::Contact(ci) => view.contacts.iter().find(|x| x.id == ci).and_then(|x| x.track.as_ref()).map(|t| t.pos),
                    };
                    if let Some(to) = to {
                        let pts = [to_screen(&cam, rect, b.pos), to_screen(&cam, rect, to)];
                        painter.extend(Shape::dashed_line(&pts, Stroke::new(1.0, c), 6.0, 4.0));
                    }
                }
            }
        }

        // Emission footprint of the selected ship, under the ships so it never hides one.
        for b in view.bodies.iter().filter(|b| self.selected == Some(Selection::Body(b.id))) {
            let w = sensors::platform_emission_w(b.kind==BodyKind::Missile,b.baseline_emission_factor,b.thrust,b.thermal.emission(b.screen_j));
            draw_emission(&painter, &cam, rect, b.pos, w);
        }

        // Only the player's commanded ship gets a defence ring, never allies/enemies.
        for b in view.bodies.iter().filter(|b| b.controllable && Some(b.faction)==self.own_faction()) {
            draw_defence_ranges(&painter,&cam,rect,b.pos,b.point_defence.is_some_and(|pd|pd.rate_hz>0.0),
                b.interceptor_battery.is_some_and(|battery|battery.rounds>0) && b.damage.operating_effectiveness(luminal_core::damage::System::PdMissiles)>0.0);
        }

        // Own (or, for the spectator, all) ships.
        for b in &view.bodies {
            let c = body_color(b, self.own_faction());
            let selected = self.selected == Some(Selection::Body(b.id));
            let p = to_screen(&cam, rect, b.pos);
            if b.kind!=BodyKind::Missile {draw_hit_bloom(&painter,p,c,view,Some(b.id),None);}
            if b.kind == BodyKind::Missile {
                if b.interceptor.is_some() {painter.circle_filled(p,2.0,c);}
                else {draw_missile(&painter, p, c, selected);}
            } else if b.kind == BodyKind::Station {
                painter.rect_filled(Rect::from_center_size(p,EVec2::splat(7.0)),0.0,c);
            } else {
                // Only the player's command ship needs a route forecast. Allied
                // autonomous platforms remain visible without map-spanning trails.
                if self.own_faction()!=Some(b.faction) || b.controllable {
                    let forecast = view.system.predict(State { pos: b.pos, vel: b.vel }, b.thrust, view.time, FORECAST_S, 120);
                    let pts: Vec<Pos2> = forecast.points.iter().map(|&p| to_screen(&cam, rect, p)).collect();
                    painter.extend(Shape::dotted_line(&pts, c.gamma_multiply(0.15), 6.0, 1.0));
                    if forecast.impact.is_some()
                        && let Some(&end) = pts.last()
                    {
                        draw_cross(&painter, end, DANGER);
                    }
                }
                draw_ship(&painter, p, b.vel, b.thrust, c, selected);
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

        // Orders. Right-click a ship or contact to intercept it, a celestial body to
        // orbit it, or empty space to fly there and stop.
        if let (Some(Selection::Body(id)), Some(click)) =
            (self.selected, resp.secondary_clicked().then(|| resp.interact_pointer_pos()).flatten())
            && let Some(b) = view.bodies.iter().find(|b| b.id == id)
            && self.own_faction() == Some(b.faction)
            && b.controllable && b.kind != BodyKind::Missile
        {
            let near = |p: Vec2, r: f32| to_screen(&cam, rect, p).distance(click) < r;
            let own = view.bodies.iter().find(|o| o.id != id && near(o.pos, 14.0)).map(|o| InterceptTarget::Own(o.id));
            let contact = view
                .contacts
                .iter()
                .find(|c| c.track.as_ref().is_some_and(|t| near(t.pos, 14.0)))
                .map(|c| InterceptTarget::Contact(c.id));
            let celestial = view.celestials.iter().position(|c| {
                let r_px = (c.radius / cam.km_per_px) as f32;
                near(c.pos, r_px.max(8.0) + 4.0)
            });
            if let Some(target) = own.or(contact) {
                if let InterceptTarget::Contact(c)=target {self.select_object(Selection::Contact(c),view);}
                else {self.command(Command::Intercept {body:id,target});}
            } else if let Some(celestial) = celestial {
                self.command(Command::Orbit { body: id, celestial });
            } else {
                self.command(Command::MoveTo { body: id, point: to_world(&cam, rect, click) });
            }
        }
        if resp.clicked()
            && let Some(click) = resp.interact_pointer_pos()
        {
            let bodies = view.bodies.iter().filter(|b| b.kind != BodyKind::Missile).map(|b| (Selection::Body(b.id), b.pos));
            let contacts = view.contacts.iter().filter_map(|c| c.track.as_ref().map(|t| (Selection::Contact(c.id), t.pos)));
            if let Some(s) = bodies
                .chain(contacts)
                .map(|(s, p)| (s, to_screen(&cam, rect, p).distance(click)))
                .filter(|(_, d)| *d < 14.0)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(s, _)| s)
            {
                self.select_object(s, view);
            }
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
    /// Replace each bearing line's newest noisy measurement with a running average, so
    /// bearing-only contacts drift rather than jump every sensor frame.
    fn smooth_bearings(&mut self, view: &mut View) {
        for c in &mut view.contacts {
            for b in &mut c.bearings {
                let key = (c.id, b.sensor);
                let shown = match self.bearing_display.get(&key) {
                    Some(&(at, _)) if at == b.emitted_at => None,
                    Some(&(_, prev)) => Some(prev + BEARING_SMOOTHING * wrap_angle(b.bearing - prev)),
                    None => Some(b.bearing),
                };
                if let Some(v) = shown {
                    self.bearing_display.insert(key, (b.emitted_at, wrap_angle(v)));
                }
                b.bearing = self.bearing_display[&key].1;
            }
        }
    }

    /// Frame own ships and contacts (or everything, for the spectator).
    fn fit(&mut self, view: &View, rect: Rect) {
        let mut pts: Vec<Vec2> = view.bodies.iter().map(|b| b.pos).collect();
        pts.extend(view.contacts.iter().filter_map(|c| c.track.as_ref().map(|t| t.pos)));
        if self.opening_fit {
            let own=view.bodies.iter().find(|b|self.selected==Some(Selection::Body(b.id)));
            let target=view.contacts.iter().find(|c|self.inspected==Some(Selection::Contact(c.id))).and_then(|c|c.track.as_ref());
            if let (Some(own),Some(target))=(own,target) {pts=vec![own.pos,target.pos];}
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
    ui.label(format!("{}. Identity unknown.", track_quality(c).0));
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
            ui.label(format!("Estimated speed {:.1} km/s, thrust {:.1} g", t.vel.length(), t.accel.length() / G0));
            if let Some(own) = view.bodies.iter().find(|b| b.controllable && b.kind == BodyKind::Ship) {
                let rel = t.pos-own.pos;
                let vel = t.vel-own.vel;
                let cpa_t = (-rel.dot(vel)/vel.dot(vel).max(1e-12)).max(0.0);
                ui.label(format!("Range {} · closure {:.0} km/s",fmt_distance(rel.length()),-vel.dot(rel.normalized())));
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

fn draw_defence_ranges(painter:&egui::Painter,cam:&Camera,rect:Rect,pos:Vec2,laser:bool,interceptors:bool) {
    let center=to_screen(cam,rect,pos);
    let radius=luminal_core::world::point_defence::defence_ring_radius(laser,interceptors);
    if radius>0.0 {
        painter.circle_stroke(center,(radius/cam.km_per_px) as f32,
            Stroke::new(1.0,Color32::from_rgba_unmultiplied(160,160,160,90)));
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
            // Use the faction's estimated contact position and its own ships only.
            // Zoom and missile positions must not change the range threshold.
            let range = view.bodies.iter().filter(|b| b.kind == BodyKind::Ship)
                .map(|b| (t.pos - b.pos).length()).fold(f64::INFINITY, f64::min);
            if range >= params::CONTACT_RING_MIN_RANGE_LS.value * LIGHT_SECOND {
                let radius = ((2.0*sigma_major(t.cov)/cam.km_per_px) as f32).max(4.0);
                painter.circle_stroke(to_screen(cam, rect, t.pos), radius, Stroke::new(1.0, color.gamma_multiply(0.45)));
                painter.circle_filled(to_screen(cam,rect,t.pos),radius,color.gamma_multiply(if ping.is_some() && !resolved_course {0.3} else {0.025}));
            }
            if contact_has_course(c) {
                let forecast = view.system.predict(State { pos: t.pos, vel: t.vel }, t.accel, view.time, FORECAST_S, 60);
                let fp: Vec<Pos2> = forecast.points.iter().map(|&p| to_screen(cam, rect, p)).collect();
                painter.extend(Shape::dotted_line(&fp, color.gamma_multiply(0.12), 8.0, 1.0));
            }
            // Footprint from the estimated thrust: what we believe it is radiating.
            if selected && contact_has_course(c) {
                draw_emission(painter, cam, rect, t.pos, sensors::ship_emission_w(t.accel));
            }
            let p = to_screen(cam, rect, t.pos);
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
            // One best bearing until fusion can establish position. Raw per-sensor
            // rays belong in diagnostics, not as duplicate tracks on the map.
            let reach = (rect.width() + rect.height()) * 2.0;
            let score=|b:&luminal_core::session::BearingView| b.sigma /
                (1.0-((view.time-b.received_at)/BEARING_FADE_S).clamp(0.0,1.0)).max(0.001);
            for b in c.bearings.iter().filter(|b|view.time-b.received_at<BEARING_FADE_S)
                .min_by(|a,b|score(a).total_cmp(&score(b))).into_iter() {
                let fade = bearing_opacity(b,view.time);
                if fade <= 0.0 {
                    continue;
                }
                let o = to_screen(cam, rect, b.origin);
                let ray = |a: f64| o + EVec2::new(a.cos() as f32, -a.sin() as f32) * reach;
                let spread = (2.0 * b.sigma).min(0.5);
                painter.add(Shape::convex_polygon(
                    vec![o, ray(b.bearing - spread), ray(b.bearing + spread)],
                    color.gamma_multiply(0.06 * fade),
                    Stroke::NONE,
                ));
                painter.line_segment([o, ray(b.bearing)], Stroke::new(if selected { 1.5 } else { 1.0 }, color.gamma_multiply(0.45 * fade)));
                {
                    let dir = EVec2::new(b.bearing.cos() as f32, -b.bearing.sin() as f32);
                    let tip = clip_to_rect(rect, o, dir).unwrap_or(o + dir * 60.0);
                    labels.add(tip - dir * 30.0, contact_label(c), color.gamma_multiply(fade.max(0.3)));
                }
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
                format!("SPEED {:.0} km/s · THRUST {:.0}g",b.vel.length(),b.thrust.length()/G0),
                format!("HULL {:.0}% · SCREEN {}",100.0*d.hull/d.hull_max,if !b.has_screen {"N/A"} else if b.screen_up {"UP"} else {"DOWN"})]
        },
        1=>{
            let c=&view.contacts[index];
            let t=c.track.as_ref().unwrap();
            vec![contact_label(c).to_uppercase(),format!("{} · {}",c.resolved_kind.map_or("UNKNOWN CLASS".into(),|k|format!("{k:?}").to_uppercase()),track_quality(c).0),
                if contact_has_course(c) {format!("EST SPEED {:.0} km/s",t.vel.length())} else {"SPEED UNKNOWN".into()},
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

/// Dim distance rings about the star, spaced to suit the zoom level.
fn draw_range_rings(painter: &egui::Painter, cam: &Camera, rect: Rect) {
    let view_km = cam.km_per_px * rect.width().max(rect.height()) as f64;
    let Some(spacing) = [0.1, 0.5, 1.0, 5.0, 10.0].into_iter().map(|a| a * AU).find(|s| view_km / s <= 12.0) else { return };
    if view_km / spacing < 1.5 {
        return;
    }
    let origin = to_screen(cam, rect, Vec2::ZERO);
    let far = [rect.left_top(), rect.right_top(), rect.left_bottom(), rect.right_bottom()]
        .into_iter()
        .map(|p| to_world(cam, rect, p).length())
        .fold(0.0, f64::max)
        .min(100.0 * AU);
    let near = if rect.contains(origin) { 0.0 } else { (to_world(cam, rect, rect.center()).length() - view_km).max(0.0) };
    let line = Color32::from_rgb(28, 34, 48);
    let text = Color32::from_rgb(80, 92, 118);
    let mut r = (near / spacing).floor().max(1.0) * spacing;
    while r <= far {
        painter.circle_stroke(origin, (r / cam.km_per_px) as f32, Stroke::new(1.0, line));
        // Label where the ring crosses the view's horizontal centre line, if it does.
        let dy = (rect.center().y - origin.y) as f64 * cam.km_per_px;
        if r > dy.abs() {
            let dx = (r * r - dy * dy).sqrt() / cam.km_per_px;
            for x in [origin.x + dx as f32, origin.x - dx as f32] {
                let p = Pos2::new(x, rect.center().y);
                if rect.contains(p) {
                    painter.text(p + EVec2::new(3.0, 0.0), egui::Align2::LEFT_CENTER, fmt_distance(r), egui::FontId::proportional(10.0), text);
                }
            }
        }
        r += spacing;
    }
}

fn draw_orbits(painter: &egui::Painter, cam: &Camera, rect: Rect, view: &View) {
    for (i, c) in view.system.bodies.iter().enumerate() {
        if let Orbit::Circular { parent, radius, .. } = c.orbit {
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
fn draw_shadow(painter: &egui::Painter, cam: &Camera, rect: Rect, eye: Vec2, center: Vec2, radius: f64) {
    let d = center - eye;
    let dist = d.length();
    if dist <= radius {
        return;
    }
    let half = (radius / dist).asin();
    let base = d.y.atan2(d.x);
    let reach = dist + (rect.width() + rect.height()) as f64 * cam.km_per_px * 2.0;
    let tangent = (dist * dist - radius * radius).sqrt();
    let edge = |a: f64, len: f64| eye + Vec2::new(a.cos(), a.sin()) * len;
    let pts = [edge(base + half, tangent), edge(base + half, reach), edge(base - half, reach), edge(base - half, tangent)];
    let screen: Vec<Pos2> = pts.iter().map(|&p| to_screen(cam, rect, p)).collect();
    if (screen[1] - screen[2]).length() < 1.0 && (screen[0] - screen[3]).length() < 1.0 {
        return;
    }
    painter.add(Shape::convex_polygon(screen, Color32::from_rgba_unmultiplied(0, 0, 0, 35), Stroke::NONE));
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

fn draw_hit_bloom(painter:&egui::Painter,p:Pos2,color:Color32,view:&View,body:Option<BodyId>,contact:Option<ContactId>) {
    let duration=0.35*view.warp.max(1.0);
    let age=view.combat.iter().filter(|e|e.kind==CombatKind::Impact &&
        (body.is_some() && e.own_body==body || contact.is_some() && e.contact==contact))
        .map(|e|(view.time-e.received_at).max(0.0)).min_by(f64::total_cmp);
    let Some(age)=age.filter(|age|*age<duration) else {return};
    let progress=(age/duration) as f32;
    let intensity=(1.0-progress).powi(2);
    let radius=12.0+24.0*progress.sqrt();
    for layer in (1..=6).rev() {
        painter.circle_filled(p,radius*layer as f32/6.0,color.gamma_multiply(intensity*0.12));
    }
}

fn draw_ship(painter: &egui::Painter, p: Pos2, vel: Vec2, thrust: Vec2, color: Color32, selected: bool) {
    let facing = if thrust.length() > 0.0 { thrust } else { vel };
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

/// Translucent red disc out to the range at which `power_w` is detected half the time.
fn draw_emission(painter: &egui::Painter, cam: &Camera, rect: Rect, pos: Vec2, power_w: f64) {
    let r = (sensors::passive_detection_range_km(power_w) / cam.km_per_px).min(1e6) as f32;
    if r < 2.0 {
        return;
    }
    let p = to_screen(cam, rect, pos);
    painter.circle_filled(p, r, EMISSION.gamma_multiply(0.06));
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
        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, text, mono(10.5), text_color);
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

fn payload_label(p: Payload) -> &'static str {
    match p {
        Payload::Kinetic => "SRM",
        Payload::Nuclear => "LRM",
        Payload::Beam => "BEAM",
    }
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

fn contact_class(c: &ContactView) -> &'static str {
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
        "velocity resolved" => ("FIRM TRACK", SYS_OK),
        "position resolution" => ("TENTATIVE TRACK", ACCENT),
        "direction indication" => ("BEARING ONLY", SYS_DAMAGED),
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
}

impl Chip {
    fn of(report: Option<Report>, system: System) -> Self {
        match report {
            None => Chip::Unknown,
            Some(r) if !r.installed[system as usize] => Chip::Absent,
            Some(r) if system==System::Power && r.damage.state(system)==Condition::Damaged => Chip::PowerOffline,
            Some(r) if system!=System::Power && !system.independent_power() && r.damage.state(System::Power)!=Condition::Intact => Chip::Inoperative,
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
            Chip::Inoperative => Color32::from_rgb(125,132,143),
            Chip::PowerOffline => SYS_DAMAGED,
        }
    }

    fn describe(self) -> &'static str {
        match self {
            Chip::Intact => "Intact · 100%",
            Chip::Damaged => "Damaged · 50%",
            Chip::Destroyed => "Destroyed · offline",
            Chip::Unknown => "Unknown or stale · not confirmed disabled · fresh active echo required",
            Chip::Absent => "Not fitted",
            Chip::Inoperative => "Inoperative · power or ship-mind dependency offline",
            Chip::PowerOffline => "Power damaged · first repair priority · passive, direction finding and mind on backup; crew and damage control operational",
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

fn compact_status(ui:&mut egui::Ui,report:Option<&Report>,thrust_g:Option<f64>,estimated:bool) {
    let (rect,_)=ui.allocate_exact_size(EVec2::new(ui.available_width(),66.0),Sense::hover());
    let bars=Rect::from_min_max(rect.min,rect.max-EVec2::new(23.0,0.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(bars),|ui| {
        compact_damage(ui,report);
        compact_meter(ui,"SCREEN HEAT",report.filter(|r|r.installed[System::Screens as usize]).map(|r|r.screen_heat/params::SCREEN_CAPACITY_J.value),HEAT);
    });
    let gauge=Rect::from_min_max(Pos2::new(rect.right()-14.0,rect.top()+9.0),Pos2::new(rect.right()-6.0,rect.bottom()-13.0));
    let p=ui.painter();
    p.rect_filled(gauge,0.0,WELL_BG);
    p.rect_stroke(gauge,0.0,Stroke::new(1.0,EDGE),StrokeKind::Inside);
    if let Some(g)=thrust_g {
        let fraction=(g/params::SHIP_MAX_ACCEL_G.value).clamp(0.0,1.0) as f32;
        p.rect_filled(Rect::from_min_max(Pos2::new(gauge.left(),gauge.bottom()-gauge.height()*fraction),gauge.max),0.0,ACCENT);
    }
    p.text(Pos2::new(gauge.center().x,rect.top()),egui::Align2::CENTER_TOP,"THR",mono(7.0),TEXT_MUTED);
    p.text(Pos2::new(gauge.center().x,rect.bottom()),egui::Align2::CENTER_BOTTOM,thrust_g.map_or("—".into(),|g|format!("{g:.0}")),mono(8.0),ACCENT);
    ui.interact(Rect::from_min_max(Pos2::new(rect.right()-22.0,rect.top()),rect.max),ui.id().with("thrust"),Sense::hover())
        .on_hover_text(thrust_g.map_or("Thrust unknown".into(),|g|format!("{}{g:.1}g thrust / {:.0}g scale",if estimated {"Estimated "} else {""},params::SHIP_MAX_ACCEL_G.value)));
}

fn compact_systems(ui:&mut egui::Ui,salt:&str,report:Option<Report>) {
    let groups:[(&str,&[System]);5]=[
        ("SENSORS",&[System::Passive,System::Active,System::Direction]),
        ("ELECTRONIC WARFARE",&[System::Ecm,System::Eccm]),
        ("WEAPONS",&[System::Beam,System::Launcher,System::PdMissiles,System::PdLaser]),
        ("ENGINEERING",&[System::Propulsion,System::Power,System::Screens,System::Repair]),
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
            paint_chip(ui.painter(),cell,system.code(),chip);
            let repair=paint_repair_progress(ui.painter(),cell,report,*system);
            ui.interact(cell,ui.id().with((salt,*system as usize)),Sense::hover()).on_hover_text(format!("{} · {}{repair}",system.name(),chip.describe()));
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
    let rate=r.damage.effectiveness(System::Repair)*r.damage.effectiveness(System::Crew);
    if rate<=0.0 {format!("\nRepair stalled · {:.0}%",fraction*100.0)}
    else {format!("\nRepair {:.0}% · {} remaining (at report time)",fraction*100.0,fmt_age((duration-r.damage.repair_progress).max(0.0)/rate))}
}

fn system_matrix(ui: &mut egui::Ui, salt: &str, report: Option<Report>) {
    const ROWS: [&[(&str, &[System])]; 3] = [
        &[("SENSORS", &[System::Passive, System::Active, System::Direction]), ("EW", &[System::Ecm, System::Eccm])],
        &[("WEAPONS", &[System::Beam, System::Launcher, System::PdMissiles, System::PdLaser]), ("COMMAND", &[System::Crew, System::Mind])],
        &[("ENGINEERING", &[System::Propulsion, System::Power, System::Screens, System::Repair])],
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
                paint_chip(&p, cell, system.code(), chip);
                let repair=paint_repair_progress(&p,cell,report,system);
                ui.interact(cell, ui.id().with(("system_chip", salt, system as usize)), Sense::hover()).on_hover_text(format!(
                    "{}\n{}{}{repair}",
                    system.name(),
                    chip.describe(),
                    if system == System::Repair { "\nOne damaged component per two effective minutes. Power first, then damage control. Hull +1% per 10 minutes; no armour regeneration." } else { "" }
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
