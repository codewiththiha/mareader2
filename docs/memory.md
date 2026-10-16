# Memory

> **Status:** M8 audit recorded · **Last updated:** 2026-10-16

Every memory claim in this repository needs:
- what was measured (RSS peak, texture bytes, heap);
- how (tool and command);
- the workload;
- before and after;
- known limits.

Add a dated section per audit; never overwrite one.

## 2026-10-13: renderer baseline (from the PdfCraft study)

- **What:** peak memory above the process baseline while rendering every page
  at 150 %, one thread.
- **How:** the harness in the earlier study (`bench/`, not in this repo).
- **Limits:** pdf.js and WASM ran in Node, not a webview, so pdf.js numbers are
  pessimistic for the GPU canvas path.

| Document | hayro native | pdf.js (Node) |
|---|---|---|
| tracemonkey, 14 pp | 12 MiB | 128 MiB |
| arXiv "Attention", 15 pp | 21 MiB | 269 MiB |
| PDF 1.7 spec, first 40 pp | 18 MiB | 56 MiB |
| scanned, 8 pp at 400 dpi | 49 MiB | 195 MiB |
| CAD, 4 pp × 170k paths | 118 MiB | 550 MiB |

## Budgets to hold (set at M2)

- **Idle window, no document:** measure it at M1 and record it here as the floor.
- **Closing the last pane on a document:** texture count and bytes return to
  their value before it opened (a kittest test).
- **The same book in two panes:** one parse, one tile set; the second pane adds
  only the tiles its own zoom needs.
- **Thumbnails:** LRU of 16, as in mareader.

## 2026-10-13: M2 audit, reading and closing (slice 7)

- **What:** process RSS and its peak (`VmRSS`, `VmHWM` in `/proc/self/status`;
  the peak is reset after each row), with the app's own counts: open
  documents, cached page rasters and their bytes, text layers, queued jobs.
- **How:** `crates/ui/examples/memory.rs`, run as
  `capped.sh cargo run -q -p mareader-ui --example memory -- BOOK.pdf 12`.
- **Workload:** a 1200 × 800 pt window at 2 px per pt, two panes, 0 workers,
  scroll jumps landing at once. Open the book in the right pane, go to every
  page in turn (each drawn), close the pane. Repeat open, read and close in a
  new pane (rounds). Then open it in one pane and again beside it.
- **Limits:**
  - The GPU is lavapipe, a software Vulkan device. Its textures and its own
    startup live in process memory, so RSS here counts what a real GPU keeps
    in VRAM, and the idle floor is high.
  - It is a dev build (`opt-level` 2 for dependencies, 1 for ours, no LTO).
    That changes code size more than heap.
  - glibc keeps freed memory for reuse, so RSS after a close stays above
    idle. The rounds tell that apart from a leak.

| Step | tracemonkey, 14 pp | arXiv "Attention", 15 pp |
|---|---|---|
| Idle, two empty panes | 163 MiB | 164 MiB |
| Opened in one pane (3 rasters, 22 MiB) | 217 MiB | 220 MiB |
| Every page read (rasters: 104 MiB / 111 MiB) | 319 MiB, peak 319 | 339 MiB, peak 339 |
| Its pane closed (0 documents, rasters, layers, jobs) | 236 MiB | 250 MiB |
| Rounds 2 to 12, each read and closed | 254 to 255 MiB | 250 MiB (rounds 2 to 4) |
| Opened again, then in a second pane too | 261 MiB, then 261 | 250 MiB, then 250 |

Findings:

- **Closing the last pane on a book returns everything the app counts:**
  documents, rasters, text layers and jobs all reach 0, as the kittest
  `closing_the_last_pane_on_a_book_frees_its_textures` asserts.
- **No leak:** after a close, RSS levels off at 254 MiB over eleven rounds of
  reading all 14 pages. The difference from idle (about 90 MiB) is memory the
  allocator keeps, and the next book reuses it.
- **One parse for the same book twice:** a file is opened by its canonical
  path, and a pane opening a book another pane shows shares its document and
  rasters. The second pane added no document, no raster and no RSS. Before
  this audit it parsed the file again
  (`one_file_in_two_panes_is_read_once_and_drawn_once`).
- **A page jump draws only where it lands:** during a glide, pages flown past
  are neither rendered nor prefetched. Without this, going to page 200 of 300
  cached 22 pages instead of 2
  (`a_jump_glides_and_only_its_landing_is_drawn`).
- **Rasters dominate:** a fitted page at 2 px per pt is about 7.4 MiB, so the
  256 MiB cap holds about 34 such pages. A book under the cap stays whole once
  read, which is why "every page read" ends at 104 MiB of rasters.

