//! Results laid into a book in place of the text they came from.
use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use mareader_reflow::{Anchor, Band, Block, BookFace, Content, Flow, Fonts, PageLine, clip};

/// A result to lay where `from..to` of the book's text was.
#[derive(Debug, Clone, PartialEq)]
pub struct Insert {
    /// Tells one result from another.
    pub id: u128,
    pub from: Anchor,
    /// Where its text ends, not included.
    pub to: Anchor,
    pub blocks: Arc<Vec<Block>>,
    /// The result's language tag, for hyphenation.
    pub language: Option<String>,
}

/// The results a book shows; `key` tells one set from another.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Inserts {
    /// Zero for none.
    pub key: u64,
    pub list: Vec<Arc<Insert>>,
}

static LAYOUTS: AtomicU64 = AtomicU64::new(1);

/// A fresh name for a layout of a book's own pages.
pub(crate) fn next_layout() -> u64 {
    LAYOUTS.fetch_add(1, Ordering::Relaxed)
}

/// Where a region's block came from.
#[derive(Debug, Clone, PartialEq)]
enum Origin {
    /// Bytes, or rows, of a block of the book.
    Book {
        block: usize,
        range: Range<usize>,
    },
    Result,
}

/// Results and the book's text around them, replacing `base` pages.
pub(crate) struct Region {
    pub(crate) base: Range<usize>,
    pub(crate) flow: Flow,
    origins: Vec<Origin>,
    /// What it lays in; a region is kept only for the same results.
    made: Vec<Arc<Insert>>,
    /// The side of the spread its first page sits on.
    side: usize,
}

/// A page: one of the book's own, or one of a region's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum At {
    Base(usize),
    Region(usize, usize),
}

/// A book's pages with results laid in.
pub(crate) struct Spliced {
    pub(crate) regions: Vec<Arc<Region>>,
    pub(crate) pages: Vec<At>,
    /// Each own page's page here; a replaced one, its region's first.
    pub(crate) from_base: Vec<usize>,
    /// Each region's first page here.
    starts: Vec<usize>,
    pub(crate) bands: Vec<Band>,
}

impl Spliced {
    /// `inserts` laid into `base`, reusing `old`'s regions; none if none fit.
    pub(crate) fn build(
        base: &Flow,
        blocks: &[Block],
        inserts: &Inserts,
        (old, faces): (Option<&Spliced>, &[BookFace]),
    ) -> Option<Self> {
        let count = base.page_count();
        let groups = groups(base, blocks, inserts);
        if groups.is_empty() {
            return None;
        }
        let mut out = Self {
            regions: Vec::new(),
            pages: Vec::new(),
            from_base: Vec::with_capacity(count),
            starts: Vec::new(),
            bands: Vec::new(),
        };
        let mut next = 0;
        for (group, range) in groups {
            out.own(base, next..range.start);
            let first = out.pages.len();
            let same = |r: &&Arc<Region>| {
                r.base == range && r.side == first % 2 && same_results(&r.made, &group)
            };
            let kept = old.and_then(|o| o.regions.iter().find(same)).cloned();
            let region = kept.unwrap_or_else(|| {
                let group = (group.as_slice(), range.clone());
                Arc::new(Region::lay(base, blocks, group, (first, faces)))
            });
            out.from_base.extend(range.clone().map(|_| first));
            let n = out.regions.len();
            out.pages
                .extend((0..region.flow.page_count()).map(|i| At::Region(n, i)));
            out.bands.extend_from_slice(region.flow.bands());
            out.starts.push(first);
            out.regions.push(region);
            next = range.end;
        }
        out.own(base, next..count);
        Some(out)
    }

    /// The book's own `pages`, as they are.
    fn own(&mut self, base: &Flow, pages: Range<usize>) {
        let bands = base.bands();
        for p in pages {
            self.from_base.push(self.pages.len());
            self.pages.push(At::Base(p));
            self.bands.extend(bands.get(p).copied());
        }
    }

