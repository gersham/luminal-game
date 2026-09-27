//! Signature and measurement physics. Pure functions of physical quantities; the world
//! decides which light reaches which sensor, this module decides what it measures.
//!
//! Every constant used here comes from `params` and is currently a placeholder.

use crate::kinematics::Vec2;
use crate::params::*;
use crate::rng::Rng;
use crate::units::G0;
use std::f64::consts::PI;

/// Isotropic emission, W, of a ship accelerating at `accel` (km/s²).
pub fn ship_emission_w(accel: Vec2) -> f64 {
    SIGNATURE_COLD_W.value + SIGNATURE_DRIVE_W_PER_G.value * accel.length() / G0
}

/// Intensity, W/m², at `range_km` from an isotropic source of `power_w`.
pub fn intensity(power_w: f64, range_km: f64) -> f64 {
    let r_m = (range_km * 1e3).max(1.0);
    power_w / (4.0 * PI * r_m * r_m)
}

/// Echo intensity back at the pinger: out to the target, reflected, and back.
pub fn echo_intensity(ping_w: f64, out_km: f64, back_km: f64) -> f64 {
    let at_target = intensity(ping_w, out_km);
    intensity(at_target * ACTIVE_CROSS_SECTION_M2.value, back_km)
}

/// Direction of `v` in radians, counter-clockwise from +x.
pub fn bearing_of(v: Vec2) -> f64 {
    v.y.atan2(v.x)
}

pub fn wrap_angle(a: f64) -> f64 {
    (a + PI).rem_euclid(2.0 * PI) - PI
}

/// A passive measurement: bearing with noise, and its 1σ.
pub fn measure_bearing(true_bearing: f64, snr: f64, rng: &mut Rng) -> (f64, f64) {
    let sigma = (PASSIVE_BEARING_SIGMA.value / snr.sqrt()).min(0.5);
    (wrap_angle(true_bearing + sigma * rng.gaussian()), sigma)
}

/// An echo measurement: bearing and range with noise, and their 1σ.
pub struct EchoMeasurement {
    pub bearing: f64,
    pub sigma_bearing: f64,
    pub range: f64,
    pub sigma_range: f64,
}

pub fn measure_echo(true_bearing: f64, true_range: f64, snr: f64, rng: &mut Rng) -> EchoMeasurement {
    let (bearing, sigma_bearing) = measure_bearing(true_bearing, snr, rng);
    let sigma_range = ACTIVE_RANGE_SIGMA_KM.value / snr.sqrt();
    EchoMeasurement { bearing, sigma_bearing, range: true_range + sigma_range * rng.gaussian(), sigma_range }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::{AU, LIGHT_SECOND};

    fn passive_snr(accel_g: f64, range_km: f64) -> f64 {
        intensity(ship_emission_w(Vec2::new(accel_g * G0, 0.0)), range_km) / PASSIVE_NOISE_FLOOR.value
    }

    #[test]
    fn cold_ships_hide_and_burning_ships_shine() {
        // The placeholder calibration is meant to give these qualitative results.
        assert!(passive_snr(0.0, 200.0 * LIGHT_SECOND) < PASSIVE_DETECT_SNR.value);
        assert!(passive_snr(0.0, 5.0 * LIGHT_SECOND) > PASSIVE_DETECT_SNR.value);
        assert!(passive_snr(10.0, 1.0 * AU) > PASSIVE_DETECT_SNR.value);
    }

    #[test]
    fn echoes_reach_several_light_seconds() {
        let snr = |r: f64| echo_intensity(ACTIVE_PING_POWER_W.value, r, r) / ACTIVE_NOISE_FLOOR.value;
        assert!(snr(5.0 * LIGHT_SECOND) > 3.0);
        assert!(snr(30.0 * LIGHT_SECOND) < 1.0);
        // The ping itself is visible far beyond its echo range.
        let seen = intensity(ACTIVE_PING_POWER_W.value, 1.0 * AU) / PASSIVE_NOISE_FLOOR.value;
        assert!(seen > 3.0);
    }

    #[test]
    fn wrap_angle_stays_in_range() {
        assert!((wrap_angle(3.0 * PI) - PI).abs() < 1e-12 || (wrap_angle(3.0 * PI) + PI).abs() < 1e-12);
        assert!((wrap_angle(0.5) - 0.5).abs() < 1e-12);
    }
}
