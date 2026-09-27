//! Luminal desktop client. Talks to the simulation only through `session`.

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Vec2 as EVec2};
use luminal_core::celestial::{CelestialKind, Orbit};
use luminal_core::kinematics::{State, Vec2};
use luminal_core::mind::{ContactId, Source};
use luminal_core::params;
use luminal_core::scenario::{self, ESCORT, RAIDER};
use luminal_core::session::{
    AutopilotStatus, BodyId, BodyView, Command, ContactView, InterceptTarget, LocalSession, Order, Role, View,
};
use luminal_core::units::{AU, G0, LIGHT_SECOND};
use luminal_core::world::{BodyKind, FactionId};
use std::collections::BTreeMap;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1400.0, 900.0]).with_title("Luminal"),
        ..Default::default()
    };
    eframe::run_native("Luminal", options, Box::new(|_cc| Ok(Box::new(LuminalApp::new()))))
}

const WARPS: &[f64] = &[1.0, 10.0, 100.0, 1_000.0, 10_000.0, 100_000.0];
/// How far ahead to forecast committed motion, seconds.
const FORECAST_S: f64 = 2.0 * 3600.0;

const BACKGROUND: Color32 = Color32::from_rgb(6, 8, 14);
const CONTACT: Color32 = Color32::from_rgb(255, 170, 60);
const BELIEF: Color32 = Color32::from_rgb(255, 220, 90);
const DANGER: Color32 = Color32::from_rgb(255, 70, 70);

#[derive(Clone, Copy)]
struct Camera {
    /// World km at the screen centre.
    center: Vec2,
    km_per_px: f64,
}

#[derive(Clone, Copy, PartialEq)]
enum Selection {
    Body(BodyId),
    Contact(ContactId),
}

struct LuminalApp {
    session: LocalSession,
    role: Role,
    /// In spectator mode, whose picture to overlay on truth.
    overlay: Option<FactionId>,
    camera: Camera,
    /// Frame the scene on the next map draw.
    fit_pending: bool,
    selected: Option<Selection>,
    last_message: Option<String>,
    dev: DevHooks,
}

/// Development hooks driven by environment variables, used for visual checks.
/// `LUMINAL_ADVANCE=<sim seconds>` pre-runs the scenario; `LUMINAL_ROLE=spectator|raider`
/// picks the starting view; `LUMINAL_SCREENSHOT=<file.ppm>` saves a frame and exits.
#[derive(Default)]
struct DevHooks {
    screenshot: Option<std::path::PathBuf>,
    frames: u32,
}

impl LuminalApp {
    fn new() -> Self {
        Self {
            session: LocalSession::new(scenario::transport_intercept()),
            role: Role::Faction(ESCORT),
            overlay: Some(ESCORT),
            camera: Camera { center: Vec2::ZERO, km_per_px: AU / 500.0 },
            fit_pending: true,
            selected: Some(Selection::Body(BodyId(0))),
            last_message: None,
            dev: DevHooks { screenshot: std::env::var_os("LUMINAL_SCREENSHOT").map(Into::into), frames: 0 },
        }
        .with_env_setup()
    }

    fn with_env_setup(mut self) -> Self {
        // `LUMINAL_ORDERS=orbit:<body>:<celestial>;intercept:<body>:own:<body>;intercept:<body>:contact:<n>`
        // `LUMINAL_ORDERS_AT=<sim seconds>` runs the scenario that far before issuing them.
        let orders_at = std::env::var("LUMINAL_ORDERS_AT").ok().and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
        let _ = self.session.command(Role::Spectator, Command::SetPaused(false));
        self.session.tick(orders_at);
        let _ = self.session.command(Role::Spectator, Command::SetPaused(true));
        if let Ok(orders) = std::env::var("LUMINAL_ORDERS") {
            let view = self.session.view(Role::Spectator);
            for o in orders.split(';').filter(|o| !o.is_empty()) {
                let f: Vec<&str> = o.split(':').collect();
                let num = |i: usize| f.get(i).and_then(|v| v.parse::<u32>().ok());
                let Some(body) = num(1).map(BodyId) else { continue };
                let Some(faction) = view.bodies.iter().find(|b| b.id == body).map(|b| b.faction) else { continue };
                let cmd = match (f[0], f.get(2).copied()) {
                    ("orbit", _) => num(2).map(|c| Command::Orbit { body, celestial: c as usize }),
                    ("intercept", Some("own")) => num(3).map(|t| Command::Intercept { body, target: InterceptTarget::Own(BodyId(t)) }),
                    ("intercept", Some("contact")) => {
                        num(3).map(|t| Command::Intercept { body, target: InterceptTarget::Contact(ContactId(t)) })
                    }
                    _ => None,
                };
                if let Some(cmd) = cmd
                    && let Err(e) = self.session.command(Role::Faction(faction), cmd)
                {
                    eprintln!("LUMINAL_ORDERS {o}: {e:?}");
                }
            }
        }
        if let Some(t) = std::env::var("LUMINAL_ADVANCE").ok().and_then(|v| v.parse::<f64>().ok()) {
            let _ = self.session.command(Role::Spectator, Command::SetPaused(false));
            self.session.tick(t);
            let _ = self.session.command(Role::Spectator, Command::SetPaused(true));
        }
        match std::env::var("LUMINAL_ROLE").as_deref() {
            Ok("spectator") => self.role = Role::Spectator,
            Ok("raider") => {
                self.role = Role::Faction(RAIDER);
                self.selected = Some(Selection::Body(BodyId(2)));
            }
            _ => {}
        }
        self
    }

