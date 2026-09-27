//! Missiles as space torpedoes (GAME_MECHANICS.md §6): burn, cruise, terminal.
//!
//! Pure guidance. The world feeds each missile what it may know: the launching
//! faction's track during burn and cruise (PLACEHOLDER: an instant datalink), and its
//! own seeker's bearings in terminal. Hits are resolved against truth by the world.

use crate::kinematics::{State, Vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Payload {
    /// One-shot nuclear-pumped laser, fired from standoff range.
    Laser,
    /// Near-contact nuclear burst.
    Nuclear,
    /// Physical impact.
    Kinetic,
}

impl Payload {
    pub const ALL: [Payload; 3] = [Payload::Kinetic, Payload::Nuclear, Payload::Laser];

    pub fn name(self) -> &'static str {
        match self {
            Payload::Laser => "laser",
            Payload::Nuclear => "nuclear",
            Payload::Kinetic => "kinetic",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Burn,
    Cruise,
    Terminal,
}

/// Proportional-navigation gain.
const PN_GAIN: f64 = 4.0;

/// Coasting encounter geometry: time to closest approach and the miss vector there.
pub fn zero_effort_miss(ship: State, target: State, target_accel: Vec2) -> (f64, Vec2) {
    let p = target.pos - ship.pos;
    let v = target.vel - ship.vel;
    let vv = v.dot(v);
    let t_go = if vv > 0.0 { (-p.dot(v) / vv).max(0.0) } else { 0.0 };
    (t_go, p + v * t_go + target_accel * (0.5 * t_go * t_go))
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

/// Seeker line-of-sight tracker: bearing and its rate, from successive bearings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seeker {
    pub t: f64,
    pub bearing: f64,
    pub rate: f64,
    pub samples: u32,
}

impl Seeker {
    pub fn update(prev: Option<Seeker>, t: f64, bearing: f64) -> Seeker {
        match prev {
            Some(p) if t > p.t => {
                let raw = crate::sensors::wrap_angle(bearing - p.bearing) / (t - p.t);
                // Light smoothing; the seeker is precise, so trust new data heavily.
                let rate = if p.samples >= 2 { 0.3 * p.rate + 0.7 * raw } else { raw };
                Seeker { t, bearing, rate, samples: p.samples + 1 }
            }
            _ => Seeker { t, bearing, rate: 0.0, samples: 1 },
        }
    }
}

/// Terminal proportional navigation from line-of-sight rate and closing speed.
pub fn pn_accel(seeker: &Seeker, closing_speed: f64) -> Vec2 {
    let n = Vec2::new(-seeker.bearing.sin(), seeker.bearing.cos());
    n * (PN_GAIN * closing_speed.max(0.0) * seeker.rate)
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
    use super::*;

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
    }
}
