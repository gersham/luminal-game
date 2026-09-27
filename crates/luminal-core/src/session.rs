//! The only interface a client (local UI, remote player, bot) has to a running game.
//!
//! A session owns world truth. Clients send `Command`s and receive `View`s built for
//! their `Role`. A faction view is built from that faction's `Perception` only, so it
//! is safe to send over a network; only the spectator role ever receives truth.

use crate::celestial::{CelestialKind, System};
use crate::kinematics::Vec2;
use crate::mind::{ContactId, Measurement, Source};
pub use crate::missile::{Payload, Phase};
use crate::params;
use crate::units::G0;
use crate::autopilot::Avoidance;
use crate::world::{Alert, AlertKind, Autopilot, BodyKind, FactionId, LossCause, Objective, OrderError, Outcome, World};
use std::collections::BTreeMap;

pub use crate::world::{AutopilotStatus, BodyId, InterceptTarget, Order};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Faction(FactionId),
    /// Omniscient. For local exploration, AI-versus-AI and replays; never granted to a
    /// competitive network player.
    Spectator,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Constant thrust from now on, km/s². Gravity acts in addition. Cancels any
    /// autopilot order.
    SetThrust { body: BodyId, thrust: Vec2 },
    /// Settle into a convenient orbit about celestial body `celestial` (index).
    Orbit { body: BodyId, celestial: usize },
    /// Close on a target, match velocity and hold station.
    Intercept { body: BodyId, target: InterceptTarget },
    /// Fly to a point in minimal time (burn, flip, brake) and stop there.
    MoveTo { body: BodyId, point: Vec2 },
    /// Come to rest in the local frame as fast as the drive allows.
    AllStop { body: BodyId },
    /// Cap autopilot thrust, in g. Lower thrust means a fainter drive signature.
    SetDriveLimit { body: BodyId, g: f64 },
    /// Launch a missile at a tracked contact.
    Launch { body: BodyId, target: ContactId, payload: Payload },
    /// Ping once per sensor frame. Pings are visible far beyond their echo range.
    SetActiveSensor { body: BodyId, on: bool },
    SetWarp(f64),
    SetPaused(bool),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Rejection {
    UnknownBody,
    NotYourBody,
    Destroyed,
    ExceedsMaxAccel { requested_g: f64, max_g: f64 },
    SpectatorCannotCommand,
    /// Intercept needs a track; a bearing alone gives no range.
    NoTrack,
    InvalidTarget,
    EmptyMagazine,
}

impl From<OrderError> for Rejection {
    fn from(e: OrderError) -> Self {
        match e {
            OrderError::Destroyed => Rejection::Destroyed,
            OrderError::NoTrack => Rejection::NoTrack,
            OrderError::InvalidTarget => Rejection::InvalidTarget,
            OrderError::EmptyMagazine => Rejection::EmptyMagazine,
        }
    }
}

/// A body whose state the viewer knows exactly: its own, or anything for the spectator.
#[derive(Clone, Debug)]
pub struct BodyView {
    pub id: BodyId,
    pub name: String,
    pub kind: BodyKind,
    pub faction: FactionId,
    pub pos: Vec2,
    pub vel: Vec2,
    /// Thrust actually applied now, km/s².
    pub thrust: Vec2,
    /// Manual thrust order (used when no autopilot order is active).
    pub commanded: Vec2,
    pub autopilot: Option<Autopilot>,
    pub avoidance: Avoidance,
    /// Cap on autopilot thrust, km/s² (infinite when unset).
    pub drive_limit: f64,
    pub active_sensor: bool,
    pub magazine: u32,
    pub missile: Option<MissileView>,
}

/// A missile's own status, as its faction knows it.
#[derive(Clone, Copy, Debug)]
pub struct MissileView {
    pub payload: Payload,
    pub target: ContactId,
    pub phase: Phase,
    pub dv_left: f64,
}

#[derive(Clone, Debug)]
pub struct TrackView {
    /// Estimate propagated to view time.
    pub pos: Vec2,
    pub vel: Vec2,
    /// Estimated thrust, km/s².
    pub accel: Vec2,
    /// Position covariance at view time, km².
    pub cov: [[f64; 2]; 2],
    /// Emission time of the newest measurement folded in.
    pub updated_at: f64,
    pub updates: u32,
}

#[derive(Clone, Debug)]
pub struct BearingView {
    pub sensor: BodyId,
    pub origin: Vec2,
    pub bearing: f64,
    pub sigma: f64,
    pub emitted_at: f64,
}

