//! The debrief. Once the game is decided the session releases the truth, and the plot
//! replays it: every platform's real path, and beside each enemy the track the side
//! actually steered by, so the player sees what they knew against what was there.
use super::*;
use luminal_core::world::debrief::{Debrief, DebriefPath, SideStats};
use luminal_core::world::LossCause;

/// Where the side believed each of its tracks to be, sampled on the game clock while it
/// played. This is the faction's own picture, recorded by the client; it holds no truth.
#[derive(Default)]
pub(crate) struct BeliefLog {
    tracks: BTreeMap<ContactId, Vec<(f64, Vec2)>>,
    last: Option<f64>,
}

impl BeliefLog {
    const EVERY_S: f64 = 30.0;

    pub(crate) fn observe(&mut self, view: &View) {
        if self.last.is_some_and(|last| view.time < last + Self::EVERY_S) { return; }
        self.last = Some(view.time);
        for c in view.contacts.iter().filter(|c| !c.resolved_missile && !c.stale) {
            if let Some(track) = &c.track { self.tracks.entry(c.id).or_default().push((view.time, track.pos)); }
        }
    }

    /// The believed position at `t`, only while the track was being held.
    fn at(&self, id: ContactId, t: f64) -> Option<Vec2> {
        let samples = self.tracks.get(&id)?;
        let i = samples.partition_point(|(st, _)| *st <= t);
        if i == 0 { return None; }
        let (t0, p0) = samples[i - 1];
        match samples.get(i) {
            // A gap longer than a few samples means the track was dropped in between.
            Some(&(t1, p1)) if t1 - t0 <= 4.0 * Self::EVERY_S => Some(p0 + (p1 - p0) * ((t - t0) / (t1 - t0))),
            Some(_) => (t - t0 <= Self::EVERY_S).then_some(p0),
            None => (t - t0 <= 2.0 * Self::EVERY_S).then_some(p0),
        }
    }

    fn trail(&self, id: ContactId, until: f64) -> Vec<Vec2> {
        self.tracks.get(&id).map(|s| s.iter().take_while(|(t, _)| *t <= until).map(|(_, p)| *p).collect()).unwrap_or_default()
    }
}

pub(crate) struct DebriefState {
    pub data: Debrief,
    pub playhead: f64,
    pub playing: bool,
    /// Game seconds per wall second.
    pub speed: f64,
    pub own: Option<FactionId>,
    pub show_belief: bool,
    pub honours: Vec<service_record::Honour>,
}

/// The whole replay takes this long at the opening speed.
const REPLAY_WALL_S: f64 = 45.0;

impl DebriefState {
    pub(crate) fn new(data: Debrief, own: Option<FactionId>) -> Self {
        let speed = (data.end / REPLAY_WALL_S).max(1.0);
        Self { data, playhead: 0.0, playing: true, speed, own, show_belief: true, honours: vec![] }
    }

    pub(crate) fn advance(&mut self, wall_dt: f64) {
        if !self.playing { return; }
        self.playhead = (self.playhead + wall_dt * self.speed).min(self.data.end);
        if self.playhead >= self.data.end { self.playing = false; }
    }

    /// Frame every platform's path, rounds aside.
    pub(crate) fn framing(&self, rect: Rect) -> Option<Camera> {
        let pts: Vec<Vec2> = self.data.paths.iter().filter(|p| p.kind != BodyKind::Missile)
            .flat_map(|p| p.samples.iter().map(|(_, pos)| *pos)).collect();
        let first = *pts.first()?;
        let (lo, hi) = pts.iter().fold((first, first), |(lo, hi), p| (Vec2::new(lo.x.min(p.x), lo.y.min(p.y)), Vec2::new(hi.x.max(p.x), hi.y.max(p.y))));
        let size = hi - lo;
        let km_per_px = (size.x / f64::from(rect.width().max(100.0))).max(size.y / f64::from(rect.height().max(100.0))) * 1.25;
        Some(Camera { center: (lo + hi) * 0.5, km_per_px: km_per_px.max(2.0 * LIGHT_SECOND / f64::from(rect.width().max(100.0))) })
    }

    fn color(&self, path: &DebriefPath) -> Color32 {
        if Some(path.faction) == self.own { FRIEND } else { ENEMY }
    }

