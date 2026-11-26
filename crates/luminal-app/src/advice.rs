//! The PWO's advice: one line on the plot naming the thing that matters most now.
//!
//! The Principal Warfare Officer reads the same picture the player does, the side's own
//! view, and nothing else. Advice is a suggestion with a reason, never an order, and it
//! never tells the player what the enemy is really doing.
use super::*;
use luminal_core::world::terrain::star_separation;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Advice {
    pub text: String,
    pub urgent: bool,
}

fn advice(text: impl Into<String>, urgent: bool) -> Option<Advice> { Some(Advice { text: text.into(), urgent }) }

/// The most pressing suggestion for `ship`, from the side's own view.
pub(crate) fn pwo_advice(view: &View, ship: &BodyView) -> Option<Advice> {
    if view.outcome.is_some() || ship.jump.is_some() { return None; }
    let heat = ship.thermal.heat_fraction();
    let inbound: Vec<&ContactView> = view.contacts.iter().filter(|c| c.resolved_missile && !c.resolved_interceptor && !c.stale)
        .filter(|c| c.track.as_ref().is_some_and(|t| (t.vel - ship.vel).dot(ship.pos - t.pos) > 0.0))
        .collect();
    if heat >= 0.85 {
        return advice(format!("Heat at {:.0}%. Ease the drive, or open radiators to dump it: the drive and lasers stop while they are open.", heat * 100.0), true);
    }
    if !inbound.is_empty() {
        let nearest = inbound.iter().filter_map(|c| c.track.as_ref().map(|t| (t.pos - ship.pos).length())).fold(f64::INFINITY, f64::min);
        let decoy = if ship.decoys > 0 { " A decoy now takes their track with it." } else { "" };
        return advice(format!("{} round{} inbound, nearest {}. Interceptors and point defence fire themselves; EVADE AUTO dodges.{decoy}",
            inbound.len(), if inbound.len() == 1 { "" } else { "s" }, fmt_distance(nearest)), true);
    }
    if heat >= 0.6 {
        return advice(format!("Heat at {:.0}%. Above half, thrust starts to fall away.", heat * 100.0), false);
    }
    let ships: Vec<&ContactView> = view.contacts.iter()
        .filter(|c| !c.stale && !c.resolved_missile && !c.decoy && !matches!(c.resolved_kind, Some(BodyKind::Station | BodyKind::Probe)))
        .collect();
    if ships.is_empty() {
        return advice(if ship.controls.active == luminal_core::world::controls::Mode::Auto {
            "Nothing on the plot. Active sensors are pinging: we will see them sooner, and they will see us much sooner."
        } else {
            "Nothing on the plot. Run quiet and listen, or PING to look: a ping is heard about ten times farther than it sees."
        }, false);
    }
    // Are we against the star from where we believe any of them to be?
    let glare: Vec<String> = ships.iter().filter_map(|c| c.track.as_ref().map(|t| (c, t.pos)))
        .filter(|(_, from)| star_separation(&view.system, *from, ship.pos, view.time).is_some_and(|r| r < params::SOLAR_GLARE_HALO_RADII.value))
        .map(|(c, _)| contact_label(c)).collect();
    let nearest = ships.iter().filter_map(|c| c.track.as_ref().map(|t| (*c, t, (t.pos - ship.pos).length())))
        .min_by(|a, b| a.2.total_cmp(&b.2));
    let Some((target, track, range)) = nearest else {
        let c = ships[0];
        return advice(format!("{} is a bearing only. Close in, let an ally cross-fix it, or PING for range.", contact_label(c)), false);
    };
    let name = contact_label(target);
    if range < 3.0 * LIGHT_SECOND && ship.armed {
        return advice(format!("{name} inside beam range at {}. Aim DRIVE to slow it, SENSORS to blind it, HEAT to cook it.", fmt_distance(range)), true);
    }
    if !glare.is_empty() {
        return advice(format!("The sun is at our back: {} cannot see us passively. Keep it there while we close.", glare.join(", ")), false);
    }
    if !contact_has_course(target) {
        return advice(format!("{name} has a position but no course yet. Another look, or a PING, firms it up for a shot."), false);
    }
    let closing = -(track.vel - ship.vel).dot((track.pos - ship.pos).normalized());
    if ship.armed && range < Payload::Kinetic.engagement_range() && ship.magazine[Payload::Kinetic.index()] > 0 {
        return advice(format!("{name} at {} is inside SRM reach. A volley there gives little warning.", fmt_distance(range)), true);
    }
    if ship.armed && range < Payload::Nuclear.engagement_range() && ship.magazine[Payload::Nuclear.index()] > 0 {
        let ping = if target.active_fire_control > 0.0 { "" } else { " A fresh PING return sharpens the shot." };
        return advice(format!("{name} at {} is inside LRM reach. A long shot works best on a good fix or a damaged target.{ping}", fmt_distance(range)), false);
    }
    let time = if closing > 1.0 { format!(", closing at {} (about {} to LRM reach)", fmt_speed(closing), fmt_age(((range - Payload::Nuclear.engagement_range()) / closing).max(0.0))) } else { String::new() };
    advice(format!("{name} at {}{time}.", fmt_distance(range)), false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advice_reads_the_side_view_and_names_what_matters() {
        let app = LuminalApp::new_with_theme(ShipClass::Frigate, theme::Theme::Luminal);
        let mut view = app.session.view(app.role);
        let ship = view.bodies.iter().find(|b| b.controllable).unwrap().clone();
        view.contacts.clear();
        let quiet = pwo_advice(&view, &ship).unwrap();
        assert!(quiet.text.starts_with("Nothing on the plot"), "{}", quiet.text);
        let mut hot = ship.clone();
        hot.thermal = luminal_core::thermal::Thermal::at_fraction(0.9, hot.thermal.capacity_scale);
        let a = pwo_advice(&view, &hot).unwrap();
        assert!(a.urgent && a.text.starts_with("Heat at 90%"), "{}", a.text);
        view.outcome = Some(luminal_core::world::Outcome { winner: FactionId(0), t: 0.0, reason: String::new() });
        assert!(pwo_advice(&view, &ship).is_none(), "nothing to advise once it is over");
    }
}
