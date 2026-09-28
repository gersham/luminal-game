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

/// Temporarily disabled for gameplay tuning; retain probe implementation/tests.
pub const PROBES_ENABLED: bool = false;
pub const MISSILE_ACTIVE_RANGE_LS: f64 = 5.0;
pub const MISSILE_ACTIVE_LEAD_S: f64 = 60.0;
pub const MISSILE_SEARCH_HALF_ANGLE: f64 = 0.17453292519943295; // ten degrees
pub const MISSILE_BLIND_SEARCH_S: f64 = 30.0;

pub const INTERCEPTOR_RANGE_LS:Param=Param {key:"interceptor.range",value:0.27*crate::units::AU/crate::units::LIGHT_SECOND,unit:"ls",commitment:Established,note:"0.27 AU outer engagement gate, further constrained by intercept geometry."};
pub const INTERCEPTOR_BURN_S:Param=Param {key:"interceptor.burn",value:180.0,unit:"s",commitment:Proposal,note:"Three-minute boost retained; added coast endurance extends reach without forcing excessive encounter speed."};
pub const INTERCEPTOR_ACCEL_G:Param=Param {key:"interceptor.acceleration",value:3000.0,unit:"g",commitment:Proposal,note:"Lightweight defensive round has twice offensive-missile acceleration for terminal correction."};
pub const INTERCEPTOR_LIFETIME_S:Param=Param {key:"interceptor.endurance",value:0.27*crate::units::AU/(INTERCEPTOR_ACCEL_G.value*crate::units::G0*180.0)+90.0,unit:"s",commitment:Proposal,note:"Boost-and-coast endurance sized for a 0.27 AU outer engagement envelope."};
pub const INTERCEPTOR_LAUNCH_INTERVAL_S:Param=Param {key:"interceptor.launch_interval",value:1.0,unit:"s",commitment:Proposal,note:"Dedicated automatic defensive launcher cadence."};
pub const INTERCEPTOR_GUIDE_S:Param=Param {key:"interceptor.guidance",value:0.1,unit:"s",commitment:Placeholder,note:"Local seeker and lateral guidance cadence."};
pub const INTERCEPTOR_KILL_RADIUS_KM:Param=Param {key:"interceptor.kill_radius",value:100.0,unit:"km",commitment:Placeholder,note:"Abstract short-range interceptor fragmentation envelope, then velocity-dependent hit roll."};
pub const INTERCEPTOR_HALF_SPEED_C:Param=Param {key:"interceptor.half_speed",value:0.25,unit:"c",commitment:Proposal,note:"Half the base 60% kill probability at this encounter speed; tuned for long-range missile defence."};
pub const INTERCEPTOR_MAX_SPEED_C:Param=Param {key:"interceptor.max_speed",value:0.5,unit:"c",commitment:Proposal,note:"No engagement or kill probability at or above this encounter speed."};

pub const PD_RATE_HZ: Param=Param {key:"point_defence.rate",value:1.0,unit:"shots/s",commitment:Established,note:"Default firing rate per fitted emplacement."};
pub const PD_HALF_RANGE_LS: Param=Param {key:"point_defence.half_range",value:0.012,unit:"ls",commitment:Proposal,note:"Last-ditch laser defence: 50% per shot at 0.012 ls; hard maximum is 2 ls."};
pub const PD_LASER_MAX_RANGE_LS: Param=Param {key:"point_defence.laser_last_ditch_range",value:2.0,unit:"ls",commitment:Proposal,note:"Lasers fire within 2 ls; interceptors are the primary outer defence. Hit probability still falls with range."};
pub const PD_FALLOFF_POWER: Param=Param {key:"point_defence.falloff",value:6.0,unit:"exponent",commitment:Proposal,note:"Hit chance 1/(1+(range/half_range)^6), rapid falloff beyond."};
pub const PD_MAX_RANGE_LS: Param=Param {key:"point_defence.max_range",value:INTERCEPTOR_RANGE_LS.value,unit:"ls",commitment:Proposal,note:"Interceptor fire-control acquisition reaches 0.27 AU; laser firing retains its separate close-in cutoff."};
pub const PD_SENSOR_NOISE_FLOOR: Param=Param {key:"point_defence.sensor_noise_floor",value:1e-22,unit:"W/m²",commitment:Proposal,note:"Dedicated missile fire control supports the extended envelope; local passive channel and light-time still required."};
pub const PD_FLASH_W: Param=Param {key:"point_defence.flash",value:1e9,unit:"W",commitment:Placeholder,note:"Detectable signature of a point-defence discharge; dedicated emplacement power is not yet in the offensive beam capacitor model."};

