//! Soft light for the map: gradient meshes blended additively. Presentation only.
//! Nothing here reads the simulation, and each effect is one mesh however large it draws.
use super::*;
use egui::epaint::Mesh;

/// Segments around a full-size ring. Small rings use fewer; see `segments`.
const SEGMENTS: usize = 48;

/// A ring a few pixels across is round at twelve sides. Most glows on the plot are that small.
fn segments(radius: f32) -> usize {
    if radius < 20.0 { 12 } else if radius < 90.0 { 24 } else if radius < 500.0 { SEGMENTS } else { 144 }
}
/// Past this radius a glow is a wash over the whole plot, and its coordinates stop being exact.
const MAX_GLOW_PX: f32 = 20_000.0;

/// `color` scaled by `k` and blended additively: overlapping light sums instead of covering.
pub(super) fn light(color: Color32, k: f32) -> Color32 {
    let k = if k.is_finite() { k.max(0.0) } else { 0.0 };
    let channel = |v: u8| (v as f32 * k).clamp(0.0, 255.0) as u8;
    Color32::from_rgba_premultiplied(channel(color.r()), channel(color.g()), channel(color.b()), 0)
}

pub(super) fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let channel = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(channel(a.r(), b.r()), channel(a.g(), b.g()), channel(a.b(), b.b()))
}

/// Finite and greater than `floor`. A NaN or infinite size draws nothing.
fn above(value: f32, floor: f32) -> bool {
    value.is_finite() && floor.is_finite() && value > floor
}

fn unit(step: usize, of: usize) -> EVec2 {
    EVec2::angled(step as f32 * std::f32::consts::TAU / of as f32)
}

/// Rings of vertices around `center`: `(radius, colour)` from the inside out. A zero first radius is a fan.
fn rings(mesh: &mut Mesh, center: Pos2, stops: &[(f32, Color32)], tint: impl Fn(EVec2) -> f32) {
    let base = mesh.vertices.len() as u32;
    let count = segments(stops.last().map_or(0.0, |stop| stop.0));
    let row = count as u32 + 1;
    for &(radius, color) in stops {
        for step in 0..=count {
            let direction = unit(step, count);
            let k = tint(direction);
            mesh.colored_vertex(center + direction * radius, if k == 1.0 { color } else { scale(color, k) });
        }
    }
    for ring in 0..stops.len().saturating_sub(1) as u32 {
        for step in 0..count as u32 {
            let a = base + ring * row + step;
            let b = a + row;
            mesh.add_triangle(a, b, a + 1);
            mesh.add_triangle(a + 1, b, b + 1);
        }
    }
}

/// Every channel, alpha included, scaled by `k`. Correct for premultiplied and additive colours alike.
fn scale(color: Color32, k: f32) -> Color32 {
    let k = k.clamp(0.0, 4.0);
    let channel = |v: u8| (v as f32 * k).clamp(0.0, 255.0) as u8;
    Color32::from_rgba_premultiplied(channel(color.r()), channel(color.g()), channel(color.b()), channel(color.a()))
}

/// A filled disc shading from `inner` at the centre to `outer` at the rim.
pub(super) fn disc(painter: &egui::Painter, center: Pos2, radius: f32, inner: Color32, outer: Color32) {
    if !above(radius, 0.0) { return; }
    let mut mesh = Mesh::default();
    rings(&mut mesh, center, &[(0.0, inner), (radius * 0.55, mix_premultiplied(inner, outer, 0.25)), (radius, outer)], |_| 1.0);
    painter.add(Shape::mesh(mesh));
}

fn mix_premultiplied(a: Color32, b: Color32, t: f32) -> Color32 {
    let channel = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(channel(a.r(), b.r()), channel(a.g(), b.g()), channel(a.b(), b.b()), channel(a.a(), b.a()))
}

/// Light that is brightest at `center` and gone at `radius`, with no visible steps.
pub(super) fn glow(painter: &egui::Painter, center: Pos2, radius: f32, color: Color32, strength: f32) {
    if !above(radius, 0.5) || radius > MAX_GLOW_PX || !above(strength, 0.0) { return; }
    let mut mesh = Mesh::default();
    rings(&mut mesh, center, &[(0.0, light(color, strength)), (radius * 0.18, light(color, strength * 0.62)),
        (radius * 0.4, light(color, strength * 0.27)), (radius * 0.7, light(color, strength * 0.07)), (radius, light(color, 0.0))], |_| 1.0);
    painter.add(Shape::mesh(mesh));
}

