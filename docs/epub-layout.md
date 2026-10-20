# EPUB layout: the book's own design

Designed pages (colour plates, character pages, title pages, boxed notes)
are drawn as the book lays them out: its grids, boxes, rules, type sizes,
spacing and embedded fonts. Prose chapters keep the book's type (sizes,
weights, indents, alignment, margins) moved by the reader's settings, and
read in one column. Colours always follow the reader's theme; pictures
keep their own.

## Decisions (2026-10-20)

| Question | Decision |
| --- | --- |
| Engine | Our own reflow engine grows a CSS box model; no web engine. |
| Colours | Book layout, theme paper and ink. Book text colours show, adapted to the theme (request #5). |
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

## Text colours

`color` (and `<font color>`, `<body text>`) is inherited as CSS says.
The book's own text colour, its body's or black, stands for the theme's
ink. Any other colour is drawn on the page's greys: as far toward the
paper as its contrast with its background falls short of the book's
text, at most 55%, so it stays readable; never darker than its hue
needs. The hue rides on that grey.

The page pass then maps the grey from the theme's ink to its paper, as
for all book text, and adds the hue back around it. If the hue does not
fit (a pale ink in a dark theme), the grey moves up to 0.1 toward the
middle, then the hue shrinks to fit. A theme with less contrast than
0.5 between ink and paper, as dim, mutes hues as much (to a quarter at
least), so colours stay as quiet as its text. Greys are drawn exactly as before,
so books without colours look the same, and a theme switch stays a
uniform change, with no new layout.

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
letter spacing, three or more body sizes). Layout CSS counts only when
the chapter is short (800 characters at most) or at least 30 % of its
text sits inside what used it, so prose with a framed figure or a
bordered note stays prose. The **Book design** setting
picks Auto, On or Off. Under it, **This book** sets one book's own
choice (As set, Auto, On or Off). The choice is saved on the book's
library record as a stamped field, so it merges like a title would
once sync arrives; older builds keep it untouched.

Designed books are made for a phone's page, about 26 of their ems wide.
A designed chapter is set as if the column were that wide, so its type
grows with a wide pane, up to 1.6 times the reader's default size and
never below it, then scales as the reader's size does.

## The book's type and the reader's settings

In Auto and On, every chapter with a style of its own (a sheet or a
`style` attribute) is set in the book's type; Off, or a chapter with no
style, as converted Markdown and text are, reads in the reader's type.
The book's sizes are in its own ems, and the reader's size is the
book's 16 px, so headings, notes and drop numbers keep their steps.
Each reader setting moves the book's value as it moves off its default,
so a bigger setting makes every size bigger rather than one size:

| Setting | Over the book's type |
| --- | --- |
| Font size | Scales every size (default 17 equals the book's own). |
| Line height | Scales a line height the book sets; unset, the reader's. |
| Paragraph spacing | Scales the book's margins (default 1 keeps them). |
| Weight | Shifts every weight by the change from 400. |
| Indent, letter and word spacing | Add to the book's. |
| Justify | On justifies start-aligned text; off leaves the book's choice. |
| Fonts | Embedded fonts stay. In prose, the reader's fonts replace named machine fonts; `sans-serif` and `monospace` take the reader's sans and code faces. Designed chapters keep every named font. |
| Hyphenation | The reader's own. |
| Text colours | The book's, adapted to the theme: see Text colours. |

## Slices

| Slice | Scope |
| --- | --- |
| L1 | Cascade engine; Look (sizes, line height, spacing, indent, weight, embedded fonts, justify, margins, spacer paragraphs); designed-chapter test; Book design setting. |
| L2 | Boxes: borders (solid, double, dashed, dotted; per side; radius), padding, widths, auto margins, rules from bordered empty blocks, vertical centring of tall flex or `vh` containers. Done, with round corners from `border-radius`. |
| L3 | Grids: tables with block cells, column widths (fixed, %, content), cell padding, borders, `vertical-align`, spans, nesting; flex rows (justify, align, wrap, gap); anchored text in cells. Done: tables with block cells, fixed and % widths shrunk to fit, cell boxes, `vertical-align`, nested tables read flat, one-column tables as framed boxes, column widths from content (least and most widths, shared by every row), `colspan` and `rowspan`, flex rows (justify, align, wrap, gap, grow, basis). A flex row holding one layout table lets the table be the grid. |
| L4 | Floats with text wrapping, inline images, `break-before`/`page-break-*`, decorations, small caps, sub and super, background pictures. Done: design scale, floats (left and right) with text wrapping line by line, `break-before`/`break-after`, underline and line-through, small caps and `text-transform`, sub and super, background colours and pictures, box heights, small pictures set inside a line (at most 3 ems tall), `transform: rotate()` drawn about the block's middle. |
| L5 | Per-book override, both samples and the test corpus checked, docs, parity. Done: the per-book choice in the Fonts tab, saved with the book; the Gutenberg Raven stays prose in Auto; hostile grids, flex rows, spans, turns, radii and line pictures open within the time cap. |
| L6 | Leftovers. Done: `flex-direction: column` (an item narrower than the column, or placed by `align-items`/`align-self`, is its own row of one; stretched items read as prose; `row-gap`); line pictures follow `vertical-align` (keywords, lengths, percentages); a `rowspan` cell sits across the rows it spans and its last row grows to hold it; a `vh` flex page centres its content (the screen share is the page's, not a length). |
| L7 | The book's type in prose: sizes in its own steps, weights, indents, alignment, margins, inline sizes and spacer lines, moved by the reader's settings (table above); unstyled chapters keep the reader's type; designed type scales with the reader's size. |