    fn dev_screenshot(&mut self, ui: &egui::Ui) {
        let Some(path) = self.dev.screenshot.clone() else { return };
        self.dev.frames += 1;
        if self.dev.frames == 20 {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        let image = ui.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = image {
            let [w, h] = img.size;
            let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
            out.extend(img.pixels.iter().flat_map(|c| [c.r(), c.g(), c.b()]));
            if let Err(e) = std::fs::write(&path, out) {
                eprintln!("screenshot failed: {e}");
            }
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn command(&mut self, cmd: Command) {
        self.last_message = match self.session.command(self.role, cmd) {
            Ok(()) => None,
            Err(r) => Some(format!("Rejected: {r:?}")),
        };
    }

    fn own_faction(&self) -> Option<FactionId> {
        match self.role {
            Role::Faction(f) => Some(f),
            Role::Spectator => None,
        }
    }
}

fn faction_color(f: FactionId) -> Color32 {
    match f {
        ESCORT => Color32::from_rgb(90, 170, 255),
        RAIDER => Color32::from_rgb(255, 110, 90),
        _ => Color32::GRAY,
    }
}

fn faction_name(f: FactionId) -> &'static str {
    match f {
        ESCORT => "Escort",
        RAIDER => "Raider",
        _ => "Unknown",
    }
}

fn celestial_color(k: CelestialKind) -> Color32 {
    match k {
        CelestialKind::Star => Color32::from_rgb(255, 214, 120),
        CelestialKind::Planet => Color32::from_rgb(90, 180, 160),
        CelestialKind::Moon => Color32::from_rgb(170, 170, 180),
    }
}

fn fmt_time(t: f64) -> String {
    let sign = if t < 0.0 { "-" } else { "" };
    let s = t.abs() as u64;
    format!("{sign}{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

fn fmt_age(s: f64) -> String {
    if s < 120.0 { format!("{s:.0} s") } else if s < 7200.0 { format!("{:.1} min", s / 60.0) } else { format!("{:.1} h", s / 3600.0) }
}

fn fmt_distance(km: f64) -> String {
    if km >= 0.1 * AU {
        format!("{:.2} AU", km / AU)
    } else if km >= 0.5 * LIGHT_SECOND {
        format!("{:.2} ls", km / LIGHT_SECOND)
    } else {
        format!("{km:.0} km")
    }
}

fn contact_label(c: &ContactView) -> String {
    format!("C{}", c.id.0)
}

impl eframe::App for LuminalApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let dt = ui.input(|i| i.stable_dt) as f64;
        self.session.tick(dt.min(0.1));
        ui.ctx().request_repaint();

        if !ui.ctx().egui_wants_keyboard_input() {
            let (space, f) = ui.input(|i| (i.key_pressed(egui::Key::Space), i.key_pressed(egui::Key::F)));
            if space {
                let paused = self.session.view(Role::Spectator).paused;
                self.command(Command::SetPaused(!paused));
            }
            if f {
                self.fit_pending = true;
            }
        }

        let view = self.session.view(self.role);
        let overlay = match (self.role, self.overlay) {
            (Role::Spectator, Some(f)) => Some((
                self.session.view(Role::Faction(f)),
                self.session.contact_truth(Role::Spectator, f).unwrap_or_default(),
            )),
            _ => None,
        };

        egui::Panel::top("time").show(ui, |ui| self.time_bar(ui, &view));
        egui::Panel::right("side").default_size(340.0).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| self.side_panel(ui, &view));
        });
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| self.map(ui, &view, overlay.as_ref()));
        self.dev_screenshot(ui);
    }
}

impl LuminalApp {
    fn time_bar(&mut self, ui: &mut egui::Ui, view: &View) {
        ui.horizontal(|ui| {
            ui.monospace(format!("T+ {}", fmt_time(view.time)));
            if ui.button(if view.paused { "▶ Run" } else { "⏸ Pause" }).clicked() {
                self.command(Command::SetPaused(!view.paused));
            }
            ui.label("Warp:");
            for &w in WARPS {
                if ui.selectable_label(view.warp == w, format!("{w}×")).clicked() {
                    self.command(Command::SetWarp(w));
                }
            }
            ui.separator();
            ui.label("View as:");
            for (label, role) in [
                ("Escort", Role::Faction(ESCORT)),
                ("Raider", Role::Faction(RAIDER)),
                ("Spectator", Role::Spectator),
            ] {
                if ui.selectable_label(self.role == role, label).clicked() && self.role != role {
                    self.role = role;
                    self.selected = None;
                    self.fit_pending = true;
                }
            }
            if self.role == Role::Spectator {
                ui.separator();
                ui.label("Overlay belief:");
                for (label, o) in [("none", None), ("Escort", Some(ESCORT)), ("Raider", Some(RAIDER))] {
                    if ui.selectable_label(self.overlay == o, label).clicked() {
                        self.overlay = o;
                    }
                }
            }
            ui.separator();
            if ui.button("Fit (F)").clicked() {
                self.fit_pending = true;
            }
        });
    }

