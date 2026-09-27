//! Unit conventions: kilometres, seconds, kilograms, joules, watts, kelvin.

/// Speed of light, km/s.
pub const C: f64 = 299_792.458;

/// Standard gravity, km/s².
pub const G0: f64 = 9.806_65e-3;

/// Astronomical unit, km.
pub const AU: f64 = 149_597_870.7;

/// One light-second, km.
pub const LIGHT_SECOND: f64 = C;

/// Speed of light, m/s, for energy calculations in SI.
pub const C_SI: f64 = 299_792_458.0;

/// Stefan–Boltzmann constant, W m⁻² K⁻⁴.
pub const SIGMA: f64 = 5.670_374_419e-8;

/// Relativistic kinetic energy in joules for mass `kg` at speed `km_s` (km/s).
pub fn kinetic_energy(kg: f64, km_s: f64) -> f64 {
    let beta = km_s / C;
    let gamma = 1.0 / (1.0 - beta * beta).sqrt();
    (gamma - 1.0) * kg * C_SI * C_SI
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinetic_energy_matches_mechanics_doc_table() {
        // GAME_MECHANICS.md §6: 100 kg at ~0.1c ≈ 45 PJ.
        let e = kinetic_energy(100.0, 0.1 * C);
        assert!((e / 1e15 - 45.2).abs() < 0.3, "{e}");
    }
}
