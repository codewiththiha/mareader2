//! What work is keyed by: document, page and scale step.

/// One opened document; never reused within a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DocId(pub(crate) u64);

/// Eighth-octave scale steps, rounded up: GPUs shrink cleanly, not grow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Scale(i16);

impl Scale {
    const STEPS_PER_OCTAVE: f32 = 8.0;
    /// 1/64 to 32 pixels per point.
    const MIN: i16 = -48;
    const MAX: i16 = 40;

    /// The smallest step at or above `pixels_per_point`.
    pub fn covering(pixels_per_point: f32) -> Self {
        // Drop float noise so an exact step maps to itself.
        Self::step(pixels_per_point, |s| (s - 1e-3).ceil())
    }

    /// The largest step at or below `pixels_per_point`, for hard caps.
    pub fn within(pixels_per_point: f32) -> Self {
        Self::step(pixels_per_point, |s| (s + 1e-3).floor())
    }

    fn step(pixels_per_point: f32, round: fn(f32) -> f32) -> Self {
        let s = if pixels_per_point.is_finite() && pixels_per_point > 0.0 {
            pixels_per_point
        } else {
            1.0
        };
        let steps = round(s.log2() * Self::STEPS_PER_OCTAVE);
        Self((steps as i16).clamp(Self::MIN, Self::MAX))
    }

    /// Eighth octaves from one pixel per point, for nearness.
    pub fn steps(self) -> i16 {
        self.0
    }

    pub fn value(self) -> f32 {
        (f32::from(self.0) / Self::STEPS_PER_OCTAVE).exp2()
    }
}

/// A page raster's identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PageKey {
    pub doc: DocId,
    pub page: usize,
    pub scale: Scale,
}

/// Work a view asks the workers for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Want {
    Raster(PageKey),
    /// A page's characters and links, for selection and clicks.
    Text {
        doc: DocId,
        page: usize,
    },
}

impl Want {
    pub fn doc(self) -> DocId {
        match self {
            Self::Raster(k) => k.doc,
            Self::Text { doc, .. } => doc,
        }
    }
}

impl From<PageKey> for Want {
    fn from(key: PageKey) -> Self {
        Self::Raster(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_covers_its_request_by_at_most_an_eighth_octave() {
        for s in [0.1_f32, 0.5, 1.0, 1.37, 2.0, 3.3, 8.0] {
            let v = Scale::covering(s).value();
            assert!(v >= s * 0.999 && v <= s * 1.091, "{s} -> {v}");
        }
        assert_eq!(Scale::covering(1.0).value(), 1.0);
        assert_eq!(Scale::covering(2.0).value(), 2.0);
    }

    #[test]
    fn a_cap_step_never_exceeds_its_cap() {
        for s in [0.1_f32, 0.77, 1.0, 2.49, 6.0] {
            let v = Scale::within(s).value();
            assert!(v <= s * 1.0001 && v >= s / 1.091, "{s} -> {v}");
        }
        assert_eq!(Scale::within(2.0).value(), 2.0);
    }

    #[test]
    fn nearby_scales_share_a_step_and_junk_is_one() {
        assert_eq!(Scale::covering(1.30), Scale::covering(1.32));
        assert_eq!(Scale::covering(f32::NAN), Scale::covering(1.0));
        assert_eq!(Scale::covering(1e9).value(), 32.0);
    }
}