    fn side_panel(&mut self, ui: &mut egui::Ui, view: &View) {
        ui.heading(match self.role {
            Role::Faction(f) => format!("{} ships", faction_name(f)),
            Role::Spectator => "All ships (truth)".into(),
        });
        for b in &view.bodies {
            let mut text = b.name.clone();
            if b.active_sensor {
                text.push_str("  · active");
            }
            let sel = self.selected == Some(Selection::Body(b.id));
            if ui.selectable_label(sel, egui::RichText::new(text).color(faction_color(b.faction))).clicked() {
                self.selected = Some(Selection::Body(b.id));
            }
        }
        for l in &view.losses {
            ui.colored_label(DANGER, format!("✖ {} lost at T+ {} ({})", l.name, fmt_time(l.t), l.cause));
        }

        if self.own_faction().is_some() {
            ui.separator();
            ui.heading("Contacts");
            if view.contacts.is_empty() {
                ui.weak("Nothing detected.");
            }
            for c in &view.contacts {
                let age = view.time - c.last_emitted_at;
                let quality = match &c.track {
                    Some(t) => format!("track ±{}", fmt_distance(2.0 * sigma_major(t.cov))),
                    None => "bearing only".into(),
                };
                let text = format!("{}  {}  · light {} old", contact_label(c), quality, fmt_age(age));
                let sel = self.selected == Some(Selection::Contact(c.id));
                if ui.selectable_label(sel, egui::RichText::new(text).color(CONTACT)).clicked() {
                    self.selected = Some(Selection::Contact(c.id));
                }
            }
        }

        match self.selected {
            Some(Selection::Body(id)) => {
                if let Some(b) = view.bodies.iter().find(|b| b.id == id) {
                    self.body_details(ui, view, b);
                }
            }
            Some(Selection::Contact(id)) => {
                if let Some(c) = view.contacts.iter().find(|c| c.id == id) {
                    contact_details(ui, view, c);
                }
            }
            None => {}
        }

        if let Some(m) = &self.last_message {
            ui.separator();
            ui.colored_label(Color32::YELLOW, m);
        }

        ui.separator();
        ui.collapsing("Parameters", |ui| {
            for p in params::ALL {
                ui.label(format!("{} = {} {}  [{:?}]", p.key, p.value, p.unit, p.commitment)).on_hover_text(p.note);
            }
        });
        ui.separator();
        ui.small(
            "Sensor values are placeholders (see Parameters). Contacts come only from light that has \
             reached your ships; reports from other ships travel to your flagship at light speed. \
             Planets, moons and the star block sensors and are fatal to touch.",
        );
        ui.small("Keys: Space pause, F fit. Drag to pan, scroll to zoom. Right-click to give orders. Ships steer around celestial bodies automatically unless it is impossible.");
    }

    fn body_details(&mut self, ui: &mut egui::Ui, view: &View, b: &BodyView) {
        ui.separator();
        ui.heading(&b.name);
        ui.label(format!("{} · {:?}", faction_name(b.faction), b.kind));
        if let Some(nearest) = view
            .celestials
            .iter()
            .map(|c| (c, (c.pos - b.pos).length() - c.radius))
            .min_by(|a, b| a.1.total_cmp(&b.1))
        {
            let rel = b.vel - view.system.state(view.celestials.iter().position(|c| c.name == nearest.0.name).unwrap(), view.time).vel;
            ui.label(format!("{:.1} km/s relative to {}, {} above surface", rel.length(), nearest.0.name, fmt_distance(nearest.1)));
        }
        ui.label(format!("Speed {:.1} km/s (system frame)", b.vel.length()));
        ui.label(format!("Thrust {:.2} g", b.thrust.length() / G0));
        let forecast = view.system.predict(State { pos: b.pos, vel: b.vel }, b.thrust, view.time, FORECAST_S, 2);
        if let Some((i, t)) = forecast.impact {
            ui.colored_label(DANGER, format!("⚠ On course to hit {} in {}", view.celestials[i].name, fmt_age(t - view.time)));
        }
        if b.avoidance.impossible {
            ui.colored_label(DANGER, "⚠ Collision unavoidable: burning to delay it");
        } else if b.avoidance.active {
            ui.colored_label(Color32::from_rgb(255, 150, 60), "⚠ Collision avoidance overriding orders");
        }
        if let Some(ap) = b.autopilot {
            let what = match ap.order {
                Order::Orbit { celestial, radius, .. } => {
                    format!("orbit {} at {} altitude", view.celestials[celestial].name, fmt_distance(radius - view.celestials[celestial].radius))
                }
                Order::Intercept(InterceptTarget::Own(o)) => {
                    format!("join {}", view.bodies.iter().find(|x| x.id == o).map_or("?".into(), |x| x.name.clone()))
                }
                Order::Intercept(InterceptTarget::Contact(c)) => format!("intercept C{}", c.0),
                Order::MoveTo { frame, .. } => format!("move and stop (frame: {})", view.celestials[frame].name),
            };
            let status = match ap.status {
                AutopilotStatus::Manoeuvring => "manoeuvring".to_string(),
                AutopilotStatus::Closing { eta, range } => format!("{} to go, ETA ~{}", fmt_distance(range), fmt_age(eta)),
                AutopilotStatus::Holding => "holding".into(),
                AutopilotStatus::NoTrack => "target track lost, coasting".into(),
            };
            ui.label(format!("Autopilot: {what} ({status})"));
        }
        if self.own_faction() == Some(b.faction) {
            let max_g = match b.kind {
                BodyKind::Ship => params::SHIP_MAX_ACCEL_G.value,
                BodyKind::Probe => params::PROBE_MAX_ACCEL_G.value,
                BodyKind::Missile => params::MISSILE_MAX_ACCEL_G.value,
            };
            let mut limit_g = if b.drive_limit.is_finite() { b.drive_limit / G0 } else { max_g };
            if ui
                .add(egui::Slider::new(&mut limit_g, 0.01..=max_g).logarithmic(true).text("g drive limit"))
                .on_hover_text("Caps thrust for all orders. Drive emission scales with thrust, so lower is quieter.")
                .changed()
            {
                self.command(Command::SetDriveLimit { body: b.id, g: limit_g });
            }
            ui.label("Right-click: empty space to fly there and stop (burn, flip, brake); a planet, moon or star to orbit it; a ship or tracked contact to intercept it.");
            ui.horizontal(|ui| {
                if ui.button("All stop").on_hover_text("Brake to rest in the local frame").clicked() {
                    self.command(Command::AllStop { body: b.id });
                }
                if ui.button(if b.autopilot.is_some() { "Cancel order" } else { "Cut thrust" }).on_hover_text("Stop thrusting and coast").clicked() {
                    self.command(Command::SetThrust { body: b.id, thrust: Vec2::ZERO });
                }
            });
            let mut active = b.active_sensor;
            if ui.checkbox(&mut active, "Active sensor (ping)").on_hover_text("Ranges targets within ~10 ls, but the pings reveal you across AU.").changed() {
                self.command(Command::SetActiveSensor { body: b.id, on: active });
            }
        }
    }