/// Something the faction has sensed. Identity and truth are not included.
#[derive(Clone, Debug)]
pub struct ContactView {
    pub id: ContactId,
    pub track: Option<TrackView>,
    pub bearings: Vec<BearingView>,
    pub last_emitted_at: f64,
    pub last_received_at: f64,
    pub last_source: Source,
    pub last_snr: f64,
    pub last_range: Option<f64>,
}

#[derive(Clone, Debug)]
pub struct CelestialView {
    pub name: String,
    pub kind: CelestialKind,
    pub pos: Vec2,
    pub radius: f64,
}

#[derive(Clone, Debug)]
pub struct LossView {
    pub body: BodyId,
    pub name: String,
    pub t: f64,
    pub cause: String,
}

#[derive(Clone, Debug)]
pub struct View {
    pub time: f64,
    pub role: Role,
    pub warp: f64,
    pub paused: bool,
    pub bodies: Vec<BodyView>,
    pub contacts: Vec<ContactView>,
    pub celestials: Vec<CelestialView>,
    /// Losses the viewer knows of. PLACEHOLDER: a faction learns of its own losses
    /// instantly; this should arrive by light like everything else.
    pub losses: Vec<LossView>,
    /// Public ephemeris, for display forecasts.
    pub system: System,
    /// The scenario goal, known to every side.
    pub objective: Option<Objective>,
    /// Referee's verdict once the game is decided.
    pub outcome: Option<Outcome>,
}

pub struct LocalSession {
    world: World,
    warp: f64,
    paused: bool,
    /// Whose alerts drop the warp (the local player's faction).
    watch: Option<FactionId>,
    last_alert: Option<(f64, String)>,
}

/// Warp that alerts drop to.
pub const ALERT_WARP: f64 = 1.0;

impl LocalSession {
    pub fn new(world: World) -> Self {
        Self { world, warp: 1.0, paused: true, watch: None, last_alert: None }
    }

    /// Advance by elapsed wall-clock seconds, scaled by warp. If the watched faction
    /// perceives something needing attention, time stops there and warp drops; the game
    /// ending pauses it.
    pub fn tick(&mut self, wall_dt: f64) {
        if self.paused {
            return;
        }
        let t = self.world.time() + wall_dt * self.warp;
        if let Some(alert) = self.world.advance_until_alert(t, self.watch) {
            self.last_alert = Some((alert.t, self.describe(&alert)));
            self.warp = self.warp.min(ALERT_WARP);
            if alert.kind == AlertKind::GameOver {
                self.paused = true;
            }
        }
    }

    /// The faction whose perceived events should drop the warp; `None` for none.
    pub fn set_watch(&mut self, f: Option<FactionId>) {
        self.watch = f;
    }

    /// The most recent alert that dropped the warp, with its time.
    pub fn last_alert(&self) -> Option<&(f64, String)> {
        self.last_alert.as_ref()
    }

    fn describe(&self, a: &Alert) -> String {
        let name = |id: BodyId| self.world.body(id).map_or("?".to_string(), |b| b.name.clone());
        match &a.kind {
            AlertKind::NewContact(c) => format!("New contact C{}", c.0),
            AlertKind::ShipLost(b) => format!("{} lost", name(*b)),
            AlertKind::CollisionWarning(b) => format!("{}: collision avoidance engaged", name(*b)),
            AlertKind::CollisionUnavoidable(b) => format!("{}: collision unavoidable", name(*b)),
            AlertKind::OrderComplete(b) => format!("{}: order complete", name(*b)),
            AlertKind::GameOver => match &self.world.outcome {
                Some(o) => format!("Game over: {}", o.reason),
                None => "Game over".into(),
            },
        }
    }

