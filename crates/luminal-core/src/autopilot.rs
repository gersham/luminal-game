//! Standing manoeuvre orders: orbit a celestial body, or intercept a target.
//!
//! Guidance is recomputed every sensor frame from what the ship's faction knows: its
//! own exact state, public ephemerides, and, for enemy targets, its track estimate.
//! Nothing here reads world truth about foreign bodies.

use crate::celestial::{Orbit, System};
use crate::kinematics::{State, Vec2};

/// Velocity-error time constant, s. Twice the sensor frame keeps the loop stable.
const VELOCITY_TAU_S: f64 = 20.0;
/// Radial-error time constant near the target orbit, s.
const RADIAL_TAU_S: f64 = 120.0;
/// Commands smaller than this are not worth burning for: coast instead. km/s².
const DEADBAND: f64 = 1e-5;

/// Radius range for a sensible parking orbit around celestial `i`: clear of the surface,
/// inside its gravitational dominance, and inside any of its moons' orbits.
pub fn orbit_bounds(sys: &System, i: usize) -> (f64, f64) {
    let b = &sys.bodies[i];
    let min = 1.5 * b.radius;
    let hill = match b.orbit {
        Orbit::Circular { parent, radius, .. } | Orbit::Frozen { parent, radius, .. } => 0.3 * radius * (b.gm / (3.0 * sys.bodies[parent].gm)).cbrt(),
        Orbit::Fixed(_) => f64::INFINITY,
    };
    let satellites = sys
        .bodies
        .iter()
        .filter_map(|c| match c.orbit {
            Orbit::Circular { parent, radius, .. } | Orbit::Frozen { parent, radius, .. } if parent == i => Some(0.6 * radius),
            _ => None,
        })
        .fold(f64::INFINITY, f64::min);
    let min = if i == 0 { 3.0 * b.radius } else { min };
    (min, hill.min(satellites).max(min * 1.5))
}

/// The orbit radius to aim for from the current distance: stay at this altitude if it
/// is sensible, otherwise the nearest sensible one.
pub fn convenient_orbit_radius(sys: &System, i: usize, distance: f64) -> f64 {
    let (lo, hi) = orbit_bounds(sys, i);
    distance.clamp(lo, hi)
}

/// Thrust to settle into, then hold, a circular orbit of `radius` about celestial `i`.
/// `sense` is +1 for counter-clockwise, −1 for clockwise.
pub fn orbit_thrust(sys: &System, i: usize, ship: State, t: f64, radius: f64, sense: f64, max_accel: f64) -> Vec2 {
    let c = sys.state(i, t);
    let rel = ship.pos - c.pos;
    let vrel = ship.vel - c.vel;
    let r = rel.length().max(1.0);
    let rhat = rel * (1.0 / r);
    let that = Vec2::new(-rhat.y, rhat.x) * sense;

    let dr = r - radius;
    // Close radial error no faster than the drive could stop it (factor 2 margin).
    let vr_des = -dr.signum() * (max_accel * dr.abs()).sqrt().min(dr.abs() / RADIAL_TAU_S);
    let vt_des = (sys.bodies[i].gm / r).sqrt();
    let v_des = rhat * vr_des + that * vt_des;

    let cmd = (v_des - vrel) * (1.0 / VELOCITY_TAU_S);
    clamp(cmd, max_accel)
}

/// Direction of `vrel × rel` sense for an existing relative motion (+1 CCW, −1 CW).
pub fn orbit_sense(rel: Vec2, vrel: Vec2) -> f64 {
    if rel.x * vrel.y - rel.y * vrel.x < 0.0 { -1.0 } else { 1.0 }
}

/// Distance at which an intercept holds station on its target, km. PLACEHOLDER.
pub const STANDOFF_KM: f64 = 1_000.0;
/// Combat manoeuvre presets sit inside the weapon's useful engagement envelope,
/// not at its outer launch limit. Beam standoff assumes a manoeuvring opponent.
pub fn weapon_standoff(payload:crate::missile::Payload)->f64 {
    use crate::missile::Payload;
    match payload {
        Payload::Beam=>crate::params::SHIP_BEAM_AUTO_RANGE_LS.value*crate::units::LIGHT_SECOND/3.0,
        Payload::Nuclear|Payload::Kinetic=>payload.engagement_range()*0.5,
    }
}
/// Fraction of the drive a move order plans to brake with; the rest absorbs control lag
/// and steers out cross-track velocity.
const MOVE_BRAKE_FRACTION: f64 = 0.8;
/// Fraction of the drive an intercept plans to brake with.
const INTERCEPT_BRAKE_FRACTION: f64 = 0.5;
/// Final-approach time constant: inside the braking curve the ship eases in, s.
const FINAL_TAU_S: f64 = 60.0;

