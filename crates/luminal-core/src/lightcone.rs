//! Retarded-time queries: what light from a source is arriving at a point now.

use crate::kinematics::{State, Trajectory, Vec2};
use crate::units::C;

/// Safeguarded Newton solve of an increasing light-cone residual. Velocity gives
/// the exact local derivative; the bracket preserves robustness at thrust changes.
fn light_root(mut lo:f64,mut hi:f64,mut sample:impl FnMut(f64)->Option<(f64,f64)>)->Option<f64> {
    let mut at=(lo+hi)*0.5;
    for _ in 0..100 {
        let (error,slope)=sample(at)?;
        if error>=0.0 {hi=at;} else {lo=at;}
        if hi-lo<1e-9 {return Some(hi);}
        if error.abs()/slope.max(1e-12)<1e-10 {return Some(at);}
        let newton=at-error/slope;
        at=if newton.is_finite() && newton>lo && newton<hi {newton} else {(lo+hi)*0.5};
    }
    Some(hi)
}

/// The emission instant whose light reaches `receiver` at `t_receive`, and the source
/// state at that instant. `None` if that light was emitted before the source existed.
///
/// Solves `|receiver − x_src(t_e)| = c (t_receive − t_e)`. The residual is strictly
/// monotonic for subluminal sources, allowing bracketed Newton iteration with
/// bisection fallback at thrust discontinuities.
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

    let t_e=light_root(lo,hi,|at| {
        let s=source.state_at(at)?;
        let rel=receiver-s.pos;
        let range=rel.length();
        let radial=if range>0.0 {rel.dot(s.vel)/range} else {0.0};
        Some((range-C*(t_receive-at),C-radial))
    })?;
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
        let lo = lo.max(self.t_emit).max(traj.start());
        if f(lo)? >= 0.0 {
            return Some(lo);
        }
        light_root(lo,hi,|at| {
            let s=traj.state_at(at)?;
            let rel=s.pos-self.origin;
            let range=rel.length();
            let radial=if range>0.0 {rel.dot(s.vel)/range} else {0.0};
            Some((self.radius_at(at)-range,C-radial))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accelerated_light_roots_match_reference_bisection() {
        let mut trajectory=Trajectory::new(0.0,State {pos:Vec2::new(C,2.0*C),vel:Vec2::new(0.8*C,0.1*C)});
        trajectory.set_thrust(1.0,Vec2::new(-100.0,20.0)).unwrap();
        trajectory.set_thrust(30.0,Vec2::new(30.0,-40.0)).unwrap();
        let front=Front {origin:Vec2::ZERO,t_emit:2.0};
        let arrival=front.arrival(&trajectory,2.0,100.0).unwrap();
        let (mut lo,mut hi)=(2.0,100.0);
        for _ in 0..100 {let m=(lo+hi)*0.5;if front.radius_at(m)>=trajectory.state_at(m).unwrap().pos.length() {hi=m;} else {lo=m;}}
        assert!((arrival-hi).abs()<2e-8);
        for received in [20.0,40.0,80.0] {
            let (emitted,_)=retarded_state(&trajectory,Vec2::ZERO,received).unwrap();
            let (mut lo,mut hi)=(0.0,received);
            for _ in 0..100 {let m=(lo+hi)*0.5;if C*(received-m)>=trajectory.state_at(m).unwrap().pos.length() {lo=m;} else {hi=m;}}
            assert!((emitted-(lo+hi)*0.5).abs()<2e-8);
        }
    }

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
