//! The gate before a folder walk: its asks, folds and a tree's new shape.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::book::Origin;
use crate::clock::Field;
use crate::conflict::{Placement, collide_shelf, next_name};
use crate::folder::{Options, dir_of_rung, key_in_zone, parent_key, rel_under, root_name};
use crate::ground;
use crate::id::{BookId, ShelfId, SourceId};
use crate::library::Library;
use crate::link::LinkTo;
use crate::removal::Data;
use crate::run::{Fold, Plan, Planned};
use crate::shelves::ShelfKind;
use crate::sources::Found;

/// What a folder pick does first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Gate {
    /// Walk this root with this plan.
    Walk { root: PathBuf, plan: Plan },
    /// A top-level shelf holds the folder's name: the reader answers.
    Clash(Clash),
}

/// The folder's own question: its name is taken at the top level.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clash {
    pub root: PathBuf,
    pub name: String,
    pub existing: ShelfId,
    /// The shelf is this folder's own, from an earlier import.
    pub own: bool,
    pub in_place: bool,
}

impl Clash {
    /// Read in place it links or merges; copied it opens, replaces or adds.
    pub fn offers(&self) -> &'static [Placement] {
        match self.in_place {
            true => Placement::SHELF_IN_PLACE,
            false => Placement::SHELF_STORED,
        }
    }
}

/// What an answer to a [`Clash`] leaves to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answered {
    /// Nothing walks: the shelf is shown, or a link stands for it.
    Done(Option<ShelfId>),
    /// The walk goes ahead with this plan.
    Walk { root: PathBuf, plan: Plan },
}

impl Library {
    /// A pick inside a tree read in place continues it; a name clash asks.
    pub fn gate(&self, root: &Path, opts: &Options) -> Gate {
        let folders = self.folder_list();
        if opts.in_place {
            let covered = ground::covering(&self.shelves, &folders, root);
            if let Some((cover, tree)) =
                covered.and_then(|c| Some((c.clone(), self.folder(c.folder)?)))
            {
                let fold = ground::family(&self.shelves, &folders, &tree.root);
                let plan = Plan {
                    continuation: Some(cover.shelf),
                    fold: fold.map(|(tree, rel)| Fold { tree, rel }),
                    rung: cover.rel,
                    ..Plan::default()
                };
                let root = tree.root.clone();
                return Gate::Walk { root, plan };
            }
            // A rung removed or never cut: the pick wants it back in its tree.
            if let Some((tree, rel)) = ground::family(&self.shelves, &folders, root) {
                let plan = Plan {
                    fold: Some(Fold { tree, rel }),
                    ..Plan::default()
                };
                let root = root.to_path_buf();
                return Gate::Walk { root, plan };
            }
        }
        let name = root_name(root);
        let tops = self.children(None);
        let tops = tops.iter().map(|s| (s.id, s.name.value.as_str()));
        let Some(existing) = collide_shelf(tops, &name) else {
            let root = root.to_path_buf();
            return Gate::Walk {
                root,
                plan: Plan::default(),
            };
        };
        let own = (self.folders.values())
            .any(|f| f.root == root && f.shelf_map.get("") == Some(&existing));
        Gate::Clash(Clash {
            root: root.to_path_buf(),
            name,
            existing,
            own,
            in_place: opts.in_place,
        })
    }

    /// Writes what an answer settles before the walk; an odd one is no-op.
    pub fn answer_clash(&mut self, clash: &Clash, answer: Placement, wall_ms: u64) -> Answered {
        if !clash.offers().contains(&answer) || self.shelf(clash.existing).is_none() {
            return Answered::Done(None);
        }
        let root = clash.root.clone();
        let beside = !clash.in_place
            && self
                .folder_at(&root, true)
                .is_some_and(|id| self.folder(id).is_some_and(|f| f.mode().in_place()));
        match answer {
            Placement::Open => Answered::Done(Some(clash.existing)),
            Placement::LinkOnly => {
                self.link_to(LinkTo::Shelf(clash.existing), None, wall_ms);
                Answered::Done(Some(clash.existing))
            }
            Placement::Merge => Answered::Walk {
                root,
                plan: Plan {
                    into: Some(clash.existing),
                    ..Plan::default()
                },
            },
            Placement::KeepBoth => {
                let tops = self.children(None);
                let name = next_name(tops.iter().map(|s| s.name.value.as_str()), &clash.name);
                let plan = Plan {
                    rename: Some(name),
                    beside,
                    ..Plan::default()
                };
                Answered::Walk { root, plan }
            }
            Placement::Replace => self.replace_shelf(clash, beside, wall_ms),
        }
    }

