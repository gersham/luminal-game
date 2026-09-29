//! Deterministic doctrine consuming exactly the player's restricted view.
//! No world, target identity, or spectator access is available here.
use crate::session::{BodyView, ContactView, Command, InterceptTarget, Payload, View};
use crate::params::*;
use crate::units::{AU,G0};
use crate::world::{BodyKind,ShipClass};
use std::collections::BTreeMap;

#[derive(Default)]
pub struct Doctrine {
    retreating:std::collections::BTreeSet<crate::world::BodyId>,
    targets:BTreeMap<crate::world::BodyId,crate::mind::ContactId>,
    probe_at: BTreeMap<crate::world::BodyId, f64>,
    ping_at: BTreeMap<crate::world::BodyId, f64>,
    salvo_at: BTreeMap<crate::world::BodyId, f64>,
}

/// Earliest predicted encounter along an accelerating route to the transport.
/// Only received positions/velocities are used; no hidden target IDs or orders.
fn blocking_encounter(ship:&BodyView,transport:&ContactView,escort:&ContactView)->Option<f64> {
    let destination=transport.track.as_ref()?;
    let threat=escort.track.as_ref()?;
    let relative=destination.pos-ship.pos;
    let acceleration=SHIP_MAX_ACCEL_G.value*G0;
    let closing=(ship.vel-destination.vel).dot(relative.normalized());
    let eta=((closing*closing+2.0*acceleration*relative.length()).sqrt()-closing)/acceleration;
    let eta=eta.max(1.0);
    let burn=(relative+(destination.vel-ship.vel)*eta)*(2.0/(eta*eta));
    let uncertainty=3.0*(threat.cov[0][0].max(0.0)+threat.cov[1][1].max(0.0)).sqrt();
    let envelope=Payload::Nuclear.engagement_range()+uncertainty;
    let separation=|t:f64|ship.pos+ship.vel*t+burn*(0.5*t*t)-(threat.pos+threat.vel*t);
    let mut previous=separation(0.0);
    for step in 1..=64 {
        let t=eta*step as f64/64.0;
        let current=separation(t);
        let delta=current-previous;
        let fraction=(-previous.dot(delta)/delta.dot(delta).max(1e-12)).clamp(0.0,1.0);
        if (previous+delta*fraction).length()<=envelope {
            return Some(eta*(step as f64-1.0+fraction)/64.0);
        }
        previous=current;
    }
    None
}

fn choose_target<'a>(view:&'a View,ship:&BodyView)->(Option<&'a ContactView>,bool) {
    let contacts:Vec<_>=view.contacts.iter().filter(|c|!c.stale && c.track.is_some()
        && !c.resolved_missile && !matches!(c.resolved_kind,Some(BodyKind::Station|BodyKind::Probe))).collect();
    let nearest=||contacts.iter().copied().min_by(|a,b|
        (a.track.as_ref().unwrap().pos-ship.pos).length().total_cmp(&(b.track.as_ref().unwrap().pos-ship.pos).length()));
    if !view.objective.as_ref().is_some_and(|o|o.attacker==ship.faction) {return (nearest(),false);}
    let transport=contacts.iter().copied().filter(|c|c.resolved_class==Some(ShipClass::Transport))
        .min_by(|a,b|(a.track.as_ref().unwrap().pos-ship.pos).length().total_cmp(&(b.track.as_ref().unwrap().pos-ship.pos).length()))
        .or_else(||contacts.iter().copied().filter(|c|c.resolved_class.is_none()).min_by(|a,b| {
            let exit=view.objective.as_ref().unwrap().center;
            (a.track.as_ref().unwrap().pos-exit).length().total_cmp(&(b.track.as_ref().unwrap().pos-exit).length())
        }));
    let Some(transport)=transport else {return (nearest(),true);};
    let blocker=contacts.iter().copied().filter(|c|c.id!=transport.id && c.resolved_class!=Some(ShipClass::Transport))
        .filter_map(|escort|blocking_encounter(ship,transport,escort).map(|at|(escort,at)))
        .min_by(|a,b|a.1.total_cmp(&b.1));
    if let Some((escort,_))=blocker {(Some(escort),true)} else {(Some(transport),false)}
}