## Tiles (M2 slice 9)

What was measured: the page cache's bytes in
`a_deep_zoom_draws_tiles_once_the_view_rests`, a three-page sample at 500 %
in a 900 × 700 pane without a GPU, where egui's texture side is 2048.

- **Before tiles:** a page past the texture side rendered at the capped
  scale and was stretched, so deep zoom was blurred.
- **With tiles:** the page draws a backdrop of at most 2048 px under tiles
  of 1024 to 2048 px. At rest the view held 2 tiles; after a pan, 4. The
  cache stayed at 58 MiB with every page and scale counted.
- **Bounds:** a tiled page is at most 16,384 px a side. Tiles share the
  256 MiB cap and go cold like pages. A page drawn at another scale drops
  its old tiles. A pan queues no tile until the view rests 0.25 s.
- **Threadless runs:** inline work queued by a demand now asks for a frame.
  Before, a test or the web preview could stop with jobs left queued.

## 2026-10-14: EPUB layout (M4 slice 7)

What was measured: resident memory (VmRSS, and VmHWM for the peak) from
`/proc/self/status` before and after `Book::open`, on CI's Linux runner.
How: `crates/epub/tests/layout_memory.rs`, an ignored test, run with
`cargo test -p mareader-epub --test layout_memory -- --ignored --nocapture`.
Each workload is one generated chapter in bundled fonts.

| Workload | Units | Open time | Pages | RSS growth | Per unit |
|---|---|---|---|---|---|
| Prose, 1,000-character paragraphs | 4M characters | 0.92 s | 1,883 | +171 MB (peak 177 MB) | ≈45 bytes a character |
| `<p>w</p>` repeated | 200,000 paragraphs | 0.97 s | 9,524 | +523 MB (VmHWM 700 MB) | ≈2,700 bytes a paragraph |
| One paragraph | 2M characters | 0.41 s | 866 | +0 MB | unreliable: the allocator reused freed memory |

- **Why it grows:** a book is laid out whole when it opens, and each
  paragraph and table cell keeps its parley `Layout` for drawing and
  selection. The cost is per paragraph as much as per character.
- **The budget:** `Block::layout_cost` estimates 3,000 bytes a paragraph or
  cell plus 48 a byte of text (`LAYOUT_BYTES`, `CHAR_BYTES`), a little over
  what was measured. Before layout, a book estimated past 1 GiB
  (`MAX_LAYOUT`) is refused as too large
  (`a_book_past_the_layout_budget_is_refused`).
- **Real books:** *War and Peace* (3.2M characters) is about 190 MB; the
  complete Shakespeare about 560 MB. A 40M-character omnibus is refused.
- **Known limit:** a book near the budget holds most of it for as long as
  it is open. Laying out only the pages near the view, and dropping the
  rest, would bound it; that is the follow-up.

## 2026-10-14: EPUB layout on demand (M4 follow-ups)

What was measured: the same as the section above, with the same test, now
also reading every page's text after opening, which lays every block again
through the kept few. CI's Linux runner, VmRSS and VmHWM.

| Workload | Open | Held after opening | After reading every page | Peak |
|---|---|---|---|---|
| Prose, 4M characters, 1,883 pages | 1.39 s | +16 MB (was +171 MB) | +44 MB, read in 1.98 s | 54 MB (was 177 MB) |
| `<p>w</p>`, 200,000 paragraphs, 9,524 pages | 0.97 s | +59 MB (was +523 MB) | +78 MB, read in 1.21 s | 130 MB (was 700 MB) |

- **Why:** a set book keeps only each block's line boxes and its text; the
  parley layouts, about 3,000 bytes plus 48 a character each, are laid
  again for the pages drawn. At most 32 MiB of them are kept (`MAX_LAID`).
- **What stays:** about 4 bytes a character of prose, and about 300 bytes
  a one-word paragraph (its `Block` and its line boxes). `MAX_BLOCKS`
  (1,048,576) bounds that part near 300 MB; real books are far below it.
- **The cost:** a page not kept is laid again before it draws, a
  1,000-character paragraph in about half a millisecond. Reading all 9,524 pages' text took
  1.2 s.
- **Known limit:** each open book has its own 32 MiB of kept layouts.

## 2026-10-16: M8 audit, both formats and the library (slice 3)

- **What:** as at M2 (`VmRSS`, `VmHWM` reset after each row), plus open
  searches, cover texture bytes and egui's texture count. Each workload ends
  in a table of budgets, and a breach fails the run.
