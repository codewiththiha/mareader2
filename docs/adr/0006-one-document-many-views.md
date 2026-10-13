# ADR 0006: one document, many views

- **Status:** accepted, 2026-10-13
- **Context:** In mareader each pane is an iframe with its own JS realm and
  WASM instance. That isolation existed because a browser realm's memory
  never shrinks; disposing the realm was the only way to get memory back. So
  every pane parsed its own copy of a book, kept its own caches and had its own
  budget. In one native process, memory is freed on drop, and nothing forces
  that duplication.
- **Decision:**
  - A book opens once into an `Arc<Document>` kept in the documents table
    (`crates/session`). hayro's `Pdf` holds its cross-reference table in an
    `Arc`, so worker threads share it.
  - A pane is a view: document handle, scroll, zoom, selection, search state.
    It is drawn in a scoped `Ui` region; that is egui layout, not isolation.
  - One byte-capped tile cache, keyed by (document, page, scale, tile), serves
    all panes. One worker pool schedules by priority: active pane's visible
    tiles, other visible tiles, prefetch.
  - The last view to close drops the `Arc`; its jobs are cancelled and its
    tiles evicted.
- **Consequences:**
  - The same book in two panes costs one parse and one glyph index; tiles are
    shared where zooms match.
  - The memory budget is the app's, not multiplied by the pane count.
  - A crash in a parser can't be contained per pane. hayro and our EPUB code
    run on workers and return errors; a panic on a worker is caught at the job
    boundary and reported as a failed page.
  - Tests: texture count returns to the baseline after the last view closes;
    a second pane on the same book adds no parse.
