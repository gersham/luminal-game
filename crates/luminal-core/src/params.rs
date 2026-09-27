//! Every tunable value, tagged with its commitment level (GAME_MECHANICS.md §1).
//!
//! `Established` values are explicit design choices. `Proposal` values are working
//! interpretations from the design discussion. `Placeholder` values were chosen to get
//! a prototype running and carry no design authority.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Commitment {
    Established,
    Proposal,
    Placeholder,
}

#[derive(Clone, Copy, Debug)]
pub struct Param {
    pub key: &'static str,
    pub value: f64,
    pub unit: &'static str,
    pub commitment: Commitment,
    pub note: &'static str,
}

use Commitment::*;

pub const SHIP_MAX_ACCEL_G: Param = Param {
    key: "ship.max_accel",
    value: 100.0,
    unit: "g",
    commitment: Established,
    note: "Ship maximum acceleration.",
};

pub const PROBE_MAX_ACCEL_G: Param = Param {
    key: "probe.max_accel",
    value: 500.0,
    unit: "g",
    commitment: Established,
    note: "Probe maximum acceleration.",
};

pub const PROBE_MASS_T: Param = Param {
    key: "probe.mass",
    value: 10.0,
    unit: "t",
    commitment: Established,
    note: "Probe class mass.",
};

pub const MISSILE_MAX_ACCEL_G: Param = Param {
    key: "missile.max_accel",
    value: 1000.0,
    unit: "g",
    commitment: Established,
    note: "Missile maximum acceleration.",
};

pub const MISSILE_MASS_KG: Param = Param {
    key: "missile.mass",
    value: 100.0,
    unit: "kg",
    commitment: Established,
    note: "Missile class mass. Launch vs impact mass not yet distinguished.",
};

pub const SCREEN_BUILD_TIME_S: Param = Param {
    key: "screen.build_time",
    value: 60.0,
    unit: "s",
    commitment: Placeholder,
    note: "Illustrative time to establish a screen (§10).",
};

pub const SCREEN_COOL_SHUTDOWN_S: Param = Param {
    key: "screen.cool_shutdown_time",
    value: 120.0,
    unit: "s",
    commitment: Placeholder,
    note: "Illustrative time to lower a cool screen; hot screens take longer (§10).",
};

pub const SENSOR_FRAME_S: Param = Param {
    key: "sensor.frame_period",
    value: 10.0,
    unit: "s",
    commitment: Placeholder,
    note: "Sensor integration frame. Observations are produced and delivered on frame boundaries.",
};

pub const SIGNATURE_COLD_W: Param = Param {
    key: "signature.cold",
    value: 1.0e8,
    unit: "W",
    commitment: Placeholder,
    note: "Isotropic emission of a ship with drive and screen off (waste heat).",
};

pub const SIGNATURE_DRIVE_W_PER_G: Param = Param {
    key: "signature.drive_per_g",
    value: 1.0e11,
    unit: "W/g",
    commitment: Placeholder,
    note: "Additional isotropic emission per g of thrust. Drive physics not yet modelled.",
};

pub const PASSIVE_NOISE_FLOOR: Param = Param {
    key: "passive.noise_floor",
    value: 1.0e-13,
    unit: "W/m²",
    commitment: Placeholder,
    note: "Received intensity giving SNR 1 for a warship's passive array. No own-screen glare yet.",
};

pub const PASSIVE_DETECT_SNR: Param = Param {
    key: "passive.detect_snr",
    value: 3.0,
    unit: "",
    commitment: Placeholder,
    note: "Minimum SNR for a passive detection in one frame.",
};

pub const PASSIVE_BEARING_SIGMA: Param = Param {
    key: "passive.bearing_sigma",
    value: 1.0e-4,
    unit: "rad",
    commitment: Placeholder,
    note: "Bearing error at SNR 1; scales as 1/sqrt(SNR). Passive sensing gives no range (proposal).",
};

pub const ACTIVE_PING_POWER_W: Param = Param {
    key: "active.ping_power",
    value: 1.0e12,
    unit: "W",
    commitment: Placeholder,
    note: "Effective isotropic power of an active ping, one per sensor frame.",
};

pub const ACTIVE_CROSS_SECTION_M2: Param = Param {
    key: "active.cross_section",
    value: 1.0e4,
    unit: "m²",
    commitment: Placeholder,
    note: "Radar-style cross-section of a ship for echo strength.",
};

pub const ACTIVE_NOISE_FLOOR: Param = Param {
    key: "active.noise_floor",
    value: 1.0e-24,
    unit: "W/m²",
    commitment: Placeholder,
    note: "Echo intensity giving SNR 1 after coherent processing. Sets echo range near 10 ls.",
};

pub const ACTIVE_RANGE_SIGMA_KM: Param = Param {
    key: "active.range_sigma",
    value: 1.0,
    unit: "km",
    commitment: Placeholder,
    note: "Echo range error at SNR 1; scales as 1/sqrt(SNR).",
};

pub const TRACK_MANEUVER_G: Param = Param {
    key: "track.maneuver_accel",
    value: 1.0,
    unit: "g",
    commitment: Placeholder,
    note: "Unmodelled target acceleration assumed by trackers (process noise).",
};

pub const ALL: &[Param] = &[
    SHIP_MAX_ACCEL_G,
    PROBE_MAX_ACCEL_G,
    PROBE_MASS_T,
    MISSILE_MAX_ACCEL_G,
    MISSILE_MASS_KG,
    SCREEN_BUILD_TIME_S,
    SCREEN_COOL_SHUTDOWN_S,
    SENSOR_FRAME_S,
    SIGNATURE_COLD_W,
    SIGNATURE_DRIVE_W_PER_G,
    PASSIVE_NOISE_FLOOR,
    PASSIVE_DETECT_SNR,
    PASSIVE_BEARING_SIGMA,
    ACTIVE_PING_POWER_W,
    ACTIVE_CROSS_SECTION_M2,
    ACTIVE_NOISE_FLOOR,
    ACTIVE_RANGE_SIGMA_KM,
    TRACK_MANEUVER_G,
];

#[cfg(test)]
mod tests {
    #[test]
    fn keys_are_unique() {
        let mut keys: Vec<_> = super::ALL.iter().map(|p| p.key).collect();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), super::ALL.len());
    }
}
