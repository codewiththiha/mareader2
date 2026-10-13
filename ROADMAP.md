# Roadmap

**Stage:** M1 in progress (slices 1 to 3 of 6 done).
**Parity:** 17.0 of 111 features (15.3 %, partial counts half). Run
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
| 5 | Tooltips and toasts styled; overflow menu at narrow widths | planned | narrow the window |
| 6 | Glass blur behind the title bar (wgpu pass) | planned | pin the bar over a page |

## Next

1. **Slice 5, tooltips, toasts and the overflow menu.**
2. **Slice 6, the glass blur pass.**

## Open questions for the owner

- **macOS traffic lights.** Moving them needs AppKit calls, and the workspace
  forbids `unsafe`. Accept the default position (the bar leaves 80 pt), or allow
  one audited `objc2` call?
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
