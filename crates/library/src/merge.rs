//! Joining two copies of one record, as a sync will; order never matters.

use crate::book::{Book, State};
use crate::clock::{NEVER, Stamp};
use crate::marks::Mark;
use crate::shelves::{Membership, Shelf};

pub trait Merge {
    /// Takes in `other`, a copy of the same record from elsewhere.
    fn merge(&mut self, other: Self);

    /// Its latest stamp, which this device's clock must then pass.
    fn latest(&self) -> Stamp;
}

/// The largest of `stamps`; never written if there are none.
fn newest(stamps: impl IntoIterator<Item = Option<Stamp>>) -> Stamp {
    stamps.into_iter().flatten().max().unwrap_or(NEVER)
}

impl Merge for Book {
    fn merge(&mut self, other: Self) {
        // A file edited in place: the newer open saw the newer content.
        let theirs = (other.opened.at, other.content) > (self.opened.at, self.content);
        if theirs {
            self.content = other.content;
            self.format = other.format;
        }
        self.title.merge(other.title);
        self.author.merge(other.author);
        self.opened.merge(other.opened);
        self.key.merge(other.key);
        self.added = self.added.min(other.added);
        self.removed = self.removed.max(other.removed);
        self.restored = self.restored.max(other.restored);
        self.twin_of.merge(other.twin_of);
        self.extra.merge(other.extra);
        // Locations are this machine's own; another's mean nothing here.
    }

    fn latest(&self) -> Stamp {
        newest([
            Some(self.title.at),
            Some(self.author.at),
            Some(self.opened.at),
            Some(self.key.at),
            Some(self.twin_of.at),
            self.removed,
            self.restored,
            self.extra.latest(),
        ])
    }
}

impl Merge for Shelf {
    fn merge(&mut self, other: Self) {
        self.name.merge(other.name);
        // Where it came from goes with the move that won.
        let (mine, theirs) = (self.parent.at, other.parent.at);
        if theirs > mine || (theirs == mine && other.from > self.from) {
            self.from = other.from;
        }
        self.parent.merge(other.parent);
        self.key.merge(other.key);
        self.added = self.added.min(other.added);
        self.removed = self.removed.max(other.removed);
        self.restored = self.restored.max(other.restored);
        self.extra.merge(other.extra);
    }

    fn latest(&self) -> Stamp {
        newest([
            Some(self.name.at),
            Some(self.parent.at),
            Some(self.key.at),
            self.removed,
            self.restored,
            self.extra.latest(),
        ])
    }
}

impl Merge for Membership {
    fn merge(&mut self, other: Self) {
        self.key.merge(other.key);
        self.alive.merge(other.alive);
        self.extra.merge(other.extra);
    }

    fn latest(&self) -> Stamp {
        newest([Some(self.key.at), Some(self.alive.at), self.extra.latest()])
    }
}

impl Merge for Mark {
    fn merge(&mut self, other: Self) {
        self.book.merge(other.book);
        self.alive.merge(other.alive);
        self.gloss.merge(other.gloss);
        self.extra.merge(other.extra);
    }

    fn latest(&self) -> Stamp {
        let fields = [self.book.at, self.alive.at, self.gloss.at].map(Some);
        newest(fields.into_iter().chain([self.extra.latest()]))
    }
}

impl Merge for State {
    fn merge(&mut self, other: Self) {
        self.position.merge(other.position);
        self.extra.merge(other.extra);
    }

    fn latest(&self) -> Stamp {
        newest([Some(self.position.at), self.extra.latest()])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::{Format, Position};
    use crate::clock::{Field, Stamp};
    use crate::extra::Extra;
    use crate::id::{BookId, ContentId, DeviceId, MarkId, ShelfId};
    use crate::marks::{Gloss, MarkKind, Pen, Spot};

    /// A small deterministic generator: the same cases every run.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self, below: u64) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0 % below
        }

        /// One write: its value follows from its stamp, as in real use.
        fn stamp(&mut self) -> Stamp {
            Stamp {
                ms: self.next(4),
                counter: u32::try_from(self.next(3)).unwrap_or(0),
                device: DeviceId(self.next(3)),
            }
        }

