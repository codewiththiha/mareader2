//! Seeded Gaussian noise, so the same phonemes always sound the same.

/// SplitMix64 numbers, paired into normal draws by Box–Muller.
pub(crate) struct Noise {
    state: u64,
    spare: Option<f32>,
    gain: f32,
}

impl Noise {
    pub(crate) fn new(seed: u64) -> Self {
        Self {
            state: seed,
            spare: None,
            gain: 1.0,
        }
    }

    /// Every draw scaled by `gain`; zero makes the source noiseless.
    pub(crate) fn with_gain(self, gain: f32) -> Self {
        Self { gain, ..self }
    }

    /// A seed drawn from token ids, so equal input gives equal sound.
    pub(crate) fn seed_for(ids: &[u32]) -> u64 {
        ids.iter().fold(0xCBF2_9CE4_8422_2325u64, |h, &id| {
            (h ^ u64::from(id)).wrapping_mul(0x0100_0000_01B3)
        })
    }

    fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `(0, 1]`, never zero so the log stays finite.
    fn unit(&mut self) -> f64 {
        ((self.next() >> 11) as f64 + 1.0) / (1u64 << 53) as f64
    }

    /// One draw from the standard normal distribution.
    pub(crate) fn normal(&mut self) -> f32 {
        if let Some(v) = self.spare.take() {
            return v * self.gain;
        }
        let radius = (-2.0 * self.unit().ln()).sqrt();
        let angle = std::f64::consts::TAU * self.unit();
        self.spare = Some((radius * angle.sin()) as f32);
        (radius * angle.cos()) as f32 * self.gain
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_repeat_per_seed_and_look_normal() {
        let a: Vec<f32> = (0..4)
            .map({
                let mut n = Noise::new(7);
                move |_| n.normal()
            })
            .collect();
        let b: Vec<f32> = (0..4)
            .map({
                let mut n = Noise::new(7);
                move |_| n.normal()
            })
            .collect();
        assert_eq!(a, b);
        let mut n = Noise::new(Noise::seed_for(&[1, 2, 3]));
        let draws: Vec<f64> = (0..20_000).map(|_| f64::from(n.normal())).collect();
        let mean = draws.iter().sum::<f64>() / draws.len() as f64;
        let var = draws.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / draws.len() as f64;
        assert!(mean.abs() < 0.03, "mean {mean}");
        assert!((var - 1.0).abs() < 0.05, "variance {var}");
    }
}
