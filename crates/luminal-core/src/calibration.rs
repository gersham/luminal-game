//! Reproducible isolated weapon trials. An accurate initial state fix is a fixture,
//! not a live-game sensor rule; subsequent guidance uses ordinary delayed sensing.
use super::*;
use crate::celestial::{Celestial, CelestialKind, Orbit};
use crate::units::{C, G0};

#[derive(Clone, Debug, PartialEq)]
pub struct TrialResult {
    pub payload: Payload, pub range_au: f64, pub closure_kms: f64, pub evasion_g: f64,
    pub seed: u64, pub closest_km: f64, pub fuel_kms: f64, pub damage_j: f64,
    pub finished: bool, pub destroyed: bool,
}

pub fn weapon_trial(payload: Payload, range_au: f64, closure_kms: f64, evasion_g: f64, seed: u64) -> TrialResult {
    weapon_trial_with_error(payload,range_au,closure_kms,evasion_g,seed,0.0,false)
}

/// Frozen launch-platform picture isolates how a wrong initial aim survives
/// boost, finite seeker search and terminal correction. Local missile reports
/// remain causal; the target truth is never supplied to guidance.
pub fn weapon_trial_with_error(payload:Payload,range_au:f64,closure_kms:f64,evasion_g:f64,seed:u64,error_ls:f64,freeze:bool)->TrialResult {
    let system = System { bodies: vec![Celestial { name:"Star".into(),kind:CelestialKind::Star,
        gm:1.0,radius:1.0,orbit:Orbit::Fixed(Vec2::ZERO) }] };
    let base = Vec2::new(20.0*AU,0.0);
    let specs = vec![
        BodySpec { name:"Shooter".into(),kind:BodyKind::Ship,faction:FactionId(0),
            state:State {pos:base,vel:Vec2::new(0.0,closure_kms)},thrust:Vec2::ZERO,magazine:10 },
        BodySpec { name:"Target".into(),kind:BodyKind::Ship,faction:FactionId(1),
            state:State {pos:base+Vec2::new(0.0,range_au*AU),vel:Vec2::ZERO},thrust:Vec2::ZERO,magazine:0 },
    ];
    let mut w=World::new(system,specs,60.0,seed);
    let cid=w.contact_id(FactionId(0),BodyId(1));
    w.perceptions.get_mut(&FactionId(0)).unwrap().ingest(Observation {detection:crate::sensors::DetectionLevel::Resolved,
        contact:cid,sensor:BodyId(0),origin:base,emitted_at:0.0,sensor_received_at:0.0,decider_received_at:0.0,
        measurement:Measurement::BearingRange {bearing:std::f64::consts::FRAC_PI_2,range:range_au*AU,sigma_range:0.001,sigma_bearing:1e-10},
        snr:1e9,source:Source::Echo,
    },&w.system);
    let tr=w.perceptions.get_mut(&FactionId(0)).unwrap().contacts.get_mut(&cid).unwrap().track.as_mut().unwrap();
    // Isolate weapon delivery from initial velocity acquisition, but retain all
    // subsequent measurement error, datalink delay and unobserved manoeuvres.
    tr.p[2][2]=1e-4; tr.p[3][3]=1e-4;
    tr.p[4][4]=1e-6; tr.p[5][5]=1e-6;
    tr.x[0]+=error_ls*crate::units::LIGHT_SECOND;
    tr.p[0][0]=(error_ls*crate::units::LIGHT_SECOND/2.0).powi(2).max(1e-4);
    if freeze {w.bodies[0].sensors=sensors::SensorSuite {passive:false,active:false,direction_finding:false};}
    w.tactical_frame();
    let m=w.launch(BodyId(0),cid,payload).unwrap();
    let mut rng=Rng::stream(seed,99);
    let mut closest=f64::INFINITY;
    let limit=2.0*range_au*AU/(closure_kms+0.5*MISSILE_DELTA_V_KMS.value)+7200.0;
    let mut next_jink=0.0;
    while w.time()<limit && w.bodies[m.0 as usize].alive_at(w.time()) {
        if w.time()>=next_jink && w.bodies[1].alive_at(w.time()) {
            let a=rng.uniform()*std::f64::consts::TAU;
            w.set_thrust(BodyId(1),Vec2::new(a.cos(),a.sin())*(evasion_g*G0)).unwrap();
            next_jink+=60.0;
        }
        let lo=w.time();
        w.advance_to((lo+10.0).min(limit));
        let mt=&w.bodies[m.0 as usize].trajectory;
        let tt=&w.bodies[1].trajectory;
        let hi=w.time().min(mt.end().unwrap_or(w.time())).min(tt.end().unwrap_or(w.time()))-1e-7;
        if hi>lo && let Some((_,d))=missile::closest_approach(lo,hi,|at| Some(tt.state_at(at)?.pos-mt.state_at(at)?.pos)) { closest=closest.min(d); }
    }
    TrialResult {payload,range_au,closure_kms,evasion_g,seed,closest_km:closest,
        fuel_kms:w.bodies[m.0 as usize].missile.unwrap().dv_left,
        damage_j:w.hits.iter().filter(|h|h.body==BodyId(1) && h.missile==m).map(|h|h.energy_j).sum(),
        finished:!w.bodies[m.0 as usize].alive_at(w.time()),destroyed:!w.bodies[1].alive_at(w.time())}
}

