//! Jump blooms and ship destruction use received events and real time, even paused.
use super::*;
const BLUE:Color32=Color32::from_rgb(45,135,255);
const BURST_SECONDS:f64=4.0;
const SHIP_EXPLOSION_SECONDS:f64=10.0;
#[derive(Default)]
pub(super) struct JumpEffects {
    seen:BTreeSet<CombatLogKey>,
    initialized:bool,
    bursts:Vec<(Vec2,f64)>,
    ship_bursts:Vec<(Vec2,f64)>,
}
fn key(e:&luminal_core::world::CombatEvent)->CombatLogKey {
    (e.emitted_at.to_bits(),e.received_at.to_bits(),e.kind as u8,e.own_body.map(|id|id.0),e.contact.map(|id|id.0))
}
fn glow(painter:&egui::Painter,p:Pos2,radius:f32,strength:f32) {
    for layer in (1..=12).rev() {
        painter.circle_filled(p,radius*layer as f32/12.0,BLUE.gamma_multiply(strength*0.06));
    }
}
impl JumpEffects {
    pub(super) fn observe(&mut self,view:&View,now:f64) {
        self.ship_bursts.retain(|(_,start)|now-start<SHIP_EXPLOSION_SECONDS);
        self.bursts.retain(|(_,start)|now-start<BURST_SECONDS);
        for e in &view.jump_events {
            if !self.seen.insert(key(e)) || !self.initialized {continue;}
            let Some(pos)=e.pos else {continue;};
            if matches!(e.kind,CombatKind::JumpDeparture|CombatKind::JumpArrival) {self.bursts.push((pos,now));}
            if e.kind==CombatKind::Destroyed && e.subject_kind==Some(BodyKind::Ship) {self.ship_bursts.push((pos,now));}
        }
        self.initialized=true;
    }
    pub(super) fn holds_auto_speed(&self,now:f64)->bool {
        self.ship_bursts.iter().any(|(_,start)|now-start<SHIP_EXPLOSION_SECONDS)
    }
    pub(super) fn draw(&self,painter:&egui::Painter,cam:&Camera,rect:Rect,view:&View,now:f64) {
        let mut spools=Vec::new();
        for b in &view.bodies {
            if let Some(JumpState::Spooling {depart_at,..})=b.jump {
                spools.push((b.pos,1.0-((depart_at-view.time)/luminal_core::world::jump::SPOOL_SECONDS).clamp(0.0,1.0) as f32));
            }
        }
        // Arrival can be seen before departure because jump is FTL. Fold by
        // emission time, not receipt order, so old light cannot restart a spool.
        let mut latest:BTreeMap<ContactId,&luminal_core::world::CombatEvent>=BTreeMap::new();
        for e in &view.jump_events {if let Some(c)=e.contact {
            if latest.get(&c).is_none_or(|old|e.emitted_at>=old.emitted_at) {latest.insert(c,e);}
        }}
        for (id,e) in latest {
            if e.kind==CombatKind::JumpSpool && view.time-e.received_at<luminal_core::world::jump::SPOOL_SECONDS && let Some(pos)=view.contacts.iter().find(|c|c.id==id).and_then(|c|c.track.as_ref()).map(|t|t.pos) {
                spools.push((pos,((view.time-e.received_at)/luminal_core::world::jump::SPOOL_SECONDS).clamp(0.0,1.0) as f32));
            }
        }
        for (pos,progress) in spools {
            let p=to_screen(cam,rect,pos);
            let pulse=(now*3.0).sin() as f32*0.08+0.92;
            let radius=(28.0+18.0*progress)*pulse;
            glow(painter,p,radius,0.75+0.25*progress);
            for i in 0..3 {
                let r=radius*(0.45+i as f32*0.22);
                painter.circle_stroke(p,r,Stroke::new(1.0,BLUE.gamma_multiply(0.28+0.2*progress)));
            }
            // Blue sparks orbit the ship throughout the complete spool.
            for i in 0..8 {
                let angle=now as f32*0.65+i as f32*std::f32::consts::TAU/8.0;
                let offset=EVec2::angled(angle)*radius*0.72;
                painter.circle_filled(p+offset,1.5,Color32::from_rgb(160,215,255).gamma_multiply(pulse));
            }
        }
        for &(pos,start) in &self.ship_bursts {
            let seconds=(now-start).max(0.0) as f32;
            let age=(seconds/SHIP_EXPLOSION_SECONDS as f32).clamp(0.0,1.0);
            let fade=(1.0-age).powf(0.8);
            let p=to_screen(cam,rect,pos);
            // An initial white flash opens into a hot asymmetric debris cloud.
            let radius=18.0+95.0*age.sqrt();
            for layer in (1..=14).rev() {
                let fraction=layer as f32/14.0;
                painter.circle_filled(p,radius*fraction,Color32::from_rgb(255,90,24).gamma_multiply(fade*(1.0-fraction*0.6)*0.10));
            }
            let flash=(-seconds*2.8).exp();
            painter.circle_filled(p,7.0+15.0*flash,Color32::from_rgb(255,242,210).gamma_multiply(fade*(0.3+0.7*flash)));
            painter.circle_stroke(p,12.0+130.0*age.sqrt(),Stroke::new(2.5*(1.0-age)+0.5,Color32::from_rgb(255,185,90).gamma_multiply(fade*0.65)));
            for i in 0..32 {
                let angle=i as f32*2.399963;
                let direction=EVec2::angled(angle);
                let speed=35.0+(i*17%53) as f32;
                let offset=direction*(8.0+speed*age);
                let tail=direction*(3.0+12.0*(1.0-age));
                let color=Color32::from_rgb(255,110+(i*13%120) as u8,45).gamma_multiply(fade);
                painter.line_segment([p+offset-tail,p+offset],Stroke::new(1.0+(i%3) as f32*0.4,color));
                // Secondary bursts spread over the first several seconds.
                let delay=(i%8) as f32*0.35;
                let local=(seconds-delay).max(0.0);
                if seconds>=delay && local<3.0 {
                    painter.circle_filled(p+offset,2.0+local*3.0,color.gamma_multiply((1.0-local/3.0)*0.35));
                }
            }
        }
        for &(pos,start) in &self.bursts {
            let age=((now-start)/BURST_SECONDS).clamp(0.0,1.0) as f32;
            let alpha=(1.0-age).powi(2);
            let p=to_screen(cam,rect,pos);
            glow(painter,p,60.0+70.0*age,alpha);
            painter.circle_filled(p,12.0*(1.0-age),Color32::from_rgb(205,235,255).gamma_multiply(alpha));
            for i in 0..3 {
                painter.circle_stroke(p,16.0+100.0*age+i as f32*11.0,Stroke::new(3.0-i as f32*0.7,BLUE.gamma_multiply(alpha)));
            }
            for angle in [0.0,std::f32::consts::FRAC_PI_2] {
                let extent=EVec2::angled(angle)*(65.0*(1.0-age));
                painter.line_segment([p-extent,p+extent],Stroke::new(2.0,Color32::from_rgb(160,220,255).gamma_multiply(alpha)));
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoint_bursts_are_fixed_in_space_and_last_four_real_seconds() {
        let app=LuminalApp::new();let mut view=app.session.view(app.role);view.jump_events.clear();
        let mut effects=JumpEffects::default();effects.observe(&view,0.0);
        let event=luminal_core::world::CombatEvent {weapon_visual:luminal_core::world::weapon_fit::WeaponVisual::Standard,velocity:None,subject_kind:Some(BodyKind::Ship),impact_strength:0.0,damage:None,contact:None,target:None,aim:None,emitted_at:600.0,received_at:600.0,pos:Some(Vec2::ZERO),kind:CombatKind::JumpDeparture,own_body:Some(BodyId(1))};
        view.jump_events=vec![event.clone(),luminal_core::world::CombatEvent {kind:CombatKind::JumpArrival,pos:Some(Vec2::new(AU,0.0)),..event}];
        view.warp=10000.0;effects.observe(&view,1.0);assert_eq!(effects.bursts.len(),2);
        view.paused=true;effects.observe(&view,4.9);assert_eq!(effects.bursts.len(),2);
        assert_eq!(effects.bursts[0].0,Vec2::ZERO);assert_eq!(effects.bursts[1].0,Vec2::new(AU,0.0));
        effects.observe(&view,5.01);assert!(effects.bursts.is_empty());
        effects.observe(&view,6.0);assert!(effects.bursts.is_empty(),"old reports must not replay");
    }    #[test]
    fn ship_destruction_lasts_and_holds_auto_speed_for_ten_real_seconds() {
        let app=LuminalApp::new();let mut view=app.session.view(app.role);view.jump_events.clear();
        let mut effects=JumpEffects::default();effects.observe(&view,0.0);
        view.jump_events.push(luminal_core::world::CombatEvent {
            weapon_visual:luminal_core::world::weapon_fit::WeaponVisual::Standard,velocity:None,subject_kind:Some(BodyKind::Ship),impact_strength:0.0,
            damage:None,contact:None,target:None,aim:None,emitted_at:50.0,received_at:60.0,pos:Some(Vec2::ZERO),kind:CombatKind::Destroyed,own_body:Some(BodyId(1))});
        effects.observe(&view,1.0);assert!(effects.holds_auto_speed(1.0));
        view.paused=true;view.warp=10000.0;effects.observe(&view,10.99);
        assert_eq!(effects.ship_bursts.len(),1);assert!(effects.holds_auto_speed(10.99));
        effects.observe(&view,11.0);assert!(!effects.holds_auto_speed(11.0));assert!(effects.ship_bursts.is_empty());
        effects.observe(&view,12.0);assert!(effects.ship_bursts.is_empty());
    }

}
