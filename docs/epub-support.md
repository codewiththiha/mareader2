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
| Vertical writing (`writing-mode`) | Missing | E6b |
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
| E6 | Languages: bidirectional text, right-to-left page order, ruby, runs in their own font family. |
| E6b | Vertical writing: `writing-mode`, upright and sideways runs, columns read right to left. |
| E7 | Fixed layout: pre-paginated pages at their viewport, spreads, absolute positions. |
| E8 | MathML: fractions, scripts, roots, rows, operators, tables. |
| E9 | Media and fallbacks: audio and video as their poster or a labelled box, `object` and `epub:switch` fallbacks, manifest fallback chains. |
| E10 | Read-aloud: media overlays played in the reader, the phrase read lit, pages turning to follow. |

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

## Fonts the book ships (E5)

A book's fonts load however they are packed. WOFF tables are inflated
one by one; WOFF2 is a single Brotli stream whose `glyf`, `loca` and
`hmtx` tables may come transformed, and these are rebuilt point by point
into plain TrueType, outlines, composites, hinting programs and side
bearings as the original had them. A font collection packed as WOFF2 is
not read; its family falls back to the next one named.

Fonts listed in `META-INF/encryption.xml` are revealed before use. The
IDPF scheme XORs the first 1040 bytes with the SHA-1 of the unique
identifier, white space removed; the older Adobe scheme XORs the first
1024 with the bytes of a `urn:uuid:` identifier. Every identifier the
book declares is tried, the unique one first, and a key is kept only if
the result starts as a font. Any other encryption leaves the file alone.
Unpacked faces keep the per-face and per-book size caps.

## Type features and page breaks (E5)

`font-variant-numeric`, `font-variant-ligatures`, `font-variant-position`,
`font-kerning` and `font-feature-settings` become OpenType features of the
run, settings last, each group inheriting on its own; the `font-variant`
shorthand sets all three groups and the caps, and `font` resets them.
Features a face lacks do nothing, as in a browser. Small capitals are
drawn smaller where the face has none, now for `all-small-caps` too, with
petite caps read as small caps.

`text-align-last` places a paragraph's last line, and any line before a
forced break, in designed chapters. Paragraphs split between pages keep
the book's `widows` and `orphans` (up to eight lines; two by default).
`break-inside: avoid` keeps a block, or a box and all it holds, on one
page when it fits on one, and `break-after: avoid` keeps a block with the
next as headings are kept. These follow the book in designed chapters;
prose keeps the reader's two-line rule.

## Right to left (E6)

A block's direction comes from its `dir` attribute or the CSS `direction`
property, inherited, the sheet winning over the attribute. `auto`, or no
direction at all, reads the block's first strong letter: Hebrew, Arabic,
Syriac, Thaana and N'Ko read right to left, anything else left to right.
Lines then start at the right and mixed runs are ordered by the Unicode
bidirectional algorithm, so English words and numbers in an Arabic line
keep their own order. List and quote indents, quote bars and list markers
move to the right side. Drop caps, table columns and an explicit `left`
or `right` alignment keep their sides.

Pages turn right to left when the spine's `page-progression-direction` is
`rtl`, or, with none given, when the book's language is Arabic, Hebrew,
Persian, Urdu, Yiddish, Pashto, Sindhi, Uyghur or Dhivehi. A spread then
shows its first page on the right, and in single and spread views the left
arrow goes forward. Scrolling views are unchanged.

## Ruby and run fonts (E6)

The text of `<rt>` and `<rtc>` becomes a note over the base read since the
ruby began or since its last note, trimmed of space; `<rp>` fallbacks are
hidden. Notes are drawn at half the base's size, centred over the first
line part of their base, and a line holding one grows by 0.6 em to make
room. Notes keep to reading order, never overlap, and hold at most 256
bytes; they aren't part of the page's text, so search and selection see
only the base.

In designed chapters, a run whose `font-family` differs from its block's
is set in its own stack (up to 255 per block), and a font the book ships
loads when only such runs use it. The book's family on `code` now wins
over the reader's monospace.


## Vertical writing (E6b)

`writing-mode: vertical-rl`, or its `-epub-` and `-webkit-` forms and the
older `tb-rl`, on the root or body turns a chapter: its lines run down the
page and follow one another from right to left. `vertical-lr` and the
`sideways` modes are read as `vertical-rl`. A chapter that doesn't ask
stays level, so a book may mix both; each run of vertical blocks gets
pages of its own.

In a vertical line, Chinese, Japanese and Korean letters, kana, Hangul and
full-width forms stand upright and take the font's vertical forms (`vert`),
so punctuation and brackets turn. Other letters lie on their side.
`text-orientation: upright` stands every letter, Latin ones in the
font's full widths (`fwid`) when it has them; `sideways` lays every
letter down. `text-combine-upright: all` (or the older `horizontal`)
sets its run, at most 256 bytes, across the column in half widths,
squeezed to one em: dates such as 10月21日. Ruby notes sit to the right
of their base. Pictures, inline or alone, stay upright, sized for the
column.