    /// The shelf's books leave, data kept; the folder's copies take it.
    fn replace_shelf(&mut self, clash: &Clash, beside: bool, wall_ms: u64) -> Answered {
        let root = clash.root.clone();
        let tree = self.folder_at(&root, true);
        let tree = tree
            .and_then(|id| self.folder(id))
            .filter(|f| f.mode().in_place());
        // The folder's own tree read in place becomes copies, data kept.
        if let (true, Some(tree)) = (clash.own, tree) {
            let placed = tree.placed.clone();
            let rows: Vec<BookId> = (self.books())
                .filter(|b| placed.contains(&b.content))
                .filter(|b| {
                    b.home()
                        .is_some_and(|l| matches!(l.origin, Origin::Source(_)))
                })
                .map(|b| b.id)
                .collect();
            self.remove_books(&rows, Data::Keep, wall_ms);
            return Answered::Walk {
                root,
                plan: Plan::default(),
            };
        }
        let doomed: Vec<BookId> = (self.shelf_books(clash.existing).iter())
            .map(|b| b.id)
            .collect();
        self.remove_books(&doomed, Data::Keep, wall_ms);
        let plan = Plan {
            into: Some(clash.existing),
            beside,
            ..Plan::default()
        };
        Answered::Walk { root, plan }
    }

    /// The shallowest tree read in place inside this walk, made on its own.
    pub(crate) fn displaced_member(&self, id: SourceId, found: &[Found]) -> Option<Fold> {
        let tree = self.folder(id).filter(|f| f.mode().in_place())?;
        let members = (self.folders.values())
            .filter(|f| f.id != id && f.mode().in_place())
            .filter_map(|other| {
                let rel = rel_under(&other.root, &tree.root).filter(|r| !r.is_empty())?;
                found.iter().find(|f| f.path.starts_with(&other.root))?;
                let shelf = other.shelf_map.get("").and_then(|s| self.shelf(*s))?;
                let inside = (self.path_to(shelf.id).iter())
                    .any(|s| s.kind.value.rung().is_some_and(|(f, _)| f == id));
                (!inside).then_some(Fold {
                    tree: other.id,
                    rel,
                })
            });
        members.min_by_key(|m| m.rel.matches('/').count())
    }

    /// The run's fold: its own pick's, else a member it walked over.
    pub(crate) fn fold_run(&mut self, planned: &Planned, wall_ms: u64) -> Option<ShelfId> {
        let groups = self.folder(planned.folder)?.opts.groups;
        if let Some(Fold { tree, rel }) = &planned.plan.fold {
            let seat = self.fold_into(*tree, planned.folder, rel, wall_ms)?;
            if self.folder(*tree)?.shape_at(rel) != groups {
                return self.reshape(*tree, rel, groups, wall_ms).or(Some(seat));
            }
            return Some(seat);
        }
        let Fold { tree: member, rel } = planned.member.clone()?;
        self.fold_into(planned.folder, member, &rel, wall_ms);
        None
    }

    /// A folder's rungs join `tree` at `rel`; its ledger joins, it goes.
    pub(crate) fn fold_into(
        &mut self,
        tree: SourceId,
        gone: SourceId,
        rel: &str,
        wall_ms: u64,
    ) -> Option<ShelfId> {
        let mut into = self.folders.get(&tree)?.clone();
        let member = self.folders.get(&gone)?.clone();
        let rungs = self.rungs_of(gone);
        let at = self.clock.tick(wall_ms);
        let seat = if into.cuts(rel) {
            if let Some(parent) = parent_key(rel) {
                self.rung_chain(&mut into, parent, None, false, wall_ms);
            }
            for (key, id) in &rungs {
                let key = match key.is_empty() {
                    true => rel.to_owned(),
                    false => format!("{rel}/{key}"),
                };
                into.shelf_map.insert(key.clone(), *id);
                if let Some(shelf) = self.shelves.get_mut(id) {
                    shelf.kind = Field::new(ShelfKind::Rung { folder: tree, key }, at);
                    shelf.moved = Field::new(false, at);
                }
            }
            into.set_tracking(rel, member.opts.watch);
            let own = rungs
                .iter()
                .find(|(key, _)| key.is_empty())
                .map(|(_, id)| *id);
            own.or_else(|| into.shelf_map.get(rel).copied())
        } else {
            let seat = self.rung_chain(&mut into, "", None, false, wall_ms);
            for (_, id) in rungs.iter().filter(|(_, id)| *id != seat) {
                self.dissolve(*id, seat, wall_ms);
            }
            Some(seat)
        };
        into.placed.extend(member.placed.iter().copied());
        for stone in &member.ignored {
            if !into.is_ignored(stone.content) {
                into.ignored.push(stone.clone());
            }
        }
        for seen in &member.last_seen {
            if !into.last_seen.contains(seen) {
                into.last_seen.push(seen.clone());
            }
        }
        self.adopt_rows(gone, tree);
        self.folders.remove(&gone);
        self.folders.insert(tree, into);
        self.rehang(tree, wall_ms);
        self.bump();
        seat
    }

