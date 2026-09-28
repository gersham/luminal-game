//! Signature and measurement physics. Pure functions of physical quantities; the world
//! decides which light reaches which sensor, this module decides what it measures.
//!
//! Every constant used here comes from `params` and is currently a placeholder.

use crate::kinematics::Vec2;
use crate::params::*;
use crate::rng::Rng;
use crate::units::G0;
use crate::mind::Measurement;
use std::f64::consts::PI;

/// Live signature multiplier used by ship sensing.
#[derive(Clone,Copy,Debug)]
pub struct EmissivityFactors {
    pub visibility_multiplier:f64,
    pub thrust_percent:f64, pub screen_percent:f64, pub screen_on:bool,
    pub size:f64, pub stealth:f64, pub ecm_on:bool,
    pub recent_missiles:bool, pub recent_beams:bool,
}

#[derive(Clone,Copy,Debug,PartialEq,Eq,PartialOrd,Ord)]
pub enum DetectionLevel {None,Bearing,Approximate,Resolved,Identity}
impl DetectionLevel {
    pub fn label(self)->&'static str {match self {Self::None=>"None",Self::Bearing=>"Bearing",Self::Approximate=>"Approximate",Self::Resolved=>"Resolved",Self::Identity=>"Identity"}}
}
pub const PING_RESOLUTION_S:f64=60.0;
pub const CONTACT_TRANSITION_S:f64=2.0;
/// Size 7, stealth 50%, full nominal thrust, raised cold screens, ECM off.
pub const REFERENCE_EF:f64=1.4;
fn signature_scale(ef:f64)->f64 {
    if ef.is_finite() {ef.max(0.0)/REFERENCE_EF} else {0.0}
}
pub fn detection_ranges(ef:f64)->[f64;4] {
    [0.01,0.1,2.0,10.0].map(|au|au*crate::units::AU*signature_scale(ef))
}
pub fn ping_range(ef:f64)->f64 {
    crate::units::AU*signature_scale(ef)
}
/// Sensor degradation affects distance, not intensity: half effectiveness is
/// explicitly half range. Active identification has its own range envelope.
pub fn ship_detection(suite:SensorSuite,effectiveness:[f64;2],active:f64,ef:f64,range:f64,direction_active:bool)->DetectionLevel {
    ship_detection_ew(suite,effectiveness,active,ef,range,direction_active,1.0)
}
/// Smooth advantage curve, with a hard 50% cap; equal ratings cancel.
pub fn resolution_factor(ecm:f64,eccm:f64)->f64 {
    let net=(ecm.max(0.0)-eccm.max(0.0)).max(0.0);
    1.0-(net/(50.0+net)).min(0.5)
}
pub fn ship_detection_ew(suite:SensorSuite,effectiveness:[f64;2],active:f64,ef:f64,range:f64,direction_active:bool,resolution_factor:f64)->DetectionLevel {
    use DetectionLevel::*;
    let [identity,resolved,approximate,bearing]=detection_ranges(ef);
    if !range.is_finite() || range<0.0 || ef<=0.0 {return None;}
    let passive=if suite.passive {effectiveness[0].clamp(0.0,1.0)} else {0.0};
    let ping=if suite.active {active.clamp(0.0,1.0)} else {0.0};
    let locating=passive.max(ping);
    let resolution_factor=resolution_factor.clamp(0.5,1.0);
    if (locating>0.0 && range<=identity*locating*resolution_factor) || (ping>0.0 && range<=ping_range(ef)*ping*resolution_factor) {Identity}
    else if passive>0.0 && range<=resolved*passive*resolution_factor {Resolved}
    else if passive>0.0 && range<=approximate*passive {Approximate}
    else if direction_active && suite.direction_finding && effectiveness[1]>0.0 && range<=bearing*effectiveness[1] {Bearing}
    else {None}
}
/// Approximate ellipse semi-axes, 1–10 light-seconds, growing with range.
pub fn approximate_axes(range:f64,ef:f64)->(f64,f64) {
    let ranges=detection_ranges(ef);
    let fraction=((range-ranges[1])/(ranges[2]-ranges[1]).max(1.0)).clamp(0.0,1.0);
    let major=(1.0+9.0*fraction)*crate::units::LIGHT_SECOND;
    (major,major*0.65)
}
pub fn ship_measurement(level:DetectionLevel,range:f64,bearing:f64,ef:f64)->Measurement {
    match level {
        DetectionLevel::None|DetectionLevel::Bearing=>Measurement::Bearing {bearing,sigma:0.002},
        DetectionLevel::Approximate=>{
            let (major,minor)=approximate_axes(range,ef);
            Measurement::BearingRange {bearing,range,sigma_range:major/2.0,sigma_bearing:minor/(2.0*range.max(1.0))}
        },
        _=>Measurement::BearingRange {bearing,range,sigma_range:0.1,sigma_bearing:1e-7},
    }
}
impl EmissivityFactors {
    pub fn direction_active(self)->bool {self.screen_on || self.thrust_percent>1e-9 || self.recent_missiles || self.recent_beams}
    pub fn value(self)->f64 {
        self.visibility_multiplier.max(0.0)*(1.0+self.thrust_percent.clamp(0.0,120.0)/100.0)
            *(1.0+self.screen_percent.clamp(0.0,100.0)/10.0)
            *if self.screen_on {2.0} else {1.0}
            *(self.size.max(0.0)/10.0)*(1.0-self.stealth.clamp(0.0,100.0)/100.0)
            *if self.ecm_on {1.5} else {1.0}
            *if self.recent_missiles {1.2} else {1.0}
            *if self.recent_beams {1.5} else {1.0}
    }
}

