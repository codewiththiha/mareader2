# Roadmap

**Stage:** M1 done (the glass moved to M2 slice 3); M2 slices 1 to 5 done.
**Parity:** 30.5 of 118 features (25.8 %, partial counts half). Run
`cargo xtask parity` for the current numbers; this line is updated each session.

## Milestones

| | Milestone | Done when |
|---|---|---|
| M0 | Foundation | Workspace, layering, gates, CI; appearance model and tokens; pane tree; title bar with hover, pin and three platforms' captions; the shot tool and kittest; docs and parity file |
| M1 | Shell and chrome | Sidebar rail, popovers, tooltips, toasts, overflow menu, settings window skeleton, the settings crate (mareader's schema and JSON), glass blur pass, title centre slot, band inset, boot paper |
| M2 | PDF viewing | pdf and session crates; hayro off the UI thread; single, spread, vertical and horizontal views; zoom presets, fit, anchor; selection, links, outline, thumbnails, page counter; shortcuts; teardown and memory measured |
| M3 | Appearance on pages | The wgpu page pass (filter chain, blends), textures, grain, blend backdrop, presets UI and the appearance menu; independent pane themes |
| M4 | EPUB | epub and reflow crates: rbook + XHTML subset + parley + hypher; pagination and the four view modes; every typography knob; TOC, links, position anchors; hostile-input tests |
| M5 | Library | library crate per `docs/library.md`: sources (read in place, rescans, follow moves), store copies, shelves, grid and list, search, drag, menus, covers, resume; drop-to-split |
| M6 | Study tools | Gloss cards and marks, CEFR highlighting, offline dictionaries, selection menu |
| M7 | Search and keyboard | Document search for both formats, lazy index, auto-scroll, full keyboard operation |
| M8 | Polish and 1.0 | Every P0 and P1 shipped; memory budgets held; packaging for macOS, Windows, Linux |

A milestone is done when every P0 entry for it is `shipped` and the P1 entries
are `shipped` or explicitly deferred with notes. Parity means the capability,
not mareader's pixels or code structure (ADR 0005).

## Checking progress

Every slice is usable as soon as it lands (`AGENTS.md` § Always checkable):

- **Desktop:** `cargo run -p mareader`.
- **Browser (best effort, a viewer for sandboxed sessions):**
  `tools/web.sh --serve 8080`, then open the page. URL options
  pick a state: `?preset=sepia`, `?theme=dark`, `?panes=3`,
  `?platform=windows`, `?open=appearance`, `?pinned=true`.
- **Screenshots:** `cargo run -p mareader-ui --example shot -- out.png` with the
  same keys.

## M1 slices

| # | Slice | State | Try it |
|---|---|---|---|
| 1 | Appearance popover: presets, mode, tint, texture, grain; live restyle | **done** | palette button, or `?open=appearance` |
| 2 | Sidebar rail: docked slide or floating with edge reveal; switcher; band inset | **done** | sidebar button, `?sidebar=outline`, `?float=true` then the left edge |
| 3 | Settings window (`Modal`): tab strip; Layout, Theme, Animations, Workspace carry today's settings; later rows named with their milestone | **done** | gear button, `?settings=theme&panes=2` |
| 4 | Settings crate: every mareader setting in our schema, persisted | **done** | change a setting, quit, relaunch |
| 5 | Tooltips and toasts styled; overflow menu at narrow widths | **done** | narrow the window, `?open=menu`, `?toast=Saved` |
| 6 | Glass blur behind the title bar (wgpu pass) | **moved to M2 slice 3** | it needs pages to blur and reuses the page pass |

## M2 slices

Owner decision (2026-10-13): the glass waits for pages. M2 draws pages through
a minimal page pass from slice 3, which the glass reuses; M3 adds the
appearance effects to that same pass.

| # | Slice | State | Try it |
|---|---|---|---|
| 1 | Open a PDF and scroll it: `pdf` crate (hayro), `session` (shared documents, worker pool, generations), virtualised vertical scroll at fit width, capped texture cache, the title from the document | **done** | `cargo run -p mareader -- book.pdf`, drop a PDF, `?pdf=sample` |
| 2 | Zoom: fifteen presets, fit width and page, centre anchor, stretch then one crisp render, refit on resize and sidebar slide | **done** | ⌘/Ctrl +/−/0, bare +/−, pinch, the menu's zoom row; `zoom=in,page` in shots |
| 3 | The page pass (a wgpu callback drawing page textures) and the glass bar blur | **done** | pin the bar and scroll a page under it; `pinned=true scroll=300` in shots |
| 4 | View modes: single page with page turn, spread, horizontal strip | **done** | the menu's view row, ⌘/Ctrl+1 and +2, arrows; `view=spread arrows=right` in shots |
| 5 | Navigation: page counter with entry, outline, thumbnails, status bar | **done** | the sidebar's outline and thumbnails, hover a pane's bottom edge; `sidebar=outline goto=17 hover=x,y` in shots |
| 6 | Text selection and copy; internal and external links | planned | drag over text, click a link |
| 7 | Keys and glide, last document on launch, loading mark, teardown and memory measured | planned | `docs/memory.md` |

## Next

1. **M2 slice 6, selection and links.**
2. **M2 slice 7, keys, glide, last document, teardown and memory.**

## Open questions for the owner

- **EPUB spike (M4).** The open questions at the end of ADR 0002 (glyph-run
  drawing, reading fonts).

## Progress log

| Date | Change |
|---|---|
| 2026-10-13 | M0: foundation repository — appearance and workspace crates, egui shell with the mareader title bar on three platforms, xtask gates, parity file (111 features), docs and ADRs |
| 2026-10-13 | Functional parity (ADR 0005): `docs/design.md` replaces `look.md`; `Command` queue and egui-native widgets in the shell; one-hue tints with gamut mapping; library redesign; pane controls kept clear of the hover band |
| 2026-10-13 | Theme pipeline: shape tokens, one apply, instant restyle (tested), live token file; dividers through commands; ADR 0006 shares documents and caches across panes |
| 2026-10-13 | M1 slice 1: Appearance popover (presets, mode, hue strip, strength, texture, grain) restyles live; browser preview (`apps/mareader-web`, `tools/web.sh`); scrolling rules for long documents recorded |
| 2026-10-13 | M1 slice 2: sidebar rail, docked (300 ms slide, panes give way) or floating (fade, 6 px edge reveal, 250 ms grace); header with close, search, settings and dock; panel switcher; Escape closes; the bar's band and macOS gutter start at the rail. The slide is our own eased width inside `egui::Panel::left`, because `show_collapsible` follows the global animation time and cannot honour the separate slide switch |
| 2026-10-13 | M1 slice 3: settings window as an `egui::Modal` (620 × 640 at most; Escape and backdrop close it, and Escape no longer reaches the sidebar). Tab strip as in mareader (Animations only while the master is on, Fonts only for EPUB). Real rows: keep title bar shown, overlay sidebar, Animations, sidebar animation, split-pane look (outline thickness and colour, gap, corners, shadow). Every row of a later feature is a muted line naming its milestone, not a dead switch. Edits apply on the same frame |
| 2026-10-13 | M1 slice 4: `crates/settings` (L2) holds every mareader setting, grouped by feature (chrome, layout, paper, motion, text, study, dictionary, workspace) with mareader's defaults and ranges. One schema `version` replaces mareader's one-shot gates; a migration step reads the shell's earlier blob. Loading never fails: a bad section resets alone, with a note. `import_mareader` maps mareader's JSON, halving pre-curve tints. Outline and gloss share one `Swatch` palette. Saved through eframe storage at most every 500 ms, on its persist thread, only when changed |
| 2026-10-13 | M1 slice 5: tooltips are a themed chip (6 pt radius, small text) naming the shortcut when one exists. Toasts: one slot, bottom centre of the panes, 3.5 s, info or error tone, hover holds them, a dismiss button; used for refused splits and repaired settings. The View & tools menu at the end of the bar holds full screen (F11, ⌃⌘F on macOS), a shortcut list and the version; Settings, Search and the splits fold into it, leftmost first, when the bar lacks room. `keymap.rs` is the one table behind keys, tooltips and the list; ⌘, (Ctrl+,) opens settings |
| 2026-10-13 | Owner decisions: independent twins stay (a second book id on the same file, `docs/library.md`); `unsafe` is allowed for the macOS traffic lights only. They now sit centred in the bar and fade with it (`apps/mareader/src/traffic_lights.rs`, feature `traffic-lights`); `cargo xtask unsafe` keeps every other file free of it |
| 2026-10-13 | Traffic lights follow the sidebar: while a rail is drawn they sit in its header at full opacity and slide out with it; otherwise they come with the bar, and in overlay mode only with the rail (the bar then drops its gutter). The floating rail now slides like the docked one instead of fading. Off a Mac, `platform=mac` draws stand-in dots so this shows in shots and the preview |
| 2026-10-13 | M1 closed; the glass moved into M2 (owner). M2 slice 1: `pdf` wraps hayro (open, page sizes, rasters, the title rule from mareader, a generated sample PDF); `session` shares documents across panes and runs a worker pool driven by per-frame demand, dropping stale jobs and results, capped at 32 finished items. Panes scroll a virtualised fit-width column with a 256 MiB texture cache; files open from the command line or by drop, `pdf=sample` in shots and the preview |
| 2026-10-13 | Owner: the traffic lights work on a real Mac; "optional" meant optional to build, so the `traffic-lights` feature stays on with no in-app setting. `rfd` approved for native dialogs (imports will need it too): it added only `rfd` and `pollster` to the tree. ⌘O / Ctrl+O, the menu's Open… and Open beside… (split right, only once a file is picked) and the empty pane's button open the system dialog off the UI thread. In the browser, picks and drops arrive as bytes, so real PDFs open in the preview |
| 2026-10-13 | M2 slice 2, zoom: mareader's fifteen presets (a step from 120 % goes to 125 %, not past it), fit width and fit page measured on the page read when chosen, refit as the pane resizes. Every relayout holds the point under the pivot: the window centre for keys and the menu, the pointer for pinch and Ctrl+wheel (new on desktop). Steps glide 120 ms and render sharp at once; pinches and resizes stretch, then render once after 180 ms of quiet. egui's own interface zoom keys are off |
| 2026-10-13 | M2 slice 3: pages leave egui's texture manager for our page pass (`crates/ui/src/gpu`): rasters upload from the UI thread through egui's own device, and each pane draws its pages in one wgpu callback of instanced quads, which ADR 0003's filters will extend in M3. The glass redraws the strip under the bar and 60 pt below at about a tenth of the resolution, 4 × 4 supersampled so scrolled text doesn't shimmer, blurs it twice (σ 20 pt), and composites it at 180 % saturation under a 72 % surface, fading with the bar. Without a GPU (headless tests) pages fall back to egui textures and the bar to 96 %. Tested on pixels from a real wgpu device, including a check that fails if the blur is skipped |
| 2026-10-13 | M2 slice 4, view modes (`crates/ui/src/layout.rs`): one `Layout` places pages for the vertical column, the horizontal strip, a single page and a two-page spread (1–2, 3–4, touching at the spine), so zoom anchoring, prefetch and the page pass work in all four. A switch keeps the page being read; entering a spread fits both widths, single page fits when auto-scale is on, and the strip stops fitting, as in mareader. mareader's arrow rule: every arrow and h/j/k/l turns, except along a strip's own axis (left for later glide). ⌘/Ctrl+1 single, ⌘/Ctrl+2 vertical; the menu has a view row. On the strip a plain wheel runs sideways while nothing overflows down. Not yet saved across launches (slice 7) |
| 2026-10-13 | M2 slice 5, navigation. The `pdf` crate reads `/Outlines` at open (direct, GoTo and named destinations, loops and depth capped); the sample PDF carries chapters. The rail's Outline panel marks the deepest entry holding the page (a click wins while its page is read), revealed when off screen and centred on opening or a re-click of the panel button. The Thumbnails panel is a two-column grid, only near rows drawn, following the page with a glide that waits 1.2 s after the reader browses it; thumbnails share the one page cache (ADR 0006) instead of mareader's separate LRU. Each pane gets a corner counter, a progress line and a bottom bar raised from its edge: previous and next, page entry, count and slider. Settings, Layout: indicator on or off and its style, progress bar. The bottom bar uses the bar's plain frost: the glass pass draws one strip per frame |
