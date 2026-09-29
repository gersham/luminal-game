//! Stars, planets and moons: fixed snapshots or analytic circular orbits.
//!
//! Their motion is public knowledge, so every faction may use it without any light
//! delay. Their gravity acts on ships; ships do not perturb them. Touching one is fatal.

use crate::kinematics::{State, Vec2};
use std::f64::consts::TAU;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CelestialKind {
    Star,
    Planet,
    Moon,
}

#[derive(Clone, Copy, Debug)]
pub enum Orbit {
    Fixed(Vec2),
    /// Snapshot of a circular orbit. Absolute position is cached once at setup.
    Frozen { parent: usize, radius: f64, pos: Vec2 },
    /// Circular, counter-clockwise about another celestial body (by index).
    Circular { parent: usize, radius: f64, period: f64, phase: f64 },
}

#[derive(Clone, Debug)]
pub struct Celestial {
    pub name: String,
    pub kind: CelestialKind,
    /// Gravitational parameter, km³/s².
    pub gm: f64,
    /// Surface radius, km.
    pub radius: f64,
    pub orbit: Orbit,
}

/// Longest integration step, s. Far from any mass the step is this.
pub const MAX_STEP_S: f64 = 60.0;
/// Shortest integration step, s.
pub const MIN_STEP_S: f64 = 0.5;
/// Step as a fraction of the local free-fall timescale sqrt(r³/GM).
const STEP_FRACTION: f64 = 0.01;

#[derive(Clone, Debug, Default)]
pub struct System {
    /// Parents must precede their satellites.
    pub bodies: Vec<Celestial>,
}

impl System {
    pub fn state(&self, i: usize, t: f64) -> State {
        match self.bodies[i].orbit {
            Orbit::Fixed(pos) | Orbit::Frozen { pos, .. } => State { pos, vel: Vec2::ZERO },
            Orbit::Circular { parent, radius, period, phase } => {
                let p = self.state(parent, t);
                let w = TAU / period;
                let a = phase + w * t;
                let (s, c) = a.sin_cos();
                State { pos: p.pos + Vec2::new(c, s) * radius, vel: p.vel + Vec2::new(-s, c) * (radius * w) }
            }
        }
    }

    /// Acceleration of celestial `i` itself (its orbital motion), km/s².
    pub fn accel(&self, i: usize, t: f64) -> Vec2 {
        let h = 1.0;
        (self.state(i, t + h).vel - self.state(i, t - h).vel) * (0.5 / h)
    }

    /// Sphere-of-influence radius, km: where this body's gravity dominates its parent's.
    pub fn soi_radius(&self, i: usize) -> f64 {
        match self.bodies[i].orbit {
            Orbit::Fixed(_) => f64::INFINITY,
            Orbit::Circular { parent, radius, .. } | Orbit::Frozen { parent, radius, .. } => radius * (self.bodies[i].gm / self.bodies[parent].gm).powf(0.4),
        }
    }

    /// The body whose frame a point naturally belongs to: the smallest sphere of
    /// influence containing it.
    pub fn frame_for(&self, pos: Vec2, t: f64) -> usize {
        (0..self.bodies.len())
            .filter(|&i| (self.state(i, t).pos - pos).length() < self.soi_radius(i))
            .min_by(|&a, &b| self.soi_radius(a).total_cmp(&self.soi_radius(b)))
            .unwrap_or(0)
    }

    /// The body whose surface (with a small margin) contains `pos`, if any.
    pub fn inside(&self, pos: Vec2, t: f64, margin: f64) -> Option<usize> {
        (0..self.bodies.len()).find(|&i| (self.state(i, t).pos - pos).length() < self.bodies[i].radius * margin)
    }

    pub fn gravity(&self, pos: Vec2, t: f64) -> Vec2 {
        self.bodies.iter().enumerate().fold(Vec2::ZERO, |g, (i, b)| {
            let d = self.state(i, t).pos - pos;
            let r = d.length().max(b.radius);
            g + d * (b.gm / (r * r * r))
        })
    }