/// Isotropic emission, W, of a ship accelerating at `accel` (km/s²).
pub fn ship_emission_w(accel: Vec2) -> f64 {
    platform_emission_w(false,1.0,accel,0.0)
}

/// Isotropic emission, W, of a missile thrusting at `accel` (km/s²).
pub fn missile_emission_w(accel: Vec2) -> f64 {
    platform_emission_w(true,1.0,accel,0.0)
}

/// Platform baseline plus actual drive output and thermal/screen radiation.
/// Stealth changes the fixed baseline, not the visibility of an arbitrary drive burn.
pub fn platform_emission_w(missile:bool,baseline_factor:f64,thrust:Vec2,thermal_w:f64)->f64 {
    let (cold,drive)=if missile {(MISSILE_SIGNATURE_COLD_W.value,MISSILE_DRIVE_W_PER_G.value)}
        else {(SIGNATURE_COLD_W.value,SIGNATURE_DRIVE_W_PER_G.value)};
    cold*baseline_factor+drive*thrust.length()/G0+thermal_w
}

/// Intensity, W/m², at `range_km` from an isotropic source of `power_w`.
pub fn intensity(power_w: f64, range_km: f64) -> f64 {
    let r_m = (range_km * 1e3).max(1.0);
    power_w / (4.0 * PI * r_m * r_m)
}

/// One frame's detection decision, P(detect) = SNR / (SNR + threshold): 50 % at the
/// threshold, and a long power-law tail beyond it (a heavy-tailed scintillation stand-in,
/// PLACEHOLDER). A distant burner flashes up now and then as a coarse bearing.
pub fn detected(snr: f64, rng: &mut Rng) -> bool {
    rng.uniform() * (snr + PASSIVE_DETECT_SNR.value) < snr
}

/// Range, km, at which an isotropic source of `power_w` is detected half the time.
pub fn passive_detection_range_km(power_w: f64) -> f64 {
    (power_w / (4.0 * PI * PASSIVE_NOISE_FLOOR.value * PASSIVE_DETECT_SNR.value)).sqrt() / 1e3
}

/// Echo intensity back at the pinger: out to the target, reflected, and back.
pub fn echo_intensity(ping_w: f64, out_km: f64, back_km: f64) -> f64 {
    let at_target = intensity(ping_w, out_km);
    intensity(at_target * ACTIVE_CROSS_SECTION_M2.value, back_km)
}

/// Echo strength falls as range to the fourth power: half the maximum
/// detection radius (SNR 9) requires 16 times the signal for resolution.
pub fn echo_resolvable(snr: f64) -> bool {
    snr >= 9.0 * 16.0 * (1.0 - 1e-12)
}

/// Direction of `v` in radians, counter-clockwise from +x.
pub fn bearing_of(v: Vec2) -> f64 {
    v.y.atan2(v.x)
}

pub fn wrap_angle(a: f64) -> f64 {
    (a + PI).rem_euclid(2.0 * PI) - PI
}