    fn map(&mut self, ui: &mut egui::Ui, view: &View, overlay: Option<&(View, BTreeMap<ContactId, BodyId>)>) {
        let rect = ui.available_rect_before_wrap();
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, BACKGROUND);

        if self.fit_pending {
            self.fit(view, rect);
            self.fit_pending = false;
        }

        // Pan and zoom about the pointer.
        if resp.dragged_by(egui::PointerButton::Primary) || resp.dragged_by(egui::PointerButton::Middle) {
            let d = resp.drag_delta();
            self.camera.center = self.camera.center - Vec2::new(d.x as f64, -d.y as f64) * self.camera.km_per_px;
        }
        if let Some(hover) = resp.hover_pos() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y) as f64;
            if scroll != 0.0 {
                let before = to_world(&self.camera, rect, hover);
                self.camera.km_per_px = (self.camera.km_per_px * (-scroll * 0.002).exp()).clamp(1e-3, 1e8);
                let after = to_world(&self.camera, rect, hover);
                self.camera.center = self.camera.center + (before - after);
            }
        }

        let cam = self.camera;
        let mut labels = Labels::default();
        draw_range_rings(&painter, &cam, rect);
        draw_orbits(&painter, &cam, rect, view);

        // Sensor shadows cast from the selected own ship.
        if let Some(Selection::Body(id)) = self.selected
            && let Some(b) = view.bodies.iter().find(|b| b.id == id)
        {
            for c in &view.celestials {
                draw_shadow(&painter, &cam, rect, b.pos, c.pos, c.radius);
            }
        }

        for c in &view.celestials {
            let p = to_screen(&cam, rect, c.pos);
            let min_px = if c.kind == CelestialKind::Star { 6.0 } else { 3.0 };
            let r = ((c.radius / cam.km_per_px) as f32).max(min_px);
            let col = celestial_color(c.kind);
            if c.kind == CelestialKind::Star {
                painter.circle_filled(p, r * 2.2, col.gamma_multiply(0.08));
            }
            painter.circle_filled(p, r, col);
            if rect.contains(p) {
                labels.add(p + EVec2::new(r + 4.0, -r - 2.0), c.name.clone(), col.gamma_multiply(0.8));
            } else {
                draw_edge_marker(&painter, rect, p, &c.name, col, &mut labels);
            }
        }

        // Contacts: estimates with uncertainty, or bare bearing lines.
        for c in &view.contacts {
            let selected = self.selected == Some(Selection::Contact(c.id));
            draw_contact(&painter, &cam, rect, view, c, CONTACT, selected, &mut labels);
        }

        // Standing orders: target orbits and intercept lines.
        for b in &view.bodies {
            let Some(ap) = b.autopilot else { continue };
            let c = faction_color(b.faction).gamma_multiply(0.5);
            match ap.order {
                Order::Orbit { celestial, radius, .. } => {
                    let center = view.celestials[celestial].pos;
                    let pts: Vec<Pos2> = (0..=96)
                        .map(|k| {
                            let a = std::f64::consts::TAU * k as f64 / 96.0;
                            to_screen(&cam, rect, center + Vec2::new(a.cos(), a.sin()) * radius)
                        })
                        .collect();
                    painter.extend(Shape::dashed_line(&pts, Stroke::new(1.0, c), 6.0, 4.0));
                }
                Order::MoveTo { frame, offset } => {
                    let to = view.celestials[frame].pos + offset;
                    let (ps, pt) = (to_screen(&cam, rect, b.pos), to_screen(&cam, rect, to));
                    painter.extend(Shape::dashed_line(&[ps, pt], Stroke::new(1.0, c), 6.0, 4.0));
                    painter.circle_stroke(pt, 5.0, Stroke::new(1.0, c));
                    painter.line_segment([pt - EVec2::new(8.0, 0.0), pt + EVec2::new(8.0, 0.0)], Stroke::new(1.0, c));
                    painter.line_segment([pt - EVec2::new(0.0, 8.0), pt + EVec2::new(0.0, 8.0)], Stroke::new(1.0, c));
                }
                Order::Intercept(target) => {
                    let to = match target {
                        InterceptTarget::Own(o) => view.bodies.iter().find(|x| x.id == o).map(|x| x.pos),
                        InterceptTarget::Contact(ci) => view.contacts.iter().find(|x| x.id == ci).and_then(|x| x.track.as_ref()).map(|t| t.pos),
                    };
                    if let Some(to) = to {
                        let pts = [to_screen(&cam, rect, b.pos), to_screen(&cam, rect, to)];
                        painter.extend(Shape::dashed_line(&pts, Stroke::new(1.0, c), 6.0, 4.0));
                    }
                }
            }
        }

        // Own (or, for the spectator, all) ships.
        for b in &view.bodies {
            let c = faction_color(b.faction);
            let forecast = view.system.predict(State { pos: b.pos, vel: b.vel }, b.thrust, view.time, FORECAST_S, 120);
            let pts: Vec<Pos2> = forecast.points.iter().map(|&p| to_screen(&cam, rect, p)).collect();
            painter.extend(Shape::dotted_line(&pts, c.gamma_multiply(0.4), 6.0, 1.0));
            if forecast.impact.is_some()
                && let Some(&end) = pts.last()
            {
                draw_cross(&painter, end, DANGER);
            }
            let p = to_screen(&cam, rect, b.pos);
            draw_ship(&painter, p, b.vel, b.thrust, c, true);
            if b.active_sensor {
                painter.circle_stroke(p, 13.0, Stroke::new(1.0, c.gamma_multiply(0.5)));
            }
            if b.avoidance.active {
                let col = if b.avoidance.impossible { DANGER } else { Color32::from_rgb(255, 150, 60) };
                painter.circle_stroke(p, 16.0, Stroke::new(1.5, col));
            }
            if self.selected == Some(Selection::Body(b.id)) {
                painter.circle_stroke(p, 10.0, Stroke::new(1.0, Color32::WHITE));
            }
            labels.add(p + EVec2::new(10.0, -10.0), b.name.clone(), c);
        }

        // Spectator overlay: the chosen faction's belief, linked to truth.
        if let Some((ov, truth)) = overlay {
            for c in &ov.contacts {
                let real = truth.get(&c.id).and_then(|id| view.bodies.iter().find(|b| b.id == *id));
                draw_contact(&painter, &cam, rect, ov, c, BELIEF, false, &mut labels);
                if let (Some(t), Some(r)) = (&c.track, real) {
                    let pb = to_screen(&cam, rect, t.pos);
                    let pt = to_screen(&cam, rect, r.pos);
                    painter.line_segment([pb, pt], Stroke::new(1.0, BELIEF.gamma_multiply(0.5)));
                    painter.rect_stroke(Rect::from_center_size(pb, EVec2::splat(9.0)), 0.0, Stroke::new(1.0, BELIEF), StrokeKind::Middle);
                }
            }
        }

        labels.paint(&painter);
        draw_scale_bar(&painter, &cam, rect);

        // Orders. Right-click a ship or contact to intercept it, a celestial body to
        // orbit it, or empty space to fly there and stop.
        if let (Some(Selection::Body(id)), Some(click)) =
            (self.selected, resp.secondary_clicked().then(|| resp.interact_pointer_pos()).flatten())
            && let Some(b) = view.bodies.iter().find(|b| b.id == id)
            && self.own_faction() == Some(b.faction)
        {
            let near = |p: Vec2, r: f32| to_screen(&cam, rect, p).distance(click) < r;
            let own = view.bodies.iter().find(|o| o.id != id && near(o.pos, 14.0)).map(|o| InterceptTarget::Own(o.id));
            let contact = view
                .contacts
                .iter()
                .find(|c| c.track.as_ref().is_some_and(|t| near(t.pos, 14.0)))
                .map(|c| InterceptTarget::Contact(c.id));
            let celestial = view.celestials.iter().position(|c| {
                let r_px = (c.radius / cam.km_per_px) as f32;
                near(c.pos, r_px.max(8.0) + 4.0)
            });
            if let Some(target) = own.or(contact) {
                self.command(Command::Intercept { body: id, target });
            } else if let Some(celestial) = celestial {
                self.command(Command::Orbit { body: id, celestial });
            } else {
                self.command(Command::MoveTo { body: id, point: to_world(&cam, rect, click) });
            }
        }
        if resp.clicked()
            && let Some(click) = resp.interact_pointer_pos()
        {
            let bodies = view.bodies.iter().map(|b| (Selection::Body(b.id), b.pos));
            let contacts = view.contacts.iter().filter_map(|c| c.track.as_ref().map(|t| (Selection::Contact(c.id), t.pos)));
            if let Some(s) = bodies
                .chain(contacts)
                .map(|(s, p)| (s, to_screen(&cam, rect, p).distance(click)))
                .filter(|(_, d)| *d < 14.0)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(s, _)| s)
            {
                self.selected = Some(s);
            }
        }
    }

    /// Frame own ships and contacts (or everything, for the spectator).
    fn fit(&mut self, view: &View, rect: Rect) {
        let mut pts: Vec<Vec2> = view.bodies.iter().map(|b| b.pos).collect();
        pts.extend(view.contacts.iter().filter_map(|c| c.track.as_ref().map(|t| t.pos)));
        if pts.is_empty() {
            pts.push(Vec2::ZERO);
        }
        let (mut lo, mut hi) = (pts[0], pts[0]);
        for p in &pts {
            lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
        }
        let size = hi - lo;
        let km_per_px = (size.x / rect.width() as f64).max(size.y / rect.height() as f64) * 1.3;
        self.camera = Camera { center: (lo + hi) * 0.5, km_per_px: km_per_px.max(2.0 * LIGHT_SECOND / rect.width() as f64) };
    }
}

