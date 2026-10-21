# EPUB support: what a full reader draws

The EPUB 3.3 reading system rules, checked item by item against our
engine (2026-10-21), and the slices that close the gaps. Layout itself
(boxes, grids, floats, the book's type) is in [epub-layout.md](epub-layout.md).

## What the specification asks

| Area | Level | Rule |
| --- | --- | --- |
| Core images | MUST | PNG, JPEG, GIF, WebP and SVG. |
| Fonts | MUST | TrueType, OpenType, WOFF and WOFF2; obfuscated fonts are de-obfuscated (IDPF and the older Adobe scheme). |
| CSS | MUST | The CSS Snapshot as browsers do it; the HTML suggested rendering as the default sheet; `@supports`; vendor `@media` types ignored. |
| SVG | MUST | SVG pages in the spine and SVG inside chapters, styled by CSS. |
| MathML | MUST | Presentation MathML drawn (a script library is allowed). |
| Fixed layout | MUST | `pre-paginated` books: one page per spine item, sized by the viewport `meta` or the SVG `viewBox`, no margins added. |
| Language | MUST | `lang`/`xml:lang` and `dir`; `page-progression-direction`; vertical writing. |
| Fallbacks | MUST | Manifest fallback chains, `object` content, `epub:switch` (deprecated) fallbacks. |
| Navigation | MUST | The nav document; the NCX for EPUB 2; page lists and landmarks. |
| Scripting, media overlays, audio and video | SHOULD / MAY | Optional. |

## Audit (2026-10-21)

| Feature | State before | Slice |
| --- | --- | --- |
| PNG, JPEG, GIF, WebP | Done | |
| SVG files (`img`, `image`, `object`, `embed`) | Not drawn | E1 |
| SVG inside chapters | Skipped; only its `image` showed | E1 |
| SVG pages in the spine | Not read | E1 |
| `data:` images | Refused | E1 |
| Black art on transparent ground | Kept its colours, so it vanished on dark paper | E1 |
| Borders, backgrounds and padding in prose chapters | Drawn only in designed chapters | E1 |
| `::before`, `::after`, `content`, counters, quotes | Dropped | E2 |
| `::first-letter` drop caps, `::first-line`, `::marker` | Dropped | E2 |
| `list-style-type` (roman, alpha, none, strings), `list-style-image` | Decimal and bullets only | E2 |
| `border-image`, `box-shadow`, gradients, multiple backgrounds | Missing | E3 |
| `outline`, `opacity`, `visibility`, `text-shadow`, `filter` | Missing | E3 |
| `hr` in the book's width, style and colour | The reader's faint rule | E3 |
| `calc()`, `var()`, `min()`, `max()`, `clamp()` | Missing | E4 |
| `@media` features (`prefers-color-scheme`, widths), `!important` | Partly | E4 |
| `@import`, `@supports`, `@layer`, `@container` | Skipped, with all their rules | E4 |
| `initial`, `inherit`, `unset`, `revert` | Read as unknown values | E4 |
| `media` on `link` and `style`, alternate sheets | Every sheet applied | E4 |
| Obfuscated fonts, WOFF, WOFF2 | Missing | E5 |
| `font-variant`, `font-feature-settings`, `text-align-last`, widows and orphans, `break-inside` | Missing | E5 |
| `dir`, right-to-left text and page order | Missing | E6 |
| Ruby | Read as plain text | E6 |
| Vertical writing (`writing-mode`) | Missing | E6 |
| Fixed layout (`pre-paginated`, spreads) | Read as reflowable | E7 |
| MathML | Skipped | E8 |
| Audio, video, `object` fallbacks, `epub:switch`, manifest fallbacks | Missing | E9 |

## Slices

| Slice | Scope |
| --- | --- |
| E1 | SVG everywhere (files, inline, spine pages, `data:`); black art on transparent ground follows the theme's ink; painted boxes (borders, backgrounds, padding, radius) in prose chapters. |
| E2 | Generated content: `::before`/`::after` (strings, `attr()`, counters, quotes, pictures, empty bordered blocks), drop caps, `::first-line`, `::marker`, every list style and list pictures. |
| E3 | Backgrounds and borders: `border-image`, shadows, gradients, layered backgrounds, outlines, opacity, visibility, text shadows, `hr` as the book draws it. |
| E4 | CSS values and queries: maths functions, custom properties, media features, importance. |
| E5 | Fonts and text: de-obfuscation, WOFF and WOFF2, font features, last-line alignment, widows and orphans, unbroken blocks. |
| E6 | Languages: bidirectional text, right-to-left page order, ruby, vertical writing. |
| E7 | Fixed layout: pre-paginated pages at their viewport, spreads, absolute positions. |
| E8 | MathML: fractions, scripts, roots, rows, operators, tables. |
| E9 | Media and fallbacks: audio and video as their poster or a labelled box, `object` and `epub:switch` fallbacks, manifest fallback chains. |

## SVG (E1)

SVG draws through resvg at the size it is shown, so it stays sharp at any
zoom. A file's size comes from its root `width`, `height` or `viewBox`;
images it links are read from the book. An inline `svg` is kept as markup
(names keep their SVG case) and drawn as a picture; one holding only an
`image`, the usual cover wrapper, still shows that image directly.

## Black art (E1)

A picture that is mostly see-through and whose marks are dark and nearly
colourless (line drawings, ornaments, title lettering) is drawn as ink: it
takes the theme's tone like text, so it reads on dark paper. Coloured and
opaque pictures keep their own colours.

## Boxes in prose (E1)

Prose chapters draw the boxes the book paints (a border, a background
colour or picture), at the reader's size, with their padding, margins,
width and corners. Boxes that only move text are left out, so prose still
reads in the reader's column.

## Generated content (E2)

`::before` and `::after` (one colon too) show their `content`: strings with
CSS escapes, `attr()`, `counter()` and `counters()` in any list style,
`open-quote` and `close-quote` by `quotes` and depth, and `url()` pictures.
A block box is laid as a block; an empty one shows only when it draws
something, so a bordered divider is a rule and a clearfix is nothing.
An `hr` with generated content shows that ornament instead of a line.
`q` quotes as browsers do. Counters reset, increment and set as CSS says;
one never reset counts from the root, and `list-item` is the list's count.

## Drop caps and first lines (E2)

A floated `::first-letter`, or a short floated run starting a paragraph,
is a drop cap: the letter is drawn large, spanning lines by its size, and
the text wraps beside it. The letter stays in the text, so search and
selection still read the word. Unfloated, the letter is only restyled.
`::first-line` styles what the first line holds, after a few passes. Both
need the book's design; read as the reader's own, the text is plain.

## Lists (E2)

Every `list-style-type` counts its own way: decimal (with a leading zero),
Roman, Latin, Greek, Armenian, Georgian, Hebrew, kana and iroha, CJK, the
native digits of Arabic, Indic, Thai, Lao, Khmer, Tibetan, Mongolian and
Myanmar scripts, symbols, and strings. `type` and `value` attributes count
as in HTML. `list-style-image` hangs the picture as the marker, at most a
line tall. `::marker` sets the marker's text and colour.

## Backgrounds and borders (E3)

Boxes draw what CSS paints around them, in the theme's ink:

- `box-shadow`, outer and inset, blurred and spread, several per box.
- `linear-gradient` and `radial-gradient`, repeating too, with the old
  prefixed forms; several background layers, the first on top.
- `border-image`: the picture cut in nine by `slice`, its sides stretched,
  repeated or rounded, with `width`, `outset` and `fill`.
- `outline` and `outline-offset`, outside the border, taking no room.
- Groove, ridge, inset and outset borders in two tones.
- A transparent border takes its room and draws nothing.
- `opacity` fades a box and everything in it, text included.
- `visibility: hidden` keeps the room and draws nothing; the text is still
  found by search. `filter` applies `opacity()` and `drop-shadow()`;
  colour filters are left to the theme.
- `text-shadow` draws a soft copy behind the glyphs.
- Background pictures and border pictures may be `data:` URLs.

Shades are strengths of the ink, as backgrounds are, so they follow every
theme. A gradient behind text is kept faint enough to read; in an empty box
it shows at full strength.

## Rules as the book draws them (E3)

An `hr` the book styles itself (a border, a background, a gradient, a
picture, a shadow) is drawn as a box in the book's width, margins and
style, so a centred short line, a double rule or a gradient fade shows as
designed. Read as the reader's own, it is the reader's plain rule. An `hr`
the book leaves alone is the reader's faint rule.

## Values and queries (E4)

Lengths may be sums: `calc()`, `min()`, `max()` and `clamp()` work out to
pixels where they can; a share of the column stays a share, so
`calc(100% - 2em)` keeps tracking the page. Custom properties cascade and
inherit like any other; `var()` takes the winning value or its fallback,
and one that names itself, or names nothing, leaves its property unset.
Properties set on `:root` reach the whole chapter, though `html` opens
before its sheets load.

Sheets meet their conditions before any rule counts. `@import` loads up to
four sheets deep, a few per sheet, each once, imports first; one with a
`media` list or a `supports()` test that fails is skipped. `@supports`
asks the engine itself whether it reads a property or selector, so
fallbacks a book writes for older readers stand down when ours draws the
real thing. `@layer` blocks rank in the order first named, nested ones
within their parent, all below unlayered rules; important rules turn
that order round. `@container` reads the column. `initial`,
`inherit`, `unset` and `revert` reset what lower rules set.

Media queries see a page 650 pixels wide and 980 tall, a colour screen
with a fine pointer and no scripting. `prefers-color-scheme` always
answers `light`: the reader already turns the book's colours and black art
to each theme, and a book that also flips them under `dark` would come out
inverted twice, its ornaments vanishing on dark paper. Vendor `@media`
types, `print`, alternate sheets, and `link` or `style` elements whose
`media` doesn't match are not applied.
