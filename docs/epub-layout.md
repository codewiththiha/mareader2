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
- **Float** (L4): the floated element a block is in. Its blocks shrink to
  their widest line at one side; the text after it breaks line by line
  past it, and pictures or other boxes start below it.

Boxes and rows are shared by their blocks, so a box is known by its
address. Pagination keeps the boxes open on the page; a page break cuts
a box's border there and reopens it overleaf. Borders are drawn in the
theme's ink. A row taller than a page reads down the page instead.

Backgrounds are drawn in the theme's ink too, as strong as the book's
colour contrasts with its own page: a white line on a black page is a
full-ink line on ours. Under text a background stays faint, so the text
reads. Background pictures keep their colours, with `background-size`,
`-position` and `-repeat`, clipped to the box. A set `height` holds a
box open below short content.

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

Designed books are made for a phone's page, about 26 of their ems wide.
A designed chapter is set as if the column were that wide, so its type
grows with a wide pane, up to 1.6 times the reader's size and never
below it. Prose chapters keep the reader's size.

## Slices

| Slice | Scope |
| --- | --- |
| L1 | Cascade engine; Look (sizes, line height, spacing, indent, weight, embedded fonts, justify, margins, spacer paragraphs); designed-chapter test; Book design setting. |
| L2 | Boxes: borders (solid, double, dashed, dotted; per side; radius), padding, widths, auto margins, rules from bordered empty blocks, vertical centring of tall flex or `vh` containers. Done, with round corners from `border-radius`. |
| L3 | Grids: tables with block cells, column widths (fixed, %, content), cell padding, borders, `vertical-align`, spans, nesting; flex rows (justify, align, wrap, gap); anchored text in cells. Done: tables with block cells, fixed and % widths shrunk to fit, cell boxes, `vertical-align`, nested tables read flat, one-column tables as framed boxes, column widths from content (least and most widths, shared by every row), `colspan` and `rowspan`, flex rows (justify, align, wrap, gap, grow, basis). A flex row holding one layout table lets the table be the grid. |
| L4 | Floats with text wrapping, inline images, `break-before`/`page-break-*`, decorations, small caps, sub and super, background pictures. Done: design scale, floats (left and right) with text wrapping line by line, `break-before`/`break-after`, underline and line-through, small caps and `text-transform`, sub and super, background colours and pictures, box heights, small pictures set inside a line (at most 3 ems tall), `transform: rotate()` drawn about the block's middle. |
| L5 | Per-book override, both samples and the test corpus checked, docs, parity. |
