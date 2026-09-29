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
        Payload::Nuclear=>payload.engagement_range()*0.5,
        Payload::Kinetic=>0.01*crate::units::AU,
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

/// Keep a margin outside the selected threat envelope, or well inside beams.
pub fn combat_range(standoff:bool,has_lrm:bool)->f64 {
    if !standoff {weapon_standoff(crate::missile::Payload::Beam)}
    else if has_lrm {1.15*crate::missile::Payload::Kinetic.engagement_range()}
    else {1.15*crate::params::SHIP_BEAM_AUTO_RANGE_LS.value*crate::units::LIGHT_SECOND}
}
/// A quiet station-keeping band prevents small track corrections from switching
/// between inward and outward burns. Velocity matching and early braking remain.
pub fn combat_approach(ship:State,target:State,ff:Vec2,range:f64,max_accel:f64)->Approach {
    let actual=(target.pos-ship.pos).length();
    let error=actual-range;
    let band=range*0.03;
    // Continuous soft deadband: no step in the commanded destination at its edge.
    let quiet_error=error.signum()*(error.abs()-band).max(0.0);
    let mut result=keep_range(ship,target,ff,actual-quiet_error,max_accel);
    result.gap=quiet_error;
    result
}

pub fn keep_range(ship:State,target:State,ff:Vec2,range:f64,max_accel:f64)->Approach {
    // Hostile acceleration is a delayed, filtered estimate. Two ships mirroring
    // it at unity gain can sustain alternating full burns forever at standoff.
    // Let velocity feedback do most of the matching; friendly Follow retains
    // its separate full feed-forward formation controller.
    approach(ship,target,ff*0.25,range,INTERCEPT_BRAKE_FRACTION*max_accel,max_accel)
}

/// Full burn toward the target's current estimated position, without approach braking.
pub fn flyby(ship: State, target: State, _target_accel: Vec2, max_accel: f64) -> Approach {
    let delta = target.pos - ship.pos;
    let closing=(ship.vel-target.vel).dot(delta.normalized());
    let eta=if closing>0.0 {delta.length()/closing} else {f64::INFINITY};
    Approach { thrust: delta.normalized() * max_accel, eta, gap: delta.length(), rel_speed: (target.vel - ship.vel).length() }
}

/// A held escape heading, in the Sol frame. Reevaluate periodically, but do not
/// erase accumulated sideways velocity whenever a seeker corrects its course.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EvasionBurn {
    pub direction: Vec2,
    pub since: f64,
    pub evaluated_at: f64,
}

const EVADE_REASSESS_S: f64 = 5.0;
const EVADE_SIDE_HOLD_S: f64 = 30.0;

/// Conservative additional delta-v a missile would need to reach a continuously
/// accelerating ship. Minimize over possible meeting times, so merely delaying
/// the original closest approach is not mistaken for a guaranteed escape.
/// This is a coasting-track estimate, not knowledge of enemy fuel or guidance.
fn evasion_correction(ship: State, missile: State, burn: Vec2) -> f64 {
    let r = ship.pos - missile.pos;
    let v = ship.vel - missile.vel;
    let encounter = (-r.dot(v) / v.dot(v).max(1e-12)).clamp(1.0, 1800.0);
    let low = (encounter * 0.2).max(0.1);
    let high = (encounter * 4.0).min(7200.0);
    let demand = |t: f64| (r * (1.0 / t) + v + burn * (0.5 * t)).length();
    // Log spacing covers imminent terminal passes and a prolonged stern chase.
    let ratio = (high / low).powf(1.0 / 32.0);
    let mut best = (f64::INFINITY, low);
    let mut at = low;
    for _ in 0..=32 {
        let score = demand(at);
        if score < best.0 { best = (score, at); }
        at *= ratio;
    }
    let (mut lo, mut hi) = ((best.1 / ratio).max(low), (best.1 * ratio).min(high));
    for _ in 0..12 {
        let a = lo + (hi - lo) / 3.0;
        let b = hi - (hi - lo) / 3.0;
        if demand(a) < demand(b) { hi = b; } else { lo = a; }
    }
    demand((lo + hi) * 0.5).min(best.0)
}