/// A passive measurement: bearing with noise, and its 1σ.
pub fn measure_bearing(true_bearing: f64, snr: f64, rng: &mut Rng) -> (f64, f64) {
    let sigma = (DIRECTION_BEARING_SIGMA.value / snr.sqrt()).min(0.5);
    (wrap_angle(true_bearing + sigma * rng.gaussian()), sigma)
}

/// Two passive channels: a high-quality localization fix nearby, otherwise a coarse
/// bearing. Range is a noisy TL7 sensor measurement, never an exact truth handoff.
/// Power is supplied by the emitter, allowing future size/stealth signatures.
/// Installed channels; active operation is separately controlled by orders/phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SensorSuite { pub passive: bool, pub active: bool, pub direction_finding: bool }
impl SensorSuite {
    pub const FULL: Self=Self {passive:true,active:true,direction_finding:true};
    pub const MISSILE: Self=Self {passive:true,active:true,direction_finding:false};
}

pub fn passive_measurement(power_w: f64, range: f64, bearing: f64, rng: &mut Rng) -> Option<(Measurement, f64)> {
    receive_measurement(SensorSuite::FULL,power_w,range,bearing,rng)
}

pub fn receive_measurement(suite:SensorSuite,power_w:f64,range:f64,bearing:f64,rng:&mut Rng)->Option<(Measurement,f64)> {
    receive_measurement_scaled(suite,power_w,range,bearing,[1.0,1.0],rng)
}
pub fn receive_measurement_scaled(suite:SensorSuite,power_w:f64,range:f64,bearing:f64,effectiveness:[f64;2],rng:&mut Rng)->Option<(Measurement,f64)> {
    let base = intensity(power_w, range) / PASSIVE_NOISE_FLOOR.value;
    let snr=base*effectiveness[0];
    if suite.passive && snr >= PASSIVE_DETECT_SNR.value && detected(snr, rng) {
        let sigma_bearing = PASSIVE_BEARING_SIGMA.value / snr.sqrt();
        let sigma_range = range * PASSIVE_RANGE_FRACTION.value / snr.sqrt();
        return Some((Measurement::BearingRange {
            bearing: wrap_angle(bearing + sigma_bearing * rng.gaussian()),
            sigma_bearing,
            range: (range + sigma_range * rng.gaussian()).max(0.0),
            sigma_range,
        }, snr));
    }
    if !suite.direction_finding { return None; }
    let direction_snr = base * effectiveness[1] * DIRECTION_RANGE_RATIO.value.powi(2);
    if direction_snr < PASSIVE_DETECT_SNR.value || !detected(direction_snr, rng) {
        return None;
    }
    let (bearing, sigma) = measure_bearing(bearing, direction_snr, rng);
    Some((Measurement::Bearing { bearing, sigma }, direction_snr))
}

/// A missile seeker's bearing: photon-limited like a passive array, but never better
/// than its pointing floor.
pub fn measure_seeker_bearing(true_bearing: f64, snr: f64, rng: &mut Rng) -> f64 {
    let sigma = (MISSILE_SEEKER_BEARING_SIGMA.value / snr.sqrt()).clamp(MISSILE_SEEKER_ANGLE_FLOOR.value, 0.5);
    wrap_angle(true_bearing + sigma * rng.gaussian())
}

/// An echo measurement: bearing and range with noise, and their 1σ.
pub struct EchoMeasurement {
    pub bearing: f64,
    pub sigma_bearing: f64,
    pub range: f64,
    pub sigma_range: f64,
}

pub fn measure_echo(true_bearing: f64, true_range: f64, snr: f64, rng: &mut Rng) -> EchoMeasurement {
    let sigma_bearing = (ACTIVE_BEARING_SIGMA.value / snr.sqrt()).min(0.5);
    let bearing = wrap_angle(true_bearing + sigma_bearing * rng.gaussian());
    let sigma_range = ACTIVE_RANGE_SIGMA_KM.value / snr.sqrt();
    EchoMeasurement { bearing, sigma_bearing, range: true_range + sigma_range * rng.gaussian(), sigma_range }
}

/// Persistent calibration-error scale. Kept in the observation covariance floor.
pub fn systematic_range(range: f64, snr: f64, source: crate::mind::Source) -> f64 {
    let fraction=if source==crate::mind::Source::Echo { ACTIVE_SYSTEMATIC_FRACTION.value } else { PASSIVE_SYSTEMATIC_FRACTION.value };
    range*fraction/snr.sqrt().max(1.0)
}

