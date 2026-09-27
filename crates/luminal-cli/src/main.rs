//! Headless runner: advances a scenario and prints what each faction believes, next to
//! the truth, plus how fast the simulation ran.
//!
//! Usage: luminal-cli [hours] [report-every-minutes]

use luminal_core::scenario::{self, ESCORT, RAIDER};
use luminal_core::session::{Command, LocalSession, Role};
use luminal_core::units::LIGHT_SECOND;
use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let hours: f64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(6.0);
    let every_min: f64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(30.0);

    let mut s = LocalSession::new(scenario::transport_intercept());
    s.command(Role::Spectator, Command::SetPaused(false)).unwrap();
    s.command(Role::Spectator, Command::SetWarp(1.0)).unwrap();

    let started = Instant::now();
    let step = every_min * 60.0;
    let mut t = 0.0;
    while t < hours * 3600.0 {
        s.tick(step);
        t += step;
        report(&s);
    }
    let wall = started.elapsed().as_secs_f64();
    println!("\nsimulated {hours} h in {wall:.2} s wall ({:.0}× real time)", hours * 3600.0 / wall);
}

fn report(s: &LocalSession) {
    let truth = s.view(Role::Spectator);
    println!("T+ {:>6.0} min", truth.time / 60.0);
    for l in &truth.losses {
        println!("  LOST {} at {:.0} s: {}", l.name, l.t, l.cause);
    }
    for f in [ESCORT, RAIDER] {
        let v = s.view(Role::Faction(f));
        let assoc = s.contact_truth(Role::Spectator, f).unwrap_or_default();
        for c in &v.contacts {
            let real = assoc.get(&c.id).and_then(|id| truth.bodies.iter().find(|b| b.id == *id));
            let age = v.time - c.last_emitted_at;
            match (&c.track, real) {
                (Some(tr), Some(r)) => println!(
                    "  {:?} C{} = {:<9} err {:>8.3} ls  2σ {:>8.3} ls  light age {:>5.0} s",
                    f,
                    c.id.0,
                    r.name,
                    (tr.pos - r.pos).length() / LIGHT_SECOND,
                    2.0 * (tr.cov[0][0] + tr.cov[1][1]).sqrt() / LIGHT_SECOND,
                    age
                ),
                (None, _) => println!("  {:?} C{} bearing only, light age {age:.0} s", f, c.id.0),
                (Some(_), None) => println!("  {:?} C{} track on a lost body", f, c.id.0),
            }
        }
    }
}
