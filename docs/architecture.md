# Architecture

> **Status:** M0 (foundation) · **Last updated:** 2026-10-13

mareader2 delivers what mareader does — the same features, the same reading
experience, the blend backdrop, panes, appearance, library and study tools — as
one native Rust process: egui for the interface, wgpu for drawing, hayro for
PDF and parley for EPUB.

It is a rewrite, not a port (ADR 0005). mareader is the specification of *what*
the app does. *How* is designed fresh for egui: much of mareader's structure
exists to work around a browser (iframes per pane, a frame transport, reactive
signals, DOM measurement, pdf.js memory latches) and has no reason to exist
here.

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
| L1 | `library` | planned M5 | books, locations, shelves, sources, scans (`docs/library.md`) |
| L1 | `study` | planned M6 | dictionary packs, CEFR levels, gloss requests and marks |
| L2 | `epub` | planned M4 | rbook container, spine, ToC, XHTML+CSS subset → `reflow` blocks |
| L2 | `settings` | planned M1 | the persisted schema and defaults; optional import of a mareader settings file |
| L3 | `session` | planned M2 | open documents shared by panes, the tile cache, the job scheduler |
| L7 | `ui` | **exists** | egui: theme, icons, title bar, panes, every view and popover |
| — | `apps/mareader` | **exists** | the window and `main` |
| — | `xtask` | **exists** | gates |

The rules:
1. A crate depends only on lower layers. Same-layer edges need an entry in
   `SIDEWAYS`.
2. egui, eframe, winit, wgpu and rfd appear only in L7 and the app.
3. L0–L3 compile for `wasm32-unknown-unknown`. A gate for this arrives with the
   first L1 crate.

## How the app is built in egui

egui is immediate mode: every frame, the UI is a function of the state. Write
it that way rather than as a web app in egui syntax.

**State.** One owner, `MareaderApp`, holds plain structs: settings, the pane
tree, the open documents, the library, which popover is open. Domain state lives in
the L0–L3 crates and has methods and tests of its own. egui's `Memory` keeps
only transient widget state (scroll offsets, hover animations, a text field's
cursor). There are no signals, effects, observers or stores, and no state is
mirrored in two places.

**Views are functions.** A view takes the state it shows and a `&mut Commands`:
`fn show(ui: &mut Ui, state: &T, out: &mut Commands)`. It draws and pushes
`Command`s (`crates/ui/src/command.rs`) for what the user asked. It does not
mutate app state, call other views' handlers or keep its own copy of the data.

**Commands are the only way to change state.** Buttons, menus, shortcuts and
drops all produce the same `Command` values, and `MareaderApp::apply` is the
one place they take effect, after the frame's UI pass. So a shortcut and its
button cannot drift apart, and every user action can be tested by pushing a
command.

**Frame order.**
1. `logic`: apply settings changes (restyle, motion switch), drain finished
   background jobs into state.
2. `ui`: panes, then the floating chrome (title bar, popovers, toasts) above
   them, each pushing commands.
3. Apply the commands.

**Use egui's tools, not re-creations of browser ones.**

| Need | egui tool | What mareader needed on the web |
|---|---|---|
| Sidebar, slide in and out | `egui::Panel::left(..).show_collapsible` | CSS transitions, layout observers |
| Popovers, menus | `egui::Popup`, `Ui::menu_button`, `Response::context_menu` | a floating layer, outside-click and Escape wiring |
| Settings window, confirmations | `egui::Modal`, `egui::Window` | modal components, focus traps |
| Tooltips | `Response::on_hover_text` | a tooltip component |
| Long lists and grids (thumbnails, library) | `ScrollArea::show_rows` / `show_viewport` | the `virtual-list` crates |
| Drag and drop inside the app | `Ui::dnd_drag_source` / `dnd_drop_zone`, `Response::dnd_*` | a custom pointer-drag engine |
| Files dropped from the OS | `ctx.input(|i| i.raw.dropped_files)` | Tauri drop events |
| Shortcuts | a keymap table of `KeyboardShortcut` → `Command`, `InputState::consume_shortcut` | key listeners per component |
| Theme | `Style` and `Visuals`, set once per appearance change | CSS variables |
| Animation | `Context::animate_bool_with_time` / `animate_value_with_time` where motion helps | CSS transitions on everything |
| Text layout in the chrome | `LayoutJob`, `Galley` (truncation, wrapping) | DOM measurement probes |
| Accessibility | AccessKit, through `WidgetInfo` on custom widgets | ARIA |

Stock widgets (`Button`, `Slider`, `ComboBox`, `Checkbox`, `TextEdit`) are
styled by the theme and used as they are. A custom widget is written only when
egui has nothing close (caption buttons, page views, library cards); it reads
its colours from `Tokens` or `Visuals` and gives AccessKit a label.

**Repaint is demand-driven.** Every deferred thing schedules its own wake with
`request_repaint_after` (the title bar's hide grace, a toast's timeout); worker
threads call `Context::request_repaint` when a result is ready. Nothing polls,
and an idle window renders no frames.

**Persistence.** Settings and the library are serde structs written through
eframe's storage or our own file with an atomic rename, debounced, off the UI
thread for anything large.

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
  once, crisp.
- **The page pass** is an `egui_wgpu::CallbackTrait` paint callback (M3). It
  takes `Appearance::page_filter()` as a ≤ 7-entry matrix uniform
  (`MAX_FILTER_OPS`) and `page_blend()`, and composites the texture tile and the
  grain. `apply_filter` in `crates/appearance` is its CPU reference, and tests
  compare the two. Appearance values are uniforms, so dragging a control
  re-renders no page (`docs/design.md` § How the theme works).
