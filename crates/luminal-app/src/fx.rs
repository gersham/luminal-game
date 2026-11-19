//! Soft light for the map: gradient meshes blended additively. Presentation only.
//! Nothing here reads the simulation, and each effect is one mesh however large it draws.
use super::*;
use egui::epaint::Mesh;

/// Segments around a full-size ring. Small rings use fewer; see `segments`.
const SEGMENTS: usize = 48;

/// A ring a few pixels across is round at twelve sides. Most glows on the plot are that small.
fn segments(radius: f32) -> usize {
    if radius < 20.0 { 12 } else if radius < 90.0 { 24 } else { SEGMENTS }
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

/// The sky behind the plot. Stars are too far to shift when the map pans, so the field is
/// fixed to the screen and built once per map size and theme.
#[derive(Default)]
pub(super) struct Backdrop {
    key: Option<(u32, u32, u32, u32, usize)>,
    mesh: std::sync::Arc<Mesh>,
}

impl Backdrop {
    pub(super) fn draw(&mut self, painter: &egui::Painter, rect: Rect, theme: theme::Theme) {
        let key = (rect.left() as u32, rect.top() as u32, rect.width() as u32, rect.height() as u32, theme as usize);
        if self.key != Some(key) {
            self.key = Some(key);
            self.mesh = std::sync::Arc::new(sky(rect, theme));
        }
        if !self.mesh.is_empty() { painter.add(Shape::mesh(self.mesh.clone())); }
    }
}

fn sky(rect: Rect, theme: theme::Theme) -> Mesh {
    let mut mesh = Mesh::default();
    if rect.width() < 8.0 || rect.height() < 8.0 { return mesh; }
    let tint = match theme {
        theme::Theme::Luminal => Color32::from_rgb(70, 110, 190),
        theme::Theme::GrimDark => Color32::from_rgb(190, 70, 50),
        theme::Theme::Imperium => Color32::from_rgb(170, 130, 70),
        theme::Theme::Culture => Color32::from_rgb(60, 160, 190),
    };
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
    let mut rng = luminal_core::rng::Rng::stream(0x5b17_f1e1_d000 + theme as u64, 3);
    let count = (rect.area() / 1500.0).clamp(80.0, 4000.0) as usize;
    for _ in 0..count {
        let mut at = rect.min + EVec2::new(rng.uniform() as f32 * rect.width(), rng.uniform() as f32 * rect.height());
        // Two in five are drawn toward the band, which gives it grain without a hard edge.
        if rng.uniform() < 0.4 {
            let distance = (at - middle).dot(across);
            at -= across * (distance * (0.45 + 0.4 * rng.uniform() as f32));
        }
        let magnitude = (rng.uniform() as f32).powi(4);
        let warmth = rng.uniform() as f32;
        let color = mix(Color32::from_rgb(170, 200, 255), Color32::from_rgb(255, 214, 170), warmth);
        let brightness = 0.10 + 0.55 * magnitude;
        let size = if magnitude > 0.55 { 1.6 } else { 1.0 };
        if !rect.contains(at) { continue; }
        mesh.add_colored_rect(Rect::from_center_size(at.round(), EVec2::splat(size)), light(color, brightness));
        if magnitude > 0.8 {
            rings(&mut mesh, at.round(), &[(0.0, light(color, 0.22)), (1.6, light(color, 0.08)), (4.5, light(color, 0.0))], |_| 1.0);
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
    fn the_sky_is_built_once_per_size_and_stays_dim() {
        let rect = Rect::from_min_size(Pos2::ZERO, EVec2::new(1920.0, 760.0));
        let mut backdrop = Backdrop::default();
        let ctx = egui::Context::default();
        let mut first = None;
        for _ in 0..2 {
            let _ = ctx.run_ui(egui::RawInput { screen_rect: Some(rect), ..Default::default() }, |ui| {
                backdrop.draw(&ui.painter_at(rect), rect, theme::Theme::Luminal);
            });
            let pointer = std::sync::Arc::as_ptr(&backdrop.mesh);
            assert!(first.is_none_or(|earlier| earlier == pointer), "the field is rebuilt every frame");
            first = Some(pointer);
        }
        assert!(backdrop.mesh.is_valid());
        assert!(backdrop.mesh.vertices.len() > 1000);
        // A plot symbol is a saturated colour at full alpha. No star may read as one.
        assert!(backdrop.mesh.vertices.iter().all(|v| v.color.a() == 0 && v.color.r().max(v.color.g()).max(v.color.b()) < 180));
        let other = sky(rect, theme::Theme::GrimDark);
        assert_ne!(other.vertices[2].color, backdrop.mesh.vertices[2].color, "each theme tints its own sky");
    }

    #[test]
    fn a_degenerate_effect_draws_nothing() {
        let ctx = egui::Context::default();
        let rect = Rect::from_min_size(Pos2::ZERO, EVec2::new(400.0, 300.0));
        let output = ctx.run_ui(egui::RawInput { screen_rect: Some(rect), ..Default::default() }, |ui| {
            let painter = ui.painter_at(rect);
            glow(&painter, Pos2::ZERO, f32::NAN, Color32::WHITE, 1.0);
            glow(&painter, Pos2::ZERO, 1.0e9, Color32::WHITE, 1.0);
            beam(&painter, Pos2::ZERO, Pos2::ZERO, 4.0, Color32::WHITE, 1.0);
            plume(&painter, Pos2::ZERO, Pos2::new(f32::INFINITY, 0.0), 4.0, Color32::WHITE, 1.0);
            disc(&painter, Pos2::ZERO, 0.0, Color32::WHITE, Color32::BLACK);
        });
        assert!(!output.shapes.iter().any(|s| matches!(s.shape, Shape::Mesh(_))));
    }
}