pub const TRANSPORT_EMISSION_FACTOR: Param=Param {key:"signature.transport_baseline",value:1.0,unit:"x",commitment:Established,note:"Transport fixed emission baseline; drive and screen radiation added separately."};
pub const FRIGATE_EMISSION_FACTOR: Param=Param {key:"signature.frigate_baseline",value:0.5,unit:"x",commitment:Established,note:"Stealth frigate fixed emission baseline."};
pub const STATION_EMISSION_FACTOR: Param=Param {key:"signature.station_baseline",value:2.0,unit:"x",commitment:Established,note:"Station fixed emission baseline."};

pub const MISSILE_ACTIVE_INTERVAL_S: Param = Param {
    key: "missile.active_interval", value: 1.0, unit: "s", commitment: Proposal,
    note: "Terminal seeker ping interval; no active emission before terminal flight.",
};

pub const STATION_PING_INTERVAL_S: Param = Param {
    key: "station.ping_interval", value: 60.0, unit: "s", commitment: Established,
    note: "Autonomous lunar sensor station ping cadence; pulses hidden from player display.",
};

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
    value: 1500.0,
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
    value: 3000.0,
    unit: "s",
    commitment: Placeholder,
    note: "Fifty minutes from empty: 2 percentage points of field charge per minute.",
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
    value: 5.0e3,
    unit: "W",
    commitment: Placeholder,
    note: "Isotropic emission of a ship with drive and screen off (waste heat).",
};

pub const SIGNATURE_DRIVE_W_PER_G: Param = Param {
    key: "signature.drive_per_g",
    value: 5.0e8,
    unit: "W/g",
    commitment: Placeholder,
    note: "Additional isotropic emission per g of thrust. Drive physics not yet modelled.",
};

pub const PASSIVE_NOISE_FLOOR: Param = Param {
    key: "passive.noise_floor",
    value: (SIGNATURE_COLD_W.value + 10.0 * SIGNATURE_DRIVE_W_PER_G.value)
        / (4.0 * std::f64::consts::PI * crate::units::AU * crate::units::AU * 1e4 * 9.0),
    unit: "W/m²",
    commitment: Placeholder,
    note: "Passive localization: 75% per frame at 0.1 AU for a 10g reference ship. No own-screen glare yet.",
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
    note: "Passive localization bearing error at SNR 1; scales as 1/sqrt(SNR).",
};

pub const ACTIVE_PING_POWER_W: Param = Param {
    key: "active.ping_power",
    value: 1.0e12,
    unit: "W",
    commitment: Placeholder,
    note: "Effective isotropic power of a single player-commanded active ping.",
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
    value: ACTIVE_PING_POWER_W.value * ACTIVE_CROSS_SECTION_M2.value
        / (16.0 * std::f64::consts::PI * std::f64::consts::PI
            * crate::units::AU * crate::units::AU * crate::units::AU * crate::units::AU * 1e12 * 9.0),
    unit: "W/m²",
    commitment: Placeholder,
    note: "75% active ranging chance per return at 1 AU for the reference cross-section.",
};