pub struct Approach {
    pub thrust: Vec2,
    /// Estimated time to arrive, s.
    pub eta: f64,
    /// Distance still to close (beyond any standoff), km.
    pub gap: f64,
    /// Speed relative to the destination, km/s.
    pub rel_speed: f64,
}

/// Time-optimal approach to a moving destination: close at the fastest speed from which
/// the drive can still stop at the destination, so the ship accelerates flat out, flips
/// over, and brakes to a halt. `ff` is the acceleration needed just to keep pace with
/// the destination (its acceleration minus the ship's gravity).
fn approach(ship: State, dest: State, ff: Vec2, stop_at: f64, brake: f64, max_accel: f64) -> Approach {
    use crate::kinematics::proper_velocity;
    use crate::units::C;
    let dp = dest.pos - ship.pos;
    let range = dp.length();
    let dir = dp.normalized();
    let gap = range - stop_at;
    // The fastest proper speed from which braking at `brake` still stops within `gap`:
    // relativistic stopping distance is (c²/a)(γ − 1).
    let stop_u = |d: f64| C * ((1.0 + brake * d / (C * C)).powi(2) - 1.0).max(0.0).sqrt();
    let speed = gap.signum() * stop_u(gap.abs()).min(gap.abs() / FINAL_TAU_S).max(-50.0);
    // Work in proper velocity, which is what thrust changes. Destinations move slowly,
    // so subtracting proper velocities is an adequate relative measure.
    let u_rel = proper_velocity(ship.vel) - proper_velocity(dest.vel);
    let cmd = ff + (dir * speed - u_rel) * (1.0 / VELOCITY_TAU_S);
    // Bang-bang estimate from the current closing speed.
    let u = u_rel.dot(dir);
    let a = brake.max(1e-12);
    let d = gap.max(0.0);
    let v_peak = (a * d + 0.5 * u * u).sqrt();
    let eta = ((2.0 * v_peak - u) / a).max(0.0);
    Approach { thrust: clamp(cmd, max_accel), eta, gap, rel_speed: (ship.vel - dest.vel).length() }
}

/// Guidance to stop at a destination that moves with a celestial frame.
pub fn move_to(ship: State, dest: State, ff: Vec2, max_accel: f64) -> Approach {
    approach(ship, dest, ff, 0.0, MOVE_BRAKE_FRACTION * max_accel, max_accel)
}

/// Guidance to close on a target, match its velocity and hold station at `STANDOFF_KM`.
pub fn rendezvous(ship: State, target: State, ff: Vec2, max_accel: f64) -> Approach {
    approach(ship, target, ff, STANDOFF_KM, INTERCEPT_BRAKE_FRACTION * max_accel, max_accel)
}

pub fn keep_range(ship:State,target:State,ff:Vec2,range:f64,max_accel:f64)->Approach {
    approach(ship,target,ff,range,INTERCEPT_BRAKE_FRACTION*max_accel,max_accel)
}

/// Full burn toward the target's current estimated position, without approach braking.
pub fn flyby(ship: State, target: State, _target_accel: Vec2, max_accel: f64) -> Approach {
    let delta = target.pos - ship.pos;
    let closing=(ship.vel-target.vel).dot(delta.normalized());
    let eta=if closing>0.0 {delta.length()/closing} else {f64::INFINITY};
    Approach { thrust: delta.normalized() * max_accel, eta, gap: delta.length(), rel_speed: (target.vel - ship.vel).length() }
}

/// Maximum lateral displacement from an incoming missile's predicted flight line.
pub fn evade_missile(ship:State,missile:State,max_accel:f64)->Vec2 {
    let relative=ship.pos-missile.pos;
    let velocity=ship.vel-missile.vel;
    let axis=velocity.normalized();
    let miss=relative-axis*relative.dot(axis);
    let direction=if miss.length()>1e-6 {miss.normalized()}
        else if axis!=Vec2::ZERO {Vec2::new(-axis.y,axis.x)}
        else {relative.normalized()};
    direction*max_accel
}

