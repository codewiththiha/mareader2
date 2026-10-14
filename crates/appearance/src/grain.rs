//! Film grain over the window; the CPU reference for `grain.wgsl`.

use crate::model::{Appearance, NoiseMode};

/// The noise repeats every 160 points, as mareader's tile does.
pub const NOISE_TILE: f32 = 160.0;
/// Noise per point (`feTurbulence baseFrequency`).
pub const NOISE_FREQUENCY: f32 = 0.9;
/// One crawl through [`CRAWL`], in seconds.
pub const CRAWL_SECS: f64 = 0.66;
/// The animated tile's offsets in points, one per eighth of a crawl.
pub const CRAWL: [[f32; 2]; 8] = [
    [0.0, 0.0],
    [-13.0, 7.0],
    [9.0, -14.0],
    [-6.0, -9.0],
    [14.0, 5.0],
    [-11.0, 12.0],
    [5.0, -6.0],
    [-8.0, -13.0],
];

/// The grain the look asks for; none when off or at zero intensity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FilmGrain {
    /// The layer's opacity, 0..1: intensity over 100.
    pub opacity: f32,
    /// Screen on dark and dim, multiply on light, as mareader's.
    pub screen: bool,
    pub animated: bool,
}

impl FilmGrain {
    pub fn of(a: &Appearance) -> Option<Self> {
        let animated = match a.noise {
            NoiseMode::Off => return None,
            NoiseMode::Static => false,
            NoiseMode::Animated => true,
        };
        let opacity = f32::from(a.noise_intensity.min(100)) / 100.0;
        (opacity > 0.0).then_some(Self {
            opacity,
            screen: a.base.is_dark(),
            animated,
        })
    }

    /// `dst` under the grain at point `p`, blended as the browser does.
    pub fn over(self, dst: [f32; 3], p: [f32; 2]) -> [f32; 3] {
        let [r, g, b, alpha] = layer(p);
        let a = alpha * self.opacity;
        let mut out = dst;
        for (d, s) in out.iter_mut().zip([r, g, b]) {
            *d = if self.screen {
                *d + a * s * (1.0 - *d)
            } else {
                *d * (1.0 - a + a * s)
            };
        }
        out
    }
}

/// The crawl's offset `secs` in, and the seconds until it steps.
pub fn crawl(secs: f64) -> ([f32; 2], f64) {
    let step = CRAWL_SECS / CRAWL.len() as f64;
    let at = secs.max(0.0) / step;
    let i = at.floor() as usize % CRAWL.len();
    let offset = CRAWL.get(i).copied().unwrap_or_default();
    (offset, (at.floor() + 1.0 - at) * step)
}

/// Scales the noise to feTurbulence's deviation, measured at 0.117.
const NOISE_SPREAD: f32 = 0.25;

/// The noise tile's colour and alpha at point `p`, each 0..1.
pub fn layer(p: [f32; 2]) -> [f32; 4] {
    let q = p.map(|v| v * NOISE_FREQUENCY);
    let cells = (NOISE_TILE * NOISE_FREQUENCY).round() as i32;
    let mut out = [0.0; 4];
    for (seed, c) in (0u32..).zip(out.iter_mut()) {
        let seed = 11 + seed * 31;
        let fine = q.map(|v| v * 2.0);
        let n = value_noise(q, cells, seed) + 0.5 * value_noise(fine, cells * 2, seed + 7);
        *c = (0.5 + n * NOISE_SPREAD).clamp(0.0, 1.0);
    }
    out
}

/// `hash` in `noise.wgsl`: 0..1 from a lattice point and a seed.
pub fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841);
    h = (h ^ seed.wrapping_mul(0xcb1a_b31f)).wrapping_mul(0x7feb_352d);
    h = (h ^ (h >> 15)).wrapping_mul(0x846c_a68b);
    h ^= h >> 16;
    h as f32 / 4_294_967_295.0
}

