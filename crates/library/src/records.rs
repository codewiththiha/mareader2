//! The library as rows: what changed becomes rows, rows become a library.

use std::collections::HashMap;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::book::{Book, Location};
use crate::clock::Clock;
use crate::library::{Dirty, Library, Parts};
use crate::shelves::{Membership, Shelf};
use crate::sources::Source;

/// What a row holds. Synced kinds are shared; the rest stay on this machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Book,
    Shelf,
    Member,
    /// A book's paths here, newest first.
    Locations,
    /// A folder read in place.
    Source,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Book => "book",
            Self::Shelf => "shelf",
            Self::Member => "member",
            Self::Locations => "locations",
            Self::Source => "source",
        }
    }

    /// None for a kind from a newer Mareader.
    pub fn parse(name: &str) -> Option<Self> {
        let all = [
            Self::Book,
            Self::Shelf,
            Self::Member,
            Self::Locations,
            Self::Source,
        ];
        all.into_iter().find(|k| k.name() == name)
    }

    /// Shared by a reader's devices, once they sync.
    pub fn synced(self) -> bool {
        matches!(self, Self::Book | Self::Shelf | Self::Member)
    }
}

/// One stored row: a kind, an id within it, and JSON.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub kind: Kind,
    pub id: String,
    pub body: String,
}

/// One save's rows: written, deleted, and the clock that stamped them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Changes {
    pub put: Vec<Row>,
    /// Only local rows go; records end in tombstones instead.
    pub gone: Vec<(Kind, String)>,
    pub clock: Option<String>,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        self.put.is_empty() && self.gone.is_empty()
    }

    fn put(&mut self, kind: Kind, id: String, value: &impl Serialize) {
        // Library types always serialize; a failure would lose one row.
        if let Ok(body) = serde_json::to_string(value) {
            self.put.push(Row { kind, id, body });
        }
    }
}

/// A membership's id: its shelf and book.
fn member_id(m: &Membership) -> String {
    format!("{}/{}", m.shelf, m.book)
}

/// The rows `dirty` names, as the library now has them.
pub(crate) fn changes(library: &Library, dirty: &Dirty) -> Changes {
    let mut out = Changes::default();
    for book in dirty.books.iter().filter_map(|id| library.books.get(id)) {
        let id = book.id.to_string();
        let record = Book {
            locations: Vec::new(),
            ..book.clone()
        };
        out.put(Kind::Book, id.clone(), &record);
        match book.locations.is_empty() {
            true => out.gone.push((Kind::Locations, id)),
            false => out.put(Kind::Locations, id, &book.locations),
        }
    }
    for shelf in dirty
        .shelves
        .iter()
        .filter_map(|id| library.shelves.get(id))
    {
        out.put(Kind::Shelf, shelf.id.to_string(), shelf);
    }
    for member in dirty.members.iter().filter_map(|k| library.members.get(k)) {
        out.put(Kind::Member, member_id(member), member);
    }
    for id in &dirty.sources {
        match library.sources.get(id) {
            Some(source) => out.put(Kind::Source, id.to_string(), source),
            None => out.gone.push((Kind::Source, id.to_string())),
        }
    }
    if !out.is_empty() {
        out.clock = serde_json::to_string(&library.clock).ok();
    }
    out
}

/// A library from its rows, and how many rows couldn't be read.
pub(crate) fn assemble(rows: Vec<Row>, clock: Clock) -> (Parts, usize) {
    let mut parts = Parts::new(clock);
    let mut skipped = 0;
    let mut locations: HashMap<String, Vec<Location>> = HashMap::new();
    for row in rows {
        let read = match row.kind {
            Kind::Book => parse::<Book>(&row).map(|b| {
                parts.books.insert(b.id, b);
            }),
            Kind::Shelf => parse::<Shelf>(&row).map(|s| {
                parts.shelves.insert(s.id, s);
            }),
            Kind::Member => parse::<Membership>(&row).map(|m| {
                parts.members.insert((m.shelf, m.book), m);
            }),
            Kind::Source => parse::<Source>(&row).map(|s| {
                parts.sources.insert(s.id, s);
            }),
            Kind::Locations => parse::<Vec<Location>>(&row).map(|l| {
                locations.insert(row.id.clone(), l);
            }),
        };
        skipped += usize::from(read.is_none());
    }
    for book in parts.books.values_mut() {
        if let Some(found) = locations.remove(&book.id.to_string()) {
            book.locations = found;
        }
    }
    (parts, skipped)
}

fn parse<T: DeserializeOwned>(row: &Row) -> Option<T> {
    serde_json::from_str(&row.body).ok()
}
