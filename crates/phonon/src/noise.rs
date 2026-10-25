//! Seeded Gaussian noise for the flow head.

/// Normal draws of variance `temperature`; none at all at zero.
#[derive(Clone, Debug)]
pub struct Noise {
    state: u64,
    std: f32,
    /// A second draw kept from the last pair.
    spare: Option<f32>,
}

impl Noise {
    pub fn new(temperature: f32, seed: u64) -> Self {
        Self {
            state: seed ^ 0x9E37_79B9_7F4A_7C15,
            std: temperature.max(0.0).sqrt(),
            spare: None,
        }
    }

    /// Fills `out`; zeros when the temperature is zero.
    pub fn fill(&mut self, out: &mut [f32]) {
        for x in out {
            *x = match self.std > 0.0 {
                true => self.normal() * self.std,
                false => 0.0,
            };
        }
    }

    /// splitmix64, then a uniform in (0, 1].
    fn uniform(&mut self) -> f64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        ((z >> 11) as f64 + 1.0) / (1u64 << 53) as f64
    }

    /// Box–Muller, a pair at a time.
    fn normal(&mut self) -> f32 {
        if let Some(s) = self.spare.take() {
            return s;
        }
        let (u, v) = (self.uniform(), self.uniform());
        let r = (-2.0 * u.ln()).sqrt();
        let a = std::f64::consts::TAU * v;
        self.spare = Some((r * a.sin()) as f32);
        (r * a.cos()) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_is_seeded_and_silent_when_cold() {
        let (mut a, mut b) = (Noise::new(0.3, 7), Noise::new(0.3, 7));
        let (mut x, mut y) = (vec![0.0; 64], vec![0.0; 64]);
        a.fill(&mut x);
        b.fill(&mut y);
        assert_eq!(x, y);
        let var = x.iter().map(|v| v * v).sum::<f32>() / 64.0;
        assert!(var > 0.1 && var < 0.6, "{var}");
        let mut cold = Noise::new(0.0, 7);
        cold.fill(&mut x);
        assert!(x.iter().all(|&v| v == 0.0));
    }
}
