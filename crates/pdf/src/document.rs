//! One opened PDF: its pages' sizes up front, rasters on demand.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use hayro::hayro_interpret::{
    Context, InterpreterCache, InterpreterSettings, TransformExt as _, interpret_page,
};
use hayro::hayro_syntax::{LoadPdfError, Pdf};
use hayro::kurbo::{Affine, Rect};
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::vello_cpu::{Pixmap, RasterizerSettings, RenderContext, Resources, TargetInit};
use hayro::{PixmapSettings, RenderCache, RenderSettings};

use crate::dest::{self, PageIds, Targets};
use crate::links;
use crate::outline::{self, OutlineEntry};
use crate::text::{Collector, PageText, TextLayer};
use crate::tiles::Tiling;

/// More pages than this is treated as a broken file.
pub const MAX_PAGES: usize = 100_000;
/// A raster's longest side; larger pages render at a lower scale.
pub const MAX_RASTER_SIDE: u32 = 4096;
/// A raster's pixel count, so a long strip can't take 4096².
pub const MAX_RASTER_PIXELS: u64 = 4096 * 4096;

/// PDF user space allows at most 14,400 points a side.
const MAX_PAGE_PT: f32 = 14_400.0;
/// US Letter, for pages whose boxes are unusable.
const FALLBACK: PageSize = PageSize {
    width: 612.0,
    height: 792.0,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum OpenError {
    #[error("the file is password-protected")]
    Encrypted,
    #[error("the file isn't a readable PDF")]
    Invalid,
    #[error("the PDF has no pages")]
    Empty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RenderError {
    #[error("there is no page {0}")]
    NoPage(usize),
    #[error("the page couldn't be drawn")]
    Failed,
}

/// A page's size in points, its rotation applied.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageSize {
    pub width: f32,
    pub height: f32,
}

impl PageSize {
    /// `scale`, lowered so the page's raster fits the size caps.
    pub fn fit_scale(self, scale: f32) -> f32 {
        fit_scale(self, scale)
    }

    fn sane(width: f32, height: f32) -> Self {
        let ok = |v: f32| v.is_finite() && v >= 1.0;
        if ok(width) && ok(height) {
            Self {
                width: width.min(MAX_PAGE_PT),
                height: height.min(MAX_PAGE_PT),
            }
        } else {
            FALLBACK
        }
    }
}

/// Premultiplied RGBA8, `width × height`, opaque white under the page.
#[derive(Clone, PartialEq, Eq)]
pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl std::fmt::Debug for Raster {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Raster({}×{})", self.width, self.height)
    }
}

impl Raster {
    pub fn bytes(&self) -> usize {
        self.pixels.len()
    }
}

/// An open PDF. `Send + Sync`: workers share one through an `Arc`.
pub struct Document {
    pdf: Pdf,
    sizes: Vec<PageSize>,
    title: Option<String>,
    outline: Vec<OutlineEntry>,
    page_ids: PageIds,
}

impl std::fmt::Debug for Document {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Document({} pages)", self.sizes.len())
    }
}

impl Document {
    /// Parses the file and measures every page; never panics.
    pub fn open(bytes: Arc<Vec<u8>>) -> Result<Self, OpenError> {
        catch_unwind(AssertUnwindSafe(|| Self::parse(bytes))).unwrap_or(Err(OpenError::Invalid))
    }

    fn parse(bytes: Arc<Vec<u8>>) -> Result<Self, OpenError> {
        let pdf = Pdf::new(bytes).map_err(|e| match e {
            LoadPdfError::Decryption(_) => OpenError::Encrypted,
            LoadPdfError::Invalid => OpenError::Invalid,
        })?;
        let pages = pdf.pages();
        if pages.is_empty() {
            return Err(OpenError::Empty);
        }
        if pages.len() > MAX_PAGES {
            return Err(OpenError::Invalid);
        }
        let sizes = pages
            .iter()
            .map(|p| {
                let (w, h) = p.render_dimensions();
                PageSize::sane(w, h)
            })
            .collect();
        let title = pdf.metadata().title.as_deref().map(decode_text);
        let page_ids = dest::page_ids(&pdf);
        let outline = outline::read(&pdf, &page_ids);
        Ok(Self {
            pdf,
            sizes,
            title,
            outline,
            page_ids,
        })
    }

