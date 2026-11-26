//! After the battle: what really happened. Child of world.
//!
//! A debrief is truth, so the session releases it to a faction only once the game is
//! decided. Nothing here feeds back into the simulation: the only state it adds is a
//! write-only record of when each side first learned of each enemy platform.
use super::*;
use std::collections::BTreeSet;

/// One platform's true path, sampled on the game clock from T+0 to its end.
#[derive(Clone, Debug)]
pub struct DebriefPath {
    pub body: BodyId,
    pub name: String,
    pub faction: FactionId,
    pub kind: BodyKind,
    pub ship_class: Option<ShipClass>,
    pub decoy: bool,
    pub interceptor: bool,
    /// (game time, true position). A jump shows as a gap no ship could cross.
    pub samples: Vec<(f64, Vec2)>,
    /// When and how it left the board, if it did.
    pub end: Option<(f64, LossCause)>,
}

impl DebriefPath {
    /// True position at `t`, interpolated between samples; `None` before it existed or after it ended.
    pub fn at(&self, t: f64) -> Option<Vec2> {
        let first = self.samples.first()?;
        let last = self.samples.last()?;
        if t < first.0 || t > last.0 { return None; }
        let i = self.samples.partition_point(|(st, _)| *st <= t);
        if i == 0 { return Some(first.1); }
        if i >= self.samples.len() { return Some(last.1); }
        let (t0, p0) = self.samples[i - 1];
        let (t1, p1) = self.samples[i];
        let u = if t1 > t0 { (t - t0) / (t1 - t0) } else { 0.0 };
        Some(p0 + (p1 - p0) * u)
    }
}

/// One side's battle, counted from the true record.
#[derive(Clone, Debug)]
pub struct SideStats {
    pub faction: FactionId,
    pub ships: u32,
    pub ships_lost: u32,
    pub missiles_launched: u32,
    pub missile_hits: u32,
    pub interceptors_launched: u32,
    pub missiles_stopped: u32,
    pub beam_pulses: u32,
    pub decoys_released: u32,
    pub kills: u32,
    /// When this side's command first received any report of an enemy ship.
    pub first_sighting: Option<f64>,
    /// When this side first held a resolved position on an enemy ship.
    pub first_fix: Option<f64>,
}

/// A notable instant for the replay timeline.
#[derive(Clone, Debug)]
pub struct DebriefMoment {
    pub t: f64,
    pub pos: Vec2,
    pub kind: CombatKind,
    pub faction: Option<FactionId>,
    pub ship: bool,
}

#[derive(Clone, Debug)]
pub struct Debrief {
    pub end: f64,
    pub paths: Vec<DebriefPath>,
    pub sides: Vec<SideStats>,
    pub moments: Vec<DebriefMoment>,
    /// The asking faction's tracks and the platforms they really were.
    pub identities: Vec<(ContactId, BodyId)>,
}

/// Samples per path: platforms in detail, rounds coarsely (Armada fires hundreds).
const PLATFORM_SAMPLES: usize = 900;
const ROUND_SAMPLES: usize = 40;

impl World {
    /// Bookkeeping only: the first time `faction`'s command received word of `contact`.
    pub(super) fn note_sighting(&mut self, faction: FactionId, contact: ContactId, received: f64, level: crate::sensors::DetectionLevel) {
        let Some(&body) = self.reverse_association.get(&(faction, contact)) else { return };
        self.sightings.entry((faction, body)).or_insert(received);
        if level >= crate::sensors::DetectionLevel::Resolved {
            self.fixes.entry((faction, body)).or_insert(received);
        }
    }

