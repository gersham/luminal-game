//! Terminal weapon animations use only received observations and wall-clock time.
use super::*;

type MarkerKey = (bool,u32);
#[derive(Clone,Copy)]
struct Marker {pos:Vec2,velocity:Vec2,color:Color32,interceptor:bool}
struct Effect {marker:Marker,started:f64,hit:bool,warp:f64}
impl Effect {
    fn position(&self,now:f64)->Vec2 {
        self.marker.pos+self.marker.velocity*((now-self.started).clamp(0.0,1.0)*self.warp)
    }
    fn alpha(&self,now:f64)->f32 {opacity(now-self.started)}
}

#[derive(Default)]
pub(super) struct WeaponEffects {
    initialized:bool,
    seen:BTreeSet<CombatLogKey>,
    markers:BTreeMap<MarkerKey,Marker>,
    effects:Vec<Effect>,
}

fn event_key(e:&luminal_core::world::CombatEvent)->CombatLogKey {
    (e.emitted_at.to_bits(),e.received_at.to_bits(),e.kind as u8,
        e.own_body.map(|id|id.0),e.contact.map(|id|id.0))
}
fn marker_key(e:&luminal_core::world::CombatEvent)->Option<MarkerKey> {
    e.own_body.map(|id|(true,id.0)).or_else(||e.contact.map(|id|(false,id.0)))
}
fn opacity(age:f64)->f32 {(1.0-age).clamp(0.0,1.0) as f32}

impl WeaponEffects {
    pub(super) fn observe(&mut self,view:&View,faction:Option<FactionId>,now:f64) {
        self.effects.retain(|e|e.alpha(now)>0.0);
        let current:BTreeMap<_,_>=view.bodies.iter().filter(|b|b.kind==BodyKind::Missile)
            .map(|b|((true,b.id.0),Marker {pos:b.pos,velocity:b.vel,color:body_color(b,faction),interceptor:b.interceptor.is_some()}))
            .chain(view.contacts.iter().filter(|c|c.resolved_missile).filter_map(|c|c.track.as_ref().map(|t|
                ((false,c.id.0),Marker {pos:t.pos,velocity:t.vel,color:CONTACT,interceptor:c.resolved_interceptor}))))
            .collect();
        let mut ended=BTreeSet::new();
        for e in &view.combat {
            let hit=e.kind==CombatKind::MissileHit
                || (e.kind==CombatKind::Destroyed && e.subject_kind==Some(BodyKind::Missile));
            if !hit && e.kind!=CombatKind::MissileMiss {continue;}
            if let Some(key)=marker_key(e) {ended.insert(key);}
            if !self.initialized || self.seen.contains(&event_key(e)) {continue;}
            let previous=marker_key(e).and_then(|key|self.markers.get(&key).or_else(||current.get(&key))).copied();
            if e.kind==CombatKind::MissileHit && e.target.is_some() {continue;}
            let target_key=e.target.filter(|_|hit).map(|t|match t {InterceptTarget::Own(id)=>(true,id.0),InterceptTarget::Contact(id)=>(false,id.0)});
            let target=target_key.and_then(|key|current.get(&key).or_else(||self.markers.get(&key)));
            let velocity=target.map(|m|m.velocity).or(e.velocity).or(previous.map(|m|m.velocity)).unwrap_or(Vec2::ZERO);
            let event_pos=e.pos.map(|p|p+velocity*(view.time-e.emitted_at).max(0.0));
            let Some(pos)=target.map(|m|m.pos).or(event_pos).or(previous.map(|m|m.pos)) else {continue;};
            let marker=Marker {pos,velocity,
                ..previous.unwrap_or(Marker {pos,velocity:Vec2::ZERO,color:CONTACT,interceptor:false})};
            self.effects.push(Effect {marker,started:now,hit,warp:view.warp});
        }
        // Contacts may disappear without an observed outcome. Fade the last
        // received marker without inventing an explosion or revealing truth.
        if self.initialized {
            for (key,marker) in &self.markers {
                if !current.contains_key(key) && !ended.contains(key) {
                    self.effects.push(Effect {marker:*marker,started:now,hit:false,warp:view.warp});
                }
            }
        }
        self.seen=view.combat.iter().map(event_key).collect();
        self.markers=current;
        self.initialized=true;
    }

