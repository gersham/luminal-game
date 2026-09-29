//! Naval recognition cards: code-drawn silhouettes, no gameplay data duplicated.
use super::*;
use luminal_core::world::ShipClass;
fn role(class:ShipClass)->&'static str {match class {ShipClass::Picket=>"SCREEN & PURSUE",ShipClass::Frigate=>"FLEXIBLE ESCORT",ShipClass::Destroyer=>"LINE COMBATANT",ShipClass::Cruiser=>"MISSILE ARTILLERY",_=>"HEAVY BATTLE LINE"}}
fn outline(p:&egui::Painter,rect:Rect,class:ShipClass,color:Color32) {
    let n=ShipClass::COMBAT.iter().position(|c|*c==class).unwrap_or(0);
    let width=rect.width()*(0.60+0.085*n as f32);
    let base=Pos2::new(rect.center().x-width*0.5,rect.center().y+13.0);
    let point=|x:f32,y:f32|base+EVec2::new(x*width,y);
    let stroke=Stroke::new(1.4,color);
    p.add(Shape::closed_line(vec![point(0.0,-9.0),point(0.08,7.0),point(0.88,7.0),point(1.0,-14.0),point(0.87,-9.0)],stroke));
    p.line_segment([point(0.1,1.0),point(0.90,1.0)],Stroke::new(0.6,color.gamma_multiply(0.45)));
    // Bridge, funnels and aerials echo a WWII recognition-book side elevation.
    for (x,w,h) in [(0.40,0.13,20.0),(0.53,0.08,28.0),(0.32,0.055,25.0)] {
        p.rect_stroke(Rect::from_min_max(point(x,-9.0-h),point(x+w,-9.0)),0.0,stroke,StrokeKind::Inside);
    }
    p.line_segment([point(0.57,-37.0),point(0.57,-53.0)],stroke);
    p.line_segment([point(0.51,-45.0),point(0.63,-45.0)],stroke);
    let turrets=if n<2 {2} else {n+1};
    for i in 0..turrets {
        let x=if i<2 {0.73+i as f32*0.13} else {0.12+(i-2) as f32*0.085};
        let pos=point(x,-13.0);
        p.rect_stroke(Rect::from_center_size(pos,EVec2::new(12.0+n as f32,9.0)),2.0,stroke,StrokeKind::Inside);
        let direction=if x>0.5 {1.0} else {-1.0};
        p.line_segment([pos+EVec2::new(4.0*direction,-2.0),pos+EVec2::new(18.0*direction,-2.0)],stroke);
        if n>=3 {p.line_segment([pos+EVec2::new(4.0*direction,1.0),pos+EVec2::new(18.0*direction,1.0)],stroke);}
    }
}
impl LuminalApp {
    pub(super) fn deploy_selected(&mut self) {
        self.restart_scenario();self.selection_pending=false;
        let _=self.session.command(self.role,Command::SetPaused(false));
    }