    /// Integration step suited to the strongest local gravity gradient.
    pub fn step_size(&self, pos: Vec2, t: f64) -> f64 {
        let tau = self
            .bodies
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let r = (self.state(i, t).pos - pos).length().max(b.radius);
                (r * r * r / b.gm).sqrt()
            })
            .fold(f64::INFINITY, f64::min);
        (STEP_FRACTION * tau).clamp(MIN_STEP_S, MAX_STEP_S)
    }

    /// Gravity sampled at the predicted midpoint of a step: second-order accurate.
    pub fn step_gravity(&self, s: State, thrust: Vec2, t: f64, dt: f64) -> Vec2 {
        let g0 = self.gravity(s.pos, t);
        let h = 0.5 * dt;
        let mid = s.pos + s.vel * h + (thrust + g0) * (0.5 * h * h);
        self.gravity(mid, t + h)
    }

    /// First contact with any surface between states `a` at `t0` and `b` at `t1`,
    /// assuming straight-line motion relative to each body over the (short) step.
    pub fn impact(&self, a: Vec2, b: Vec2, t0: f64, t1: f64) -> Option<(usize, f64)> {
        let mut best: Option<(usize, f64)> = None;
        for (i, body) in self.bodies.iter().enumerate() {
            let r0 = a - self.state(i, t0).pos;
            let r1 = b - self.state(i, t1).pos;
            let d = r1 - r0;
            // Solve |r0 + d u| = R for the first u in [0, 1].
            let (qa, qb, qc) = (d.dot(d), 2.0 * r0.dot(d), r0.dot(r0) - body.radius * body.radius);
            let u = if qc <= 0.0 {
                0.0
            } else if qa == 0.0 {
                continue;
            } else {
                let disc = qb * qb - 4.0 * qa * qc;
                if disc < 0.0 {
                    continue;
                }
                let u = (-qb - disc.sqrt()) / (2.0 * qa);
                if !(0.0..=1.0).contains(&u) {
                    continue;
                }
                u
            };
            let t = t0 + u * (t1 - t0);
            if best.is_none_or(|(_, bt)| t < bt) {
                best = Some((i, t));
            }
        }
        best
    }

    /// Visible stellar disc fraction, including finite-star umbra and penumbra.
    /// Independent of sensor line-of-sight occlusion. `exclude` is the surface
    /// being shaded, so planets and moons never eclipse themselves.
    pub fn stellar_visibility(&self,point:Vec2,t:f64,exclude:Option<usize>)->f64 {
        let Some((star_id,star))=self.bodies.iter().enumerate().find(|(_,b)|b.kind==CelestialKind::Star) else {return 1.0;};
        let toward=self.state(star_id,t).pos-point;let distance=toward.length();
        if distance<=star.radius {return 1.0;}
        let a=(star.radius/distance).asin();
        let mut visibility=1.0_f64;
        for (i,b) in self.bodies.iter().enumerate() {
            if i==star_id || Some(i)==exclude {continue;}
            let delta=self.state(i,t).pos-point;let d=delta.length();
            if d>=distance || delta.dot(toward)<=0.0 {continue;}
            if d<=b.radius {return 0.0;}
            let radius=(b.radius/d).asin();
            let separation=(delta.dot(toward)/(d*distance)).clamp(-1.0,1.0).acos();
            if separation>=a+radius {continue;}
            let covered=if separation<=(a-radius).abs() {
                if radius>=a {1.0} else {(radius/a).powi(2)}
            } else {
                let x=(separation*separation+a*a-radius*radius)/(2.0*separation*a);
                let y=(separation*separation+radius*radius-a*a)/(2.0*separation*radius);
                let lens=(-separation+a+radius)*(separation+a-radius)*(separation-a+radius)*(separation+a+radius);
                (a*a*x.clamp(-1.0,1.0).acos()+radius*radius*y.clamp(-1.0,1.0).acos()-0.5*lens.max(0.0).sqrt())/(std::f64::consts::PI*a*a)
            };
            visibility=visibility.min((1.0-covered).clamp(0.0,1.0));
        }
        visibility
    }

    /// The first body whose disc blocks a light path from `from` (emitted at `t_from`)
    /// to `to` (received at `t_to`). Each body's motion is linearised over the transit,
    /// which is accurate for light crossing a solar system in minutes.
    pub fn occluder(&self, from: Vec2, t_from: f64, to: Vec2, t_to: f64) -> Option<usize> {
        let tm = 0.5 * (t_from + t_to);
        let span = t_to - t_from;
        self.bodies.iter().enumerate().find_map(|(i, b)| {
            let c = self.state(i, tm);
            // Relative position of the light point to the body, linear in u ∈ [0, 1].
            let r0 = from - (c.pos - c.vel * (0.5 * span));
            let d = (to - from) - c.vel * span;
            let u = if d.dot(d) > 0.0 { (-r0.dot(d) / d.dot(d)).clamp(0.0, 1.0) } else { 0.0 };
            // Ignore grazes at the endpoints so a ship on the surface is not self-blocked.
            ((r0 + d * u).length() < b.radius && u > 0.0 && u < 1.0).then_some(i)
        })
    }

    /// Display-only forecast of a free body under constant thrust plus gravity.
    /// Stops at the first surface contact.
    pub fn predict(&self, mut s: State, thrust: Vec2, t0: f64, duration: f64, max_points: usize) -> Prediction {
        let mut t = t0;
        let mut points = vec![s.pos];
        let every = (duration / max_points as f64).max(MIN_STEP_S);
        let mut next_point = t0 + every;
        while t < t0 + duration {
            let dt = self.step_size(s.pos, t).min(t0 + duration - t).max(1e-3);
            let a = thrust + self.step_gravity(s, thrust, t, dt);
            let n = crate::kinematics::advance(s, a, dt);
            if let Some((body, ti)) = self.impact(s.pos, n.pos, t, t + dt) {
                let u = (ti - t) / dt;
                points.push(s.pos + (n.pos - s.pos) * u);
                return Prediction { points, impact: Some((body, ti)) };
            }
            s = n;
            t += dt;
            if t >= next_point {
                points.push(s.pos);
                next_point += every;
            }
        }
        points.push(s.pos);
        Prediction { points, impact: None }
    }
}