pub fn survey(seeds:u64)->Vec<TrialResult> {
    let mut rows=vec![];
    for payload in Payload::ALL { for r in [0.01,0.1,1.0,10.0] { for v in [0.0,0.01*C] { for g in [0.0,10.0] { for seed in 0..seeds {
        rows.push(weapon_trial(payload,r,v,g,1000+seed));
    } } } } }
    rows
}

/// One ship-beam pulse with an initially exact track. The target may start an
/// unobserved lateral burn immediately after emission; no future truth is aimed at.
pub fn beam_trial(range_ls:f64,evasion_g:f64,seed:u64)->f64 {
    let system=System {bodies:vec![Celestial {name:"Reference".into(),kind:CelestialKind::Star,
        gm:0.0,radius:1.0,orbit:Orbit::Fixed(Vec2::ZERO)}]};
    let base=Vec2::new(20.0*AU,0.0);
    let mut w=World::new(system,(0..2).map(|i|BodySpec {name:format!("Ship {i}"),kind:BodyKind::Ship,faction:FactionId(i),
        state:State {pos:base+Vec2::new(i as f64*range_ls*crate::units::LIGHT_SECOND,0.0),vel:Vec2::ZERO},thrust:Vec2::ZERO,magazine:1}).collect(),1000.0,seed);
    let c=w.contact_id(FactionId(0),BodyId(1));
    w.perceptions.get_mut(&FactionId(0)).unwrap().ingest(Observation {detection:crate::sensors::DetectionLevel::Resolved,
        contact:c,sensor:BodyId(0),origin:base,emitted_at:0.0,sensor_received_at:0.0,decider_received_at:0.0,
        measurement:Measurement::BearingRange {bearing:0.0,range:range_ls*crate::units::LIGHT_SECOND,sigma_range:1e-5,sigma_bearing:1e-12},
        snr:1e12,source:Source::Echo},&w.system);
    w.fire_beam(BodyId(0),c).unwrap();
    w.set_thrust(BodyId(1),Vec2::new(0.0,evasion_g*G0)).unwrap();
    w.advance_to(range_ls+1.0);
    w.hits.iter().filter(|h|h.body==BodyId(1)).map(|h|h.energy_j).sum()
}

