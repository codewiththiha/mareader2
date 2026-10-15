# Library: design

> **Status:** design for M5 · **Last updated:** 2026-10-15

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
| Rows vs books: link rows, twin rows, `independent` rows | One file could be several rows; every writer had to fold twins | **One content, one book, unless the reader asks for a twin.** A book has a list of locations; a book on two shelves is two memberships. A twin is a second book with its own id, and reading state is keyed by id, so nothing folds (§ Independent twins). |
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
           locations: Vec<Location>, added, opened, position, marks_key,
           twin_of: Option<BookId> }
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

### Independent twins

Kept from mareader (owner decision, 2026-10-13): two books of one file, each
with its own name, place, highlights and shelves. Read one to page 90 and the
other stays where it was; removing one takes nothing from the other.

- **Making one.** "Read separately" on a book, or "Add as a separate book" when
  an import finds content the library already has. Both call
  `twin(book) -> BookId`.
- **What is its own:** id, title (the counter name, "Moby Dick (2)"),
  position, marks, memberships. All of these are keyed by `BookId`, so a twin
  needs no special case in any writer.
- **What it shares:** the file's facts. Locations, `present`, relinks and the
  cover belong to the content, so `scan` and `relink` update every book at
  that location.
- **Opening by path** (drop, file manager, recent) opens the original, the
  book with `twin_of: None`.
- **Removing** drops only that book's record. A store copy is deleted only
  when no other book still lists the location.

**Walk rules kept from mareader:** filter while walking (formats, depth,
result cap), refuse symlinks, skip hidden directories, re-measure before any
relink, and delete only paths that canonicalise inside the store.

**Persistence:** one versioned serde file, written atomically (temp + rename)
on a worker after a short debounce. Covers live in the cache directory, keyed
by content id, rebuilt on demand.

## Identity and naming edge cases (2026-10-15)

- **Text is hashed whole.** Head and tail suffice for PDF and EPUB, whose
  tails (the xref, the zip's directory with each entry's CRC) change with
  any edit. A same-length typo fix in the middle of a Markdown note
  changes neither end, so `.txt` and `.md` (64 MiB at most) hash
  entirely.
- **Metadata never decides identity.** Generic PDF titles ("Microsoft Word
  - Document1", "PowerPoint Presentation", "untitled") would merge
  unrelated files; only content does.
- **Display title.** The document's title, unless it is empty, generic or
  just the file name; then the file name, without a copy mark: `book
  (1).pdf` shows as `book`.
- **A source file edited in place** (a Markdown note, a re-saved PDF) is
  the same book with new content: it keeps its id, position, marks and
  shelves. A source path is the user's own file. Store copies never
  change.
- **A newer version imported** (a re-downloaded or re-exported book) is new
  content. When its title, author and format match a book, the import
  sheet offers "Replace": the book keeps its id, shelves and marks, and its
  position clamps to the new length. One book, last import wins.
- **Hash races.** Two imports of one content in a single batch, or a scan
  and a drop at once, meet in one lookup by content id inside the
  operation, so they make one book.

## Text files in the library (2026-10-15)

Markdown and plain text open by converting to an EPUB on the session's
workers (`docs/epub.md` § Text files as EPUBs). Conversion is cheap and
byte-for-byte repeatable, so the library keeps the source and treats the
EPUB as a cache:

- **What is kept.** A store copy is the source file. Exception: a copied
  Markdown file with local pictures is kept as its converted EPUB, which
  packs them, since the copy would lose the pictures beside the original.
- **The converted cache** lives in the cache directory, keyed by content id
  and converter version, and is rebuilt on a miss.
- **Positions survive a newer converter.** A converter that finds more
  headings regroups blocks, so a block anchor could drift. A converted
  book also saves its position as a character offset into the book's text,
  which re-conversion keeps, and uses it when the converter version
  differs.
- **Metadata without converting.** The library needs a title and author
  for a row, not chapters: `describe` reads front matter, header labels
  and the file name without building the book.

## Covers in detail (2026-10-15)

mareader made covers for PDFs only, keyed by path (a move re-rendered
them), held as data URLs and capped at 60. mareader2 covers every format,
keyed by content:

- **Where the art comes from.**
  - PDF: the first page, rendered at cover size on a worker, apart from the
    page cache.
  - EPUB: the manifest's `cover-image`, then EPUB 2's `meta name="cover"`,
    then the first picture of the first chapter, as the reader places it.
  - Markdown: the front matter's `cover`, then its first picture.
  - Plain text and books without art: a drawn cover, the title and author
    on a card tinted from the theme. Painted by egui, so it follows the
    theme instantly and costs no file.
- **The cached file:** `<cache>/covers/v<N>/<content-id>.jpg` (or `.png`
  when the art has transparency), long edge 512 px, never upscaled.
  - Written atomically, temp then rename.
  - Bumping `N` when rendering changes rebuilds every cover; the old
    directory is deleted at launch.
  - Twins share one cover, and a moved file keeps it.
- **Failures.** Undecodable art writes nothing; the content is remembered
  as failed until the next launch, and shows the drawn cover.
- **Loading in egui.** `ScrollArea::show_rows` names the visible rows, so
  no observer is needed.
  - The visible rows are requested first, then a screen above and below.
    Rows scrolled away are dropped from the queue by a generation number.
  - Workers decode to a `ColorImage` at the card's pixel size
    (`pixels_per_point`); the UI uploads a few per frame, so a fast scroll
    never stalls a frame.
  - Textures sit in an LRU keyed by content id with a byte budget; the
    drawn cover shows until the art arrives, then fades in unless
    Animations are off.
- **A cover the user picks** (later) is user data: it lives in the data
  directory, not the cache, and syncs.

## Ready for sync (2026-10-15)

Sync comes after 1.0 (`docs/sync.md`), but its ground is laid in M5 so no
library needs migrating later: ids nobody else mints, a hybrid-clock stamp
on each synced field, tombstones instead of deletion, sort keys for
manual order, and synced facts kept apart from a machine's own
(`docs/sync.md` § What M5 must lay down).

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

- **Shelves that mirror folders and are editable.** Replaced by read-only
  source trees plus user shelves.
