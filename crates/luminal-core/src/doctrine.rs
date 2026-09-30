//! Deterministic doctrine consuming exactly the player's restricted view.
//! No world, target identity, or spectator access is available here.
use crate::session::{BodyView, ContactView, Command, InterceptTarget, Payload, View};
use crate::params::*;
use crate::units::{AU,G0};
use crate::world::{BodyKind,ShipClass,Stance};
use std::collections::BTreeMap;

/// Defenders leave the prize only once a tracked ship is this close to it.
const SCREEN_LEASH: f64 = 1.5 * AU;
/// Matched drives cannot be run down, so the quarry fights inside this range.
const EVADE_CATCH: f64 = 0.1 * AU;

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
    let attacker=view.objective.as_ref().is_some_and(|o|o.attacker==ship.faction);
    let hunt_prize=attacker && view.objective.as_ref().is_some_and(|o|o.prize.is_some());
    let mut contacts:Vec<_>=view.contacts.iter().filter(|c|!c.stale && c.track.is_some()
        && !c.resolved_missile && !matches!(c.resolved_kind,Some(BodyKind::Probe))
        && (hunt_prize || !matches!(c.resolved_kind,Some(BodyKind::Station)))).collect();
    // The station is the raid. Shoot it once no ship is still inside the screen leash.
    if hunt_prize {
        if let Some(station)=contacts.iter().copied().find(|c|c.resolved_kind==Some(BodyKind::Station)) {
            let at=station.track.as_ref().unwrap().pos;
            let guarded=contacts.iter().copied().any(|c|c.resolved_kind==Some(BodyKind::Ship)
                && (c.track.as_ref().unwrap().pos-at).length()<=SCREEN_LEASH);
            if !guarded {return (Some(station),true);}
        }
        contacts.retain(|c|c.resolved_kind!=Some(BodyKind::Station));
    }
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
            // Propulsion destroyed and nothing left inside beam range. A damaged hull still fights.
            let stranded=!crate::world::endgame::recoverable(damage,S::Propulsion)
                && !view.contacts.iter().filter(|c|!c.resolved_missile).any(|c|c.track.as_ref().is_some_and(|tr|
                    (tr.pos-b.pos).length()<=SHIP_BEAM_AUTO_RANGE_LS.value*crate::units::LIGHT_SECOND));
            if !can_fight || stranded {
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
            let stance=view.objective.as_ref().map(|o|o.stance).unwrap_or(Stance::Intercept);
            let screening=stance==Stance::Screen && view.objective.as_ref().is_some_and(|o|o.defender==b.faction);
            let running=stance==Stance::Evade && objective.is_some();
            if screening {
                let obj=view.objective.as_ref().unwrap();
                let threatened=target.is_some_and(|c|(c.track.as_ref().unwrap().pos-obj.center).length()<=SCREEN_LEASH);
                if !threatened {
                    if let Some(prize)=obj.prize {
                        let following=matches!(b.autopilot.map(|ap|ap.order),Some(crate::world::Order::Follow {target,..}) if target==prize);
                        if !following {
                            out.push(Command::Follow {body:b.id,target:prize,offset:None});
                            out.push(Command::SetDriveLimit {body:b.id,g:class.max_g()});
                        }
                    }
                    continue;
                }
            }
            if target.is_none() && let Some(point)=search_destination {
                let toward=point-b.pos;
                let distance=toward.length();
                let direction=if distance>1.0 {toward.normalized()} else {crate::kinematics::Vec2::new(1.0,0.0)};
                let closing=b.vel.dot(direction);
                let aligned=b.vel.length()>1.0 && distance>1.0 && b.vel.normalized().dot(direction)>0.7;
                let time_left=objective.and_then(|o|o.escape_by).map(|deadline|deadline-view.time);
                // Coast only once this vector reaches the ground before the clock. A few km/s never does.
                let arrives=match time_left {
                    Some(left)=>left>0.0 && closing>1.0 && distance/closing<=left,
                    None=>aligned,
                };
                let coast=running && aligned && arrives;
                if coast {
                    if b.autopilot.is_some() || b.thrust.length()>1e-6 {out.push(Command::Coast {body:b.id});}
                } else if running {
                    let thrust=direction*class.max_g()*G0;
                    let burning=b.autopilot.is_none() && (b.thrust-thrust).length()<thrust.length()*0.05;
                    if !burning {out.push(Command::SetThrust {body:b.id,thrust});}
                } else {out.push(Command::MoveTo {body:b.id,point});}
                continue;
            }
            if running && target.is_none() {continue;}
            if target.is_none() && view.time >= *self.ping_at.get(&b.id).unwrap_or(&0.0) {
                out.push(Command::Ping { body: b.id });
                self.ping_at.insert(b.id,view.time+BOT_PING_S.value);
            }
            if target.is_none() && b.probes>0 && view.time>=*self.probe_at.get(&b.id).unwrap_or(&0.0)
                && let Some((contact,bearing))=view.contacts.iter().filter(|c|!c.stale).flat_map(|c| c.bearings.iter().map(move |bearing|(c,bearing))).max_by(|a,b|a.1.emitted_at.total_cmp(&b.1.emitted_at)) {
                let direction=crate::kinematics::Vec2::new(bearing.bearing.cos(),bearing.bearing.sin());
                let destination=contact.track.as_ref().map(|track|track.pos).unwrap_or_else(|| b.pos+direction*bearing.max_range.max(1_000.0));
                out.push(Command::DeployProbe {body:b.id,direction,destination});
                self.probe_at.insert(b.id,view.time+PROBE_PING_INTERVAL_S.value);
            }
            let Some(c) = target else { continue };
            if self.targets.insert(b.id,c.id).is_some_and(|previous|previous!=c.id) {
                if b.missile_queued.iter().any(|n|*n>0) {out.push(Command::CancelLaunches {body:b.id});}
                self.salvo_at.remove(&b.id);
            }
            let tr = c.track.as_ref().unwrap();
            let range = (tr.pos-b.pos).length();
            if class.has_projector() {
                use crate::world::weapon_fit::{BeamMode,support_worthwhile};
                let mode=if support_worthwhile(view,b,c) {BeamMode::Interference} else {BeamMode::Damage};
                if b.beam_mode!=mode {out.push(Command::SetBeamMode {body:b.id,mode});}
            }
            if !running && engage && range<2.0*AU && c.active_fire_control<0.5
                && view.time>=*self.ping_at.get(&b.id).unwrap_or(&0.0) {
                out.push(Command::Ping {body:b.id});
                self.ping_at.insert(b.id,view.time+BOT_PING_S.value);
            }
            if running && range>EVADE_CATCH {
                out.push(Command::Evade {body:b.id,target:InterceptTarget::Contact(c.id)});
            } else if engage || running {
                let payload=if b.magazine[Payload::Kinetic.index()]>0 && crate::world::endgame::recoverable(damage,S::SrmLauncher) {Payload::Kinetic}
                    else if b.magazine[Payload::Nuclear.index()]>0 && crate::world::endgame::recoverable(damage,S::Launcher) {Payload::Nuclear} else {Payload::Beam};
                out.push(Command::KeepRange {body:b.id,target:InterceptTarget::Contact(c.id),
                    range:crate::autopilot::weapon_standoff(payload)});
            } else {out.push(Command::Flyby { body: b.id, target: InterceptTarget::Contact(c.id) });}
            // Inside ten light-seconds a raised field is waited out. The beam fires when that report falls.
            let screen=c.damage.as_ref().map(|report|report.screen_available);
            let in_beam=range<10.0*crate::units::LIGHT_SECOND;
            let screen_up=in_beam && screen.is_some_and(|charge|charge>0.5);
            let screen_down=in_beam && screen.is_some_and(|charge|charge<0.2);
            if b.ship_class!=Some(crate::world::ShipClass::Picket) {
                if screen_up {
                    if b.beam_auto || b.beam_target.is_some() {out.push(Command::EngageBeam {body:b.id,target:None});}
                } else if screen_down {
                    if b.beam_auto || b.beam_target!=Some(c.id) {out.push(Command::EngageBeam {body:b.id,target:Some(c.id)});}
                } else if !b.beam_auto {out.push(Command::ArmBeams {body:b.id});}
            }
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
                        if !running && range<crate::units::AU && view.time>=*self.ping_at.get(&b.id).unwrap_or(&0.0) {
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

    fn resolved_ship(id:u32,pos:Vec2)->ContactView {
        ContactView {identified_name:None,display_class:None,detection:crate::sensors::DetectionLevel::Resolved,
            ping_remaining:0.0,active_fire_control:0.0,reporting_sensor:None,resolved_class:Some(ShipClass::Cruiser),
            resolved_interceptor:false,damage:None,id:ContactId(id),resolved_kind:Some(BodyKind::Ship),
            resolved_missile:false,quality:"resolved",stale:false,
            track:Some(TrackView {velocity_sigma:10.0,pos,vel:Vec2::ZERO,accel:Vec2::ZERO,
                cov:[[1.0e16,0.0],[0.0,1.0e16]],updated_at:0.0,updates:4}),
            bearings:vec![],last_emitted_at:0.0,last_received_at:0.0,last_source:Source::Echo,
            last_snr:1e6,last_range:None}
    }

    fn own_ships(view:&mut View) {
        for b in &mut view.bodies {
            if b.kind==BodyKind::Ship && b.armed {b.controllable=true;}
        }
    }

    #[test]
    fn raid_screen_holds_the_station_until_a_ship_reaches_it() {
        let session=LocalSession::new(crate::scenario::Scenario::Raid.build(42,crate::scenario::home_system()));
        let mut view=session.view(Role::Faction(crate::scenario::RAIDER));
        own_ships(&mut view);
        let station=view.bodies.iter().find(|b|b.kind==BodyKind::Station).unwrap().pos;
        view.contacts=vec![resolved_ship(1,station+Vec2::new(4.0*AU,0.0))];
        let orders=Doctrine::default().orders(&view);
        assert!(orders.iter().all(|c|!matches!(c,Command::KeepRange {..}|Command::Flyby {..}|Command::MoveTo {..}|Command::Evade {..})));
        for ship in view.bodies.iter().filter(|b|b.controllable) {
            let already=matches!(ship.autopilot.map(|ap|ap.order),Some(crate::world::Order::Follow {target:crate::world::BodyId(1),..}));
            let ordered=orders.iter().any(|c|matches!(c,Command::Follow {body,target:crate::world::BodyId(1),..} if *body==ship.id));
            assert!(already||ordered,"{ship:?} left the screen\n{orders:?}");
        }
        view.contacts[0].track.as_mut().unwrap().pos=station+Vec2::new(0.2*AU,0.0);
        let orders=Doctrine::default().orders(&view);
        for ship in view.bodies.iter().filter(|b|b.controllable) {
            assert!(orders.iter().any(|c|matches!(c,Command::Flyby {body,..} if *body==ship.id)),"{:?} did not fight",ship.id);
        }
        assert!(orders.iter().all(|c|!matches!(c,Command::Follow {..}|Command::MoveTo {..})));
    }

    #[test]
    fn quarry_stays_dark_until_the_hunter_is_close() {
        let session=LocalSession::new(crate::scenario::Scenario::HideAndSeek.build(42,crate::scenario::home_system()));
        let mut view=session.view(Role::Faction(crate::scenario::RAIDER));
        let dark=|orders:&[Command]| !orders.iter().any(|c|matches!(c,Command::Ping {..}|Command::SetSystemMode {..}|Command::SetScreen {..}));
        let orders=Doctrine::default().orders(&view);
        assert!(orders.iter().any(|c|matches!(c,Command::SetThrust {thrust,..} if thrust.length()>100.0*G0)),"{orders:?}");
        assert!(dark(&orders));
        assert!(!orders.iter().any(|c|matches!(c,Command::KeepRange {..}|Command::Flyby {..}|Command::Evade {..})));
        let quarry=view.bodies.iter().find(|b|b.controllable).unwrap().pos;
        view.contacts=vec![resolved_ship(1,quarry+Vec2::new(1.5*AU,0.0))];
        let orders=Doctrine::default().orders(&view);
        assert!(orders.iter().any(|c|matches!(c,Command::Evade {..})));
        assert!(dark(&orders));
        assert!(!orders.iter().any(|c|matches!(c,Command::KeepRange {..}|Command::Flyby {..}|Command::MoveTo {..})));
        view.contacts[0].track.as_mut().unwrap().pos=quarry+Vec2::new(0.05*AU,0.0);
        let orders=Doctrine::default().orders(&view);
        assert!(orders.iter().any(|c|matches!(c,Command::KeepRange {..})));
        assert!(dark(&orders));
        assert!(!orders.iter().any(|c|matches!(c,Command::Evade {..}|Command::Ping {..})));
    }

    #[test]
    fn quarry_burns_until_its_speed_can_beat_the_clock() {
        let session=LocalSession::new(crate::scenario::Scenario::HideAndSeek.build(42,crate::scenario::home_system()));
        let mut view=session.view(Role::Faction(crate::scenario::RAIDER));
        let hunt=view.objective.as_ref().unwrap().center;
        let deadline=view.objective.as_ref().unwrap().escape_by.expect("the hunt is on a clock");
        let quarry=view.bodies.iter_mut().find(|b|b.controllable).unwrap();
        let direction=(hunt-quarry.pos).normalized();
        let distance=(hunt-quarry.pos).length();
        quarry.vel=direction*9.0;
        quarry.thrust=direction*120.0*G0;
        let orders=Doctrine::default().orders(&view);
        assert!(orders.iter().any(|c|matches!(c,Command::SetThrust {..})),"{orders:?}");
        assert!(!orders.iter().any(|c|matches!(c,Command::Coast {..}|Command::MoveTo {..})));
        assert!(9.0*deadline<distance,"a 9 km/s coast must miss the hunting ground");
        let quarry=view.bodies.iter_mut().find(|b|b.controllable).unwrap();
        quarry.vel=direction*(distance/(deadline*0.5));
        quarry.thrust=direction*120.0*G0;
        let orders=Doctrine::default().orders(&view);
        assert!(orders.iter().any(|c|matches!(c,Command::Coast {..})));
        assert!(!orders.iter().any(|c|matches!(c,Command::SetThrust {..}|Command::MoveTo {..})));
    }

    #[test]
    fn armada_attackers_close_and_defenders_are_not_sent_home() {
        let session=LocalSession::new(crate::scenario::Scenario::Armada.build(42,crate::scenario::home_system()));
        let center=session.view(Role::Spectator).objective.unwrap().center;
        let mut defenders=session.view(Role::Faction(crate::scenario::RAIDER));
        own_ships(&mut defenders);
        let orders=Doctrine::default().orders(&defenders);
        assert!(orders.iter().all(|c|!matches!(c,Command::MoveTo {point,..} if (*point-center).length()<AU)),"{orders:?}");
        let mut attackers=session.view(Role::Faction(crate::scenario::ESCORT));
        attackers.contacts.clear();
        let player=attackers.objective.as_ref().unwrap().player;
        for b in &mut attackers.bodies {b.controllable=b.kind==BodyKind::Ship && b.armed && Some(b.id)!=player;}
        let orders=Doctrine::default().orders(&attackers);
        let closing=orders.iter().filter(|c|matches!(c,Command::MoveTo {point,..} if (*point-center).length()<1.0)).count();
        assert_eq!(closing,9,"allies without a track head for the enemy fleet");
    }

    #[test]
    fn raid_attacker_fires_on_an_unguarded_station() {
        let session=LocalSession::new(crate::scenario::Scenario::Raid.build(42,crate::scenario::home_system()));
        let mut view=session.view(Role::Faction(crate::scenario::ESCORT));
        let cruiser=view.bodies.iter().find(|b|b.controllable).unwrap().id;
        let origin=view.bodies.iter().find(|b|b.id==cruiser).unwrap().pos;
        let station_at=origin+Vec2::new(2.0*AU,0.0);
        let mut station=resolved_ship(1,station_at);
        station.resolved_kind=Some(BodyKind::Station);
        station.resolved_class=None;
        let mut probe=resolved_ship(3,origin+Vec2::new(LIGHT_SECOND,0.0));
        probe.resolved_kind=Some(BodyKind::Probe);
        probe.resolved_class=None;
        view.contacts=vec![station.clone(),probe.clone()];
        let orders=Doctrine::default().orders(&view);
        let aimed=|orders:&[Command],id:u32| orders.iter().any(|c|matches!(c,
            Command::KeepRange {body,target:InterceptTarget::Contact(ContactId(n)),..}
            | Command::Flyby {body,target:InterceptTarget::Contact(ContactId(n)),..}
            if *body==cruiser && *n==id));
        assert!(aimed(&orders,1),"{orders:?}");
        assert!(!aimed(&orders,3),"{orders:?}");
        assert!(orders.iter().any(|c|matches!(c,Command::KeepRange {target:InterceptTarget::Contact(ContactId(1)),..})));
        assert!(!orders.iter().any(|c|matches!(c,Command::Flyby {target:InterceptTarget::Contact(ContactId(1)),..})));
        view.contacts=vec![station,resolved_ship(2,station_at+Vec2::new(0.2*AU,0.0)),probe];
        let orders=Doctrine::default().orders(&view);
        assert!(aimed(&orders,2),"{orders:?}");
        assert!(!aimed(&orders,1) && !aimed(&orders,3),"{orders:?}");

        let mut defenders=session.view(Role::Faction(crate::scenario::RAIDER));
        own_ships(&mut defenders);
        let mut station=resolved_ship(9,defenders.objective.as_ref().unwrap().center);
        station.resolved_kind=Some(BodyKind::Station);
        station.resolved_class=None;
        defenders.contacts=vec![station];
        let orders=Doctrine::default().orders(&defenders);
        assert!(orders.iter().all(|c|!matches!(c,Command::KeepRange {..}|Command::Flyby {..}|Command::Evade {..})));
        for ship in defenders.bodies.iter().filter(|b|b.controllable) {
            let already=matches!(ship.autopilot.map(|ap|ap.order),Some(crate::world::Order::Follow {target:crate::world::BodyId(1),..}));
            let ordered=orders.iter().any(|c|matches!(c,Command::Follow {body,target:crate::world::BodyId(1),..} if *body==ship.id));
            assert!(already||ordered,"{ship:?} left the screen\n{orders:?}");
        }
    }

    #[test]
    fn beam_holds_until_the_received_screen_drops() {
        use crate::damage::{Damage,Report,System};
        let mut view=LocalSession::new(crate::scenario::transport_intercept_class(42,ShipClass::Destroyer)).view(Role::Faction(crate::scenario::RAIDER));
        view.objective.as_mut().unwrap().sensor_site=None;
        let ship=view.bodies.iter().find(|b|b.controllable).unwrap().id;
        let origin=view.bodies.iter().find(|b|b.id==ship).unwrap().pos;
        let mut contact=resolved_ship(7,origin+Vec2::new(4.0*LIGHT_SECOND,0.0));
        contact.damage=Some(Report {damage:Damage::default(),installed:[true;System::COUNT],observed_at:0.0,screen_available:1.0});
        view.contacts=vec![contact];
        let body=view.bodies.iter_mut().find(|b|b.id==ship).unwrap();
        body.beam_auto=true;
        body.beam_target=None;
        #[derive(Clone,Copy,Debug,PartialEq)]
        enum Beam {Hold,Fire(ContactId),Arm}
        let beam=|orders:&[Command]| -> Vec<Beam> {
            orders.iter().filter_map(|c|match c {
                Command::EngageBeam {body,target:None} if *body==ship => Some(Beam::Hold),
                Command::EngageBeam {body,target:Some(id)} if *body==ship => Some(Beam::Fire(*id)),
                Command::ArmBeams {body} if *body==ship => Some(Beam::Arm),
                _=>None,
            }).collect()
        };
        let orders=Doctrine::default().orders(&view);
        assert_eq!(beam(&orders),vec![Beam::Hold],"a raised screen is held, not fired into: {orders:?}");
        view.bodies.iter_mut().find(|b|b.id==ship).unwrap().beam_auto=false;
        let orders=Doctrine::default().orders(&view);
        assert!(beam(&orders).is_empty(),"a beam already held is not ordered again: {orders:?}");
        view.contacts[0].damage.as_mut().unwrap().screen_available=0.1;
        let orders=Doctrine::default().orders(&view);
        assert_eq!(beam(&orders),vec![Beam::Fire(ContactId(7))],"{orders:?}");
        view.bodies.iter_mut().find(|b|b.id==ship).unwrap().beam_target=Some(ContactId(7));
        let orders=Doctrine::default().orders(&view);
        assert!(beam(&orders).is_empty(),"the same target is not reissued: {orders:?}");
        let body=view.bodies.iter_mut().find(|b|b.id==ship).unwrap();
        body.beam_auto=false;
        body.beam_target=None;
        view.contacts[0].damage.as_mut().unwrap().screen_available=0.35;
        let orders=Doctrine::default().orders(&view);
        assert_eq!(beam(&orders),vec![Beam::Arm],"the band between 0.2 and 0.5 still arms: {orders:?}");
        view.contacts[0].track.as_mut().unwrap().pos=origin+Vec2::new(AU,0.0);
        view.contacts[0].damage.as_mut().unwrap().screen_available=1.0;
        let orders=Doctrine::default().orders(&view);
        assert_eq!(beam(&orders),vec![Beam::Arm],"outside ten light-seconds a full screen is not a hold: {orders:?}");
        view.contacts[0].track.as_mut().unwrap().pos=origin+Vec2::new(4.0*LIGHT_SECOND,0.0);
        view.contacts[0].damage=None;
        let orders=Doctrine::default().orders(&view);
        assert_eq!(beam(&orders),vec![Beam::Arm],"an unknown screen still arms: {orders:?}");

        let mut picket=LocalSession::new(crate::scenario::transport_intercept_class(42,ShipClass::Picket)).view(Role::Faction(crate::scenario::RAIDER));
        picket.objective.as_mut().unwrap().sensor_site=None;
        let ship=picket.bodies.iter().find(|b|b.controllable).unwrap().id;
        let origin=picket.bodies.iter().find(|b|b.id==ship).unwrap().pos;
        let mut contact=resolved_ship(7,origin+Vec2::new(4.0*LIGHT_SECOND,0.0));
        contact.damage=Some(Report {damage:Damage::default(),installed:[true;System::COUNT],observed_at:0.0,screen_available:0.1});
        picket.contacts=vec![contact];
        picket.bodies.iter_mut().find(|b|b.id==ship).unwrap().beam_auto=false;
        let orders=Doctrine::default().orders(&picket);
        assert!(orders.iter().all(|c|!matches!(c,Command::EngageBeam {..}|Command::ArmBeams {..})),"{orders:?}");
    }

    #[test]
    fn a_ship_with_a_dead_drive_still_fights_inside_beam_range() {
        use crate::damage::{Condition,System};
        use crate::session::LocalSession;
        let mut close=LocalSession::new(crate::scenario::transport_intercept_class(42,ShipClass::Frigate)).view(Role::Faction(crate::scenario::RAIDER));
        close.objective.as_mut().unwrap().sensor_site=None;
        let id=close.bodies.iter().find(|b|b.armed && b.controllable).unwrap().id;
        let pos=close.bodies.iter().find(|b|b.id==id).unwrap().pos;
        close.contacts=vec![resolved_ship(4,pos+Vec2::new(4.0*LIGHT_SECOND,0.0))];
        close.bodies.iter_mut().find(|b|b.id==id).unwrap().damage.damage.systems[System::Propulsion as usize]=Condition::Destroyed;
        let orders=Doctrine::default().orders(&close);
        assert!(orders.iter().any(|o|matches!(o,Command::KeepRange {..})));
        assert!(!orders.iter().any(|o|matches!(o,Command::Surrender {..}|Command::Withdraw {..}|Command::SetRepairGoal {goal:crate::damage::RepairGoal::Escape,..})));
        close.contacts.clear();
        assert!(Doctrine::default().orders(&close).iter().any(|o|matches!(o,Command::Surrender {..})));
        let mut stranded=LocalSession::new(crate::scenario::transport_intercept_class(42,ShipClass::Destroyer)).view(Role::Faction(crate::scenario::RAIDER));
        stranded.contacts.clear();
        stranded.bodies.iter_mut().find(|b|b.armed && b.controllable).unwrap().damage.damage.systems[System::Propulsion as usize]=Condition::Destroyed;
        let orders=Doctrine::default().orders(&stranded);
        assert!(orders.iter().any(|o|matches!(o,Command::Withdraw {..})));
        assert!(!orders.iter().any(|o|matches!(o,Command::Surrender {..}|Command::KeepRange {..})));
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
        let mut v=view(ShipClass::Destroyer);
        let b=v.bodies.iter_mut().find(|b|b.armed && b.controllable).unwrap();
        b.damage.damage.systems[System::Power as usize]=Condition::Damaged;
        b.damage.damage.hull=0.4*b.damage.damage.hull_max;
        let orders=Doctrine::default().orders(&v);
        assert!(!orders.iter().any(|o|matches!(o,Command::Withdraw {..}|Command::Surrender {..}|Command::SetRepairGoal {goal:RepairGoal::Escape,..})));
        assert!(orders.iter().any(|o|matches!(o,Command::KeepRange {..}|Command::Flyby {..}|Command::MoveTo {..})));
        let mut frigate=view(ShipClass::Frigate);
        let b=frigate.bodies.iter_mut().find(|b|b.armed && b.controllable).unwrap();
        b.damage.damage.hull=0.4*b.damage.damage.hull_max;
        assert!(!Doctrine::default().orders(&frigate).iter().any(|o|matches!(o,Command::Surrender {..}|Command::Withdraw {..})));
        let b=v.bodies.iter_mut().find(|b|b.armed && b.controllable).unwrap();
        b.magazine=[0,0];
        b.damage.damage.systems[System::Beam as usize]=Condition::Destroyed;
        b.damage.damage.systems[System::Repair as usize]=Condition::Destroyed;
        let orders=Doctrine::default().orders(&v);
        assert!(orders.iter().any(|o|matches!(o,Command::Surrender {..})));
        assert!(!orders.iter().any(|o|matches!(o,Command::Withdraw {..}|Command::MoveTo {..}|Command::KeepRange {..})));
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
