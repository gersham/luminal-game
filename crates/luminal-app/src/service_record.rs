//! The player's service record: each finished game, its result and the commendations it
//! earned, kept between runs in a small tab-separated file and shown on the scenario cards.
use super::*;
use luminal_core::world::debrief::Debrief;
use std::io::Write as _;

/// One commendation: a name, and what it was given for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Honour { FirstSight, Ghost, Untouched, CleanSweep, Marksman, Wall }

impl Honour {
    pub(crate) const ALL: [Honour; 6] = [Honour::FirstSight, Honour::Ghost, Honour::Untouched, Honour::CleanSweep, Honour::Marksman, Honour::Wall];
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::FirstSight => "First Sight",
            Self::Ghost => "Ghost",
            Self::Untouched => "Untouched",
            Self::CleanSweep => "Clean Sweep",
            Self::Marksman => "Marksman",
            Self::Wall => "The Wall",
        }
    }
    pub(crate) fn citation(self) -> &'static str {
        match self {
            Self::FirstSight => "Had word of the enemy before they had word of you",
            Self::Ghost => "Won without the enemy ever holding a firing fix on your side",
            Self::Untouched => "Won without losing a ship",
            Self::CleanSweep => "Destroyed every enemy ship",
            Self::Marksman => "Half or more of at least six missiles found their mark",
            Self::Wall => "Stopped a dozen or more incoming rounds",
        }
    }
    fn token(self) -> &'static str {
        match self {
            Self::FirstSight => "first-sight", Self::Ghost => "ghost", Self::Untouched => "untouched",
            Self::CleanSweep => "clean-sweep", Self::Marksman => "marksman", Self::Wall => "wall",
        }
    }
    fn parse(token: &str) -> Option<Self> { Self::ALL.into_iter().find(|h| h.token() == token) }
}

/// What a finished game earned, from the true record. Victory is required for the honours
/// that are about winning well; the others are earned in defeat too.
pub(crate) fn honours(debrief: &Debrief, own: FactionId, won: bool) -> Vec<Honour> {
    let Some(us) = debrief.sides.iter().find(|s| s.faction == own) else { return vec![] };
    let them: Vec<_> = debrief.sides.iter().filter(|s| s.faction != own).collect();
    let mut out = vec![];
    let their_first = them.iter().filter_map(|s| s.first_sighting).reduce(f64::min);
    if let Some(ours) = us.first_sighting && their_first.is_none_or(|t| ours + 60.0 < t) { out.push(Honour::FirstSight); }
    if won && them.iter().all(|s| s.first_fix.is_none()) { out.push(Honour::Ghost); }
    if won && us.ships_lost == 0 { out.push(Honour::Untouched); }
    let enemy_ships: u32 = them.iter().map(|s| s.ships).sum();
    let enemy_lost: u32 = them.iter().map(|s| s.ships_lost).sum();
    if enemy_ships > 0 && enemy_lost >= enemy_ships && us.kills > 0 { out.push(Honour::CleanSweep); }
    if us.missiles_launched >= 6 && 2 * us.missile_hits >= us.missiles_launched { out.push(Honour::Marksman); }
    if us.missiles_stopped >= 12 { out.push(Honour::Wall); }
    out
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Entry {
    pub scenario: String,
    pub won: bool,
    /// Game seconds to the outcome.
    pub t: f64,
    pub honours: Vec<Honour>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Summary {
    pub played: u32,
    pub won: u32,
    pub fastest_win: Option<f64>,
    pub honours: BTreeSet<Honour>,
}

#[derive(Default)]
pub(crate) struct ServiceRecord {
    entries: Vec<Entry>,
    path: Option<std::path::PathBuf>,
}

impl ServiceRecord {
    /// The player's record, or an empty one that is never written in tests and screenshots.
    pub(crate) fn load() -> Self {
        if cfg!(test) || std::env::var_os("LUMINAL_SCREENSHOT").is_some() { return Self::default(); }
        let Some(path) = record_path() else { return Self::default() };
        let entries = std::fs::read_to_string(&path).map(|text| parse(&text)).unwrap_or_default();
        Self { entries, path: Some(path) }
    }

    pub(crate) fn add(&mut self, entry: Entry) {
        if let Some(path) = &self.path {
            let written = path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| {
                let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
                file.write_all(format_line(&entry).as_bytes())
            });
            if let Err(e) = written { eprintln!("service record not saved: {e}"); }
        }
        self.entries.push(entry);
    }

    pub(crate) fn summary(&self, scenario: &str) -> Option<Summary> {
        let mut s = Summary::default();
        for e in self.entries.iter().filter(|e| e.scenario == scenario) {
            s.played += 1;
            if e.won {
                s.won += 1;
                s.fastest_win = Some(s.fastest_win.map_or(e.t, |f: f64| f.min(e.t)));
            }
            s.honours.extend(e.honours.iter().copied());
        }
        (s.played > 0).then_some(s)
    }
}

fn record_path() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME").map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| std::path::Path::new(&home).join(".local/share")))?;
    Some(base.join("luminal").join("service-record.tsv"))
}