    pub fn command(&mut self, role: Role, cmd: Command) -> Result<(), Rejection> {
        match cmd {
            Command::SetWarp(w) => self.warp = w.clamp(0.0, 1e6),
            Command::SetPaused(p) => self.paused = p,
            Command::SetThrust { body, thrust } => {
                let kind = self.owned(role, body)?;
                let max_g = match kind {
                    BodyKind::Ship => params::SHIP_MAX_ACCEL_G.value,
                    BodyKind::Probe => params::PROBE_MAX_ACCEL_G.value,
                    BodyKind::Missile => params::MISSILE_MAX_ACCEL_G.value,
                };
                let requested_g = thrust.length() / G0;
                if requested_g > max_g * (1.0 + 1e-9) {
                    return Err(Rejection::ExceedsMaxAccel { requested_g, max_g });
                }
                self.world.set_thrust(body, thrust)?;
            }
            Command::Orbit { body, celestial } => {
                self.owned(role, body)?;
                self.world.set_orbit(body, celestial)?;
            }
            Command::Intercept { body, target } => {
                self.owned(role, body)?;
                self.world.set_intercept(body, target)?;
            }
            Command::MoveTo { body, point } => {
                self.owned(role, body)?;
                self.world.set_move(body, point)?;
            }
            Command::AllStop { body } => {
                self.owned(role, body)?;
                self.world.set_all_stop(body)?;
            }
            Command::SetDriveLimit { body, g } => {
                self.owned(role, body)?;
                self.world.set_drive_limit(body, g * G0)?;
            }
            Command::Launch { body, target, payload } => {
                self.owned(role, body)?;
                self.world.launch(body, target, payload)?;
            }
            Command::SetActiveSensor { body, on } => {
                self.owned(role, body)?;
                if !self.world.set_active_sensor(body, on) {
                    return Err(Rejection::Destroyed);
                }
            }
        }
        Ok(())
    }

    fn owned(&self, role: Role, body: BodyId) -> Result<BodyKind, Rejection> {
        let Role::Faction(faction) = role else {
            return Err(Rejection::SpectatorCannotCommand);
        };
        let b = self.world.body(body).ok_or(Rejection::UnknownBody)?;
        if b.faction != faction {
            return Err(Rejection::NotYourBody);
        }
        Ok(b.kind)
    }

    pub fn view(&self, role: Role) -> View {
        let w = &self.world;
        let t = w.time();
        let visible = |f: FactionId| match role {
            Role::Spectator => true,
            Role::Faction(me) => me == f,
        };
        let bodies = w
            .bodies
            .iter()
            .enumerate()
            .filter(|(_, b)| visible(b.faction))
            .filter_map(|(i, b)| {
                let s = b.trajectory.state_at(t)?;
                Some(BodyView {
                    id: BodyId(i as u32),
                    name: b.name.clone(),
                    kind: b.kind,
                    faction: b.faction,
                    pos: s.pos,
                    vel: s.vel,
                    thrust: b.trajectory.thrust_at(t).unwrap_or(Vec2::ZERO),
                    commanded: b.commanded,
                    autopilot: b.autopilot,
                    avoidance: b.avoidance,
                    drive_limit: b.drive_limit,
                    active_sensor: b.active_sensor,
                    magazine: b.magazine,
                    missile: b.missile.map(|m| MissileView { payload: m.payload, target: m.target, phase: m.phase, dv_left: m.dv_left }),
                })
            })
            .collect();

        let contacts = match role {
            Role::Spectator => vec![],
            Role::Faction(f) => w.perception(f).map(|p| {
                p.contacts
                    .values()
                    .map(|c| {
                        let track = c.track.as_ref().map(|tr| {
                            let now = tr.at(t, &w.system);
                            TrackView { pos: now.pos(), vel: now.vel(), accel: now.accel(), cov: now.pos_cov(), updated_at: tr.t, updates: tr.updates }
                        });
                        let bearings = c
                            .bearings
                            .values()
                            .filter_map(|o| match o.measurement {
                                Measurement::Bearing { bearing, sigma } => Some(BearingView {
                                    sensor: o.sensor,
                                    origin: o.origin,
                                    bearing,
                                    sigma,
                                    emitted_at: o.emitted_at,
                                }),
                                Measurement::BearingRange { .. } => None,
                            })
                            .collect();
                        let last_range = match c.last.measurement {
                            Measurement::BearingRange { range, .. } => Some(range),
                            Measurement::Bearing { .. } => None,
                        };
                        ContactView {
                            id: c.id,
                            track,
                            bearings,
                            last_emitted_at: c.last.emitted_at,
                            last_received_at: c.last.decider_received_at,
                            last_source: c.last.source,
                            last_snr: c.last.snr,
                            last_range,
                        }
                    })
                    .collect()
            }).unwrap_or_default(),
        };

        let celestials = w
            .system
            .bodies
            .iter()
            .enumerate()
            .map(|(i, c)| CelestialView { name: c.name.clone(), kind: c.kind, pos: w.system.state(i, t).pos, radius: c.radius })
            .collect();

        let losses = w
            .losses
            .iter()
            .filter(|l| l.cause != LossCause::Expended && w.body(l.body).is_some_and(|b| visible(b.faction)))
            .map(|l| LossView {
                body: l.body,
                name: w.body(l.body).map(|b| b.name.clone()).unwrap_or_default(),
                t: l.t,
                cause: match l.cause {
                    LossCause::Impact(i) => format!("hit {}", w.system.bodies[i].name),
                    LossCause::Missile { payload, missile } => {
                        format!("{} missile {}", payload.name(), w.body(missile).map_or("?".into(), |m| m.name.clone()))
                    }
                    LossCause::Expended => "expended".into(),
                },
            })
            .collect();

        View {
            time: t,
            role,
            warp: self.warp,
            paused: self.paused,
            bodies,
            contacts,
            celestials,
            losses,
            system: w.system.clone(),
            objective: w.objective.clone(),
            outcome: w.outcome.clone(),
        }
    }

