# Roadmap

**Stage:** M1 in progress (slice 1 of 6 done).
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
- **Browser:** `tools/web.sh --serve 8080`, then open the page. URL options
  pick a state: `?preset=sepia`, `?theme=dark`, `?panes=3`,
  `?platform=windows`, `?open=appearance`, `?pinned=true`.
- **Screenshots:** `cargo run -p mareader-ui --example shot -- out.png` with the
  same keys.

## M1 slices

| # | Slice | State | Try it |
|---|---|---|---|
| 1 | Appearance popover: presets, mode, tint, texture, grain; live restyle | **done** | palette button, or `?open=appearance` |
| 2 | Sidebar rail (docked or overlay), band inset | next | sidebar button |
| 3 | Settings window skeleton (`Modal`) with appearance and chrome pages | planned | gear button |
| 4 | Settings crate: every mareader setting in our schema, persisted | planned | settings survive a restart |
| 5 | Tooltips and toasts styled; overflow menu at narrow widths | planned | narrow the window |
| 6 | Glass blur behind the title bar (wgpu pass) | planned | pin the bar over a page |

## Next

1. **Slice 2, the sidebar** (`chrome.sidebar`):
   `egui::Panel::left(..).show_collapsible`, docked or overlay; feeds the title
   bar's band inset.
2. **Slice 3, the settings window** (`settings.modal`): `egui::Modal`.

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
| 2026-10-13 | Theme pipeline from ArtCraft: shape tokens, one apply, instant restyle (tested), live token file; dividers through commands; ADR 0006 shares documents and caches across panes |
| 2026-10-13 | M1 slice 1: Appearance popover (presets, mode, hue strip, strength, texture, grain) restyles live; browser preview (`apps/mareader-web`, `tools/web.sh`); scrolling lessons from PdfCraft recorded |
