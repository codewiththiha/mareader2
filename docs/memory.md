# Memory

> **Status:** M2 audit recorded · **Last updated:** 2026-10-13

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