pub const DIRECTION_RANGE_RATIO: Param = Param {
    key: "direction.range_ratio", value: 100.0, unit: "", commitment: Established,
    note: "Direction finding reaches 10 AU when passive localization reaches 0.1 AU.",
};
pub const ACTIVE_EXPOSURE_RANGE: Param = Param {
    key: "active.exposure_range_multiplier", value: 10.0, unit: "", commitment: Established,
    note: "A ping exposes the emitter at ten times its passive and direction-finding ranges; travels at c.",
};
pub const PASSIVE_RANGE_FRACTION: Param = Param {
    key: "passive.range_sigma_fraction", value: 0.001, unit: "", commitment: Proposal,
    note: "TL7 passive localization range error at SNR 1 as a fraction of range; a sensor abstraction, not brightness-derived exact range.",
};
pub const DIRECTION_BEARING_SIGMA: Param = Param {
    key: "direction.bearing_sigma", value: 0.03, unit: "rad", commitment: Placeholder,
    note: "Direction-only bearing error at SNR 1.",
};
pub const CONTACT_AGE_LIMIT_S: Param = Param {
    key: "display.contact_age_limit", value: 3600.0, unit: "s", commitment: Placeholder,
    note: "Contact age ring reaches its maximum size after one hour.",
};
pub const CONTACT_RING_MAX_PX: Param = Param {
    key: "display.contact_ring_max", value: 40.0, unit: "px", commitment: Placeholder,
    note: "Maximum on-map age ring radius; does not cap the track's actual uncertainty.",
};
pub const CONTACT_RING_MIN_RANGE_LS: Param = Param {
    key: "display.contact_ring_min_range", value: 60.0, unit: "ls", commitment: Established,
    note: "Only show contact uncertainty rings at least this far from the nearest friendly ship.",
};
pub const FLYBY_HORIZON_S: Param = Param {
    key: "autopilot.flyby_horizon", value: 30.0 * 86400.0, unit: "s", commitment: Placeholder,
    note: "Maximum lookahead for the full-thrust flyby encounter solver.",
};

pub const ACTIVE_BEARING_SIGMA: Param = Param {
    key: "active.bearing_sigma",
    value: 1.0e-4,
    unit: "rad",
    commitment: Placeholder,
    note: "Echo bearing error at SNR 1; scales as 1/sqrt(SNR). Far finer than passive: the ping is a known, coherent pulse.",
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
    value: 10.0,
    unit: "g",
    commitment: Placeholder,
    note: "Unmodelled target acceleration assumed by trackers (process noise).",
};

pub const MISSILE_DELTA_V_KMS: Param = Param {
    key: "missile.delta_v",
    value: 29_979.0,
    unit: "km/s",
    commitment: Placeholder,
    note: "Total propulsion budget, about 0.1c: about 34 minutes at full 1,500 g; initial burn uses 60% (about 0.06c). Ships are reactionless and unlimited.",
};

pub const MISSILE_BURN_FRACTION: Param = Param {
    key: "missile.burn_fraction",
    value: 0.6,
    unit: "",
    commitment: Placeholder,
    note: "Share of delta-v spent in the initial burn.",
};

pub const MISSILE_RESERVE_FRACTION: Param = Param {
    key: "missile.terminal_reserve",
    value: 0.3,
    unit: "",
    commitment: Placeholder,
    note: "Share of delta-v cruise corrections may not touch; kept for terminal.",
};

pub const MISSILE_TERMINAL_S: Param = Param {
    key: "missile.terminal_time",
    value: 60.0,
    unit: "s",
    commitment: Placeholder,
    note: "Time to go at which the missile switches to its own seeker.",
};

pub const MISSILE_SEEKER_NOISE_FLOOR: Param = Param {
    key: "missile.seeker_noise_floor",
    value: 7.4e-15,
    unit: "W/m²",
    commitment: Placeholder,
    note: "Terminal bearing seeker: a 25 kW cold target reaches SNR 3 near 1 light-second. Prototype calibration, not a full warship ranging array.",
};

pub const MISSILE_SIGNATURE_COLD_W: Param = Param {
    key: "missile.signature_cold",
    value: 5.0e2,
    unit: "W",
    commitment: Placeholder,
    note: "Missile emission when coasting.",
};

pub const MISSILE_DRIVE_W_PER_G: Param = Param {
    key: "missile.drive_per_g",
    value: 5.0e7,
    unit: "W/g",
    commitment: Placeholder,
    note: "Missile drive emission per g: a full 1,000 g burn shines like a ship at 100 g. Missiles are bright.",
};

pub const MAGAZINE_CRUISER: Param = Param {
    key: "magazine.cruiser",
    value: 20.0,
    unit: "missiles/type",
    commitment: Established,
    note: "Default scenario raider starts with twenty SRMs and ten LRMs; class presets override both.",
};

