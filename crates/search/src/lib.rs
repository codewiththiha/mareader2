//! Finding text in a document: folding, pages made searchable, hits.

mod fold;
mod page;
mod walk;

use std::ops::Range;

pub use fold::{fold, invisible, spell};
pub use page::{Glyph, Needle, Page, SNIPPET_RADIUS};
pub use walk::{from_page, reveal, step};

/// Hits kept for one query; past this the count says "or more".
pub const MAX_HITS: usize = 10_000;

/// One place a query turns up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub page: usize,
    /// Its characters in the page's text layer.
    pub chars: Range<usize>,
    pub snippet: String,
}

/// `needle`'s hits on page `number`, at most `room` of them.
pub fn hits(number: usize, page: &Page, needle: &Needle, room: usize) -> Vec<Hit> {
    let found = page.find(needle).into_iter().take(room);
    let hit = |chars: Range<usize>| {
        Some(Hit {
            page: number,
            chars: page.layer_range(chars.clone())?,
            snippet: page.snippet(chars),
        })
    };
    found.filter_map(hit).collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn hits_carry_their_page_layer_range_and_snippet() {
        let glyphs = "A fox, a Fox".chars().map(|ch| Glyph {
            ch,
            line: 0,
            space_before: false,
        });
        let page = Page::new(glyphs);
        let needle = Needle::new("fox").unwrap();
        let all = hits(4, &page, &needle, 9);
        assert_eq!(all.len(), 2);
        assert_eq!((all[0].page, all[0].chars.clone()), (4, 2..5));
        assert_eq!(all[1].snippet, "A fox, a Fox");
        assert_eq!(hits(4, &page, &needle, 1).len(), 1, "room caps them");
    }
}
