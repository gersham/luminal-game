//! Small drifting motes: drive exhaust, sparks, embers. Presentation only.
//!
//! A mote is anchored to something the view already shows (an own body, a contact's
//! track, or a received position) and drifts from it in screen pixels on the wall clock,
//! so it reads the same at one kilometre a pixel and at one AU a pixel. Nothing here
//! reads truth, and the random numbers are its own.
use super::*;
use egui::epaint::Mesh;

/// Hard ceiling. Past it the oldest motes go first.
const LIMIT: usize = 900;
/// Past this many burning rounds on the plot, each sheds proportionally fewer motes.
const MISSILE_BUDGET: f32 = 30.0;

#[derive(Clone, Copy, PartialEq)]
enum Anchor {
    Place,
    Body(BodyId),
    Contact(ContactId),
}

struct Mote {
    anchor: Anchor,
    /// Last known world position of the anchor.
    at: Vec2,
    offset: EVec2,
    velocity: EVec2,
    /// Fraction of speed lost each second.
    drag: f32,
    born: f64,
    life: f32,
    size: f32,
    color: Color32,
}

#[derive(Default)]
pub(super) struct Particles {
    motes: Vec<Mote>,
    seen: BTreeSet<CombatLogKey>,
    initialized: bool,
    last: Option<f64>,
    noise: u64,
}

fn key(e: &luminal_core::world::CombatEvent) -> CombatLogKey {
    (e.emitted_at.to_bits(), e.received_at.to_bits(), e.kind as u8, e.own_body.map(|id| id.0), e.contact.map(|id| id.0))
}

const EXHAUST: Color32 = Color32::from_rgb(255, 196, 96);
const EMBER: Color32 = Color32::from_rgb(255, 104, 40);
const SPARK: Color32 = Color32::from_rgb(255, 228, 170);
const FIELD: Color32 = Color32::from_rgb(150, 220, 255);
const JUMP: Color32 = Color32::from_rgb(110, 165, 255);

impl Particles {
    /// Uniform in 0..1.
    fn roll(&mut self) -> f32 {
        if self.noise == 0 { self.noise = 0x2545_f491_4f6c_dd1d; }
        self.noise ^= self.noise << 13;
        self.noise ^= self.noise >> 7;
        self.noise ^= self.noise << 17;
        (self.noise >> 40) as f32 / (1u64 << 24) as f32
    }

    fn between(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * self.roll()
    }

    /// How many to release this frame for a rate per second, carrying the fraction as chance.
    fn release(&mut self, per_second: f32, dt: f32) -> usize {
        let expected = (per_second * dt).max(0.0);
        expected.floor() as usize + usize::from(self.roll() < expected.fract())
    }

    #[allow(clippy::too_many_arguments)]
    fn add(&mut self, anchor: Anchor, at: Vec2, offset: EVec2, velocity: EVec2, drag: f32, now: f64, life: f32, size: f32, color: Color32) {
        if self.motes.len() >= LIMIT { self.motes.remove(0); }
        self.motes.push(Mote { anchor, at, offset, velocity, drag, born: now, life, size, color });
    }

    /// A burst in every direction.
    #[allow(clippy::too_many_arguments)]
    fn burst(&mut self, anchor: Anchor, at: Vec2, now: f64, count: usize, speed: (f32, f32), life: (f32, f32), size: f32, drag: f32, color: Color32) {
        for _ in 0..count {
            let direction = EVec2::angled(self.between(0.0, std::f32::consts::TAU));
            let (speed, life) = (self.between(speed.0, speed.1), self.between(life.0, life.1));
            let size = size * self.between(0.75, 1.25);
            self.add(anchor, at, direction * 2.0, direction * speed, drag, now, life, size, color);
        }
    }