pub const MAGAZINE_FRIGATE: Param = Param {
    key: "magazine.frigate",
    value: 20.0,
    unit: "missiles/type",
    commitment: Established,
    note: "Frigate carries twenty SRMs and ten LRMs. Transport carries none.",
};

pub const MISSILE_LAUNCH_INTERVAL_S: Param = Param {
    key: "missile.launch_interval", value: 10.0, unit: "s", commitment: Established,
    note: "LRM launcher fires every ten seconds: 1 PJ per round matches the SRM nominal damage rate.",
};
pub const SRM_LAUNCH_INTERVAL_S:Param=Param {key:"missile.srm_launch_interval",value:5.0,unit:"s",commitment:Established,note:"Independent SRM launcher fires once per five simulation seconds."};

pub const SHIP_RADIUS_KM: Param = Param {
    key: "ship.radius",
    value: 0.1,
    unit: "km",
    commitment: Placeholder,
    note: "Physical size for kinetic and beam hits.",
};

pub const SCREEN_CAPACITY_J: Param = Param {
    key: "screen.capacity",
    value: 4.5e14,
    unit: "J",
    commitment: Placeholder,
    note: "Rated established-field energy capacity; partial fields have proportionally less headroom, and stored energy radiates thermally.",
};

pub const HULL_INTEGRITY_J: Param = Param {
    key: "hull.integrity",
    value: 2.0e14,
    unit: "J",
    commitment: Placeholder,
    note: "Full hull energy budget; ablative armour adds an equal initial budget.",
};

pub const KINETIC_SHOT_MASS_KG: Param = Param {
    key: "payload.kinetic_shot_mass",
    value: 1.0,
    unit: "kg",
    commitment: Placeholder,
    note: "Mass of shot that actually strikes: 4.5 TJ at 0.01c, 0.45 PJ at 0.1c.",
};

pub const KINETIC_PATTERN_KM: Param = Param {
    key: "payload.kinetic_pattern",
    value: 5000.0,
    unit: "km",
    commitment: Placeholder,
    note: "SRM shotgun proximity-burst envelope; local resolution begins at twice this distance, followed by the terminal accuracy roll.",
};

pub const MISSILE_NAV_DRIFT: Param = Param {
    key: "missile.nav_drift",
    value: 1.5e-4,
    unit: "",
    commitment: Placeholder,
    note: "Aim error per km flown (1σ per axis), reduced after seeded delivery trials. Long-range success depends on fresh tracking and terminal acquisition, not a universal hit percentage.",
};

pub const NUCLEAR_AOE_KM: Param = Param {
    key: "payload.nuclear_aoe",
    value: 29_979.0,
    unit: "km",
    commitment: Placeholder,
    note: "0.1 ls. The burst hits every ship within this distance of the detonation.",
};

pub const NUCLEAR_ENERGY_J: Param = Param {
    key: "payload.nuclear_energy",
    value: 1.0e15,
    unit: "J",
    commitment: Placeholder,
    note: "1 PJ from an LRM nuclear-pumped laser strike, before screen absorption.",
};


pub const LASER_POINTING_RAD: Param = Param {
    key: "payload.laser_pointing",
    value: 1.0e-7,
    unit: "rad",
    commitment: Placeholder,
    note: "Beam pointing jitter (1σ) through the laser's own optics: 30 m at 1 ls, growing with range.",
};


pub const MISSILE_SEEKER_FRAME_S: Param = Param {
    key: "missile.seeker_frame",
    value: 0.02,
    unit: "s",
    commitment: Placeholder,
    note: "Fastest seeker update. At high closing speed each look comes from farther out.",
};

pub const MISSILE_RESPONSE_S: Param = Param {
    key: "missile.response_time",
    value: 0.05,
    unit: "s",
    commitment: Placeholder,
    note: "Delay from seeker measurement to changed thrust.",
};

pub const MISSILE_SEEKER_BEARING_SIGMA: Param = Param {
    key: "missile.seeker_bearing_sigma",
    value: 1.0e-4,
    unit: "rad",
    commitment: Placeholder,
    note: "Seeker bearing error at SNR 1; scales as 1/sqrt(SNR). Far finer than a ship's wide-field passive array.",
};