    /// The replay on the plot: true paths to the playhead, markers where each platform
    /// really was, and the side's own track beside each enemy it held.
    pub(crate) fn paint(&self, painter: &egui::Painter, cam: &Camera, rect: Rect, belief: &BeliefLog, wall: f64) {
        painter.rect_filled(rect, 0.0, Color32::from_rgba_unmultiplied(2, 5, 10, 170));
        let now = self.playhead;
        let screen = |p: Vec2| to_screen(cam, rect, p);
        // A step no ship could make is a jump: break the line there.
        let jumped = |a: &(f64, Vec2), b: &(f64, Vec2)| (b.1 - a.1).length() > 1.5 * C * (b.0 - a.0).max(1.0);
        let mut order: Vec<&DebriefPath> = self.data.paths.iter().collect();
        order.sort_by_key(|p| (p.kind != BodyKind::Missile, p.faction));
        for path in order {
            let round = path.kind == BodyKind::Missile;
            let col = if path.decoy { pal().warm } else { self.color(path) };
            let (past_alpha, width) = if round { (if path.interceptor { 0.18 } else { 0.32 }, 1.0) } else { (0.9, 2.0) };
            let stroke_runs = |from: usize, to: usize, alpha: f32, w: f32| {
                let mut run: Vec<Pos2> = vec![];
                for i in from..to {
                    if i > from && jumped(&path.samples[i - 1], &path.samples[i]) {
                        if run.len() > 1 { painter.add(Shape::line(std::mem::take(&mut run), Stroke::new(w, col.gamma_multiply(alpha)))); }
                        run.clear();
                    }
                    run.push(screen(path.samples[i].1));
                }
                if run.len() > 1 { painter.add(Shape::line(run, Stroke::new(w, col.gamma_multiply(alpha)))); }
            };
            let split = path.samples.partition_point(|(t, _)| *t <= now);
            if split == 0 { continue; }
            if !round { stroke_runs(split.saturating_sub(1), path.samples.len(), 0.12, 1.0); }
            stroke_runs(0, split, past_alpha, width);
            let ended = path.end.filter(|(t, _)| *t <= now);
            if let Some((t, cause)) = ended {
                if round { continue; }
                let at = screen(path.at(t).unwrap_or(path.samples[split - 1].1));
                let departed = matches!(cause, LossCause::Withdrawn | LossCause::Surrendered);
                if path.decoy && matches!(cause, LossCause::Expended) {
                    painter.circle_stroke(at, 4.0, Stroke::new(1.0, col.gamma_multiply(0.5)));
                    painter.text(at + EVec2::new(8.0, 3.0), egui::Align2::LEFT_TOP, format!("{} decoy · burned out", path.name), mono(9.0), col.gamma_multiply(0.55));
                    continue;
                }
                if departed {
                    painter.circle_stroke(at, 6.0, Stroke::new(1.0, col.gamma_multiply(0.7)));
                } else {
                    // A loss flares, then leaves a cross.
                    let age = ((now - t) / (0.025 * self.data.end.max(1.0))) as f32;
                    if age < 1.0 { fx::glow(painter, at, 10.0 + 40.0 * age, col, 1.0 - age); }
                    let s = 5.0;
                    for d in [EVec2::new(s, s), EVec2::new(s, -s)] { painter.line_segment([at - d, at + d], Stroke::new(2.0, col)); }
                }
                let name = if path.decoy { format!("{} decoy", path.name) } else { path.name.clone() };
                painter.text(at + EVec2::new(9.0, 4.0), egui::Align2::LEFT_TOP, format!("{name} · {}", loss_word(cause)), mono(9.5), col.gamma_multiply(0.75));
                continue;
            }
            let Some(pos) = path.at(now) else { continue };
            let p = screen(pos);
            if round {
                painter.circle_filled(p, if path.interceptor { 1.2 } else { 1.8 }, col.gamma_multiply(0.9));
                continue;
            }
            let ahead = path.at((now + 60.0).min(path.samples.last().unwrap().0)).unwrap_or(pos);
            let heading = if (ahead - pos).length() > 1.0 { ahead - pos } else { Vec2::new(1.0, 0.0) };
            draw_ship(painter, p, Vec2::ZERO, heading, col, false, path.ship_class);
            let label = if path.decoy { format!("{} decoy", path.name) } else { path.name.clone() };
            painter.text(p + EVec2::new(11.0, -14.0), egui::Align2::LEFT_TOP, label, egui::FontId::proportional(11.5), col);
        }
        // Bursts and hits, as they happened.
        for m in self.data.moments.iter().filter(|m| m.t <= now) {
            let age = ((now - m.t) / (0.012 * self.data.end.max(1.0))) as f32;
            if age >= 1.0 { continue; }
            let col = match m.kind { CombatKind::NuclearBurst => Color32::from_rgb(255, 214, 150), CombatKind::JumpArrival => pal().accent, _ => Color32::WHITE };
            fx::glow(painter, screen(m.pos), 6.0 + 26.0 * age, col, 1.0 - age);
        }
        if !self.show_belief { return; }
        // What we steered by: each enemy we held, where we thought it was.
        let amber = BELIEF;
        let flicker = 0.75 + 0.25 * ((wall * 3.0).sin() as f32);
        for &(contact, body) in &self.data.identities {
            let Some(path) = self.data.paths.iter().find(|p| p.body == body) else { continue };
            if path.kind == BodyKind::Missile || Some(path.faction) == self.own { continue; }
            let trail = belief.trail(contact, now);
            if trail.len() > 1 {
                let pts: Vec<Pos2> = trail.iter().map(|p| screen(*p)).collect();
                painter.add(Shape::dashed_line(&pts, Stroke::new(1.0, amber.gamma_multiply(0.45)), 5.0, 4.0));
            }
            let Some(believed) = belief.at(contact, now) else { continue };
            let b = screen(believed);
            painter.circle_stroke(b, 7.0, Stroke::new(1.2, amber.gamma_multiply(flicker)));
            if path.end.is_none_or(|(t, _)| t > now) && let Some(truth) = path.at(now) {
                let t = screen(truth);
                if b.distance(t) > 9.0 { painter.add(Shape::dashed_line(&[b, t], Stroke::new(1.0, amber.gamma_multiply(0.6)), 3.0, 3.0)); }
                painter.text(b + EVec2::new(9.0, 2.0), egui::Align2::LEFT_TOP, format!("T{} · ours, off by {}", contact.0, fmt_distance((truth - believed).length())), mono(9.0), amber.gamma_multiply(0.85));
            }
        }
    }