/// `value_noise` in `noise.wgsl`: smooth, -1..1, repeating every `cells`.
pub fn value_noise(p: [f32; 2], cells: i32, seed: u32) -> f32 {
    let [px, py] = p;
    let (x, y) = (px.floor(), py.floor());
    let (fx, fy) = (px - x, py - y);
    let (ux, uy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let (i, j) = (x as i32, y as i32);
    let at = |dx: i32, dy: i32| {
        let n = cells.max(1);
        hash((i + dx).rem_euclid(n), (j + dy).rem_euclid(n), seed)
    };
    let mix = |a: f32, b: f32, t: f32| a * (1.0 - t) + b * t;
    let top = mix(at(0, 0), at(1, 0), ux);
    let bottom = mix(at(0, 1), at(1, 1), ux);
    mix(top, bottom, uy) * 2.0 - 1.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::BaseMode;

    fn look(base: BaseMode, noise: NoiseMode, intensity: u8) -> Appearance {
        Appearance {
            base,
            noise,
            noise_intensity: intensity,
            ..Appearance::default()
        }
    }

    #[test]
    fn grain_is_off_at_zero_and_screens_on_dark_and_dim() {
        assert_eq!(FilmGrain::of(&Appearance::default()), None);
        assert_eq!(
            FilmGrain::of(&look(BaseMode::Light, NoiseMode::Static, 0)),
            None
        );
        let light = FilmGrain::of(&look(BaseMode::Light, NoiseMode::Static, 25));
        let want = FilmGrain {
            opacity: 0.25,
            screen: false,
            animated: false,
        };
        assert_eq!(light, Some(want));
        for base in [BaseMode::Dark, BaseMode::Dim] {
            let g = FilmGrain::of(&look(base, NoiseMode::Animated, 100));
            assert!(g.is_some_and(|g| g.screen && g.animated && g.opacity == 1.0));
        }
    }

    #[test]
    fn the_crawl_steps_eight_times_a_cycle() {
        let step = CRAWL_SECS / 8.0;
        let (first, wait) = crawl(0.0);
        assert_eq!(first, [0.0, 0.0]);
        assert!((wait - step).abs() < 1e-9);
        assert_eq!(crawl(step * 1.5).0, [-13.0, 7.0]);
        assert_eq!(crawl(step * 7.5).0, [-8.0, -13.0]);
        // Round again, and never stuck on a negative clock.
        assert_eq!(crawl(CRAWL_SECS + step * 0.5).0, [0.0, 0.0]);
        assert_eq!(crawl(-3.0).0, [0.0, 0.0]);
        let (_, wait) = crawl(step * 2.25);
        assert!((wait - step * 0.75).abs() < 1e-9);
    }

    #[test]
    fn the_noise_tiles_every_160_points() {
        for p in [[3.3, 7.9], [151.2, 40.4], [80.0, 159.5]] {
            let a = layer(p);
            let [x, y] = p;
            let b = layer([x + NOISE_TILE, y - NOISE_TILE]);
            for (x, y) in a.iter().zip(b) {
                assert!((x - y).abs() < 2e-3, "{a:?} {b:?}");
            }
        }
    }

    #[test]
    fn the_channels_vary_apart_around_a_half() {
        let samples: Vec<[f32; 4]> = (0..4000)
            .map(|i| layer([(i % 80) as f32 * 1.7, (i / 80) as f32 * 2.3]))
            .collect();
        for c in 0..4 {
            let v: Vec<f32> = samples.iter().filter_map(|s| s.get(c).copied()).collect();
            let mean = v.iter().sum::<f32>() / v.len() as f32;
            let spread =
                (v.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / v.len() as f32).sqrt();
            assert!((mean - 0.5).abs() < 0.05, "channel {c} mean {mean}");
            // feTurbulence's fractalNoise here deviates by 0.117.
            assert!((spread - 0.117).abs() < 0.02, "channel {c} spread {spread}");
        }
        let coloured = samples.iter().any(|[r, g, ..]| (r - g).abs() > 0.1);
        assert!(coloured, "the channels differ");
    }

    #[test]
    fn multiply_only_darkens_and_screen_only_lightens() {
        let mut g = FilmGrain {
            opacity: 1.0,
            screen: false,
            animated: false,
        };
        let points = (0..200).map(|i| [i as f32 * 0.77, i as f32 * 1.31]);
        for p in points.clone() {
            let out = g.over([1.0; 3], p);
            assert!(out.iter().all(|c| *c <= 1.0 && *c >= 0.0));
        }
        let mean = |g: FilmGrain, base: f32| {
            let sum: f32 = points.clone().map(|p| g.over([base; 3], p)[1]).sum();
            sum / 200.0
        };
        assert!(mean(g, 1.0) < 0.9, "multiply darkens white");
        g.screen = true;
        assert!(mean(g, 0.0) > 0.1, "screen lightens black");
        assert!(
            g.over([1.0; 3], [5.0, 5.0])
                .iter()
                .all(|c| (*c - 1.0).abs() < 1e-6)
        );
        g.opacity = 0.0;
        assert_eq!(g.over([0.3; 3], [5.0, 5.0]), [0.3; 3]);
    }
}