pub const MISSILE_SEEKER_ANGLE_FLOOR: Param = Param {
    key: "missile.seeker_angle_floor",
    value: 3.0e-5,
    unit: "rad",
    commitment: Placeholder,
    note: "Pointing jitter: seeker bearings are never better than this. Proportional navigation multiplies the resulting rate noise by closing speed.",
};


pub const ALL: &[Param] = &[
    PROBE_INVENTORY, PROBE_BURN_S, PROBE_SENSOR_FACTOR, PROBE_PING_INTERVAL_S,
    PASSIVE_SYSTEMATIC_FRACTION, ACTIVE_SYSTEMATIC_FRACTION, DIRECTION_SYSTEMATIC_RAD,
    SEEKER_RESOLVE_LS, SEEKER_FIX_INTERVAL_S, SEEKER_RANGE_SIGMA_KM,
    HOSTILE_PING_LIFETIME_S,
    PD_RATE_HZ, PD_HALF_RANGE_LS, PD_LASER_MAX_RANGE_LS, PD_FALLOFF_POWER, PD_MAX_RANGE_LS, PD_SENSOR_NOISE_FLOOR, PD_FLASH_W,
    INTERCEPTOR_RANGE_LS, INTERCEPTOR_BURN_S, INTERCEPTOR_LIFETIME_S,
    INTERCEPTOR_LAUNCH_INTERVAL_S, INTERCEPTOR_GUIDE_S, INTERCEPTOR_KILL_RADIUS_KM, INTERCEPTOR_ACCEL_G,
    INTERCEPTOR_HALF_SPEED_C, INTERCEPTOR_MAX_SPEED_C,
    TRANSPORT_EMISSION_FACTOR, FRIGATE_EMISSION_FACTOR, STATION_EMISSION_FACTOR,
    TRACK_STALE_S, TRACK_LOST_S, TRACK_VELOCITY_SIGMA, BEAM_CAPACITOR_J, REACTOR_W,
    BEAM_EFFICIENCY, BEAM_HEAT_LIMIT_J, HULL_COOLING_S,
      SCREEN_GLARE_W, TACTICAL_FRAME_S, BOT_PING_S, BOT_SALVO_S,
    SHIP_BEAM_ENERGY_J,
    SHIP_BEAM_AUTO_RANGE_LS,
    SHIP_BEAM_MIN_EXPECTED_J,
    SHIP_BEAM_RECHARGE_S,
    SHIP_BEAM_DIVERGENCE,
    SHIP_BEAM_POINTING_RAD,
    SHIP_MAX_ACCEL_G,
    PROBE_MAX_ACCEL_G,
    PROBE_MASS_T,
    MISSILE_MAX_ACCEL_G,
    MISSILE_MASS_KG,
    SCREEN_BUILD_TIME_S,

    SENSOR_FRAME_S,
    SIGNATURE_COLD_W,
    SIGNATURE_DRIVE_W_PER_G,
    PASSIVE_NOISE_FLOOR,
    DIRECTION_RANGE_RATIO,
    ACTIVE_EXPOSURE_RANGE,
    PASSIVE_RANGE_FRACTION,
    DIRECTION_BEARING_SIGMA,
    CONTACT_AGE_LIMIT_S,
    CONTACT_RING_MAX_PX,
    CONTACT_RING_MIN_RANGE_LS,
    FLYBY_HORIZON_S,
    PASSIVE_DETECT_SNR,
    PASSIVE_BEARING_SIGMA,
    ACTIVE_PING_POWER_W,
    ACTIVE_CROSS_SECTION_M2,
    ACTIVE_NOISE_FLOOR,
    ACTIVE_BEARING_SIGMA,
    ACTIVE_RANGE_SIGMA_KM,
    TRACK_MANEUVER_G,
    MISSILE_DELTA_V_KMS,
    MISSILE_BURN_FRACTION,
    MISSILE_RESERVE_FRACTION,
    MISSILE_TERMINAL_S,
    MISSILE_SEEKER_NOISE_FLOOR,
    MISSILE_SIGNATURE_COLD_W,
    MISSILE_DRIVE_W_PER_G,
    MAGAZINE_CRUISER,
    MAGAZINE_FRIGATE,
    MISSILE_LAUNCH_INTERVAL_S,
    SRM_LAUNCH_INTERVAL_S,
    SHIP_RADIUS_KM,
    SCREEN_CAPACITY_J,
    HULL_INTEGRITY_J,
    KINETIC_SHOT_MASS_KG,
    KINETIC_PATTERN_KM,
    MISSILE_NAV_DRIFT,
    NUCLEAR_AOE_KM,
    NUCLEAR_ENERGY_J,
    LASER_POINTING_RAD,
    MISSILE_SEEKER_FRAME_S,
    MISSILE_ACTIVE_INTERVAL_S,
    MISSILE_RESPONSE_S,
    MISSILE_SEEKER_BEARING_SIGMA,
    MISSILE_SEEKER_ANGLE_FLOOR,
];

