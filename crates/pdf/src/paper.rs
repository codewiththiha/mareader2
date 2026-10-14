//! A page's paper colour, and how it changes between pages.

use std::collections::{BTreeMap, HashMap};

use mareader_appearance::Rgb;

use crate::document::{Document, Raster, RenderError};

/// A sample's longest side, in pixels: plenty to find the paper.
pub const SAMPLE_SIDE: f32 = 96.0;
/// The margin the edges area reads, in sample pixels.
pub const EDGE: usize = 10;
/// The share of read pixels a colour needs to count as the paper.
pub const SHARE: f32 = 0.1;

/// One page's paper, read two ways; none where no colour dominates.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Paper {
    pub whole: Option<Rgb>,
    /// Only the margins, for scans whose text or pictures fill the page.
    pub edges: Option<Rgb>,
}

impl Paper {
    /// The dominant colours of an opaque raster.
    pub fn detect(raster: &Raster) -> Self {
        let (w, h) = (raster.width as usize, raster.height as usize);
        if w == 0 || h == 0 || raster.pixels.len() < w * h * 4 {
            return Self::default();
        }
        let edge = EDGE.clamp(1, (w.min(h) / 4).max(1));
        let (mut whole, mut edges) = (Histogram::default(), Histogram::default());
        let (pixels, _) = raster.pixels.as_chunks::<4>();
        for (i, &[r, g, b, _]) in pixels.iter().take(w * h).enumerate() {
            let (x, y) = (i % w, i / w);
            whole.add(r, g, b);
            if x < edge || y < edge || x >= w - edge || y >= h - edge {
                edges.add(r, g, b);
            }
        }
        Self {
            whole: whole.dominant(),
            edges: edges.dominant(),
        }
    }
}

impl Document {
    /// Page `page` drawn small and read; never panics.
    pub fn paper(&self, page: usize) -> Result<Paper, RenderError> {
        let size = self
            .page_sizes()
            .get(page)
            .ok_or(RenderError::NoPage(page))?;
        let scale = SAMPLE_SIDE / size.width.max(size.height);
        Ok(Paper::detect(&self.render(page, scale)?))
    }
}

/// Colours in 5-bit-per-channel buckets, with sums for their means.
#[derive(Default)]
struct Histogram {
    buckets: HashMap<u16, [u32; 4]>,
    total: u32,
}

impl Histogram {
    fn add(&mut self, r: u8, g: u8, b: u8) {
        let key = (u16::from(r >> 3) << 10) | (u16::from(g >> 3) << 5) | u16::from(b >> 3);
        let [n, sr, sg, sb] = self.buckets.entry(key).or_default();
        *n += 1;
        *sr += u32::from(r);
        *sg += u32::from(g);
        *sb += u32::from(b);
        self.total += 1;
    }

    /// The biggest bucket's mean, if it holds at least [`SHARE`].
    fn dominant(&self) -> Option<Rgb> {
        // Ties go to the lower key, so the answer never depends on hashing.
        let best = self.buckets.iter().max_by(|a, b| {
            let (ka, kb) = (a.0, b.0);
            a.1[0].cmp(&b.1[0]).then(kb.cmp(ka))
        });
        let (_, &[n, r, g, b]) = best?;
        if (n as f32) < SHARE * self.total as f32 || n == 0 {
            return None;
        }
        let mean = |s: u32| ((s + n / 2) / n).min(255) as u8;
        Some(Rgb(mean(r), mean(g), mean(b)))
    }
}

/// Known pages' paper; the pages between are mixed from their neighbours.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Palette {
    known: BTreeMap<usize, Rgb>,
}

impl Palette {
    pub fn set(&mut self, page: usize, colour: Rgb) {
        self.known.insert(page, colour);
    }

    pub fn contains(&self, page: usize) -> bool {
        self.known.contains_key(&page)
    }

    pub fn is_empty(&self) -> bool {
        self.known.is_empty()
    }

    /// The paper at fractional page `at`; outside the known span, its end.
    pub fn at(&self, at: f64) -> Option<Rgb> {
        if !at.is_finite() {
            return None;
        }
        let at = at.max(0.0);
        let below = self.known.range(..=at.floor() as usize).next_back();
        let above = self.known.range(at.ceil() as usize..).next();
        match (below, above) {
            (Some((&lo, &a)), Some((&hi, &b))) if hi > lo => {
                Some(lerp(a, b, (at - lo as f64) / (hi - lo) as f64))
            }
            (Some((_, &c)), _) | (None, Some((_, &c))) => Some(c),
            (None, None) => None,
        }
    }
}

/// `a` to `b` by `t`, each channel rounded.
fn lerp(a: Rgb, b: Rgb, t: f64) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| (f64::from(x) + (f64::from(y) - f64::from(x)) * t).round() as u8;
    Rgb(mix(a.0, b.0), mix(a.1, b.1), mix(a.2, b.2))
}

