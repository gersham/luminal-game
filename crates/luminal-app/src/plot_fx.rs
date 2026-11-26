//! Motion and feedback on the plot: trails, echo glints, screen shimmer, the flare wash,
//! the masked zone in front of a giant, and a camera that reacts to hits and jumps.
//!
//! Presentation only. Everything is drawn from the view the player already has, and
//! timed on the wall clock so it reads the same at any warp.
use super::*;
use egui::epaint::Mesh;
use luminal_core::world::terrain;

/// Seconds of wall clock a trail keeps.
const TRAIL_S: f64 = 6.0;
const TRAIL_SAMPLE_S: f64 = 0.12;
const TRAIL_POINTS: usize = 52;
/// Past this many things on the plot, rounds stop leaving trails. Ships and contacts always do.
const TRAIL_BUDGET: usize = 90;
const GLINT_S: f64 = 0.7;
/// Least wall-clock time between two glints on one contact.
const GLINT_GAP_S: f64 = 2.0;
const SHAKE_S: f64 = 0.38;
/// Most the plot moves on a hit, px. Bursts at high warp do not stack past it.
const SHAKE_MAX_PX: f32 = 7.0;
const PULSE_S: f64 = 0.55;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Subject {
    Body(BodyId),
    Contact(ContactId),
}

/// The light of a burst or a loss we have seen keeps going at c. A shell is only made once
/// that light has reached us, so it is always at least as far out as we are: it shows how
/// far the news has spread by now, never anything we could not know.
struct LightShell {
    origin: Vec2,
    t_emit: f64,
    color: Color32,
    strength: f32,
}
/// Game seconds a shell is drawn for: about 1.8 AU of travel.
const SHELL_AGE_S: f64 = 900.0;
const SHELLS_MAX: usize = 48;

struct Trail {
    color: Color32,
    points: VecDeque<(f64, Vec2)>,
}

#[derive(Default)]
pub(super) struct PlotFx {
    trails: BTreeMap<Subject, Trail>,
    sampled: f64,
    sim: f64,
    /// Newest echo already seen per contact, and when a newer one last arrived (wall clock).
    echoes: BTreeMap<ContactId, (u64, f64)>,
    seen: BTreeSet<CombatLogKey>,
    initialized: bool,
    shake: f32,
    shake_at: f64,
    pulse_at: Option<f64>,
    shells: VecDeque<LightShell>,
}

fn key(e: &luminal_core::world::CombatEvent) -> CombatLogKey {
    (e.emitted_at.to_bits(), e.received_at.to_bits(), e.kind as u8, e.own_body.map(|id| id.0), e.contact.map(|id| id.0))
}

/// How hard a hit of this strength shakes the plot, px. Light hits on the screens do not.
pub(super) fn shake_for(strength: f32) -> f32 {
    if strength < 0.3 { 0.0 } else { (2.0 + 6.0 * strength.clamp(0.0, 1.0)).min(SHAKE_MAX_PX) }
}

