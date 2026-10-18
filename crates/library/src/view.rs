//! How the library page lays out and orders its books.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use crate::arrange::Item;
use crate::book::Book;
use crate::library::Library;
use crate::link::LinkTo;

/// The fewest and most grid columns a reader may pin.
pub const COLUMNS_MIN: u8 = 2;
pub const COLUMNS_MAX: u8 = 10;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    #[default]
    Grid,
    List,
}

/// How a cover meets the card's A4 box.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CoverFit {
    /// Whole, at its own shape.
    #[default]
    Fit,
    /// Filling the box, trimmed about its centre.
    Crop,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortKey {
    /// The reader's own order, shelves and books mixed; set by dragging.
    #[default]
    #[serde(alias = "manual")]
    Arranged,
    LastRead,
    Title,
    Author,
    Added,
}

impl SortKey {
    pub const ALL: [Self; 5] = [
        Self::Arranged,
        Self::LastRead,
        Self::Title,
        Self::Author,
        Self::Added,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::LastRead => "Last read",
            Self::Title => "Title",
            Self::Author => "Author",
            Self::Added => "Date added",
            Self::Arranged => "Arranged",
        }
    }

    /// Names first from A, dates newest first.
    pub fn natural_ascending(self) -> bool {
        matches!(self, Self::Title | Self::Author | Self::Arranged)
    }

    /// What each direction reads as, ascending first; Arranged has one.
    pub fn directions(self) -> Option<[&'static str; 2]> {
        match self {
            Self::Title | Self::Author => Some(["A–Z", "Z–A"]),
            Self::LastRead | Self::Added => Some(["Oldest first", "Newest first"]),
            Self::Arranged => None,
        }
    }
}

/// What the library's top level shows besides its shelves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Home {
    /// Every book, shelved or not.
    #[default]
    Everything,
    /// Only books on no shelf.
    Unshelved,
}

/// The page's layout and order; kept with the settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct View {
    pub layout: Layout,
    /// Pinned grid columns; none fits as many as the width allows.
    pub columns: Option<u8>,
    pub cover: CoverFit,
    pub sort: SortKey,
    pub ascending: bool,
    pub home: Home,
}

impl Default for View {
    fn default() -> Self {
        Self {
            layout: Layout::Grid,
            columns: None,
            cover: CoverFit::Fit,
            sort: SortKey::Arranged,
            ascending: true,
            home: Home::Everything,
        }
    }
}

impl View {
    pub fn sanitize(&mut self) {
        self.columns = self.columns.map(|n| n.clamp(COLUMNS_MIN, COLUMNS_MAX));
        self.ascending |= self.sort == SortKey::Arranged;
    }

    /// A new key starts in its natural direction; the same key flips.
    pub fn set_sort(&mut self, key: SortKey) {
        if self.sort == key {
            // Arranged has no other way round.
            self.ascending = !self.ascending || key == SortKey::Arranged;
        } else {
            self.sort = key;
            self.ascending = key.natural_ascending();
        }
    }

    /// `books` in this view's order.
    pub fn sort<'a>(&self, books: impl Iterator<Item = &'a Book>) -> Vec<&'a Book> {
        let mut books: Vec<&Book> = books.collect();
        books.sort_by(|a, b| self.compare(a, b));
        books
    }

    /// Books and links in view order; a link sorts as its target.
    pub fn sort_items(&self, model: &Library, items: &mut [Item]) {
        let book = |i: &Item| match *i {
            Item::Book(id) => model.book(id),
            Item::Link(id) => match model.link(id).map(|l| l.target) {
                Some(LinkTo::Book(b)) => model.book(b),
                _ => None,
            },
            Item::Shelf(_) => None,
        };
        let name = |i: &Item| model.item_name(*i).unwrap_or_default();
        items.sort_by(|a, b| match (book(a), book(b)) {
            (Some(x), Some(y)) => self.compare(x, y).then(a.cmp(b)),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => natural(name(a), name(b)).then(a.cmp(b)),
        });
    }

    fn compare(&self, a: &Book, b: &Book) -> Ordering {
        let direct = |o: Ordering| if self.ascending { o } else { o.reverse() };
        let first = match self.sort {
            SortKey::LastRead => direct(a.opened.value.cmp(&b.opened.value)),
            SortKey::Added | SortKey::Arranged => direct(a.added.cmp(&b.added)),
            SortKey::Title => direct(natural(&a.title.value, &b.title.value)),
            // No author sorts last whichever way the rest run.
            SortKey::Author => match (author(a), author(b)) {
                (Some(x), Some(y)) => direct(natural(x, y)),
                (x, y) => x.is_none().cmp(&y.is_none()),
            },
        };
        first
            .then_with(|| natural(&a.title.value, &b.title.value))
            .then_with(|| a.id.cmp(&b.id))
    }
}

fn author(book: &Book) -> Option<&str> {
    book.author
        .value
        .as_deref()
        .map(str::trim)
        .filter(|a| !a.is_empty())
}

/// Text ignoring case, with digit runs read as numbers: 2 before 10.
pub fn natural(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.trim(), b.trim());
    loop {
        let mut ca = a.chars();
        let mut cb = b.chars();
        let (x, y) = match (ca.next(), cb.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => (x, y),
        };
        if x.is_ascii_digit() && y.is_ascii_digit() {
            let (na, ra) = digits(a);
            let (nb, rb) = digits(b);
            let (na, nb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
            let o = na.len().cmp(&nb.len()).then_with(|| na.cmp(nb));
            if o != Ordering::Equal {
                return o;
            }
            (a, b) = (ra, rb);
            continue;
        }
        let o = x.to_lowercase().cmp(y.to_lowercase());
        if o != Ordering::Equal {
            return o;
        }
        (a, b) = (ca.as_str(), cb.as_str());
    }
}

/// The leading run of ASCII digits, and the rest.
fn digits(s: &str) -> (&str, &str) {
    let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    s.split_at_checked(end).unwrap_or((s, ""))
}
