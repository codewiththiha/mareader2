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

**Persistence** (owner decision, 2026-10-15): versioned serde files, each
written atomically (temp + rename) on a worker after a short debounce.

- `library.json` holds the facts that rarely change: books, locations,
  shelves, sources.
- `state/<book-id>.json` holds one book's position (and its marks, M6). A
  page turn rewrites only that small file, never the whole library; the
  same split is the sync layout (`docs/sync.md` § Wire layout).
- Covers live in the cache directory, keyed by content id, rebuilt on
  demand.

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

## Slice 1: records and resume (2026-10-15)

What the first slice built, and where it stops short of the model above.

- **Opened files are books.** Every file a pane opens (dialog, drop, the
  system, launch) is measured on the library's worker and recorded with a
  third origin, `Opened`: read where it is, never moved or deleted. Sources
  and the store arrive with their slices. Samples and other in-memory
  documents have no file, so they are not recorded.
- **Recording an open.** The book holding that content takes the path as
  its newest location (a move). Failing that, the book already at that path
  takes the new content (an edit in place). Failing both, a new book is
  made, and a removed book of that content comes back. A path belongs to
  one book; the book that held it loses it.
- **Positions** are the page, the page count then, and a book's block
  anchor, recorded each frame but written only when they change. Opening a
  known file without a page asks the library, so any open resumes. Opens
  that come before the library has loaded wait for it.
- **Measuring.** SHA-256 of the size and, for PDF and EPUB, 64 KiB from each
  end; text, and anything up to 128 KiB, is hashed whole. The first 16
  bytes are the content id.
- **Files.** `library.json` is `{ schema, clock, books }`;
  `state/<id>.json` is `{ schema, book, state }`. Each field carries its
  stamp. Writes leave at most every half second, and on exit.
- **Failure.** A `library.json` from a newer schema is not written to; the
  library lives in memory and says so. An unreadable one is renamed to
  `library.unreadable.json` and the library starts empty. An unreadable or
  orphaned state file is skipped. Each location is checked when the library
  loads; a book with none left shows as missing and says so when clicked.
- **The page.** The title bar's Library button swaps the panes for a list,
  last opened first: format, title, page and progress, and when it was last
  opened. Escape goes back if something is open; any open does too.
- **Not yet.** No `Disk` trait: the pure rules take what they need (the
  existence check is a closure) and `disk` does the rest. Authors, covers,
  the character offset for converted books, and following a file that
  moved while the app was closed all wait for later slices.

## Slice 2: grid, list, sort and covers (2026-10-15)

- **View.** `mareader_library::View` keeps the layout, the pinned columns
  (none is Auto), the cover fit and the sort, inside the settings as
  `library`. An older settings file without it loads the defaults: grid,
  Auto, Fit, last read newest first. The View options menu (in the title
  bar, § Routes) changes each at once and stays open to chain changes.
- **Sort.** Last read, title, author or date added, either way. A new key
  starts in its natural direction (names A–Z, dates newest first). Names
  compare ignoring case, with digit runs as numbers, so Volume 2 comes
  before Volume 10 and `007` equals `7`. A blank author sorts last both
  ways. Ties fall to the title, then the book id, so the order never jumps.
- **Grid.** Auto fits as many 150 pt cards as the width allows, 2 to 10;
  a pinned count shares the width. Each card is an A4 cover box, the title,
  the author (else the page reached), and a progress line. Fit shows the
  whole cover bottom-aligned; Crop fills the box. Covers shaped past 0.55
  to 1.8 are trimmed to it. Rows are virtual (`show_rows`).
- **List.** The rows gain a thumbnail; the format joins the detail line.
  A missing row fades its time too, and the header lines up with the rows.
- **Authors.** An EPUB's first non-blank `dc:creator`, or a PDF's
  `/Author`, fills a book's author when it has none; a later open never
  overwrites it, so a reader's own edit will stand.
- **Covers** follow the section above. The session's `cover` reads the
  art: a PDF's first page; an EPUB's declared cover, else a picture among
  the first four blocks of its opening chapter; text and Markdown are
  converted first. One `covers` thread takes the first still-wanted ask;
  each frame replaces the asks (shown, then a screen below, then above).
  Results keep as JPEG (opaque) or PNG at 512 px in
  `<cache>/covers/v1/<content-id>`, written atomically; other versions'
  folders are deleted. The cache is the platform's (`~/Library/Caches`,
  `$XDG_CACHE_HOME` or `~/.cache`, `%LOCALAPPDATA%`), and tests have none.
  Four textures upload per frame within a 64 MiB budget, least recently
  shown dropped first; each fades in unless Animations are off. A book
  without art gets a drawn cover: a steady per-book tint, title, author
  and format.