    /// Advance the motes and release new ones for what the view shows. `now` is wall-clock seconds.
    pub(super) fn observe(&mut self, view: &View, now: f64) {
        let dt = self.last.map_or(0.0, |last| (now - last).clamp(0.0, 0.1)) as f32;
        self.last = Some(now);
        if !self.motes.is_empty() {
            // One lookup a frame, not a search per mote.
            let bodies: BTreeMap<BodyId, Vec2> = view.bodies.iter().map(|b| (b.id, b.pos)).collect();
            let tracks: BTreeMap<ContactId, Vec2> = view.contacts.iter().filter_map(|c| Some((c.id, c.track.as_ref()?.pos))).collect();
            for mote in &mut self.motes {
                mote.offset += mote.velocity * dt;
                mote.velocity *= (-mote.drag * dt).exp();
                let moved = match mote.anchor {
                    Anchor::Body(id) => bodies.get(&id),
                    Anchor::Contact(id) => tracks.get(&id),
                    Anchor::Place => None,
                };
                if let Some(at) = moved { mote.at = *at; }
            }
        }
        self.motes.retain(|mote| now - mote.born < mote.life as f64);

        // Bursts for events whose light has just arrived. The first frame only takes stock.
        // A jump or a ship loss is in both lists. It is one event.
        let mut taken = BTreeSet::new();
        let fresh: Vec<_> = view.combat.iter().chain(&view.jump_events)
            .filter(|e| !self.seen.contains(&key(e)) && taken.insert(key(e))).cloned().collect();
        self.seen = view.combat.iter().chain(&view.jump_events).map(key).collect();
        if self.initialized {
            for event in fresh { self.event(view, &event, now); }
        }
        self.initialized = true;

        // A stopped clock releases nothing. What is already drifting fades out.
        if view.paused || dt <= 0.0 { return; }
        let rounds = view.bodies.iter().filter(|b| b.kind == BodyKind::Missile && b.interceptor.is_none() && b.thrust.length() > 0.0).count() as f32;
        let thinning = (MISSILE_BUDGET / rounds.max(1.0)).min(1.0);
        for b in &view.bodies {
            let burn = (b.thrust.length() / G0) as f32;
            match b.kind {
                BodyKind::Ship => {
                    if burn > 0.5 {
                        // Exhaust: a thin stream of cooling motes behind the plume.
                        let rear = -screen_dir(b.heading);
                        let side = EVec2::new(-rear.y, rear.x);
                        let power = (burn / 120.0).clamp(0.05, 1.5);
                        for _ in 0..self.release(5.0 + 20.0 * power, dt) {
                            let lateral = self.between(-1.0, 1.0);
                            let offset = rear * (SHIP_STERN + self.between(0.0, 6.0)) + side * (lateral * 1.6);
                            let velocity = rear * (self.between(26.0, 58.0) * (0.6 + 0.6 * power)) + side * (lateral * 7.0);
                            let (life, size) = (self.between(0.35, 0.85), self.between(0.8, 1.4));
                            let color = fx::mix(EXHAUST, EMBER, self.roll());
                            self.add(Anchor::Body(b.id), b.pos, offset, velocity, 1.6, now, life, size, color);
                        }
                    }
                    if b.thermal.dumping {
                        // Open radiators: slow embers leaving the hull in every direction.
                        for _ in 0..self.release(9.0, dt) {
                            let direction = EVec2::angled(self.between(0.0, std::f32::consts::TAU));
                            let (start, speed, life) = (self.between(5.0, 9.0), self.between(6.0, 15.0), self.between(0.9, 1.7));
                            self.add(Anchor::Body(b.id), b.pos, direction * start, direction * speed, 0.5, now, life, 1.1, EMBER);
                        }
                    }
                    if matches!(b.jump, Some(JumpState::Spooling { .. })) {
                        // A spooling drive draws light in toward the hull.
                        for _ in 0..self.release(16.0, dt) {
                            let direction = EVec2::angled(self.between(0.0, std::f32::consts::TAU));
                            let (start, life) = (self.between(26.0, 40.0), self.between(0.6, 1.0));
                            self.add(Anchor::Body(b.id), b.pos, direction * start, -direction * (start / life), 0.0, now, life, 1.0, JUMP);
                        }
                    }
                }
                BodyKind::Missile if b.interceptor.is_none() && burn > 0.5 => {
                    let rear = -screen_dir(b.thrust);
                    for _ in 0..self.release(7.0 * thinning, dt) {
                        let (speed, life) = (self.between(10.0, 22.0), self.between(0.25, 0.5));
                        self.add(Anchor::Body(b.id), b.pos, rear * 3.0, rear * speed, 2.0, now, life, 0.8, EXHAUST);
                    }
                }
                _ => {}
            }
        }
    }