impl PlotFx {
    /// Take in one frame of the view. `own` is the command ship; `visible` says whether a place is on the plot.
    pub(super) fn observe(&mut self, view: &View, own: Option<BodyId>, faction: Option<FactionId>, wall: f64, visible: impl Fn(Vec2) -> bool) {
        // A restart, or any step back in time, starts everything again.
        if view.time < self.sim { *self = Self::default(); }
        self.sim = view.time;

        for event in view.combat.iter().chain(&view.jump_events) {
            if !self.seen.insert(key(event)) || !self.initialized { continue; }
            match event.kind {
                CombatKind::Impact if own.is_some() && event.own_body == own => {
                    let strength = shake_for(event.impact_strength);
                    if strength > 0.0 {
                        // A burst of hits is one shake at the strength of its worst, not a sum.
                        let left = self.shake * (1.0 - ((wall - self.shake_at) / SHAKE_S).clamp(0.0, 1.0)) as f32;
                        self.shake = strength.max(left).min(SHAKE_MAX_PX);
                        self.shake_at = wall;
                    }
                }
                CombatKind::JumpArrival | CombatKind::JumpDeparture if event.pos.is_some_and(&visible) => self.pulse_at = Some(wall),
                _ => {}
            }
            let shell = match event.kind {
                CombatKind::Destroyed if event.subject_kind == Some(BodyKind::Ship) =>
                    Some((if event.own_body.is_some() { FRIEND } else { ENEMY }, 1.0)),
                CombatKind::NuclearBurst => Some((Color32::from_rgb(255, 214, 150), 0.75)),
                CombatKind::JumpArrival => Some((Color32::from_rgb(140, 215, 255), 0.6)),
                CombatKind::Impact if event.impact_strength >= 0.65 => Some((Color32::WHITE, 0.45)),
                _ => None,
            };
            if let (Some((color, strength)), Some(origin)) = (shell, event.pos) {
                self.shells.push_back(LightShell { origin, t_emit: event.emitted_at, color, strength });
                if self.shells.len() > SHELLS_MAX { self.shells.pop_front(); }
            }
        }
        self.shells.retain(|s| view.time - s.t_emit < SHELL_AGE_S);
        if self.seen.len() > 4096 {
            self.seen = view.combat.iter().chain(&view.jump_events).map(key).collect();
        }
        self.initialized = true;

        // A glint when a new echo of ours has actually come back from a contact.
        for c in &view.contacts {
            let stamp = c.last_received_at.to_bits();
            let echo = c.last_source == Source::Echo && c.track.is_some();
            match self.echoes.get_mut(&c.id) {
                // At high warp returns arrive many times a second. One glint, then a pause.
                Some(entry) if echo && entry.0 != stamp => *entry = (stamp, if wall - entry.1 >= GLINT_GAP_S { wall } else { entry.1 }),
                Some(_) => {}
                None => { self.echoes.insert(c.id, (stamp, f64::NEG_INFINITY)); }
            }
        }
        self.echoes.retain(|id, _| view.contacts.iter().any(|c| c.id == *id));

        if view.paused || wall - self.sampled < TRAIL_SAMPLE_S { return; }
        self.sampled = wall;
        let crowded = view.bodies.len() + view.contacts.len() > TRAIL_BUDGET;
        let mut present = BTreeSet::new();
        let mut sample = |subject: Subject, pos: Vec2, speed: f64, color: Color32, trails: &mut BTreeMap<Subject, Trail>| {
            present.insert(subject);
            let trail = trails.entry(subject).or_insert_with(|| Trail { color, points: VecDeque::new() });
            trail.color = color;
            // A jump, or a track that has been corrected a long way, is not a path between the two places.
            if let Some(&(at, last)) = trail.points.back() {
                let expected = speed * view.warp.max(1.0) * (wall - at).max(TRAIL_SAMPLE_S);
                if (pos - last).length() > 6.0 * expected + 1.0 { trail.points.clear(); }
            }
            trail.points.push_back((wall, pos));
            while trail.points.len() > TRAIL_POINTS || trail.points.front().is_some_and(|(at, _)| wall - at > TRAIL_S) { trail.points.pop_front(); }
        };
        for b in &view.bodies {
            let round = b.kind == BodyKind::Missile;
            if matches!(b.kind, BodyKind::Station | BodyKind::Probe) || b.interceptor.is_some() || (round && crowded) || b.jump.is_some() { continue; }
            sample(Subject::Body(b.id), b.pos, b.vel.length(), body_color(b, faction), &mut self.trails);
        }
        for c in &view.contacts {
            if c.stale || (c.resolved_missile && crowded) || c.resolved_interceptor { continue; }
            if let Some(track) = &c.track { sample(Subject::Contact(c.id), track.pos, track.vel.length(), CONTACT, &mut self.trails); }
        }
        self.trails.retain(|subject, _| present.contains(subject));
    }

    /// Light shells: a bright front with a soft wake inside it, fading as it spreads.
    fn draw_shells(&self, painter: &egui::Painter, cam: &Camera, rect: Rect, view: &View) {
        let reach = rect.width().hypot(rect.height());
        for shell in &self.shells {
            let age = view.time - shell.t_emit;
            if age <= 0.0 { continue; }
            let center = to_screen(cam, rect, shell.origin);
            let px = (C * age / cam.km_per_px) as f32;
            // Wholly off the plot: either it has not reached the edge, or it has swept past every corner.
            let nearest = (center - rect.clamp(center)).length();
            let farthest = [rect.left_top(), rect.right_top(), rect.left_bottom(), rect.right_bottom()].iter().map(|c| center.distance(*c)).fold(0.0, f32::max);
            if px < nearest || px > farthest + 4.0 || !px.is_finite() { continue; }
            let fade = shell.strength * (1.0 - (age / SHELL_AGE_S) as f32).powi(2) * (1.0 - (px / (3.0 * reach)).min(1.0));
            if fade <= 0.01 { continue; }
            if px < 20_000.0 {
                fx::wavefront(painter, center, px, (px * 0.06).clamp(4.0, 20.0), shell.color, 0.42 * fade);
            }
            painter.circle_stroke(center, px, Stroke::new(1.0, shell.color.gamma_multiply(0.35 * fade)));
        }
    }