    /// The true account of the game so far. `viewer` selects whose tracks are identified.
    pub fn debrief(&self, viewer: Option<FactionId>) -> Debrief {
        let end = self.time;
        let mut paths = Vec::new();
        for (i, b) in self.bodies.iter().enumerate() {
            let id = BodyId(i as u32);
            let from = b.trajectory.start().max(0.0);
            let to = b.trajectory.end().unwrap_or(end).min(end);
            if to < from { continue; }
            let round = b.kind == BodyKind::Missile;
            let n = if round { ROUND_SAMPLES } else { PLATFORM_SAMPLES };
            let samples: Vec<(f64, Vec2)> = (0..=n).filter_map(|k| {
                let t = from + (to - from) * k as f64 / n as f64;
                b.trajectory.state_at(t).map(|s| (t, s.pos))
            }).collect();
            if samples.is_empty() { continue; }
            let end = self.losses.iter().find(|l| l.body == id).map(|l| (l.t, l.cause));
            paths.push(DebriefPath { body: id, name: b.name.clone(), faction: b.faction, kind: b.kind, ship_class: b.ship_class,
                decoy: b.decoy, interceptor: b.interceptor.is_some(), samples, end });
        }
        let factions: BTreeSet<FactionId> = self.perceptions.keys().copied().collect();
        let faction_of = |id: BodyId| self.bodies.get(id.0 as usize).map(|b| b.faction);
        let is_ship = |id: BodyId| self.bodies.get(id.0 as usize).is_some_and(|b| b.kind == BodyKind::Ship && !b.decoy);
        let sides = factions.iter().map(|&f| {
            let mut s = SideStats { faction: f, ships: 0, ships_lost: 0, missiles_launched: 0, missile_hits: 0, interceptors_launched: 0,
                missiles_stopped: 0, beam_pulses: 0, decoys_released: 0, kills: 0, first_sighting: None, first_fix: None };
            for b in self.bodies.iter().filter(|b| b.faction == f) {
                match b.kind {
                    BodyKind::Ship if b.decoy => s.decoys_released += 1,
                    BodyKind::Ship => s.ships += 1,
                    BodyKind::Missile if b.interceptor.is_some() => s.interceptors_launched += 1,
                    BodyKind::Missile => s.missiles_launched += 1,
                    _ => {}
                }
            }
            for l in &self.losses {
                if faction_of(l.body) == Some(f) && is_ship(l.body) && !matches!(l.cause, LossCause::Withdrawn) { s.ships_lost += 1; }
                let killer = match l.cause {
                    LossCause::Missile { missile, .. } | LossCause::Interceptor { missile } => faction_of(missile),
                    LossCause::ShipBeam { shooter } | LossCause::PointDefence { shooter } => faction_of(shooter),
                    _ => None,
                };
                if killer == Some(f) && faction_of(l.body) != Some(f) {
                    if is_ship(l.body) { s.kills += 1; }
                    else if self.bodies[l.body.0 as usize].kind == BodyKind::Missile { s.missiles_stopped += 1; }
                }
            }
            for e in &self.refinement.truth_events {
                let mine = e.own_body.and_then(faction_of) == Some(f);
                match e.kind {
                    CombatKind::MissileHit if mine => s.missile_hits += 1,
                    CombatKind::BeamPulse | CombatKind::SpinalPulse if mine => s.beam_pulses += 1,
                    _ => {}
                }
            }
            let enemy_ship = |(viewer, body): &(FactionId, BodyId)| *viewer == f && is_ship(*body) && faction_of(*body) != Some(f);
            s.first_sighting = self.sightings.iter().filter(|(k, _)| enemy_ship(k)).map(|(_, t)| *t).reduce(f64::min);
            s.first_fix = self.fixes.iter().filter(|(k, _)| enemy_ship(k)).map(|(_, t)| *t).reduce(f64::min);
            s
        }).collect();
        let moments = self.refinement.truth_events.iter().filter(|e| matches!(e.kind,
            CombatKind::Destroyed | CombatKind::NuclearBurst | CombatKind::MissileHit | CombatKind::JumpArrival | CombatKind::Withdrawn | CombatKind::Surrendered))
            .filter(|e| e.kind != CombatKind::Destroyed || e.subject_kind != Some(BodyKind::Missile))
            .filter_map(|e| e.pos.map(|pos| DebriefMoment { t: e.emitted_at, pos, kind: e.kind,
                faction: e.own_body.and_then(faction_of), ship: e.subject_kind == Some(BodyKind::Ship) }))
            .collect();
        let identities = viewer.map(|f| self.reverse_association.iter().filter(|((g, _), _)| *g == f)
            .map(|((_, c), b)| (*c, *b)).collect()).unwrap_or_default();
        Debrief { end, paths, sides, moments, identities }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::{self, Scenario};

    #[test]
    fn a_debrief_traces_every_platform_and_counts_both_sides() {
        let mut w = Scenario::Escort.build(42, scenario::home_system());
        w.advance_to(3.0 * 3600.0);
        let d = w.debrief(Some(FactionId(0)));
        assert_eq!(d.end, w.time());
        let ships: Vec<_> = d.paths.iter().filter(|p| p.kind == BodyKind::Ship && !p.decoy).collect();
        assert!(ships.len() >= 3, "escort, transport and raider");
        for p in &ships {
            assert!(p.samples.first().unwrap().0 >= 0.0, "the replay starts at T+0, not in the pre-history");
            assert!(p.samples.windows(2).all(|w| w[0].0 <= w[1].0));
            let mid = (p.samples.first().unwrap().0 + p.samples.last().unwrap().0) * 0.5;
            let truth = w.bodies[p.body.0 as usize].trajectory.state_at(mid).unwrap().pos;
            assert!((p.at(mid).unwrap() - truth).length() < 0.01 * AU, "interpolation stays on the true path");
        }
        assert_eq!(d.sides.len(), 2);
        assert!(d.sides.iter().all(|s| s.ships >= 1));
    }

    #[test]
    fn a_side_gets_the_truth_only_after_the_game_is_decided() {
        use crate::session::{LocalSession, Role};
        let build = || { let mut w = Scenario::Escort.build(42, scenario::home_system()); w.advance_to(600.0); w };
        let s = LocalSession::new(build());
        assert!(s.debrief(Role::Faction(FactionId(0))).is_none());
        assert!(s.debrief(Role::Spectator).is_some());
        let mut w = build();
        w.outcome = Some(Outcome { winner: FactionId(0), t: 600.0, reason: "test".into() });
        let s = LocalSession::new(w);
        let d = s.debrief(Role::Faction(FactionId(0))).unwrap();
        assert!(d.identities.iter().all(|(_, b)| d.paths.iter().any(|p| p.body == *b)));
    }

    #[test]
    fn sightings_are_bookkeeping_and_do_not_change_the_game() {
        // Two identical runs: one asks for debriefs along the way. The outcome must not move.
        let run = |ask: bool| {
            let mut w = Scenario::HideAndSeek.build(7, scenario::home_system());
            for h in 1..=6 {
                w.advance_to(h as f64 * 3600.0);
                if ask { let _ = w.debrief(Some(FactionId(0))); }
            }
            (w.losses.len(), w.bodies.iter().map(|b| b.trajectory.state_at(w.time()).map(|s| s.pos)).collect::<Vec<_>>())
        };
        let (a, b) = (run(false), run(true));
        assert_eq!(a.0, b.0);
        assert_eq!(a.1, b.1);
    }
}
