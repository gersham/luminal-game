//! Things a scenario has arranged to happen part-way through: a stellar flare, a drive
//! casualty, a ship jumping in. Each is scheduled when the scenario is built, from the
//! scenario seed, and is known to a side only when its light or its report arrives.
use super::*;
use crate::damage::{Condition, System as S};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scripted {
    /// The star flares. See `terrain`.
    Flare,
    /// A drive casualty aboard this ship: propulsion goes from intact to damaged.
    Casualty(BodyId),
    /// A ship of `class` jumps in beside its side's senior ship. With no faction named it
    /// joins whichever side has fewer armed ships left, and nobody if they are level.
    Reinforcement { faction: Option<FactionId>, class: ShipClass },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Timed {
    pub at: f64,
    pub kind: Scripted,
}

impl World {
    pub fn schedule_script(&mut self, events: Vec<Timed>) {
        for event in events {
            self.scheduler.schedule(event.at.max(self.time), Event::Scripted(self.script.len()));
            self.script.push(event);
        }
    }

    /// A report for one side, carried at light speed from where it happened.
    fn notify(&mut self, faction: FactionId, origin: Vec2, text: String) {
        self.pending_notices.push((faction, Front { origin, t_emit: self.time }, text));
    }

    /// Reports that have reached a side's command ship, oldest first. `None` is every side's.
    pub fn notices(&self, faction: Option<FactionId>) -> Vec<(f64, String)> {
        let mut all: Vec<(f64, String)> = self.notices.iter().filter(|(f, _)| faction.is_none_or(|own| own == **f))
            .flat_map(|(_, notices)| notices.iter().cloned()).collect();
        all.sort_by(|a, b| a.0.total_cmp(&b.0));
        all
    }

    pub(super) fn run_scripted(&mut self, index: usize) {
        if !self.events_enabled || self.outcome.is_some() { return; }
        let Some(event) = self.script.get(index).copied() else { return };
        let t = self.time;
        match event.kind {
            Scripted::Flare => {
                let Some(star) = self.system.bodies.iter().position(|b| b.kind == crate::celestial::CelestialKind::Star) else { return };
                self.terrain.flares.push(terrain::Flare { t_emit: t, duration: FLARE_DURATION_S.value });
                let origin = self.system.state(star, t).pos;
                let text = format!("Stellar flare. Passive sensors degraded for {:.0} minutes.", FLARE_DURATION_S.value / 60.0);
                for faction in self.perceptions.keys().copied().collect::<Vec<_>>() { self.notify(faction, origin, text.clone()); }
            }
            Scripted::Casualty(id) => {
                let Some(b) = self.bodies.get_mut(id.0 as usize).filter(|b| b.alive_at(t)) else { return };
                if b.damage.state(S::Propulsion) != Condition::Intact { return; }
                b.damage.systems[S::Propulsion as usize].hit();
                let (faction, name) = (b.faction, b.name.clone());
                let Some(at) = b.trajectory.state_at(t).map(|s| s.pos) else { return };
                self.snapshot_platform(id, t);
                self.guide(id);
                self.notify(faction, at, format!("{name}: drive casualty. Propulsion damaged."));
            }
            Scripted::Reinforcement { faction, class } => {
                let armed = |w: &World, f: FactionId| w.bodies.iter().filter(|b| b.faction == f && b.kind == BodyKind::Ship && b.armed && !b.decoy && b.alive_at(t)).count();
                let faction = faction.or_else(|| {
                    let mut sides: Vec<(usize, FactionId)> = self.perceptions.keys().map(|f| (armed(self, *f), *f)).filter(|(n, _)| *n > 0).collect();
                    sides.sort();
                    (sides.len() >= 2 && sides[0].0 < sides[1].0).then(|| sides[0].1)
                });
                let Some(faction) = faction else { return };
                // The arrival is dressed on a ship already there. A side with nobody left gets nobody.
                let Some(senior) = self.bodies.iter().position(|b| b.faction == faction && b.kind == BodyKind::Ship && b.armed && !b.decoy && b.alive_at(t)) else { return };
                let Some(near) = self.bodies[senior].trajectory.state_at(t) else { return };
                let mut rng = Rng::stream(self.refinement_seed() ^ index as u64, 0x5245_494E);
                let angle = std::f64::consts::TAU * rng.uniform();
                let pos = near.pos + Vec2::new(angle.cos(), angle.sin()) * (REINFORCEMENT_OFFSET_AU.value * AU);
                let id = BodyId(self.bodies.len() as u32);
                let mut ship = self.bodies[senior].clone();
                ship.name = format!("Relief {}", class.name());
                ship.display_class = None;
                ship.trajectory = Trajectory::new(t, State { pos, vel: near.vel });
                ship.damage = Default::default();
                ship.thermal = crate::thermal::Thermal { last_t: t, ..Default::default() };
                ship.controls = Default::default();
                ship.autopilot = None; ship.route = None; ship.commanded = Vec2::ZERO; ship.avoidance = Avoidance { thrust: Vec2::ZERO, active: false, impossible: false };
                ship.jump = None; ship.jump_ready_at = t; ship.withdrawing = false;
                ship.missile = None; ship.missile_queued = [0; 2]; ship.missile_ready_at = [t; 2]; ship.missile_queue_ready_at = [t; 2];
                ship.last_missile_launch = None; ship.launch_generation = 0;
                ship.beam_ready_at = t; ship.beam_target = None; ship.last_beam = None; ship.beam_emitted_j = 0.0;
                ship.beam_aim = Default::default(); ship.beam_mode = Default::default(); ship.disrupted_until = 0.0;
                ship.spinal_ready_at = t; ship.spinal_tracking = None;
                ship.decoy = false; ship.probes = 0; ship.hull_j = 0.0; ship.screen_up = false; ship.step_generation = 0;
                self.bodies.push(ship);
                self.last_step.push(t);
                crate::scenario::fit_combatant(self, id, class, true);
                self.reset_platform_history(id);
                self.scheduler.schedule(t, Event::Step(id, 0));
                if class != ShipClass::Picket { let _ = self.arm_beams(id); }
                // On a side nobody is flying for it, it forms on the command ship. Doctrine replaces this at once.
                if let Some(flag) = self.decider(faction, t) { let _ = self.set_follow(id, flag); }
                self.record_combat(t, pos, CombatKind::JumpArrival, Some(id), None);
                let name = self.bodies[id.0 as usize].name.clone();
                self.notify(faction, pos, format!("{name} has jumped in to join you."));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::{home_system, Scenario, ESCORT, RAIDER};

    #[test]
    fn a_reinforcement_joins_the_weaker_side_fresh_and_is_seen_arriving() {
        let mut w = Scenario::Armada.build(42, home_system());
        w.script.clear();
        let before = w.bodies.len();
        w.schedule_script(vec![Timed { at: 100.0, kind: Scripted::Reinforcement { faction: None, class: ShipClass::Destroyer } }]);
        w.advance_to(200.0);
        assert_eq!(w.bodies.len(), before, "level sides get nobody");
        // Lose two raiders, and the next arrival is theirs.
        for id in [BodyId(17), BodyId(18)] { assert_eq!(w.bodies[id.0 as usize].faction, RAIDER); w.destroy(id, 200.0, LossCause::Impact(0)); }
        w.schedule_script(vec![Timed { at: 300.0, kind: Scripted::Reinforcement { faction: None, class: ShipClass::Destroyer } }]);
        w.advance_to(400.0);
        assert_eq!(w.bodies.len(), before + 1);
        let ship = w.bodies.last().unwrap();
        let senior = &w.bodies[10];
        assert_eq!((ship.faction, ship.ship_class, ship.kind), (RAIDER, Some(ShipClass::Destroyer), BodyKind::Ship));
        assert!(ship.armed && ship.controllable && ship.alive_at(400.0) && !ship.decoy);
        assert_eq!(ship.magazine, ShipClass::Destroyer.magazine());
        assert_eq!(ship.damage.hull, ship.damage.hull_max);
        assert!(ship.damage.systems.iter().all(|s| *s == Condition::Intact));
        let gap = (ship.trajectory.state_at(400.0).unwrap().pos - senior.trajectory.state_at(400.0).unwrap().pos).length();
        assert!((gap / AU - REINFORCEMENT_OFFSET_AU.value).abs() < 0.05, "it arrives beside its senior ship, {} AU off", gap / AU);
        assert_eq!(w.decider(RAIDER, 400.0), Some(BodyId(10)), "command does not pass to the newcomer");
        assert!(w.jump_events(None).iter().any(|e| e.kind == CombatKind::JumpArrival), "the arrival is a jump flash like any other");
        // Its own side hears at light speed; the other side is told nothing.
        w.advance_to(2_000.0);
        assert!(w.notices(Some(RAIDER)).iter().any(|(_, text)| text.contains("jumped in")));
        assert!(w.notices(Some(ESCORT)).is_empty());
    }

    #[test]
    fn a_drive_casualty_damages_propulsion_once_and_reports_at_light_speed() {
        let mut w = Scenario::Convoy.build(42, home_system());
        w.script.clear();
        let victim = BodyId(w.bodies.iter().position(|b| b.ship_class == Some(ShipClass::Transport)).unwrap() as u32);
        let faction = w.bodies[victim.0 as usize].faction;
        let limit = w.bodies[victim.0 as usize].max_accel();
        w.schedule_script(vec![Timed { at: 50.0, kind: Scripted::Casualty(victim) }, Timed { at: 60.0, kind: Scripted::Casualty(victim) }]);
        w.advance_to(55.0);
        assert_eq!(w.bodies[victim.0 as usize].damage.state(S::Propulsion), Condition::Damaged);
        assert!(w.bodies[victim.0 as usize].max_accel() < 0.6 * limit);
        w.advance_to(70.0);
        assert_eq!(w.bodies[victim.0 as usize].damage.state(S::Propulsion), Condition::Damaged, "a second casualty does not finish the drive");
        w.advance_to(1_200.0);
        let notices = w.notices(Some(faction));
        assert_eq!(notices.iter().filter(|(_, text)| text.contains("drive casualty")).count(), 1);
        assert!(notices[0].0 >= 50.0);
        assert!(w.notices(Some(if faction == ESCORT { RAIDER } else { ESCORT })).is_empty(), "the enemy is not told");
    }

    #[test]
    fn a_flare_is_announced_when_its_light_arrives_and_events_can_be_switched_off() {
        let mut w = Scenario::Escort.build(42, home_system());
        w.script.clear();
        w.schedule_script(vec![Timed { at: 10.0, kind: Scripted::Flare }]);
        w.advance_to(20.0);
        assert_eq!(w.terrain.flares.len(), 1);
        assert!(w.notices(None).is_empty(), "the front is still on its way out");
        w.advance_to(3_600.0);
        assert!(w.notices(Some(ESCORT)).iter().any(|(at, text)| *at > 100.0 && text.contains("flare")));
        let mut quiet = Scenario::Escort.build(42, home_system());
        quiet.script.clear();
        quiet.events_enabled = false;
        quiet.schedule_script(vec![Timed { at: 10.0, kind: Scripted::Flare }]);
        quiet.advance_to(20.0);
        assert!(quiet.terrain.flares.is_empty());
    }

    #[test]
    fn every_scenario_schedules_its_events_from_the_seed_alone() {
        for scenario in Scenario::ALL {
            let (a, b) = (scenario.build(42, home_system()), scenario.build(42, home_system()));
            assert_eq!(a.script, b.script, "{}", scenario.name());
            for event in &a.script {
                assert!(event.at >= 3_600.0, "{} has an event in the first hour", scenario.name());
                if let Scripted::Casualty(id) = event.kind { assert!(a.bodies[id.0 as usize].ship_class == Some(ShipClass::Transport)); }
            }
        }
        // Relief and Last Ship bring in an enemy of the player, some games and not others.
        for (scenario, class) in [(Scenario::Relief, ShipClass::Frigate), (Scenario::LastShip, ShipClass::Picket)] {
            let scripts: Vec<Vec<Timed>> = (0..24).map(|seed| scenario.build(seed, home_system()).script).collect();
            let world = scenario.build(1, home_system());
            let player = world.objective.as_ref().unwrap().player.unwrap();
            let enemy = if world.bodies[player.0 as usize].faction == ESCORT { RAIDER } else { ESCORT };
            let arrivals = scripts.iter().filter(|script| script.iter().any(|e| e.kind == Scripted::Reinforcement { faction: Some(enemy), class })).count();
            assert!(arrivals > 4 && arrivals < 20, "{}: {arrivals} of 24 games", scenario.name());
        }
        let relief: Vec<bool> = (0..24).map(|seed| !Scenario::Relief.build(seed, home_system()).script.is_empty()).collect();
        let last: Vec<bool> = (0..24).map(|seed| !Scenario::LastShip.build(seed, home_system()).script.is_empty()).collect();
        assert_ne!(relief, last, "two scenarios on one seed do not share their luck");
        for scenario in Scenario::ALL {
            let world = scenario.build(42, home_system());
            let player = world.objective.as_ref().unwrap().player.unwrap();
            assert!(world.bodies[player.0 as usize].decoys > 0, "{}: the player's ship carries decoys", scenario.name());
        }
        // Across seeds, every kind turns up somewhere and the timings differ.
        let all: Vec<Timed> = (0..40).flat_map(|seed| Scenario::ALL.into_iter().flat_map(move |s| s.build(seed, home_system()).script)).collect();
        assert!(all.iter().any(|e| e.kind == Scripted::Flare));
        assert!(all.iter().any(|e| matches!(e.kind, Scripted::Casualty(_))));
        assert!(all.iter().any(|e| matches!(e.kind, Scripted::Reinforcement { .. })));
    }
}
