//! Committed piecewise constant-acceleration trajectories in the system frame.

use crate::units::C;
use std::ops::{Add, Mul, Neg, Sub};

pub const MAX_SPEED: f64 = 0.99 * C;

fn limit_velocity(v: Vec2) -> Vec2 {
    let speed=v.length();
    if speed>MAX_SPEED {v*(MAX_SPEED/speed)} else {v}
}

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

/// Relativistic motion (PROPOSAL). Drives and gravity change a body's proper velocity
/// `u = γv` at a constant rate in system time; ordinary velocity is `u / sqrt(1 + u²/c²)`,
/// with a gameplay cap of 0.99c applied by `advance`. At everyday speeds this is Newtonian.
pub fn proper_velocity(vel: Vec2) -> Vec2 {
    let b2 = (vel.dot(vel) / (C * C)).min(1.0 - 1e-15);
    vel * (1.0 / (1.0 - b2).sqrt())
}

pub fn velocity_from_proper(u: Vec2) -> Vec2 {
    u * (1.0 / (1.0 + u.dot(u) / (C * C)).sqrt())
}

/// Lorentz factor of a velocity.
pub fn gamma(vel: Vec2) -> f64 {
    1.0 / (1.0 - (vel.dot(vel) / (C * C)).min(1.0 - 1e-15)).sqrt()
}

/// Displacement over `tau` seconds starting at proper velocity `u0` with proper
/// velocity changing at `a` (km/s²). Exact.
fn displacement(u0: Vec2, a: Vec2, tau: f64) -> Vec2 {
    let f = a.length();
    if f * tau.abs() < 1e-3 * C {
        // Velocity barely changes: 5-point Gauss–Legendre is exact to far below a metre.
        const X: [f64; 5] = [0.0, -0.538_469_310_105_683, 0.538_469_310_105_683, -0.906_179_845_938_664, 0.906_179_845_938_664];
        const W: [f64; 5] = [0.568_888_888_888_889, 0.478_628_670_499_366, 0.478_628_670_499_366, 0.236_926_885_056_189, 0.236_926_885_056_189];
        let h = 0.5 * tau;
        return (0..5).fold(Vec2::ZERO, |acc, k| acc + velocity_from_proper(u0 + a * (h * (1.0 + X[k]))) * (W[k] * h));
    }
    // ∫ (u0 + a s) / sqrt(1 + |u0 + a s|²/c²) ds in closed form.
    let (b, uu) = (u0.dot(a), u0.dot(u0));
    let f2 = f * f;
    let q = |s: f64| f2 * s * s + 2.0 * b * s + C * C + uu;
    let d = (f2 * (C * C + uu) - b * b).max(1e-300);
    let i1 = |s: f64| ((f2 * s + b) / d.sqrt()).asinh() / f;
    let i2 = |s: f64| q(s).sqrt() / f2 - b / f2 * i1(s);
    (u0 * (i1(tau) - i1(0.0)) + a * (i2(tau) - i2(0.0))) * C
}

/// State after `tau` seconds of constant proper-velocity change `accel` from `s`.
pub fn advance(s: State, accel: Vec2, tau: f64) -> State {
    let vel=limit_velocity(s.vel);
    if tau==0.0 {return State {pos:s.pos,vel};}
    if accel==Vec2::ZERO {return State {pos:s.pos+vel*tau,vel};}
    let sign=tau.signum();
    let dt=tau.abs();
    let a=accel*sign;
    let u0=proper_velocity(vel);
    let cap=proper_velocity(Vec2::new(MAX_SPEED,0.0)).length();
    let f=a.length();
    let u1=u0+a*dt;
    if u1.length()<=cap {
        return State {pos:s.pos+displacement(u0,a,dt)*sign,vel:limit_velocity(velocity_from_proper(u1))};
    }
    // Integrate up to the speed limit, then discard outward acceleration while
    // retaining its turning component. No hidden excess momentum accumulates.
    let dot=u0.dot(a);
    let remaining=(cap*cap-u0.dot(u0)).max(0.0);
    let root=(dot*dot+f*f*remaining).sqrt();
    let hit=if dot>=0.0 {remaining/(root+dot).max(f64::MIN_POSITIVE)} else {(root-dot)/(f*f)};
    let hit=hit.clamp(0.0,dt);
    let n=(u0+a*hit).normalized();
    let axis=a*(1.0/f);
    let q=n.dot(axis).clamp(0.0,1.0);
    let tail=dt-hit;
    let (travel,end)=if q>=1.0-1e-14 {
        (axis*(MAX_SPEED*tail),axis*MAX_SPEED)
    } else {
        // On the limit sphere, q'=|a|(1-q²)/cap, so q=tanh(z).
        let z0=q.atanh();
        let h=f*tail/cap;
        let z1=z0+h;
        let transverse=(n-axis*q).normalized();
        let log_cosh_delta=h+(-2.0*z1).exp().ln_1p()-(-2.0*z0).exp().ln_1p();
        let side=2.0*((-z0).exp().atan()-(-z1).exp().atan());
        let travel=if h<1e-6 {
            let zm=z0+0.5*h;
            (axis*zm.tanh()+transverse*(2.0*(-zm).exp()/(1.0+(-2.0*zm).exp())))*(MAX_SPEED*tail)
        } else {(axis*log_cosh_delta+transverse*side)*(MAX_SPEED*cap/f)};
        let end=(axis*z1.tanh()+transverse*(2.0*(-z1).exp()/(1.0+(-2.0*z1).exp())))*MAX_SPEED;
        (travel,end)
    };
    State {pos:s.pos+(displacement(u0,a,hit)+travel)*sign,vel:limit_velocity(end)}
}