    fn event(&mut self, view: &View, e: &luminal_core::world::CombatEvent, now: f64) {
        // Follow the thing that was hit when the view still holds it; otherwise stay where the light came from.
        let own = e.own_body.and_then(|id| view.bodies.iter().find(|b| b.id == id).map(|b| (Anchor::Body(id), b.pos)));
        let contact = e.contact.and_then(|id| view.contacts.iter().find(|c| c.id == id).and_then(|c| c.track.as_ref()).map(|t| (Anchor::Contact(id), t.pos)));
        let Some((anchor, at)) = own.or(contact).or(e.pos.map(|pos| (Anchor::Place, pos))) else { return };
        let place = e.pos.map_or((anchor, at), |pos| (Anchor::Place, pos));
        match e.kind {
            CombatKind::Impact => {
                let strength = e.impact_strength.clamp(0.0, 1.0);
                if strength < 0.3 {
                    // Absorbed: a ring of field light sliding around the ship.
                    for _ in 0..14 {
                        let direction = EVec2::angled(self.between(0.0, std::f32::consts::TAU));
                        let around = EVec2::new(-direction.y, direction.x) * self.between(-34.0, 34.0);
                        let life = self.between(0.3, 0.6);
                        self.add(anchor, at, direction * 11.0, around + direction * 8.0, 2.5, now, life, 1.1, FIELD);
                    }
                } else {
                    self.burst(anchor, at, now, 10 + (26.0 * strength) as usize, (50.0, 90.0 + 110.0 * strength), (0.3, 0.8), 1.2, 2.6, SPARK);
                    self.burst(anchor, at, now, 4 + (10.0 * strength) as usize, (6.0, 22.0), (1.2, 2.6), 1.3, 0.6, EMBER);
                }
            }
            CombatKind::NuclearBurst => {
                self.burst(place.0, place.1, now, 34, (150.0, 300.0), (0.25, 0.5), 1.3, 3.5, SPARK);
                self.burst(place.0, place.1, now, 16, (10.0, 40.0), (1.0, 2.2), 1.4, 0.8, EXHAUST);
            }
            CombatKind::Destroyed => {
                if e.subject_kind == Some(BodyKind::Missile) {
                    self.burst(place.0, place.1, now, 7, (30.0, 80.0), (0.25, 0.5), 1.0, 3.0, SPARK);
                } else {
                    // Wreckage cools for a few seconds after the flash.
                    self.burst(place.0, place.1, now, 46, (4.0, 34.0), (1.8, 4.5), 1.4, 0.35, EMBER);
                    self.burst(place.0, place.1, now, 20, (60.0, 170.0), (0.4, 0.9), 1.2, 2.2, SPARK);
                }
            }
            CombatKind::PointDefence => self.burst(place.0, place.1, now, 4, (20.0, 55.0), (0.15, 0.3), 0.9, 3.0, FIELD),
            CombatKind::MissileHit => self.burst(place.0, place.1, now, 9, (30.0, 90.0), (0.25, 0.55), 1.0, 3.0, SPARK),
            CombatKind::JumpDeparture | CombatKind::JumpArrival => {
                self.burst(place.0, place.1, now, 30, (40.0, 120.0), (0.6, 1.3), 1.1, 2.0, JUMP);
                self.burst(place.0, place.1, now, 10, (6.0, 18.0), (1.2, 2.2), 1.2, 0.5, FIELD);
            }
            _ => {}
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.motes.is_empty()
    }

    /// Every mote, as one mesh of soft additive dots.
    pub(super) fn draw(&self, painter: &egui::Painter, cam: &Camera, rect: Rect, now: f64) {
        if self.motes.is_empty() { return; }
        let visible = rect.expand(24.0);
        let dark = Color32::from_rgba_premultiplied(0, 0, 0, 0);
        let mut mesh = Mesh::default();
        for mote in &self.motes {
            let at = to_screen(cam, rect, mote.at) + mote.offset;
            if !visible.contains(at) { continue; }
            let age = ((now - mote.born) as f32 / mote.life).clamp(0.0, 1.0);
            // In quickly, out slowly.
            let level = (age / 0.08).min(1.0) * (1.0 - age).powf(1.5);
            let reach = mote.size * 1.9;
            let base = mesh.vertices.len() as u32;
            mesh.colored_vertex(at, fx::light(mote.color, 0.85 * level));
            for corner in [EVec2::new(-reach, 0.0), EVec2::new(0.0, -reach), EVec2::new(reach, 0.0), EVec2::new(0.0, reach)] {
                mesh.colored_vertex(at + corner, dark);
            }
            for k in 0..4 { mesh.add_triangle(base, base + 1 + k, base + 1 + (k + 1) % 4); }
        }
        if !mesh.is_empty() { painter.add(Shape::mesh(mesh)); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene() -> (View, BodyId) {
        let app = LuminalApp::new();
        let mut view = app.session.view(app.role);
        view.combat.clear();
        view.jump_events.clear();
        view.paused = false;
        let ship = view.bodies.iter_mut().find(|b| b.controllable && b.kind == BodyKind::Ship).unwrap();
        ship.thrust = Vec2::new(60.0 * G0, 0.0);
        ship.heading = Vec2::new(1.0, 0.0);
        let id = ship.id;
        (view, id)
    }

    fn impact(view: &View, body: BodyId, strength: f32, at: f64) -> luminal_core::world::CombatEvent {
        luminal_core::world::CombatEvent { weapon_visual: luminal_core::world::weapon_fit::WeaponVisual::Standard, velocity: None,
            subject_kind: Some(BodyKind::Ship), impact_strength: strength, damage: None, contact: None, target: None, aim: None,
            emitted_at: at, received_at: at, pos: view.bodies.iter().find(|b| b.id == body).map(|b| b.pos), kind: CombatKind::Impact, own_body: Some(body) }
    }

    #[test]
    fn a_burning_ship_trails_exhaust_behind_it_and_a_stopped_clock_releases_none() {
        let (mut view, id) = scene();
        let mut particles = Particles::default();
        particles.observe(&view, 0.0);
        assert!(particles.is_empty(), "the first frame only takes stock");
        for frame in 1..=60 { particles.observe(&view, frame as f64 / 60.0); }
        let count = particles.motes.len();
        assert!(count > 4 && count < 40, "a thin stream, not a cloud: {count}");
        // Heading is +x in the world, so astern is -x on screen.
        assert!(particles.motes.iter().all(|m| m.anchor == Anchor::Body(id) && m.offset.x < 0.0 && m.velocity.x < 0.0));
        view.paused = true;
        for frame in 61..=180 { particles.observe(&view, frame as f64 / 60.0); }
        assert!(particles.is_empty(), "nothing is released while paused, and what was drifting has faded");
        view.paused = false;
        view.bodies.iter_mut().find(|b| b.id == id).unwrap().thrust = Vec2::ZERO;
        for frame in 181..=240 { particles.observe(&view, frame as f64 / 60.0); }
        assert!(particles.is_empty(), "a coasting ship has no exhaust");
    }

    #[test]
    fn a_hit_sparks_once_and_harder_hits_throw_more() {
        let (mut view, id) = scene();
        view.bodies.iter_mut().find(|b| b.id == id).unwrap().thrust = Vec2::ZERO;
        let mut particles = Particles::default();
        view.combat.push(impact(&view, id, 1.0, 1.0));
        particles.observe(&view, 0.0);
        assert!(particles.is_empty(), "events already in the picture at start are not replayed");
        view.combat.push(impact(&view, id, 0.1, 2.0));
        particles.observe(&view, 0.01);
        let absorbed = particles.motes.len();
        assert!(absorbed > 0 && particles.motes.iter().all(|m| m.color == FIELD));
        particles.observe(&view, 0.02);
        assert_eq!(particles.motes.len(), absorbed, "the same event does not burst twice");
        view.combat.push(impact(&view, id, 1.0, 3.0));
        particles.observe(&view, 0.03);
        assert!(particles.motes.len() - absorbed > 2 * absorbed, "a penetrating hit throws far more than a screen flare");
        particles.observe(&view, 60.0);
        assert!(particles.is_empty(), "every mote expires");
        // A jump is reported in the combat list and again in the retained jump list.
        let mut arrival = impact(&view, id, 0.0, 9.0);
        arrival.kind = CombatKind::JumpArrival;
        view.combat = vec![arrival.clone()];
        particles.observe(&view, 61.0);
        let once = particles.motes.len();
        particles.observe(&view, 120.0);
        let mut departure = arrival.clone();
        departure.kind = CombatKind::JumpDeparture;
        view.combat = vec![departure.clone()];
        view.jump_events = vec![departure];
        particles.observe(&view, 121.0);
        assert_eq!(particles.motes.len(), once, "one event in both lists is one burst");
    }

    /// `cargo test --release -p luminal-app mote_cost -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn mote_cost() {
        let mut app = LuminalApp::new_with_scenario(Scenario::Armada, luminal_core::world::ShipClass::Destroyer, theme::Theme::Luminal);
        app.role = Role::Spectator;
        let mut view = app.session.view(app.role);
        view.paused = false;
        for b in &mut view.bodies { if b.kind == BodyKind::Ship { b.thrust = Vec2::new(60.0 * G0, 0.0); } }
        let id = view.bodies[0].id;
        let mut particles = Particles::default();
        particles.observe(&view, 0.0);
        for k in 0..40 { view.combat.push(impact(&view, id, 1.0, 10.0 + k as f64)); }
        let ctx = egui::Context::default();
        let rect = Rect::from_min_size(Pos2::ZERO, EVec2::new(1920.0, 760.0));
        let camera = Camera { center: view.bodies[0].pos, km_per_px: 1000.0 };
        let started = std::time::Instant::now();
        let frames = 600;
        for frame in 1..=frames {
            let now = frame as f64 / 60.0;
            let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(rect), ..Default::default() }, |ui| {
                particles.observe(&view, now);
                particles.draw(&ui.painter_at(rect), &camera, rect, now);
            });
            output.textures_delta.clear();
            ctx.tessellate(output.shapes, output.pixels_per_point);
        }
        println!("MOTES {} alive, {:.3} ms/frame with twenty ships burning", particles.motes.len(), started.elapsed().as_secs_f64() * 1000.0 / frames as f64);
    }