    pub(super) fn draw(&self,painter:&egui::Painter,cam:&Camera,rect:Rect,now:f64) {
        for e in &self.effects {
            let alpha=e.alpha(now);
            let p=to_screen(cam,rect,e.position(now));
            if e.hit {
                draw_missile(painter,p,e.marker.color.gamma_multiply(alpha*alpha),false);
                let radius=8.0+24.0*(1.0-alpha);
                let color=Color32::from_rgb(255,45,45);
                for layer in (1..=8).rev() {
                    painter.circle_filled(p,radius*layer as f32/8.0,color.gamma_multiply(alpha*0.18));
                }
                painter.circle_filled(p,3.0+3.0*alpha,color.gamma_multiply(alpha));
            } else {
                let color=e.marker.color.gamma_multiply(alpha);
                if e.marker.interceptor {painter.circle_filled(p,2.0,color);}
                else {draw_missile(painter,p,color,false);}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interception_removes_interceptor_and_moves_one_victim_bloom() {
        let app=LuminalApp::new();let mut view=app.session.view(app.role);
        view.combat.clear();view.contacts.clear();view.bodies.retain(|b|b.kind!=BodyKind::Missile);
        let mut effects=WeaponEffects::default();effects.observe(&view,Some(ESCORT),0.0);
        let marker=Marker {pos:Vec2::ZERO,velocity:Vec2::new(2.0,0.0),color:CONTACT,interceptor:false};
        effects.markers.insert((true,999),Marker {interceptor:true,..marker});
        effects.markers.insert((true,1000),marker);
        let hit=luminal_core::world::CombatEvent {target:Some(InterceptTarget::Own(BodyId(1000))),velocity:Some(marker.velocity),
            subject_kind:Some(BodyKind::Missile),impact_strength:0.0,damage:None,contact:None,
            aim:None,pos:Some(Vec2::ZERO),kind:CombatKind::MissileHit,emitted_at:49.0,received_at:50.0,own_body:Some(BodyId(999))};
        view.combat=vec![hit.clone(),luminal_core::world::CombatEvent {target:None,kind:CombatKind::Destroyed,own_body:Some(BodyId(1000)),..hit}];
        view.time=50.0;view.warp=1000.0;
        effects.observe(&view,Some(ESCORT),10.0);
        assert_eq!(effects.effects.len(),1,"no lingering interceptor or duplicate victim fade");
        assert!(effects.effects[0].hit);
        assert_eq!(effects.effects[0].position(10.0),Vec2::new(2.0,0.0));
        assert_eq!(effects.effects[0].position(10.5),Vec2::new(1002.0,0.0));
        effects.observe(&view,Some(ESCORT),11.01);assert!(effects.effects.is_empty());
    }
    #[test]
    fn trajectory_dots_fade_monotonically_to_transparent() {
        let dots=fading_path(&[Pos2::ZERO,Pos2::new(100.0,0.0)],Color32::WHITE,6.0);
        let alpha:Vec<_>=dots.iter().filter_map(|dot|match dot {Shape::Circle(c)=>Some(c.fill.a()),_=>None}).collect();
        assert_eq!(alpha.first(),Some(&255));
        assert_eq!(alpha.last(),Some(&0));
        assert!(alpha.windows(2).all(|a|a[0]>=a[1]));
    }

    #[test]
    fn results_animate_once_and_expire_in_wall_time_while_paused() {
        let app=LuminalApp::new();
        let mut view=app.session.view(app.role);
        view.combat.clear();
        let mut effects=WeaponEffects::default();
        effects.observe(&view,Some(ESCORT),0.0);
        view.combat=vec![luminal_core::world::CombatEvent {target:None,velocity:Some(Vec2::new(2.0,3.0)),
            subject_kind:Some(BodyKind::Missile),impact_strength:0.0,damage:None,contact:None,
            aim:None,pos:Some(Vec2::ZERO),kind:CombatKind::MissileMiss,
            emitted_at:0.0,received_at:0.0,own_body:Some(BodyId(999)),
        }];
        view.paused=true;
        view.warp=1000.0;
        effects.observe(&view,Some(ESCORT),10.0);
        assert_eq!(effects.effects.len(),1);
        assert!(!effects.effects[0].hit);
        effects.observe(&view,Some(ESCORT),10.5);
        assert_eq!(effects.effects.len(),1);
        assert_eq!(opacity(10.5-effects.effects[0].started),0.5);
        assert_eq!(effects.effects[0].position(10.5),Vec2::new(1000.0,1500.0));
        effects.observe(&view,Some(ESCORT),11.0);
        assert!(effects.effects.is_empty());
        view.combat[0].kind=CombatKind::MissileHit;
        effects.observe(&view,Some(ESCORT),12.0);
        assert!(effects.effects[0].hit);
        assert_eq!(effects.effects[0].position(12.25),Vec2::new(500.0,750.0),"bloom follows target velocity");
        effects.observe(&view,Some(ESCORT),13.01);
        assert!(effects.effects.is_empty(),"hit bloom lasts one second");
    }
}