#[derive(Clone, Copy, Debug)]
pub struct SeekerFix {
    pub t: f64, pub pos: Vec2, pub vel: Vec2, pub samples: u32,
    history: [(f64,Vec2);32],
}
impl SeekerFix {
    /// A quadratic local fit avoids interpreting a thrusting missile's average
    /// velocity over the observation window as its current velocity.
    pub fn accelerating(mut self)->Self {
        let n=self.samples.min(32) as usize;
        if n<8 {return self;}
        let mean=self.history[..n].iter().map(|(t,_)|t-self.t).sum::<f64>()/n as f64;
        let mut m2=0.0;let mut m3=0.0;let mut m4=0.0;
        let mut p0=Vec2::ZERO;let mut p1=Vec2::ZERO;let mut p2=Vec2::ZERO;
        let origin=self.history[0].1;
        for (t,p) in &self.history[..n] {
            let x=t-self.t-mean;let y=*p-origin;
            m2+=x*x;m3+=x*x*x;m4+=x.powi(4);
            p0=p0+y;p1=p1+y*x;p2=p2+y*(x*x);
        }
        let q=m4-m2*m2/n as f64;let det=m2*q-m3*m3;
        if det<=1e-12 {return self;}
        let c=((p2-p0*(m2/n as f64))*m2-p1*m3)*(1.0/det);
        let b=(p1-c*m3)*(1.0/m2);
        self.pos=origin+p0*(1.0/n as f64)-b*mean+c*(mean*mean-m2/n as f64);
        self.vel=b-c*(2.0*mean);
        self
    }
    pub fn update(previous: Option<Self>, t: f64, pos: Vec2, prior_vel: Vec2) -> Self {
        let mut history=previous.map_or([(f64::NEG_INFINITY,Vec2::ZERO);32],|old|old.history);
        history.rotate_right(1); history[0]=(t,pos);
        let samples=previous.map_or(1,|old|old.samples+1);
        let n=samples.min(32) as usize;
        let mean_t=history[..n].iter().map(|(at,_)|at-t).sum::<f64>()/n as f64;
        let mean_p=history[..n].iter().fold(Vec2::ZERO,|sum,(_,p)|sum+(*p-pos))*(1.0/n as f64);
        let denom=history[..n].iter().map(|(at,_)|(at-t-mean_t).powi(2)).sum::<f64>();
        let vel=if denom>1e-12 { history[..n].iter().fold(Vec2::ZERO,|sum,(at,p)|sum+(*p-pos-mean_p)*(at-t-mean_t))*(1.0/denom) } else {prior_vel};
        let fitted=pos+mean_p-vel*mean_t;
        Self {t,pos:fitted,vel,samples,history}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reference_frigate_has_requested_boundaries_and_independent_ping_range() {
        use DetectionLevel::*;
        let au=crate::units::AU;
        for (range,passive,active) in [(0.1,Resolved,Identity),(0.101,Approximate,Identity),
            (1.0,Approximate,Identity),(1.001,Approximate,Approximate),
            (2.001,Bearing,Bearing),(10.0,Bearing,Bearing),(10.001,None,None)] {
            assert_eq!(ship_detection(SensorSuite::FULL,[1.0,1.0],0.0,REFERENCE_EF,range*au,true),passive);
            assert_eq!(ship_detection(SensorSuite::FULL,[1.0,1.0],1.0,REFERENCE_EF,range*au,true),active);
        }
        // Typical ECM adds 50% signature but cuts resolution distance by half.
        let ef=REFERENCE_EF*1.5;
        for (range,expected) in [(0.749,Identity),(0.751,Approximate)] {
            assert_eq!(ship_detection_ew(SensorSuite::FULL,[1.0,1.0],1.0,ef,range*au,true,resolution_factor(100.0,50.0)),expected);
        }
    }
    #[test] fn ef_ranges_activity_damage_and_ping_identity() {
        use DetectionLevel::*;
        let cold=EmissivityFactors {visibility_multiplier:1.0,thrust_percent:0.0,screen_percent:0.0,screen_on:false,size:7.0,stealth:50.0,ecm_on:false,recent_missiles:false,recent_beams:false};
        assert!((cold.value()-0.35).abs()<1e-12);assert!(!cold.direction_active());
        assert!(!EmissivityFactors {ecm_on:true,..cold}.direction_active());
        let hot=EmissivityFactors {thrust_percent:100.0,screen_percent:100.0,screen_on:true,ecm_on:true,recent_missiles:true,recent_beams:true,..cold};
        assert!((hot.value()-41.58).abs()<1e-9);
        let au=crate::units::AU;
        for (range,expected) in [(0.005,Identity),(0.08,Resolved),(1.0,Approximate),(5.0,Bearing),(11.0,None)] {
            assert_eq!(ship_detection(SensorSuite::FULL,[1.0,1.0],0.0,REFERENCE_EF,range*au,true),expected);
        }
        assert_eq!(ship_detection(SensorSuite::FULL,[1.0,1.0],0.0,REFERENCE_EF,5.0*au,false),None);
        assert_eq!(ship_detection(SensorSuite::FULL,[1.0,0.5],0.0,REFERENCE_EF,6.0*au,true),None);
        assert_eq!(ship_detection(SensorSuite::FULL,[1.0,1.0],1.0,REFERENCE_EF,0.9*au,false),Identity);
        assert_eq!(ship_detection(SensorSuite::FULL,[0.0,0.0],0.5,REFERENCE_EF,0.6*au,true),None);
        assert_eq!(resolution_factor(100.0,50.0),0.5);
        assert_eq!(resolution_factor(50.0,100.0),1.0);
        assert_eq!(resolution_factor(1e6,0.0),0.5);
        assert_eq!(ship_detection_ew(SensorSuite::FULL,[1.0,1.0],0.0,REFERENCE_EF,0.08*au,true,0.5),Approximate);
        assert_eq!(ship_detection_ew(SensorSuite::FULL,[1.0,1.0],0.0,REFERENCE_EF,5.0*au,true,0.5),Bearing);
    }
    #[test]
    fn accelerating_seeker_fits_current_not_average_velocity() {
        let mut fix=None;
        for i in 0..32 {
            let t=i as f64;
            fix=Some(SeekerFix::update(fix,t,Vec2::new(1e9+3.0*t+2.0*t*t,5.0*t),Vec2::ZERO));
        }
        let fit=fix.unwrap().accelerating();
        assert!((fit.vel-Vec2::new(127.0,5.0)).length()<1e-6);
        assert!((fit.pos-Vec2::new(1e9+3.0*31.0+2.0*31.0*31.0,155.0)).length()<1e-6);
    }
    #[test]
    fn platform_signature_adds_baseline_actual_thrust_and_screen_radiation() {
        let cold=platform_emission_w(false,1.0,Vec2::ZERO,0.0);
        assert_eq!(platform_emission_w(false,0.5,Vec2::ZERO,0.0),cold*0.5);
        assert_eq!(platform_emission_w(false,2.0,Vec2::ZERO,0.0),cold*2.0);
        let burn=Vec2::new(3.0*G0,4.0*G0);
        assert_eq!(platform_emission_w(false,0.5,burn,0.0),cold*0.5+5.0*SIGNATURE_DRIVE_W_PER_G.value);
        let mut thermal=crate::thermal::Thermal::default();
        let mut screen=0.0;
        thermal.advance(60.0,true,&mut screen);
        assert_eq!(thermal.emission(screen),0.0,"raising an unhit screen adds no ambient heat");
        screen=crate::params::SCREEN_CAPACITY_J.value*0.5;
        thermal.captured_j+=screen;
        assert!(thermal.emission(screen)>0.0,"absorbed hit energy radiates");
        assert!(platform_emission_w(false,0.5,Vec2::ZERO,thermal.emission(screen))>cold*0.5);
        assert!(platform_emission_w(false,0.5,burn,thermal.emission(screen))>platform_emission_w(false,0.5,Vec2::ZERO,thermal.emission(screen)));
    }

