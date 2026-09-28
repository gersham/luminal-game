//! Recognizable Sol, not an ephemeris: coplanar circular orbits frozen at setup.
use crate::celestial::{Celestial, CelestialKind, Orbit, System};
use crate::kinematics::Vec2;
use crate::rng::Rng;
use crate::units::AU;
use std::f64::consts::TAU;

/// One randomized epoch drives every orbital phase; simulation time never moves them.
/// Indices 0/1/2 remain Sun/Earth/Moon for scenario and navigation compatibility.
pub fn system(seed: u64) -> System {
    let days = Rng::stream(seed, 0x534f4c).uniform() * 365.25 * 200.0;
    let mut system = System { bodies: vec![Celestial {
        name: "Sun".into(), kind: CelestialKind::Star, gm: 1.3271244e11,
        radius: 696_000.0, orbit: Orbit::Fixed(Vec2::ZERO),
    }] };
    // name, parent, radius km, GM km³/s², orbital radius km, period days, phase degrees.
    // Rounded physical values; phases are illustrative, not a historical sky chart.
    let bodies = [
        ("Earth", 0, 6371.0, 398600.4, AU, 365.256, 100.5),
        ("Moon", 1, 1737.4, 4902.8, 384400.0, 27.322, 218.3),
        ("Mercury", 0, 2439.4, 22031.9, 0.3871*AU, 87.969, 252.3),
        ("Venus", 0, 6051.8, 324858.6, 0.7233*AU, 224.701, 182.0),
        ("Mars", 0, 3389.5, 42828.4, 1.5237*AU, 686.980, 355.4),
        ("Phobos", 5, 11.1, 0.000711, 9378.0, 0.31891, 30.0),
        ("Deimos", 5, 6.2, 0.0000985, 23459.0, 1.26244, 210.0),
        ("Jupiter", 0, 69911.0, 126686534.0, 5.2029*AU, 4332.589, 34.4),
        ("Io", 8, 1821.5, 5959.9, 421800.0, 1.76914, 20.0),
        ("Europa", 8, 1560.8, 3202.7, 671100.0, 3.55118, 140.0),
        ("Ganymede", 8, 2631.2, 9887.8, 1070400.0, 7.15455, 260.0),
        ("Callisto", 8, 2410.3, 7179.3, 1882700.0, 16.689, 310.0),
        ("Saturn", 0, 58232.0, 37931187.0, 9.5367*AU, 10759.22, 50.0),
        ("Titan", 13, 2574.7, 8978.1, 1221870.0, 15.945, 120.0),
        ("Rhea", 13, 763.8, 153.9, 527108.0, 4.518, 20.0),
        ("Iapetus", 13, 734.5, 120.5, 3560820.0, 79.3215, 240.0),
        ("Uranus", 0, 25362.0, 5793940.0, 19.1892*AU, 30685.4, 313.2),
        ("Titania", 17, 788.9, 228.2, 436300.0, 8.70587, 30.0),
        ("Oberon", 17, 761.4, 192.4, 583500.0, 13.4632, 210.0),
        ("Neptune", 0, 24622.0, 6835099.0, 30.0699*AU, 60189.0, 304.9),
        ("Triton", 20, 1353.4, 1427.6, 354760.0, -5.87685, 90.0),
    ];
    for (name, parent, radius, gm, distance, period, phase) in bodies {
        let angle = (phase / 360.0 + days / period).rem_euclid(1.0) * TAU;
        let (sin, cos) = angle.sin_cos();
        let pos = system.state(parent, 0.0).pos + Vec2::new(cos, sin) * distance;
        system.bodies.push(Celestial {
            name: name.into(), kind: if parent == 0 {CelestialKind::Planet} else {CelestialKind::Moon},
            gm, radius, orbit: Orbit::Frozen {parent, radius: distance, pos},
        });
    }
    system
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sol_is_complete_repeatable_and_frozen() {
        let a = system(42);
        let b = system(42);
        assert_eq!(a.bodies.len(), 22);
        assert_eq!(a.bodies.iter().filter(|b| b.kind == CelestialKind::Planet).count(), 8);
        assert_eq!(a.bodies.iter().filter(|b| b.kind == CelestialKind::Moon).count(), 13);
        for i in 0..a.bodies.len() {
            assert_eq!(a.state(i, 0.0), b.state(i, 1e12));
            assert_eq!(a.state(i, 0.0), a.state(i, -1e12));
            assert_eq!(a.state(i, 0.0).vel, Vec2::ZERO);
            assert_eq!(a.accel(i, 1e9), Vec2::ZERO);
            if i > 0 { assert!(a.soi_radius(i).is_finite()); }
        }
    }
    #[test]
    fn starting_epoch_changes_earth_mars_relationship() {
        let relative_angle = |seed| {
            let s = system(seed);
            let earth = s.state(1, 0.0).pos;
            let mars = s.state(5, 0.0).pos;
            mars.y.atan2(mars.x) - earth.y.atan2(earth.x)
        };
        assert!((relative_angle(42) - relative_angle(43)).abs() > 0.01);
    }
}