/// Keep outside the station's ranging/ping footprint where practical. Direction
/// finding can still see a burning ship farther out; this is risk reduction.
fn station_detour(ship:&BodyView,site:&crate::world::SensorSite,destination:crate::kinematics::Vec2)->Option<crate::kinematics::Vec2> {
    use crate::kinematics::Vec2;

    // Avoid the close precision-sensor zone, never chase an expanding heat signature.
    // A station is a tactical risk, not a mission-ending exclusion radius.
    let radius=if site.sensors.active {1.5*AU} else if site.sensors.passive {AU} else {0.0};
    if radius<=0.0 {return None;}
    let relative=ship.pos-site.pos;
    let distance=relative.length();
    let direction=if distance>1.0 {relative.normalized()} else {Vec2::new(1.0,0.0)};
    if distance<radius {return Some(site.pos+direction*(radius*1.1));}
    let route=destination-ship.pos;
    let closest=(-relative.dot(route)/route.dot(route).max(1.0)).clamp(0.0,1.0);
    if (relative+route*closest).length()>=radius {return None;}
    if (destination-site.pos).length()<radius {return Some(site.pos+direction*(radius*1.1));}
    let side=Vec2::new(-direction.y,direction.x);
    let tangent=radius/distance;
    let lateral=(1.0-tangent*tangent).max(0.0).sqrt();
    let a=site.pos+(direction*tangent+side*lateral)*(radius*1.1);
    let b=site.pos+(direction*tangent-side*lateral)*(radius*1.1);
    Some(if (a-destination).length()<(b-destination).length() {a} else {b})
}

