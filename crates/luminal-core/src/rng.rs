//! Deterministic random streams. No global state: every consumer owns a seeded stream,
//! and streams are only drawn from in simulation order, so a run is reproducible
//! regardless of warp or frame rate.

/// SplitMix64. Small, fast, and good enough for measurement noise.
#[derive(Clone, Debug)]
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// An independent stream derived from this seed and a label.
    pub fn stream(seed: u64, label: u64) -> Self {
        let mut r = Self::new(seed ^ label.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        r.next_u64();
        r
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in (0, 1].
    pub fn uniform(&mut self) -> f64 {
        ((self.next_u64() >> 11) as f64 + 1.0) / (1u64 << 53) as f64
    }

    /// Standard normal via Box–Muller.
    pub fn gaussian(&mut self) -> f64 {
        let (u1, u2) = (self.uniform(), self.uniform());
        (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_sequence() {
        let mut a = Rng::stream(7, 3);
        let mut b = Rng::stream(7, 3);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
        assert_ne!(Rng::stream(7, 3).next_u64(), Rng::stream(7, 4).next_u64());
    }

    #[test]
    fn gaussian_has_unit_variance() {
        let mut r = Rng::new(1);
        let n = 20_000;
        let xs: Vec<f64> = (0..n).map(|_| r.gaussian()).collect();
        let mean = xs.iter().sum::<f64>() / n as f64;
        let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n as f64;
        assert!(mean.abs() < 0.03, "{mean}");
        assert!((var - 1.0).abs() < 0.05, "{var}");
    }
}