- **Blend backdrop** (PDF): the dominant paper colour of the whole page or its
  edges is sampled from the raster, and the pane background takes that colour
  so the page melts into it.
- **Text overlays are geometry.** Selection, search hits and gloss marks are
  rectangles from the page's glyph index (hayro text extraction or parley
  clusters), painted over the texture.

### Scrolling long documents

mareader2 scrolls 10,000-page files at low memory with no virtual-list
crate. The rules:

- **Layout from page sizes alone.** Page rects come from the document's page
  boxes, never from rendering. `ScrollArea::show_viewport` allocates the full
  content height; each frame draws and requests only the pages meeting the
  viewport plus a small prefetch window. A test opens 10,000 pages and checks
  that demand is the visible pages plus prefetch.
- **Visible work is always admitted.** Byte and count caps limit prefetch and
  what stays cached; the viewport already bounds what is on screen, so a
  visible page is never left blank to honour a cap.
- **Whole pages first, tiles only for huge rasters.** hayro interprets the
  whole page for each tile, so a page renders as one
  raster until its long side passes ~4096 px. Past that, a ≤ 2048 px backdrop
  plus at most four tiles per side, the tile side growing with zoom.
- **Hold tiles while panning.** New tiles are requested only after the scroll
  offset rests for ~0.25 s; meanwhile the backdrop or older tiles show,
  stretched. A drag never queues one interpret per frame.
- **Demand first, results bounded.** Each frame publishes its demand before
  receiving. A result no longer in demand is dropped before it becomes a
  `ColorImage`. Finished results wait in a bounded queue (~32 MiB, 32 items).
- **Upload budget per frame.** At most ~8 results, ~16 MiB or ~4 ms of texture
  uploads per frame, so a burst of finished tiles never stalls scrolling.
- **Stale stays until replaced.** Textures of an old zoom are drawn stretched
  until the sharp ones cover the page, then evicted.
- **Text layers are lazy.** Glyph data is extracted on the first selection or
  search, cached by count separately from rasters.
- **Touchpads scroll 1:1.** Precise wheel deltas bypass egui's easing
  or the page lags the fingers on Windows.
- **Colours never re-render.** Appearance is uniforms of the page pass, not
  a recolour while rasterising, so a colour change re-renders nothing.

## EPUB

EPUB replaces mareader's txt and md formats. `docs/epub.md` has the plan and
ADR 0002 the decision: rbook reads the container, a small XHTML+CSS subset
becomes styled blocks, parley lays out and paginates them, and vello_cpu (the
rasteriser hayro uses) draws page textures. Every mareader typography setting
has a parley equivalent.

## Threads and resources

- **UI thread:** egui only. It never blocks on I/O or raster work.
- **Workers:** raster, parse and scan jobs run on threads and report over
  channels. Each result carries the document id and generation it was made
  for; a result for a closed pane, an old zoom or an old document is dropped
  before it is uploaded.
- **One document, many views** (ADR 0006). A book opens once into an
  `Arc<Document>` held by the documents table; a pane holds only its view
  (document handle, scroll, zoom, selection). Two panes on one book share the
  parse, the glyph index and the tiles.
- **One cache, one budget, one pool.** A single byte-capped tile cache keyed by
  (document, page, scale, tile) serves every pane. One worker pool takes jobs
  in priority order: visible tiles in the active pane, then other visible
  tiles, then prefetch. Jobs carry a generation, so a superseded one is skipped.
- **Teardown is ownership.** When the last view of a document closes, its
  `Arc` drops, its pending jobs are cancelled and its tiles leave the cache. A
  test proves the texture count returns to the baseline.
- **Caps everywhere.** Tile cache capped by bytes, thumbnails by count, and the
  glyph index built on the first search, not at open.
- **Memory claims need measurements:** what was measured, how, the workload,
  before and after, and known limits, recorded in `docs/memory.md`. The starting
  baseline: at 150 %, native hayro peaked 12–118 MiB above baseline, about
  3–15× less than pdf.js on light files and 2–4× less on heavy ones. That run
  was in Node, not a webview, so the comparison is indicative only.

## Window chrome

- **macOS:** full-size content view, title hidden, native traffic lights.
- **Windows and Linux:** `with_decorations(false)`, our own caption buttons in
  the platform's style, and 5 pt resize edges.
- Behaviour: `docs/design.md` § Title bar.

## Decisions

| ADR | Decision |
|---|---|
| [0001](adr/0001-egui-wgpu-native.md) | Native egui + wgpu in one process; no webview |
| [0002](adr/0002-epub-via-parley.md) | EPUB via rbook + our XHTML subset + parley, not a browser engine |
| [0003](adr/0003-pages-are-textures.md) | Both formats rasterise to textures; one GPU page pass applies the look |
| [0004](adr/0004-lucide-icons.md) | Lucide via egui-lucide behind an `Icon` facade, tinted, never recoloured |
| [0005](adr/0005-functional-parity.md) | Match mareader's features, not its pixels or its structure |
| [0006](adr/0006-one-document-many-views.md) | A book opens once; panes are views sharing one cache and worker pool |

## Revision history

| Date | Summary |
|---|---|
| 2026-10-13 | Created with the M0 foundation |
| 2026-10-13 | Functional parity and the egui architecture section (ADR 0005) |
| 2026-10-13 | Shared documents, cache and workers across panes (ADR 0006); appearance as uniforms |
| 2026-10-13 | Scrolling long documents: admission, tiling and upload rules |