/// Choose sustained lateral/oblique escape using received motion only. Both
/// sides are considered initially; a reversal needs a 50% improvement after a
/// 30-second commitment. Small angle changes need a 10% improvement.
pub fn evade_missile(ship: State, missile: State, max_accel: f64, now: f64,
    previous: Option<EvasionBurn>) -> EvasionBurn {
    if let Some(held) = previous && now - held.evaluated_at < EVADE_REASSESS_S {
        return held;
    }
    let away = (ship.pos - missile.pos).normalized();
    let away = if away == Vec2::ZERO { Vec2::new(1.0, 0.0) } else { away };
    let lateral = Vec2::new(-away.y, away.x);
    let preferred = if ship.vel.dot(lateral) < 0.0 { -lateral } else { lateral };
    let mut direction = previous.map_or(preferred, |held| held.direction);
    let current = evasion_correction(ship, missile, direction * max_accel);
    let mut best = current;
    for side in [preferred, -preferred] {
        // Angles from directly away: lateral, then increasingly oblique. An
        // oblique candidate still spends at least half its thrust sideways.
        for angle in [90.0_f64, 75.0, 60.0, 45.0, 30.0] {
            let (sin, cos) = angle.to_radians().sin_cos();
            let candidate = side * sin + away * cos;
            let score = evasion_correction(ship, missile, candidate * max_accel);
            if let Some(held) = previous {
                let reverses = candidate.dot(lateral) * held.direction.dot(lateral) < 0.0;
                if reverses && now - held.since < EVADE_SIDE_HOLD_S { continue; }
                let threshold = if reverses { 1.5 } else { 1.1 };
                if score <= current * threshold + 0.01 { continue; }
            }
            // Tiny tracking noise must not choose against existing sideways
            // motion on the initial decision either.
            if previous.is_none() && ship.vel.dot(lateral).abs()>0.001
                && candidate.dot(preferred)<0.0 && score<=best*1.03+0.01 {continue;}
            if score > best + 1e-6 { direction = candidate; best = score; }
        }
    }
    EvasionBurn { direction, since: previous.filter(|p|p.direction.dot(lateral)*direction.dot(lateral)>=0.0)
        .map_or(now, |held|held.since), evaluated_at: now }
}

