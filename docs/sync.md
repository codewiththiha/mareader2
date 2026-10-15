# Sync: design

> **Status:** design for after 1.0 (M9); M5 lays its ground · **Last updated:** 2026-10-15

mareader has no sync. mareader2 will sync one library across machines
through several backends. Sync itself comes after 1.0, but the library's
model (M5) must be ready for it from its first release: retrofitting ids,
clocks and tombstones later means migrating every user's library.

## What sync is for

1. **One library on every machine:** books, shelves, positions, marks
   (M6) and the portable settings.
2. **Bring your own storage.** A synced folder (Dropbox, iCloud Drive,
   OneDrive's folder, Syncthing), WebDAV, S3-compatible storage, Google
   Drive and OneDrive's API.
3. **Offline first.** Every machine is complete on its own. Sync merges
   whenever a backend is reachable; nothing waits on the network.
4. **Never lose work.** Concurrent edits on two machines both survive when
   they touch different fields; a removal on one machine never deletes a
   user's own file anywhere.

## What M5 must lay down

These cost little in M5 and make sync a transport, not a migration.

- **Ids nobody else mints.** An original book's id derives from its first
  content id, so two machines that import the same file agree on it
  without talking. Twins, shelves, memberships and marks take random
  128-bit ids. No counters.
- **Synced and local facts kept apart.** Synced: content id, title,
  author, format, added, opened, position, marks, shelves, memberships,
  `twin_of`. Local to a machine: locations and `present`, size and mtime
  stamps, sources and their `hidden` sets, window and pane state. Sources
  are folders on one disk, so they never sync.
- **A stamp on every synced field.** A hybrid logical clock (below), so a
  rename on one machine and a shelf move on another both land.
- **Removal is a tombstone.** A removed book or shelf keeps its record
  with a removal stamp, and the UI filters it out. Records never vanish.
- **Manual order by key, not index.** A shelf's order is a sort key per
  membership (a string between its neighbours'), so two machines that
  reorder different books merge without a conflict.
- **A device id**, random, made on first launch and kept in the data
  directory.

## Clock: hybrid logical time

A stamp is `(milliseconds, counter, device)`, packed as fixed-width hex so
comparing the strings compares the times. Making a stamp takes the larger
of the wall clock and the last stamp seen, local or remote; a tie bumps
the counter. Ties between devices fall to the device id, so every machine
picks the same winner.

**Why not the wall clock.** A machine whose clock runs an hour fast (a
wrong time zone hand-corrected by the user is common) would win every
write for that hour and lock its peers out. With hybrid stamps a peer that
sees the fast stamp moves its own clock past it, so its next edit still
wins. Any "last synced" label clamps future times to now.

## Merge rules

Merging is a pure function, `merge(local, remote) -> merged`, in the
library crate, with property tests that it is commutative, associative
and idempotent. Because every machine holds full state and re-merges, a
blind upload of the merged state converges even when a write is lost.

| Record | Rule |
|---|---|
| Book fields (title, author, opened, `twin_of`) | Last writer wins, per field |
| Position | Last writer wins; an open book shows "Read to p. 90 on another device" with Jump, and never moves the page by itself |
| Shelf (name, parent) | Per field; a move that would make a cycle is dropped, keeping the older parent |
| Membership | A record per (shelf, book): alive flag and sort key, each last writer wins |
| Mark (M6) | A set by id; fields per stamp; tombstoned |
| Removal | `removed` and `restored` stamps; alive while `restored > removed`. Field edits never revive a record; only restore does |
| Settings | Per section, last writer wins; only the portable sections sync |

**Same content, two originals.** Ids derived from content make this rare,
but an edited text file imported on two machines can still give two
originals of one content. The merge folds them: the lower id survives,
the other becomes a tombstone that names it, and memberships and marks
move across.

**A newer version of a book** (a re-downloaded or re-exported file) is new
content. Importing it with "Replace" keeps the book's id, shelves and
marks and tombstones the old content, so peers stop showing a card with
no file behind it. One book, last import wins; versions never pile up.

## Backends

One small blocking trait, run on a sync worker thread. The UI never waits
on it; results arrive as `Command`s, as the session's do. No async
runtime: the HTTP client is blocking and the worker owns it.

```text
trait Remote {
    read(path) -> Option<bytes>          // None: not there
    head(path) -> Option<{size, etag}>
    list(dir)  -> Vec<Entry>
    write(path, bytes, if_match: Option<etag>) -> etag
    upload(path, local file) / download(path, local file)   // streamed
    delete(path)
}
errors: Auth | NotFound | Network | Conflict | Quota | Other
```

Every backend passes one conformance suite, run against a fake and a local
folder in CI. Known quirks the adapters absorb: S3 has no directories;
WebDAV wants each parent created top-down; OneDrive creates parents
itself; Google Drive allows two files of one name in a folder, so its
adapter caches ids and picks the oldest. Writes are conditional (`If-Match`)
where the backend supports it; elsewhere a lost race heals next round
because merging is state-based.

Credentials live in the system keychain, never in the settings file.

## Wire layout (file backends)

```text
<root>/mareader/
  library.json                    books, shelves, memberships (synced fields)
  state/<book-id>.json            position and marks of one book
  files/<content-id>/<title>.<ext>   the file, when file sync is on
  files/<content-id>/cover.jpg
```

Each JSON file is an envelope: `{ schema, device, records }`. The layout
is frozen once shipped; later versions only add paths, which older
clients ignore. Content-id directories mean a rename never renames a
remote file. Text files sync as their source, which converts the same
way on every machine (`docs/epub.md` § Text files as EPUBs).

## A sync round

1. Pull `library.json` when its etag changed; merge; apply.
2. Push the merged state when it differs from the remote.
3. Pull and push `state/` files of books changed on either side: a local
   dirty set plus the listing's etags.
4. Files: upload store copies the remote lacks; a book only on the remote
   shows as a cloud card with its cover and downloads when opened.

**Changes made during a round are kept.** The round snapshots the dirty
set at its start; anything edited after stays dirty for the next round.
Dropping in-flight changes is the classic way a new import never reaches
the other machines.

## Removing

- Removing a book tombstones it everywhere; the remote file stays.
- "Remove everywhere" also deletes the remote file. It is a separate,
  explicit choice with its own stamp.
- Nothing ever deletes a file in a user's source folder, on any machine.
  Only store copies the app made are its to delete (`docs/library.md`).
- Tombstones are kept for good. They are small, and dropping them lets a
  machine that was offline for months bring removed books back.

## Why SQLite (owner, 2026-10-15)

The owner reversed the plain-files decision: the library is one bundled
SQLite file, `library.sqlite` (`docs/library.md` § Slice 9). The record
model above is unchanged; only where records rest moved.

- **Records** (books, shelves, memberships) are rows of `{kind, id, body,
  seq}`. The body is the record's JSON, every field stamped as above. Rows
  are never deleted; a removal is a tombstone in the body.
- **Local rows** (locations, folders) live in their own table and never
  leave the machine.
- **The change cursor.** Each save gets the next `seq` and stamps the rows
  it wrote. A sync remembers the last `seq` it pushed and asks the store
  for rows past it, so a round never diffs the whole library.
- **Merge** is `Merge` in `crates/library`: per field last-writer-wins,
  tombstones only grow. Property tests check it commutes, associates and
  is idempotent, which is what lets backends race safely.
- **Positions** stay `state/<book-id>.json`, as in the wire layout.
- **Migrations** are dated, append-only SQL steps keyed by `user_version`.
  A newer file is read but never written.

The file layout above is the remote side and is unchanged: a file backend
writes records from the store into those envelopes. No backend exists yet.

## Revision history

| Date | Summary |
|---|---|
| 2026-10-15 | First design: what M5 lays down, clock, merge rules, backends, layout |
| 2026-10-15 | SQLite store replaces plain files (owner): records, local rows, change cursor, `Merge` |