    pub(crate) fn at(&self, page: usize) -> Option<At> {
        self.pages.get(page).copied()
    }

    pub(crate) fn region(&self, r: usize) -> Option<&Arc<Region>> {
        self.regions.get(r)
    }

    pub(crate) fn start(&self, r: usize) -> Option<usize> {
        self.starts.get(r).copied()
    }

    /// The page here showing region `region`, wherever it sits.
    pub(crate) fn find(&self, region: &Arc<Region>) -> Option<usize> {
        let r = self.regions.iter().position(|o| Arc::ptr_eq(o, region))?;
        self.start(r)
    }

    pub(crate) fn shows(&self, id: u128) -> bool {
        self.regions
            .iter()
            .any(|r| r.made.iter().any(|i| i.id == id))
    }
}

/// Whether `was` and `is` lay the same results, by id and content.
fn same_results(was: &[Arc<Insert>], is: &[&Arc<Insert>]) -> bool {
    let one = |(a, b): (&Arc<Insert>, &&Arc<Insert>)| Arc::ptr_eq(a, b) || **a == ***b;
    was.len() == is.len() && was.iter().zip(is).all(one)
}

/// The inserts that can be laid, in order, grouped by pages they share.
fn groups<'a>(
    base: &Flow,
    blocks: &[Block],
    inserts: &'a Inserts,
) -> Vec<(Vec<&'a Arc<Insert>>, Range<usize>)> {
    let mut list: Vec<&Arc<Insert>> = (inserts.list.iter())
        .filter(|i| i.from < i.to && i.to.block <= blocks.len())
        .collect();
    list.sort_by_key(|i| (i.from, i.id));
    let mut groups: Vec<(Vec<&Arc<Insert>>, Range<usize>)> = Vec::new();
    let mut end: Option<Anchor> = None;
    for ins in list {
        // Two results of the same text: the first is laid.
        if end.is_some_and(|e| ins.from < e) {
            continue;
        }
        end = Some(ins.to);
        let first = base.locate(ins.from);
        let last = Anchor {
            block: ins.to.block,
            offset: ins.to.offset.saturating_sub(1),
        };
        let pages = first..base.locate(last).max(first) + 1;
        match groups.last_mut() {
            Some((group, range)) if pages.start < range.end => {
                group.push(ins);
                range.end = range.end.max(pages.end);
            }
            _ => groups.push((vec![ins], pages)),
        }
    }
    groups
}

impl Region {
    /// The text of `pages` with each result in place of its own.
    fn lay(
        base: &Flow,
        blocks: &[Block],
        (group, pages): (&[&Arc<Insert>], Range<usize>),
        (first, faces): (usize, &[BookFace]),
    ) -> Self {
        let start = base.anchor(pages.start).unwrap_or_default();
        let end = match base.anchor(pages.end) {
            Some(a) if pages.end < base.page_count() => a,
            _ => Anchor {
                block: blocks.len(),
                offset: 0,
            },
        };
        let mut out = (Vec::new(), Vec::new());
        let mut at = start;
        for ins in group {
            slice(blocks, at..ins.from, &mut out);
            for b in ins.blocks.iter() {
                out.0.push(b.clone());
                out.1.push(Origin::Result);
            }
            at = ins.to;
        }
        slice(blocks, at..end, &mut out);
        let language = group.iter().find_map(|i| i.language.as_deref());
        let (laid, origins) = out;
        let fonts = Fonts::system().with_faces(faces);
        let flow = base.region(Arc::new(laid), language, fonts, (first, pages.len()));
        Self {
            base: pages,
            flow,
            origins,
            made: group.iter().map(|i| Arc::clone(i)).collect(),
            side: first % 2,
        }
    }

