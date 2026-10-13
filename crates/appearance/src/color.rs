//! sRGB and OKLCH, without a colour library.

/// An 8-bit sRGB colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// An OKLCH colour: lightness 0..1, chroma ≥ 0, hue in degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Oklch {
    pub l: f64,
    pub c: f64,
    pub h: f64,
}

fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f64) -> f64 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

fn unit_to_u8(c: f64) -> u8 {
    // NaN casts to 0, which is the safe end of the range.
    (c.clamp(0.0, 1.0) * 255.0).round() as u8
}

impl Rgb {
    /// Parses `#rrggbb` (the `#` is optional); anything else is `None`.
    pub fn from_hex(hex: &str) -> Option<Self> {
        let h = hex.trim().trim_start_matches('#');
        if h.len() != 6 || !h.is_ascii() {
            return None;
        }
        let byte = |i: usize| h.get(i..i + 2).and_then(|s| u8::from_str_radix(s, 16).ok());
        Some(Self(byte(0)?, byte(2)?, byte(4)?))
    }

    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }

    /// Components as 0..1 floats, still sRGB-encoded.
    pub fn to_unit(self) -> [f32; 3] {
        [self.0, self.1, self.2].map(|c| f32::from(c) / 255.0)
    }

    pub fn from_unit(c: [f32; 3]) -> Self {
        let [r, g, b] = c.map(|v| unit_to_u8(f64::from(v)));
        Self(r, g, b)
    }

    pub fn to_oklch(self) -> Oklch {
        let [r, g, b] = [self.0, self.1, self.2].map(|c| srgb_to_linear(f64::from(c) / 255.0));
        let l = 0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b;
        let m = 0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b;
        let s = 0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b;
        let (l_, m_, s_) = (l.cbrt(), m.cbrt(), s.cbrt());
        let ll = 0.210_454_255_3 * l_ + 0.793_617_785_0 * m_ - 0.004_072_046_8 * s_;
        let aa = 1.977_998_495_1 * l_ - 2.428_592_205_0 * m_ + 0.450_593_709_9 * s_;
        let bb = 0.025_904_037_1 * l_ + 0.782_771_766_2 * m_ - 0.808_675_766_0 * s_;
        Oklch {
            l: ll,
            c: (aa * aa + bb * bb).sqrt(),
            h: bb.atan2(aa).to_degrees().rem_euclid(360.0),
        }
    }
}

impl Oklch {
    /// Back to sRGB, clipping per channel where CSS would gamut-map.
    pub fn to_rgb(self) -> Rgb {
        let l = self.l.clamp(0.0, 1.0);
        let c = self.c.max(0.0);
        let (sin, cos) = self.h.to_radians().sin_cos();
        let (a, b) = (c * cos, c * sin);
        let l_ = l + 0.396_337_777_4 * a + 0.215_803_757_3 * b;
        let m_ = l - 0.105_561_345_8 * a - 0.063_854_172_8 * b;
        let s_ = l - 0.089_484_177_5 * a - 1.291_485_548_0 * b;
        let (lc, mc, sc) = (l_.powi(3), m_.powi(3), s_.powi(3));
        let r = 4.076_741_662_1 * lc - 3.307_711_591_3 * mc + 0.230_969_929_2 * sc;
        let g = -1.268_438_004_6 * lc + 2.609_757_401_1 * mc - 0.341_319_396_5 * sc;
        let bl = -0.004_196_086_3 * lc - 0.703_418_614_7 * mc + 1.707_614_701_0 * sc;
        Rgb(
            unit_to_u8(linear_to_srgb(r)),
            unit_to_u8(linear_to_srgb(g)),
            unit_to_u8(linear_to_srgb(bl)),
        )
    }
}

/// Moves hue `from` toward `to` by `t` along the shorter arc.
pub fn hue_toward(from: f64, to: f64, t: f64) -> f64 {
    let delta = (to - from + 180.0).rem_euclid(360.0) - 180.0;
    (from + delta * t).rem_euclid(360.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips_and_rejects_junk() {
        assert_eq!(Rgb::from_hex("#2563eb"), Some(Rgb(0x25, 0x63, 0xeb)));
        assert_eq!(Rgb(0x25, 0x63, 0xeb).to_hex(), "#2563eb");
        for junk in ["", "#12345", "#gggggg", "#ééé", "1234567"] {
            assert_eq!(Rgb::from_hex(junk), None, "{junk}");
        }
    }

    #[test]
    fn white_and_black_are_achromatic_extremes() {
        let w = Rgb(255, 255, 255).to_oklch();
        assert!((w.l - 1.0).abs() < 0.002 && w.c < 0.002, "{w:?}");
        let k = Rgb(0, 0, 0).to_oklch();
        assert!(k.l.abs() < 0.002, "{k:?}");
    }

    #[test]
    fn oklch_round_trips_in_gamut_colours() {
        for hex in ["#2563eb", "#131316", "#7a9bd4", "#f59e0b", "#e5e7eb"] {
            let rgb = Rgb::from_hex(hex).unwrap();
            let back = rgb.to_oklch().to_rgb();
            let d = |a: u8, b: u8| a.abs_diff(b);
            assert!(
                d(rgb.0, back.0) <= 1 && d(rgb.1, back.1) <= 1 && d(rgb.2, back.2) <= 1,
                "{hex} → {back:?}"
            );
        }
    }

    #[test]
    fn hue_takes_the_short_arc() {
        assert!((hue_toward(350.0, 10.0, 0.5) - 0.0).abs() < 1e-9);
        assert!((hue_toward(10.0, 350.0, 0.5) - 0.0).abs() < 1e-9);
        assert!((hue_toward(90.0, 180.0, 1.0) - 180.0).abs() < 1e-9);
    }
}