/// Spend heat on evasion only when the additional correction demanded is a
/// meaningful share of the missile class's nominal maneuver reserve. No actual
/// remaining fuel is read. Hot ships require a larger expected benefit.
pub fn evasion_worthwhile(ship:State,missile:State,max_accel:f64,plan:EvasionBurn,
    correction_reserve:f64,heat_fraction:f64,missile_accel:f64)->bool {
    let r=missile.pos-ship.pos;
    let closing=-(missile.vel-ship.vel).dot(r.normalized());
    let earliest=2.0*r.length()/(closing+(closing*closing+2.0*missile_accel*r.length()).sqrt()).max(1e-9);
    let threshold=correction_reserve*(0.1+0.2*heat_fraction.clamp(0.0,1.0));
    if 0.5*max_accel*earliest < threshold {return false;}
    let coast=evasion_correction(ship,missile,Vec2::ZERO);
    let escape=evasion_correction(ship,missile,plan.direction*max_accel);
    escape-coast > threshold
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
        let burn=evade_missile(ship,missile,1.2,0.0,None).direction*1.2;
        assert!((burn.length()-1.2).abs()<1e-12);
        assert!(burn.x<=1e-12);
        assert!(burn.y.abs()>=0.59);
        let offset=State {pos:Vec2::new(10000.0,100.0),..missile};
        assert!(evade_missile(ship,offset,1.2,0.0,None).direction.y<0.0);
    }

    #[test]
    fn evasion_spends_heat_only_for_meaningful_escape_opportunities() {
        let ship=State {pos:Vec2::ZERO,vel:Vec2::ZERO};
        let assess=|distance:f64,velocity:f64,heat:f64| {
            let missile=State {pos:Vec2::new(distance,0.0),vel:Vec2::new(-velocity,0.0)};
            let plan=evade_missile(ship,missile,1.2,0.0,None);
            evasion_worthwhile(ship,missile,1.2,plan,7946.3,heat,14.71)
        };
        assert!(!assess(1_500_000.0,100.0,0.0),"fresh close missile can accelerate after us");
        assert!(!assess(1_500_000.0,40_000.0,0.0),"imminent terminal pass leaves no useful dodge");
        assert!(assess(60_000_000.0,10_000.0,0.0),"distant known missile permits a useful burn");
        assert!(!assess(60_000_000.0,10_000.0,1.0),"hot ship should conserve heat for a marginal dodge");
    }

    #[test]
    fn sustained_evasion_keeps_side_through_noisy_seeker_corrections() {
        let ship=State {pos:Vec2::ZERO,vel:Vec2::new(0.0,10.0)};
        let mut previous=None;
        for tick in 0..25 {
            let noise=if tick%2==0 {0.01} else {-0.01};
            let missile=State {pos:Vec2::new(10000.0,noise),vel:Vec2::new(-100.0,10.0+noise)};
            let plan=evade_missile(ship,missile,1.2,tick as f64*5.0,previous);
            assert!(plan.direction.y>0.0,"must preserve accumulated sideways motion");
            if let Some(held)=previous {assert!(plan.direction.dot(held.direction)>0.99);}
            previous=Some(plan);
        }
    }

    #[test]
    fn evasion_selects_oblique_for_slow_chase_and_lateral_for_fast_terminal() {
        let ship=State {pos:Vec2::ZERO,vel:Vec2::ZERO};
        let slow=State {pos:Vec2::new(10000.0,0.0),vel:Vec2::new(-100.0,0.0)};
        let fast=State {vel:Vec2::new(-1000.0,0.0),..slow};
        let oblique=evade_missile(ship,slow,1.2,0.0,None);
        let lateral=evade_missile(ship,fast,1.2,0.0,None);
        assert!(oblique.direction.x < -0.2);
        assert!(lateral.direction.x.abs()<0.05);
        let old=Vec2::new(0.0,-1.2);
        assert!(evasion_correction(ship,slow,oblique.direction*1.2)>evasion_correction(ship,slow,old)*1.05);
        // A clearly better opposite side can win after commitment, not before.
        let wrong=EvasionBurn {direction:-oblique.direction,since:0.0,evaluated_at:0.0};
        let held=evade_missile(ship,slow,1.2,5.0,Some(wrong));
        assert!(held.direction.y>0.0);
        let ship=State {vel:Vec2::new(0.0,-100.0),..ship};
        let changed=evade_missile(ship,slow,1.2,35.0,Some(wrong));
        assert!(changed.direction.y<0.0);
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

#[cfg(test)]
mod combat_range_tests {
    use super::*;
    use crate::units::{LIGHT_SECOND,G0};
    #[test]
    fn close_brakes_without_crossing_target_and_settles_quietly() {
        let target=State {pos:Vec2::ZERO,vel:Vec2::ZERO};
        let desired=combat_range(false,false);
        for speed in [0.0,500.0,2000.0] {
            let mut ship=State {pos:Vec2::new(-50.0*LIGHT_SECOND,0.0),vel:Vec2::new(speed,0.0)};
            let mut nearest=f64::INFINITY;
            for _ in 0..21600 {
                let a=combat_approach(ship,target,Vec2::ZERO,desired,100.0*G0);
                ship.vel=ship.vel+a.thrust;ship.pos=ship.pos+ship.vel;
                nearest=nearest.min(ship.pos.length());
            }
            assert!(nearest>desired*0.9,"overshoot at {speed}: {nearest}");
            assert!((ship.pos.length()-desired).abs()<desired*0.031);
            assert!(ship.vel.length()<0.01);
        }
    }
    #[test]
    fn station_band_ignores_small_position_noise_but_brakes_relative_motion() {
        let range=combat_range(true,false);
        for fraction in [0.98,1.0,1.02] {
            let ship=State {pos:Vec2::ZERO,vel:Vec2::ZERO};
            let target=State {pos:Vec2::new(range*fraction,0.0),vel:Vec2::ZERO};
            assert_eq!(combat_approach(ship,target,Vec2::ZERO,range,G0).thrust,Vec2::ZERO);
            assert!(combat_approach(State {vel:Vec2::new(10.0,0.0),..ship},target,Vec2::ZERO,range,G0).thrust.x<0.0);
        }
        assert!(combat_range(true,true)>crate::missile::Payload::Kinetic.engagement_range());
        assert!(range>crate::params::SHIP_BEAM_AUTO_RANGE_LS.value*LIGHT_SECOND);
    }
}