/// Light in a band from `inner` out to `outer`, brightest at the inner edge. `tint` weights each direction.
pub(super) fn halo(painter: &egui::Painter, center: Pos2, inner: f32, outer: f32, color: Color32, strength: f32, tint: impl Fn(EVec2) -> f32) {
    if !above(outer, inner) || outer > MAX_GLOW_PX || !above(strength, 0.0) { return; }
    let mut mesh = Mesh::default();
    let at = |f: f32| inner + (outer - inner) * f;
    rings(&mut mesh, center, &[(at(0.0), light(color, strength)), (at(0.3), light(color, strength * 0.4)), (at(1.0), light(color, 0.0))], tint);
    painter.add(Shape::mesh(mesh));
}

/// An expanding front: sharp at `radius`, trailing off inward over `depth`.
pub(super) fn wavefront(painter: &egui::Painter, center: Pos2, radius: f32, depth: f32, color: Color32, strength: f32) {
    if !above(radius, 1.0) || radius > MAX_GLOW_PX || !above(strength, 0.0) { return; }
    let inner = (radius - depth.max(2.0)).max(0.0);
    let mut mesh = Mesh::default();
    rings(&mut mesh, center, &[(inner, light(color, 0.0)), (radius - 1.0, light(color, strength)), (radius + 0.5, light(color, 0.0))], |_| 1.0);
    painter.add(Shape::mesh(mesh));
}

/// A flat band of light between two radii, soft at both edges. `tint` weights each direction.
pub(super) fn band(painter: &egui::Painter, center: Pos2, inner: f32, outer: f32, color: Color32, strength: f32, tint: impl Fn(EVec2) -> f32) {
    if !above(outer, inner) || outer > MAX_GLOW_PX || !above(strength, 0.0) { return; }
    let edge = ((outer - inner) * 0.18).min(3.0);
    let mut mesh = Mesh::default();
    rings(&mut mesh, center, &[(inner, light(color, 0.0)), (inner + edge, light(color, strength)), (outer - edge, light(color, strength)), (outer, light(color, 0.0))], tint);
    painter.add(Shape::mesh(mesh));
}

/// A line of light from `a` to `b`: full strength on the axis, nothing at `half_width`.
pub(super) fn beam(painter: &egui::Painter, a: Pos2, b: Pos2, half_width: f32, color: Color32, strength: f32) {
    taper(painter, a, b, half_width, half_width, light(color, strength), light(color, strength));
}

/// A drive plume: `half_width` wide and `core` bright at the nozzle, a point and dark at `tip`.
pub(super) fn plume(painter: &egui::Painter, nozzle: Pos2, tip: Pos2, half_width: f32, core: Color32, strength: f32) {
    taper(painter, nozzle, tip, half_width, half_width * 0.15, light(core, strength), light(core, 0.0));
}

fn taper(painter: &egui::Painter, a: Pos2, b: Pos2, width_a: f32, width_b: f32, color_a: Color32, color_b: Color32) {
    let along = b - a;
    let length = along.length();
    if !above(length, 0.01) || !above(width_a, 0.0) { return; }
    let side = EVec2::new(-along.y, along.x) / length;
    let dark = Color32::from_rgba_premultiplied(0, 0, 0, 0);
    let mut mesh = Mesh::default();
    for (at, width, color) in [(a, width_a, color_a), (b, width_b, color_b)] {
        mesh.colored_vertex(at + side * width, dark);
        mesh.colored_vertex(at, color);
        mesh.colored_vertex(at - side * width, dark);
    }
    for (p, q) in [(0, 1), (1, 2)] {
        mesh.add_triangle(p, p + 3, q);
        mesh.add_triangle(q, p + 3, q + 3);
    }
    painter.add(Shape::mesh(mesh));
}

/// How far each star layer slides for a pixel of pan. Far stars barely move.
const PARALLAX: [f32; 3] = [0.004, 0.012, 0.03];
/// A pan larger than this in one frame is a jump (a fit, a restart), not a drag.
const PAN_JUMP_PX: f32 = 400.0;

struct Star {
    /// Position as a fraction of the map, so a resize keeps the pattern.
    at: EVec2,
    size: f32,
    color: Color32,
    halo: bool,
    layer: usize,
}

/// The sky behind the plot: a tinted nebula, a band of unresolved stars, and three layers
/// of stars that slide very slightly as the map pans. Zooming in place moves nothing,
/// because the stars are too far for any of this to be real parallax; the drift is only
/// a cue that the map is moving. Built once, and rebuilt only when the pan has moved a
/// layer by a visible amount.
#[derive(Default)]
pub(super) struct Backdrop {
    theme: Option<usize>,
    stars: Vec<Star>,
    /// Accumulated pan, px.
    pan: EVec2,
    key: Option<(i32, i32, i32, i32, i32, i32)>,
    mesh: std::sync::Arc<Mesh>,
}

