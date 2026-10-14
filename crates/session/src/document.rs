//! Either format behind one set of calls, so views never ask which.
use std::sync::Arc;

use mareader_epub::Book;
use mareader_pdf::Tiling;
use mareader_pdf::{OutlineEntry, PageSize, Paper, Raster, RenderError, SAMPLE_SIDE, TextLayer};

use crate::OpenError;

/// An open document. An EPUB's layout unit shows as one point.
pub enum Document {
    Pdf(mareader_pdf::Document),
    Epub { book: Book, sizes: Vec<PageSize> },
}

impl std::fmt::Debug for Document {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pdf(d) => d.fmt(f),
            Self::Epub { book, .. } => book.fmt(f),
        }
    }
}

impl Document {
    /// Opens by content, not name: every EPUB is a zip.
    pub fn open(bytes: Arc<Vec<u8>>) -> Result<Self, OpenError> {
        if !mareader_epub::sniff(&bytes) {
            return Ok(Self::Pdf(mareader_pdf::Document::open(bytes)?));
        }
        let book = Book::open(bytes)?;
        let (width, height) = book.page_size();
        let sizes = vec![PageSize { width, height }; book.page_count()];
        Ok(Self::Epub { book, sizes })
    }

    /// Whether the text reflows: the typography settings apply.
    pub fn is_reflowable(&self) -> bool {
        matches!(self, Self::Epub { .. })
    }

    pub fn page_count(&self) -> usize {
        self.page_sizes().len()
    }

    pub fn page_sizes(&self) -> &[PageSize] {
        match self {
            Self::Pdf(d) => d.page_sizes(),
            Self::Epub { sizes, .. } => sizes,
        }
    }

    /// Chapters in reading order; empty when the file has none.
    pub fn outline(&self) -> &[OutlineEntry] {
        match self {
            Self::Pdf(d) => d.outline(),
            Self::Epub { .. } => &[],
        }
    }

    /// The title as written; see `display_name` for what to show.
    pub fn raw_title(&self) -> Option<&str> {
        match self {
            Self::Pdf(d) => d.raw_title(),
            Self::Epub { book, .. } => book.title(),
        }
    }

    /// The scale `render` really uses: `scale`, lowered to fit the caps.
    pub fn fitted_scale(&self, page: usize, scale: f32) -> Option<f32> {
        Some(self.page_sizes().get(page)?.fit_scale(scale))
    }

    /// Draws page `page` (0-based) at `scale` pixels per point.
    pub fn render(&self, page: usize, scale: f32) -> Result<Raster, RenderError> {
        let book = match self {
            Self::Pdf(d) => return d.render(page, scale),
            Self::Epub { book, .. } => book,
        };
        let size = self.size(page)?;
        let scale = size.fit_scale(scale);
        // Truncated, as a PDF page's raster is.
        let w = ((size.width * scale) as u32).max(1);
        let h = ((size.height * scale) as u32).max(1);
        draw(book, page, scale, [0, 0, w, h])
    }

    /// Tile `col`, `row` of page `page` split at `scale`; see [`Tiling`].
    pub fn render_tile(
        &self,
        page: usize,
        scale: f32,
        col: u32,
        row: u32,
    ) -> Result<Raster, RenderError> {
        let book = match self {
            Self::Pdf(d) => return d.render_tile(page, scale, col, row),
            Self::Epub { book, .. } => book,
        };
        let tiling = Tiling::new(self.size(page)?, scale);
        let rect = tiling.rect(col, row).ok_or(RenderError::Failed)?;
        draw(book, page, tiling.scale, rect)
    }

    /// Page `page`'s characters and links; an EPUB's come later.
    pub fn text_layer(&self, page: usize) -> Result<TextLayer, RenderError> {
        match self {
            Self::Pdf(d) => d.text_layer(page),
            Self::Epub { .. } => self.size(page).map(|_| TextLayer::default()),
        }
    }

    /// Page `page`'s paper colour, read from a small drawing of it.
    pub fn paper(&self, page: usize) -> Result<Paper, RenderError> {
        if let Self::Pdf(d) = self {
            return d.paper(page);
        }
        let size = self.size(page)?;
        let scale = SAMPLE_SIDE / size.width.max(size.height);
        Ok(Paper::detect(&self.render(page, scale)?))
    }

    fn size(&self, page: usize) -> Result<PageSize, RenderError> {
        self.page_sizes()
            .get(page)
            .copied()
            .ok_or(RenderError::NoPage(page))
    }
}

fn draw(book: &Book, page: usize, scale: f32, region: [u32; 4]) -> Result<Raster, RenderError> {
    let px = book
        .render(page, scale, region)
        .ok_or(RenderError::Failed)?;
    Ok(Raster {
        width: px.width,
        height: px.height,
        pixels: px.rgba,
    })
}