        fn maybe(&mut self) -> Option<Stamp> {
            (self.next(2) == 0).then(|| self.stamp())
        }
    }

    fn word(at: Stamp) -> String {
        format!("{}-{}-{}", at.ms, at.counter, at.device.0)
    }

    /// A newer build's fields: stamped, bare, or none at all.
    fn extra(rng: &mut Rng) -> Extra {
        let mut out = Extra::default();
        for name in ["rating", "colour"] {
            let value = match rng.next(3) {
                0 => continue,
                1 => serde_json::json!(rng.next(3)),
                _ => {
                    let at = rng.stamp();
                    serde_json::json!({ "v": word(at), "t": String::from(at) })
                }
            };
            out.insert(name, value);
        }
        out
    }

    fn book(rng: &mut Rng) -> Book {
        let (title, author, opened) = (rng.stamp(), rng.stamp(), rng.stamp());
        let key = rng.stamp();
        Book {
            id: BookId(1),
            content: ContentId(u128::from(opened.ms)),
            format: Format::ALL
                .get(usize::try_from(opened.counter).unwrap_or(0))
                .copied()
                .unwrap_or(Format::Pdf),
            title: Field::new(word(title), title),
            author: Field::new(Some(word(author)), author),
            added: rng.next(5),
            opened: Field::new(opened.ms, opened),
            key: Field::new(word(key), key),
            locations: Vec::new(),
            removed: rng.maybe(),
            restored: rng.maybe(),
            twin_of: {
                let at = rng.stamp();
                Field::new((at.counter > 0).then_some(BookId(u128::from(at.ms))), at)
            },
            extra: extra(rng),
        }
    }

    fn shelf(rng: &mut Rng) -> Shelf {
        let (name, parent, key) = (rng.stamp(), rng.stamp(), rng.stamp());
        Shelf {
            id: ShelfId(1),
            name: Field::new(word(name), name),
            parent: Field::new(Some(ShelfId(u128::from(parent.ms))), parent),
            from: Some(ShelfId(u128::from(parent.counter))),
            added: rng.next(5),
            key: Field::new(word(key), key),
            removed: rng.maybe(),
            restored: rng.maybe(),
            extra: extra(rng),
        }
    }

    fn member(rng: &mut Rng) -> Membership {
        let (key, alive) = (rng.stamp(), rng.stamp());
        Membership {
            shelf: ShelfId(1),
            book: BookId(1),
            key: Field::new(word(key), key),
            alive: Field::new(alive.ms % 2 == 0, alive),
            extra: extra(rng),
        }
    }

    fn mark(rng: &mut Rng) -> Mark {
        let (book, alive, said) = (rng.stamp(), rng.stamp(), rng.stamp());
        let gloss = (said.ms % 3 != 0).then(|| Gloss {
            meaning: format!("said at {}", said.ms),
            ..Gloss::default()
        });
        Mark {
            id: MarkId(1),
            book: Field::new(BookId(u128::from(book.ms)), book),
            kind: MarkKind::Explain,
            pen: Pen::Full,
            color: None,
            word: "fox".into(),
            context: "the quick brown fox".into(),
            spot: Spot::Text {
                block: 2,
                start: 16,
                end: 19,
            },
            added: 5,
            alive: Field::new(alive.ms % 2 == 0, alive),
            gloss: Field::new(gloss, said),
            extra: extra(rng),
        }
    }

    fn state(rng: &mut Rng) -> State {
        let at = rng.stamp();
        let page = usize::try_from(at.ms).unwrap_or(0);
        let position = Position {
            page,
            pages: 10,
            place: None,
        };
        State {
            position: Field::new(position, at),
            extra: extra(rng),
        }
    }

    fn joined<T: Merge + Clone>(mut a: T, b: &T) -> T {
        a.merge(b.clone());
        a
    }

    /// Any order and any repeat of merges comes to the same record.
    fn laws<T: Merge + Clone + PartialEq + std::fmt::Debug>(make: fn(&mut Rng) -> T) {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        for _ in 0..2_000 {
            let (a, b, c) = (make(&mut rng), make(&mut rng), make(&mut rng));
            assert_eq!(joined(a.clone(), &b), joined(b.clone(), &a), "commutes");
            let left = joined(joined(a.clone(), &b), &c);
            let right = joined(a.clone(), &joined(b.clone(), &c));
            assert_eq!(left, right, "associates");
            assert_eq!(joined(a.clone(), &a), a, "idempotent");
        }
    }

    #[test]
    fn books_merge_in_any_order() {
        laws(book);
    }

    #[test]
    fn shelves_merge_in_any_order() {
        laws(shelf);
    }

    #[test]
    fn memberships_merge_in_any_order() {
        laws(member);
    }

    #[test]
    fn marks_merge_in_any_order() {
        laws(mark);
    }

    #[test]
    fn positions_merge_in_any_order() {
        laws(state);
    }

    #[test]
    fn a_removal_outlasts_an_older_restore() {
        let mut rng = Rng(7);
        let mut kept = book(&mut rng);
        let early = Stamp {
            ms: 1,
            counter: 0,
            device: DeviceId(1),
        };
        let late = Stamp { ms: 9, ..early };
        kept.removed = None;
        kept.restored = Some(early);
        let mut removed = kept.clone();
        removed.removed = Some(late);
        kept.merge(removed);
        assert!(!kept.alive());
    }
}
