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
