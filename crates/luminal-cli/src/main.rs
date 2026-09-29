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
    if matches!(first.as_deref(),Some("--version"|"-V")) {
        println!("luminal-cli {} commit={} dirty={}",env!("LUMINAL_VERSION"),env!("LUMINAL_COMMIT"),env!("LUMINAL_DIRTY"));
        return;
    }
    if matches!(first.as_deref(),Some("--missile-envelope"|"--edge-envelope")) {
        let seeds=args.next().and_then(|s|s.parse::<u64>().ok()).unwrap_or(32);
        println!("payload,range_au,closure_kms,evade,active,seed,hit,time,fuel,boost_au,terminal_au,reversals");
        for p in luminal_core::missile::Payload::ALL {
            let nominal=p.engagement_range()/luminal_core::units::AU;
            let cases=if first.as_deref()==Some("--edge-envelope") {
                vec![(1.25,0.0,false,false),(1.25,0.0,true,false),(1.3,0.0,false,false),(1.3,0.0,true,false),(1.5,0.0,false,false),(1.5,0.0,true,false)]
            } else {vec![(0.1,0.0,false,false),(0.1,0.0,true,false),(1.0,0.0,false,false),
                (1.0,0.0,true,false),(1.0,0.0,false,true),(1.1,0.0,false,false),(1.2,0.0,false,false),(1.2,0.0,true,false),(1.0,5000.0,false,false),(1.0,-5000.0,false,false)]};
            for (fraction,closure,evade,active) in cases {
                for seed in 0..seeds {
                    let r=luminal_core::world::calibration::envelope_trial(p,nominal*fraction,closure,evade,active,5000+seed);
                    println!("{},{},{},{},{},{},{},{},{},{},{},{}",p.name(),nominal*fraction,closure,evade,active,seed,r.hit,r.time,r.fuel,r.boost_au,r.terminal_au,r.engine_reversals);
                }
            }
        }
        return;
    }
    if first.as_deref()==Some("--class-evasion") {
        let seeds=args.next().and_then(|s|s.parse::<u64>().ok()).unwrap_or(64);
        println!("class,payload,evade,seed,hit,time");
        for class in luminal_core::world::ShipClass::COMBAT {for p in luminal_core::missile::Payload::ALL {for evade in [false,true] {for seed in 0..seeds {
            let r=luminal_core::world::calibration::envelope_trial_for_class(p,p.engagement_range()/luminal_core::units::AU,0.0,evade,false,36000+seed,0.0,class);
            println!("{},{},{evade},{seed},{},{}",class.name(),p.name(),r.hit,r.time);
        }}}}
        return;
    }
    if first.as_deref()==Some("--maneuver-envelope") {
        let seeds=args.next().and_then(|s|s.parse::<u64>().ok()).unwrap_or(128);
        println!("payload,fraction,closure_kms,target_g,seed,hit,time,fuel");
        for p in luminal_core::missile::Payload::ALL {
            for fraction in [0.5,1.0,1.1] {for (closure,burn) in [(5000.0,-100.0),(0.0,0.0),(-5000.0,100.0)] {
                for seed in 0..seeds {
                    let r=luminal_core::world::calibration::envelope_trial_with_burn(p,p.engagement_range()/luminal_core::units::AU*fraction,closure,false,false,20000+seed,burn);
                    println!("{},{fraction},{closure},{burn},{seed},{},{},{}",p.name(),r.hit,r.time,r.fuel);
                }
            }}
        }
        return;
    }
    if first.as_deref()==Some("--class-balance") {
        let seeds=args.next().and_then(|s|s.parse::<u64>().ok()).unwrap_or(10);
        let selection=args.next().unwrap_or_else(||"all".into());
        let depths=args.next().unwrap_or_else(||"stock".into());
        let range=args.next().and_then(|s|s.parse().ok()).unwrap_or(0.35);
        let first_seed=args.next().and_then(|s|s.parse::<u64>().ok()).unwrap_or(1000);
        println!("class,seed,depth,time,winner,srm_launched,lrm_launched,srm_hits,lrm_hits,srm_hp,lrm_hp,srm_crit,lrm_crit,beam_hits,beam_hp,beam_finish,reached_beams,hull_a,hull_b,interceptors_used,interceptor_kills,pd_kills,loss_reason,final_range_au,drive_disabled_a,drive_disabled_b");
        for class in luminal_core::world::ShipClass::COMBAT {
            if selection!="all" && !class.name().eq_ignore_ascii_case(&selection) {continue;}
            for depth in depths.split(',').map(|s|if s=="stock" {None} else {Some(s.parse().expect("interceptor depth"))}) {
                for seed in first_seed..first_seed+seeds {
                    let r=luminal_core::world::calibration::class_battle(class,seed,depth,range);
                    println!("{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",class.name(),r.seed,r.depth,r.time,r.winner,r.launched[0],r.launched[1],r.missile_hits[0],r.missile_hits[1],r.missile_hp[0],r.missile_hp[1],r.missile_crit[0],r.missile_crit[1],r.beam_hits,r.beam_hp,r.beam_finish,r.reached_beams,r.hull[0],r.hull[1],r.interceptors_used,r.interceptor_kills,r.pd_kills,r.loss_reason,r.final_range_au,r.propulsion_disabled[0],r.propulsion_disabled[1]);
                }
            }
        }
        return;
    }
    if first.as_deref()==Some("--sensor-sweep") {
        use luminal_core::sensors::{EmissivityFactors,SensorSuite,ship_detection_ew,detection_ranges,ping_range,resolution_factor};
        let cold=EmissivityFactors {visibility_multiplier:1.0,thrust_percent:0.0,heat_multiplier:1.0,size:7.0,stealth:50.0,ecm_on:true,recent_missiles:false,recent_beams:false};
        println!("state,ef,identity_au,resolved_au,approximate_au,bearing_au,ping_au,range_au,passive,damaged_df,ping");
        for (name,f) in [("reference",EmissivityFactors {thrust_percent:100.0,ecm_on:false,..cold}),("cold",cold),("thrust",EmissivityFactors {thrust_percent:100.0,..cold}),
            ("screens_thrust",EmissivityFactors {thrust_percent:100.0,..cold}),
            ("hot_fighting",EmissivityFactors {thrust_percent:100.0,heat_multiplier:11.0,recent_missiles:true,recent_beams:true,..cold})] {
            let ef=f.value();let r=detection_ranges(ef).map(|v|v/luminal_core::units::AU);
            let ew=resolution_factor(if f.ecm_on {100.0} else {0.0},50.0);
            for range in [0.002,0.03,0.1,0.25,0.75,1.0,1.25,3.0,10.0] {
                let detect=|df,ping|ship_detection_ew(SensorSuite::FULL,[1.0,df],ping,ef,range*luminal_core::units::AU,f.direction_active(),ew).label();
                println!("{name},{ef},{},{},{},{},{},{range},{},{},{}",r[0]*ew,r[1]*ew,r[2],r[3],ping_range(ef)/luminal_core::units::AU*ew,detect(1.0,0.0),detect(0.5,0.0),detect(1.0,1.0));
            }
        }
        return;
    }
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
    if matches!(first.as_deref(),Some("--srm-salvo"|"--lrm-salvo")) {
        let lrm=first.as_deref()==Some("--lrm-salvo");
        let payload=if lrm {luminal_core::missile::Payload::Nuclear} else {luminal_core::missile::Payload::Kinetic};
        let seeds=args.next().and_then(|s|s.parse::<u64>().ok()).unwrap_or(50);
        println!("range_au,depth,seed,killed,hull,hits,interceptor_kills,pd_kills,finished");
        for range in if lrm {[0.1,1.0]} else {[0.1,0.19]} {for depth in [0,30] {for seed in 1000..1000+seeds {
            let r=luminal_core::world::calibration::missile_salvo(payload,depth,seed,range);
            println!("{range},{depth},{seed},{},{},{},{},{},{}",r.destroyed[1],r.hull[1],r.hits[1],r.interceptor_kills,r.pd_kills,r.finished);
        }}}
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

    let class_name=args.next().unwrap_or_else(||"Frigate".into());
    let class=luminal_core::world::ShipClass::COMBAT.into_iter().find(|c|c.name().eq_ignore_ascii_case(&class_name)).expect("ship class");
    let seed=args.next().and_then(|s|s.parse().ok()).unwrap_or(42);
    let mut s = LocalSession::new(scenario::transport_intercept_class(seed,class));
    s.set_build_identity(env!("LUMINAL_VERSION"),env!("LUMINAL_COMMIT"),env!("LUMINAL_DIRTY")=="1");
    if let Some(path)=std::env::var_os("LUMINAL_DUEL_LOG") {s.enable_debug_log(std::path::Path::new(&path)).unwrap();}
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
        if s.view(Role::Spectator).outcome.is_some() {break;}
    }
    let wall = started.elapsed().as_secs_f64();
    let simulated=s.view(Role::Spectator).time;
    println!("\nsimulated {:.2} h in {wall:.2} s wall ({:.0}× real time)", simulated/3600.0,simulated/wall);
}

fn report(s: &LocalSession) {
    let truth = s.view(Role::Spectator);
    println!("T+ {:>6.0} min", truth.time / 60.0);
    if let Some(o)=&truth.outcome { println!("  OUTCOME {:?}: {}",o.winner,o.reason); }
    for b in truth.bodies.iter().filter(|b|b.kind==luminal_core::world::BodyKind::Ship) {
        println!("  SHIP {} hull={:.1}/{:.0} armour={:.1} screens={:.0}% heat={:.1}PJ SRM={} LRM={}",
            b.name,b.damage.damage.hull,b.damage.damage.hull_max,b.damage.damage.armour,b.damage.screen_available*100.0,
            b.thermal.heat_j/1e15,b.magazine[0],b.magazine[1]);
    }
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