    /// Where region block `block`'s `at` is in the book; none in a result.
    fn place(&self, (block, at): (usize, usize)) -> Option<(usize, usize)> {
        match self.origins.get(block)? {
            Origin::Book { block, range } => Some((*block, range.start + at)),
            Origin::Result => None,
        }
    }

    /// Page `page`'s lines, placed in the book where they came from.
    pub(crate) fn text(&self, page: usize) -> Vec<PageLine> {
        let mut lines = self.flow.text(page);
        for c in lines.iter_mut().flat_map(|l| l.chars.iter_mut()) {
            c.at = c.at.and_then(|at| self.place(at));
        }
        lines
    }

    /// Page `page`'s book text, by block; results have none.
    pub(crate) fn excerpts(&self, page: usize) -> Vec<(usize, Range<usize>)> {
        let excerpts = self.flow.excerpts(page).into_iter();
        excerpts
            .filter_map(|(block, r)| match self.origins.get(block)? {
                Origin::Book { block, range } => {
                    let to = match r.end {
                        usize::MAX => range.end,
                        end => (range.start + end).min(range.end),
                    };
                    Some((*block, range.start + r.start..to))
                }
                Origin::Result => None,
            })
            .collect()
    }
}

/// How far a block's places run: bytes, rows, or one for the rest.
fn extent(block: &Block) -> usize {
    match &block.content {
        Content::Text(t) => t.text.len(),
        Content::Table(t) => t.rows.len(),
        _ => 1,
    }
}