pub struct Prediction {
    pub points: Vec<Vec2>,
    pub impact: Option<(usize, f64)>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::AU;

    fn sun_only() -> System {
        System {
            bodies: vec![Celestial {
                name: "Sun".into(),
                kind: CelestialKind::Star,
                gm: 1.327_124_4e11,
                radius: 696_000.0,
                orbit: Orbit::Fixed(Vec2::ZERO),
            }],
        }
    }

    #[test]
    fn circular_orbit_closes_after_one_period() {
        let sys = sun_only();
        let r = AU;
        let v = (sys.bodies[0].gm / r).sqrt();
        let period = TAU * r / v;
        // Coarse steps so the test is quick; max step is 60 s → ~5e5 steps. Use a
        // shorter arc: a quarter orbit must land on the +y axis at radius r.
        let p = sys.predict(State { pos: Vec2::new(r, 0.0), vel: Vec2::new(0.0, v) }, Vec2::ZERO, 0.0, period / 4.0, 4);
        let end = *p.points.last().unwrap();
        assert!((end.length() / r - 1.0).abs() < 1e-5, "{}", end.length() / r);
        assert!(end.x.abs() / r < 1e-3, "{}", end.x / r);
    }

    #[test]
    fn falling_into_the_sun_is_detected() {
        let sys = sun_only();
        let p = sys.predict(State { pos: Vec2::new(0.1 * AU, 0.0), vel: Vec2::ZERO }, Vec2::ZERO, 0.0, 30.0 * 86_400.0, 10);
        assert_eq!(p.impact.map(|(b, _)| b), Some(0));
    }

    #[test]
    fn the_sun_casts_a_sensor_shadow() {
        let sys = sun_only();
        let (a, b) = (Vec2::new(-AU, 0.0), Vec2::new(AU, 0.0));
        assert_eq!(sys.occluder(a, 0.0, b, 1000.0), Some(0));
        let b2 = Vec2::new(AU, 0.1 * AU);
        assert_eq!(sys.occluder(a, 0.0, b2, 1000.0), None);
    }

    #[test]
    fn points_belong_to_the_smallest_sphere_of_influence() {
        let sys = crate::scenario::home_system();
        let planet = sys.state(1, 0.0).pos;
        let moon = sys.state(2, 0.0).pos;
        assert_eq!(sys.frame_for(planet + Vec2::new(100_000.0, 0.0), 0.0), 1);
        assert_eq!(sys.frame_for(moon + Vec2::new(5_000.0, 0.0), 0.0), 2);
        assert_eq!(sys.frame_for(Vec2::new(0.5 * AU, 0.3 * AU), 0.0), 0);
    }

    #[test]
    fn satellite_orbits_its_parent() {
        let mut sys = sun_only();
        sys.bodies.push(Celestial {
            name: "Planet".into(),
            kind: CelestialKind::Planet,
            gm: 398_600.0,
            radius: 6371.0,
            orbit: Orbit::Circular { parent: 0, radius: AU, period: 365.25 * 86_400.0, phase: 0.0 },
        });
        let s = sys.state(1, 365.25 * 86_400.0 / 4.0);
        assert!(s.pos.x.abs() < 1.0 && (s.pos.y - AU).abs() < 1.0);
    }
}