    pub(super) fn startup(&mut self,ui:&mut egui::Ui)->bool {
        let mut start=false;
        let width=(ui.available_width()-48.0).clamp(280.0,1700.0);
        egui::Window::new("FLEET COMMAND").anchor(egui::Align2::CENTER_CENTER,EVec2::ZERO)
            .title_bar(false).frame(panel_frame().inner_margin(18.0)).collapsible(false).resizable(false).fixed_size(EVec2::new(width,(ui.available_height()-48.0).min(800.0))).show(ui.ctx(),|ui| {
            ui.style_mut().text_styles.insert(egui::TextStyle::Body,egui::FontId::proportional(16.0));
            ui.style_mut().text_styles.insert(egui::TextStyle::Button,egui::FontId::proportional(16.0));
            ui.style_mut().text_styles.insert(egui::TextStyle::Small,egui::FontId::proportional(13.0));
            egui::ScrollArea::vertical().show(ui,|ui| {
                ui.add_space(8.0);
                ui.label(egui::RichText::new("CHOOSE YOUR COMMAND").monospace().size(28.0).color(TEXT));
                ui.label("Escort the transport. Defeat the raider. Both sides receive the same ship class.");
                ui.add_space(16.0);
                ui.label(egui::RichText::new("01  /  FLEET VOCABULARY").monospace().color(ACCENT));
                ui.horizontal_wrapped(|ui| {for theme in theme::Theme::ALL {ui.selectable_value(&mut self.theme,theme,theme.name());}});
                ui.small(self.theme.description());
                ui.small("Names only: identical weapons, flight rules, damage and balance in every theme.");
                ui.add_space(12.0);
                ui.horizontal_wrapped(|ui| {for (label,value) in [("LRM",self.theme.weapon(Payload::Nuclear)),("SRM",self.theme.weapon(Payload::Kinetic)),("BEAM",self.theme.weapon(Payload::Beam)),("JUMP",self.theme.jump()),("SCREENS",self.theme.screens())] {ui.label(egui::RichText::new(format!("{label} → {value}   ")).monospace().size(12.0).color(TEXT_MUTED));}});
                ui.add_space(20.0);
                ui.label(egui::RichText::new("02  /  SHIP RECOGNITION").monospace().color(ACCENT));
                let columns=((ui.available_width()+10.0)/270.0).floor().clamp(1.0,5.0) as usize;
                let card_width=(ui.available_width()-(columns-1) as f32*10.0)/columns as f32;
                egui::Grid::new("ship_cards").spacing(EVec2::splat(10.0)).show(ui,|ui| {
                    for (i,class) in ShipClass::COMBAT.into_iter().enumerate() {
                        let (r,response)=ui.allocate_exact_size(EVec2::new(card_width,312.0),Sense::click());
                        if response.clicked() {self.chosen_class=class;}
                        let selected=self.chosen_class==class;
                        let col=if selected {ACCENT} else {TEXT_MUTED};
                        let p=ui.painter();
                        p.rect_filled(r,5.0,if selected {Color32::from_rgb(18,39,51)} else {Color32::from_rgb(12,20,29)});
                        p.rect_stroke(r,5.0,Stroke::new(if selected {2.0} else {1.0},if response.hovered() {TEXT} else {col.gamma_multiply(0.6)}),StrokeKind::Inside);
                        p.text(r.left_top()+EVec2::new(12.0,12.0),egui::Align2::LEFT_TOP,format!("0{}  {}",i+1,class.name().to_uppercase()),mono(18.0),if selected {ACCENT} else {TEXT});
                        p.text(r.left_top()+EVec2::new(12.0,41.0),egui::Align2::LEFT_TOP,role(class),mono(11.0),TEXT_MUTED);
                        outline(p,Rect::from_min_size(r.min+EVec2::new(12.0,76.0),EVec2::new(card_width-24.0,85.0)),class,col);
                        let mag=class.magazine();
                        for (row,line) in [format!("HULL {:.0}  /  ARMOUR {:.0}",1000.0*class.scale(),500.0*class.protection()),format!("SRM {}  /  LRM {}",mag[0],mag[1]),format!("PD {}  /  INTERCEPTORS {}",class.pd_lasers(),class.interceptors()),format!("{:.0} G  /  TURN {:.0}s",class.max_g(),class.turn_seconds()),if class.has_jump_drive() {self.theme.jump().into()} else {"SUBLIGHT ONLY".into()}].iter().enumerate() {
                            p.text(r.min+EVec2::new(12.0,178.0+row as f32*20.0),egui::Align2::LEFT_TOP,line,mono(12.0),TEXT_MUTED);
                        }
                        p.text(r.left_bottom()+EVec2::new(12.0,-12.0),egui::Align2::LEFT_BOTTOM,if selected {"● SELECTED"} else {"SELECT COMMAND"},mono(12.0),col);
                        response.on_hover_text(format!("{}: {} short-range and {} long-range launchers. Sensor rating {:.0}. Screen capacity {:.1}×.",class.name(),class.launchers(Payload::Kinetic),class.launchers(Payload::Nuclear),class.sensor_rating(),class.protection()));
                        if (i+1)%columns==0 {ui.end_row();}
                    }
                });
                ui.add_space(14.0);
                if self.chosen_class==ShipClass::Battleship {ui.label(format!("{} · 60 LS · 10× beam energy · 120s cycle · forward 2° arc",self.theme.spinal()));}
                else {ui.label(format!("{} · sensors {:.0} · {} short / {} long launchers",self.chosen_class.name(),self.chosen_class.sensor_rating(),self.chosen_class.launchers(Payload::Kinetic),self.chosen_class.launchers(Payload::Nuclear)));}
                ui.add_space(10.0);
                start=tac_button(ui,&format!("DEPLOY {}",self.chosen_class.name().to_uppercase()),EVec2::new(ui.available_width(),40.0),ACCENT,true,true).clicked();
            });
        });
        start
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(app:&mut LuminalApp,ctx:&egui::Context,events:Vec<egui::Event>)->egui::FullOutput {
        let mut output=ctx.run_ui(egui::RawInput {screen_rect:Some(Rect::from_min_size(Pos2::ZERO,EVec2::new(1900.0,1000.0))),events,..Default::default()},|ui| {
            if app.startup(ui) {app.deploy_selected();}
        });
        output.textures_delta.clear();output
    }
    fn text(output:&egui::FullOutput,label:&str)->Pos2 {
        output.shapes.iter().find_map(|s|match &s.shape {Shape::Text(t) if t.galley.text()==label=>Some(t.pos+EVec2::new(8.0,6.0)),_=>None}).unwrap_or_else(||panic!("missing {label}"))
    }
    fn click(app:&mut LuminalApp,ctx:&egui::Context,pos:Pos2) {
        for pressed in [true,false] {frame(app,ctx,vec![egui::Event::PointerMoved(pos),egui::Event::PointerButton {pos,button:egui::PointerButton::Primary,pressed,modifiers:Default::default()}]);}
    }
    #[test]
    fn cards_and_theme_deploy_the_selected_class_and_survive_restart() {
        let mut app=LuminalApp::new_with_class(ShipClass::Frigate);app.theme=theme::Theme::Luminal;let ctx=egui::Context::default();
        frame(&mut app,&ctx,vec![]);let output=frame(&mut app,&ctx,vec![]);
        click(&mut app,&ctx,text(&output,"Culture"));assert_eq!(app.theme,theme::Theme::Culture);
        let output=frame(&mut app,&ctx,vec![]);click(&mut app,&ctx,text(&output,"04  CRUISER"));assert_eq!(app.chosen_class,ShipClass::Cruiser);
        // Preview changes must not instantiate a different game before deployment.
        assert_eq!(app.session.view(app.role).bodies.iter().find(|b|b.controllable).unwrap().ship_class,Some(ShipClass::Frigate));
        let output=frame(&mut app,&ctx,vec![]);click(&mut app,&ctx,text(&output,"DEPLOY CRUISER"));
        assert!(!app.selection_pending);assert_eq!(app.theme,theme::Theme::Culture);
        let before=app.session.view(Role::Spectator);
        assert_eq!(before.bodies.iter().find(|b|b.controllable).unwrap().ship_class,Some(ShipClass::Cruiser));
        app.restart_scenario();assert_eq!(app.theme,theme::Theme::Culture);
        for theme in theme::Theme::ALL {
            app.theme=theme;app.restart_scenario();
            let after=app.session.view(Role::Spectator);
            assert_eq!(before.bodies.len(),after.bodies.len());
            for (a,b) in before.bodies.iter().zip(&after.bodies) {
                assert_eq!((a.ship_class,a.magazine,a.pos,a.vel,a.damage.damage),(b.ship_class,b.magazine,b.pos,b.vel,b.damage.damage),"theme must not change the scenario");
            }
        }
    }
}
