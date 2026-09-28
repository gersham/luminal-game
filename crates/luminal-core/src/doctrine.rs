//! Deterministic doctrine consuming exactly the player's restricted view.
//! No world, target identity, or spectator access is available here.
use crate::session::{Command, InterceptTarget, Payload, View};
use crate::params::*;
use crate::units::AU;
use crate::world::BodyKind;
use std::collections::BTreeMap;

#[derive(Default)]
pub struct Doctrine {
    targets: BTreeMap<crate::world::BodyId, crate::mind::ContactId>,
    probe_at: BTreeMap<crate::world::BodyId, f64>,
    ping_at: BTreeMap<crate::world::BodyId, f64>,
    salvo_at: BTreeMap<crate::world::BodyId, f64>,
}

impl Doctrine {
    pub fn orders(&mut self, view: &View) -> Vec<Command> {
        let mut out = vec![];
        if view.outcome.is_some() { return out; }
        // A second ranged ship contact implies an escort threat. Do not use
        // hidden ship names/loadouts to decide whether a frigate is present.
        let escort_known=view.contacts.iter().filter(|c|c.resolved_kind==Some(BodyKind::Ship)).count()>=2;
        for b in view.bodies.iter().filter(|b| b.kind == BodyKind::Ship && b.controllable && b.armed) {
            let target = self.targets.get(&b.id).and_then(|id| view.contacts.iter().find(|c| c.id==*id && !c.stale && c.track.is_some() && !c.resolved_missile))
                .or_else(|| view.contacts.iter().filter(|c| !c.stale && c.track.is_some() && !c.resolved_missile).min_by(|a,c| {
                let score = |contact: &crate::session::ContactView| {
                    let p = contact.track.as_ref().unwrap().pos;
                    let objective = view.objective.as_ref();
                    // Attack tracks nearest the escape objective; escorts screen their transport.
                    if objective.is_some_and(|o| b.faction == o.attacker) {
                        (p-objective.unwrap().center).length()
                    } else { (p-b.pos).length() }
                };
                score(a).total_cmp(&score(c))
            }));
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
            self.targets.insert(b.id,c.id);
            let tr = c.track.as_ref().unwrap();
            let range = (tr.pos-b.pos).length();
            out.push(Command::Flyby { body: b.id, target: InterceptTarget::Contact(c.id) });
            // Let beam fire control judge useful long-range shots from the
            // received solution; do not force wasteful directed fire at 1 AU.
            if !b.beam_auto {out.push(Command::ArmBeams {body:b.id});}
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
                        crate::world::weapon_probability::quality(c.detection),sigma,0.0,1.0);
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

    #[test]
    fn known_escort_reduces_salvos_and_preserves_close_range_reserve() {
        let session=LocalSession::new(crate::scenario::transport_intercept());
        let mut view=session.view(Role::Faction(crate::scenario::RAIDER));
        let ship=view.bodies.iter().find(|b|b.controllable).unwrap().clone();
        view.contacts=(1..=2).map(|id|ContactView {detection:crate::sensors::DetectionLevel::Resolved,ping_remaining:0.0,reporting_sensor:None,resolved_class:None,
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
