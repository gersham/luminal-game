//! Places in a system that change what sensors can do: a giant planet's disc behind a
//! target, the star's glare behind it, a debris field around it, and a stellar flare
//! washing over the sensor.
//!
//! None of it is an extra detection roll. Each lowers the level an existing look
//! returns, from geometry the sensing ship could work out for itself.
use super::*;
use crate::celestial::{CelestialKind, Orbit};
use crate::sensors::DetectionLevel;

/// A cloud of rock and ice. An active return from inside it is one echo among many.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DebrisField {
    /// Fixed in the system frame, km. A belt's drift over a battle is not worth modelling.
    pub center: Vec2,
    pub radius: f64,
}

/// One stellar flare: a front leaving the star at `t_emit` that dazzles passive arrays for `duration`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Flare {
    pub t_emit: f64,
    pub duration: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Terrain {
    pub fields: Vec<DebrisField>,
    pub flares: Vec<Flare>,
}

/// One step down the ladder of what a look returns.
pub fn degrade(level: DetectionLevel) -> DetectionLevel {
    match level {
        DetectionLevel::Identity => DetectionLevel::Resolved,
        DetectionLevel::Resolved => DetectionLevel::Approximate,
        DetectionLevel::Approximate => DetectionLevel::Bearing,
        DetectionLevel::Bearing | DetectionLevel::None => DetectionLevel::None,
    }
}

impl Terrain {
    /// Two fields in the gaps between planetary orbits, placed by the scenario seed.
    /// Works from orbit radii alone, so every star system gets fields that suit it.
    pub fn generate(system: &System, seed: u64) -> Self {
        let mut orbits: Vec<f64> = system.bodies.iter().filter(|b| b.kind == CelestialKind::Planet)
            .filter_map(|b| match b.orbit { Orbit::Circular { parent: 0, radius, .. } | Orbit::Frozen { parent: 0, radius, .. } => Some(radius), _ => None })
            .collect();
        orbits.sort_by(f64::total_cmp);
        let mut rng = Rng::stream(seed, 0x5445_5252);
        let mut fields = vec![];
        let gaps: Vec<(f64, f64)> = orbits.windows(2).map(|pair| (pair[0], pair[1])).filter(|(inner, outer)| outer - inner > DEBRIS_MIN_GAP_AU.value * AU).collect();
        // The first wide gap and the third: one field in the inner system, one further out.
        for (inner, outer) in gaps.iter().copied().step_by(2).take(DEBRIS_FIELDS.value as usize) {
            let angle = std::f64::consts::TAU * rng.uniform();
            let orbit = 0.5 * (inner + outer);
            let radius = (DEBRIS_RADIUS_AU.value * AU).min(0.3 * (outer - inner));
            fields.push(DebrisField { center: Vec2::new(angle.cos(), angle.sin()) * orbit, radius });
        }
        Self { fields, flares: vec![] }
    }

    pub fn in_debris(&self, pos: Vec2) -> bool {
        self.fields.iter().any(|field| (pos - field.center).length() < field.radius)
    }
}

/// A planet large and warm enough to hide a drive against. The same threshold the map uses for a giant.
fn giant(body: &crate::celestial::Celestial) -> bool {
    body.kind == CelestialKind::Planet && body.radius > GIANT_RADIUS_KM.value
}

/// Half-angle of the zone a giant of `radius` masks, seen from `distance` away. None from on or inside it.
pub fn mask_half_angle(radius: f64, distance: f64) -> Option<f64> {
    (distance > radius).then(|| GIANT_MASK_RADII.value * (radius / distance).asin())
}

/// How a passive look at a target against the star fares: lost in the glare, or dimmed by
/// scattered light. Separation is in apparent stellar radii as the sensor sees the star.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Glare { Lost, Dimmed }

pub fn glare_for(separation_radii: f64) -> Option<Glare> {
    if separation_radii < SOLAR_GLARE_CORE_RADII.value { Some(Glare::Lost) }
    else if separation_radii < SOLAR_GLARE_HALO_RADII.value { Some(Glare::Dimmed) }
    else { None }
}