When the spine gives no `page-progression-direction`, a book with any
vertical chapter turns its pages right to left. Selection, search, links
and pictures follow the turned page: a drag down a column selects along
it, and columns are read from the right.

Margins aren't remapped to the turned page, `vertical-lr` columns aren't
read left to right, `text-orientation` applies to whole paragraphs, not
to inline runs, and `text-combine-upright: digits` combines the
whole run.

sha256 of `vertical.epub`: `18aadeea5e9ac0d666d5fddc2b5537cb4348ab481945b52bf72d5fd7472231f8`

## Fixed layout (E7)

A book whose package sets `rendition:layout` to `pre-paginated` is laid
out by the book, page by page. A spine item's
`rendition:layout-pre-paginated` or `rendition:layout-reflowable`
overrides that, so one book may mix fixed pages and reflowed chapters.

Each fixed item is one page at its viewport: the size its
`<meta name="viewport" content="width=…, height=…">` names, else its
body's width and height in px, else the reflowed page's size. A picture
in the spine, SVG or bitmap, is a page the picture's size. The page is
laid out in CSS px, an em being 16 px, with the book's own type: the
reader's font, size, spacing and margins don't apply. It's then scaled
to fit the reader's page and keeps its shape. The theme's paper and ink
still apply, as in reflowed chapters.

`position: absolute` and `fixed` take a box out of the flow and set it
at its `top`, `right`, `bottom` and `left`: px, percentages, or viewport
units of the page. Its `width` and `height` come from the book, or else
from the room its offsets leave. A box with no offsets sits at the corner
of its containing block. `position: relative` stays in the flow and is
the containing block for the boxes it holds. Positioned boxes paint over
the flow, in `z-index` order, ties in document order; a negative
`z-index` paints under the flow. The background of the root (else the
body) covers the whole page, under everything.

In the Two pages view, a first page marked `page-spread-right` (or
`page-spread-left` in a right-to-left book) shows alone, and the pages
after it pair. `rendition:spread` set to `none` shows every page alone.
Each page in a spread keeps its own size. Selection, search, links,
pictures and page AI work on fixed pages at their scale.

A positioned box with no width takes the room its offsets leave, not
its content's width. `position: relative` doesn't move the box itself.
Only the first page's `page-spread-*` is read, so
`rendition:page-spread-center`, `rendition:orientation` and the
landscape-only spreads aren't followed.

sha256 of `fxl.epub`: `85764517a37055bb1ffea996559fad292c7067a9f66a5cf4c4357ca056333361`

## MathML (E8)

Presentation MathML is laid out by the reader, without scripts, and
drawn as a picture. Its type is STIX Two Math (OFL), bundled as a subset
of the math ranges with its MATH table, whose constants set every shift,
gap and rule. Glyphs are drawn as outlines, so formulas stay sharp at any
zoom. A black formula is line art: it takes the theme's ink, as text does.
`mathcolor` keeps its colour.

A `math` element sits in its line, its baseline on the text's. With
`display="block"`, or `display: block` from the book's CSS, it's its own
block, centred and set in display style. Formulas show in table cells
too, where display math sits in the cell's line. A prefixed `m:math`
reads the same.

| Element | How it's set |
| --- | --- |
| `mi`, `mn`, `mo`, `mtext`, `ms`, `mspace` | One-letter `mi` in italic; `mathvariant` maps letters, Greek and digits to the math alphabets |
| `mo` | An operator dictionary: spacing by kind, a sign after an operator is unary, `form`, `lspace`, `rspace`, `stretchy`, `largeop`, `movablelimits`, `accent` |
| `mrow`, `mstyle`, `mpadded`, `mphantom`, `merror` | `displaystyle`, `scriptlevel`, `mathsize`, `mathcolor`; padding by lengths, percentages and `width`, `height`, `depth` |
| `mfrac` | `linethickness` (none for binomials), `numalign`, `denomalign`, `bevelled` |
| `msqrt`, `mroot` | The radical grows by size variants, then by parts; the index sits by the MATH kerns |
| `msub`, `msup`, `msubsup`, `mmultiscripts` | Scripts shrink a level and shift by the MATH rules, prescripts too |
| `munder`, `mover`, `munderover` | Limits above and below in display style, beside as scripts in a line; accents sit close; braces and arrows stretch across |
| `mtable`, `mtr`, `mtd` | Centred on the math axis; `columnalign`, `columnspacing`, `rowspacing`, `columnlines`, `rowlines`, `frame` |
| `menclose` | `box`, `roundedbox`, `circle`, sides, `actuarial`, `madruwb`, `longdiv`, `radical` and the strikes |
| `mfenced`, `semantics`, `maction`, `mstack` | Fences and separators; the first child that isn't an annotation; the selected child; rows right-aligned |

Fences in a row grow to what they hold. Large operators take their
display size in display style. A formula that nests deeper than 48
levels or holds over 4096 elements isn't set. Its `altimg` shows
instead, else its `alttext`, else its text.