/// Symmetric stationary full-magazine exchange. Initial fixes are supplied to
/// isolate weapon balance; subsequent sensing, fuses and defence are unchanged.
#[derive(Debug, PartialEq)]
pub struct DuelResult {
    pub seed:u64, pub depth:u32, pub hits:[usize;2], pub damage_j:[f64;2],
    pub destroyed:[bool;2], pub interceptors_launched:[u32;2], pub finished:bool,
    pub interceptor_kills:usize,
    pub elapsed_s:f64, pub hull:[f64;2], pub armour:[f64;2],
    pub first_damage_s:Option<f64>, pub first_loss_s:Option<f64>,
    pub pd_kills:usize, pub peak_screen:[f64;2],pub system_casualties:[usize;2],pub systems_destroyed:[usize;2],
}
pub fn stationary_frigate_duel(depth:u32,seed:u64)->DuelResult {
    frigate_duel(depth,seed,1.0)
}
pub fn frigate_duel(depth:u32,seed:u64,range_au:f64)->DuelResult {
    run_frigate_duel(depth,seed,range_au,None,None)
}
/// Sustained battle: finite missile magazines followed by repeating beams. The
/// second ship has the same initial fix but cannot fire until its reaction delay.
pub fn frigate_battle(depth:u32,seed:u64,range_au:f64,reaction_s:f64)->DuelResult {
    assert!(reaction_s.is_finite() && reaction_s>=0.0);
    run_frigate_duel(depth,seed,range_au,Some(reaction_s),None)
}
/// One attacker dumps twenty SRMs; target retains screens and laser PD but
/// does not counterfire. Both platforms manoeuvre together at 100g so the
/// target evasion penalty is exercised without changing their separation.
pub fn srm_salvo(depth:u32,seed:u64,range_au:f64)->DuelResult {
    missile_salvo(Payload::Kinetic,depth,seed,range_au)
}
pub fn missile_salvo(payload:Payload,depth:u32,seed:u64,range_au:f64)->DuelResult {
    assert!(payload!=Payload::Beam);
    run_frigate_duel(depth,seed,range_au,None,Some(payload))
}
fn run_frigate_duel(depth:u32,seed:u64,range_au:f64,battle:Option<f64>,salvo:Option<Payload>)->DuelResult {
    assert!(range_au.is_finite() && range_au>0.0);
    let system=System {bodies:vec![Celestial {name:"Reference".into(),kind:CelestialKind::Star,
        gm:0.0,radius:1.0,orbit:Orbit::Fixed(Vec2::ZERO)}]};
    let specs=(0..2).map(|i|BodySpec {name:format!("Frigate {i}"),kind:BodyKind::Ship,faction:FactionId(i),
        state:State {pos:Vec2::new(20.0*AU,i as f64*range_au*AU),vel:Vec2::ZERO},thrust:Vec2::ZERO,
        magazine:MAGAZINE_FRIGATE.value as u32}).collect();
    let mut w=World::new(system,specs,1000.0,seed);
    if let Some(path)=std::env::var_os("LUMINAL_DUEL_LOG") {w.enable_debug_log(std::path::Path::new(&path)).unwrap();}
    let mut contacts=vec![];
    for i in 0..2 {
        let id=BodyId(i);
        let other=BodyId(1-i);
        w.bodies[i as usize].damage.hull=crate::damage::FRIGATE_HULL_HP;
        w.bodies[i as usize].damage.hull_max=crate::damage::FRIGATE_HULL_HP;
        w.bodies[i as usize].baseline_emission_factor=FRIGATE_EMISSION_FACTOR.value;
        w.set_screen(id,true).unwrap();
        w.bodies[i as usize].thermal.field=1.0;
        w.fit_point_defence(id);
        w.bodies[i as usize].point_defence.as_mut().unwrap().rate_hz=2.0;
        w.bodies[i as usize].interceptor_battery=Some(interceptor::Battery {rounds:depth,launched:0,ready_at:0.0,status:"Ready"});
        let origin=w.state(id,0.0).unwrap().pos;
        let rel=w.state(other,0.0).unwrap().pos-origin;
        let c=w.contact_id(FactionId(i as u8),other);
        w.perceptions.get_mut(&FactionId(i as u8)).unwrap().ingest(Observation {detection:crate::sensors::DetectionLevel::Resolved,
            contact:c,sensor:id,origin,emitted_at:0.0,sensor_received_at:0.0,decider_received_at:0.0,
            measurement:Measurement::BearingRange {bearing:bearing_of(rel),range:range_au*AU,sigma_range:0.001,sigma_bearing:1e-10},
            snr:1e9,source:Source::Echo},&w.system);
        let tr=w.perceptions.get_mut(&FactionId(i as u8)).unwrap().contacts.get_mut(&c).unwrap().track.as_mut().unwrap();
        tr.p[2][2]=1e-4;tr.p[3][3]=1e-4;tr.p[4][4]=1e-6;tr.p[5][5]=1e-6;
        contacts.push(c);
    }
    w.tactical_frame();
    if salvo.is_some() {for i in 0..2 {w.set_thrust(BodyId(i),Vec2::new(100.0*G0,0.0)).unwrap();}}
    let mut opened=[false;2];
    let limit=std::env::var("LUMINAL_DUEL_STOP_S").ok().and_then(|v|v.parse().ok()).unwrap_or(if battle.is_some() {7200.0} else {2.0*range_au*AU/(MISSILE_DELTA_V_KMS.value*MISSILE_BURN_FRACTION.value)+7200.0});
    let progress=std::env::var_os("LUMINAL_CALIBRATION_PROGRESS").is_some();
    let mut next_progress=0.0;
    let mut first_damage_s=None;
    let mut first_loss_s=None;
    let mut peak_screen=[0.0_f64;2];
    while w.time()<limit {
        for i in 0..2 {
            if salvo.is_some() && i==1 {continue;}
            if !opened[i] && w.time()>=if i==1 {battle.unwrap_or(0.0)} else {0.0} && w.bodies[i].alive_at(w.time()) {
                for _ in 0..if salvo.is_some() {20} else {MAGAZINE_FRIGATE.value as u32} {for payload in Payload::ALL {
                    if salvo.is_some_and(|selected|payload!=selected) {continue;}
                    // Match the game's range gates: impossible SRM shots must
                    // not act as free decoys that exhaust defence at 1 AU.
                    if range_au*AU>payload.engagement_range() {continue;}
                    w.queue_launch(BodyId(i as u32),contacts[i],payload).unwrap();
                }}
                if battle.is_some() {w.arm_beams(BodyId(i as u32)).unwrap();}
                opened[i]=true;
            }
            if battle.is_some() && w.bodies[i].alive_at(w.time()) && (w.time() as u64).is_multiple_of(60) {let _=w.ping(BodyId(i as u32));}
        }
        w.advance_to((w.time()+if battle.is_some() {1.0} else {30.0}).min(limit));
        for (i,peak) in peak_screen.iter_mut().enumerate() {*peak=peak.max(1.0-w.bodies[i].screen_available());}
        if first_damage_s.is_none() && w.bodies[..2].iter().any(|b|b.damage.hull<b.damage.hull_max) {first_damage_s=Some(w.time());}
        if first_loss_s.is_none() && w.bodies[..2].iter().any(|b|!b.alive_at(w.time())) {first_loss_s=Some(w.time());}
        if progress && w.time()>=next_progress {
            eprintln!("duel {range_au} AU depth {depth} seed {seed}: t={}s alive={} launches={:?}",w.time(),w.bodies.iter().filter(|b|b.missile.is_some() && b.alive_at(w.time())).count(),[w.bodies[0].interceptor_battery.unwrap().launched,w.bodies[1].interceptor_battery.unwrap().launched]);
            next_progress=w.time()+1000.0;
        }
        if battle.is_some() {
            // Continue after a knockout long enough to resolve counterfire already
            // in flight; stop at both lost or two minutes after the first loss.
            if w.bodies[..2].iter().all(|b|!b.alive_at(w.time())) || first_loss_s.is_some_and(|at|w.time()>at+120.0) {break;}
        } else if w.time()>30.0 && !w.bodies.iter().any(|b|b.alive_at(w.time()) && (b.missile.is_some() || b.missile_queued.iter().any(|n|*n>0))) {break;}
    }
    DuelResult {seed,depth,
        pd_kills:w.losses.iter().filter(|loss|matches!(loss.cause,LossCause::PointDefence {..}) && w.bodies[loss.body.0 as usize].missile.is_some()).count(),
        peak_screen,system_casualties:std::array::from_fn(|i|w.bodies[i].damage.systems.iter().filter(|s|**s!=crate::damage::Condition::Intact).count()),
        systems_destroyed:std::array::from_fn(|i|w.bodies[i].damage.systems.iter().filter(|s|**s==crate::damage::Condition::Destroyed).count()),
        elapsed_s:w.time(),hull:std::array::from_fn(|i|w.bodies[i].damage.hull),armour:std::array::from_fn(|i|w.bodies[i].damage.armour),first_damage_s,first_loss_s,
        interceptor_kills:w.losses.iter().filter(|loss|matches!(loss.cause,LossCause::Interceptor {..})).count(),
        hits:std::array::from_fn(|i|w.hits.iter().filter(|h|h.body==BodyId(i as u32) && h.energy_j>1e6).count()),
        damage_j:std::array::from_fn(|i|w.hits.iter().filter(|h|h.body==BodyId(i as u32)).map(|h|h.energy_j).sum()),
        destroyed:std::array::from_fn(|i|!w.bodies[i].alive_at(w.time())),
        interceptors_launched:std::array::from_fn(|i|w.bodies[i].interceptor_battery.unwrap().launched),
        finished:if battle.is_some() {first_loss_s.is_some()} else {!w.bodies.iter().any(|b|b.alive_at(w.time()) && (b.missile.is_some() || b.missile_queued.iter().any(|n|*n>0)))}}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "slow full-salvo balance calibration; run explicitly in release mode"]
    fn stationary_duel_is_repeatable_and_interceptors_reduce_hits() {
        let empty=frigate_duel(0,1000,0.03);
        let stocked=frigate_duel(30,1000,0.03);
        assert_eq!(stocked,frigate_duel(30,1000,0.03));
        assert!(empty.finished && stocked.finished);
        assert!(stocked.interceptors_launched.iter().all(|n|*n>0));
        assert!(stocked.hits.iter().sum::<usize>()<empty.hits.iter().sum::<usize>());
    }
    #[test]
    #[ignore = "sustained balance gate; run explicitly in release mode"]
    fn short_range_exchange_is_punishing_but_allows_counterfire() {
        let r=frigate_battle(5,1000,0.002,10.0);
        assert!(r.hits.iter().all(|n|*n>=5),"both ships must return sustained fire: {r:?}");
        assert!(r.first_loss_s.zip(r.first_damage_s).is_some_and(|(loss,first)|loss-first>=30.0 && loss<300.0),
            "inside 1 ls both ships must exchange fire, then die promptly: {r:?}");
        assert_eq!(r.interceptors_launched,[0,0],"inside 5 ls the lasers defend instead");
    }
    #[test]
    #[ignore = "full closing duel; run explicitly in release mode"]
    fn closing_frigates_reach_damaging_beam_combat() {
        let r=class_battle(ShipClass::Frigate,2000,None,0.35);
        assert!(r.missile_hp[0]>500.0,"SRMs must inflict substantial damage: {r:?}");
        assert!(r.reached_beams && r.beam_finish && r.beam_hp>500.0,"beam hits must penetrate and finish: {r:?}");
        assert!(r.interceptor_kills>0 && r.winner>=0);
    }
    #[test]
    #[ignore = "full regression duel for delayed acceleration feedback"]
    fn mutual_range_holding_does_not_permanently_spoil_beam_aim() {
        let r=class_battle(ShipClass::Destroyer,2000,None,0.35);
        assert!(r.beam_finish && r.beam_hp>1000.0 && r.time<28_800.0,
            "previously both ships oscillated at full thrust and missed forever: {r:?}");
    }
    #[test]
    fn weapon_trial_is_repeatable_and_records_delivery() {
        let a=weapon_trial(Payload::Nuclear,0.01,0.01*C,0.0,7);
        assert_eq!(a,weapon_trial(Payload::Nuclear,0.01,0.01*C,0.0,7));
        assert!(a.finished && a.closest_km.is_finite());
        assert!(a.fuel_kms>=0.0 && a.fuel_kms<=MISSILE_DELTA_V_KMS.value);
    }
    #[test]
    fn beams_reach_stationary_targets_beyond_knife_fight_range() {
        assert!(beam_trial(30.0,0.0,1000)>1e9);
        assert!(beam_trial(30.0,10.0,1000)<1.0);
        assert!(beam_trial(1.0,10.0,1000)>1e12);
    }
}