/// How far ahead the collision check looks, s.
pub const AVOID_HORIZON_S: f64 = 3600.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Avoidance {
    /// The thrust to actually apply.
    pub thrust: Vec2,
    /// The desired thrust was overridden to avoid a collision.
    pub active: bool,
    /// No thrust within limits avoids the collision; `thrust` is the best delay found.
    pub impossible: bool,
}

/// Safety layer: keep `desired` thrust unless it leads into a celestial body within the
/// horizon, in which case find the smallest deviation (weakest thrust, then nearest
/// direction) that clears every surface.
pub fn avoid(sys: &System, ship: State, t: f64, desired: Vec2, max_accel: f64) -> Avoidance {
    let keep = Avoidance { thrust: desired, active: false, impossible: false };
    // Cheap reject: no surface is reachable within the horizon even at the strongest
    // gravity (surface value) plus the desired thrust.
    let h = AVOID_HORIZON_S;
    let reachable = sys.bodies.iter().enumerate().any(|(i, b)| {
        let c = sys.state(i, t);
        let reach = (ship.vel - c.vel).length() * h + 0.5 * (desired.length() + b.gm / (b.radius * b.radius)) * h * h;
        (ship.pos - c.pos).length() - b.radius < reach
    });
    if !reachable {
        return keep;
    }
    let Some((body, t_hit)) = sys.predict(ship, desired, t, AVOID_HORIZON_S, 2).impact else { return keep };

    // Prefer the direction that widens the miss: away from the body, across our motion.
    let c = sys.state(body, t);
    let rel = ship.pos - c.pos;
    let vrel = (ship.vel - c.vel).normalized();
    let mut away = rel - vrel * rel.dot(vrel);
    if away.length() < 1e-9 {
        away = Vec2::new(-vrel.y, vrel.x);
    }
    let preferred = if desired.length() > 0.0 { desired.normalized() } else { away.normalized() };

    const DIRECTIONS: usize = 24;
    let mut dirs: Vec<Vec2> = (0..DIRECTIONS)
        .map(|k| {
            let a = std::f64::consts::TAU * k as f64 / DIRECTIONS as f64;
            Vec2::new(a.cos(), a.sin())
        })
        .collect();
    dirs.push(away.normalized());
    dirs.sort_by(|a, b| b.dot(preferred).total_cmp(&a.dot(preferred)));

    let g = crate::units::G0;
    let mut mags: Vec<f64> = [desired.length(), g, 10.0 * g, max_accel].into_iter().filter(|m| *m > 0.0 && *m <= max_accel).collect();
    mags.dedup();
    let horizon = (1.5 * (t_hit - t) + 60.0).min(AVOID_HORIZON_S);

    let mut best = (desired, t_hit);
    for &m in &mags {
        for &d in &dirs {
            let thrust = d * m;
            match sys.predict(ship, thrust, t, horizon, 2).impact {
                None => return Avoidance { thrust, active: true, impossible: false },
                Some((_, th)) if th > best.1 => best = (thrust, th),
                Some(_) => {}
            }
        }
    }
    Avoidance { thrust: best.0, active: true, impossible: true }
}

