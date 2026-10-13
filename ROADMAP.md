# Roadmap

**Stage:** M0 foundation complete. Next: M1 shell and chrome.
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
| M5 | Library | library crate: shelves, read-in-place folders, watched rescans, fingerprints, grid and list, search, drag, menus, conflicts, covers, resume; drop-to-split |
| M6 | Study tools | Gloss cards and marks, CEFR highlighting, offline dictionaries, selection menu |
| M7 | Search and keyboard | Document search for both formats, lazy index, auto-scroll, full keyboard operation |
| M8 | Polish and 1.0 | Every P0 and P1 shipped; memory budgets held; packaging for macOS, Windows, Linux |

A milestone is done when every P0 entry for it is `shipped` and the P1 entries
are `shipped` or explicitly deferred with notes.

## Next

1. **M1 settings crate** (`crates/settings`, L2): mareader's settings fields and
   defaults, reading `mareader.settings.v1` JSON unchanged
   (`../mareader/crates/reader-core/src/settings/`).
2. **M1 sidebar** (`chrome.sidebar`): the rail, docked or overlay, 300 ms slide;
   feeds the title bar's band inset.
3. **M1 popovers and the settings window** (`chrome.popovers`,
   `settings.modal`).
4. **M1 glass blur** (`chrome.glass-blur`): a wgpu backdrop blur behind the bar.

## Open questions for the owner

- **macOS traffic lights.** Moving them needs AppKit calls, and the workspace
  forbids `unsafe`. Accept the default position with the 88 pt gutter, or allow
  one audited `objc2` call?
- **Sepia magenta.** mareader's sepia tint takes the short arc through magenta,
  so the chrome turns pink. mareader2 matches it; fix in both apps or neither.
- **EPUB spike (M4).** The three questions at the end of ADR 0002.

## Progress log

| Date | Change |
|---|---|
| 2026-10-13 | M0: foundation repository — appearance and workspace crates, egui shell with the mareader title bar on three platforms, xtask gates, parity file (111 features), docs and ADRs |