- **Title bar.** The library keeps its own pin (on by default), set from
  the bar's Pin alone, as mareader's.
- **Not yet.** Manual order arrives with shelves. A cover that failed is
  retried only after a relaunch.

## Slice 3: folders read in place (2026-10-15)

- **Adding.** Folders (beside Open…, and on the empty page) adds a folder
  through the system dialog. Its books are read where they are: nothing is
  copied, moved or written there. A folder inside one already added is
  refused by name; one holding added folders takes them in (their books
  keep their places) and says so. The library's own folders are never
  walked. `library.json` moves to schema 2 with a `sources` list; an older
  app leaves the newer file alone.
- **What counts.** Each folder has formats (all four by default, at least
  one), a size floor and watching (on), under What counts. Narrowing
  keeps books already found; widening rescans that folder. A walk skips
  hidden and `~$` names, system folders (`node_modules`, `__MACOSX`, the
  recycle bin and the like) and links, stops at
  20,000 files, 20,000 folders or 24 levels, and says when a folder was cut
  short. A file written in the last 3 s waits for the next look, 3.5 s
  later, so a copy in progress isn't measured half done.
- **Rescans.** On launch, when the window comes back (at most every 2 s),
  from Rescan all, and per folder from its Rescan button. A full rescan
  first re-checks every known file, then walks the watched folders; a named
  one walks only that folder. Unchanged files (same size and modified time)
  aren't hashed again, and names are read only from contents the library
  hasn't met. The walk, hashing and reading run on the library worker.
- **What a scan does.** A known content under a new path is the same book,
  followed with its place, and its lost addresses are dropped. A known path
  with new content is an edit in place: the same book. A removed book's
  content comes back as that book, with its place. Anything else is a new
  book, found but never opened: it lists "Not opened" and sorts last by
  last read. Titles come from the file (an EPUB's `dc:title`, a PDF's
  `/Title` unless generic, Markdown front matter), else the file name; PDFs
  opened in the reader now use the same rule.
