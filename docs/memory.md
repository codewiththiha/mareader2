# Memory

> **Status:** baseline only · **Last updated:** 2026-10-13

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
- **Closing a pane:** texture count and bytes return to their value before it
  opened (a kittest test).
- **Thumbnails:** LRU of 16, as in mareader.