    pub fn page_count(&self) -> usize {
        self.sizes.len()
    }

    pub fn page_sizes(&self) -> &[PageSize] {
        &self.sizes
    }

    /// Chapters in reading order; empty when the file has none.
    pub fn outline(&self) -> &[OutlineEntry] {
        &self.outline
    }

    /// The `/Title` as written; see `display_name` for what to show.
    pub fn raw_title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// The scale `render` really uses: `scale`, lowered to fit the caps.
    pub fn fitted_scale(&self, page: usize, scale: f32) -> Option<f32> {
        let size = self.sizes.get(page)?;
        Some(fit_scale(*size, scale))
    }

    /// Draws page `page` (0-based) at `scale` pixels per point.
    pub fn render(&self, page: usize, scale: f32) -> Result<Raster, RenderError> {
        let size = *self.sizes.get(page).ok_or(RenderError::NoPage(page))?;
        let scale = fit_scale(size, scale);
        catch_unwind(AssertUnwindSafe(|| self.raster(page, scale)))
            .unwrap_or(Err(RenderError::Failed))
    }

    /// Tile `col`, `row` of page `page` split at `scale`; see [`Tiling`].
    pub fn render_tile(
        &self,
        page: usize,
        scale: f32,
        col: u32,
        row: u32,
    ) -> Result<Raster, RenderError> {
        let size = *self.sizes.get(page).ok_or(RenderError::NoPage(page))?;
        let tiling = Tiling::new(size, scale);
        let rect = tiling.rect(col, row).ok_or(RenderError::Failed)?;
        catch_unwind(AssertUnwindSafe(|| self.tile(page, tiling.scale, rect)))
            .unwrap_or(Err(RenderError::Failed))
    }

    /// Page `page`'s characters and links; never panics.
    pub fn text_layer(&self, page: usize) -> Result<TextLayer, RenderError> {
        if page >= self.sizes.len() {
            return Err(RenderError::NoPage(page));
        }
        catch_unwind(AssertUnwindSafe(|| self.read_layer(page))).unwrap_or(Err(RenderError::Failed))
    }

    fn read_layer(&self, index: usize) -> Result<TextLayer, RenderError> {
        let pages = self.pdf.pages();
        let page = pages.get(index).ok_or(RenderError::NoPage(index))?;
        let (w, h) = page.render_dimensions();
        // Form fields draw their values, but pdf.js doesn't select them.
        let settings = InterpreterSettings {
            render_annotations: false,
            ..InterpreterSettings::default()
        };
        let cache = InterpreterCache::new();
        let start = page.initial_transform(true).to_kurbo();
        let bbox = Rect::new(0.0, 0.0, f64::from(w), f64::from(h));
        let mut cx = Context::new(start, bbox, &cache, page.xref(), settings);
        let mut collector = Collector::default();
        interpret_page(page, &mut cx, &mut collector);
        let links = Targets::new(&self.pdf, &self.page_ids)
            .map(|t| links::read(page, index, &t))
            .unwrap_or_default();
        Ok(TextLayer {
            text: PageText::from_glyphs(collector.glyphs),
            links,
        })
    }

    fn raster(&self, page: usize, scale: f32) -> Result<Raster, RenderError> {
        let pages = self.pdf.pages();
        let page = pages.get(page).ok_or(RenderError::NoPage(page))?;
        let cache = RenderCache::new();
        let settings = PixmapSettings {
            x_scale: scale,
            y_scale: scale,
            bg_color: WHITE,
        };
        let interpreter = InterpreterSettings::default();
        let pixmap = hayro::render(
            page,
            &cache,
            &interpreter,
            &RenderSettings::default(),
            &settings,
        );
        finish(&pixmap)
    }

