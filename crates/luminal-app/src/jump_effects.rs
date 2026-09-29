//! Blue jump effects use received events and real time, including while paused.
use super::*;
const BLUE:Color32=Color32::from_rgb(45,135,255);
const BURST_SECONDS:f64=4.0;
#[derive(Default)]
pub(super) struct JumpEffects {
    seen:BTreeSet<CombatLogKey>,
    initialized:bool,
    bursts:Vec<(Vec2,f64)>,
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
        self.bursts.retain(|(_,start)|now-start<BURST_SECONDS);
        for e in &view.jump_events {
            if self.seen.insert(key(e)) && self.initialized && matches!(e.kind,CombatKind::JumpDeparture|CombatKind::JumpArrival) && let Some(pos)=e.pos {
                self.bursts.push((pos,now));
            }
        }
        self.initialized=true;
    }
    pub(super) fn draw(&self,painter:&egui::Painter,cam:&Camera,rect:Rect,view:&View,now:f64) {
        let mut spools=Vec::new();
        for b in &view.bodies {
            if let Some(JumpState::Spooling {depart_at,..})=b.jump {
                spools.push((b.pos,1.0-((depart_at-view.time)/600.0).clamp(0.0,1.0) as f32));
            }
        }
        // Arrival can be seen before departure because jump is FTL. Fold by
        // emission time, not receipt order, so old light cannot restart a spool.
        let mut latest:BTreeMap<ContactId,&luminal_core::world::CombatEvent>=BTreeMap::new();
        for e in &view.jump_events {if let Some(c)=e.contact {
            if latest.get(&c).is_none_or(|old|e.emitted_at>=old.emitted_at) {latest.insert(c,e);}
        }}
        for (id,e) in latest {
            if e.kind==CombatKind::JumpSpool && view.time-e.received_at<600.0 && let Some(pos)=view.contacts.iter().find(|c|c.id==id).and_then(|c|c.track.as_ref()).map(|t|t.pos) {
                spools.push((pos,((view.time-e.received_at)/600.0).clamp(0.0,1.0) as f32));
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
    }
}
