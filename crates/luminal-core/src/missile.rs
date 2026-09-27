//! Missiles as space torpedoes (GAME_MECHANICS.md §6): burn, cruise, terminal.
//!
//! Pure guidance. The world feeds each missile what it may know: the launching
//! faction's causally delivered track during burn and cruise, and its
//! own seeker's bearings in terminal. Hits are resolved against truth by the world.

use crate::kinematics::{State, Vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Payload {
    /// Direct ship-beam damage only; not a launchable missile or magazine slot.
    Beam,
    /// Near-contact nuclear burst.
    Nuclear,
    /// Physical impact.
    Kinetic,
}

impl Payload {
    pub const ALL: [Payload; 2] = [Payload::Kinetic, Payload::Nuclear];

    /// Magazine slot, in the same order as ALL.
    pub const fn index(self) -> usize {
        match self { Payload::Kinetic => 0, Payload::Nuclear => 1, Payload::Beam => panic!("ship beams have no magazine slot") }
    }

    pub fn name(self) -> &'static str {
        match self {
            Payload::Beam => "beam",
            Payload::Nuclear => "LRM",
            Payload::Kinetic => "SRM",
        }
    }

    pub fn delta_v(self)->f64 {
        crate::params::MISSILE_DELTA_V_KMS.value * if self==Self::Kinetic {0.5} else {1.0}
    }
    pub fn acceleration_g(self)->f64 {crate::params::MISSILE_MAX_ACCEL_G.value*if self==Self::Kinetic {2.0} else {1.0}}
    pub fn launch_interval(self)->f64 {if self==Self::Kinetic {crate::params::SRM_LAUNCH_INTERVAL_S.value} else {crate::params::MISSILE_LAUNCH_INTERVAL_S.value}}
    pub fn endurance(self)->f64 {if self==Self::Kinetic {4500.0} else {21600.0}}
    pub fn engagement_range(self)->f64 {crate::units::AU*if self==Self::Kinetic {0.1} else {2.0}}
}

/// Fixed limited field of regard about a received aim, never the true bearing.
pub fn in_search_cone(heading:Vec2,relative:Vec2)->bool {
    heading.length()>0.0 && relative.length()>0.0 && heading.normalized().dot(relative.normalized())>=crate::params::MISSILE_SEARCH_HALF_ANGLE.cos()
}

/// AI opportunity score: predicted position uncertainty must fit both the
/// seeker's search footprint and the remaining lateral correction budget.
pub fn launch_confidence(range:f64,sigma:f64,velocity_sigma:f64,payload:Payload)->f64 {
    let speed=(payload.delta_v()*crate::params::MISSILE_BURN_FRACTION.value).max(1.0);
    let accel=payload.acceleration_g()*crate::units::G0;
    let eta=range/speed+speed/(2.0*accel);
    let uncertainty=(sigma*sigma+(velocity_sigma*eta).powi(2)
        +(0.5*crate::params::TRACK_MANEUVER_G.value*crate::units::G0*ACCEL_PERSIST_S*eta).powi(2)).sqrt();
    let acquisition=crate::params::MISSILE_ACTIVE_RANGE_LS*crate::units::LIGHT_SECOND;
    let time=(acquisition/speed).min(crate::params::MISSILE_TERMINAL_S.value);
    let reach=lateral_reach(accel,
        payload.delta_v()*crate::params::MISSILE_RESERVE_FRACTION.value,time);
    let footprint=acquisition*crate::params::MISSILE_SEARCH_HALF_ANGLE.tan();
    (reach.min(footprint)/uncertainty.max(1.0)).min(1.0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Burn,
    Cruise,
    Terminal,
}

/// Relativistic kinetic energy, J, of `mass_kg` at `speed_kms`: (γ − 1)mc².
pub fn kinetic_energy_j(mass_kg: f64, speed_kms: f64) -> f64 {
    let b2 = (speed_kms / crate::units::C).powi(2).min(1.0 - 1e-12);
    let r = (1.0 - b2).sqrt();
    // γ − 1 without cancellation at low speed.
    let gamma_m1 = b2 / (r * (1.0 + r));
    mass_kg * (crate::units::C * 1e3).powi(2) * gamma_m1
}

/// Proportional-navigation gain.
const PN_GAIN: f64 = 4.0;

/// How long a target's observed acceleration is assumed to persist, s. Manoeuvres are
/// not predictable beyond about one of them; projecting a jink over hours sends a
/// missile chasing phantoms until its fuel is gone.
pub const ACCEL_PERSIST_S: f64 = 60.0;

/// Upper bound on lateral displacement with remaining thrust time and delta-v.
/// It is not permission to teleport onto the firing solution.
pub fn lateral_reach(accel: f64, delta_v: f64, time_left: f64) -> f64 {
    if accel<=0.0 || time_left<=0.0 { return 0.0; }
    let burn=(delta_v.max(0.0)/accel).min(time_left);
    accel*burn*(time_left-0.5*burn)
}

/// Coasting encounter geometry: time to closest approach and the miss vector there,
/// with the target's acceleration held for at most `ACCEL_PERSIST_S`.
pub fn zero_effort_miss(ship: State, target: State, target_accel: Vec2) -> (f64, Vec2) {
    let p = target.pos - ship.pos;
    let v = target.vel - ship.vel;
    let vv = v.dot(v);
    let t_go = if vv > 0.0 { (-p.dot(v) / vv).max(0.0) } else { 0.0 };
    let h = t_go.min(ACCEL_PERSIST_S);
    (t_go, p + v * t_go + target_accel * (0.5 * h * h + h * (t_go - h)))
}

/// Cruise correction: null the zero-effort miss by the encounter (augmented PN form).
pub fn cruise_correction(t_go: f64, zem: Vec2) -> Vec2 {
    if t_go <= 0.0 { Vec2::ZERO } else { zem * (3.0 / (t_go * t_go)) }
}

/// Constant-thrust intercept: the aim that reaches the target's predicted position
/// soonest at `accel`. Returns the thrust direction and time, or `None` if the target
/// cannot be caught within `horizon`.
pub fn intercept_aim(ship: State, target: State, target_accel: Vec2, accel: f64, horizon: f64) -> Option<(Vec2, f64)> {
    let dp = target.pos - ship.pos;
    let dv = target.vel - ship.vel;
    let miss = |t: f64| dp + dv * t + target_accel * (0.5 * t * t);
    let f = |t: f64| 0.5 * accel * t * t - miss(t).length();
    let (mut lo, mut hi) = (0.0, 0.1);
    while f(hi) < 0.0 {
        lo = hi;
        hi *= 1.25;
        if hi > horizon {
            return None;
        }
    }
    for _ in 0..100 {
        let mid = 0.5 * (lo + hi);
        if f(mid) < 0.0 { lo = mid } else { hi = mid }
    }
    Some((miss(hi).normalized(), hi))
}

/// Samples a seeker keeps for fitting its line-of-sight rate.
const SEEKER_HISTORY: usize = 32;

/// Seeker line-of-sight tracker: bearing and its rate, from successive bearings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seeker {
    pub t: f64,
    pub bearing: f64,
    pub rate: f64,
    pub samples: u32,
    history: [(f64, f64); SEEKER_HISTORY],
}

