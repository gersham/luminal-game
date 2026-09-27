//! Retarded-time queries: what light from a source is arriving at a point now.

use crate::kinematics::{State, Trajectory, Vec2};
use crate::units::C;

/// The emission instant whose light reaches `receiver` at `t_receive`, and the source
/// state at that instant. `None` if that light was emitted before the source existed.
///
/// Solves `|receiver − x_src(t_e)| = c (t_receive − t_e)`. The residual is strictly
/// monotonic for subluminal sources, so bisection on a bracket is robust.
pub fn retarded_state(source: &Trajectory, receiver: Vec2, t_receive: f64) -> Option<(f64, State)> {
    let residual = |t_e: f64| {
        let s = source.state_at(t_e).expect("t_e within bracket");
        C * (t_receive - t_e) - (receiver - s.pos).length()
    };

    // A destroyed source emits nothing after its end.
    let hi = source.end().map_or(t_receive, |e| e.min(t_receive));
    if hi < source.start() || residual(hi) > 0.0 {
        return None;
    }
    // Residual ≤ 0 at hi; step back until it becomes ≥ 0.
    let mut span = (receiver - source.state_at(hi)?.pos).length() / C;
    let mut lo = hi - span;
    loop {
        if lo < source.start() {
            lo = source.start();
            if residual(lo) < 0.0 {
                return None;
            }
            break;
        }
        if residual(lo) >= 0.0 {
            break;
        }
        span = span.max(1e-9) * 2.0;
        lo = hi - span;
    }

    let (mut lo, mut hi) = (lo, hi);
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if residual(mid) >= 0.0 { lo = mid } else { hi = mid }
        if hi - lo < 1e-9 {
            break;
        }
    }
    let t_e = 0.5 * (lo + hi);
    Some((t_e, source.state_at(t_e)?))
}

/// A spherical light-front: something emitted at `origin` at time `t_emit`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Front {
    pub origin: Vec2,
    pub t_emit: f64,
}

impl Front {
    pub fn radius_at(&self, t: f64) -> f64 {
        C * (t - self.t_emit)
    }

    /// When this front first reaches a receiver on `traj`, searched within `(lo, hi]`,
    /// using only committed motion up to `hi`. `None` if it has not arrived by `hi`.
    ///
    /// `c (t − t_e) − |x(t) − origin|` is strictly increasing for a subluminal receiver,
    /// so the crossing is unique and bisection is exact to tolerance.
    pub fn arrival(&self, traj: &Trajectory, lo: f64, hi: f64) -> Option<f64> {
        let f = |t: f64| traj.state_at(t).map(|s| self.radius_at(t) - (s.pos - self.origin).length());
        let hi = traj.end().map_or(hi, |e| e.min(hi));
        if f(hi)? < 0.0 {
            return None;
        }
        let mut lo = lo.max(self.t_emit).max(traj.start());
        let mut hi = hi;
        if f(lo)? >= 0.0 {
            return Some(lo);
        }
        for _ in 0..200 {
            let mid = 0.5 * (lo + hi);
            if f(mid)? >= 0.0 { hi = mid } else { lo = mid }
            if hi - lo < 1e-9 {
                break;
            }
        }
        Some(hi)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn front_reaches_receiver_after_light_travel_time() {
        let rx = at_rest(4.0 * C);
        let front = Front { origin: Vec2::ZERO, t_emit: 1.0 };
        assert!(front.arrival(&rx, 0.0, 4.9).is_none());
        let t = front.arrival(&rx, 0.0, 10.0).unwrap();
        assert!((t - 5.0).abs() < 1e-6, "{t}");
    }

    fn at_rest(x: f64) -> Trajectory {
        Trajectory::new(0.0, State { pos: Vec2::new(x, 0.0), vel: Vec2::ZERO })
    }

    #[test]
    fn stationary_source_is_seen_one_light_travel_time_late() {
        let src = at_rest(3.0 * C);
        let (t_e, _) = retarded_state(&src, Vec2::ZERO, 10.0).unwrap();
        assert!((t_e - 7.0).abs() < 1e-6);
    }

    #[test]
    fn light_not_yet_arrived_is_invisible() {
        let src = at_rest(3.0 * C);
        assert!(retarded_state(&src, Vec2::ZERO, 2.0).is_none());
    }

    #[test]
    fn destroyed_source_stays_visible_until_its_last_light_passes() {
        let mut src = at_rest(3.0 * C);
        src.terminate(5.0);
        assert!(retarded_state(&src, Vec2::ZERO, 7.5).is_some());
        assert!(retarded_state(&src, Vec2::ZERO, 8.5).is_none());
    }

    #[test]
    fn moving_source_residual_is_zero() {
        let src = Trajectory::new(
            0.0,
            State { pos: Vec2::new(5.0 * C, 0.0), vel: Vec2::new(-0.1 * C, 0.05 * C) },
        );
        let (t_e, s) = retarded_state(&src, Vec2::ZERO, 20.0).unwrap();
        let err = C * (20.0 - t_e) - s.pos.length();
        assert!(err.abs() < 1e-3, "{err}");
    }
}