/// Constant acceleration from `t0` until the next segment begins.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    pub t0: f64,
    pub pos: Vec2,
    pub vel: Vec2,
    /// Total rate of change of proper velocity, km/s²: thrust plus sampled gravity.
    pub accel: Vec2,
    /// The thrust part of `accel`, km/s². This is what the drive emits.
    pub thrust: Vec2,
}

impl Segment {
    pub fn state_at(&self, t: f64) -> State {
        advance(State { pos: self.pos, vel: self.vel }, self.accel, t - self.t0)
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
    jump_gaps: Vec<(f64, Option<f64>)>,
}

impl Trajectory {
    pub fn new(t0: f64, initial: State) -> Self {
        Self {
            segments: vec![Segment { t0, pos: initial.pos, vel: limit_velocity(initial.vel), accel: Vec2::ZERO, thrust: Vec2::ZERO }],
            end: None,
            jump_gaps: Vec::new(),
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

    pub fn jump_gaps(&self) -> &[(f64, Option<f64>)] { &self.jump_gaps }

    /// Leave normal space. History remains available to light-delayed observers.
    pub(crate) fn jump_departure(&mut self, t: f64) {
        self.jump_gaps.push((t, None));
    }

    pub(crate) fn jump_arrival(&mut self, t: f64, state: State) {
        self.jump_gaps.last_mut().expect("jump departure").1 = Some(t);
        self.segments.push(Segment {t0:t,pos:state.pos,vel:limit_velocity(state.vel),accel:Vec2::ZERO,thrust:Vec2::ZERO});
    }

    fn segment_at(&self, t: f64) -> Option<&Segment> {
        if self.jump_gaps.iter().any(|(start,end)|t>=*start && end.is_none_or(|end|t<end)) {return None;}

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

    /// Delta-v spent by the drive over `[t0, t1]`, km/s.
    pub fn thrust_impulse(&self, t0: f64, t1: f64) -> f64 {
        let end = self.end.map_or(t1, |e| e.min(t1));
        if end<=t0 {return 0.0;}
        let first=self.segments.partition_point(|s|s.t0<=t0).saturating_sub(1);
        self.segments
            .iter()
            .enumerate()
            .skip(first)
            .take_while(|(_,s)|s.t0<end)
            .map(|(k, s)| {
                let s1 = self.segments.get(k + 1).map_or(end, |n| n.t0).min(end);
                let (a, b) = (s.t0.max(t0), s1);
                s.thrust.length() * (b - a).max(0.0)
            })
            .sum()
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

    /// An abstract weapon flight changes its displayed course, not a physical
    /// thrust integration. Preserve position and all previously emitted history.
    pub(crate) fn weapon_course(&mut self,t:f64,velocity:Vec2,signature_thrust:Vec2) {
        assert!(t>=self.last().t0);
        let pos=self.last().state_at(t).pos;
        let segment=Segment {t0:t,pos,vel:limit_velocity(velocity),accel:Vec2::ZERO,thrust:signature_thrust};
        if t==self.last().t0 {*self.segments.last_mut().unwrap()=segment;} else {self.segments.push(segment);}
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
        // GAME_MECHANICS.md §3 gives ~8.49 h and ~3.06 AU (Newtonian). Relativistic motion
        // is slightly longer: t = γv/a, d = (c²/a)(γ − 1).
        let v = 0.1 * C;
        let a = 100.0 * G0;
        let g = 1.0 / (1.0f64 - 0.01).sqrt();
        let t = g * v / a;
        let mut traj = Trajectory::new(0.0, State { pos: Vec2::ZERO, vel: Vec2::new(v, 0.0) });
        traj.set_thrust(0.0, Vec2::new(-a, 0.0)).unwrap();
        let s = traj.state_at(t).unwrap();
        assert!(s.vel.length() < 1e-6, "{}", s.vel.length());
        assert!((t / 3600.0 - 8.53).abs() < 0.01, "{}", t / 3600.0);
        let d = C * C / a * (g - 1.0);
        assert!((s.pos.x - d).abs() < 1.0, "{} vs {d}", s.pos.x);
        assert!((s.pos.x / AU - 3.09).abs() < 0.01);
    }

    #[test]
    fn a_month_at_100g_stays_below_light_speed() {
        let mut traj = Trajectory::new(0.0, State { pos: Vec2::ZERO, vel: Vec2::ZERO });
        traj.set_thrust(0.0, Vec2::new(100.0 * G0, 0.0)).unwrap();
        let month = 30.0 * 86_400.0;
        let s = traj.state_at(month).unwrap();
        assert!((s.vel.length()-MAX_SPEED).abs()<1e-8, "{}", s.vel.length() / C);
        // Position is continuous and never outruns light.
        assert!(s.pos.x <= MAX_SPEED * month);
    }

    #[test]
    fn speed_cap_turns_consistently_and_brakes_without_windup() {
        let initial=State {pos:Vec2::ZERO,vel:Vec2::new(0.98*C,0.0)};
        let a=Vec2::new(1.0,1.0);
        let duration=4e6;
        let whole=advance(initial,a,duration);
        let mut stepped=initial;
        for _ in 0..1000 {stepped=advance(stepped,a,duration/1000.0);}
        assert!((whole.vel-stepped.vel).length()<1e-6);
        assert!((whole.pos-stepped.pos).length()<10.0);
        assert!(whole.pos.length()<=MAX_SPEED*duration);
        assert!(whole.vel.length()<=MAX_SPEED+1e-9);
        let brake=advance(whole,-whole.vel.normalized(),1000.0);
        assert!(brake.vel.length()<whole.vel.length());
        let coast=advance(State {pos:Vec2::ZERO,vel:Vec2::new(2.0*C,0.0)},Vec2::ZERO,10.0);
        assert_eq!(coast.vel.length(),MAX_SPEED);
        assert_eq!(coast.pos.x,10.0*MAX_SPEED);
    }

    #[test]
    fn slow_motion_is_newtonian() {
        let s = advance(State { pos: Vec2::ZERO, vel: Vec2::new(30.0, 5.0) }, Vec2::new(0.01, -0.02), 60.0);
        let newton = Vec2::new(30.0 * 60.0 + 0.5 * 0.01 * 3600.0, 5.0 * 60.0 - 0.5 * 0.02 * 3600.0);
        assert!((s.pos - newton).length() < 1e-3, "{:?} vs {newton:?}", s.pos);
    }

    #[test]
    fn closed_form_and_quadrature_agree_at_the_switchover() {
        let s0 = State { pos: Vec2::ZERO, vel: Vec2::new(0.5 * C, 0.1 * C) };
        let a = Vec2::new(-3.0, 2.0);
        let tau = 0.99e-3 * C / a.length();
        let q = advance(s0, a, tau);
        let c = advance(s0, a, tau * 1.02);
        // Continuity across the branch: positions differ by ~2 % of the path, smoothly.
        let expected = (c.pos - q.pos).length() / (0.02 * tau);
        assert!((expected - q.vel.length()).abs() / q.vel.length() < 0.02);
    }

    #[test]
    fn turning_thrust_does_not_turn_velocity_instantly() {
        let mut traj = Trajectory::new(0.0, State { pos: Vec2::ZERO, vel: Vec2::new(100.0, 0.0) });
        traj.set_thrust(10.0, Vec2::new(0.0, 1.0)).unwrap();
        let s = traj.state_at(11.0).unwrap();
        assert!((s.vel - Vec2::new(100.0, 1.0)).length() < 1e-6);
        assert!((s.pos - Vec2::new(1100.0, 0.5)).length() < 1e-6);
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
    #[test]
    fn impulse_window_matches_full_history_integration() {
        let mut tr=Trajectory::new(0.0,State {pos:Vec2::ZERO,vel:Vec2::ZERO});
        for i in 0..100 {tr.set_thrust(i as f64,Vec2::new((i%3) as f64,1.0)).unwrap();}
        tr.terminate(99.5);
        for (start,end) in [(-1.0,2.0_f64),(50.0,50.0),(50.2,51.7),(99.0,101.0),(100.0,101.0)] {
            let end=end.min(99.5_f64);
            let reference:f64=tr.segments().iter().enumerate().map(|(i,s)| {
                let stop=tr.segments().get(i+1).map_or(end,|n|n.t0).min(end);
                s.thrust.length()*(stop-s.t0.max(start)).max(0.0)
            }).sum();
            assert_eq!(tr.thrust_impulse(start,end),reference);
        }
    }
}