/// Separation of `target` from the star's direction as seen from `sensor`, in apparent stellar
/// radii, when the star lies behind the target. Pure geometry: a side can run it on its own
/// estimates, to ask whether it is hidden against the star from where it thinks an enemy is.
pub fn star_separation(system: &System, sensor: Vec2, target: Vec2, t: f64) -> Option<f64> {
    let line = target - sensor;
    let range = line.length();
    if range <= 0.0 { return None; }
    system.bodies.iter().enumerate().filter(|(_, b)| b.kind == CelestialKind::Star).filter_map(|(i, b)| {
        let to_star = system.state(i, t).pos - sensor;
        let distance = to_star.length();
        if distance <= range || distance <= b.radius { return None; }
        let separation = (line.dot(to_star) / (range * distance)).clamp(-1.0, 1.0).acos();
        Some(separation / (b.radius / distance).asin())
    }).reduce(f64::min)
}

/// Half-angle of the glare zone, radians, for a star of `radius` seen from `distance`.
pub fn glare_half_angle(radius: f64, distance: f64) -> Option<f64> {
    (distance > radius).then(|| (SOLAR_GLARE_HALO_RADII.value * (radius / distance).asin()).min(std::f64::consts::FRAC_PI_2))
}

impl World {
    /// The target sits against the star as this sensor sees it, with the star behind it:
    /// its separation from the star's direction, in apparent stellar radii.
    pub fn against_star(&self, sensor: Vec2, target: Vec2, t: f64) -> Option<f64> {
        star_separation(&self.system, sensor, target, t)
    }

    /// The target sits in front of a giant's disc as this sensor sees it: inside a few disc
    /// radii of the planet's direction, with the planet behind it.
    pub fn against_giant(&self, sensor: Vec2, target: Vec2, t: f64) -> bool {
        let line = target - sensor;
        let range = line.length();
        if range <= 0.0 { return false; }
        self.system.bodies.iter().enumerate().filter(|(_, b)| giant(b)).any(|(i, b)| {
            let to_planet = self.system.state(i, t).pos - sensor;
            let distance = to_planet.length();
            if distance <= range || distance <= b.radius { return false; }
            let separation = (line.dot(to_planet) / (range * distance)).clamp(-1.0, 1.0).acos();
            mask_half_angle(b.radius, distance).is_some_and(|half| separation < half)
        })
    }

    /// When a flare's light is on a sensor at `pos`: start and end, if the star is not eclipsed there.
    pub fn flare_window(&self, flare: &Flare, pos: Vec2) -> Option<(f64, f64)> {
        let star = self.system.bodies.iter().position(|b| b.kind == CelestialKind::Star)?;
        let source = self.system.state(star, flare.t_emit).pos;
        let arrival = flare.t_emit + (pos - source).length() / crate::units::C;
        // A planet between the star and the sensor is shade from the flare as from anything else.
        if self.system.occluder(source, flare.t_emit, pos, arrival).is_some() { return None; }
        Some((arrival, arrival + flare.duration))
    }

    /// A flare is dazzling a sensor at `pos` at time `t`. Returns when it ends.
    pub fn flared(&self, pos: Vec2, t: f64) -> Option<f64> {
        self.terrain.flares.iter().filter_map(|flare| self.flare_window(flare, pos))
            .filter(|(start, end)| t >= *start && t < *end).map(|(_, end)| end).reduce(f64::max)
    }

