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
| M5 | Library | library crate per `docs/library.md`: sources (read in place, rescans, follow moves), store copies, shelves, grid and list, search, drag, menus, covers, resume; drop-to-split |
| M6 | Study tools | Gloss cards and marks, CEFR highlighting, offline dictionaries, selection menu |
| M7 | Search and keyboard | Document search for both formats, lazy index, auto-scroll, full keyboard operation |
| M8 | Polish and 1.0 | Every P0 and P1 shipped; memory budgets held; packaging for macOS, Windows, Linux |

A milestone is done when every P0 entry for it is `shipped` and the P1 entries
are `shipped` or explicitly deferred with notes. Parity means the capability,
not mareader's pixels or code structure (ADR 0005).

## Next

1. **M1 settings crate** (`crates/settings`, L2): every mareader setting, in our
   own schema with defaults (`../mareader/crates/reader-core/src/settings/` for
   the list and ranges).
2. **M1 sidebar** (`chrome.sidebar`): `egui::Panel::left(..).show_collapsible`,
   docked or overlay; feeds the title bar's band inset.
3. **M1 popovers and the settings window** (`chrome.popovers`,
   `settings.modal`): `egui::Popup` and `egui::Modal`.
4. **M1 glass blur** (`chrome.glass-blur`): a wgpu backdrop blur behind the bar.

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
