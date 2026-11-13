//! The library's search: its field, what the page lists, and the card a choice
//! lights.
//!
//! The bar draws the field and the suggestions; this is what they read and what
//! a choice leaves behind, so a listing survives the frames that show it.

use std::rc::Rc;

use mareader_library::picks::Pick;
use mareader_library::{Home, Item, Query, ShelfId, SortKey, Suggestion};

use super::Scope;

/// The field's text, its query, and what the suggestions list shows.
#[derive(Default)]
pub struct Search {
    pub text: String,
    pub query: Query,
    /// Suggestions list under the field while it's focused.
    pub open: bool,
    pub lit: Option<usize>,
    pub reveal: Option<Reveal>,
    pub listing: Option<(Shape, Rc<Listing>)>,
    pub suggestions: Option<(u64, Query, Rc<[Suggestion]>)>,
}

impl Search {
    pub fn set_text(&mut self, text: String) {
        self.query = Query::new(&text);
        self.text = text;
        self.open = !self.query.is_empty();
        // The first row is lit, so Enter goes to the best match.
        self.lit = self.open.then_some(0);
    }

    pub fn close(&mut self) {
        self.open = false;
        self.lit = None;
    }

    /// Empties the field; true if it held anything.
    pub fn clear(&mut self) -> bool {
        let had = !self.text.is_empty();
        self.set_text(String::new());
        had
    }
}

/// A card shown by choosing it: ringed for a while, scrolled to once.
#[derive(Clone, Copy, Debug)]
pub struct Reveal {
    pub pick: Pick,
    pub since: f64,
    pub scroll: bool,
}

/// What the page lists, kept until the library or the page changes.
#[derive(Debug, Default)]
pub struct Listing {
    /// Shelves and books in the order shown.
    pub items: Vec<Item>,
    /// Books before the search: the 12 in "3 of 12 books".
    pub total: usize,
    /// Matches hidden by the shelf or filter, when none show.
    pub elsewhere: usize,
}

/// Everything a listing depends on.
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub revision: u64,
    pub at: Option<ShelfId>,
    pub scope: Scope,
    pub sort: SortKey,
    pub ascending: bool,
    pub home: Home,
    pub query: Query,
}

/// What the bar's field shows; the bar edits a copy.
#[derive(Clone, Debug, Default)]
pub struct Field {
    pub text: String,
    pub placeholder: String,
    pub open: bool,
    pub lit: Option<usize>,
}

/// The card being revealed: what it is, and how long ago it was chosen.
#[derive(Clone, Copy, Debug)]
pub struct Lit {
    pub pick: Pick,
    pub age: f64,
}
