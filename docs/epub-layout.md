# EPUB layout: the book's own design

Designed pages (colour plates, character pages, title pages, boxed notes)
are drawn as the book lays them out: its grids, boxes, rules, type sizes,
spacing and embedded fonts. Prose chapters keep the reader's typography.
Colours always follow the reader's theme; pictures keep their own.

## Decisions (2026-10-20)

| Question | Decision |
| --- | --- |
| Engine | Our own reflow engine grows a CSS box model; no web engine. |
| Colours | Book layout, theme paper and ink. Book colours are ignored. |
| When | Automatically per chapter; a setting can force it on or off. |

## Model

The stream stays flat: every paragraph is still a top-level block, so
anchors, highlights, search, CEFR marks, notes and page AI keep working.
Structure rides on each block's frame:

- **Look**: the computed text style of a designed block (size, line
  height, letter and word spacing, indent, weight, faces, justification)
  and the space above it, margins collapsed as CSS does.
- **Boxes** (L2): the styled boxes around a block, outermost first:
  margins, borders, padding, width and auto-margin centring. A box spans
  every block inside it and splits across pages.
- **Cells** (L3): a block's place in a grid (table or flex row). A grid
  row's blocks are laid side by side and paginate as one unit.

## Cascade

`css.rs` is a real cascade: compound selectors with descendant, child,
adjacent and sibling combinators, ids, classes, attributes, `:first-child`
and `:not()`, specificity, source order, `!important`, inline styles, a
small user-agent sheet, `@media` (screen, all), `@font-face`, inheritance
and computed lengths (px, pt, pc, in, cm, mm, em, rem, ex, ch, %, vh, vw).
Selectors needing later siblings (`:last-child`) don't match: the parser
streams.

## Designed chapters

A chapter counts as designed when its visible elements use layout CSS
(layout tables, flex or grid, borders, block backgrounds, block widths,
`vh` heights), or at least two lighter signals (embedded faces, wide
letter spacing, three or more body sizes). The **Book design** setting
picks Auto, On or Off.

## Slices

| Slice | Scope |
| --- | --- |
| L1 | Cascade engine; Look (sizes, line height, spacing, indent, weight, embedded fonts, justify, margins, spacer paragraphs); designed-chapter test; Book design setting. |
| L2 | Boxes: borders (solid, double, dashed, dotted; per side; radius), padding, widths, auto margins, rules from bordered empty blocks, vertical centring of tall flex or `vh` containers. |
| L3 | Grids: tables with block cells, column widths (fixed, %, content), cell padding, borders, `vertical-align`, spans, nesting; flex rows (justify, align, wrap, gap); anchored text in cells. |
| L4 | Floats with text wrapping, inline images, `break-before`/`page-break-*`, decorations, small caps, sub and super, background pictures. |
| L5 | Per-book override, both samples and the test corpus checked, docs, parity. |