    /// hayro's `render`, shifted so the tile's corner is the origin.
    fn tile(&self, page: usize, scale: f32, [x, y, w, h]: [u32; 4]) -> Result<Raster, RenderError> {
        let pages = self.pdf.pages();
        let page = pages.get(page).ok_or(RenderError::NoPage(page))?;
        let (w, h) = (u16::try_from(w), u16::try_from(h));
        let (Ok(w), Ok(h)) = (w, h) else {
            return Err(RenderError::Failed);
        };
        let mut ctx = RenderContext::new(w, h);
        let shift = Affine::translate((-f64::from(x), -f64::from(y)));
        let transform =
            shift * Affine::scale(f64::from(scale)) * page.initial_transform(true).to_kurbo();
        let cache = RenderCache::new();
        let interpreter = InterpreterSettings::default();
        let settings = RenderSettings::default();
        hayro::render_into(page, &cache, &interpreter, &settings, &mut ctx, transform);
        ctx.flush();
        let mut pixmap = Pixmap::new(w, h);
        let raster = RasterizerSettings {
            target_init: TargetInit::Clear(WHITE),
            ..Default::default()
        };
        ctx.render_with(&mut pixmap, &mut Resources::default(), raster);
        finish(&pixmap)
    }
}

fn finish(pixmap: &Pixmap) -> Result<Raster, RenderError> {
    let (width, height) = (u32::from(pixmap.width()), u32::from(pixmap.height()));
    if width == 0 || height == 0 {
        return Err(RenderError::Failed);
    }
    Ok(Raster {
        width,
        height,
        pixels: pixmap.data_as_u8_slice().to_vec(),
    })
}

/// Lowers `scale` until the raster fits the side and pixel caps.
fn fit_scale(size: PageSize, scale: f32) -> f32 {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let side = MAX_RASTER_SIDE as f32 / size.width.max(size.height);
    let area = (MAX_RASTER_PIXELS as f32 / (size.width * size.height)).sqrt();
    scale.min(side).min(area).max(1.0 / 64.0)
}

/// A PDF text string: UTF-16 with a BOM, UTF-8 with one, else Latin-1.
pub(crate) fn decode_text(bytes: &[u8]) -> String {
    match bytes {
        [0xFE, 0xFF, rest @ ..] => utf16(rest, u16::from_be_bytes),
        [0xFF, 0xFE, rest @ ..] => utf16(rest, u16::from_le_bytes),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => bytes.iter().map(|&b| char::from(b)).collect(),
    }
}

