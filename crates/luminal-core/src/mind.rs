//! The agent-facing information boundary. A `Perception` is everything one faction's
//! decider has causally received. Players and bots see the world only through this.
//!
//! Nothing here holds a reference to world truth. Contacts are opaque ids assigned by
//! the faction's own association, never body indices.

// Small fixed-size matrix algebra reads more clearly with explicit indices.
#![allow(clippy::needless_range_loop)]

use crate::celestial::System;
use crate::kinematics::Vec2;
use crate::params::TRACK_MANEUVER_G;
use crate::sensors::{bearing_of, wrap_angle};
use crate::units::G0;
use crate::world::{BodyId, FactionId};
use std::collections::{BTreeMap, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContactId(pub u32);

impl std::fmt::Display for ContactId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "T{}", self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Measurement {
    /// Long-range direction finding, without localization.
    Bearing { bearing: f64, sigma: f64 },
    /// Passive localization or active echo: noisy direction and range.
    BearingRange { bearing: f64, sigma_bearing: f64, range: f64, sigma_range: f64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The contact's own emission (waste heat, drive).
    Emission,
    /// An echo of one of our pings.
    Echo,
    /// The contact's active ping, seen directly.
    Ping,
}

#[derive(Clone, Copy, Debug)]
pub struct Observation {
    pub contact: ContactId,
    /// Our ship whose sensor made it.
    pub sensor: BodyId,
    /// Sensor position at receipt.
    pub origin: Vec2,
    /// When the measured light left the contact (emission or reflection).
    pub emitted_at: f64,
    /// When the sensor received it.
    pub sensor_received_at: f64,
    /// When the faction's decider received the sensor's report.
    pub decider_received_at: f64,
    pub measurement: Measurement,
    pub snr: f64,
    pub source: Source,
}

const N: usize = 6;
type M = [[f64; N]; N];

fn mat_mul(a: &M, b: &M) -> M {
    let mut r = [[0.0; N]; N];
    for i in 0..N {
        for j in 0..N {
            r[i][j] = (0..N).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    r
}

fn transpose(a: &M) -> M {
    let mut r = [[0.0; N]; N];
    for i in 0..N {
        for j in 0..N {
            r[i][j] = a[j][i];
        }
    }
    r
}

fn symmetrize(p: &mut M) {
    for i in 0..N {
        for j in 0..i {
            let m = 0.5 * (p[i][j] + p[j][i]);
            p[i][j] = m;
            p[j][i] = m;
        }
    }
}

/// Covariance of a point located by range and bearing, km².
fn polar_cov(bearing: f64, range: f64, sigma_range: f64, sigma_bearing: f64) -> [[f64; 2]; 2] {
    let (s, c) = bearing.sin_cos();
    let (a, t) = (sigma_range * sigma_range, (range * sigma_bearing).powi(2));
    [[a * c * c + t * s * s, (a - t) * c * s], [(a - t) * c * s, a * s * s + t * c * c]]
}

/// Constant-acceleration Kalman track. Ships fly long constant-thrust segments, so the
/// thrust is a state; known gravity is added on top in prediction. The state refers to
/// the contact at `t`, an emission time: what it was doing then.
#[derive(Clone, Debug)]
pub struct Track {
    pub t: f64,
    /// x, y (km), vx, vy (km/s), ax, ay (thrust, km/s²).
    pub x: [f64; N],
    pub p: M,
    pub updates: u32,
}

/// Initial velocity uncertainty for a new track, km/s.
const INITIAL_SIGMA_V: f64 = 10_000.0;
/// Initial thrust uncertainty for a new track, in g.
const INITIAL_SIGMA_A_G: f64 = 20.0;

impl Track {
    pub fn new(t: f64, pos: Vec2, cov: [[f64; 2]; 2]) -> Self {
        let v2 = INITIAL_SIGMA_V * INITIAL_SIGMA_V;
        let a2 = (INITIAL_SIGMA_A_G * G0).powi(2);
        let mut p = [[0.0; N]; N];
        p[0][0] = cov[0][0];
        p[0][1] = cov[0][1];
        p[1][0] = cov[1][0];
        p[1][1] = cov[1][1];
        p[2][2] = v2;
        p[3][3] = v2;
        p[4][4] = a2;
        p[5][5] = a2;
        Self { t, x: [pos.x, pos.y, 0.0, 0.0, 0.0, 0.0], p, updates: 1 }
    }

    pub fn pos(&self) -> Vec2 {
        Vec2::new(self.x[0], self.x[1])
    }

    pub fn vel(&self) -> Vec2 {
        Vec2::new(self.x[2], self.x[3])
    }

    /// Estimated thrust, km/s².
    pub fn accel(&self) -> Vec2 {
        Vec2::new(self.x[4], self.x[5])
    }

    pub fn pos_cov(&self) -> [[f64; 2]; 2] {
        [[self.p[0][0], self.p[0][1]], [self.p[1][0], self.p[1][1]]]
    }

    /// Propagate to `t` (may be earlier, for out-of-order reports).
    pub fn predict(&mut self, t: f64, sys: &System) {
        let dt = t - self.t;
        if dt == 0.0 {
            return;
        }
        // State: substeps so gravity near planets is followed.
        let n = (dt.abs() / 300.0).ceil().clamp(1.0, 200.0) as usize;
        let h = dt / n as f64;
        let (mut pos, mut vel, acc, mut tt) = (self.pos(), self.vel(), self.accel(), self.t);
        for _ in 0..n {
            let a = acc + sys.gravity(pos + vel * (0.5 * h), tt + 0.5 * h);
            pos = pos + vel * h + a * (0.5 * h * h);
            vel = vel + a * h;
            tt += h;
        }
        self.x = [pos.x, pos.y, vel.x, vel.y, acc.x, acc.y];

        // Covariance: constant-acceleration transition, white-jerk process noise.
        let mut f = [[0.0; N]; N];
        for i in 0..N {
            f[i][i] = 1.0;
        }
        for k in 0..2 {
            f[k][2 + k] = dt;
            f[k][4 + k] = 0.5 * dt * dt;
            f[2 + k][4 + k] = dt;
        }
        let mut p = mat_mul(&mat_mul(&f, &self.p), &transpose(&f));
        // Thrust may change by about TRACK_MANEUVER_G per minute, 1σ.
        let q = (TRACK_MANEUVER_G.value * G0).powi(2) / 60.0;
        let d = dt.abs();
        let qa = [
            [d.powi(5) / 20.0, d.powi(4) / 8.0, d.powi(3) / 6.0],
            [d.powi(4) / 8.0, d.powi(3) / 3.0, d * d / 2.0],
            [d.powi(3) / 6.0, d * d / 2.0, d],
        ];
        for k in 0..2 {
            let idx = [k, 2 + k, 4 + k];
            for (r, &i) in idx.iter().enumerate() {
                for (c, &j) in idx.iter().enumerate() {
                    p[i][j] += q * qa[r][c];
                }
            }
        }
        self.p = p;
        self.t = t;
    }

    /// A copy propagated to `t`, for display or planning.
    pub fn at(&self, t: f64, sys: &System) -> Track {
        let mut c = self.clone();
        c.predict(t, sys);
        c
    }

    /// Extended Kalman update with a bearing seen from `origin`.
    pub fn update_bearing(&mut self, origin: Vec2, z: f64, sigma: f64) {
        let d = self.pos() - origin;
        let r2 = d.dot(d).max(1.0);
        let mut h = [0.0; N];
        h[0] = -d.y / r2;
        h[1] = d.x / r2;
        let ph: [f64; N] = std::array::from_fn(|i| (0..N).map(|k| self.p[i][k] * h[k]).sum());
        let s = (0..N).map(|k| h[k] * ph[k]).sum::<f64>() + sigma * sigma;
        let y = wrap_angle(z - bearing_of(d));
        for i in 0..N {
            self.x[i] += ph[i] / s * y;
        }
        for i in 0..N {
            for j in 0..N {
                self.p[i][j] -= ph[i] * ph[j] / s;
            }
        }
        symmetrize(&mut self.p);
        self.updates += 1;
    }

    /// Linear update with a measured position and its covariance.
    pub fn update_position(&mut self, z: Vec2, r: [[f64; 2]; 2]) {
        let s = [[self.p[0][0] + r[0][0], self.p[0][1] + r[0][1]], [self.p[1][0] + r[1][0], self.p[1][1] + r[1][1]]];
        let det = s[0][0] * s[1][1] - s[0][1] * s[1][0];
        if det.abs() < 1e-30 {
            return;
        }
        let si = [[s[1][1] / det, -s[0][1] / det], [-s[1][0] / det, s[0][0] / det]];
        // K = P Hᵀ S⁻¹ with H selecting position: PHᵀ is the first two columns of P.
        let k: [[f64; 2]; N] = std::array::from_fn(|i| {
            [
                self.p[i][0] * si[0][0] + self.p[i][1] * si[1][0],
                self.p[i][0] * si[0][1] + self.p[i][1] * si[1][1],
            ]
        });
        let y = [z.x - self.x[0], z.y - self.x[1]];
        for i in 0..N {
            self.x[i] += k[i][0] * y[0] + k[i][1] * y[1];
        }
        let old = self.p;
        for i in 0..N {
            for j in 0..N {
                self.p[i][j] = old[i][j] - (k[i][0] * old[0][j] + k[i][1] * old[1][j]);
            }
        }
        symmetrize(&mut self.p);
        self.updates += 1;
    }
}

/// Intersect two bearing lines; returns the point and its covariance, or `None` when
/// the lines are too close to parallel to say anything useful.
pub fn triangulate(a: (Vec2, f64, f64), b: (Vec2, f64, f64)) -> Option<(Vec2, [[f64; 2]; 2])> {
    let (pa, ba, sa) = a;
    let (pb, bb, sb) = b;
    let (ua, ub) = (Vec2::new(ba.cos(), ba.sin()), Vec2::new(bb.cos(), bb.sin()));
    let cross = ua.x * ub.y - ua.y * ub.x;
    if cross.abs() < 3.0 * (sa + sb) {
        return None;
    }
    let d = pb - pa;
    let s = (d.x * ub.y - d.y * ub.x) / cross;
    let t = (d.x * ua.y - d.y * ua.x) / cross;
    if s <= 0.0 || t <= 0.0 {
        return None;
    }
    let p = pa + ua * s;
    // Information from each line constrains only the perpendicular direction.
    let mut info = [[0.0; 2]; 2];
    for (u, range, sigma) in [(ua, s, sa), (ub, t, sb)] {
        let n = Vec2::new(-u.y, u.x);
        let w = 1.0 / (range * sigma).powi(2);
        info[0][0] += w * n.x * n.x;
        info[0][1] += w * n.x * n.y;
        info[1][0] += w * n.x * n.y;
        info[1][1] += w * n.y * n.y;
    }
    let det = info[0][0] * info[1][1] - info[0][1] * info[1][0];
    let cov = [[info[1][1] / det, -info[0][1] / det], [-info[1][0] / det, info[0][0] / det]];
    Some((p, cov))
}

#[derive(Clone, Debug)]
pub struct Contact {
    /// Identification requires a direct passive localisation or a usable echo,
    /// not triangulated bearings or interception of the target's ping.
    pub resolved: bool,
    systematic_floor: Option<(f64,[[f64;2];2])>,
    pub id: ContactId,
    pub track: Option<Track>,
    /// Latest bearing report from each of our sensors.
    pub bearings: BTreeMap<BodyId, Observation>,
    pub last: Observation,
}

/// A report whose light left this long before the track's newest one is too stale to
/// fold in, s: retrodicting and re-predicting across a long gap jerks the estimate.
/// It still shows as a bearing.
const STALE_REPORT_S: f64 = 0.0;
/// Slack beyond the light-time across the baseline for pairing two bearings into a
/// triangulation, s. Bearings taken further apart in time see a moving target in two
/// different places, and the fix would be confidently wrong.
const TRIANGULATION_SLACK_S: f64 = 1.0;
/// How many recent observations to keep for inspection.
const LOG_LEN: usize = 200;

#[derive(Clone, Debug)]
pub struct Perception {
    pub faction: FactionId,
    pub contacts: BTreeMap<ContactId, Contact>,
    pub log: VecDeque<Observation>,
}

impl Perception {
    pub fn new(faction: FactionId) -> Self {
        Self { faction, contacts: BTreeMap::new(), log: VecDeque::new() }
    }

    /// Fold a report that has reached the decider into the picture.
    pub fn ingest(&mut self, obs: Observation, sys: &System) {
        self.log.push_back(obs);
        if self.log.len() > LOG_LEN {
            self.log.pop_front();
        }
        let c = self
            .contacts
            .entry(obs.contact)
            .or_insert_with(|| Contact { resolved:false, systematic_floor:None,id: obs.contact, track: None, bearings: BTreeMap::new(), last: obs });
        if obs.emitted_at >= c.last.emitted_at {
            c.last = obs;
        }
        match obs.measurement {
            Measurement::BearingRange { bearing, sigma_bearing, range, sigma_range } => {
                if matches!(obs.source, Source::Emission | Source::Echo) { c.resolved = true; }
                if c.bearings.get(&obs.sensor).is_none_or(|old|obs.emitted_at>=old.emitted_at) {
                    c.bearings.insert(obs.sensor, Observation {measurement:Measurement::Bearing {bearing,sigma:sigma_bearing},..obs});
                }
                let pos = obs.origin + Vec2::new(bearing.cos(), bearing.sin()) * range;
                let radial=crate::sensors::systematic_range(range,obs.snr,obs.source);
                let angular=crate::params::DIRECTION_SYSTEMATIC_RAD.value/obs.snr.sqrt().max(1.0);
                let cov = polar_cov(bearing, range, sigma_range.hypot(radial), sigma_bearing.hypot(angular));
                match &mut c.track {
                    Some(t) if obs.emitted_at < t.t - STALE_REPORT_S => {}
                    Some(t) => {
                        t.predict(obs.emitted_at, sys);
                        t.update_position(pos, cov);
                    }
                    None => c.track = Some(Track::new(obs.emitted_at, pos, cov)),
                }
            }
            Measurement::Bearing { bearing, sigma } => {
                if c.bearings.get(&obs.sensor).is_none_or(|old|obs.emitted_at>=old.emitted_at) {
                    c.bearings.insert(obs.sensor, obs);
                }
                match &mut c.track {
                    Some(t) if obs.emitted_at < t.t - STALE_REPORT_S => {}
                    Some(t) => {
                        t.predict(obs.emitted_at, sys);
                        let angular=crate::params::DIRECTION_SYSTEMATIC_RAD.value/obs.snr.sqrt().max(1.0);
                        t.update_bearing(obs.origin, bearing, sigma.hypot(angular));
                    }
                    None => {
                        // Try every other sensor's recent bearing; keep the tightest fix.
                        let best = c
                            .bearings
                            .values()
                            .filter(|o| {
                                let window = (o.origin - obs.origin).length() / crate::units::C + TRIANGULATION_SLACK_S;
                                o.sensor != obs.sensor && (o.emitted_at - obs.emitted_at).abs() < window
                            })
                            .filter_map(|o| match o.measurement {
                                Measurement::Bearing { bearing: b2, sigma: s2 } => {
                                    triangulate((obs.origin, bearing, sigma), (o.origin, b2, s2))
                                }
                                Measurement::BearingRange { .. } => None,
                            })
                            .min_by(|a, b| (a.1[0][0] + a.1[1][1]).total_cmp(&(b.1[0][0] + b.1[1][1])));
                        if let Some((pos, cov)) = best {
                            c.track = Some(Track::new(obs.emitted_at, pos, cov));
                        }
                    }
                }
            }
        }
        // Correlated calibration error cannot be averaged away by repeated frames.
        // Smaller/stronger-range measurements progressively lower this floor.
        if let Some(tr)=&mut c.track {
            let (bearing,range)=match obs.measurement {
                Measurement::BearingRange {bearing,range,..} => (bearing,range),
                Measurement::Bearing {bearing,..} => (bearing,(tr.pos()-obs.origin).length()),
            };
            let radial=crate::sensors::systematic_range(range,obs.snr,obs.source);
            let angular=crate::params::DIRECTION_SYSTEMATIC_RAD.value/obs.snr.sqrt().max(1.0);
            let candidate=polar_cov(bearing,range,radial,angular);
            // A weak newer report must not inflate the retained solution to that
            // report's much larger calibration uncertainty. Motion still grows P.
            let retained=c.systematic_floor.map(|(at,mut floor)| {
                let manoeuvre=0.5*crate::params::TRACK_MANEUVER_G.value*G0*(tr.t-at).max(0.0).powi(2);
                floor[0][0]+=manoeuvre.powi(2); floor[1][1]+=manoeuvre.powi(2);
                floor
            });
            let use_new=retained.is_none_or(|old|candidate[0][0]+candidate[1][1]<old[0][0]+old[1][1]);
            if use_new {
                c.systematic_floor=Some((tr.t,candidate));
            }
            let floor=if use_new {candidate} else {retained.unwrap()};
            tr.p[0][0]=tr.p[0][0].max(floor[0][0]);
            tr.p[1][1]=tr.p[1][1].max(floor[1][1]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearing_triangulation_and_ping_do_not_identify_a_platform() {
        let mut p=Perception::new(FactionId(0));
        let sys=System::default();
        let target=Vec2::new(1000.0,1000.0);
        let report=Observation {contact:ContactId(1),sensor:BodyId(0),origin:Vec2::ZERO,
            emitted_at:0.0,sensor_received_at:1.0,decider_received_at:1.0,
            measurement:Measurement::Bearing {bearing:bearing_of(target),sigma:0.001},snr:9.0,source:Source::Emission};
        p.ingest(report,&sys);
        let origin=Vec2::new(2000.0,0.0);
        p.ingest(Observation {sensor:BodyId(1),origin,
            measurement:Measurement::Bearing {bearing:bearing_of(target-origin),sigma:0.001},..report},&sys);
        assert!(p.contacts[&ContactId(1)].track.is_some());
        assert!(!p.contacts[&ContactId(1)].resolved);
        let ranged=Observation {source:Source::Ping,measurement:Measurement::BearingRange {
            bearing:bearing_of(target),sigma_bearing:0.001,range:target.length(),sigma_range:10.0},..report};
        p.ingest(ranged,&sys);
        assert!(!p.contacts[&ContactId(1)].resolved);
        p.ingest(Observation {source:Source::Echo,..ranged},&sys);
        assert!(p.contacts[&ContactId(1)].resolved);
    }

    #[test]
    fn two_bearings_triangulate() {
        let target = Vec2::new(1000.0, 1000.0);
        let a = Vec2::ZERO;
        let b = Vec2::new(2000.0, 0.0);
        let (p, cov) =
            triangulate((a, bearing_of(target - a), 1e-4), (b, bearing_of(target - b), 1e-4)).unwrap();
        assert!((p - target).length() < 1e-6);
        assert!(cov[0][0].sqrt() < 1.0);
    }

    #[test]
    fn parallel_bearings_do_not_triangulate() {
        assert!(triangulate((Vec2::ZERO, 0.0, 1e-3), (Vec2::new(0.0, 1.0), 0.0, 1e-3)).is_none());
    }

    #[test]
    fn range_fixes_converge_on_a_moving_target() {
        let sys = System::default();
        let v = Vec2::new(30.0, -10.0);
        let truth = |t: f64| Vec2::new(5e5, 2e5) + v * t;
        let r = [[100.0, 0.0], [0.0, 100.0]];
        let mut tr = Track::new(0.0, truth(0.0), r);
        for i in 1..=30 {
            let t = i as f64 * 10.0;
            tr.predict(t, &sys);
            tr.update_position(truth(t), r);
        }
        assert!((tr.vel() - v).length() < 1.0, "{:?}", tr.vel());
        assert!((tr.pos() - truth(300.0)).length() < 20.0);
    }

    #[test]
    fn bearing_updates_from_two_sensors_localise() {
        let sys = System::default();
        let target = Vec2::new(3e6, 1e6);
        let sensors = [Vec2::ZERO, Vec2::new(0.0, 6e5)];
        let mut tr = Track::new(0.0, Vec2::new(2.5e6, 1.2e6), [[1e11, 0.0], [0.0, 1e11]]);
        let mut errs = vec![];
        for i in 0..600 {
            let s = sensors[i % 2];
            tr.predict(i as f64 * 10.0, &sys);
            tr.update_bearing(s, bearing_of(target - s), 1e-5);
            errs.push((tr.pos() - target).length());
        }
        // Converges from a bad start, and ends consistent with its own covariance.
        let err = *errs.last().unwrap();
        let sigma = (tr.pos_cov()[0][0] + tr.pos_cov()[1][1]).sqrt();
        assert!(err < 1e3 && err < 4.0 * sigma + 1.0, "err {err} sigma {sigma} at 40: {}", errs[40]);
    }
}
