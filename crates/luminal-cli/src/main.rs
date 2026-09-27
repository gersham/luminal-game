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
    let first = args.next();
    if first.as_deref()==Some("--beam-survey") {
        println!("range_ls,evasion_g,mean_coupled_j");
        for range in [1.0,3.0,10.0,30.0,100.0] {for evasion in [0.0,10.0] {
            let mean=(0..50).map(|seed|luminal_core::world::calibration::beam_trial(range,evasion,1000+seed)).sum::<f64>()/50.0;
            println!("{range},{evasion},{mean}");
        }}
        return;
    }
    if matches!(first.as_deref(),Some("--frigate-duel" | "--frigate-battle")) {
        let battle=first.as_deref()==Some("--frigate-battle");
        let seeds=args.next().and_then(|s|s.parse().ok()).unwrap_or(if battle {3} else {10});
        let depths=args.next().unwrap_or_else(||if battle {"30".into()} else {"0,5,10,20,40,80".into()});
        let range=args.next().map(|s|s.parse::<f64>().expect("range in AU")).unwrap_or(if first.as_deref()==Some("--frigate-battle") {0.002} else {1.0});
        let reaction=args.next().map(|s|s.parse::<f64>().expect("reaction seconds")).unwrap_or(10.0);
        println!("range_au,depth,seed,hits_a,hits_b,damage_a,damage_b,destroyed_a,destroyed_b,launched_a,launched_b,finished,interceptor_kills,elapsed_s,hull_a,hull_b,armour_a,armour_b,first_damage_s,first_loss_s,pd_kills,peak_screen_a,peak_screen_b,systems_a,systems_b,destroyed_systems_a,destroyed_systems_b");
        for depth in depths.split(',').map(|s|s.parse::<u32>().expect("integer magazine depth")) {
            for seed in 0..seeds {
                eprintln!("Starting duel: range={range} AU depth={depth} seed={}",1000+seed);
                let r=if first.as_deref()==Some("--frigate-battle") {luminal_core::world::calibration::frigate_battle(depth,1000+seed,range,reaction)} else {luminal_core::world::calibration::frigate_duel(depth,1000+seed,range)};
                println!("{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",range,r.depth,r.seed,r.hits[0],r.hits[1],r.damage_j[0],r.damage_j[1],r.destroyed[0],r.destroyed[1],r.interceptors_launched[0],r.interceptors_launched[1],r.finished,r.interceptor_kills,r.elapsed_s,r.hull[0],r.hull[1],r.armour[0],r.armour[1],r.first_damage_s.map_or(String::new(),|v|v.to_string()),r.first_loss_s.map_or(String::new(),|v|v.to_string()),r.pd_kills,r.peak_screen[0],r.peak_screen[1],r.system_casualties[0],r.system_casualties[1],r.systems_destroyed[0],r.systems_destroyed[1]);
            }
        }
        return;
    }
    if first.as_deref()==Some("--missile-balance") {
        let seeds=args.next().and_then(|s|s.parse::<u64>().ok()).unwrap_or(5);
        println!("payload,range_au,error_ls,seed,hit,damage_j,closest_km,finished");
        for payload in luminal_core::missile::Payload::ALL {for range in [0.03,0.1,1.0,2.5] {for error in [0.0,0.5,5.0] {for seed in 1000..1000+seeds {
            let r=luminal_core::world::calibration::weapon_trial_with_error(payload,range,0.0,0.0,seed,error,true);
            println!("{},{range},{error},{seed},{},{},{},{}",payload.name(),r.damage_j>1e6,r.damage_j,r.closest_km,r.finished);
        }}}}
        return;
    }
    if first.as_deref() == Some("--survey") {
        let seeds=args.next().and_then(|s|s.parse().ok()).unwrap_or(5);
        println!("payload,range_au,closure_kms,evasion_g,seed,closest_km,fuel_kms,damage_j,finished,destroyed");
        for r in luminal_core::world::calibration::survey(seeds) {
            println!("{},{},{},{},{},{:.3},{:.3},{:.3},{},{}",r.payload.name(),r.range_au,r.closure_kms,r.evasion_g,r.seed,r.closest_km,r.fuel_kms,r.damage_j,r.finished,r.destroyed);
        }
        return;
    }
    let raider_only = first.as_deref() == Some("--raider-only");
    let bots = first.as_deref() == Some("--doctrine") || raider_only;
    let hours: f64 = if bots { args.next() } else { first }.and_then(|a| a.parse().ok()).unwrap_or(6.0);
    let every_min: f64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(30.0);

    let mut s = LocalSession::new(scenario::transport_intercept());
    if bots { s.enable_bot(RAIDER,true); s.enable_bot(ESCORT,!raider_only); }
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
    if let Some(o)=&truth.outcome { println!("  OUTCOME {:?}: {}",o.winner,o.reason); }
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