`mlabeledtr` labels are left out. `mglyph` and `annotation-xml` aren't
drawn. Line breaking inside a formula isn't done: a long inline formula
stays in one piece. Copy and search skip formulas.

sha256 of `math.epub`: `a9b54ffb7b329a7352072e0379844b7b1a351122880b5b5260f385217f6cb488`

## Media and fallbacks (E9)

The reader has no player and runs no scripts. Sound and video show as
cards, and clicking one hands the clip to the system's player.

| Content | How it shows |
| --- | --- |
| `video` | Its `poster` under a play mark, else a drawn player frame; a caption names it by `title`, `aria-label` or file |
| `audio` | A drawn player strip and its caption |
| `source` | The first one that plays is the clip; else the first is named |
| `object`, `embed` | A picture shows as one; sound or video as a card; anything else shows the `object`'s own content |
| `iframe` | A link to the page it embeds |
| `epub:switch` | The first `epub:case` whose `required-namespace` the reader draws (MathML, SVG, XHTML), else `epub:default` |
| `noscript` | Shown, since scripts never run |
| `canvas` | Its content, drawn as the fallback |
| `picture`, `srcset` | The first `source` whose `media` applies and whose `type` the reader decodes; a `srcset`'s largest candidate |
| `a` around a picture | The whole picture is the link |
| `img usemap` | Each `area` of its `map` is a link over its share of the picture; circles and polygons by their bounds |

Drawn cards are line art, so they take the theme's ink. Text inside
`video` and `audio` is for readers that can't play them, so it stays
hidden. A clip at an `https` address opens in the browser. A card whose
clip the book lacks says so, and one whose type no player opens says it
can't play here; neither is a link. A picture
small enough to sit in a line of text isn't a link or a map itself; only
the linked text around it is.

A clip is copied to the system's temporary folder before it plays. Its
name comes from the type the manifest declares, never from the book: only
`audio/*` and `video/*` types with a known file extension are handed out.
Copies older than a day are cleared. Only books with clips keep their file
in memory to read them from.

Manifest fallback chains are followed, eight items deep at most. A
picture that won't decode shows its first fallback that does. A spine
item that isn't XHTML or a picture reads as its first fallback that is,
and links to the item land on it.

Media overlays read aloud in the reader (E10, below). `track` captions,
`epub:trigger` and scripted content aren't supported yet.

sha256 of `media.epub`: `1c735eea7efc902ce4e1231a3cef6cfb5d4f0c20a8d585fa16578ee5c82c64d8`

## Read-aloud (E10)

A book whose chapters name a `media-overlay` can be read aloud. Right-click
a page and choose **Read aloud from here**: reading starts at the phrase
under the pointer, or at the page's first phrase. The sound plays inside
the reader; clips in the text still open in the system's player.

| Overlay | How it reads |
| --- | --- |
| `par` | One phrase: the text its `text` names, read by its `audio` clip |
| `text src` | `file#id`, or `file#xpointer(id('id'))`; a phrase runs from where the element starts to where it ends. No fragment, or one the book lacks, reads the whole chapter |
| `audio` | `clipBegin` and `clipEnd` in full, partial or timecount form; no `clipBegin` starts at 0, no `clipEnd` plays to the file's end |
| `par` without `audio` | Skipped |
| `seq` | Nesting is followed; its `epub:type` applies to the pars inside |
| Skippable | Notes (`footnote`, `endnote`, `rearnote`, `note`, `noteref`) and page numbers (`pagebreak`) can each be left out |
| Escapable | In a `table`, `list`, `figure`, `aside` or `sidebar` (and their rows, cells and items), a button leaves the whole structure |
| `media:active-class` | The background the book's stylesheets give that class colours the phrase read; without one the theme's accent does |
| `media:narrator`, `media:duration` | Read; the whole book's length is kept |

The phrase read is lit on the page and pages turn to keep it in view.
Reading goes on across chapters to the book's end. Text inside a table
isn't lit, but the page still turns to it. The lit colour lies lightly
over the page, more faintly on dark themes, so the text stays readable.

The bar at the foot of the pane has the previous and next phrase, play
and pause, the way out of a table, the speed (0.75× to 2.5×, at the
narrator's pitch), and the options to skip notes or page numbers and to
turn pages. Its choices are kept with the settings: by default page
numbers are skipped and pages turn.

The sound is decoded by the reader itself: MP3, AAC and M4A, FLAC, Ogg
Vorbis and WAV, with no system codecs. Only a little of a file is decoded
ahead of what is heard, so a long chapter doesn't fill memory. Phrases
whose clips follow on in one file play as one, without gaps. Without a
sound output, or with a file that won't decode, the bar says so.

`media:playback-active-class`, which styles the whole document while it
plays, isn't applied. Overlays with more than 100,000 phrases are cut
there, and an overlay file is read up to 16 MB.

sha256 of `narration.epub`: `480889f7ab1f0ca0d752d2e24e74f93112f8ae737bdac3f51470ea420006b645`