    /// The camera to draw this frame with: the steady one, moved by a hit and breathed by a jump.
    pub(super) fn camera(&self, steady: Camera, wall: f64) -> Camera {
        let mut cam = steady;
        let shaking = ((wall - self.shake_at) / SHAKE_S).clamp(0.0, 1.0);
        if self.shake > 0.0 && shaking < 1.0 {
            let reach = self.shake as f64 * (1.0 - shaking).powi(2);
            // Two unrelated sines read as a jolt and never leave the plot offset.
            let offset = Vec2::new((wall * 61.0).sin(), (wall * 47.0 + 1.3).sin()) * (reach * cam.km_per_px);
            cam.center = cam.center + offset;
        }
        if let Some(at) = self.pulse_at {
            let age = ((wall - at) / PULSE_S).clamp(0.0, 1.0);
            if age < 1.0 { cam.km_per_px *= 1.0 + 0.035 * (1.0 - age).powi(2) * (age * std::f64::consts::TAU * 1.5).sin(); }
        }
        cam
    }

    /// True while something here is moving on the wall clock and wants every frame.
    pub(super) fn animating(&self, wall: f64) -> bool {
        (self.shake > 0.0 && wall - self.shake_at < SHAKE_S) || self.pulse_at.is_some_and(|at| wall - at < PULSE_S)
            || self.echoes.values().any(|(_, at)| wall - at < GLINT_S)
    }