    /// The bottom deck while the debrief is open: timeline, controls and the count.
    /// Returns true when the player asks for the live plot back.
    pub(crate) fn deck(&mut self, ui: &mut egui::Ui, outcome: Option<&luminal_core::world::Outcome>) -> bool {
        let mut close = false;
        ui.horizontal(|ui| {
            ui.add_space(10.0);
            ui.vertical(|ui| {
                ui.set_width((ui.available_width() * 0.46).max(420.0));
                let (title, col) = match (outcome, self.own) {
                    (Some(o), Some(f)) if o.winner == f => ("VICTORY", FRIEND),
                    (Some(_), Some(_)) => ("DEFEAT", DANGER),
                    _ => ("THE RECORD", pal().text_hi),
                };
                ui.label(egui::RichText::new(format!("DEBRIEF · {title}")).monospace().size(18.0).color(col));
                if let Some(o) = outcome {
                    ui.label(egui::RichText::new(format!("T+ {} · {}", fmt_time(o.t), o.reason)).monospace().size(11.0).color(pal().text_muted));
                }
                ui.add_space(6.0);
                ui.label(egui::RichText::new("Solid lines are where every ship really was. Amber is where your side believed the enemy to be, at the time.")
                    .size(11.0).color(pal().text_muted));
                ui.add_space(8.0);
                self.timeline(ui);
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if tac_button(ui, if self.playing { "PAUSE" } else { "PLAY" }, EVec2::new(80.0, 26.0), pal().accent, self.playing, true).clicked() {
                        if !self.playing && self.playhead >= self.data.end { self.playhead = 0.0; }
                        self.playing = !self.playing;
                    }
                    let base = (self.data.end / REPLAY_WALL_S).max(1.0);
                    for (label, factor) in [("½×", 0.5), ("1×", 1.0), ("4×", 4.0)] {
                        if tac_button(ui, label, EVec2::new(46.0, 26.0), pal().accent, (self.speed - base * factor).abs() < 1e-6, true).clicked() { self.speed = base * factor; }
                    }
                    if tac_button(ui, "BELIEF", EVec2::new(80.0, 26.0), BELIEF, self.show_belief, true)
                        .on_hover_text("Show where your side believed each enemy was").clicked() { self.show_belief = !self.show_belief; }
                    close = tac_button(ui, "LIVE PLOT", EVec2::new(96.0, 26.0), pal().warm, false, true).clicked();
                });
                ui.add_space(8.0);
                self.key_moments(ui);
            });
            ui.add_space(16.0);
            ui.vertical(|ui| self.ledger(ui));
        });
        close
    }

    fn timeline(&mut self, ui: &mut egui::Ui) {
        let (rect, resp) = ui.allocate_exact_size(EVec2::new(ui.available_width(), 46.0), Sense::click_and_drag());
        let p = ui.painter();
        let bar = Rect::from_min_max(rect.left_center() - EVec2::new(0.0, 3.0), rect.right_center() + EVec2::new(0.0, 3.0));
        p.rect_filled(bar, 2.0, pal().well_bg);
        let end = self.data.end.max(1.0);
        let x = |t: f64| bar.left() + bar.width() * (t / end).clamp(0.0, 1.0) as f32;
        p.rect_filled(Rect::from_min_max(bar.min, Pos2::new(x(self.playhead), bar.max.y)), 2.0, pal().accent.gamma_multiply(0.5));
        for side in &self.data.sides {
            let col = if Some(side.faction) == self.own { FRIEND } else { ENEMY };
            if let Some(t) = side.first_sighting {
                p.line_segment([Pos2::new(x(t), bar.top() - 12.0), Pos2::new(x(t), bar.top())], Stroke::new(1.5, col));
                p.text(Pos2::new(x(t), bar.top() - 13.0), egui::Align2::CENTER_BOTTOM, "◆", mono(8.0), col);
            }
        }
        for m in &self.data.moments {
            let (col, tall) = match m.kind {
                CombatKind::Destroyed if m.ship => (if m.faction == self.own { FRIEND } else { ENEMY }, 12.0),
                CombatKind::NuclearBurst => (Color32::from_rgb(255, 214, 150), 5.0),
                CombatKind::MissileHit => (Color32::WHITE, 7.0),
                CombatKind::JumpArrival | CombatKind::Withdrawn | CombatKind::Surrendered => (pal().accent, 9.0),
                _ => continue,
            };
            p.line_segment([Pos2::new(x(m.t), bar.bottom()), Pos2::new(x(m.t), bar.bottom() + tall)], Stroke::new(1.0, col.gamma_multiply(0.8)));
        }
        let head = x(self.playhead);
        p.line_segment([Pos2::new(head, rect.top()), Pos2::new(head, rect.bottom())], Stroke::new(2.0, pal().text_hi));
        let (x, align) = if head > bar.right() - 90.0 { (head - 4.0, egui::Align2::RIGHT_TOP) } else { (head + 4.0, egui::Align2::LEFT_TOP) };
        p.text(Pos2::new(x, rect.top()), align, format!("T+ {}", fmt_time(self.playhead)), mono(9.5), pal().text_hi);
        if let Some(at) = resp.interact_pointer_pos() && (resp.dragged() || resp.clicked()) {
            self.playhead = f64::from((at.x - bar.left()) / bar.width()).clamp(0.0, 1.0) * end;
            self.playing = false;
        }
        resp.on_hover_text("Drag to scrub. ◆ marks when each side first had word of the other. Ticks: losses (tall), hits, bursts and jumps.");
    }

    /// The turning points, in order. Clicking one moves the replay there.
    fn key_moments(&mut self, ui: &mut egui::Ui) {
        let own = self.own;
        let side = |f: FactionId| if Some(f) == own { "You" } else { "The enemy" };
        let mut moments: Vec<(f64, String, Color32)> = vec![];
        for s in &self.data.sides {
            let col = if Some(s.faction) == own { FRIEND } else { ENEMY };
            if let Some(t) = s.first_sighting { moments.push((t, format!("{} had first word of the other side", side(s.faction)), col)); }
            if let Some(t) = s.first_fix { moments.push((t, format!("{} held a firing fix", side(s.faction)), col)); }
        }
        for p in self.data.paths.iter().filter(|p| p.kind == BodyKind::Ship) {
            let col = if Some(p.faction) == own { FRIEND } else { ENEMY };
            if p.decoy {
                if let Some(&(t, _)) = p.samples.first() && t > 0.0 { moments.push((t, format!("{} released a decoy", side(p.faction)), col)); }
                continue;
            }
            if let Some((t, cause)) = p.end { moments.push((t, format!("{} {}", p.name, loss_word(cause)), col)); }
        }
        for m in self.data.moments.iter().filter(|m| m.kind == CombatKind::JumpArrival) {
            moments.push((m.t, format!("{} jumped in", m.faction.map_or("A ship", side)), pal().accent));
        }
        moments.sort_by(|a, b| a.0.total_cmp(&b.0));
        moments.dedup_by(|a, b| a.1 == b.1 && (a.0 - b.0).abs() < 1.0);
        let current = moments.iter().rposition(|(t, _, _)| *t <= self.playhead);
        let first = current.map_or(0, |i| i.saturating_sub(2)).min(moments.len().saturating_sub(5));
        for (i, (t, text, col)) in moments.iter().enumerate().skip(first).take(5) {
            let dim = if Some(i) == current { 1.0 } else if current.is_some_and(|c| i < c) { 0.55 } else { 0.4 };
            let row = ui.add(egui::Label::new(egui::RichText::new(format!("T+ {}   {text}", fmt_time(*t))).monospace().size(10.5).color(col.gamma_multiply(dim)))
                .sense(Sense::click()));
            if row.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() { self.playhead = *t; self.playing = false; }
        }
    }

    fn ledger(&self, ui: &mut egui::Ui) {
        let own = self.data.sides.iter().find(|s| Some(s.faction) == self.own);
        let them = self.data.sides.iter().find(|s| Some(s.faction) != self.own);
        let (Some(a), Some(b)) = (own.or(self.data.sides.first()), them.or(self.data.sides.get(1))) else { return };
        let (left, right) = if self.own.is_some() { ("YOUR SIDE", "THE ENEMY") } else { ("SIDE ONE", "SIDE TWO") };
        let hit_rate = |s: &SideStats| if s.missiles_launched > 0 { format!("{} ({:.0}%)", s.missile_hits, 100.0 * s.missile_hits as f64 / s.missiles_launched as f64) } else { "0".into() };
        let when = |t: Option<f64>| t.map_or("never".into(), |t| format!("T+ {}", fmt_time(t)));
        let rows: Vec<(&str, String, String)> = vec![
            ("Ships", a.ships.to_string(), b.ships.to_string()),
            ("Ships lost", a.ships_lost.to_string(), b.ships_lost.to_string()),
            ("Ships destroyed", a.kills.to_string(), b.kills.to_string()),
            ("Missiles fired", a.missiles_launched.to_string(), b.missiles_launched.to_string()),
            ("Missile hits", hit_rate(a), hit_rate(b)),
            ("Interceptors fired", a.interceptors_launched.to_string(), b.interceptors_launched.to_string()),
            ("Rounds stopped", a.missiles_stopped.to_string(), b.missiles_stopped.to_string()),
            ("Beam pulses", a.beam_pulses.to_string(), b.beam_pulses.to_string()),
            ("Decoys released", a.decoys_released.to_string(), b.decoys_released.to_string()),
            ("First word of the enemy", when(a.first_sighting), when(b.first_sighting)),
            ("First firing fix", when(a.first_fix), when(b.first_fix)),
        ];
        egui::Grid::new("debrief_ledger").num_columns(3).spacing(EVec2::new(26.0, 3.0)).show(ui, |ui| {
            ui.label("");
            ui.label(egui::RichText::new(left).monospace().size(11.0).color(FRIEND));
            ui.label(egui::RichText::new(right).monospace().size(11.0).color(ENEMY));
            ui.end_row();
            for (name, x, y) in rows {
                ui.label(egui::RichText::new(name).size(11.5).color(pal().text_muted));
                ui.label(egui::RichText::new(x).monospace().size(11.5).color(pal().text_hi));
                ui.label(egui::RichText::new(y).monospace().size(11.5).color(pal().text_hi));
                ui.end_row();
            }
        });
        if !self.honours.is_empty() {
            ui.add_space(8.0);
            ui.label(egui::RichText::new("COMMENDATIONS").monospace().size(11.0).color(BELIEF));
            for h in &self.honours {
                ui.label(egui::RichText::new(format!("★ {} · {}", h.name(), h.citation())).size(11.5).color(BELIEF.gamma_multiply(0.9)));
            }
        }
        if let (Some(x), Some(y)) = (a.first_sighting, b.first_sighting) {
            let gap = y - x;
            let line = if gap.abs() < 60.0 { "Both sides had word of each other within the same minute.".to_string() }
                else if gap > 0.0 { format!("You had word of them {} before they had word of you.", fmt_age(gap)) }
                else { format!("They had word of you {} before you had word of them.", fmt_age(-gap)) };
            ui.add_space(6.0);
            ui.label(egui::RichText::new(line).size(12.0).color(if gap >= 0.0 { FRIEND } else { pal().warm }));
        }
    }
}

fn loss_word(cause: LossCause) -> &'static str {
    match cause {
        LossCause::Withdrawn => "withdrew",
        LossCause::Surrendered => "surrendered",
        LossCause::Impact(_) => "struck a world",
        LossCause::Missile { .. } => "destroyed by missile",
        LossCause::ShipBeam { .. } => "destroyed by beam",
        LossCause::PointDefence { .. } => "destroyed by point defence",
        LossCause::Interceptor { .. } => "destroyed by interceptor",
        _ => "lost",
    }
}