impl Backdrop {
    /// `pan` is how far the map moved under the view since the last frame, in pixels.
    pub(super) fn draw(&mut self, painter: &egui::Painter, rect: Rect, theme: theme::Theme, pan: EVec2) {
        if rect.width() < 8.0 || rect.height() < 8.0 { return; }
        if self.theme != Some(theme as usize) {
            self.theme = Some(theme as usize);
            self.stars = stars(theme);
            self.key = None;
        }
        if pan.x.is_finite() && pan.y.is_finite() && pan.length() < PAN_JUMP_PX { self.pan += pan; }
        // The nearest layer decides when anything has visibly moved.
        let shift = self.pan * PARALLAX[2];
        let key = (rect.left() as i32, rect.top() as i32, rect.width() as i32, rect.height() as i32, (shift.x * 2.0).round() as i32, (shift.y * 2.0).round() as i32);
        if self.key != Some(key) {
            self.key = Some(key);
            self.mesh = std::sync::Arc::new(sky(rect, theme, &self.stars, self.pan));
        }
        if !self.mesh.is_empty() { painter.add(Shape::mesh(self.mesh.clone())); }
    }
}

fn sky_tint(theme: theme::Theme) -> [Color32; 2] {
    match theme {
        theme::Theme::Luminal => [Color32::from_rgb(70, 110, 190), Color32::from_rgb(120, 80, 170)],
        theme::Theme::GrimDark => [Color32::from_rgb(190, 70, 50), Color32::from_rgb(150, 100, 40)],
        theme::Theme::Imperium => [Color32::from_rgb(170, 130, 70), Color32::from_rgb(70, 110, 150)],
        theme::Theme::Culture => [Color32::from_rgb(60, 160, 190), Color32::from_rgb(90, 190, 150)],
    }
}

fn stars(theme: theme::Theme) -> Vec<Star> {
    let mut rng = luminal_core::rng::Rng::stream(0x5b17_f1e1_d000 + theme as u64, 3);
    (0..1400).map(|_| {
        let at = EVec2::new(rng.uniform() as f32, rng.uniform() as f32);
        let magnitude = (rng.uniform() as f32).powi(4);
        let color = mix(Color32::from_rgb(170, 200, 255), Color32::from_rgb(255, 214, 170), rng.uniform() as f32);
        // Brighter stars sit in nearer layers, so the ones that catch the eye are the ones that drift.
        let layer = if magnitude > 0.5 { 2 } else if rng.uniform() < 0.4 { 1 } else { 0 };
        Star { at, size: if magnitude > 0.55 { 1.6 } else { 1.0 }, color: light(color, 0.10 + 0.55 * magnitude), halo: magnitude > 0.8, layer }
    }).collect()
}