- **How:** `crates/ui/examples/memory.rs`, in CI on Linux as the Remote
  memory task (`tools/ci.py memory --ref BRANCH`), which also runs the EPUB
  layout test. Run 38919011226 on `a2a844c`; tables in the `memory` artifact.
  Modes: `memory BOOK [ROUNDS]`, `memory --library N` (N one-page PDFs read
  in place, in three folders), `memory --seeded N` (books without files).
- **Workload:** the M2 one, now searching each book for "the" before its
  pane closes, and drawing a second of frames after the close. The library
  is scrolled to its end by the wheel in the grid, then in the list,
  rendering every fourth turn as frames do.
- **Limits:** those of M2 (lavapipe counts VRAM as RSS; dev build; glibc
  keeps freed memory). The harness only submits GPU uploads when it renders,
  so a scroll that never renders holds every upload until it does (below).

| Step | tracemonkey, 14 pp | arXiv, 15 pp | The Raven, EPUB | War and Peace, EPUB |
|---|---|---|---|---|
| Idle, two empty panes | 184 MiB | 177 MiB | 177 MiB | 177 MiB |
| Opened in one pane | 243 | 231 | 240 | 247 |
| Every page read (rasters) | 357 (104 MiB) | 365 (111) | 517 (251) | 600 (251) |
| Its pane closed, a second later | 350 | 367 | 397 | 493 |
| Later rounds (2 to last) | 396 to 412 | 405 | 542 | 621 (one round) |
| Floor of the later rounds against the early top | −8 MiB | +0 | +1 | — |
| A second pane on the same book | +0 MiB | +0 | +0 | +0 |

| Library | RSS | Peak |
|---|---|---|
| Empty page | 186 MiB | 200 |
| 1,000 PDFs read in place | 217 | 231 |
| 1,000 more, second folder | 249 | 249 |
| 1,000 more, third folder | 258 | 258 |
| Grid scrolled to the end (401 turns; covers 63.3 MiB) | 410 | 410 |
| List scrolled to the end (450 turns) | 411 | 411 |
| 20,000 books without files, then scrolled | 255, then 332 | 332 |

Budgets, all held:

| Budget | Measured |
|---|---|
| A closed book leaves no document and no search | 0 and 0, every round |
| Later rounds' floor rises at most 16 MiB (no leak) | −8 to +1 MiB |
| A second pane adds under a quarter of the first open | +0 MiB of 54 to 70 |
| Page rasters within 256 MiB | 251 MiB at most (EPUBs) |
| Cover textures within 64 MiB | 63.3 MiB at most |
| Each book's share of the library, covers aside, at most 16 KiB | 3.3 KiB read in place, 3.4 KiB without files |
| A scroll to the end adds at most 256 MiB at its peak | +152 MiB grid, +0 list |

EPUB layout (`crates/epub/tests/layout_memory.rs`): 4 million characters
open in 1.39 s for +16 MB (peak 54); 200,000 paragraphs in 0.98 s for
+60 MB (peak 131).

Findings:

- **Covers no longer pile up when scrolled past.** Every cover asked for
  stayed waiting even after its card left the view, so a fling over 3,000
  books read and uploaded all of them. Now only covers asked for this frame
  or last wait, at most four load inline a frame, and they upload nearest
  first (`covers_scrolled_past_never_pile_up_waiting`).
- **A closed book's search let go of it late.** The find state kept the
  closed pane's document until the bar was used again; it is dropped on the
  frame the pane closes (`a_book_set_again_or_closed_lets_its_old_search_go`).
- **The 5 GB peak in the first run was the harness.** Run without rendering,
  egui_kittest writes each texture to the wgpu queue but only submits on
  render, so all 3,000 covers' staging buffers waited at once. Rendering
  every fourth turn, as a real frame does, brought the grid's peak from
  4,981 to 410 MiB with the same covers.
- **The first books carry fixed costs.** The first 1,000 books in place
  cost 31 MiB and the second 1,000 a further 32 (28 and 26 KiB a book,
  covers aside): SQLite's page cache and the memory kept after scanning.
  The third thousand cost 9 MiB, 6 of them covers: 3.3 KiB a book, as
  without files.
- **After a close, RSS stays higher than at M2** (350 against 236 MiB for
  tracemonkey; rounds 396 to 412 against 254). The app's counts reach zero
  and the later rounds stay level, so this is not a leak. A second of
  drawn frames returned nothing, which rules out wgpu's deferred frees. The
  difference is what lavapipe's allocator and glibc keep since the glass,
  the search and EPUB landed. On a real GPU that part is VRAM, and the
  rounds show it reused.
- **Seeding the library adds books** instead of renumbering the same ones,
  so `books=N` can grow a library in steps.
