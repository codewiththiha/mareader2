# EPUB: the plan

> **Status:** planned (M4) · **Last updated:** 2026-10-13

EPUB replaces mareader's `.txt`/`.md` pipeline. Everything mareader's text
formats can do, an EPUB page must do too. The choice of stack is in
[ADR 0002](adr/0002-epub-via-parley.md).

## Pipeline

```
.epub ──rbook──▶ spine + ToC + resources
          │
          ▼  html5ever + CSS subset           (crate: epub, L2)
     reflow::Block[]  (heading, para, list, quote, pre, table, image, rule)
          │
          ▼  parley 0.12 + fontique            (crate: reflow, L1)
     lines → pages   (real line heights; never orphan a heading)
          │
          ▼  vello_cpu 0.3
     page RGBA → texture → page pass (texture + grain; no filter)
```

- **Lazy by chapter.** Only the visible spine items and their neighbours are
  parsed and laid out. Pagination is estimated first and refined as chapters are
  laid out. This is the same contract as mareader's measured pagination: the
  reading position anchors to a block, so a re-cut keeps your place.
- **Zoom never re-paginates.** Pages scale uniformly, as in mareader. A
  typography change re-paginates and keeps the anchor block in view.
- **Untrusted input.** Paths in the container resolve only inside it. XML depth,
  entity expansion, image dimensions and the total text size are capped. No
  network, ever: remote images and fonts are refused. Scripts are ignored.

## Typography knobs

`TextSettings` from `reader-core/src/settings/typography.rs` ports as-is,
serde names included, so mareader settings JSON loads. The defaults are
mareader's.

| Knob | Default | parley / reflow mapping |
|---|---|---|
| `font_size` | 17 px | `StyleProperty::FontSize` on the body; headings scale from it |
| `font_weight` | 400 | `FontWeight` (variable axis when the face has one) |
| `line_height` | 1.7 | `LineHeight` as a font-size multiple |
| `paragraph_margin` | 1.0 em | block spacing in `reflow` |
| `word_spacing` | 0 px | `WordSpacing` |
| `letter_spacing` | 0 em | `LetterSpacing` (× font size) |
| `text_indent` | 0 em | first-line indent in `reflow` |
| `justify` | off | `Alignment::Justify` (last line start-aligned) |
| `hyphenation` | off | `hypher` break points (ADR 0002, open question 1) |
| `default_font`, `serif_font`, `sans_font`, `mono_font` | Default stacks | fontique families: bundled OFL faces first, then system |
| `book_layout` | off | spread pages get a spine-side gutter |
| `column_align` | centre | where the column sits in the vertical stream |
| `ink_contrast` | mareader's default | mixes the ink token towards paper for body text only |

Publisher CSS supplies structure (emphasis, alignment, block margins). A knob
the user set always wins over it.

## View modes

- **Single and spread:** pages as above.
- **Horizontal strip:** page by page.
- **Vertical:** one continuous column with no visible page breaks (mareader's
  streaming mode), still rasterised in page-height bands so memory stays bounded.

## Features that ride on the layout

- **Search:** scans the chapter text in-process. Hits are cluster rectangles in
  page space, with the current hit in amber `#f59e0b`.
- **Selection and gloss:** hit-test pointer → cluster → word. The word and its
  sentence go to the study tools (M6).
- **Links:** internal hrefs map to spine item + anchor → page. External links
  open in the browser after the same scheme checks as mareader (`javascript:`
  and `file:` refused).
- **ToC:** from nav/NCX, flattened with depth. The active entry follows the
  reading position, and the deeper entry wins on shared pages.
- **Covers:** the OPF cover image, for the library.

## Revision history

| Date | Summary |
|---|---|
| 2026-10-13 | Created with the M0 foundation |
