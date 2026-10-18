//! Links: pointer rows at a book or a shelf, shown on another level.

use serde::{Deserialize, Serialize};

use crate::clock::{Field, Stamp};
use crate::extra::Extra;
use crate::id::{BookId, LinkId, ShelfId};

/// What a link points at; it is never opened, only followed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", tag = "is", content = "id")]
pub enum LinkTo {
    Book(BookId),
    Shelf(ShelfId),
}

/// A pointer row, named when it was made; it goes with its target.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Link {
    pub id: LinkId,
    pub name: Field<String>,
    pub target: LinkTo,
    /// The shelf it stands on; `None` is the top level.
    pub level: Field<Option<ShelfId>>,
    /// Its place among the rows beside it.
    pub key: Field<String>,
    pub added: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removed: Option<Stamp>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Link {
    /// A removed link never comes back: a new one is made instead.
    pub fn alive(&self) -> bool {
        self.removed.is_none()
    }
}