    #[test]
    fn installed_channels_are_independent_and_missiles_have_no_df() {
        let mut rng=Rng::new(42);
        let range=1_000_000.0;
        let weak=PASSIVE_NOISE_FLOOR.value/intensity(1.0,range)*0.01;
        for _ in 0..100 {
            assert!(receive_measurement(SensorSuite::MISSILE,weak,range,0.0,&mut rng).is_none());
            assert!(receive_measurement(SensorSuite {passive:false,active:true,direction_finding:false},weak*1e12,range,0.0,&mut rng).is_none());
        }
        assert!((0..100).any(|_| matches!(receive_measurement(SensorSuite {passive:false,active:false,direction_finding:true},weak,range,0.0,&mut rng),Some((Measurement::Bearing {..},_)))));
        assert!((0..100).any(|_| matches!(receive_measurement(SensorSuite::MISSILE,weak*1e6,range,0.0,&mut rng),Some((Measurement::BearingRange {..},_)))));
    }

    #[test]
    fn local_seeker_estimates_velocity_from_its_own_measurements() {
        let mut fix=None;
        for i in 0..32 {
            let t=i as f64*0.1;
            fix=Some(SeekerFix::update(fix,t,Vec2::new(1000.0+2.0*t,10.0-t),Vec2::ZERO));
        }
        assert!((fix.unwrap().vel-Vec2::new(2.0,-1.0)).length()<1e-8);
    }
    use crate::units::AU;