- **Missing books.** A missing book stays listed, faded, until found or
  removed. When its folder's drive isn't there, it reads Offline ("Drive
  not connected") instead of Moved or deleted. The chips over the books
  show All books, Missing (when any) and each folder with its count.
- **Find again.** Clicking a missing book opens a sheet with its last path:
  Choose the file… or Search a folder…. A chosen file of the same content
  heals the book and opens it. A search walks the folder for the same
  content under any name, checking only files of its old name or size.
  A namesake with other content (another edition, or edited) is offered:
  Use this file keeps the book and its place. A file that is another book,
  or another format, is refused in one sentence. Cancel or Escape changes
  nothing, and Escape doesn't leave the library while a sheet is up.
- **Removing a folder** asks first and says how many books found only there
  leave the library. Books also opened from elsewhere stay. Files are never
  touched; adding the folder again brings the books back with their places.
- **Not yet.** Folders as a browsable tree, hiding single books from a
  folder and restoring them, and a progress card for big first scans come
  with slices 4, 5 and 7. There is no file watcher: rescans are cheap and
  run when the window comes back, as mareader does.

## Slice 4: shelves and the breadcrumb (2026-10-15)

- **Shelves.** New shelf (in the page header and the View menu) makes
  one where the reader is, opens it and selects its name in the
  breadcrumb: type, then Enter or click away; Escape keeps the old name.
  Shelves nest without limit and sort by name, numbers by value. Names are
  one line, trimmed, at most 120 characters, and unique among siblings
  ignoring case; a clash is refused in a toast. A book may sit on any
  number of shelves; a shelf holds books by id, so moves, edits in place
  and find-again keep it on its shelves.
- **The page.** Shelf cards come first: a 2 × 2 plate of the first four
  shelves (a shelf tile) and covers, the name and "4 books · 1 shelf". In
  the list, shelves are rows with a chevron. A click opens a shelf; the
  header then shows its name and counts, and the top level keeps Open…
  and Folders. The chips (All books, Missing, each folder) show only at
  the top level and list matching books flat, without shelves; opening a
  shelf returns them to All books. Right-click on a shelf: Open, Rename…,
  Remove shelf….
- **What the top level shows** is a View setting, Library shows: All books
  (the default) or Unshelved, books on no shelf. Either way shelves come
  first.
- **Drag to file.** A book card dragged onto a shelf card or a crumb is
  filed there, with a label under the pointer. From an open shelf it moves:
  it leaves that shelf, unless Alt is held, which adds it and keeps it
  (the label shows "+"). Dropped on the top level's crumb, it leaves the
  open shelf only. A shelf dragged onto a shelf or crumb moves inside it;
  into itself or its own shelves is refused ("A shelf can't go inside
  itself."). A book already there says so. Escape drops what's carried
  and stays on the page. Each drop says what it did in a toast.
- **Manual order.** Sort by Manual (First to last, Last to first) shows a
  shelf in its own order; books are added at its end, and dragging a book
  beside another on that shelf puts it there, with an accent bar where it
  lands. Elsewhere Manual reads as date added. Each membership carries a
  base-62 fractional key, so a move rewrites one record and another key
  always fits between two (`crates/library/src/key.rs`); a merge that
  leaves two keys equal or out of order still yields a usable one.
- **Breadcrumb.** On a shelf the bar's left shows Library › … › the last
  levels, and the centre title gives way. Past four crumbs the middle
  folds into a … menu; each name is cut at 160 pt. Crumbs open their level
  and take drops. The open shelf's crumb has Rename… and Remove shelf….
- **Removing a shelf** asks first: its books stay in the library, and its
  shelves move up to its parent (or the top), a clashing one renamed
  "Name 2". The page then shows the parent. The shelf and its memberships
  become tombstones, so a sync can't bring them back. A shelf open while
  removed elsewhere gives way to its nearest living parent.
- **Saved.** `library.json` moves to schema 3 with `shelves` and
  `members`; names, parents, keys and the alive flag are stamped fields
  (`docs/sync.md`). A parent that is removed, unknown or loops back counts
  as the top level, so a bad merge can't hide a shelf.
- **Not yet.** Several books at once (selection) and shelf menus for
  books come with slice 5; dragging onto panes with slice 7.

## Slice 5: selection, menus, removal and imports (2026-10-15)

- **Picking.** Ctrl-click (⌘ on macOS) picks a book or shelf card;
  Shift-click picks the run from the last pick, in the order shown;
  Ctrl/⌘+A picks everything on the page. While anything is picked a plain
  click picks too, instead of opening (owner decision). Picked cards wear
  an accent ring and a check. Escape, a click on empty page or Clear
  selection lets go; changing level or chip does too.
- **The selection bar** rises at the page's foot: "3 books · 1 shelf
  selected", then New shelf from these, Take off this shelf (on a shelf),
  Remove… and Clear. Dragging a picked card carries all the picks; the
  drop files them together and lets go.
- **Menus.** Right-click a book: Open (Find… when missing), Select,
  Rename…, Read separately, Show in Finder / Explorer / folder, Take off
  this shelf, Remove…. A shelf: Open, Select, Rename…, New shelf inside,
  Remove shelf…. Right-click inside a selection: the bar's actions for
  all of it. Empty page: New shelf, Import books…, Select all.
- **Removal receipt.** Remove… asks first and says what goes: how many
  books, the places and shelf spots kept, the store copies deleted and the
  folder books hidden. A book that is also somewhere else is only taken
  off; removing a shelf among the picks keeps its books.
- **What removing does** depends on where the book lives (owner decision):
  - in a folder: hidden. Its file is never touched, a rescan leaves it out,
    and Folders lists it under Removed · N with Restore and Restore all.
    Restored, it comes back with its place and shelves.
  - in the library's own folder (a store copy): the record becomes a
    tombstone and the copy is deleted once no other book lists it.
  - opened from elsewhere: the record goes; opening the file again brings
    the book back.
  The toast names what went and, for folder books, where to restore them.
- **Read separately** makes an independent twin ("The Raven (2)") with
  its own place, name and shelves, sharing the file. Opening a twin reads
  at its own place; the original keeps its own. Removing the original
  promotes the first living twin, so opening the file by path still finds
  a book.
  Which book a pane reads as travels with its open request into the
  pane's view, and is saved with the last-read file. A failed open
  leaves nothing behind, another pane on the same file is never taken
  over, and a twin reopens as itself after a relaunch. A pane reading a
  removed book stops recording its place rather than hand it to another.
- **Rename…** edits a book's title in a sheet: Enter or Rename saves,
  Escape cancels; at most 120 characters. Rescans and later opens keep
  it; the title is a stamped field for sync.
- **Imports are copies** (owner decision). Import… (the page header) and
  files dropped on the library copy books into `files/<content>/<name>` in
  the library's folder, onto the shelf shown, and record them as store
  locations. The copy is written to a `.part` file and renamed, so a
  crash leaves no half book. Content the library already has is the same
  book, not a second copy ("was already in your library"); files it can't
  read are named in the toast. Folders dropped are added as folders read
  in place, and Open… still reads a file where it is.
- **Reveal.** Show in Finder selects the file (`open -R`); on Windows
  Explorer selects it; on Linux the file manager's D-Bus ShowItems, else
  the parent folder opens. A failure says so in a toast.
- **Saved.** `library.json` moves to schema 4: `Origin::Store`
  locations, `twin_of`, and each folder's `hidden` set.
- **Not yet.** Search with slice 6; dragging onto panes and import
  progress with slice 7.

## Slice 6: search (2026-10-15)

- **One rule.** `mareader_library::Query` decides both what the page
  keeps and what the bar suggests. A query is split into words, and every
  word must turn up in the title, the author or the address. A word can
  turn up as a substring, or as an ordered subsequence that keeps its
  shape: `mthmtcl` finds "Mathematical", `tlhod` finds "The Left Hand of
  Darkness", while `dunee` does not find "Dune". Case doesn't matter, nor do
  Latin accents (`elan` finds "Élan", `zola` finds "Zoła"), curly quotes or
  long dashes. Nothing is indexed: the page's list is cached instead.
- **Scores.** A substring scores by its length, and more at the start of a
  word. A subsequence scores per letter: more for runs, less for gaps, and
  more at word starts. Words start after a space or mark, at a case step
  ("MobyDick") and where letters meet digits ("vol10"). Each word takes its
  best field: title ×3, author ×2, address ×1. Ties go to the book read
  last, then by name.
- **The address** is what the file's place says: the path inside its folder
  ("Scifi/1984.pdf"), or the parent and name of a loose file
  ("Downloads/paper.pdf"). A store copy's path names only its content, so
  it isn't searched.
- **The field** takes the centre of the library's bar where the reader's
  title sits. It reads "Search 12 books", or names the shelf or folder
  chip it searches. ⌘F or Ctrl+F focuses it with its text selected. With
  no document open, the find key opens the library and focuses its
  search. While typing, or while a search filters the page, the bar stays
  shown even if unpinned.
- **The page** filters as you type. At the top level a search looks through
  every book, shelved or not, whatever Library shows; a folder or Missing
  chip narrows it, and on a shelf it searches that shelf. Shelf cards are
  kept by name; at the top a search lists nested shelves too. The header counts "3 of 12 books". When nothing here
  matches, the page says so and offers "Show the N matches elsewhere" and
  Clear the search. Going into a shelf ends the search (it was found);
  going up keeps it.
- **Suggestions** list under the field: the best seven books and shelves
  from the whole library, each with its thumbnail or shelf mark, the
  matched letters picked out, and the author underneath (or the address,
  when that's what matched; a shelf's parents for a shelf). Down opens the
  list or moves down, Up moves up and closes it from the top row. Enter or
  a click shows the chosen one. Enter with nothing lit closes the list and
  keeps the filter.
- **Showing a choice.** A book visible where you are is ringed in place.
  Otherwise the page goes to the top level, where the search lists every
  match; a shelf chosen from inside another shelf opens its parent. The card is scrolled to the middle
  and ringed in the accent with a halo that pulses twice. The ring fades
  after four seconds; with reduced motion only the ring shows.
- **Escape peels** one layer at a time. In the field: the list, then the
  text, then the focus. On the page: the selection, then the search, then
  back to reading.
- **Kept fresh** by a revision number the model bumps on every change it
  shows, saved or not (a rescan, a presence check). The page's list is
  cached by revision, open shelf, chip, sort, Library setting and query.
  That also ends the per-frame sort of slice 2.
- **Shots.** `search=<text>` types into the field and `open=search`
  focuses it: `books=10 library=show search=essays open=search`.

## Slice 7: the Library panel, drops on panes, progress (2026-10-15)

- **The panel.** The reading sidebar's Library panel lists shelves, then
  books, by name (natural order, case folded). The top level shows what
  Home shows: every book or only unshelved ones. A shelf opens in place
  with its chevron, a click or Right, and folds with Left; each counts
  the books under it. Books carry a format badge and a dot when open in
  a pane; the focused pane's book is lit. A missing book is faded and
  opens the find-again sheet instead of a pane.
- **Tabs.** While the window is split, the panes are listed above the tree
  as tabs, named by the library (so twins differ) or the document. A
  click focuses a pane, × closes it.
- **A click** follows Settings › Workspace › Library panel:
  Open in focused pane (default), Open as a new split, or Do
  nothing. A book already shown only focuses its pane. Beside splits to
  the right, or below when there is no room, and an empty focused pane
  takes the book instead. Do nothing still lets Enter open beside, so
  the panel works from the keyboard. The Library page follows the first
  two; with Do nothing it opens in the focused pane, since the page has
  no panes to drag onto.
- **A drag** from a book row onto a pane shows where it lands: the half
  the new pane would take, lit, named "Split right of Middlemarch". The
  edge is the nearest one, ties going right, then below, left, above. An
  empty pane takes the book whole ("Open here"). Letting go elsewhere, on
  the rail, or on the library page drops nothing. The rule lives in
  `mareader_workspace::DropTarget`, so it is tested without a window.
- **Progress.** A folder walk or an import that runs longer than 0.4 s
  gets a card in the bottom-left corner: a ring that spins while a
  folder is walked, then fills as files are read or copied ("Reading 4 of
  12", "Importing 3 of 12") with the file at hand underneath. One card
  per run; it leaves when the run ends, and the toast reports the result,
  so the card never needs closing. The worker sends a beat only when the
  ring moves a whole percent, so huge folders don't flood the queue.
- **Not yet.** Importing a mareader library is deferred (owner,
  2026-10-15); see § Optional below.
- **Shots.** `sidebar=library` opens the panel; `library_click=split`
  picks the click; `carry=x1,y1,x2,y2` holds a drag without letting go;
  `runs=demo` shows two cards: `pdf=sample:5 books=10 shelves=demo
  sidebar=library panes=2`.

## Routes: the reader and the library (2026-10-15)

The window shows one of two routes, `crates/ui/src/route.rs`'s `Route`:
the reader (panes beside the rail) or the library (the shelf alone). The
library replaces the panes without closing them, unlike mareader, which
closes the book. `Route::allows` is one exhaustive table of which commands
each route takes. The keymap skips bindings the route drops, so their keys
reach the page's own widgets. The app drops the same commands from any
other source, and the bar, rail and lights read the route, not flags.

**No rail in the library.** The sidebar's panels (outline, pages) serve
the focused document, which the library hides. The library draws no docked
or overlay rail, no edge reveal and no toggle. The rail keeps its state, so
going back finds it as it was. On macOS the bar's gutter hosts the lights.

**The library's bar** keeps only what acts on the shelf or the window:

| Item | Library | Why |
|---|---|---|
| Sidebar toggle | gone | No rail in the library (above). |
| Library button | pressed, "Back to reading", only while a pane is open | Panes wait behind the page; with nothing open there is no way back. |
| Title | the search field | The centre holds the library's search (slice 6); the breadcrumb (slice 4) takes the left. |
| Settings | gone | Settings are about reading; the gear lives on the reader's bar, as in mareader. |
| Search | gone | It finds in the focused document. The library's own search field replaces the title (slice 6). |
| Split down, Split right | gone | They split panes, which the library hides. |
| View & tools (☰) | replaced by View options | Its rows serve the panes: folded tools, view mode, zoom, the reader's shortcuts. Open… stays on the shelf, full screen on its key. |
| View options | in the ☰ slot | mareader keeps the shelf's View menu in its bar; the slot keeps Pin and Appearance where the reader has them. |
| Appearance | kept, without Page texture | The shelf has no page, as in mareader. The ink dial and per-pane switches go too. It edits the window's look, which the shelf wears. |
| Pin | kept | The library's own pin, on by default. |
| Window buttons | kept | The window's. |

**Keys in the library.** Zoom, page moves, view modes, the sidebar, Split
and Settings do nothing. Find focuses the library's search (slice 6). Open (⌘O) and full screen still work. The
View menu command (`open=menu` in shots) opens the library's own menu.
Escape and the Library button go back only while a pane has a document.
Entering the library closes the find bar and the settings window.

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
