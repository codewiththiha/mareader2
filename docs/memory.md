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
