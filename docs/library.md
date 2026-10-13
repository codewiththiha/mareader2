# Library: design

> **Status:** design for M5 · **Last updated:** 2026-10-13

mareader's library is ~32k lines (`library-core` 10k, `library-runtime` 22k,
plus `src-tauri/src/commands/library.rs`), and its design notes
(`Mareader.md` § The library) run past 1,000 lines. This document keeps what it
achieves and drops the complexity its model created.

## What the library is for

Read from mareader's README § Library and `Mareader.md`. A user can:

1. **Gather books from anywhere.** Read a folder in place, or have the app
   keep its own copy. Loose files (dialog, drop) are kept as copies.
2. **Stay in sync with the disk.** Watched folders rescan on launch and when the
   window regains focus: new files appear, moved files are followed,
   vanished files are flagged missing, never silently dropped.
3. **Organise.** Nested shelves, drag to file (several books at once), a book
   on more than one shelf, a manual order or a sort.
4. **Find.** Search as you type over title, author and path, with substring and
   fuzzy matching and suggestions; grid or list, column count, sort, covers.
5. **Never lose their place.** Reading position and marks belong to the book
   and survive moves, renames and relinks.
6. **Never lose a file.** The app deletes only copies it made. Removing shows
   what goes first. A book removed from a watched folder can be restored. A
   missing book can be found again (pick the file, or a folder to search).
7. **See imports progress** without blocking the window.

## Where mareader's size comes from

| mareader mechanism | Why it was needed there | mareader2 |
|---|---|---|
| A watched folder's tree *is* a set of editable shelves ("rungs" on "seats") | Moving a read-in-place book off its rung had to turn it into a copy ("departure"), with families, returns, moved-out logs and a copy-cost sheet | **Sources are read-only.** A watched folder is browsed as the disk's own tree; shelves are separate, user-made collections. Filing a book never touches a source, so departures don't exist. |
| Names unique per level | Every drop, import and filing could collide: a conflict sheet with three answers per gesture | **Shelves hold book ids.** Two books may share a title; dropping a book on a shelf that has it is a no-op that highlights it. Only sibling *shelf* names must differ (rename refuses). |
| Rows vs books: link rows, twin rows, `independent` rows | One file could be several rows; every writer had to fold twins | **One content, one book.** A book has a list of locations. A book on two shelves is two memberships. |
| Fingerprint = size + mtime + 8 KiB head hash; placeholders gating rescans | mtime changes on copy; migrated rows had no measurement | **Content identity:** size + hash of the first and last 64 KiB. A `(path, size, mtime)` cache skips re-hashing unchanged files, so a focus rescan stays cheap. |
| Per-folder tombstones with names, addresses and shelves | To stop rescans re-adding removed books, and to offer them back | **`hidden` per source:** removing a source's book hides it there and keeps its record (with progress) for restore. |
| A custom pointer-drag engine, DOM probes for crumbs, virtual-list crates | Browser limits | egui `dnd_*`, `Galley` measurement, `ScrollArea::show_rows`. |

## Model (`crates/library`, L1)

Plain data plus pure operations. Filesystem access sits behind a small `Disk`
trait (walk, measure, copy, delete-in-store) so every rule is a unit test with a
fake disk.

```text
Library  { books: BTreeMap<BookId, Book>, shelves: BTreeMap<ShelfId, Shelf>,
           sources: BTreeMap<SourceId, Source> }
Book     { id, content: ContentId, title, author, format: Pdf | Epub,
           locations: Vec<Location>, added, opened, position, marks_key }
Location { path, origin: Source(SourceId) | Store, present: bool, stamp: (size, mtime) }
Shelf    { id, name, parent: Option<ShelfId>, books: Vec<BookId> }   // manual order
Source   { id, root, formats, depth, hidden: BTreeSet<BookId> }
```

**Operations** return a `Result` and a description of what changed (for the
UI's toast and for undo later):

- `add_source(root, formats)` and `scan(source, walk) -> ScanReport`
  (added, followed, missing, back). Pure: the walk is input.
- `import_files(paths)` copies into the store (`<data>/items/<id>/`), keyed by
  id so names never collide on disk.
- `file(books, shelf)`, `unfile`, `reorder`; `create_shelf`, `rename_shelf`,
  `move_shelf` (refuses cycles), `remove_shelf(keep_books | remove_books)`.
- `removal_receipt(books)` then `remove(books)`: hides source books, deletes
  store copies, drops memberships; marks and position go with the record
  unless it is only hidden.
- `relink(book, path)` after re-measuring the file; `restore(source, book)`.
- `record_position(book, position)`.
- Queries: `query(text, sort, scope) -> Vec<(BookId, Spans)>` (one rule for
  the filter and the suggestions), `children(shelf)`, `path_to(shelf)`.

**Walk rules kept from mareader:** filter while walking (formats, depth,
result cap), refuse symlinks, skip hidden directories, re-measure before any
relink, and delete only paths that canonicalise inside the store.

**Persistence:** one versioned serde file, written atomically (temp + rename)
on a worker after a short debounce. Covers live in the cache directory, keyed
by content id, rebuilt on demand.

## UI (`crates/ui`, M5)

- **Sections:** All books, Shelves (a tree), Sources (each folder's own tree,
  read-only), Missing, Recent.
- **Grid and list:** `ScrollArea::show_rows`, card size from the column
  setting; covers load lazily on a worker.
- **Selection:** click, Ctrl/Cmd-click, Shift-click, select all; a selection bar.
- **Drag:** `dnd_drag_source` with a `Vec<BookId>` payload; drop zones on shelf
  cards, breadcrumb crumbs, the shelf tree, and panes (open or split).
- **Menus:** `Response::context_menu` per kind (book, shelf, empty space).
- **Breadcrumb:** crumbs laid out with `Galley`; the oldest fold behind `…`
  when they don't fit.
- **Search:** a `TextEdit` with a suggestion `Popup`; arrows and Enter move and
  open, Escape clears.
- **Sheets:** removal receipt, find-again, import options as `Modal`s.
- **Import progress:** a corner card per running import.

## Optional: importing a mareader library (P2)

A one-time import of `mareader.library.v3` maps linked books to source
locations, stored books to store copies, shelves to shelves, and keeps
positions. Departed shelves become plain shelves; tombstones become `hidden`.

## Size target

`crates/library` under ~2.5k lines with its tests; the library UI under ~2.5k.
If it grows past that, the model is probably carrying a case it shouldn't.

## Dropped on purpose

- **Independent twins** (two reading states for one file). Rare; can return
  as "duplicate as a separate copy" if the owner wants it.
- **Shelves that mirror folders and are editable.** Replaced by read-only
  source trees plus user shelves.