    fn passive_snr(accel_g: f64, range_km: f64) -> f64 {
        intensity(ship_emission_w(Vec2::new(accel_g * G0, 0.0)), range_km) / PASSIVE_NOISE_FLOOR.value
    }

    #[test]
    fn cold_ships_hide_and_burning_ships_shine() {
        // The placeholder calibration is meant to give these qualitative results.
        assert!(passive_snr(0.0, 0.1 * AU) < PASSIVE_DETECT_SNR.value);
        assert!((passive_snr(10.0, 0.1 * AU) - 9.0).abs() < 1e-9);
    }

    #[test]
    fn detection_range_is_where_snr_hits_threshold() {
        let p = ship_emission_w(Vec2::new(G0, 0.0));
        let r = passive_detection_range_km(p);
        let snr = intensity(p, r) / PASSIVE_NOISE_FLOOR.value;
        assert!((snr / PASSIVE_DETECT_SNR.value - 1.0).abs() < 1e-9);
    }

    #[test]
    fn detection_is_even_odds_at_threshold_with_a_long_tail() {
        let mut rng = Rng::new(7);
        let rate = |snr: f64, rng: &mut Rng| (0..20_000).filter(|_| detected(snr, rng)).count() as f64 / 20_000.0;
        let t = PASSIVE_DETECT_SNR.value;
        assert!((rate(t, &mut rng) - 0.5).abs() < 0.02);
        // Ten times the 50 % range is a hundredth of the SNR: still about 1 % a frame.
        assert!((rate(t / 100.0, &mut rng) - 1.0 / 101.0).abs() < 0.003);
    }

    #[test]
    fn echoes_have_a_good_chance_at_one_au() {
        let snr = |r: f64| echo_intensity(ACTIVE_PING_POWER_W.value, r, r) / ACTIVE_NOISE_FLOOR.value;
        assert!((snr(AU) - 9.0).abs() < 1e-9);
        assert!(echo_resolvable(snr(0.5*AU)));
        assert!(!echo_resolvable(snr(0.501*AU)));
        assert!(!echo_resolvable(snr(AU)));
        assert!(!echo_resolvable(snr(1.01*AU)));
        assert!(!echo_resolvable(snr(2.0*AU)));
        assert!(snr(3.0 * AU) < 1.0);
    }

    #[test]
    fn passive_lock_direction_and_pinging_ranges() {
        let power = ship_emission_w(Vec2::new(10.0 * G0, 0.0));
        for boost in [1.0, ACTIVE_EXPOSURE_RANGE.value] {
            let mut rng = Rng::new(81);
            let mut locks = 0;
            let mut directions = 0;
            for _ in 0..10_000 {
                if matches!(passive_measurement(power * boost * boost, 0.1 * AU * boost, 0.0, &mut rng), Some((Measurement::BearingRange { .. }, _))) {
                    locks += 1;
                }
                match passive_measurement(power * boost * boost, 10.0 * AU * boost, 0.0, &mut rng) {
                    Some((Measurement::Bearing { .. }, _)) => directions += 1,
                    Some((Measurement::BearingRange { .. }, _)) => panic!("direction finding must not invent a range"),
                    None => {}
                }
            }
            assert!((7000..8000).contains(&locks), "{locks}");
            assert!((7000..8000).contains(&directions), "{directions}");
        }
    }

    #[test]
    fn wrap_angle_stays_in_range() {
        assert!((wrap_angle(3.0 * PI) - PI).abs() < 1e-12 || (wrap_angle(3.0 * PI) + PI).abs() < 1e-12);
        assert!((wrap_angle(0.5) - 0.5).abs() < 1e-12);
    }
}
