//! Either format behind one set of calls, so views never ask which.
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use mareader_epub::{Book, Destination, Format, Inserts, MAX_PICTURE_FILE};
use mareader_pdf::TextLayer;
use mareader_pdf::{Bounds, Link, LinkTarget, MAX_DEPTH, Tiling, web_address};
use mareader_pdf::{OutlineEntry, PageSize, PageText, Paper, Raster, RenderError};
use mareader_reflow::{Anchor, Picture, Typography};

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
        // Fixed pages keep their own shape; reflowed ones share one.
        let size = |page| {
            let (width, height) = book.size(page);
            PageSize { width, height }
        };
        let sizes = (0..book.page_count()).map(size).collect();
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

    /// A book's sound or video at `path`, read to hand to a player.
    pub fn media(&self, path: &str) -> Option<mareader_epub::Media> {
        match self {
            Self::Pdf(_) => None,
            Self::Epub { book, .. } => book.media(path),
        }
    }

    /// The subtitle files of a book's clip at `path`, for its player.
    pub fn subtitles(&self, path: &str) -> Vec<mareader_epub::Subtitle> {
        match self {
            Self::Pdf(_) => Vec::new(),
            Self::Epub { book, .. } => book.subtitles(path),
        }
    }

    /// What a book's trigger link `link` sets off.
    pub fn trigger(&self, link: usize) -> Vec<mareader_epub::Triggered> {
        match self {
            Self::Pdf(_) => Vec::new(),
            Self::Epub { book, .. } => book.trigger(link),
        }
    }

    /// A book's scripted chapters, for a script engine to run.
    pub fn scripts(&self) -> Vec<mareader_epub::Scripted> {
        match self {
            Self::Pdf(_) => Vec::new(),
            Self::Epub { book, .. } => book.scripts(),
        }
    }

    /// A book's narration, if its chapters have media overlays.
    pub fn narration(&self) -> Option<Arc<mareader_epub::Narration>> {
        match self {
            Self::Pdf(_) => None,
            Self::Epub { book, .. } => {
                let n = book.narration();
                (!n.phrases.is_empty()).then(|| Arc::clone(n))
            }
        }
    }

    /// A book's files, to read its narration's sound from.
    pub fn files(&self) -> Option<mareader_epub::Files> {
        match self {
            Self::Pdf(_) => None,
            Self::Epub { book, .. } => book.files(),
        }
    }

    /// At rest: a book lets go of layouts and decodes out of view.
    pub fn rest(&self) {
        if let Self::Epub { book, .. } = self {
            book.rest();
        }
    }

    /// The same book in other type; none for a PDF or on failure.
    pub fn retypeset(&self, typo: &Typography) -> Option<Self> {
        self.reset(typo, None)
    }

    /// The book in `typo` with `inserts` laid in; none keeps its own.
    pub fn reset(&self, typo: &Typography, inserts: Option<&Arc<Inserts>>) -> Option<Self> {
        match self {
            Self::Pdf(_) => None,
            Self::Epub { book, .. } => book.reset(typo, inserts).map(Self::epub),
        }
    }

    /// The results a book was asked to show; zero for none, or a PDF.
    pub fn inserts_key(&self) -> u64 {
        match self {
            Self::Pdf(_) => 0,
            Self::Epub { book, .. } => book.inserts_key(),
        }
    }

    /// Names a book's own layout, which results laid in keep.
    pub fn layout(&self) -> Option<u64> {
        match self {
            Self::Pdf(_) => None,
            Self::Epub { book, .. } => Some(book.layout()),
        }
    }

    /// Whether result `id` is laid in a book's pages.
    pub fn shows(&self, id: u128) -> bool {
        matches!(self, Self::Epub { book, .. } if book.shows(id))
    }

    /// The own page page `page` stands for; a PDF's pages are its own.
    pub fn to_base(&self, page: usize) -> usize {
        match self {
            Self::Pdf(_) => page,
            Self::Epub { book, .. } => book.to_base(page),
        }
    }

    /// The page showing own page `page`.
    pub fn from_base(&self, page: usize) -> usize {
        match self {
            Self::Pdf(_) => page,
            Self::Epub { book, .. } => book.from_base(page),
        }
    }

    /// The own pages page `page` stands for; a result's later ones, none.
    pub fn base_pages(&self, page: usize) -> std::ops::Range<usize> {
        match self {
            Self::Pdf(_) => page..page + 1,
            Self::Epub { book, .. } => book.base_pages(page),
        }
    }

    /// How many own pages: a book's results aside.
    pub fn base_count(&self) -> usize {
        match self {
            Self::Pdf(d) => d.page_count(),
            Self::Epub { book, .. } => book.base_count(),
        }
    }

    /// The same book, perhaps set in other type; a PDF only as itself.
    pub fn same_book(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Epub { book, .. }, Self::Epub { book: o, .. }) => book.same_book(o),
            _ => std::ptr::eq(self, other),
        }
    }

    /// The page of `other` drawn just like page `page`, if any.
    pub fn same_page(&self, page: usize, other: &Self) -> Option<usize> {
        match (self, other) {
            (Self::Epub { book, .. }, Self::Epub { book: o, .. }) => book.same_page(page, o),
            _ => None,
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

    /// Whether its pages turn right to left, as a book may ask.
    pub fn right_to_left(&self) -> bool {
        matches!(self, Self::Epub { book, .. } if book.right_to_left())
    }

    /// How many pages show alone before spreads pair them, as a book asks.
    pub fn lead(&self) -> usize {
        match self {
            Self::Epub { book, .. } => book.lead(),
            Self::Pdf(_) => 0,
        }
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
                Destination::Note { page, text } => LinkTarget::Note { page, text },
                // The same schemes a PDF's links may use.
                Destination::Outside(a) => LinkTarget::Web(web_address(a.as_bytes())?),
                Destination::Media { path, video } => LinkTarget::Media { path, video },
                Destination::Transcript { card, track } => LinkTarget::Transcript { card, track },
                Destination::Trigger(link) => LinkTarget::Trigger {
                    link,
                    what: mareader_epub::Triggered::label(&book.trigger(link)),
                },
                Destination::Script { chapter, node } => LinkTarget::Script { chapter, node },
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
        let tables = table_runs(&lines);
        let lines = lines.into_iter().map(|l| {
            let chars = l.chars.into_iter().map(|c| (c.ch, bounds(c.rect)));
            (l.wraps, chars.collect())
        });
        let mut text = PageText::from_lines(lines);
        if book.turned(page) {
            text.set_vertical();
        }
        places.truncate(text.len());
        Ok(TextLayer {
            text,
            links: links.collect(),
            places,
            figures: Vec::new(),
            tables,
        })
    }

    /// Page `page`'s pictures, spared the ink dial; a PDF lists none.
    pub fn pictures(&self, page: usize) -> Vec<Bounds> {
        match self {
            Self::Pdf(_) => Vec::new(),
            Self::Epub { book, .. } => book.pictures(page).into_iter().map(bounds).collect(),
        }
    }

    /// Box `at` of a PDF's page drawn large; a book's pictures are its own.
    pub fn render_region(&self, page: usize, at: Bounds, side: u32) -> Result<Raster, RenderError> {
        match self {
            Self::Pdf(d) => d.render_region(page, at, side),
            Self::Epub { .. } => Err(RenderError::Failed),
        }
    }

    /// A book's picture at a point of page `page`; a PDF has none.
    pub fn picture_at(&self, page: usize, (x, y): (f32, f32)) -> Option<Arc<Picture>> {
        match self {
            Self::Pdf(_) => None,
            Self::Epub { book, .. } => book.picture_at(page, x, y),
        }
    }

    /// Page `page`'s colour glyphs, emoji mostly; a PDF lists none.
    pub fn emojis(&self, page: usize) -> Vec<Bounds> {
        match self {
            Self::Pdf(_) => Vec::new(),
            Self::Epub { book, .. } => book.emojis(page).into_iter().map(bounds).collect(),
        }
    }

    /// Page `page`'s paper colour; a book's is the theme's, so it has none.
    pub fn paper(&self, page: usize) -> Result<Paper, RenderError> {
        if let Self::Pdf(d) = self {
            return d.paper(page);
        }
        self.size(page)?;
        Ok(Paper::default())
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

/// A small enough picture a note names, else a copy's stand-in for it.
pub(crate) fn read_picture(folder: &Path, path: &str) -> Option<Vec<u8>> {
    let read = |path: PathBuf| {
        let len = std::fs::metadata(&path).ok()?.len();
        let fits = usize::try_from(len).is_ok_and(|len| len <= MAX_PICTURE_FILE);
        fits.then(|| std::fs::read(&path).ok()).flatten()
    };
    read(folder.join(path)).or_else(|| read(folder.join(stand_in(path)?)))
}

/// Where a copied note keeps `path`: `../a.png` as `^/a.png`.
pub(crate) fn stand_in(path: &str) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for part in Path::new(path).components() {
        match part {
            Component::Normal(n) => out.push(n),
            Component::CurDir => {}
            Component::ParentDir => out.push("^"),
            Component::RootDir => out.push("^root"),
            Component::Prefix(p) => {
                let drive = p.as_os_str().to_string_lossy();
                let safe: String = drive.chars().filter(char::is_ascii_alphanumeric).collect();
                out.push(format!("^{safe}"));
            }
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

/// Runs of characters in table cells, by table block, in text order.
fn table_runs(lines: &[mareader_reflow::PageLine]) -> Vec<(std::ops::Range<usize>, u32)> {
    let mut runs: Vec<(std::ops::Range<usize>, u32)> = Vec::new();
    let mut at = 0;
    for line in lines {
        let end = at + line.chars.len();
        let block = line.table.and_then(|b| u32::try_from(b).ok());
        match (block, runs.last_mut()) {
            (Some(b), Some((run, last))) if *last == b && run.end == at => run.end = end,
            (Some(b), _) => runs.push((at..end, b)),
            (None, _) => {}
        }
        at = end;
    }
    runs
}