/// The book's blocks from `span.start` up to `span.end`, cut to fit.
fn slice(blocks: &[Block], span: Range<Anchor>, out: &mut (Vec<Block>, Vec<Origin>)) {
    let (a, b) = (span.start, span.end);
    if a >= b {
        return;
    }
    for index in a.block..=b.block {
        let Some(block) = blocks.get(index).filter(|b| !b.is_blank()) else {
            continue;
        };
        let len = extent(block);
        let start = if index == a.block { a.offset } else { 0 };
        let end = if index == b.block { b.offset } else { len };
        let mut range = start.min(len)..end.min(len);
        if let Content::Text(t) = &block.content {
            range = clip(&t.text, range);
        }
        if range.is_empty() {
            continue;
        }
        let mut piece = block.cut(range.clone());
        // A block begun afresh keeps its chapter's page break.
        piece.break_before = range.start == 0 && block.break_before;
        if piece.is_blank() {
            continue;
        }
        out.0.push(piece);
        out.1.push(Origin::Book {
            block: index,
            range,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mareader_reflow::{Role, Typography};

    fn para(n: usize, words: usize) -> Block {
        let text = vec![format!("w{n}"); words].join(" ");
        Block::text(Role::Paragraph, &text)
    }

    fn book() -> (Vec<Block>, Flow) {
        let blocks: Vec<Block> = (0..40).map(|n| para(n, 120)).collect();
        let typo = Typography::default();
        let flow = Flow::new(Arc::new(blocks.clone()), &typo, Fonts::bundled());
        (blocks, flow)
    }

    fn insert(id: u128, from: Anchor, to: Anchor, words: usize) -> Arc<Insert> {
        let text = vec!["result"; words].join(" ");
        Arc::new(Insert {
            id,
            from,
            to,
            blocks: Arc::new(vec![Block::text(Role::Paragraph, &text)]),
            language: None,
        })
    }

    fn page_span(flow: &Flow, page: usize) -> (Anchor, Anchor) {
        let from = flow.anchor(page).unwrap_or_default();
        let to = flow.anchor(page + 1).unwrap_or_default();
        (from, to)
    }

    type Out = Result<(), &'static str>;

    #[test]
    fn a_long_result_takes_more_pages_and_the_rest_stay() -> Out {
        let (blocks, flow) = book();
        let (from, to) = page_span(&flow, 3);
        let inserts = Inserts {
            key: 1,
            list: vec![insert(7, from, to, 900)],
        };
        let s = Spliced::build(&flow, &blocks, &inserts, (None, &[])).ok_or("laid")?;
        let extra = s.pages.len() - flow.page_count();
        assert!(extra >= 1, "{extra}");
        // Pages before and after are the book's own, in order.
        assert_eq!(s.at(2), Some(At::Base(2)));
        assert_eq!(s.at(3), Some(At::Region(0, 0)));
        assert_eq!(s.at(4 + extra), Some(At::Base(4)));
        assert_eq!(s.from_base.get(3), Some(&3));
        assert_eq!(s.from_base.get(4), Some(&(4 + extra)));
        assert_eq!(s.bands.len(), s.pages.len());
        assert!(s.shows(7) && !s.shows(8));
        Ok(())
    }

    #[test]
    fn a_result_from_another_layout_keeps_the_text_around_it() -> Out {
        let (blocks, flow) = book();
        // Mid-page to mid-page: the cut text either side stays.
        let from = Anchor {
            block: 10,
            offset: 40,
        };
        let to = Anchor {
            block: 11,
            offset: 50,
        };
        let inserts = Inserts {
            key: 1,
            list: vec![insert(1, from, to, 20)],
        };
        let s = Spliced::build(&flow, &blocks, &inserts, (None, &[])).ok_or("laid")?;
        let region = s.region(0).ok_or("a region")?;
        let all: Vec<_> = (0..region.flow.page_count())
            .flat_map(|p| region.excerpts(p))
            .collect();
        // The replaced bytes are gone; the rest keep their places.
        assert!(all.iter().any(|(b, r)| *b == 10 && r.end == 40), "{all:?}");
        assert!(
            all.iter().any(|(b, r)| *b == 11 && r.start == 50),
            "{all:?}"
        );
        let inside = |(b, r): &(usize, Range<usize>)| *b == 10 && r.end > 40;
        assert!(!all.iter().any(inside), "{all:?}");
        let placed = (0..region.flow.page_count()).flat_map(|p| region.text(p));
        let result = placed.flat_map(|l| l.chars).filter(|c| c.at.is_none());
        assert!(result.count() > 0, "the result's text has no place");
        Ok(())
    }

    #[test]
    fn overlapping_results_lay_the_first_and_shared_pages_group() -> Out {
        let (blocks, flow) = book();
        let (a, b) = page_span(&flow, 5);
        let (_, c) = page_span(&flow, 6);
        let inserts = Inserts {
            key: 1,
            list: vec![insert(1, a, c, 30), insert(2, b, c, 30)],
        };
        let s = Spliced::build(&flow, &blocks, &inserts, (None, &[])).ok_or("laid")?;
        assert!(s.shows(1) && !s.shows(2));
        assert_eq!(s.regions.len(), 1);
        let none = Inserts {
            key: 2,
            list: vec![insert(3, c, a, 30)],
        };
        assert!(Spliced::build(&flow, &blocks, &none, (None, &[])).is_none());
        Ok(())
    }

    #[test]
    fn an_unchanged_region_is_shared_by_the_next_set() -> Out {
        let (blocks, flow) = book();
        let (a, b) = page_span(&flow, 2);
        let (c, d) = page_span(&flow, 9);
        let one = Inserts {
            key: 1,
            list: vec![insert(1, a, b, 50)],
        };
        let old = Spliced::build(&flow, &blocks, &one, (None, &[])).ok_or("laid")?;
        let two = Inserts {
            key: 2,
            list: vec![insert(1, a, b, 50), insert(2, c, d, 50)],
        };
        let new = Spliced::build(&flow, &blocks, &two, (Some(&old), &[])).ok_or("laid")?;
        let (was, is) = (old.region(0).ok_or("old")?, new.region(0).ok_or("new")?);
        assert!(Arc::ptr_eq(was, is));
        assert_eq!(new.find(was), Some(2));
        Ok(())
    }
}