/// Closing, like-class duels with the actual fitted hulls, magazines, sensors,
/// thermal model, automatic evasion and local missile defence. Both pilots use
/// their received tracks, spend LRMs while closing, then SRMs, then beam standoff.
#[derive(Debug)]
pub struct ClassBattle {
    pub class:ShipClass,pub seed:u64,pub depth:u32,pub time:f64,pub winner:i32,
    pub launched:[u32;2],pub missile_hits:[usize;2],pub missile_hp:[f64;2],pub missile_crit:[usize;2],
    pub beam_hits:usize,pub beam_hp:f64,pub beam_finish:bool,pub reached_beams:bool,
    pub loss_reason:&'static str,
    pub hull:[f64;2],pub interceptors_used:u32,pub interceptor_kills:usize,pub pd_kills:usize,
}
pub fn class_battle(class:ShipClass,seed:u64,depth:Option<u32>,range_au:f64)->ClassBattle {
    assert!(range_au.is_finite() && range_au>0.0);
    let closing=std::env::var("LUMINAL_DUEL_CLOSURE_KMS").ok().and_then(|s|s.parse::<f64>().ok()).unwrap_or(2000.0);
    assert!(closing.is_finite() && closing.abs()<0.5*C);
    let beams_only=std::env::var("LUMINAL_DUEL_MISSILES").as_deref()==Ok("off");
    let source=crate::scenario::transport_intercept_class(seed,class);
    let system=System {bodies:vec![]};
    let base=Vec2::new(20.0*AU,0.0);
    let specs=(0..2).map(|i|BodySpec {name:format!("{} {i}",class.name()),kind:BodyKind::Ship,faction:FactionId(i),
        state:State {pos:base+Vec2::new(0.0,i as f64*range_au*AU),vel:Vec2::new(0.0,if i==0 {closing*0.5} else {-closing*0.5})},
        thrust:Vec2::ZERO,magazine:0}).collect();
    let mut w=World::new(system,specs,100_000.0,seed);
    let depth=depth.unwrap_or(class.interceptors());
    for i in 0..2 {
        let original=w.bodies[i].clone();
        let mut body=source.bodies[1].clone();body.faction=FactionId(i as u8);body.name=original.name;
        body.trajectory=original.trajectory;body.autopilot=None;body.route=None;body.commanded=Vec2::ZERO;
        body.beam_auto=false;body.beam_target=None;body.controllable=true;
        body.controls=controls::Controls::default();body.controls.screens=controls::Mode::On;
        body.screen_up=true;body.thermal.field=1.0;
        if beams_only {body.magazine=[0,0];}
        body.interceptor_battery.as_mut().unwrap().rounds=depth;
        body.facing=if i==0 {std::f64::consts::FRAC_PI_2} else {-std::f64::consts::FRAC_PI_2};body.turn_target=body.facing;
        w.bodies[i]=body;
        w.scheduler.schedule(0.0,Event::PointDefence(BodyId(i as u32)));
        w.reset_platform_history(BodyId(i as u32));
    }
    let contacts:Vec<_>=(0..2).map(|i| {
        let id=BodyId(i);let other=BodyId(1-i);let faction=FactionId(i as u8);
        let c=w.contact_id(faction,other);let origin=w.state(id,0.0).unwrap().pos;let seen=w.state(other,0.0).unwrap();let rel=seen.pos-origin;
        w.perceptions.get_mut(&faction).unwrap().ingest(Observation {contact:c,sensor:id,origin,emitted_at:0.0,sensor_received_at:0.0,decider_received_at:0.0,
            source:Source::Emission,detection:sensors::DetectionLevel::Resolved,snr:1e12,
            measurement:Measurement::BearingRange {range:rel.length(),bearing:bearing_of(rel),sigma_range:0.001,sigma_bearing:1e-10}},&w.system);
        let tr=w.perceptions.get_mut(&faction).unwrap().contacts.get_mut(&c).unwrap().track.as_mut().unwrap();
        tr.x[2]=seen.vel.x;tr.x[3]=seen.vel.y;tr.p[2][2]=0.001;tr.p[3][3]=0.001;
        c
    }).collect();
    w.tactical_frame();
    if let Some(path)=std::env::var_os("LUMINAL_DUEL_LOG") {w.enable_debug_log(std::path::Path::new(&path)).unwrap();}
    let mut reached_beams=false;
    let limit=std::env::var("LUMINAL_DUEL_STOP_S").ok().and_then(|s|s.parse().ok()).unwrap_or(28_800.0);
    while w.time()<limit && w.bodies[..2].iter().all(|b|b.alive_at(w.time())) {
        for (i,&c) in contacts.iter().enumerate() {
            let id=BodyId(i as u32);let b=&w.bodies[i];
            let Some(tr)=w.received_picture(id).and_then(|p|p.contacts.get(&c)).and_then(|c|c.estimate(w.time(),&w.system)) else {let _=w.ping(id);continue;};
            let range=(tr.pos()-w.state(id,w.time()).unwrap().pos).length();
            let payload=if b.magazine[0]>0 {Payload::Kinetic} else {Payload::Beam};
            let desired=autopilot::weapon_standoff(payload);
            if b.autopilot.is_none_or(|ap|!matches!(ap.order,Order::KeepRange(_,range) if (range-desired).abs()<1.0)) {
                let _=w.set_tactical_range(id,InterceptTarget::Contact(c),Some(desired));
            }
            let heat=w.bodies[i].thermal;
            if !heat.dumping && heat.heat_fraction()>1.0 {let _=w.set_heat_dump(id,true);}
            else if heat.dumping && heat.heat_fraction()<0.25 {let _=w.set_heat_dump(id,false);}
            if class!=ShipClass::Picket && !w.bodies[i].beam_auto {let _=w.arm_beams(id);}
            for p in Payload::ALL {
                if range<=if p==Payload::Kinetic {2.0*autopilot::weapon_standoff(p)} else {p.engagement_range()} && w.bodies[i].missile_queued[p.index()]==0 && w.time()>=w.bodies[i].missile_ready_at[p.index()] {
                    let _=w.queue_launch(id,c,p);
                }
            }
            if (w.time() as u64).is_multiple_of(60) {let _=w.ping(id);}
        }
        w.advance_to(w.time()+5.0);
        if (w.time() as u64).is_multiple_of(300) {
            let range=w.state(BodyId(0),w.time()).zip(w.state(BodyId(1),w.time())).map(|(a,b)|(a.pos-b.pos).length()/crate::units::LIGHT_SECOND);
            for (i,contact) in contacts.iter().enumerate() {
                let b=&w.bodies[i];
                let tr=w.received_picture(BodyId(i as u32)).and_then(|p|p.contacts.get(contact)).and_then(|c|c.estimate(w.time(),&w.system));
                let actual=w.state(BodyId(1-i as u32),w.time());
                let tracking=tr.zip(actual).map(|(tr,s)|(tr.pos()-s.pos,tr.vel()-s.vel,tr.accel()));
                w.debug_note("DUEL_STATE",format!("body={i} range_ls={range:?} hull={} armour={} heat_fraction={} dumping={} autopilot={:?} commanded={:?} systems={:?}",
                    b.damage.hull,b.damage.armour,b.thermal.heat_fraction(),b.thermal.dumping,b.autopilot,b.commanded,b.damage.systems));
                w.debug_note("DUEL_TRACK",format!("body={i} error_pos_vel_accel={tracking:?} actual={actual:?} thrust={:?}",w.bodies[1-i].trajectory.thrust_at(w.time())));
            }
        }
        // A shield-only grazing pulse is not evidence of a damaging beam fight.
        reached_beams|=w.bodies[..2].iter().all(|b|b.alive_at(w.time()))
            && w.hits.iter().filter(|h|h.payload==Payload::Beam).map(|h|h.hull_damage+h.armour_damage).sum::<f64>()
                >=0.05*w.bodies[0].damage.hull_max;
        for loss in &w.losses {
            assert!(!w.bodies[loss.body.0 as usize].alive_at(w.time()),"lost body remains alive");
        }
    }
    ClassBattle {class,seed,depth,time:w.time(),winner:match (w.bodies[0].alive_at(w.time()),w.bodies[1].alive_at(w.time())) {(true,false)=>0,(false,true)=>1,(false,false)=>2,_=>-1},
        launched:std::array::from_fn(|j|w.bodies.iter().filter(|b|b.missile.is_some_and(|m|m.payload==Payload::ALL[j])).count() as u32),
        missile_hits:std::array::from_fn(|j|w.hits.iter().filter(|h|h.payload==Payload::ALL[j]).count()),
        missile_hp:std::array::from_fn(|j|w.hits.iter().filter(|h|h.payload==Payload::ALL[j]).map(|h|h.hull_damage+h.armour_damage).sum()),
        missile_crit:std::array::from_fn(|j|w.hits.iter().filter(|h|h.payload==Payload::ALL[j] && h.system_hit.is_some()).count()),
        beam_hits:w.hits.iter().filter(|h|h.payload==Payload::Beam).count(),beam_hp:w.hits.iter().filter(|h|h.payload==Payload::Beam).map(|h|h.hull_damage+h.armour_damage).sum(),
        beam_finish:w.losses.iter().any(|l|l.body.0<2 && matches!(l.cause,LossCause::ShipBeam {..})),reached_beams,
        loss_reason:if w.bodies[..2].iter().any(|b|b.damage.hull<=0.0) {"hull"}
            else if w.bodies[..2].iter().any(|b|b.damage.state(crate::damage::System::Power)==crate::damage::Condition::Destroyed) {"reactor"} else {"timeout"},
        hull:std::array::from_fn(|i|w.bodies[i].damage.hull/w.bodies[i].damage.hull_max),
        interceptors_used:w.bodies[..2].iter().map(|b|b.interceptor_battery.unwrap().launched).sum(),
        interceptor_kills:w.losses.iter().filter(|l|matches!(l.cause,LossCause::Interceptor {..})).count(),
        pd_kills:w.losses.iter().filter(|l|matches!(l.cause,LossCause::PointDefence {..})).count()}
}