pub const TRACK_STALE_S: Param = Param { key: "track.stale", value: 120.0, unit: "s since receipt", commitment: Placeholder, note: "Hold automatic fire when reports stop arriving." };
pub const HOSTILE_PING_LIFETIME_S: Param = Param { key: "sensor.hostile_ping_lifetime", value: 600.0, unit: "s since receipt", commitment: Established, note: "Orange enemy ping indication fades over ten minutes; manoeuvre envelope uses the 100g ship acceleration bound." };
pub const PASSIVE_SYSTEMATIC_FRACTION: Param = Param { key: "sensor.passive_systematic", value: 0.02, unit: "range fraction", commitment: Placeholder, note: "Persistent per-sensor contact range bias; repeated frames cannot average it away." };
pub const ACTIVE_SYSTEMATIC_FRACTION: Param = Param { key: "sensor.active_systematic", value: 0.002, unit: "range fraction", commitment: Placeholder, note: "Persistent active-range calibration floor, shrinking with distance and signal strength." };
pub const DIRECTION_SYSTEMATIC_RAD: Param = Param { key: "sensor.direction_systematic", value: 0.002, unit: "rad", commitment: Placeholder, note: "Persistent bearing calibration bias; triangulation retains an uncertainty floor." };
pub const SEEKER_RESOLVE_LS: Param = Param { key: "missile.resolve_range", value: 3.0, unit: "ls", commitment: Placeholder, note: "Within this envelope and sufficient SNR, the missile can form its own noisy local range/velocity solution." };
pub const SEEKER_FIX_INTERVAL_S: Param = Param { key: "missile.fix_interval", value: 0.1, unit: "s", commitment: Placeholder, note: "Independent local ranging integration, not every guidance iteration." };
pub const SEEKER_RANGE_SIGMA_KM: Param = Param { key: "missile.range_sigma", value: 0.1, unit: "km", commitment: Placeholder, note: "Local optical/TL7 ranging error at SNR 1; never an exact truth handoff." };
pub const PROBE_INVENTORY: Param = Param {key:"probe.inventory",value:3.0,unit:"per military ship",commitment:Established,note:"Three reconnaissance probes per military ship; transports, stations and other platforms carry none."};
pub const PROBE_BURN_S: Param = Param {key:"probe.burn",value:600.0,unit:"s",commitment:Placeholder,note:"Fixed-heading 500g burn, then ballistic coast. Not the old one-hour allowance."};
pub const PROBE_SENSOR_FACTOR: Param = Param {key:"probe.sensor_factor",value:0.01,unit:"power/SNR fraction",commitment:Placeholder,note:"Weaker passive arrays and active transmitter than a warship; local observations relay home at c."};
pub const PROBE_PING_INTERVAL_S: Param = Param {key:"probe.ping_interval",value:300.0,unit:"s",commitment:Placeholder,note:"Autonomous reconnaissance pulse interval; exposes probe, not carrier."};
pub const SCREEN_GLARE_W: Param = Param { key: "screen.glare_reference", value: 1e12, unit: "W", commitment: Placeholder, note: "Local thermal emission at which effective sensor noise doubles." };
pub const TRACK_LOST_S: Param = Param { key: "track.lost", value: 600.0, unit: "s since receipt", commitment: Placeholder, note: "Track is retained for identification but no longer a firing solution." };
pub const TRACK_VELOCITY_SIGMA: Param = Param { key: "track.velocity_sigma", value: 100.0, unit: "km/s", commitment: Placeholder, note: "Velocity solution confidence threshold, in addition to multiple observations." };
pub const BEAM_CAPACITOR_J: Param = Param { key: "beam.capacitor", value: 4.5e14, unit: "J", commitment: Placeholder, note: "Stored reactor energy; beam input includes conversion losses." };
pub const REACTOR_W: Param = Param { key: "beam.reactor", value: 1.5e13, unit: "W", commitment: Placeholder, note: "Power allocated to recharging the beam capacitor." };
pub const BEAM_EFFICIENCY: Param = Param { key: "beam.efficiency", value: 0.5, unit: "fraction", commitment: Placeholder, note: "Remaining beam input becomes ship heat." };
pub const BEAM_HEAT_LIMIT_J: Param = Param { key: "beam.heat_limit", value: 1.5e17, unit: "J", commitment: Placeholder, note: "Hold fire before exceeding thermal storage limit." };
pub const HULL_COOLING_S: Param = Param { key: "thermal.cooling", value: 7200.0, unit: "s", commitment: Placeholder, note: "Shared ship heat reservoir passive cooling time constant." };
pub const TACTICAL_FRAME_S: Param = Param { key: "tactical.frame", value: 1.0, unit: "s", commitment: Placeholder, note: "Command delivery, telemetry and thermal update integration interval." };
pub const BOT_PING_S: Param = Param { key: "doctrine.ping_interval", value: 300.0, unit: "s", commitment: Placeholder, note: "Opponent pulse interval while seeking a firing solution." };
pub const BOT_SALVO_S: Param = Param { key: "doctrine.salvo_interval", value: 120.0, unit: "s", commitment: Placeholder, note: "Opponent interval between mixed-payload salvos." };

