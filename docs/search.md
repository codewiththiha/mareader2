# Search and keyboard (M7)

Finding text in the open document, then running the reader from the
keyboard alone. The parity entries are `search.document`,
`search.lazy-index`, `a11y.keyboard-only` and `pdf.auto-scroll` in
`parity/mareader-features.toml`.

## What mareader does

- **Matching.** Case-insensitive and non-overlapping, one page at a time.
  Each hit keeps 32 characters either side as its snippet.
- **Index.** Built on the first search, not when the file opens.
- **Bar.** Top right of the reader, at most 560 px wide (90% of a narrow
  window): an icon, the field, a count, previous and next, close, and a
  results toggle. Typing waits 180 ms. Enter goes to the next hit, Shift+Enter
  to the previous; both wrap. Escape or a press outside closes it and keeps
  the query.
- **Count.** Empty with no query, "0/0" with no hits, "n" before a step,
  "i/n" after one.
- **Results.** One row per hit: a "p. N" badge over the snippet, the
  current row tinted, "No results" when there are none.
- **Hits.** Every hit is the accent at 30% (26% on dark pages), radius 2.
  The current one is amber (#f59e0b, #fbbf24 on dark) at 58% (46%) with a
  1.5 px ring at 85%.
- **Reveal.** A hit already in view stays put, inside the insets plus a
  24 px margin (48 px sideways). Otherwise it scrolls into view. Paged
  modes turn to its page.

## What we do differently

- **Across lines.** A query matches across line breaks and words hyphenated
  at a line's end ("docu-" "ment" finds "document").
- **Folding.** Accents, curly quotes and ligatures fold, so "cafe" finds
  "café" and "fi" finds the "ﬁ" ligature, which is spelled out when its
  page is read.
- **From where you read.** The first Enter goes to the first hit on or after
  the page read, not the first in the document. It waits for that page to be
  indexed, or for the whole document.
- **While indexing.** Hits arrive as pages are read; the count ends in "…"
  until every page is in. Indices never shift, as later pages only append.
- **A cap.** At most 10,000 hits are kept; the count ends in "+" past it.

## Where it lives

| Piece | Home | Why |
|---|---|---|
| Folding, a page's searchable text, matching, snippets, stepping and the reveal rule | `crates/search` (L0) | pure, no dependencies, tested alone; the library's search folds with it too |
| Per-document indexes, threads, hit painting | `crates/ui/src/search.rs` | owns the `Document`, wakes egui |
| The bar, count, results list, debounce and steps | `crates/ui/src/find_bar.rs` | egui |
| Revealing a hit | `PdfView::seek`, `Documents::reveal_hit` | the view knows its layout and scroll |

## The index

`Search` keeps one job per open document, keyed by the `Document` it reads,
so a re-typeset book starts again. The three most recently searched are
kept; dropping one ends its thread.

The thread reads pages in order, turning each text layer into a `Page`:
folded characters with a line number per glyph and a map back to the
layer's characters. A new query answers at once over the pages read, then
keeps reading, sending each page's hits as they turn up. Only the newest of
several queued queries is answered. Progress is reported every 16 pages, so
idle frames aren't woken for each page.

Tests and the browser run inline (`workers=0` and wasm): the first query
reads every page on the spot.

## The bar

Cmd/Ctrl+F opens the bar over the focused pane; with nothing open it
searches the library instead. Its hits paint only in that pane, and only
while it is open. A step sets the current hit and asks its pane to show it,
retrying for up to 90 frames while a page's text is still being read.

The reveal leaves 72 px clear at the top for the bar, and 24 px at the
other edges. It moves along both axes, so a zoomed-in page also scrolls
sideways to the hit.

## Slices

| # | Slice | Check |
|---|---|---|
| 1 | Document search: the `search` crate, threaded lazy index, bar with count, steps and results, hit painting, reveal | `crates/search` unit tests; `crates/ui/tests/find.rs` |
| 2 | Keyboard-only operation and focus rings | planned |
| 3 | Auto-scroll | planned |