fn contact_details(ui: &mut egui::Ui, view: &View, c: &ContactView) {
    ui.separator();
    ui.heading(format!("Contact {}", contact_label(c)));
    ui.label("Identity unknown.");
    ui.label(format!(
        "Last seen by {} · light emitted T+ {}, reached flagship T+ {}",
        match c.last_source {
            Source::Emission => "its own emission",
            Source::Echo => "our ping's echo",
            Source::Ping => "its active ping",
        },
        fmt_time(c.last_emitted_at),
        fmt_time(c.last_received_at),
    ));
    ui.label(format!("SNR {:.1}", c.last_snr));
    if let Some(r) = c.last_range {
        ui.label(format!("Echo range {}", fmt_distance(r)));
    }
    match &c.track {
        Some(t) => {
            ui.label(format!("Estimated speed {:.1} km/s, thrust {:.1} g", t.vel.length(), t.accel.length() / G0));
            ui.label(format!("Position ±{} (2σ, now)", fmt_distance(2.0 * sigma_major(t.cov))));
            ui.label(format!("{} measurements; newest emitted {} ago", t.updates, fmt_age(view.time - t.updated_at)));
        }
        None => {
            ui.label("Bearing only: no range yet. A second ship's bearing or an active ping would fix it.");
        }
    }
}