fn utf16(bytes: &[u8], word: fn([u8; 2]) -> u16) -> String {
    let units = bytes.as_chunks::<2>().0.iter().map(|c| word(*c));
    char::decode_utf16(units)
        .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample;

    type Out = Result<(), Box<dyn std::error::Error>>;

    fn sample_doc(pages: usize) -> Result<Document, OpenError> {
        Document::open(Arc::new(sample::pdf(pages, "Sample")))
    }

    fn dark(r: &Raster) -> bool {
        r.pixels.as_chunks::<4>().0.iter().any(|[r, ..]| *r < 128)
    }

    #[test]
    fn a_sample_opens_with_its_page_sizes_and_title() -> Out {
        let doc = sample_doc(3)?;
        assert_eq!(doc.page_count(), 3);
        let letter = PageSize {
            width: 612.0,
            height: 792.0,
        };
        assert_eq!(doc.page_sizes().first(), Some(&letter));
        assert_eq!(doc.raw_title(), Some("Sample"));
        Ok(())
    }

    #[test]
    fn a_page_renders_at_the_scale_asked_with_ink_on_it() -> Out {
        let r = sample_doc(2)?.render(1, 0.5)?;
        assert_eq!((r.width, r.height), (306, 396));
        assert_eq!(r.bytes(), 306 * 396 * 4);
        assert!(dark(&r), "the page has text");
        assert!(r.pixels.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
        Ok(())
    }

    #[test]
    fn huge_scales_stay_under_the_caps() -> Out {
        let doc = sample_doc(1)?;
        let r = doc.render(0, 1000.0)?;
        assert!(r.width.max(r.height) <= MAX_RASTER_SIDE);
        assert!(u64::from(r.width) * u64::from(r.height) <= MAX_RASTER_PIXELS);
        for bad in [f32::NAN, -1.0, 0.0, f32::INFINITY] {
            assert!(doc.render(0, bad).is_ok());
        }
        Ok(())
    }

    #[test]
    fn a_tile_matches_its_part_of_the_whole_page() -> Out {
        let doc = sample_doc(1)?;
        // At 2×, 1224 × 1584 splits into 2 × 2 tiles of 1024.
        let whole = doc.render(0, 2.0)?;
        let tiling = Tiling::new(doc.page_sizes()[0], 2.0);
        assert_eq!((tiling.width, tiling.height), (whole.width, whole.height));
        let [x, y, w, h] = tiling.rect(1, 1).ok_or("no tile")?;
        let tile = doc.render_tile(0, 2.0, 1, 1)?;
        assert_eq!((tile.width, tile.height), (w, h));
        let mut off = 0usize;
        for row in 0..h {
            for col in 0..w {
                let at = |r: &Raster, x: u32, y: u32| (y * r.width + x) as usize * 4;
                let a = &tile.pixels[at(&tile, col, row)..][..4];
                let b = &whole.pixels[at(&whole, x + col, y + row)..][..4];
                let diff = a.iter().zip(b).map(|(a, b)| a.abs_diff(*b)).max();
                off += usize::from(diff.unwrap_or(0) > 2);
            }
        }
        assert!(off < 20, "{off} pixels differ");
        assert!(
            dark(&doc.render_tile(0, 2.0, 0, 0)?),
            "the title's tile has ink"
        );
        assert!(doc.render_tile(0, 2.0, 2, 0).is_err());
        Ok(())
    }

    #[test]
    fn a_page_reads_back_its_lines_words_and_links() -> Out {
        let doc = sample_doc(12)?;
        let layer = doc.text_layer(1)?;
        let text = layer.text.text(0..layer.text.len());
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.first(), Some(&"Page 2 of 12"));
        assert!(lines.contains(&"Sphinx of black quartz, judge my vow."));
        assert!(lines.contains(&"Next chapter, page 6 github.com/codewiththiha/mareader2"));
        let targets: Vec<_> = layer.links.iter().map(|l| l.target.clone()).collect();
        let web = crate::LinkTarget::Web(sample::WEB_LINK.into());
        assert_eq!(targets, [crate::LinkTarget::Page(5), web]);
        // Each link covers its label's first letter.
        for link in &layer.links {
            let b = link.bounds;
            let hit = layer.text.char_at(b.x0 + 3.0, (b.y0 + b.y1) / 2.0);
            assert!(hit.is_some(), "{b:?}");
        }
        assert_eq!(doc.text_layer(99).err(), Some(RenderError::NoPage(99)));
        Ok(())
    }

    #[test]
    fn a_missing_page_is_an_error() -> Out {
        assert_eq!(sample_doc(1)?.render(5, 1.0), Err(RenderError::NoPage(5)));
        Ok(())
    }

    #[test]
    fn garbage_and_truncation_are_refused_not_panics() {
        let junk = Document::open(Arc::new(b"not a pdf".to_vec()));
        assert_eq!(junk.err(), Some(OpenError::Invalid));
        let full = sample::pdf(2, "Cut");
        for cut in [0, 9, full.len() / 3, full.len() / 2, full.len() - 10] {
            let part = full.get(..cut).unwrap_or_default().to_vec();
            // hayro may recover a truncated file; it must not panic.
            if let Ok(doc) = Document::open(Arc::new(part)) {
                let _ = doc.render(0, 0.25);
            }
        }
    }

    #[test]
    fn bad_page_boxes_fall_back_to_letter() {
        assert_eq!(PageSize::sane(f32::NAN, 10.0), FALLBACK);
        assert_eq!(PageSize::sane(0.0, 10.0), FALLBACK);
        assert_eq!(PageSize::sane(1e9, 100.0).width, MAX_PAGE_PT);
    }

    #[test]
    fn text_strings_decode_from_each_encoding() {
        assert_eq!(decode_text(b"Plain"), "Plain");
        assert_eq!(decode_text(&[0xFE, 0xFF, 0, b'H', 0, b'i']), "Hi");
        assert_eq!(decode_text(&[0xFE, 0xFF, 0xD8, 0x00]), "\u{FFFD}");
        assert_eq!(decode_text(&[0xE9]), "é");
    }
}