fn format_line(e: &Entry) -> String {
    let honours: Vec<&str> = e.honours.iter().map(|h| h.token()).collect();
    format!("{}\t{}\t{:.0}\t{}\n", e.scenario, if e.won { "won" } else { "lost" }, e.t, honours.join(","))
}

fn parse(text: &str) -> Vec<Entry> {
    text.lines().filter_map(|line| {
        let mut parts = line.split('\t');
        let scenario = parts.next()?.to_string();
        let won = parts.next()? == "won";
        let t = parts.next()?.parse().ok()?;
        let honours = parts.next().unwrap_or("").split(',').filter_map(Honour::parse).collect();
        (!scenario.is_empty()).then_some(Entry { scenario, won, t, honours })
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_round_trips_and_sums_by_scenario() {
        let entries = vec![
            Entry { scenario: "escort".into(), won: true, t: 3600.0 * 30.0, honours: vec![Honour::FirstSight, Honour::Untouched] },
            Entry { scenario: "escort".into(), won: false, t: 3600.0 * 12.0, honours: vec![Honour::Wall] },
            Entry { scenario: "escort".into(), won: true, t: 3600.0 * 20.0, honours: vec![] },
            Entry { scenario: "raid".into(), won: false, t: 100.0, honours: vec![] },
        ];
        let text: String = entries.iter().map(format_line).collect();
        assert_eq!(parse(&text), entries);
        assert_eq!(parse("garbage\n\nescort\twon\tnot-a-number\t\n"), vec![], "bad lines are skipped, not fatal");
        let record = ServiceRecord { entries, path: None };
        let s = record.summary("escort").unwrap();
        assert_eq!((s.played, s.won, s.fastest_win), (3, 2, Some(3600.0 * 20.0)));
        assert_eq!(s.honours, [Honour::FirstSight, Honour::Untouched, Honour::Wall].into_iter().collect());
        assert!(record.summary("convoy").is_none());
    }

    #[test]
    fn honours_follow_the_true_record() {
        use luminal_core::world::debrief::SideStats;
        let side = |f: u8, sighting: Option<f64>, fix: Option<f64>| SideStats { faction: FactionId(f), ships: 1, ships_lost: 0, missiles_launched: 0,
            missile_hits: 0, interceptors_launched: 0, missiles_stopped: 0, beam_pulses: 0, decoys_released: 0, kills: 0,
            first_sighting: sighting, first_fix: fix };
        let mut us = side(0, Some(100.0), Some(200.0));
        us.missiles_launched = 8; us.missile_hits = 4; us.kills = 1; us.missiles_stopped = 12;
        let mut them = side(1, Some(5000.0), None);
        them.ships_lost = 1;
        let debrief = Debrief { end: 9000.0, paths: vec![], sides: vec![us, them], moments: vec![], identities: vec![] };
        let won = honours(&debrief, FactionId(0), true);
        assert_eq!(won, vec![Honour::FirstSight, Honour::Ghost, Honour::Untouched, Honour::CleanSweep, Honour::Marksman, Honour::Wall]);
        let lost = honours(&debrief, FactionId(0), false);
        assert!(!lost.contains(&Honour::Ghost) && !lost.contains(&Honour::Untouched), "winning-well honours need the win");
        assert!(lost.contains(&Honour::FirstSight));
    }
}
