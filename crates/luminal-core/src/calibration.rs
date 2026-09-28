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
    run_frigate_duel(depth,seed,range_au,None)
}
/// Sustained battle: finite missile magazines followed by repeating beams. The
/// second ship has the same initial fix but cannot fire until its reaction delay.
pub fn frigate_battle(depth:u32,seed:u64,range_au:f64,reaction_s:f64)->DuelResult {
    assert!(reaction_s.is_finite() && reaction_s>=0.0);
    run_frigate_duel(depth,seed,range_au,Some(reaction_s))
}
fn run_frigate_duel(depth:u32,seed:u64,range_au:f64,battle:Option<f64>)->DuelResult {
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
    let mut opened=[false;2];
    let limit=std::env::var("LUMINAL_DUEL_STOP_S").ok().and_then(|v|v.parse().ok()).unwrap_or(if battle.is_some() {7200.0} else {2.0*range_au*AU/(MISSILE_DELTA_V_KMS.value*MISSILE_BURN_FRACTION.value)+7200.0});
    let progress=std::env::var_os("LUMINAL_CALIBRATION_PROGRESS").is_some();
    let mut next_progress=0.0;
    let mut first_damage_s=None;
    let mut first_loss_s=None;
    let mut peak_screen=[0.0_f64;2];
    while w.time()<limit {
        for i in 0..2 {
            if !opened[i] && w.time()>=if i==1 {battle.unwrap_or(0.0)} else {0.0} && w.bodies[i].alive_at(w.time()) {
                for _ in 0..MAGAZINE_FRIGATE.value as u32 {for payload in Payload::ALL {
                    w.queue_launch(BodyId(i as u32),contacts[i],payload).unwrap();
                }}
                if battle.is_some() {w.arm_beams(BodyId(i as u32)).unwrap();}
                opened[i]=true;
            }
            if battle.is_some() && w.bodies[i].alive_at(w.time()) && (w.time() as u64).is_multiple_of(60) {let _=w.ping(BodyId(i as u32));}
        }
        w.advance_to((w.time()+if battle.is_some() {1.0} else {30.0}).min(limit));
        for (i,peak) in peak_screen.iter_mut().enumerate() {*peak=peak.max(w.bodies[i].screen_j/SCREEN_CAPACITY_J.value);}
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
        } else if w.time()>30.0 && !w.bodies.iter().any(|b|b.missile.is_some() && b.alive_at(w.time())) {break;}
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
        finished:if battle.is_some() {first_loss_s.is_some()} else {!w.bodies.iter().any(|b|b.missile.is_some() && b.alive_at(w.time()))}}
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
    fn sustained_frigate_battle_survives_opening_and_reaches_knockout() {
        let r=frigate_battle(5,1000,0.002,10.0);
        assert!(r.hits.iter().all(|n|*n>=5),"both ships must return sustained fire: {r:?}");
        assert!(r.first_loss_s.is_some_and(|t|(300.0..7200.0).contains(&t)),"no opening kill or indefinite screen tank: {r:?}");
        assert_eq!(r.interceptors_launched,[0,0],"inside 5 ls the lasers defend instead");
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
