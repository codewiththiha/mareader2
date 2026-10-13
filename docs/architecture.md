# Architecture

> **Status:** M0 (foundation) · **Last updated:** 2026-10-13

mareader2 is mareader rebuilt as one native Rust process: egui for the
interface, wgpu for drawing, hayro for PDF, and our own EPUB pipeline. There are
no webviews, iframes, JS realms, pdf.js or Tauri. The look and behaviour come
from mareader (`docs/look.md`, `parity/`). The structure comes from what egui is
good at.

## Layers

Lower layers never see egui. `cargo xtask layers` enforces this from
`xtask/src/layers.rs`, which also lists planned crates so a new crate lands in
the right place.

| Layer | Crate (`mareader-…`) | State | Owns |
|---|---|---|---|
| L0 | `appearance` | **exists** | base mode, tint, texture, grain, presets; colour maths; the page filter chain as matrices |
| L0 | `workspace` | **exists** | the pane split tree (≤ 4), layout, dividers, drop edges |
| L1 | `pdf` | planned M2 | hayro: open, page sizes, tile raster, text and glyph boxes, outline, links, search index |
| L1 | `reflow` | planned M4 | styled blocks → parley lines → pages; typography knobs; hit-testing |
| L1 | `library` | planned M5 | books, shelves, watched folders, ledger, fingerprints (port of `library-core`) |
| L1 | `study` | planned M6 | dictionary packs, CEFR levels, gloss request/response types (ports of `dict-core`, `cefr-core`, `ai-core`) |
| L2 | `epub` | planned M4 | rbook container, spine, ToC, XHTML+CSS subset → `reflow` blocks |
| L2 | `settings` | planned M1 | the persisted schema, defaults and migration (port of `reader-core/settings`) |
| L3 | `session` | planned M2 | a document open in a pane: render jobs, caches, cancellation, teardown |
| L7 | `ui` | **exists** | egui: theme, icons, title bar, panes, every view and popover |
| — | `apps/mareader` | **exists** | the window and `main` |
| — | `xtask` | **exists** | gates |

The rules:
1. A crate depends only on lower layers. Same-layer edges need an entry in
   `SIDEWAYS`.
2. egui, eframe, winit, wgpu and rfd appear only in L7 and the app.
3. L0–L3 compile for `wasm32-unknown-unknown`. A gate for this arrives with the
   first L1 crate.

## Frame model

eframe calls `App::logic` and then `App::ui` once per frame, and only when
something changed. egui is immediate mode, so the state lives in plain structs
owned by `MareaderApp`, not in signals or a DOM.

- **`logic`** applies settings changes (restyle, motion switch) and drains
  finished background jobs into state.
- **`ui`** paints in this order:
  1. the content;
  2. the panes;
  3. the floating title bar, popovers and toasts.

  Interactions return action values (`BarAction`, `PaneAction`), and the app
  applies them after painting. This keeps one owner per state change.
- **Repaint is demand-driven.** Every deferred thing schedules its own wake with
  `request_repaint_after`: the bar's hide grace, a toast's timeout, a job
  finishing. Nothing polls, and an idle window costs zero frames. This is
  mareader's "every deferral has a guaranteed wake" rule.

## The page pipeline: every page is a texture

A page from either format ends up as GPU textures that one compositor draws.
`docs/adr/0003-pages-are-textures.md` gives the reasoning.

```
PDF:  hayro → RGBA tile ─┐
                         ├─▶ TextureHandle (LRU, capped) ─▶ page pass (wgpu callback)
EPUB: parley → vello_cpu ┘                                    filter · blend · texture · grain
```

- **Raster off the UI thread.** A small worker pool renders tiles at the
  settled scale. During a zoom, the existing texture stretches and is replaced
  once, crisp. This is mareader's "stretch, then one crisp re-render".
- **The page pass** is an `egui_wgpu::CallbackTrait` paint callback (M3). It
  takes `Appearance::page_filter()` as a ≤ 7-entry matrix uniform
  (`MAX_FILTER_OPS`) and `page_blend()`, and composites the texture tile and the
  grain. `apply_filter` in `crates/appearance` is its CPU reference, and tests
  compare the two.
- **Blend backdrop** (PDF): the dominant paper colour of the WholePage or
  10 pt Edges area is sampled from the raster. The pane background takes that
  colour so the page melts into it.
- **Text overlays are geometry, not DOM.** Selection, search hits and gloss
  marks are rectangles from the page's glyph index (hayro text extraction or
  parley clusters), painted over the texture.

## EPUB

The EPUB pipeline replaces mareader's txt/md pipeline. `docs/epub.md` has the
plan, and `docs/adr/0002-epub-via-parley.md` the choice. In short: rbook reads
the container, a small XHTML+CSS subset becomes styled blocks, parley lays out
and paginates them, and vello_cpu (the same rasteriser hayro uses) draws page
textures. The mareader typography knobs map one-to-one.

## Threads and resources

- **UI thread:** egui only. It never blocks on I/O or raster work.
- **Workers:** raster and parse jobs carry a generation number. A result for a
  closed pane, an old zoom or an old document is dropped at the queue edge.
  This is mareader's stale-result rejection.
- **Teardown is explicit.** Closing a pane drops its `session`, which:
  - cancels its jobs;
  - frees its textures (`TextureHandle` drop);
  - releases its document.

  A test proves the texture count returns to the baseline.
- **Caps everywhere.** Tile LRU by bytes, thumbnails LRU of 16 (mareader's
  number), and the glyph index built on first search, not at open.
- **Memory claims need measurements:** what was measured, how, the workload,
  before and after, and known limits. Record them in `docs/memory.md`. The earlier
  benchmark is the starting baseline. At 150 %, native hayro peaked 12–118
  MiB above baseline, about 3–15× less than pdf.js on light files and 2–4× less on heavy ones.
  That run was in Node, not a webview, so the comparison is indicative only.

## Window chrome

- **macOS:** full-size content view, title bar and title hidden, native traffic
  lights.
- **Windows and Linux:** `with_decorations(false)`, our own captions, and 5 pt
  resize edges.
- Behaviour is in `docs/look.md` § Title bar.
- Transparency and rounded window corners are deferred to an ADR in M1. They
  differ per platform and compositor.

## Decisions

| ADR | Decision |
|---|---|
| [0001](adr/0001-egui-wgpu-native.md) | Native egui + wgpu in one process; no webview |
| [0002](adr/0002-epub-via-parley.md) | EPUB via rbook + our XHTML subset + parley, not a browser engine |
| [0003](adr/0003-pages-are-textures.md) | Both formats rasterise to textures; one GPU page pass applies the look |
| [0004](adr/0004-lucide-icons.md) | Lucide via egui-lucide behind an `Icon` facade, tinted, never recoloured |

## Revision history

| Date | Summary |
|---|---|
| 2026-10-13 | Created with the M0 foundation |
