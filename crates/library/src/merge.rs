//! Joining two copies of one record, as a sync will; order never matters.

use crate::book::{Book, State};
use crate::shelves::{Membership, Shelf};

pub trait Merge {
    /// Takes in `other`, a copy of the same record from elsewhere.
    fn merge(&mut self, other: Self);
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
        self.added = self.added.min(other.added);
        self.removed = self.removed.max(other.removed);
        self.restored = self.restored.max(other.restored);
        self.twin_of = self.twin_of.max(other.twin_of);
        // Locations are this machine's own; another's mean nothing here.
    }
}

impl Merge for Shelf {
    fn merge(&mut self, other: Self) {
        self.name.merge(other.name);
        self.parent.merge(other.parent);
        self.added = self.added.min(other.added);
        self.removed = self.removed.max(other.removed);
        self.restored = self.restored.max(other.restored);
    }
}

impl Merge for Membership {
    fn merge(&mut self, other: Self) {
        self.key.merge(other.key);
        self.alive.merge(other.alive);
    }
}

impl Merge for State {
    fn merge(&mut self, other: Self) {
        self.position.merge(other.position);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::{Format, Position};
    use crate::clock::{Field, Stamp};
    use crate::id::{BookId, ContentId, DeviceId, ShelfId};

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

    fn book(rng: &mut Rng) -> Book {
        let (title, author, opened) = (rng.stamp(), rng.stamp(), rng.stamp());
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
            locations: Vec::new(),
            removed: rng.maybe(),
            restored: rng.maybe(),
            twin_of: (rng.next(2) == 0).then(|| BookId(u128::from(rng.next(3)))),
        }
    }

    fn shelf(rng: &mut Rng) -> Shelf {
        let (name, parent) = (rng.stamp(), rng.stamp());
        Shelf {
            id: ShelfId(1),
            name: Field::new(word(name), name),
            parent: Field::new(Some(ShelfId(u128::from(parent.ms))), parent),
            added: rng.next(5),
            removed: rng.maybe(),
            restored: rng.maybe(),
        }
    }

    fn member(rng: &mut Rng) -> Membership {
        let (key, alive) = (rng.stamp(), rng.stamp());
        Membership {
            shelf: ShelfId(1),
            book: BookId(1),
            key: Field::new(word(key), key),
            alive: Field::new(alive.ms % 2 == 0, alive),
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