fn clamp(v: Vec2, max: f64) -> Vec2 {
    let l = v.length();
    if l < DEADBAND {
        Vec2::ZERO
    } else if l > max {
        v * (max / l)
    } else {
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn evasive_burn_maximizes_lateral_miss_distance() {
        let ship=State {pos:Vec2::ZERO,vel:Vec2::ZERO};
        let missile=State {pos:Vec2::new(10000.0,0.0),vel:Vec2::new(-100.0,0.0)};
        let burn=evade_missile(ship,missile,1.2);
        assert!((burn.length()-1.2).abs()<1e-12);
        assert!(burn.x.abs()<1e-12);
        assert!(burn.y.abs()>1.19);
        let offset=State {pos:Vec2::new(10000.0,100.0),..missile};
        assert!(evade_missile(ship,offset,1.2).y<0.0);
    }

    #[test]
    fn weapon_range_presets_settle_from_inside_and_outside_against_moving_targets() {
        use crate::missile::Payload;
        for payload in [Payload::Beam,Payload::Kinetic,Payload::Nuclear] {
            let radius=weapon_standoff(payload);
            for fraction in [0.5,2.0] {
                let mut target=State {pos:Vec2::ZERO,vel:Vec2::new(10.0,-5.0)};
                let mut ship=State {pos:Vec2::new(radius*fraction,0.0),vel:Vec2::ZERO};
                for _ in 0..3600 {
                    let a=keep_range(ship,target,Vec2::ZERO,radius,100.0*crate::units::G0);
                    ship=crate::kinematics::advance(ship,a.thrust,10.0);
                    target.pos=target.pos+target.vel*10.0;
                }
                assert!(((ship.pos-target.pos).length()-radius).abs()<10.0,"{payload:?} from {fraction}");
                assert!((ship.vel-target.vel).length()<0.1,"holds relative velocity");
            }
        }
    }
    #[test]
    fn range_orders_close_withdraw_and_hold_without_overspeed() {
        let target=State {pos:Vec2::ZERO,vel:Vec2::ZERO};
        for (distance,sign) in [(200_000.0,-1.0),(50_000.0,1.0)] {
            let ship=State {pos:Vec2::new(distance,0.0),vel:Vec2::ZERO};
            let r=keep_range(ship,target,Vec2::ZERO,100_000.0,1.0);
            assert!(r.thrust.x*sign>0.0);
            assert!(r.thrust.length()<=1.0+1e-9);
        }
        let ship=State {pos:Vec2::new(100_000.0,0.0),vel:Vec2::ZERO};
        assert_eq!(keep_range(ship,target,Vec2::ZERO,100_000.0,1.0).thrust,Vec2::ZERO);
        let closing=State {vel:Vec2::new(-100.0,0.0),..ship};
        assert!(keep_range(closing,target,Vec2::ZERO,100_000.0,1.0).thrust.x>0.0);
    }
    use crate::scenario::home_system;
    use crate::units::G0;

    #[test]
    fn convenient_orbits_avoid_surfaces_and_moons() {
        let sys = home_system();
        let (lo, hi) = orbit_bounds(&sys, 1);
        assert!(lo > sys.bodies[1].radius);
        assert!(hi < 384_400.0, "planet orbits stay inside the moon: {hi}");
        let (mlo, mhi) = orbit_bounds(&sys, 2);
        assert!(mlo > 1737.4 && mhi > mlo && mhi < 30_000.0, "{mlo} {mhi}");
        assert_eq!(convenient_orbit_radius(&sys, 1, 60_000.0), 60_000.0);
        assert_eq!(convenient_orbit_radius(&sys, 1, 1.0), lo);
    }

    /// Fly the rendezvous law in free space with 10 s updates, like the world does.
    fn fly(mut ship: State, mut target: State, target_accel: Vec2, max_accel: f64, secs: f64) -> (State, State, f64) {
        let dt = 10.0;
        let mut peak = 0.0f64;
        for _ in 0..(secs / dt) as usize {
            let r = rendezvous(ship, target, target_accel, max_accel);
            peak = peak.max(r.thrust.length());
            ship = State { pos: ship.pos + ship.vel * dt + r.thrust * (0.5 * dt * dt), vel: ship.vel + r.thrust * dt };
            target = State { pos: target.pos + target.vel * dt + target_accel * (0.5 * dt * dt), vel: target.vel + target_accel * dt };
        }
        (ship, target, peak)
    }

    #[test]
    fn move_order_flips_over_and_stops_on_the_point_in_near_minimal_time() {
        use crate::units::AU;
        let a = 100.0 * G0;
        let d = 0.3 * AU;
        let dest = State { pos: Vec2::new(d, 0.0), vel: Vec2::ZERO };
        let mut ship = State { pos: Vec2::ZERO, vel: Vec2::new(0.0, 20.0) };
        let dt = 10.0;
        let (mut t, mut flipped_at, mut arrived_at) = (0.0, None, None);
        while t < 8.0 * 3600.0 {
            let m = move_to(ship, dest, Vec2::ZERO, a);
            if flipped_at.is_none() && m.thrust.x < 0.0 {
                flipped_at = Some(t);
            }
            if arrived_at.is_none() && m.gap < 10.0 && m.rel_speed < 0.1 {
                arrived_at = Some(t);
            }
            ship = State { pos: ship.pos + ship.vel * dt + m.thrust * (0.5 * dt * dt), vel: ship.vel + m.thrust * dt };
            t += dt;
        }
        let optimal = 2.0 * (d / a).sqrt();
        let flip = flipped_at.expect("flipped");
        let arrived = arrived_at.expect("arrived and stopped");
        assert!((flip / optimal - 0.5).abs() < 0.1, "flip at {flip} s of {optimal} s");
        assert!(arrived < 1.25 * optimal, "arrived at {arrived} s, optimum {optimal} s");
        assert!((ship.pos - dest.pos).length() < 1.0 && ship.vel.length() < 0.01, "stays put");
    }

    #[test]
    fn a_relativistic_move_still_stops_on_the_point() {
        use crate::kinematics::advance;
        use crate::units::{AU, C};
        let a = 100.0 * G0;
        let dest = State { pos: Vec2::new(20.0 * AU, 0.0), vel: Vec2::ZERO };
        let mut ship = State { pos: Vec2::ZERO, vel: Vec2::ZERO };
        let (dt, mut t, mut peak) = (10.0, 0.0, 0.0f64);
        while t < 48.0 * 3600.0 {
            let m = move_to(ship, dest, Vec2::ZERO, a);
            ship = advance(ship, m.thrust, dt);
            peak = peak.max(ship.vel.length());
            t += dt;
        }
        assert!(peak > 0.15 * C, "peak {} c", peak / C);
        assert!((ship.pos - dest.pos).length() < 5.0, "off by {} km", (ship.pos - dest.pos).length());
        assert!(ship.vel.length() < 0.01);
    }

    #[test]
    fn rendezvous_arrives_matches_velocity_and_throttles_down() {
        let ship = State { pos: Vec2::ZERO, vel: Vec2::ZERO };
        let target = State { pos: Vec2::new(2e6, 5e5), vel: Vec2::new(20.0, -10.0) };
        let (s, t, peak) = fly(ship, target, Vec2::ZERO, 10.0 * G0, 6.0 * 3600.0);
        let range = (t.pos - s.pos).length();
        assert!((range - STANDOFF_KM).abs() < 50.0, "holding at standoff: {range}");
        assert!((t.vel - s.vel).length() < 0.5, "velocity matched");
        assert!(peak > 9.9 * G0, "used full thrust while far");
        assert!(rendezvous(s, t, Vec2::ZERO, 10.0 * G0).thrust.length() < 0.01 * G0, "coasting when holding");
    }

    #[test]
    fn rendezvous_keeps_up_with_an_accelerating_target() {
        let ship = State { pos: Vec2::ZERO, vel: Vec2::ZERO };
        let target = State { pos: Vec2::new(5e5, 0.0), vel: Vec2::ZERO };
        let (s, t, _) = fly(ship, target, Vec2::new(0.0, 2.0 * G0), 10.0 * G0, 4.0 * 3600.0);
        let range = (t.pos - s.pos).length();
        assert!(range < 2.0 * STANDOFF_KM, "{range}");
    }

    #[test]
    fn a_ship_coasting_at_the_planet_is_steered_clear() {
        let sys = home_system();
        let p = sys.state(1, 0.0);
        let ship = State { pos: p.pos + Vec2::new(200_000.0, 0.0), vel: p.vel + Vec2::new(-100.0, 0.0) };
        assert!(sys.predict(ship, Vec2::ZERO, 0.0, AVOID_HORIZON_S, 2).impact.is_some());
        let a = avoid(&sys, ship, 0.0, Vec2::ZERO, 100.0 * G0);
        assert!(a.active && !a.impossible);
        assert!(sys.predict(ship, a.thrust, 0.0, AVOID_HORIZON_S, 2).impact.is_none());
        assert!(a.thrust.length() <= 1.0 * G0 + 1e-12, "gentle correction suffices: {}", a.thrust.length() / G0);
    }

    #[test]
    fn a_safe_course_is_left_alone() {
        let sys = home_system();
        let p = sys.state(1, 0.0);
        let ship = State { pos: p.pos + Vec2::new(200_000.0, 0.0), vel: p.vel + Vec2::new(0.0, 5.0) };
        let a = avoid(&sys, ship, 0.0, Vec2::ZERO, 100.0 * G0);
        assert!(!a.active);
    }

}
