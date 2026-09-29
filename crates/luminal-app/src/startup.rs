//! Scenario selection. The live world stays put until deploy.
//! Cards come from `Scenario::ALL`, so a new catalog row appears here on its own.
use super::*;

impl LuminalApp {
    pub(super) fn deploy_selected(&mut self) {
        if let Some(class)=self.chosen_scenario.deployed_class() {self.chosen_class=class;}
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
                ui.label(egui::RichText::new("CHOOSE YOUR SCENARIO").monospace().size(28.0).color(TEXT));
                ui.label(egui::RichText::new(BUILD_VERSION).monospace().size(12.0).color(TEXT_MUTED)).on_hover_text(build_hover());
                ui.label(self.chosen_scenario.brief());
                ui.add_space(16.0);
                ui.label(egui::RichText::new("01  /  FLEET VOCABULARY").monospace().color(ACCENT));
                ui.horizontal_wrapped(|ui| {for theme in theme::Theme::ALL {ui.selectable_value(&mut self.theme,theme,theme.name());}});
                ui.small(self.theme.description());
                ui.small(format!("{} system · {}",self.theme.star_name(),self.theme.system_description()));
                ui.small(format!("The other side is the {}.",self.theme.adversary()));
                ui.small("Distinct planetary layouts; identical ship and weapon specifications.");
                ui.add_space(12.0);
                ui.horizontal_wrapped(|ui| {for (label,value) in [("LRM",self.theme.weapon(Payload::Nuclear)),("SRM",self.theme.weapon(Payload::Kinetic)),("BEAM",self.theme.weapon(Payload::Beam)),("JUMP",self.theme.jump()),("SCREENS",self.theme.screens())] {ui.label(egui::RichText::new(format!("{label} → {value}   ")).monospace().size(12.0).color(TEXT_MUTED));}});
                ui.add_space(20.0);
                ui.label(egui::RichText::new("02  /  SCENARIO").monospace().color(ACCENT));
                let columns=((ui.available_width()+10.0)/320.0).floor().clamp(1.0,4.0) as usize;
                let card_width=(ui.available_width()-(columns-1) as f32*10.0)/columns as f32;
                egui::Grid::new("scenario_cards").spacing(EVec2::splat(10.0)).show(ui,|ui| {
                    for (i,scenario) in Scenario::ALL.into_iter().enumerate() {
                        let (r,response)=ui.allocate_exact_size(EVec2::new(card_width,268.0),Sense::click());
                        if response.clicked() {self.chosen_scenario=scenario;}
                        let selected=self.chosen_scenario==scenario;
                        let col=if selected {ACCENT} else {TEXT_MUTED};
                        let p=ui.painter().with_clip_rect(r);
                        p.rect_filled(r,5.0,if selected {Color32::from_rgb(18,39,51)} else {Color32::from_rgb(12,20,29)});
                        p.rect_stroke(r,5.0,Stroke::new(if selected {2.0} else {1.0},if response.hovered() {TEXT} else {col.gamma_multiply(0.6)}),StrokeKind::Inside);
                        p.text(r.left_top()+EVec2::new(12.0,12.0),egui::Align2::LEFT_TOP,format!("{:02}  {}",i+1,scenario.name().to_uppercase()),mono(15.0),if selected {ACCENT} else {TEXT});
                        p.text(r.left_top()+EVec2::new(12.0,38.0),egui::Align2::LEFT_TOP,scenario.forces(),mono(11.0),TEXT_MUTED);
                        let class=scenario.flagship();
                        self.ship_art.draw(ui.ctx(),&p,Rect::from_min_size(r.min+EVec2::new(12.0,62.0),EVec2::new(card_width-24.0,64.0)),self.theme,class,col);
                        let detail=p.layout(scenario.detail().to_string(),mono(12.0),TEXT_MUTED,card_width-24.0);
                        p.galley(r.min+EVec2::new(12.0,136.0),detail,TEXT_MUTED);
                        p.text(r.left_bottom()+EVec2::new(12.0,-12.0),egui::Align2::LEFT_BOTTOM,if selected {"● SELECTED"} else {"SELECT SCENARIO"},mono(12.0),col);
                        response.on_hover_text(format!("{} · your ship is a {}.",scenario.name(),self.theme.class_name(class,false)));
                        if (i+1)%columns==0 {ui.end_row();}
                    }
                });
                ui.add_space(14.0);
                ui.label(self.chosen_scenario.forces());
                ui.label(self.chosen_scenario.detail());
                ui.add_space(10.0);
                start=tac_button(ui,&format!("DEPLOY {}",self.chosen_scenario.name().to_uppercase()),EVec2::new(ui.available_width(),40.0),ACCENT,true,true).clicked();
            });
        });
        start
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use luminal_core::world::ShipClass;
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
    fn cards_and_theme_deploy_the_selected_scenario_and_survive_restart() {
        let mut app=LuminalApp::new_with_class(ShipClass::Frigate);app.theme=theme::Theme::Luminal;let ctx=egui::Context::default();
        frame(&mut app,&ctx,vec![]);let output=frame(&mut app,&ctx,vec![]);
        click(&mut app,&ctx,text(&output,"Culture"));assert_eq!(app.theme,theme::Theme::Culture);
        let output=frame(&mut app,&ctx,vec![]);click(&mut app,&ctx,text(&output,"02  RAID"));assert_eq!(app.chosen_scenario,Scenario::Raid);
        // Preview changes must not instantiate a different game before deployment.
        assert_eq!(app.session.view(app.role).bodies.iter().find(|b|b.controllable).unwrap().ship_class,Some(ShipClass::Frigate));
        let output=frame(&mut app,&ctx,vec![]);click(&mut app,&ctx,text(&output,"DEPLOY RAID"));
        assert!(!app.selection_pending);assert_eq!(app.theme,theme::Theme::Culture);assert_eq!(app.chosen_scenario,Scenario::Raid);
        let before=app.session.view(Role::Spectator);
        let player=before.bodies.iter().find(|b|b.id==BodyId(0)).unwrap();
        assert_eq!((player.ship_class,player.faction),(Some(ShipClass::Cruiser),ESCORT));
        let station=before.bodies.iter().find(|b|b.kind==BodyKind::Station).unwrap();
        assert_eq!(station.faction,RAIDER);
        let screen:Vec<_>=before.bodies.iter().filter(|b|b.faction==RAIDER && b.kind==BodyKind::Ship).map(|b|b.ship_class.unwrap()).collect();
        assert_eq!(screen,vec![ShipClass::Destroyer,ShipClass::Frigate,ShipClass::Frigate]);
        assert_eq!(before.objective.as_ref().unwrap().prize,Some(station.id));
        app.restart_scenario();assert_eq!(app.theme,theme::Theme::Culture);assert_eq!(app.chosen_scenario,Scenario::Raid);
        for theme in theme::Theme::ALL {
            app.theme=theme;app.restart_scenario();
            let after=app.session.view(Role::Spectator);
            assert_eq!(before.bodies.len(),after.bodies.len());
            if theme!=theme::Theme::Culture {assert_ne!(before.bodies[0].pos,after.bodies[0].pos,"themes use distinct physical systems");}
            for (a,b) in before.bodies.iter().zip(&after.bodies) {
                assert_eq!((a.ship_class,a.magazine,a.damage.damage),(b.ship_class,b.magazine,b.damage.damage),"theme must preserve ship fits");
            }
        }
    }
    #[test]
    fn escort_deploy_fields_destroyers_and_keeps_the_transport_in_frame() {
        let mut app=LuminalApp::new_with_class(ShipClass::Frigate);let ctx=egui::Context::default();
        frame(&mut app,&ctx,vec![]);let output=frame(&mut app,&ctx,vec![]);
        click(&mut app,&ctx,text(&output,"DEPLOY ESCORT"));
        assert_eq!(app.chosen_class,ShipClass::Destroyer);
        assert!(app.selected==Some(Selection::Body(BodyId(1))));
        assert!(app.inspected==Some(Selection::Body(BodyId(0))));
        let view=app.session.view(Role::Spectator);
        assert_eq!(view.bodies[1].ship_class,Some(ShipClass::Destroyer));
        assert_eq!(view.bodies[2].ship_class,Some(ShipClass::Destroyer));
    }
}