    /// Under the symbols: the flare wash, the masked zones, and the trails.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_under(&self, painter: &egui::Painter, cam: &Camera, rect: Rect, view: &View, ship: Option<&BodyView>, star_color: Color32, wall: f64) {
        self.draw_shells(painter, cam, rect, view);
        if view.flare_until.is_some() && let Some(star) = view.celestials.first() {
            // The whole plot is lit from the star's side while the flare is on the command ship.
            let source = to_screen(cam, rect, star.pos);
            let reach = (source - rect.center()).length() + rect.width() + rect.height();
            let level = 0.085 + 0.02 * (wall * 1.1).sin() as f32;
            if reach < 18_000.0 { fx::glow(painter, source, reach, star_color, level * 2.4); }
            else {
                let mut wash = Mesh::default();
                wash.add_colored_rect(rect, fx::light(star_color, level));
                painter.add(Shape::mesh(wash));
            }
        }
        if let Some(ship) = ship {
            for (index, body) in view.celestials.iter().enumerate() {
                if view.system.bodies[index].kind != CelestialKind::Planet || body.radius <= params::GIANT_RADIUS_KM.value { continue; }
                let to_planet = body.pos - ship.pos;
                let distance = to_planet.length();
                let Some(half) = terrain::mask_half_angle(body.radius, distance) else { continue };
                let planet = to_screen(cam, rect, body.pos);
                let width_px = (distance * half.tan() / cam.km_per_px) as f32;
                // Worth drawing only when the zone has some width and its far end is near the plot.
                // Close in on the giant itself the wedge would be a wash across the whole view, so it is left off.
                let disc_px = (body.radius / cam.km_per_px) as f32;
                if width_px < 6.0 || disc_px > 40.0 || !rect.expand(width_px + 200.0).contains(planet) { continue; }
                let apex = to_screen(cam, rect, ship.pos);
                let along = planet - apex;
                let across = { let d = along.normalized(); EVec2::new(-d.y, d.x) * width_px };
                let tint = Color32::from_rgb(120, 200, 190);
                // Brightest most of the way out and gone at the planet, so the zone has no hard far edge.
                let dark = Color32::from_rgba_premultiplied(0, 0, 0, 0);
                let mut zone = Mesh::default();
                let mid = apex + along * 0.72;
                zone.colored_vertex(apex, dark);
                for (at, level) in [(mid, 0.075), (planet, 0.0)] {
                    let reach = across * ((at - apex).length() / along.length().max(1.0));
                    zone.colored_vertex(at + reach, dark);
                    zone.colored_vertex(at, fx::light(tint, level));
                    zone.colored_vertex(at - reach, dark);
                }
                for (a, b, c) in [(0, 1, 2), (0, 2, 3), (1, 4, 2), (2, 4, 5), (2, 5, 3), (3, 5, 6)] { zone.add_triangle(a, b, c); }
                painter.add(Shape::mesh(zone));
                for edge in [planet + across, planet - across] {
                    painter.extend(Shape::dashed_line(&[apex, edge], Stroke::new(1.0, tint.gamma_multiply(0.14)), 3.0, 7.0));
                }
            }
        }
        // Trails: one mesh, each segment a thin strip that fades with age.
        let mut mesh = Mesh::default();
        for trail in self.trails.values() {
            let Some(&(newest, _)) = trail.points.back() else { continue };
            let mut previous: Option<(Pos2, f32)> = None;
            for &(at, pos) in &trail.points {
                let p = to_screen(cam, rect, pos);
                let level = 0.34 * (1.0 - ((newest - at) / TRAIL_S).clamp(0.0, 1.0) as f32).powf(1.6);
                if let Some((q, before)) = previous && (rect.expand(40.0).contains(p) || rect.expand(40.0).contains(q)) {
                    let along = p - q;
                    let length = along.length();
                    if length > 0.75 && length < 4_000.0 {
                        let side = EVec2::new(-along.y, along.x) / length * 0.75;
                        let base = mesh.vertices.len() as u32;
                        for (corner, value) in [(q + side, before), (q - side, before), (p + side, level), (p - side, level)] {
                            mesh.colored_vertex(corner, fx::light(trail.color, value));
                        }
                        mesh.add_triangle(base, base + 1, base + 2);
                        mesh.add_triangle(base + 1, base + 3, base + 2);
                    }
                }
                previous = Some((p, level));
            }
        }
        if !mesh.is_empty() { painter.add(Shape::mesh(mesh)); }
    }

    /// Over the symbols: screens on the command ship and the inspected target, and echo glints.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_over(&self, painter: &egui::Painter, cam: &Camera, rect: Rect, view: &View, ship: Option<&BodyView>, inspected: Option<ContactId>, wall: f64) {
        let cyan = Color32::from_rgb(140, 215, 255);
        let shimmer = |at: Pos2, field: f32, phase: f32| {
            if field <= 0.03 || !rect.contains(at) { return; }
            fx::band(painter, at, 13.0, 19.0, cyan, 0.07 * field, |_| 1.0);
            // Three arcs turning slowly: a raised field, not a range ring.
            for arc in 0..3 {
                let start = phase + arc as f32 * std::f32::consts::TAU / 3.0;
                let points: Vec<Pos2> = (0..=10).map(|k| at + EVec2::angled(start + k as f32 * 0.085) * 16.0).collect();
                painter.add(Shape::line(points, Stroke::new(1.0, cyan.gamma_multiply(0.32 * field))));
            }
        };
        if let Some(ship) = ship && ship.screen_up {
            shimmer(to_screen(cam, rect, ship.pos), ship.thermal.field as f32, wall as f32 * 0.35);
        }
        // An enemy's screens are known only from an echo's damage report, and only as of that echo.
        if let Some(contact) = inspected.and_then(|id| view.contacts.iter().find(|c| c.id == id))
            && let (Some(track), Some(report)) = (&contact.track, &contact.damage) {
            shimmer(to_screen(cam, rect, track.pos), report.screen_available as f32, -(wall as f32) * 0.35);
        }
        for (id, (_, at)) in &self.echoes {
            let age = ((wall - at) / GLINT_S) as f32;
            if !(0.0..1.0).contains(&age) { continue; }
            let Some(track) = view.contacts.iter().find(|c| c.id == *id).and_then(|c| c.track.as_ref()) else { continue };
            let p = to_screen(cam, rect, track.pos);
            fx::glow(painter, p, 12.0 + 26.0 * age, Color32::WHITE, 0.8 * (1.0 - age).powi(2));
            painter.circle_stroke(p, 9.0 + 18.0 * age, Stroke::new(1.0, cyan.gamma_multiply(0.7 * (1.0 - age))));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_contact(pos: Vec2) -> ContactView {
        ContactView { decoy: false, identified_name: None, display_class: None, detection: luminal_core::sensors::DetectionLevel::Resolved,
            ping_remaining: 0.0, active_fire_control: 0.0, reporting_sensor: None, resolved_class: None, resolved_interceptor: false, damage: None,
            id: ContactId(1), resolved_kind: Some(BodyKind::Ship), resolved_missile: false, quality: "resolved", stale: false,
            track: Some(luminal_core::session::TrackView { velocity_sigma: 0.0, pos, vel: Vec2::ZERO, accel: Vec2::ZERO, cov: [[0.0; 2]; 2], updated_at: 0.0, updates: 4 }),
            bearings: vec![], last_emitted_at: 0.0, last_received_at: 0.0, last_source: Source::Echo, last_snr: 1e6, last_range: None }
    }

    fn scene() -> (View, BodyId) {
        let app = LuminalApp::new();
        let mut view = app.session.view(app.role);
        view.combat.clear();
        view.jump_events.clear();
        view.contacts.clear();
        view.paused = false;
        view.warp = 1.0;
        let id = view.bodies.iter().find(|b| b.controllable && b.kind == BodyKind::Ship).unwrap().id;
        (view, id)
    }

    fn event(view: &View, body: BodyId, kind: CombatKind, strength: f32, at: f64) -> luminal_core::world::CombatEvent {
        luminal_core::world::CombatEvent { weapon_visual: luminal_core::world::weapon_fit::WeaponVisual::Standard, velocity: None,
            subject_kind: Some(BodyKind::Ship), impact_strength: strength, damage: None, contact: None, target: None, aim: None,
            emitted_at: at, received_at: at, pos: view.bodies.iter().find(|b| b.id == body).map(|b| b.pos), kind, own_body: Some(body) }
    }

    #[test]
    fn a_hull_hit_jolts_the_drawn_camera_and_leaves_the_steady_one_alone() {
        let (mut view, id) = scene();
        let mut fx = PlotFx::default();
        let steady = Camera { center: Vec2::new(5.0, 5.0), km_per_px: 100.0 };
        fx.observe(&view, Some(id), None, 0.0, |_| true);
        view.combat.push(event(&view, id, CombatKind::Impact, 0.1, 1.0));
        fx.observe(&view, Some(id), None, 1.0, |_| true);
        assert_eq!(fx.camera(steady, 1.05).center, steady.center, "a hit the screens took does not move the plot");
        view.combat.push(event(&view, id, CombatKind::Impact, 1.0, 2.0));
        fx.observe(&view, Some(id), None, 2.0, |_| true);
        let reach = (0..40).map(|k| (fx.camera(steady, 2.0 + k as f64 * 0.005).center - steady.center).length() / steady.km_per_px).fold(0.0, f64::max);
        assert!(reach > 1.0 && reach <= SHAKE_MAX_PX as f64 * 1.42, "the plot moved {reach} px");
        assert!(fx.animating(2.1));
        // A burst of ten more is still one shake, no larger.
        for k in 0..10 { view.combat.push(event(&view, id, CombatKind::Impact, 1.0, 3.0 + k as f64)); }
        fx.observe(&view, Some(id), None, 2.05, |_| true);
        assert!(fx.shake <= SHAKE_MAX_PX);
        assert_eq!(fx.camera(steady, 2.05 + SHAKE_S + 0.01).center, steady.center, "and it settles back exactly");
        assert_eq!(fx.camera(steady, 9.0).km_per_px, steady.km_per_px);
        assert!(!fx.animating(9.0));
        assert_eq!((shake_for(0.29), shake_for(5.0)), (0.0, SHAKE_MAX_PX));
    }

    #[test]
    fn a_jump_on_the_plot_breathes_the_scale_and_one_off_it_does_not() {
        let (mut view, id) = scene();
        let steady = Camera { center: Vec2::ZERO, km_per_px: 100.0 };
        for (on_plot, expected) in [(false, false), (true, true)] {
            let mut fx = PlotFx::default();
            fx.observe(&view, Some(id), None, 0.0, |_| on_plot);
            view.jump_events.push(event(&view, id, CombatKind::JumpArrival, 0.0, if on_plot { 2.0 } else { 1.0 }));
            fx.observe(&view, Some(id), None, 1.0, |_| on_plot);
            let moved = (0..20).any(|k| fx.camera(steady, 1.0 + k as f64 * 0.02).km_per_px != steady.km_per_px);
            assert_eq!(moved, expected);
            assert_eq!(fx.camera(steady, 1.0 + PULSE_S + 0.01).km_per_px, steady.km_per_px);
        }
    }

    #[test]
    fn trails_follow_motion_break_on_a_jump_and_stop_while_paused() {
        let (mut view, id) = scene();
        view.warp = 1_000.0;
        let mut fx = PlotFx::default();
        let step = Vec2::new(40_000.0, 0.0);
        let speed = 300.0;
        for frame in 0..20 {
            let ship = view.bodies.iter_mut().find(|b| b.id == id).unwrap();
            ship.pos = ship.pos + step;
            ship.vel = Vec2::new(speed, 0.0);
            view.time += 1.0;
            fx.observe(&view, Some(id), None, frame as f64 * 0.13, |_| true);
        }
        let length = fx.trails[&Subject::Body(id)].points.len();
        assert!(length >= 18, "a moving ship leaves a trail, {length} points");
        // Paused: nothing is added, and what is there stays.
        view.paused = true;
        fx.observe(&view, Some(id), None, 10.0, |_| true);
        assert_eq!(fx.trails[&Subject::Body(id)].points.len(), length);
        view.paused = false;
        // A jump is not a path. The trail starts again from the far side.
        let ship = view.bodies.iter_mut().find(|b| b.id == id).unwrap();
        ship.pos = ship.pos + Vec2::new(5.0 * AU, 0.0);
        view.time += 1.0;
        fx.observe(&view, Some(id), None, 10.2, |_| true);
        assert_eq!(fx.trails[&Subject::Body(id)].points.len(), 1);
        // A restart clears everything.
        view.time = 0.0;
        fx.observe(&view, Some(id), None, 10.4, |_| true);
        assert!(fx.trails.values().all(|trail| trail.points.len() <= 1) && fx.initialized);
        let ctx = egui::Context::default();
        let rect = Rect::from_min_size(Pos2::ZERO, EVec2::new(800.0, 600.0));
        let at = view.bodies.iter().find(|b| b.id == id).unwrap().pos;
        let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(rect), ..Default::default() }, |ui| {
            let painter = ui.painter_at(rect);
            let cam = Camera { center: at, km_per_px: 5_000.0 };
            fx.draw_under(&painter, &cam, rect, &view, view.bodies.iter().find(|b| b.id == id), Color32::WHITE, 10.5);
            fx.draw_over(&painter, &cam, rect, &view, view.bodies.iter().find(|b| b.id == id), None, 10.5);
        });
        output.textures_delta.clear();
        assert!(output.shapes.iter().all(|s| match &s.shape { Shape::Mesh(mesh) => mesh.is_valid(), _ => true }));
    }

    #[test]
    fn a_glint_needs_an_echo_that_actually_came_back() {
        let (mut view, id) = scene();
        let mut fx = PlotFx::default();
        let mut contact = test_contact(view.bodies.iter().find(|b| b.id == id).unwrap().pos + Vec2::new(LIGHT_SECOND, 0.0));
        contact.last_source = Source::Emission;
        view.contacts.push(contact);
        fx.observe(&view, Some(id), None, 0.0, |_| true);
        assert!(!fx.animating(0.1), "a contact already in the picture does not glint");
        view.contacts[0].last_received_at = 5.0;
        fx.observe(&view, Some(id), None, 1.0, |_| true);
        assert!(!fx.animating(1.1), "passive light is not an echo");
        view.contacts[0].last_source = Source::Echo;
        view.contacts[0].last_received_at = 6.0;
        fx.observe(&view, Some(id), None, 2.0, |_| true);
        assert!(fx.animating(2.1));
        fx.observe(&view, Some(id), None, 2.2, |_| true);
        assert!(fx.animating(2.3) && !fx.animating(2.0 + GLINT_S + 0.01), "the same echo glints once");
        // Returns every frame, as at high warp, are one glint and then a pause.
        for k in 0..20 {
            view.contacts[0].last_received_at = 7.0 + k as f64;
            fx.observe(&view, Some(id), None, 2.3 + k as f64 * 0.05, |_| true);
        }
        assert!(!fx.animating(2.0 + GLINT_S + 0.2), "a stream of echoes does not hold the glint on");
        view.contacts[0].last_received_at = 99.0;
        fx.observe(&view, Some(id), None, 2.0 + GLINT_GAP_S + 0.1, |_| true);
        assert!(fx.animating(2.0 + GLINT_GAP_S + 0.2));
    }
}