impl Seeker {
    pub fn update(prev: Option<Seeker>, t: f64, bearing: f64) -> Seeker {
        let mut history = prev.map_or([(f64::NEG_INFINITY, 0.0); SEEKER_HISTORY], |p| p.history);
        history.rotate_right(1);
        history[0] = (t, bearing);
        match prev {
            Some(p) if t > p.t => {
                let raw = crate::sensors::wrap_angle(bearing - p.bearing) / (t - p.t);
                // Light smoothing for steering; `fit_rate` gives a steadier value.
                let rate = if p.samples >= 2 { 0.3 * p.rate + 0.7 * raw } else { raw };
                Seeker { t, bearing, rate, samples: p.samples + 1, history }
            }
            _ => Seeker { t, bearing, rate: 0.0, samples: 1, history },
        }
    }

    /// The recent bearing samples, newest first, as (time, bearing).
    pub fn samples_within(&self, window: f64) -> impl Iterator<Item = (f64, f64)> + '_ {
        self.history.iter().copied().filter(move |(t, _)| self.t - t <= window)
    }

    /// Least-squares line-of-sight rate over the last `window` seconds, which averages
    /// out pointing jitter. `None` with fewer than three samples in the window.
    pub fn fit_rate(&self, window: f64) -> Option<f64> {
        let pts: Vec<(f64, f64)> = self
            .history
            .iter()
            .filter(|(t, _)| self.t - t <= window)
            .map(|(t, b)| (t - self.t, crate::sensors::wrap_angle(b - self.bearing)))
            .collect();
        if pts.len() < 3 {
            return None;
        }
        let n = pts.len() as f64;
        let (mt, mb) = (pts.iter().map(|p| p.0).sum::<f64>() / n, pts.iter().map(|p| p.1).sum::<f64>() / n);
        let (num, den) = pts.iter().fold((0.0, 0.0), |(a, b), (t, y)| (a + (t - mt) * (y - mb), b + (t - mt) * (t - mt)));
        (den > 0.0).then(|| num / den)
    }
}

/// Window over which terminal guidance fits the line-of-sight rate, s.
const PN_RATE_WINDOW_S: f64 = 0.2;

/// Terminal proportional navigation from line-of-sight rate and closing speed.
pub fn pn_accel(seeker: &Seeker, closing_speed: f64) -> Vec2 {
    let n = Vec2::new(-seeker.bearing.sin(), seeker.bearing.cos());
    let rate = seeker.fit_rate(PN_RATE_WINDOW_S).unwrap_or(seeker.rate);
    n * (PN_GAIN * closing_speed.max(0.0) * rate)
}