/// Largest 1σ axis of a 2×2 covariance, km.
fn sigma_major(cov: [[f64; 2]; 2]) -> f64 {
    let (a, b, d) = (cov[0][0], cov[0][1], cov[1][1]);
    let tr = 0.5 * (a + d);
    let disc = (0.25 * (a - d).powi(2) + b * b).sqrt();
    (tr + disc).max(0.0).sqrt()
}

/// 2σ ellipse points of a covariance about `center`.
fn ellipse_points(center: Vec2, cov: [[f64; 2]; 2], n: usize) -> Vec<Vec2> {
    let (a, b, d) = (cov[0][0], cov[0][1], cov[1][1]);
    let tr = 0.5 * (a + d);
    let disc = (0.25 * (a - d).powi(2) + b * b).sqrt();
    let (l1, l2) = ((tr + disc).max(0.0), (tr - disc).max(0.0));
    let angle = 0.5 * (2.0 * b).atan2(a - d);
    let (s, c) = angle.sin_cos();
    (0..=n)
        .map(|i| {
            let th = std::f64::consts::TAU * i as f64 / n as f64;
            let (x, y) = (2.0 * l1.sqrt() * th.cos(), 2.0 * l2.sqrt() * th.sin());
            center + Vec2::new(c * x - s * y, s * x + c * y)
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn draw_contact(
    painter: &egui::Painter,
    cam: &Camera,
    rect: Rect,
    view: &View,
    c: &ContactView,
    color: Color32,
    selected: bool,
    labels: &mut Labels,
) {
    let age = view.time - c.last_emitted_at;
    match &c.track {
        Some(t) => {
            let pts: Vec<Pos2> = ellipse_points(t.pos, t.cov, 48).into_iter().map(|p| to_screen(cam, rect, p)).collect();
            painter.add(Shape::closed_line(pts, Stroke::new(1.0, color.gamma_multiply(0.6))));
            let forecast = view.system.predict(State { pos: t.pos, vel: t.vel }, t.accel, view.time, FORECAST_S, 60);
            let fp: Vec<Pos2> = forecast.points.iter().map(|&p| to_screen(cam, rect, p)).collect();
            painter.extend(Shape::dotted_line(&fp, color.gamma_multiply(0.3), 8.0, 1.0));
            let p = to_screen(cam, rect, t.pos);
            draw_ship(painter, p, t.vel, t.accel, color, false);
            if selected {
                painter.circle_stroke(p, 10.0, Stroke::new(1.0, Color32::WHITE));
            }
            labels.add(p + EVec2::new(10.0, -10.0), format!("{} · {}", contact_label(c), fmt_age(age)), color);
        }
        None => {
            for b in &c.bearings {
                let o = to_screen(cam, rect, b.origin);
                let dir = EVec2::new(b.bearing.cos() as f32, -b.bearing.sin() as f32);
                let far = o + dir * (rect.width() + rect.height()) * 2.0;
                painter.line_segment([o, far], Stroke::new(if selected { 1.5 } else { 1.0 }, color.gamma_multiply(0.45)));
                let tip = clip_to_rect(rect, o, dir).unwrap_or(o + dir * 60.0);
                labels.add(tip - dir * 30.0, format!("{} · {}", contact_label(c), fmt_age(age)), color);
            }
        }
    }
}

/// Where a ray from `o` along `dir` leaves `rect`, if `o` is inside.
fn clip_to_rect(rect: Rect, o: Pos2, dir: EVec2) -> Option<Pos2> {
    if !rect.contains(o) {
        return None;
    }
    let tx = if dir.x > 0.0 { (rect.right() - o.x) / dir.x } else if dir.x < 0.0 { (rect.left() - o.x) / dir.x } else { f32::INFINITY };
    let ty = if dir.y > 0.0 { (rect.bottom() - o.y) / dir.y } else if dir.y < 0.0 { (rect.top() - o.y) / dir.y } else { f32::INFINITY };
    Some(o + dir * tx.min(ty))
}

/// Greedy label placement: move a label down until it no longer overlaps.
#[derive(Default)]
struct Labels {
    items: Vec<(Pos2, String, Color32)>,
}

impl Labels {
    fn add(&mut self, at: Pos2, text: String, color: Color32) {
        self.items.push((at, text, color));
    }

    fn paint(self, painter: &egui::Painter) {
        let font = egui::FontId::proportional(12.0);
        let mut placed: Vec<Rect> = vec![];
        for (at, text, color) in self.items {
            let galley = painter.layout_no_wrap(text, font.clone(), color);
            let size = galley.size();
            let mut r = Rect::from_min_size(at - EVec2::new(0.0, size.y), size);
            for _ in 0..12 {
                if !placed.iter().any(|p| p.expand(1.0).intersects(r)) {
                    break;
                }
                r = r.translate(EVec2::new(0.0, size.y + 1.0));
            }
            if r.min != at - EVec2::new(0.0, size.y) {
                painter.line_segment([at, r.left_center()], Stroke::new(0.5, color.gamma_multiply(0.4)));
            }
            painter.galley(r.min, galley, color);
            placed.push(r);
        }
    }
}

fn draw_scale_bar(painter: &egui::Painter, cam: &Camera, rect: Rect) {
    let target_km = 150.0 * cam.km_per_px;
    let unit = if target_km >= 0.1 * AU { AU } else if target_km >= LIGHT_SECOND { LIGHT_SECOND } else { 1.0 };
    let raw = target_km / unit;
    let mag = 10f64.powf(raw.log10().floor());
    let nice = [1.0, 2.0, 5.0, 10.0].into_iter().map(|m| m * mag).rfind(|v| *v <= raw).unwrap_or(mag);
    let km = nice * unit;
    let px = (km / cam.km_per_px) as f32;
    let y = rect.bottom() - 20.0;
    let x = rect.left() + 20.0;
    let label = if unit == AU {
        format!("{nice} AU")
    } else if unit == LIGHT_SECOND {
        format!("{nice} ls")
    } else {
        format!("{nice} km")
    };
    painter.line_segment([Pos2::new(x, y), Pos2::new(x + px, y)], Stroke::new(2.0, Color32::LIGHT_GRAY));
    painter.text(Pos2::new(x, y - 6.0), egui::Align2::LEFT_BOTTOM, label, egui::FontId::proportional(12.0), Color32::LIGHT_GRAY);
}

/// Dim distance rings about the star, spaced to suit the zoom level.
fn draw_range_rings(painter: &egui::Painter, cam: &Camera, rect: Rect) {
    let view_km = cam.km_per_px * rect.width().max(rect.height()) as f64;
    let Some(spacing) = [0.1, 0.5, 1.0, 5.0, 10.0].into_iter().map(|a| a * AU).find(|s| view_km / s <= 12.0) else { return };
    if view_km / spacing < 1.5 {
        return;
    }
    let origin = to_screen(cam, rect, Vec2::ZERO);
    let far = [rect.left_top(), rect.right_top(), rect.left_bottom(), rect.right_bottom()]
        .into_iter()
        .map(|p| to_world(cam, rect, p).length())
        .fold(0.0, f64::max)
        .min(100.0 * AU);
    let near = if rect.contains(origin) { 0.0 } else { (to_world(cam, rect, rect.center()).length() - view_km).max(0.0) };
    let line = Color32::from_rgb(28, 34, 48);
    let text = Color32::from_rgb(80, 92, 118);
    let mut r = (near / spacing).floor().max(1.0) * spacing;
    while r <= far {
        painter.circle_stroke(origin, (r / cam.km_per_px) as f32, Stroke::new(1.0, line));
        // Label where the ring crosses the view's horizontal centre line, if it does.
        let dy = (rect.center().y - origin.y) as f64 * cam.km_per_px;
        if r > dy.abs() {
            let dx = (r * r - dy * dy).sqrt() / cam.km_per_px;
            for x in [origin.x + dx as f32, origin.x - dx as f32] {
                let p = Pos2::new(x, rect.center().y);
                if rect.contains(p) {
                    painter.text(p + EVec2::new(3.0, 0.0), egui::Align2::LEFT_CENTER, fmt_distance(r), egui::FontId::proportional(10.0), text);
                }
            }
        }
        r += spacing;
    }
}

fn draw_orbits(painter: &egui::Painter, cam: &Camera, rect: Rect, view: &View) {
    for (i, c) in view.system.bodies.iter().enumerate() {
        if let Orbit::Circular { parent, radius, .. } = c.orbit {
            let r = (radius / cam.km_per_px) as f32;
            if r < 4.0 {
                continue;
            }
            let center = to_screen(cam, rect, view.system.state(parent, view.time).pos);
            painter.circle_stroke(center, r, Stroke::new(1.0, celestial_color(view.celestials[i].kind).gamma_multiply(0.18)));
        }
    }
}

/// The region a celestial body hides from an observer at `eye`.
fn draw_shadow(painter: &egui::Painter, cam: &Camera, rect: Rect, eye: Vec2, center: Vec2, radius: f64) {
    let d = center - eye;
    let dist = d.length();
    if dist <= radius {
        return;
    }
    let half = (radius / dist).asin();
    let base = d.y.atan2(d.x);
    let reach = dist + (rect.width() + rect.height()) as f64 * cam.km_per_px * 2.0;
    let tangent = (dist * dist - radius * radius).sqrt();
    let edge = |a: f64, len: f64| eye + Vec2::new(a.cos(), a.sin()) * len;
    let pts = [edge(base + half, tangent), edge(base + half, reach), edge(base - half, reach), edge(base - half, tangent)];
    let screen: Vec<Pos2> = pts.iter().map(|&p| to_screen(cam, rect, p)).collect();
    if (screen[1] - screen[2]).length() < 1.0 && (screen[0] - screen[3]).length() < 1.0 {
        return;
    }
    painter.add(Shape::convex_polygon(screen.clone(), Color32::from_rgba_unmultiplied(0, 0, 0, 110), Stroke::NONE));
    let edge_col = Color32::from_rgb(40, 44, 60);
    painter.line_segment([screen[0], screen[1]], Stroke::new(1.0, edge_col));
    painter.line_segment([screen[3], screen[2]], Stroke::new(1.0, edge_col));
}

/// A small arrow at the map edge pointing toward an off-screen object.
fn draw_edge_marker(painter: &egui::Painter, rect: Rect, target: Pos2, name: &str, color: Color32, labels: &mut Labels) {
    let inner = rect.shrink(18.0);
    let dir = (target - inner.center()).normalized();
    let Some(at) = clip_to_rect(inner, inner.center(), dir) else { return };
    let side = EVec2::new(-dir.y, dir.x);
    painter.add(Shape::convex_polygon(vec![at + dir * 8.0, at - dir * 4.0 + side * 5.0, at - dir * 4.0 - side * 5.0], color.gamma_multiply(0.7), Stroke::NONE));
    labels.add(at - dir * 14.0 + EVec2::new(-10.0, 0.0), name.to_string(), color.gamma_multiply(0.7));
}

fn draw_cross(painter: &egui::Painter, p: Pos2, color: Color32) {
    let s = 5.0;
    painter.line_segment([p + EVec2::new(-s, -s), p + EVec2::new(s, s)], Stroke::new(2.0, color));
    painter.line_segment([p + EVec2::new(-s, s), p + EVec2::new(s, -s)], Stroke::new(2.0, color));
}

/// Screen direction (y up in world, y down on screen) of a world vector.
fn screen_dir(v: Vec2) -> EVec2 {
    let n = v.normalized();
    EVec2::new(n.x as f32, -n.y as f32)
}

/// A ship: arrow along thrust (or velocity when coasting), with its velocity vector
/// trailing behind. Tail length is log-scaled so 1 km/s and 0.1c both read.
fn draw_ship(painter: &egui::Painter, p: Pos2, vel: Vec2, thrust: Vec2, color: Color32, solid: bool) {
    let speed = vel.length();
    let facing = if thrust.length() > 0.0 { thrust } else { vel };
    let f = if facing.length() > 0.0 { screen_dir(facing) } else { EVec2::new(0.0, -1.0) };
    let side = EVec2::new(-f.y, f.x);

    if speed > 0.0 {
        let tail_px = 10.0 * (1.0 + speed.log10().max(0.0)) as f32;
        let tail = p - screen_dir(vel) * tail_px;
        painter.line_segment([p, tail], Stroke::new(1.5, color.gamma_multiply(0.6)));
    }

    let (len, half_w) = (10.0, 5.5);
    let nose = p + f * len;
    let pts = vec![nose, p - f * (len * 0.5) + side * half_w, p - f * (len * 0.2), p - f * (len * 0.5) - side * half_w];
    let shape = if solid { Shape::convex_polygon(pts, color, Stroke::NONE) } else { Shape::closed_line(pts, Stroke::new(1.2, color)) };
    painter.add(shape);

    if thrust.length() > 0.0 {
        let flame = p - f * (len * 0.2);
        painter.line_segment([flame, flame - f * 6.0], Stroke::new(2.0, Color32::from_rgb(255, 200, 120)));
    }
}

fn to_screen(cam: &Camera, rect: Rect, p: Vec2) -> Pos2 {
    let d = (p - cam.center) * (1.0 / cam.km_per_px);
    // Clamp so far-off geometry does not overflow f32 painting.
    let (x, y) = (d.x.clamp(-1e6, 1e6), d.y.clamp(-1e6, 1e6));
    rect.center() + EVec2::new(x as f32, -y as f32)
}

fn to_world(cam: &Camera, rect: Rect, p: Pos2) -> Vec2 {
    let d = p - rect.center();
    cam.center + Vec2::new(d.x as f64, -d.y as f64) * cam.km_per_px
}