    /// Which body each of a faction's contacts really is. Truth: spectator only.
    pub fn contact_truth(&self, role: Role, faction: FactionId) -> Option<BTreeMap<ContactId, BodyId>> {
        (role == Role::Spectator).then(|| self.world.contact_truth(faction))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::{self, ESCORT, RAIDER};

    fn running(secs: f64) -> LocalSession {
        let mut s = LocalSession::new(scenario::transport_intercept());
        s.command(Role::Spectator, Command::SetPaused(false)).unwrap();
        s.tick(secs);
        s
    }

    #[test]
    fn faction_view_holds_own_ships_and_contacts_only() {
        let s = running(60.0);
        let v = s.view(Role::Faction(ESCORT));
        assert!(v.bodies.iter().all(|b| b.faction == ESCORT));
        assert_eq!(v.bodies.len(), 2);
        let c = v.contacts.first().expect("burning cruiser is detected at once");
        assert!(c.last_emitted_at < v.time - 100.0, "its light is old");
        assert!(c.track.is_some(), "two escorts triangulate it");
    }

    #[test]
    fn escort_track_on_cruiser_is_statistically_honest() {
        // The estimate must stay within a few sigma of truth as the engagement runs.
        let mut s = running(0.0);
        for _ in 0..16 {
            s.tick(1800.0);
            let v = s.view(Role::Faction(ESCORT));
            let truth = s.view(Role::Spectator);
            let assoc = s.contact_truth(Role::Spectator, ESCORT).unwrap();
            for c in &v.contacts {
                let (Some(t), Some(id)) = (&c.track, assoc.get(&c.id)) else { continue };
                let Some(real) = truth.bodies.iter().find(|b| b.id == *id) else { continue };
                let err = (t.pos - real.pos).length();
                let sigma = (t.cov[0][0] + t.cov[1][1]).sqrt();
                assert!(err < 4.0 * sigma + 100.0, "T+{} err {err} km, sigma {sigma} km", v.time);
            }
        }
    }

    #[test]
    fn a_perceived_event_drops_the_warp() {
        let mut s = LocalSession::new(scenario::transport_intercept());
        s.set_watch(Some(ESCORT));
        s.command(Role::Faction(ESCORT), Command::Orbit { body: BodyId(0), celestial: 1 }).unwrap();
        s.command(Role::Spectator, Command::SetWarp(100_000.0)).unwrap();
        s.command(Role::Spectator, Command::SetPaused(false)).unwrap();
        s.tick(10.0); // a million seconds requested
        let v = s.view(Role::Faction(ESCORT));
        assert_eq!(v.warp, ALERT_WARP);
        assert!(v.time < 1e6, "stopped at the event, not the end of the tick: {}", v.time);
        assert_eq!(s.last_alert().unwrap().1, "Transport: order complete");
    }

    #[test]
    fn spectator_sees_truth_and_may_ask_for_associations() {
        let s = running(60.0);
        let v = s.view(Role::Spectator);
        assert_eq!(v.bodies.len(), 3);
        assert_eq!(v.celestials.len(), 3);
        assert!(s.contact_truth(Role::Faction(ESCORT), ESCORT).is_none());
        assert!(s.contact_truth(Role::Spectator, ESCORT).is_some());
    }

    #[test]
    fn cannot_command_enemy_or_exceed_max_accel() {
        let mut s = running(1.0);
        let me = Role::Faction(ESCORT);
        let cruiser = BodyId(2);
        assert_eq!(s.command(me, Command::SetThrust { body: cruiser, thrust: Vec2::ZERO }), Err(Rejection::NotYourBody));
        let too_hard = Vec2::new(101.0 * G0, 0.0);
        assert!(matches!(
            s.command(me, Command::SetThrust { body: BodyId(0), thrust: too_hard }),
            Err(Rejection::ExceedsMaxAccel { .. })
        ));
        let _ = RAIDER;
    }
}