/// Least-squares straight-line fit of timed positions: the position at the newest time
/// and the velocity. `None` with fewer than three points.
pub fn fit_track(points: &[(f64, Vec2)]) -> Option<(Vec2, Vec2)> {
    if points.len() < 3 {
        return None;
    }
    let n = points.len() as f64;
    let t_new = points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
    let mt = points.iter().map(|p| p.0 - t_new).sum::<f64>() / n;
    let mp = points.iter().fold(Vec2::ZERO, |a, p| a + p.1) * (1.0 / n);
    let (mut num, mut den) = (Vec2::ZERO, 0.0);
    for p in points {
        let dt = p.0 - t_new - mt;
        num = num + (p.1 - mp) * dt;
        den += dt * dt;
    }
    if den <= 0.0 {
        return None;
    }
    let vel = num * (1.0 / den);
    Some((mp - vel * mt, vel))
}

/// Closest approach between two trajectories' states over `[t0, t1]`, searched by
/// sampling then golden-section refinement. `f` gives the separation vector at `t`.
pub fn closest_approach(t0: f64, t1: f64, f: impl Fn(f64) -> Option<Vec2>) -> Option<(f64, f64)> {
    const N: usize = 32;
    let mut best: Option<(f64, f64)> = None;
    for k in 0..=N {
        let t = t0 + (t1 - t0) * k as f64 / N as f64;
        if let Some(d) = f(t).map(|v| v.length())
            && best.is_none_or(|(_, bd)| d < bd)
        {
            best = Some((t, d));
        }
    }
    let (tb, _) = best?;
    let h = (t1 - t0) / N as f64;
    let (mut a, mut b) = ((tb - h).max(t0), (tb + h).min(t1));
    let g = 0.618_033_988_75;
    for _ in 0..60 {
        let (c, d) = (b - g * (b - a), a + g * (b - a));
        let (fc, fd) = (f(c).map_or(f64::INFINITY, |v| v.length()), f(d).map_or(f64::INFINITY, |v| v.length()));
        if fc < fd { b = d } else { a = c }
    }
    let t = 0.5 * (a + b);
    f(t).map(|v| (t, v.length()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn srm_has_double_acceleration_and_half_lrm_fuel() {
        assert_eq!(super::Payload::Kinetic.acceleration_g(),2.0*super::Payload::Nuclear.acceleration_g());
        assert_eq!(super::Payload::Kinetic.delta_v(),0.5*super::Payload::Nuclear.delta_v());
    }
    use super::*;
    #[test]
    fn high_closure_and_low_fuel_limit_sideways_correction() {
        let a=1500.0*crate::units::G0;
        assert!(lateral_reach(a,60_000.0,1.0)<8.0);
        assert!(lateral_reach(a,60_000.0,10.0)>700.0);
        assert!(lateral_reach(a,1.0,10.0)<10.0);
        assert_eq!(lateral_reach(a,0.0,10.0),0.0);
    }

    #[test]
    fn kinetic_energy_matches_the_design_table() {
        // GAME_MECHANICS §6: 100 kg at 0.1c is about 45 PJ; at 0.01c, 0.45 PJ.
        let c = crate::units::C;
        assert!((kinetic_energy_j(100.0, 0.1 * c) / 4.5e16 - 1.0).abs() < 0.02);
        assert!((kinetic_energy_j(100.0, 0.01 * c) / 4.5e14 - 1.0).abs() < 0.01);
    }

    #[test]
    fn zem_of_a_head_on_pass_is_the_offset() {
        let m = State { pos: Vec2::ZERO, vel: Vec2::new(100.0, 0.0) };
        let t = State { pos: Vec2::new(1000.0, 5.0), vel: Vec2::ZERO };
        let (t_go, zem) = zero_effort_miss(m, t, Vec2::ZERO);
        assert!((t_go - 10.0).abs() < 1e-9);
        assert!((zem - Vec2::new(0.0, 5.0)).length() < 1e-9);
    }

    #[test]
    fn closest_approach_finds_the_pass() {
        let f = |t: f64| Some(Vec2::new(1000.0 - 100.0 * t, 3.0));
        let (t, d) = closest_approach(0.0, 20.0, f).unwrap();
        assert!((t - 10.0).abs() < 1e-6 && (d - 3.0).abs() < 1e-6);
    }

    #[test]
    fn seeker_rate_follows_a_rotating_line_of_sight() {
        let mut s = None;
        for k in 0..10 {
            s = Some(Seeker::update(s, k as f64, 0.01 * k as f64));
        }
        assert!((s.unwrap().rate - 0.01).abs() < 1e-9);
        assert!((s.unwrap().fit_rate(5.0).unwrap() - 0.01).abs() < 1e-9);
    }
}
