//! Committed piecewise constant-acceleration trajectories in the system frame.

use std::ops::{Add, Mul, Neg, Sub};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f64,
    pub y: f64,
}

impl Vec2 {
    pub const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };

    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub fn dot(self, o: Vec2) -> f64 {
        self.x * o.x + self.y * o.y
    }

    pub fn length(self) -> f64 {
        self.dot(self).sqrt()
    }

    pub fn normalized(self) -> Vec2 {
        let l = self.length();
        if l == 0.0 { Vec2::ZERO } else { self * (1.0 / l) }
    }
}

impl Add for Vec2 {
    type Output = Vec2;
    fn add(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x + o.x, self.y + o.y)
    }
}

impl Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x - o.x, self.y - o.y)
    }
}

impl Neg for Vec2 {
    type Output = Vec2;
    fn neg(self) -> Vec2 {
        Vec2::new(-self.x, -self.y)
    }
}

impl Mul<f64> for Vec2 {
    type Output = Vec2;
    fn mul(self, s: f64) -> Vec2 {
        Vec2::new(self.x * s, self.y * s)
    }
}

/// Position (km) and velocity (km/s) at an instant.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct State {
    pub pos: Vec2,
    pub vel: Vec2,
}

/// Constant acceleration from `t0` until the next segment begins.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    pub t0: f64,
    pub pos: Vec2,
    pub vel: Vec2,
    /// Total acceleration, km/s²: thrust plus sampled gravity.
    pub accel: Vec2,
    /// The thrust part of `accel`, km/s². This is what the drive emits.
    pub thrust: Vec2,
}

impl Segment {
    pub fn state_at(&self, t: f64) -> State {
        let dt = t - self.t0;
        State {
            pos: self.pos + self.vel * dt + self.accel * (0.5 * dt * dt),
            vel: self.vel + self.accel * dt,
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum TrajectoryError {
    /// Segments may only be appended at or after the latest segment start.
    RewritesHistory { latest: f64, requested: f64 },
}

/// Append-only history of committed motion. The body does not exist before `start()`
/// or after `end()`.
#[derive(Clone, Debug)]
pub struct Trajectory {
    segments: Vec<Segment>,
    end: Option<f64>,
}

impl Trajectory {
    pub fn new(t0: f64, initial: State) -> Self {
        Self {
            segments: vec![Segment { t0, pos: initial.pos, vel: initial.vel, accel: Vec2::ZERO, thrust: Vec2::ZERO }],
            end: None,
        }
    }

    pub fn start(&self) -> f64 {
        self.segments[0].t0
    }

    pub fn end(&self) -> Option<f64> {
        self.end
    }

    pub fn last(&self) -> &Segment {
        self.segments.last().unwrap()
    }

    /// The body stops existing at `t` (destroyed). Light it emitted earlier still travels.
    pub fn terminate(&mut self, t: f64) {
        self.end = Some(self.end.map_or(t, |e| e.min(t)));
    }

    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    fn segment_at(&self, t: f64) -> Option<&Segment> {
        if t < self.start() || self.end.is_some_and(|e| t > e) {
            return None;
        }
        let i = self.segments.partition_point(|s| s.t0 <= t);
        Some(&self.segments[i - 1])
    }

    /// Exact state at `t`, or `None` before the body existed.
    pub fn state_at(&self, t: f64) -> Option<State> {
        self.segment_at(t).map(|s| s.state_at(t))
    }

    pub fn accel_at(&self, t: f64) -> Option<Vec2> {
        self.segment_at(t).map(|s| s.accel)
    }

    pub fn thrust_at(&self, t: f64) -> Option<Vec2> {
        self.segment_at(t).map(|s| s.thrust)
    }

    /// Change thrust from `t` onward, keeping the current gravity sample. Velocity and
    /// position are continuous; existing motion persists.
    pub fn set_thrust(&mut self, t: f64, thrust: Vec2) -> Result<(), TrajectoryError> {
        let last = *self.last();
        self.push(t, thrust, last.accel - last.thrust)
    }

    /// Begin a new constant-acceleration segment at `t` with the given thrust and
    /// gravity sample.
    pub fn push(&mut self, t: f64, thrust: Vec2, gravity: Vec2) -> Result<(), TrajectoryError> {
        let last = *self.last();
        if t < last.t0 {
            return Err(TrajectoryError::RewritesHistory { latest: last.t0, requested: t });
        }
        let s = last.state_at(t);
        let seg = Segment { t0: t, pos: s.pos, vel: s.vel, accel: thrust + gravity, thrust };
        if t == last.t0 {
            *self.segments.last_mut().unwrap() = seg;
        } else {
            self.segments.push(seg);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::{AU, C, G0};

    #[test]
    fn braking_from_tenth_c_at_100g_matches_reference() {
        // GAME_MECHANICS.md §3: ~8.49 h and ~3.06 AU.
        let v = 0.1 * C;
        let a = 100.0 * G0;
        let t = v / a;
        let mut traj = Trajectory::new(0.0, State { pos: Vec2::ZERO, vel: Vec2::new(v, 0.0) });
        traj.set_thrust(0.0, Vec2::new(-a, 0.0)).unwrap();
        let s = traj.state_at(t).unwrap();
        assert!(s.vel.length() < 1e-6);
        assert!((t / 3600.0 - 8.49).abs() < 0.01);
        assert!((s.pos.x / AU - 3.06).abs() < 0.01);
    }

    #[test]
    fn turning_thrust_does_not_turn_velocity_instantly() {
        let mut traj = Trajectory::new(0.0, State { pos: Vec2::ZERO, vel: Vec2::new(100.0, 0.0) });
        traj.set_thrust(10.0, Vec2::new(0.0, 1.0)).unwrap();
        let s = traj.state_at(11.0).unwrap();
        assert_eq!(s.vel, Vec2::new(100.0, 1.0));
        assert_eq!(s.pos, Vec2::new(1100.0, 0.5));
    }

    #[test]
    fn history_cannot_be_rewritten() {
        let mut traj = Trajectory::new(0.0, State { pos: Vec2::ZERO, vel: Vec2::ZERO });
        traj.set_thrust(5.0, Vec2::new(1.0, 0.0)).unwrap();
        assert!(traj.set_thrust(4.0, Vec2::ZERO).is_err());
        assert!(traj.state_at(-1.0).is_none());
        traj.terminate(8.0);
        assert!(traj.state_at(7.9).is_some());
        assert!(traj.state_at(8.1).is_none());
    }
}