fn sky(rect: Rect, theme: theme::Theme, stars: &[Star], pan: EVec2) -> Mesh {
    let mut mesh = Mesh::default();
    let [tint, second] = sky_tint(theme);
    // Nebula: a few very large, very faint clouds. Fixed, like the band.
    let mut rng = luminal_core::rng::Rng::stream(0x4e45_4255 + theme as u64, 5);
    for cloud in 0..6 {
        let at = rect.min + EVec2::new(rng.uniform() as f32 * rect.width(), rng.uniform() as f32 * rect.height());
        let radius = rect.height() * (0.28 + 0.34 * rng.uniform() as f32);
        let color = if cloud % 2 == 0 { tint } else { second };
        let level = 0.022 + 0.022 * rng.uniform() as f32;
        rings(&mut mesh, at, &[(0.0, light(color, level)), (radius * 0.45, light(color, level * 0.55)), (radius, light(color, 0.0))], |_| 1.0);
    }
    // A faint band of unresolved stars across the field, the way a galactic plane sits in a real sky.
    let axis = EVec2::new(0.93, -0.37);
    let across = EVec2::new(-axis.y, axis.x);
    let middle = rect.center() + across * (rect.height() * 0.08);
    let reach = rect.width() + rect.height();
    let half = rect.height() * 0.3;
    let base = mesh.vertices.len() as u32;
    for end in [-reach, reach] {
        for (offset, k) in [(-half, 0.0), (-half * 0.35, 0.03), (0.0, 0.05), (half * 0.35, 0.03), (half, 0.0)] {
            mesh.colored_vertex(middle + axis * end + across * offset, light(tint, k));
        }
    }
    for strip in 0..4 {
        mesh.add_triangle(base + strip, base + strip + 5, base + strip + 1);
        mesh.add_triangle(base + strip + 1, base + strip + 5, base + strip + 6);
    }
    let count = ((rect.area() / 1500.0) as usize).clamp(80, stars.len());
    for star in &stars[..count] {
        // Slide with the pan and wrap, so the field never runs out.
        let slid = star.at + EVec2::new(pan.x / rect.width(), pan.y / rect.height()) * PARALLAX[star.layer];
        let at = (rect.min + EVec2::new(slid.x.rem_euclid(1.0) * rect.width(), slid.y.rem_euclid(1.0) * rect.height())).round();
        mesh.add_colored_rect(Rect::from_center_size(at, EVec2::splat(star.size)), star.color);
        if star.halo {
            rings(&mut mesh, at, &[(0.0, scale(star.color, 0.34)), (1.6, scale(star.color, 0.12)), (4.5, light(Color32::BLACK, 0.0))], |_| 1.0);
        }
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_is_additive_and_never_covers_what_is_under_it() {
        let c = light(Color32::from_rgb(200, 100, 50), 0.5);
        assert_eq!((c.r(), c.g(), c.b(), c.a()), (100, 50, 25, 0));
        assert_eq!(light(Color32::WHITE, f32::NAN), Color32::from_rgba_premultiplied(0, 0, 0, 0));
        assert_eq!(light(Color32::WHITE, 9.0).r(), 255);
    }

    #[test]
    fn the_sky_is_rebuilt_only_when_a_pan_moves_it_and_stays_dim() {
        let rect = Rect::from_min_size(Pos2::ZERO, EVec2::new(1920.0, 760.0));
        let mut backdrop = Backdrop::default();
        let ctx = egui::Context::default();
        let frame = |backdrop: &mut Backdrop, pan: EVec2, theme: theme::Theme| {
            ctx.run_ui(egui::RawInput { screen_rect: Some(rect), ..Default::default() }, |ui| {
                backdrop.draw(&ui.painter_at(rect), rect, theme, pan);
            }).textures_delta.clear();
            std::sync::Arc::as_ptr(&backdrop.mesh)
        };
        let still = frame(&mut backdrop, EVec2::ZERO, theme::Theme::Luminal);
        assert_eq!(frame(&mut backdrop, EVec2::ZERO, theme::Theme::Luminal), still, "a still map does not rebuild the sky");
        assert_eq!(frame(&mut backdrop, EVec2::new(3.0, 0.0), theme::Theme::Luminal), still, "nor does a pan too small to move any star");
        let before = backdrop.mesh.clone();
        assert_ne!(frame(&mut backdrop, EVec2::new(300.0, 0.0), theme::Theme::Luminal), still);
        // Near stars moved further than far ones, and none by more than a few pixels.
        let moved: Vec<f32> = before.vertices.iter().zip(&backdrop.mesh.vertices).map(|(a, b)| (b.pos.x - a.pos.x).abs()).filter(|d| *d < 100.0).collect();
        let furthest = moved.iter().copied().fold(0.0_f32, f32::max);
        assert!((8.0..=11.0).contains(&furthest), "the nearest layer slid {furthest} px for 303 px of pan");
        assert!(moved.iter().filter(|d| **d <= 2.0).count() > moved.len() / 3, "the far layer barely moves");
        let pan = backdrop.pan;
        frame(&mut backdrop, EVec2::new(5_000.0, 0.0), theme::Theme::Luminal);
        assert_eq!(backdrop.pan, pan, "a jump of the camera is not a pan");
        assert!(backdrop.mesh.is_valid() && backdrop.mesh.vertices.len() > 1000);
        // A plot symbol is a saturated colour at full alpha. No star may read as one.
        assert!(backdrop.mesh.vertices.iter().all(|v| v.color.a() == 0 && v.color.r().max(v.color.g()).max(v.color.b()) < 180));
        let luminal = backdrop.mesh.vertices[0].color;
        frame(&mut backdrop, EVec2::ZERO, theme::Theme::GrimDark);
        assert_ne!(backdrop.mesh.vertices[0].color, luminal, "each theme tints its own sky");
    }

    #[test]
    fn a_degenerate_effect_draws_nothing() {
        let ctx = egui::Context::default();
        let rect = Rect::from_min_size(Pos2::ZERO, EVec2::new(400.0, 300.0));
        let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(rect), ..Default::default() }, |ui| {
            let painter = ui.painter_at(rect);
            glow(&painter, Pos2::ZERO, f32::NAN, Color32::WHITE, 1.0);
            glow(&painter, Pos2::ZERO, 1.0e9, Color32::WHITE, 1.0);
            beam(&painter, Pos2::ZERO, Pos2::ZERO, 4.0, Color32::WHITE, 1.0);
            plume(&painter, Pos2::ZERO, Pos2::new(f32::INFINITY, 0.0), 4.0, Color32::WHITE, 1.0);
            disc(&painter, Pos2::ZERO, 0.0, Color32::WHITE, Color32::BLACK);
        });
        output.textures_delta.clear();
        assert!(!output.shapes.iter().any(|s| matches!(s.shape, Shape::Mesh(_))));
    }
}
