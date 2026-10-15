//! How the library page lays out and orders its books.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use crate::book::Book;

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
    #[default]
    LastRead,
    Title,
    Author,
    Added,
    /// A shelf's own order, set by dragging; elsewhere, date added.
    Manual,
}

impl SortKey {
    pub const ALL: [Self; 5] = [
        Self::LastRead,
        Self::Title,
        Self::Author,
        Self::Added,
        Self::Manual,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::LastRead => "Last read",
            Self::Title => "Title",
            Self::Author => "Author",
            Self::Added => "Date added",
            Self::Manual => "Manual",
        }
    }

    /// Names first from A, dates newest first.
    pub fn natural_ascending(self) -> bool {
        matches!(self, Self::Title | Self::Author | Self::Manual)
    }

    /// What each direction reads as, ascending first.
    pub fn directions(self) -> [&'static str; 2] {
        match self {
            Self::Title | Self::Author => ["A–Z", "Z–A"],
            Self::LastRead | Self::Added => ["Oldest first", "Newest first"],
            Self::Manual => ["First to last", "Last to first"],
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
            sort: SortKey::LastRead,
            ascending: SortKey::LastRead.natural_ascending(),
            home: Home::Everything,
        }
    }
}

impl View {
    pub fn sanitize(&mut self) {
        self.columns = self.columns.map(|n| n.clamp(COLUMNS_MIN, COLUMNS_MAX));
    }

    /// A new key starts in its natural direction; the same key flips.
    pub fn set_sort(&mut self, key: SortKey) {
        if self.sort == key {
            self.ascending = !self.ascending;
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

    fn compare(&self, a: &Book, b: &Book) -> Ordering {
        let direct = |o: Ordering| if self.ascending { o } else { o.reverse() };
        let first = match self.sort {
            SortKey::LastRead => direct(a.opened.value.cmp(&b.opened.value)),
            SortKey::Added | SortKey::Manual => direct(a.added.cmp(&b.added)),
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
