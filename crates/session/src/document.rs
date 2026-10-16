//! Either format behind one set of calls, so views never ask which.
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mareader_epub::{Book, Destination, Format, MAX_PICTURE_FILE};
use mareader_pdf::TextLayer;
use mareader_pdf::{Bounds, Link, LinkTarget, MAX_DEPTH, Tiling, web_address};
use mareader_pdf::{OutlineEntry, PageSize, PageText, Paper, Raster, RenderError, SAMPLE_SIDE};
use mareader_reflow::{Anchor, Typography};

use crate::OpenError;

/// A text file converted to an EPUB before it opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TextFile {
    pub format: Format,
    /// Its name, which titles a plain text file.
    pub name: String,
    /// The folder its Markdown pictures are read from.
    pub folder: Option<PathBuf>,
}

/// An open document. An EPUB's layout unit shows as one point.
pub enum Document {
    Pdf(mareader_pdf::Document),
    Epub {
        book: Book,
        sizes: Vec<PageSize>,
        /// The contents on this setting's pages.
        outline: Vec<OutlineEntry>,
    },
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
    /// Opens by content, not name: every EPUB is a zip, set in `typo`.
    pub fn open(bytes: Arc<Vec<u8>>, typo: &Typography) -> Result<Self, OpenError> {
        if !mareader_epub::sniff(&bytes) {
            return Ok(Self::Pdf(mareader_pdf::Document::open(bytes)?));
        }
        Ok(Self::epub(Book::open(bytes, typo)?))
    }

    /// Converts Markdown or plain text to an EPUB, then opens that.
    pub(crate) fn convert(
        bytes: &[u8],
        file: &TextFile,
        typo: &Typography,
    ) -> Result<Self, OpenError> {
        let folder = file.folder.as_deref();
        let read = |path: &str| folder.and_then(|f| read_picture(f, path));
        let epub = mareader_epub::convert(bytes, file.format, Some(&file.name), &read)?;
        Ok(Self::epub(Book::open(Arc::new(epub), typo)?))
    }

    fn epub(book: Book) -> Self {
        let (width, height) = book.page_size();
        let sizes = vec![PageSize { width, height }; book.page_count()];
        let outline = book
            .toc()
            .iter()
            .map(|e| OutlineEntry {
                title: e.title.clone(),
                page: book.locate(e.at),
                depth: u8::try_from(e.depth).unwrap_or(u8::MAX).min(MAX_DEPTH),
            })
            .collect();
        Self::Epub {
            book,
            sizes,
            outline,
        }
    }

    /// The same book in other type; none for a PDF or on failure.
    pub fn retypeset(&self, typo: &Typography) -> Option<Self> {
        match self {
            Self::Pdf(_) => None,
            Self::Epub { book, .. } => book.retypeset(typo).map(Self::epub),
        }
    }

    /// Where a reflowable page starts, to find again in other type.
    pub fn anchor(&self, page: usize) -> Option<Anchor> {
        match self {
            Self::Pdf(_) => None,
            Self::Epub { book, .. } => book.anchor(page),
        }
    }

    /// The page showing `anchor`; a PDF has none.
    pub fn locate(&self, anchor: Anchor) -> Option<usize> {
        match self {
            Self::Pdf(_) => None,
            Self::Epub { book, .. } => Some(book.locate(anchor)),
        }
    }

    /// What of each page a continuous stream shows; a PDF streams whole
    /// pages.
    pub fn bands(&self) -> Option<&[mareader_reflow::Band]> {
        match self {
            Self::Pdf(_) => None,
            Self::Epub { book, .. } => Some(book.bands()),
        }
    }

    /// A PDF that opened without its end: pages may be missing.
    pub fn cut_short(&self) -> bool {
        matches!(self, Self::Pdf(d) if d.cut_short())
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
            Self::Epub { outline, .. } => outline,
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

    /// Page `page`'s characters and links.
    pub fn text_layer(&self, page: usize) -> Result<TextLayer, RenderError> {
        let book = match self {
            Self::Pdf(d) => return d.text_layer(page),
            Self::Epub { book, .. } => book,
        };
        self.size(page)?;
        let links = book.links(page).into_iter().filter_map(|l| {
            let target = match l.to {
                Destination::Page(page) => LinkTarget::Page(page),
                // The same schemes a PDF's links may use.
                Destination::Outside(a) => LinkTarget::Web(web_address(a.as_bytes())?),
            };
            Some(Link {
                bounds: bounds(l.rect),
                target,
            })
        });
        let lines = book.text(page);
        let narrow =
            |(b, i): (usize, usize)| Some((u32::try_from(b).ok()?, u32::try_from(i).ok()?));
        let mut places: Vec<_> = lines
            .iter()
            .flat_map(|l| l.chars.iter().map(|c| c.at.and_then(narrow)))
            .collect();
        let lines = lines.into_iter().map(|l| {
            let chars = l.chars.into_iter().map(|c| (c.ch, bounds(c.rect)));
            (l.wraps, chars.collect())
        });
        let text = PageText::from_lines(lines);
        places.truncate(text.len());
        Ok(TextLayer {
            text,
            links: links.collect(),
            places,
        })
    }

    /// Page `page`'s pictures, spared the ink dial; a PDF lists none.
    pub fn pictures(&self, page: usize) -> Vec<Bounds> {
        match self {
            Self::Pdf(_) => Vec::new(),
            Self::Epub { book, .. } => book.pictures(page).into_iter().map(bounds).collect(),
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

/// Left, top, width, height as a box.
fn bounds([x, y, w, h]: [f32; 4]) -> Bounds {
    Bounds {
        x0: x,
        y0: y,
        x1: x + w,
        y1: y + h,
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

/// A picture beside a Markdown file, if it is small enough to read.
pub(crate) fn read_picture(folder: &Path, path: &str) -> Option<Vec<u8>> {
    let path = folder.join(path);
    let len = std::fs::metadata(&path).ok()?.len();
    let fits = usize::try_from(len).is_ok_and(|len| len <= MAX_PICTURE_FILE);
    fits.then(|| std::fs::read(&path).ok()).flatten()
}