impl Doctrine {
    pub fn orders(&mut self, view: &View) -> Vec<Command> {
        let mut out = vec![];
        if view.outcome.is_some() { return out; }
        // A second ranged ship contact implies an escort threat. Do not use
        // hidden ship names/loadouts to decide whether a frigate is present.
        let escort_known=view.contacts.iter().filter(|c|c.resolved_kind==Some(BodyKind::Ship)).count()>=2;
        for b in view.bodies.iter().filter(|b| b.kind == BodyKind::Ship && b.controllable && b.armed) {
            use crate::damage::{RepairGoal,System as S};
            let class=b.ship_class.unwrap_or(ShipClass::Frigate);
            let damage=&b.damage.damage;
            if b.jump.is_some() {continue;}
            let can_fight=crate::world::endgame::can_fight_again(class,damage,b.magazine);
            let fraction=damage.hull/damage.hull_max.max(1.0);
            let stranded=!crate::world::endgame::recoverable(damage,S::Propulsion)
                && !view.contacts.iter().filter(|c|!c.resolved_missile).any(|c|c.track.as_ref().is_some_and(|tr|
                    (tr.pos-b.pos).length()<=SHIP_BEAM_AUTO_RANGE_LS.value*crate::units::LIGHT_SECOND));
            if !can_fight || stranded || fraction<0.45 || damage.state(S::Power)==crate::damage::Condition::Damaged {
                self.retreating.insert(b.id);
            }
            if self.retreating.contains(&b.id) {
                if damage.repair_goal!=RepairGoal::Escape {out.push(Command::SetRepairGoal {body:b.id,goal:RepairGoal::Escape});}
                if class.has_jump_drive() && b.damage.operating_effectiveness(S::Jump)>0.0 {
                    out.push(Command::Withdraw {body:b.id});
                } else if class.has_jump_drive() && crate::world::endgame::recoverable(damage,S::Jump)
                    && damage.hull/damage.hull_max.max(1.0)>=0.2 {
                    // Repair escape capability while opening range using received positions.
                    let away=view.contacts.iter().filter_map(|c|c.track.as_ref()).min_by(|a,c|
                        (a.pos-b.pos).length().total_cmp(&(c.pos-b.pos).length())).map_or(crate::kinematics::Vec2::new(1.0,0.0),|tr|(b.pos-tr.pos).normalized());
                    out.push(Command::MoveTo {body:b.id,point:b.pos+away*AU});
                    if b.thermal.heat_fraction()>1.0 && !b.thermal.dumping {out.push(Command::SetHeatDump {body:b.id,enabled:true});}
                    else if b.thermal.dumping && b.thermal.heat_fraction()<0.25 {out.push(Command::SetHeatDump {body:b.id,enabled:false});}
                } else {out.push(Command::Surrender {body:b.id});}
                continue;
            }
            let goal=if b.damage.operating_effectiveness(S::Propulsion)==0.0 {RepairGoal::Automatic} else {RepairGoal::Fight};
            if damage.repair_goal!=goal {out.push(Command::SetRepairGoal {body:b.id,goal});}
            if b.thermal.heat_fraction()>1.0 && !b.thermal.dumping {out.push(Command::SetHeatDump {body:b.id,enabled:true});}
            else if b.thermal.dumping && b.thermal.heat_fraction()<0.25 {out.push(Command::SetHeatDump {body:b.id,enabled:false});}
            let (target,engage)=choose_target(view,b);
            let objective=view.objective.as_ref().filter(|o|o.attacker==b.faction);
            let site=objective.and_then(|o|o.sensor_site.as_ref());
            let search_destination=objective.map(|o|o.center);
            let detour=site.filter(|_|target.is_none()).and_then(|site|station_detour(b,site,
                search_destination.unwrap_or(site.pos)));
            if site.is_some() {
                use crate::world::controls::{ControlledSystem,Mode};
                for (system,current,mode) in [
                    (ControlledSystem::Screens,b.controls.screens,if detour.is_some() {Mode::Off} else {Mode::Auto}),
                    (ControlledSystem::Ecm,b.controls.ecm,if detour.is_some() {Mode::Off} else {Mode::Auto}),
                    (ControlledSystem::Active,b.controls.active,Mode::Off),
                ] {if current!=mode {out.push(Command::SetSystemMode {body:b.id,system,mode});}}
                let drive=if detour.is_some() {20.0} else {SHIP_MAX_ACCEL_G.value};
                if (b.drive_limit/G0-drive).abs()>0.01 {out.push(Command::SetDriveLimit {body:b.id,g:drive});}
            }
            if let Some(point)=detour {
                out.push(Command::MoveTo {body:b.id,point});
                if b.beam_auto || b.beam_target.is_some() {out.push(Command::EngageBeam {body:b.id,target:None});}
                if b.missile_queued.iter().any(|n|*n>0) {out.push(Command::CancelLaunches {body:b.id});}
                continue;
            }
            if target.is_none() && let Some(point)=search_destination {
                out.push(Command::MoveTo {body:b.id,point});
                continue;
            }
            if target.is_none() && view.time >= *self.ping_at.get(&b.id).unwrap_or(&0.0) {
                out.push(Command::Ping { body: b.id });
                self.ping_at.insert(b.id,view.time+BOT_PING_S.value);
            }
            if target.is_none() && b.probes>0 && view.time>=*self.probe_at.get(&b.id).unwrap_or(&0.0)
                && let Some(bearing)=view.contacts.iter().filter(|c|!c.stale).flat_map(|c|&c.bearings).max_by(|a,b|a.emitted_at.total_cmp(&b.emitted_at)) {
                out.push(Command::DeployProbe {body:b.id,direction:crate::kinematics::Vec2::new(bearing.bearing.cos(),bearing.bearing.sin())});
                self.probe_at.insert(b.id,view.time+PROBE_PING_INTERVAL_S.value);
            }
            let Some(c) = target else { continue };
            if self.targets.insert(b.id,c.id).is_some_and(|previous|previous!=c.id) {
                if b.missile_queued.iter().any(|n|*n>0) {out.push(Command::CancelLaunches {body:b.id});}
                self.salvo_at.remove(&b.id);
            }
            let tr = c.track.as_ref().unwrap();
            let range = (tr.pos-b.pos).length();
            if engage && range<2.0*AU && c.active_fire_control<0.5
                && view.time>=*self.ping_at.get(&b.id).unwrap_or(&0.0) {
                out.push(Command::Ping {body:b.id});
                self.ping_at.insert(b.id,view.time+BOT_PING_S.value);
            }
            if engage {
                let payload=if b.magazine[Payload::Kinetic.index()]>0 && crate::world::endgame::recoverable(damage,S::SrmLauncher) {Payload::Kinetic}
                    else if b.magazine[Payload::Nuclear.index()]>0 && crate::world::endgame::recoverable(damage,S::Launcher) {Payload::Nuclear} else {Payload::Beam};
                out.push(Command::KeepRange {body:b.id,target:InterceptTarget::Contact(c.id),
                    range:crate::autopilot::weapon_standoff(payload)});
            } else {out.push(Command::Flyby { body: b.id, target: InterceptTarget::Contact(c.id) });}
            // Let beam fire control judge useful long-range shots from the
            // received solution; do not force wasteful directed fire at 1 AU.
            if !b.beam_auto && b.ship_class!=Some(crate::world::ShipClass::Picket) {out.push(Command::ArmBeams {body:b.id});}
            // Screen policy belongs to the platform's On/Off/Auto controller.
            if range <= Payload::Nuclear.engagement_range() && view.time >= *self.salvo_at.get(&b.id).unwrap_or(&0.0) {
                let close=range<0.1*AU;
                let conserve=escort_known && view.objective.as_ref().is_some_and(|o|o.attacker==b.faction)
                    && !close;
                let reserve=if close {0} else {4};
                for payload in [Payload::Nuclear, Payload::Kinetic] {
                    if range>payload.engagement_range() {continue;}
                    let eta=crate::world::weapon_probability::flight_seconds(payload,range,0.0);
                    let sigma=(tr.cov[0][0]+tr.cov[1][1]+(tr.velocity_sigma*eta).powi(2)).max(0.0).sqrt();
                    let confidence=crate::world::weapon_probability::hit_chance(payload,range,
                        crate::world::weapon_probability::quality(c.detection),sigma,
                        crate::world::weapon_probability::evasion_score(tr.accel,tr.vel-b.vel,tr.pos-b.pos),1.0);
                    if confidence<0.5 || c.detection<crate::sensors::DetectionLevel::Resolved {
                        if range<crate::units::AU && view.time>=*self.ping_at.get(&b.id).unwrap_or(&0.0) {
                            out.push(Command::Ping {body:b.id});
                            self.ping_at.insert(b.id,view.time+BOT_PING_S.value);
                        }
                        continue;
                    }
                    let available=b.magazine[payload.index()].saturating_sub(b.missile_queued[payload.index()]);
                    if available > reserve {
                        for _ in 0..if close && confidence>=0.8 {available} else {1} {
                            out.push(Command::Launch { body: b.id, target: c.id, payload });
                        }
                        if !close {break;}
                    }
                }
                self.salvo_at.insert(b.id, view.time+if close {1.0} else {BOT_SALVO_S.value*if conserve {3.0} else {1.0}});
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use crate::units::LIGHT_SECOND;
    use super::*;
    use crate::session::{LocalSession,Role,ContactView,TrackView};
    use crate::mind::{ContactId,Source};
    use crate::kinematics::Vec2;

    fn encounter_view()->View {
        let session=LocalSession::new(crate::scenario::transport_intercept());
        let mut view=session.view(Role::Faction(crate::scenario::RAIDER));
        view.objective.as_mut().unwrap().sensor_site=None;
        view.objective.as_mut().unwrap().center=Vec2::new(10.0*AU,0.0);
        let ship=view.bodies.iter_mut().find(|b|b.controllable).unwrap();
        ship.pos=Vec2::ZERO;ship.vel=Vec2::ZERO;
        view.contacts=(1..=2).map(|id|ContactView {identified_name:None,display_class:None,detection:crate::sensors::DetectionLevel::Resolved,
            ping_remaining:0.0,active_fire_control:0.0,reporting_sensor:None,
            resolved_class:Some(if id==1 {ShipClass::Transport} else {ShipClass::Frigate}),
            resolved_interceptor:false,damage:None,id:ContactId(id),resolved_kind:Some(BodyKind::Ship),
            resolved_missile:false,quality:"resolved",stale:false,
            track:Some(TrackView {velocity_sigma:0.0,pos:Vec2::new(if id==1 {4.0*AU} else {2.0*AU},0.0),
                vel:Vec2::ZERO,accel:Vec2::ZERO,cov:[[0.0;2];2],updated_at:0.0,updates:4}),
            bearings:vec![],last_emitted_at:0.0,last_received_at:0.0,last_source:Source::Echo,
            last_snr:1e6,last_range:None}).collect();
        view
    }

    #[test]
    fn raider_retargets_blocking_escort_then_returns_to_clear_transport_route() {
        let mut view=encounter_view();
        let mut ai=Doctrine::default();
        assert!(ai.orders(&view).iter().any(|c|matches!(c,Command::KeepRange {target:InterceptTarget::Contact(ContactId(2)),..})));
        view.contacts[1].track.as_mut().unwrap().pos=Vec2::new(0.0,4.0*AU);
        assert!(ai.orders(&view).iter().any(|c|matches!(c,Command::Flyby {target:InterceptTarget::Contact(ContactId(1)),..})));
        view.contacts[1].track.as_mut().unwrap().pos=Vec2::new(2.0*AU,0.0);
        assert!(ai.orders(&view).iter().any(|c|matches!(c,Command::KeepRange {target:InterceptTarget::Contact(ContactId(2)),..})));
        view.contacts[1].stale=true;
        assert!(ai.orders(&view).iter().any(|c|matches!(c,Command::Flyby {target:InterceptTarget::Contact(ContactId(1)),..})));
    }

    #[test]
    fn crossing_escort_motion_blocks_an_otherwise_clear_route() {
        let mut view=encounter_view();
        let eta=(2.0*4.0*AU/(SHIP_MAX_ACCEL_G.value*G0)).sqrt();
        let escort=view.contacts[1].track.as_mut().unwrap();
        escort.pos=Vec2::new(AU,4.0*AU);
        escort.vel=Vec2::new(0.0,-8.0*AU/eta);
        let ship=view.bodies.iter().find(|b|b.controllable).unwrap();
        assert_eq!(choose_target(&view,ship).0.unwrap().id,ContactId(2));
    }

    #[test]
    fn known_active_station_changes_approach_but_does_not_prevent_self_defence() {
        let mut view=encounter_view();
        view.contacts[1].track.as_mut().unwrap().pos=Vec2::new(0.0,4.0*AU);
        view.objective.as_mut().unwrap().sensor_site=Some(crate::world::SensorSite {
            pos:Vec2::new(3.0*AU,0.0),sensors:crate::sensors::SensorSuite::FULL});
        let mut ai=Doctrine::default();
        let cautious=ai.orders(&view);
        assert!(cautious.iter().any(|c|matches!(c,Command::Flyby {..})));
        assert!(!cautious.iter().any(|c|matches!(c,Command::Ping {..}|Command::Launch {..})));
        view.contacts[1].track.as_mut().unwrap().pos=Vec2::new(0.1*AU,0.0);
        assert!(ai.orders(&view).iter().any(|c|matches!(c,Command::KeepRange {target:InterceptTarget::Contact(ContactId(2)),..})));
        let ship=view.bodies.iter().find(|b|b.controllable).unwrap();
        let mut site=view.objective.as_ref().unwrap().sensor_site.clone().unwrap();
        site.sensors.passive=false;site.sensors.active=false;
        assert!(station_detour(ship,&site,site.pos).is_none());
        site.sensors.active=true;
        assert!(station_detour(ship,&site,site.pos).is_some());
    }

    #[test]
    fn no_track_raider_heads_for_the_known_exit() {
        let mut view=encounter_view();
        view.contacts.clear();
        let exit=view.objective.as_ref().unwrap().center;
        let orders=Doctrine::default().orders(&view);
        assert!(orders.iter().any(|c|matches!(c,Command::MoveTo {point,..} if *point==exit)));
        assert!(!orders.iter().any(|c|matches!(c,Command::Ping {..})));
    }

    #[test]
    fn known_escort_reduces_salvos_and_preserves_close_range_reserve() {
        let session=LocalSession::new(crate::scenario::transport_intercept());
        let mut view=session.view(Role::Faction(crate::scenario::RAIDER));
        view.objective.as_mut().unwrap().sensor_site=None;
        let ship=view.bodies.iter().find(|b|b.controllable).unwrap().clone();
        view.contacts=(1..=2).map(|id|ContactView {identified_name:None,display_class:None,detection:crate::sensors::DetectionLevel::Resolved,ping_remaining:0.0,active_fire_control:0.0,reporting_sensor:None,resolved_class:None,
            resolved_interceptor:false,
            damage:None,
            id:ContactId(id),resolved_kind:Some(BodyKind::Ship),resolved_missile:false,
            quality:"position resolution",stale:false,
            track:Some(TrackView {velocity_sigma:1.0,pos:ship.pos+Vec2::new(0.1*AU,id as f64),vel:Vec2::ZERO,accel:Vec2::ZERO,
                cov:[[1.0,0.0],[0.0,1.0]],updated_at:0.0,updates:4}),
            bearings:vec![],last_emitted_at:0.0,last_received_at:0.0,last_source:Source::Echo,last_snr:1e6,last_range:Some(0.1*AU),
        }).collect();
        let launches=|orders:Vec<Command>|orders.into_iter().filter(|c|matches!(c,Command::Launch {..})).count();
        let mut ai=Doctrine::default();
        assert_eq!(launches(ai.orders(&view)),1);
        view.time=BOT_SALVO_S.value;
        assert_eq!(launches(ai.orders(&view)),0,"longer pause between salvos");
        view.time=3.0*BOT_SALVO_S.value;
        view.bodies.iter_mut().find(|b|b.id==ship.id).unwrap().magazine=[4; 2];
        assert_eq!(launches(ai.orders(&view)),0,"four rounds of each type held back");
        view.time=6.0*BOT_SALVO_S.value;
        for c in &mut view.contacts {c.track.as_mut().unwrap().pos=ship.pos+Vec2::new(4.0*LIGHT_SECOND,0.0);}
        assert_eq!(launches(ai.orders(&view)),8,"reserve committed for close combat");
        view.time+=10.0;
        view.bodies.iter_mut().find(|b|b.id==ship.id).unwrap().missile_queued=[4; 2];
        assert_eq!(launches(ai.orders(&view)),0,"already queued rounds are not queued twice");
        view.time+=10.0;
        for c in &mut view.contacts {c.track.as_mut().unwrap().pos=ship.pos+Vec2::new(3.0*AU,0.0);}
        assert_eq!(launches(ai.orders(&view)),0,"no routine long-range expenditure");
    }
}

#[cfg(test)]
mod survival_tests {
    use super::*;
    use crate::damage::{Condition,RepairGoal,System};
    use crate::session::{LocalSession,Role};
    fn view(class:ShipClass)->View {LocalSession::new(crate::scenario::transport_intercept_class(42,class)).view(Role::Faction(crate::scenario::RAIDER))}
    #[test]
    fn incapable_ships_escape_if_possible_or_surrender_instead_of_chasing() {
        for class in [ShipClass::Frigate,ShipClass::Destroyer] {
            let mut v=view(class);let b=v.bodies.iter_mut().find(|b|b.armed && b.controllable).unwrap();let id=b.id;
            b.magazine=[0,0];b.damage.damage.systems[System::Beam as usize]=Condition::Destroyed;
            let orders=Doctrine::default().orders(&v);
            assert!(orders.iter().any(|o|if class.has_jump_drive() {matches!(o,Command::Withdraw {body} if *body==id)} else {matches!(o,Command::Surrender {body} if *body==id)}));
            assert!(!orders.iter().any(|o|matches!(o,Command::KeepRange {..}|Command::Launch {..})));
        }
    }
    #[test]
    fn damaged_escape_system_is_repaired_and_irrecoverable_escape_surrenders() {
        let mut v=view(ShipClass::Destroyer);let b=v.bodies.iter_mut().find(|b|b.armed && b.controllable).unwrap();
        b.damage.damage.systems[System::Power as usize]=Condition::Damaged;
        let orders=Doctrine::default().orders(&v);
        assert!(orders.iter().any(|o|matches!(o,Command::SetRepairGoal {goal:RepairGoal::Escape,..})));
        assert!(orders.iter().any(|o|matches!(o,Command::MoveTo {..})));
        assert!(!orders.iter().any(|o|matches!(o,Command::Withdraw {..}|Command::Surrender {..})));
        v.bodies.iter_mut().find(|b|b.armed && b.controllable).unwrap().damage.damage.systems[System::Repair as usize]=Condition::Destroyed;
        assert!(Doctrine::default().orders(&v).iter().any(|o|matches!(o,Command::Surrender {..})));
    }
    #[test]
    fn healthy_ships_still_fight_and_spooling_ships_receive_no_new_helm_orders() {
        let mut v=view(ShipClass::Destroyer);
        assert!(!Doctrine::default().orders(&v).iter().any(|o|matches!(o,Command::Withdraw {..}|Command::Surrender {..})));
        let b=v.bodies.iter_mut().find(|b|b.armed && b.controllable).unwrap();
        b.jump=Some(crate::world::jump::JumpState::Spooling {destination:crate::kinematics::Vec2::ZERO,depart_at:600.0});
        assert!(Doctrine::default().orders(&v).is_empty());
    }
}