    /// What terrain leaves of a look that would otherwise return `level`.
    ///
    /// Passive: a target against a giant's disc gives a bearing at best; one against the star
    /// is lost in its glare close in and loses a level in the halo; and a flared array loses
    /// one level on everything. Active: a target inside debris gives an approximate
    /// position at best. A missile seeker is too close to its target for any of it.
    pub(super) fn through_terrain(&self, level: DetectionLevel, sensor: BodyId, sensor_pos: Vec2, target_pos: Vec2, t: f64, ping: bool) -> DetectionLevel {
        if level == DetectionLevel::None || self.bodies[sensor.0 as usize].missile.is_some() { return level; }
        if ping {
            return if self.terrain.in_debris(target_pos) { level.min(DetectionLevel::Approximate) } else { level };
        }
        let mut level = level;
        if self.against_giant(sensor_pos, target_pos, t) { level = level.min(DetectionLevel::Bearing); }
        match self.against_star(sensor_pos, target_pos, t).and_then(glare_for) {
            Some(Glare::Lost) => return DetectionLevel::None,
            Some(Glare::Dimmed) => level = degrade(level),
            None => {}
        }
        if self.flared(sensor_pos, t).is_some() { level = degrade(level); }
        level
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::home_system;

    #[test]
    fn each_step_down_loses_one_level_and_nothing_goes_below_none() {
        use DetectionLevel as D;
        assert_eq!([D::Identity, D::Resolved, D::Approximate, D::Bearing, D::None].map(degrade), [D::Resolved, D::Approximate, D::Bearing, D::None, D::None]);
    }

    #[test]
    fn fields_sit_between_orbits_and_follow_the_seed() {
        let system = home_system();
        let terrain = Terrain::generate(&system, 7);
        assert_eq!(terrain.fields.len(), DEBRIS_FIELDS.value as usize);
        assert_eq!(terrain, Terrain::generate(&system, 7));
        assert_ne!(terrain.fields[0].center, Terrain::generate(&system, 8).fields[0].center);
        for field in &terrain.fields {
            // Clear of every planet's orbit, so no world sits inside its own clutter.
            for body in system.bodies.iter().filter(|b| b.kind == CelestialKind::Planet) {
                if let Orbit::Circular { parent: 0, radius, .. } | Orbit::Frozen { parent: 0, radius, .. } = body.orbit {
                    assert!((field.center.length() - radius).abs() > field.radius, "{} runs through a field", body.name);
                }
            }
            assert!(terrain.in_debris(field.center) && !terrain.in_debris(field.center + Vec2::new(field.radius * 1.01, 0.0)));
        }
        assert!(Terrain::generate(&System::default(), 7).fields.is_empty(), "an empty system has no gaps to fill");
    }

    fn giant_world() -> (World, usize, Vec2) {
        let system = home_system();
        let index = system.bodies.iter().position(giant).expect("the home system has a giant");
        let at = system.state(index, 0.0).pos;
        let world = World::new(system, vec![BodySpec { name: "Watcher".into(), kind: BodyKind::Ship, faction: FactionId(0),
            state: State { pos: at + Vec2::new(-0.5 * AU, 0.0), vel: Vec2::ZERO }, thrust: Vec2::ZERO, magazine: 1 }], 10.0, 3);
        (world, index, at)
    }

    #[test]
    fn a_giant_behind_the_target_hides_it_and_one_in_front_or_aside_does_not() {
        let (w, index, at) = giant_world();
        let radius = w.system.bodies[index].radius;
        let sensor = at + Vec2::new(-0.5 * AU, 0.0);
        let across = Vec2::new(0.0, 1.0);
        assert!(w.against_giant(sensor, at + Vec2::new(-0.1 * AU, 0.0), 0.0), "on the line to the planet, in front of it");
        assert!(w.against_giant(sensor, at + Vec2::new(-0.1 * AU, 0.0) + across * (2.0 * radius * 0.8), 0.0), "inside the masking disc");
        assert!(!w.against_giant(sensor, at + Vec2::new(-0.1 * AU, 0.0) + across * (GIANT_MASK_RADII.value * radius * 1.2), 0.0), "outside it");
        assert!(!w.against_giant(sensor, at + Vec2::new(0.2 * AU, 0.0), 0.0), "behind the planet is occlusion's business, not this rule's");
        assert!(!w.against_giant(sensor, sensor + Vec2::new(-0.1 * AU, 0.0), 0.0), "looking the other way");
        use DetectionLevel as D;
        let hidden = at + Vec2::new(-0.1 * AU, 0.0);
        assert_eq!(w.through_terrain(D::Identity, BodyId(0), sensor, hidden, 0.0, false), D::Bearing);
        assert_eq!(w.through_terrain(D::Identity, BodyId(0), sensor, hidden, 0.0, true), D::Identity, "an active return is not masked by warmth");
        assert_eq!(w.through_terrain(D::None, BodyId(0), sensor, hidden, 0.0, false), D::None);
    }

    #[test]
    fn the_star_behind_a_target_hides_it_close_in_and_dims_it_in_the_halo() {
        let (w, _, _) = giant_world();
        let star = w.system.state(0, 0.0).pos;
        let radius = w.system.bodies[0].radius;
        let sensor = star + Vec2::new(-1.0 * AU, 0.0);
        let to_star = (star - sensor).normalized();
        let across = Vec2::new(-to_star.y, to_star.x);
        let apparent = (radius / AU).asin();
        // A target 0.3 AU sunward of the sensor, offset sideways to a chosen separation.
        let at = |radii: f64| sensor + to_star * (0.3 * AU) + across * (0.3 * AU * (radii * apparent).tan());
        use DetectionLevel as D;
        assert_eq!(w.through_terrain(D::Identity, BodyId(0), sensor, at(2.0), 0.0, false), D::None, "lost in the glare");
        assert_eq!(w.through_terrain(D::Identity, BodyId(0), sensor, at(10.0), 0.0, false), D::Resolved, "dimmed in the halo");
        assert_eq!(w.through_terrain(D::Identity, BodyId(0), sensor, at(30.0), 0.0, false), D::Identity, "clear of it");
        assert_eq!(w.through_terrain(D::Identity, BodyId(0), sensor, at(2.0), 0.0, true), D::Identity, "a ping is not blinded by starlight");
        // The same direction, but the target is beyond the star: the star is not behind it.
        assert_eq!(w.against_star(sensor, star + to_star * (0.5 * AU), 0.0), None);
        // Looking away from the star.
        assert_eq!(w.through_terrain(D::Identity, BodyId(0), sensor, sensor - to_star * (0.3 * AU), 0.0, false), D::Identity);
        // Farther out the star is smaller, so the same angle is more radii and the zone narrower.
        let far = star + Vec2::new(-5.0 * AU, 0.0);
        assert!(glare_half_angle(radius, 5.0 * AU).unwrap() < glare_half_angle(radius, AU).unwrap());
        assert!(w.against_star(far, far + to_star * (0.3 * AU) + across * (0.3 * AU * (10.0 * apparent).tan()), 0.0).unwrap() > 18.0);
    }

    #[test]
    fn debris_clutters_a_ping_and_leaves_passive_looks_alone() {
        let (mut w, _, at) = giant_world();
        let far = at + Vec2::new(-3.0 * AU, 2.0 * AU);
        w.terrain.fields = vec![DebrisField { center: far, radius: 0.2 * AU }];
        let sensor = far + Vec2::new(0.0, AU);
        use DetectionLevel as D;
        assert_eq!(w.through_terrain(D::Identity, BodyId(0), sensor, far, 0.0, true), D::Approximate);
        assert_eq!(w.through_terrain(D::Bearing, BodyId(0), sensor, far, 0.0, true), D::Bearing, "clutter caps a return, it does not improve one");
        assert_eq!(w.through_terrain(D::Identity, BodyId(0), sensor, far, 0.0, false), D::Identity);
        assert_eq!(w.through_terrain(D::Identity, BodyId(0), sensor, far + Vec2::new(0.3 * AU, 0.0), 0.0, true), D::Identity);
    }

    #[test]
    fn a_flare_reaches_each_sensor_at_light_speed_and_shade_is_shelter() {
        let (mut w, index, at) = giant_world();
        w.terrain.flares.push(Flare { t_emit: 100.0, duration: 600.0 });
        let star = w.system.state(0, 0.0).pos;
        let near = star + Vec2::new(0.0, AU);
        let delay = AU / crate::units::C;
        assert_eq!(w.flared(near, 100.0 + delay - 1.0), None, "the front has not arrived");
        assert_eq!(w.flared(near, 100.0 + delay + 1.0), Some(100.0 + delay + 600.0));
        assert_eq!(w.flared(near, 100.0 + delay + 601.0), None);
        // Directly behind the giant, the star is eclipsed and the flare never arrives.
        let radius = w.system.bodies[index].radius;
        let shade = at + (at - star).normalized() * (radius * 3.0);
        let radial = (at - star).normalized();
        let lit = shade + Vec2::new(-radial.y, radial.x) * (radius * 40.0);
        let when = 100.0 + (shade - star).length() / crate::units::C + 10.0;
        assert_eq!(w.flared(shade, when), None);
        assert!(w.flared(lit, when).is_some());
        use DetectionLevel as D;
        let target = near + Vec2::new(0.01 * AU, 0.0);
        assert_eq!(w.through_terrain(D::Resolved, BodyId(0), near, target, 100.0 + delay + 1.0, false), D::Approximate);
        assert_eq!(w.through_terrain(D::Resolved, BodyId(0), near, target, 100.0 + delay + 1.0, true), D::Resolved, "a ping is not dazzled");
        assert_eq!(w.through_terrain(D::Bearing, BodyId(0), near, target, 100.0 + delay + 1.0, false), D::None);
    }
}