    #[test]
    fn the_count_is_bounded_and_the_mesh_is_valid() {
        let (mut view, id) = scene();
        let mut particles = Particles::default();
        particles.observe(&view, 0.0);
        for k in 0..200 {
            view.combat.push(impact(&view, id, 1.0, 10.0 + k as f64));
            particles.observe(&view, 0.001 * (k + 1) as f64);
        }
        assert_eq!(particles.motes.len(), LIMIT);
        let ctx = egui::Context::default();
        let rect = Rect::from_min_size(Pos2::ZERO, EVec2::new(800.0, 600.0));
        let at = view.bodies.iter().find(|b| b.id == id).unwrap().pos;
        let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(rect), ..Default::default() }, |ui| {
            particles.draw(&ui.painter_at(rect), &Camera { center: at, km_per_px: 1000.0 }, rect, 0.25);
        });
        output.textures_delta.clear();
        let meshes: Vec<_> = output.shapes.iter().filter_map(|s| match &s.shape { Shape::Mesh(mesh) => Some(mesh.clone()), _ => None }).collect();
        assert_eq!(meshes.len(), 1, "one mesh however many motes");
        assert!(meshes[0].is_valid() && meshes[0].vertices.len() <= LIMIT * 5);
        assert!(meshes[0].vertices.iter().all(|v| v.color.a() == 0), "additive: a mote never covers a plot symbol");
    }
}