pub const SHIP_BEAM_AUTO_RANGE_LS: Param = Param {
    key: "ship_beam.auto_range", value: 6.0, unit: "ls", commitment: Placeholder,
    note: "Close-combat automatic engagement band. Beyond it, automatic fire requires useful predicted energy; directed fire has no range cutoff.",
};
pub const SHIP_BEAM_MIN_EXPECTED_J:Param=Param {key:"ship_beam.min_expected_energy",value:1e9,unit:"J",commitment:Proposal,note:"Minimum expected coupled energy for automatic shots outside knife-fight range, estimated from received track covariance and beam spreading."};

pub const SHIP_BEAM_ENERGY_J: Param = Param {
    key: "ship_beam.pulse_energy", value: 7.5e13, unit: "J", commitment: Placeholder,
    note: "Emitted pulse energy, drawn from the capacitor with conversion losses into ship heat; intercepted energy enters the screen/hull model.",
};
pub const SHIP_BEAM_RECHARGE_S: Param = Param {
    key: "ship_beam.recharge", value: 5.0, unit: "s", commitment: Placeholder,
    note: "Minimum interval between ship beam pulses. Independent of the missile magazine.",
};
pub const SHIP_BEAM_DIVERGENCE: Param = Param {
    key: "ship_beam.divergence", value: 1.5e-7, unit: "rad", commitment: Placeholder,
    note: "Gaussian beam radius per distance. Coupling falls with spot area; useful combat envelope is several light-seconds, without a hard range cutoff.",
};
pub const SHIP_BEAM_POINTING_RAD: Param = Param {
    key: "ship_beam.pointing_sigma", value: 5e-8, unit: "rad", commitment: Placeholder,
    note: "Ship emitter pointing jitter, added to aim from the faction's delayed track.",
};

// Shared heat budget: a cold ship can sustain its rated maximum for one hour.
pub const SHIP_HEAT_LIMIT_J: f64 = 1e17;
pub const HEAT_BALANCED_THRUST: f64 = 0.5;
pub const SCREEN_IDLE_HEAT_FRACTION: f64 = 0.01;
pub const HEAT_FULL_BURN_S: f64 = 3600.0;
pub const HEAT_DUMP_RATE: f64 = 5.0;
pub const HEAT_DUMP_SIGNATURE: f64 = 10.0;
pub const PD_WASTE_HEAT_J: f64 = 5e12;

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