    /// Books read in place under one folder now read under another.
    fn adopt_rows(&mut self, from: SourceId, to: SourceId) {
        let ids: Vec<BookId> = (self.books.values())
            .filter(|b| b.locations.iter().any(|l| l.origin == Origin::Source(from)))
            .map(|b| b.id)
            .collect();
        for id in ids {
            let Some(book) = self.books.get_mut(&id) else {
                continue;
            };
            for location in &mut book.locations {
                if location.origin == Origin::Source(from) {
                    location.origin = Origin::Source(to);
                }
            }
        }
        self.index();
    }

    /// The standing shelves a folder's rungs stand on, by key.
    pub(crate) fn rungs_of(&self, folder: SourceId) -> Vec<(String, ShelfId)> {
        (self.shelves.values())
            .filter(|s| s.alive())
            .filter_map(|s| {
                let (f, key) = s.kind.value.rung()?;
                (f == folder).then(|| (key.to_owned(), s.id))
            })
            .collect()
    }

    /// A shelf goes; its books and shelves move onto `home`.
    fn dissolve(&mut self, id: ShelfId, home: ShelfId, wall_ms: u64) {
        let books: Vec<BookId> = self.on(id).map(|(_, b)| b.id).collect();
        let inner: Vec<ShelfId> = self.children(Some(id)).iter().map(|s| s.id).collect();
        let at = self.clock.tick(wall_ms);
        for child in inner {
            let key = self.next_key(Some(home));
            if let Some(shelf) = self.shelves.get_mut(&child) {
                shelf.from = shelf.parent.value;
                shelf.parent = Field::new(Some(home), at);
                shelf.key = Field::new(key, at);
            }
        }
        for book in books {
            self.unfile(book, id, wall_ms);
            self.file(book, home, wall_ms);
        }
        if let Some(shelf) = self.shelves.get_mut(&id) {
            shelf.removed = Some(at);
        }
        self.bump();
    }

    /// Re-files a tree's books under `rung` by the new shape; the seat.
    pub(crate) fn reshape(
        &mut self,
        id: SourceId,
        rung: &str,
        groups: bool,
        wall_ms: u64,
    ) -> Option<ShelfId> {
        let mut folder = self.folders.get(&id)?.clone();
        folder.set_shape(rung, groups);
        let own = self.rungs_of(id);
        let root_stands = (folder.shelf_map.get("")).is_some_and(|s| self.shelf(*s).is_some());
        if own.is_empty() || !root_stands {
            self.folders.insert(id, folder);
            return None;
        }
        let ground = dir_of_rung(&folder.root, rung);
        // Rungs no shape cuts any more: their books come home.
        let hurt: HashSet<ShelfId> = (own.iter())
            .filter(|(key, _)| key_in_zone(key, rung) && !folder.cuts(key))
            .map(|(_, s)| *s)
            .collect();
        folder.shelf_map.retain(|_, s| !hurt.contains(s));
        let home_key = folder.rung_for(rung);
        let home = self.rung_chain(&mut folder, &home_key, None, false, wall_ms);
        let mut moves = Vec::new();
        for (_, shelf) in &own {
            for book in self.shelf_books(*shelf) {
                let path = book.path().filter(|p| p.starts_with(&ground));
                let key = path.and_then(|p| folder.rung_of(p));
                let doomed = hurt.contains(shelf);
                if key.is_none() && !doomed {
                    continue;
                }
                moves.push((book.id, *shelf, key));
            }
        }
        for (book, from, key) in moves {
            let seat = match key {
                Some(key) => self.rung_chain(&mut folder, &key, None, false, wall_ms),
                None => home,
            };
            if seat != from {
                self.unfile(book, from, wall_ms);
                self.file(book, seat, wall_ms);
            }
        }
        for shelf in hurt {
            self.dissolve(shelf, home, wall_ms);
        }
        self.folders.insert(id, folder);
        self.rehang(id, wall_ms);
        Some(home)
    }
}
