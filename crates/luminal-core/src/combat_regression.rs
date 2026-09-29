//! Cross-system regressions for the missile/evasion/ping balance pass.
use super::*;

fn quiet_lrm()->(World,BodyId) {
    let specs=(0..2).map(|i|BodySpec {name:format!("Ship {i}"),kind:BodyKind::Ship,faction:FactionId(i),
        state:State {pos:Vec2::new(i as f64*2.0*AU,0.0),vel:Vec2::ZERO},thrust:Vec2::ZERO,magazine:10}).collect();
    let mut w=World::new(System {bodies:vec![]},specs,0.0,8);
    for b in &mut w.bodies {b.controls.ecm=controls::Mode::Off;b.controls.evade=controls::Mode::Off;}
    let c=w.contact_id(FactionId(1),BodyId(0));
    w.perceptions.get_mut(&FactionId(1)).unwrap().ingest(Observation {contact:c,sensor:BodyId(1),origin:Vec2::new(2.0*AU,0.0),
        detection:sensors::DetectionLevel::Resolved,emitted_at:0.0,sensor_received_at:0.0,decider_received_at:0.0,source:Source::Emission,snr:1e12,
        measurement:Measurement::BearingRange {bearing:std::f64::consts::PI,range:2.0*AU,sigma_range:0.1,sigma_bearing:1e-9}},&w.system);
    let id=w.launch(BodyId(1),c,Payload::Nuclear).unwrap();
    // Isolate detection of a missile whose earlier boost was never observed.
    w.probability_flights.remove(&id);
    w.bodies[id.0 as usize].trajectory=Trajectory::new(0.0,State {pos:Vec2::new(0.2*AU,0.0),vel:Vec2::ZERO});
    w.bodies[id.0 as usize].missile.as_mut().unwrap().phase=Phase::Cruise;
    (w,id)
}
fn resolved(w:&World,id:BodyId)->bool {
    w.association.get(&(FactionId(0),id)).and_then(|c|w.perceptions[&FactionId(0)].contacts.get(c))
        .is_some_and(|c|c.detection(w.time)>=sensors::DetectionLevel::Resolved)
}

#[test]
fn ping_finds_unseen_cruising_lrm_after_echo_and_enables_interceptors() {
    let (mut w,id)=quiet_lrm();
    w.bodies[0].interceptor_battery=Some(interceptor::Battery {rounds:20,launched:0,ready_at:0.0,status:"Ready"});
    w.fit_point_defence(BodyId(0));
    w.advance_to(30.0);
    assert!(!resolved(&w,id));
    assert_eq!(w.bodies[0].interceptor_battery.unwrap().launched,0);
    assert!(w.ping(BodyId(0)));
    w.advance_to(100.0);
    assert!(!resolved(&w,id),"must wait for round-trip light, not the ping click");
    w.advance_to(30.0+0.4*AU/crate::units::C+12.0);
    assert!(resolved(&w,id));
    assert!(w.bodies[0].interceptor_battery.unwrap().launched>0,"echo should enable a defensive shot");
}

#[test]
fn terminal_lrms_and_all_srms_resolve_without_seeing_the_launcher() {
    let (mut w,id)=quiet_lrm();
    w.refresh_resolved_missiles();assert!(!resolved(&w,id));
    w.bodies[id.0 as usize].missile.as_mut().unwrap().phase=Phase::Terminal;
    w.refresh_resolved_missiles();assert!(resolved(&w,id));
    let (mut w,id)=quiet_lrm();
    w.bodies[id.0 as usize].missile.as_mut().unwrap().payload=Payload::Kinetic;
    w.bodies[id.0 as usize].missile.as_mut().unwrap().phase=Phase::Burn;
    w.refresh_resolved_missiles();assert!(resolved(&w,id));
}

#[test]
fn a_boost_acquisition_survives_dark_cruise() {
    let (mut w,id)=quiet_lrm();
    w.bodies[id.0 as usize].missile.as_mut().unwrap().phase=Phase::Burn;
    w.refresh_missile_contact(FactionId(0),id);
    w.bodies[id.0 as usize].missile.as_mut().unwrap().phase=Phase::Cruise;
    w.time=600.0;w.refresh_resolved_missiles();assert!(resolved(&w,id));
}

#[test]
fn nominal_lrm_is_75_percent_and_active_support_improves_both_payloads() {
    use weapon_probability::{hit_chance,ping_supported_chance};
    assert!((hit_chance(Payload::Nuclear,1.4*AU,1.0,0.0,0.0,1.0)-0.75).abs()<1e-12);
    for p in Payload::ALL {
        let passive=hit_chance(p,p.engagement_range(),1.0,0.0,0.0,1.0);
        assert!(ping_supported_chance(passive,1.0)>passive);
        assert_eq!(ping_supported_chance(passive,0.0),passive);
    }
    assert!((ping_supported_chance(0.75,1.0)-0.85).abs()<1e-12);
}

#[test]
fn active_fire_control_expires_and_missile_seeker_echoes_do_not_grant_ship_ping_bonus() {
    let (mut w,id)=quiet_lrm();
    let faction=FactionId(1);let c=w.association[&(faction,BodyId(0))];
    let mut obs=w.perceptions[&faction].contacts[&c].last;
    obs.source=Source::Echo;obs.sensor=BodyId(1);
    w.perceptions.get_mut(&faction).unwrap().ingest(obs,&w.system);
    let contact=&w.perceptions[&faction].contacts[&c];
    assert_eq!(contact.active_fire_control(0.0,|sensor|sensor==BodyId(1)),1.0);
    assert_eq!(contact.active_fire_control(76.0,|_|true),0.0);
    obs.sensor=id;
    w.perceptions.get_mut(&faction).unwrap().ingest(obs,&w.system);
    assert_eq!(w.perceptions[&faction].contacts[&c].active_fire_control(0.0,|sensor|sensor==BodyId(0)),0.0);
}

#[test]
fn damaged_power_or_screens_cap_charge_without_instant_recharge_on_repair() {
    use crate::damage::{Condition,System as S};
    for casualties in [vec![S::Power],vec![S::Screens],vec![S::Power,S::Screens]] {
        let (mut w,_)=quiet_lrm();let b=&mut w.bodies[0];
        b.has_screen=true;b.screen_up=true;b.thermal.field=1.0;
        for s in &casualties {b.damage.systems[*s as usize]=Condition::Damaged;}
        b.advance_thermal(0.0);
        assert_eq!(b.screen_available(),0.5);
        b.thermal.field=0.2;b.advance_thermal(60.0);
        assert!((b.screen_available()-0.202).abs()<1e-8,"normal recharge at damaged cap");
        b.advance_thermal(30_000.0);assert_eq!(b.screen_available(),0.5);
        for s in &casualties {b.damage.systems[*s as usize]=Condition::Intact;}
        assert_eq!(b.screen_available(),0.5,"repair restores ceiling, not charge");
        b.advance_thermal(30_060.0);assert!((b.screen_available()-0.502).abs()<1e-8);
        b.advance_thermal(60_000.0);assert_eq!(b.screen_available(),1.0);
    }
}