/// The page looked at, fractional: pages weighted by their area on screen.
pub fn position(shown: impl IntoIterator<Item = (usize, f32)>) -> Option<f64> {
    let (mut sum, mut weight) = (0.0, 0.0);
    for (page, area) in shown {
        if area.is_finite() && area > 0.0 {
            sum += page as f64 * f64::from(area);
            weight += f64::from(area);
        }
    }
    (weight > 0.0).then(|| sum / weight)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CREAM: Rgb = Rgb(0xfa, 0xf4, 0xe8);
    const BLUE: Rgb = Rgb(0x20, 0x40, 0x80);

    fn raster(w: u32, h: u32, at: impl Fn(u32, u32) -> Rgb) -> Raster {
        let mut pixels = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let Rgb(r, g, b) = at(x, y);
                pixels.extend([r, g, b, 255]);
            }
        }
        Raster {
            width: w,
            height: h,
            pixels,
        }
    }

    #[test]
    fn a_plain_page_is_its_own_paper() {
        let p = Paper::detect(&raster(40, 50, |_, _| CREAM));
        assert_eq!(p.whole, Some(CREAM));
        assert_eq!(p.edges, Some(CREAM));
    }

    #[test]
    fn the_paper_is_the_mean_of_its_bucket() {
        // Two shades in one bucket average, rounded.
        let p = Paper::detect(&raster(40, 40, |x, _| {
            if x % 2 == 0 {
                Rgb(248, 240, 232)
            } else {
                Rgb(251, 243, 235)
            }
        }));
        assert_eq!(p.whole, Some(Rgb(250, 242, 234)));
    }

    #[test]
    fn the_edges_ignore_a_picture_filling_the_middle() {
        let p = Paper::detect(&raster(60, 80, |x, y| {
            let inside = (10..50).contains(&x) && (10..70).contains(&y);
            if inside { BLUE } else { CREAM }
        }));
        assert_eq!(p.whole, Some(BLUE));
        assert_eq!(p.edges, Some(CREAM));
    }

    #[test]
    fn noise_has_no_paper() {
        // Every pixel its own bucket: none reaches the share.
        let p = Paper::detect(&raster(32, 32, |x, y| {
            Rgb((x * 8) as u8, (y * 8) as u8, ((x + y) * 4) as u8)
        }));
        assert_eq!(p, Paper::default());
    }

    #[test]
    fn a_short_or_empty_raster_reads_nothing() {
        let mut r = raster(8, 8, |_, _| CREAM);
        r.pixels.truncate(10);
        assert_eq!(Paper::detect(&r), Paper::default());
        assert_eq!(Paper::detect(&raster(0, 0, |_, _| CREAM)), Paper::default());
    }

    #[test]
    fn a_tiny_raster_still_has_edges() {
        let p = Paper::detect(&raster(3, 3, |_, _| CREAM));
        assert_eq!(p.edges, Some(CREAM));
    }

    #[test]
    fn the_sample_page_reads_white_paper() {
        let bytes = std::sync::Arc::new(crate::sample::pdf(2, "t"));
        let doc = Document::open(bytes).expect("the sample opens");
        let paper = doc.paper(0).expect("page 0 draws");
        assert_eq!(paper.whole, Some(Rgb(255, 255, 255)));
        assert!(doc.paper(9).is_err());
    }

    #[test]
    fn a_scan_reads_its_tinted_paper() {
        let bytes = std::sync::Arc::new(crate::sample::scan(6, "t"));
        let doc = Document::open(bytes).expect("the scan opens");
        let first = doc.paper(0).expect("page 0 draws").edges;
        let sixth = doc.paper(5).expect("page 5 draws").edges;
        assert!(first.is_some() && sixth.is_some());
        assert_ne!(first, sixth, "chapters change paper");
    }

    #[test]
    fn the_palette_mixes_between_known_pages() {
        let mut p = Palette::default();
        assert_eq!(p.at(3.0), None);
        p.set(2, Rgb(0, 0, 0));
        p.set(6, Rgb(200, 100, 40));
        assert_eq!(p.at(2.0), Some(Rgb(0, 0, 0)));
        assert_eq!(p.at(4.0), Some(Rgb(100, 50, 20)));
        assert_eq!(p.at(5.5), Some(Rgb(175, 88, 35)));
        assert_eq!(p.at(6.0), Some(Rgb(200, 100, 40)));
    }

    #[test]
    fn the_palette_clamps_to_its_known_span() {
        let mut p = Palette::default();
        p.set(3, CREAM);
        p.set(5, BLUE);
        assert_eq!(p.at(0.0), Some(CREAM));
        assert_eq!(p.at(-4.0), Some(CREAM));
        assert_eq!(p.at(99.0), Some(BLUE));
        assert_eq!(p.at(f64::NAN), None);
        assert!(p.contains(3) && !p.contains(4) && !p.is_empty());
    }

    #[test]
    fn the_position_weights_pages_by_area() {
        assert_eq!(position([]), None);
        assert_eq!(position([(4, 100.0)]), Some(4.0));
        assert_eq!(position([(2, 300.0), (3, 100.0)]), Some(2.25));
        assert_eq!(position([(1, 50.0), (2, 50.0)]), Some(1.5));
    }

    #[test]
    fn the_position_ignores_nothing_shown() {
        assert_eq!(position([(5, 0.0), (6, -2.0), (7, f32::NAN)]), None);
        assert_eq!(position([(5, 0.0), (6, 10.0)]), Some(6.0));
    }
}
