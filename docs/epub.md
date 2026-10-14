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
          ▼  vello_cpu 0.3 (text as a coverage mask; colour in the page pass)
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

## The XHTML subset (M4 slice 2)

What the epub crate reads, and what each element becomes:

| Markup | Block | Drawn as |
|---|---|---|
| `h1`–`h6`, `p`, `div` and other blocks | text | heading or body face; `h6` muted |
| `ul`, `ol` (`start`), `li`, `dd` | text, framed | 1.6 em indent per level; `•` `◦` `▪` or `N.` hung in the margin; 0.3 em between items |
| `blockquote` | text, framed | 1 em indent per level, a 3 px bar, softer ink |
| `pre` | preformatted | mono, spaces and tabs kept, long lines wrap; shaded box |
| `code`, `kbd`, `samp`, `tt` | inline | mono at 0.88 em on a light ground |
| `table`, `caption`, `th`, `thead` | table | equal columns, 1 px borders, head rows bold and shaded; splits between rows |
| `hr` | rule | a hairline |
| `img`, SVG `image` | image | its own size, shrunk to the column and the page; never split |
| the OPF cover | image | a page of its own, fitted and centred |

- **CSS:** inline `style`, `<style>` and linked stylesheets. Selectors `tag`,
  `.class` and `tag.class`; anything else, and every @-rule, is skipped.
  Properties: `font-style`, `font-weight`, `text-align`, `display: none`.
  `justify` reads as start until the typography slice.
- **Images:** read once per path, only from inside the archive. The header is
  probed when the book opens. Past 16 384 px a side, 50 MP or a 256 MiB
  budget per book, the alt text shows in italics. Pixels are decoded when a
  page needs them, at a power-of-two size near the drawn size, and kept in a
  64 MB cache. One that won't decode draws as a grey box.
- **Faces:** the mono face is JetBrains Mono (OFL). Every face falls back to
  Inter for symbols it lacks.

## Typography as built (M4 slice 3)

- **Settings to type.** `session::typography` turns `TextSettings` into
  `reflow::Typography`. The body takes the serif slot until the Font slot
  names a face; headings take the sans slot, code the mono slot.
- **Stacks.** A slot's pick comes first, then its bundled face (Literata,
  Inter, JetBrains Mono) as the last resort. System picks expand to their
  CSS stack; `serif`, `sans-serif` and `monospace` mean the machine's own
  face of that kind. fontique scans the machine once (fontconfig is opened
  at run time, so a box without it just has no system faces).
- **Hyphenation.** `hypher` marks break points with U+00AD in words of five
  letters or more, never inside inline code. The patterns follow the book's
  `dc:language`, falling back to English. parley breaks there but draws
  nothing, so the raster draws the hyphen at the line end.
- **Justify and indent.** `Alignment::Justify` leaves last lines ragged.
  The first-line indent applies to body paragraphs only; headings, quotes,
  list items, table cells and code start flush, as mareader's do.
- **Re-setting keeps the place.** A change re-sets each open book off the
  UI thread; the old pages stay up until the new ones arrive as
  `Event::Retypeset`. The view keeps the anchor of the page it showed and
  lands on the page holding it. Going back to the old type returns to the
  same page, since the stored anchor is reused, not re-derived.
- **Fonts tab.** Settings › Fonts: the four slots, size and weight
  steppers, spacing dials, and the justify and hyphenation switches.
  A system face that isn't installed shows as "Not installed".

## View modes as built (M4 slice 4)

- **One raster per page in every mode.** Single, spread, the horizontal
  strip and the stream all draw the same page textures, so switching modes
  draws nothing new and zoom never re-paginates.
- **Book layout.** The column moves toward the spine: 56 units on the
  outer edge, 92 on the spine side, so the column is 646 wide instead of
  650 and the book re-paginates once. Spreads pair pages from the first,
  so even pages (0-based) sit left with their gutter on the right. Single
  pages and the strip follow the same parity, unlike mareader, whose lone
  pages count the other way; one rule keeps one texture per page.
- **The stream.** The vertical strip of a book shows each page's band:
  its column, cut to the text plus the block gap its page cut swallowed
  (a chapter start gets a pad's worth). Bands are laid edge to edge with
  no gap or shadow, on the pages' own paper, so the pane reads as one
  column. Each page is drawn whole and clipped to its band (with 24 units
  of slack either side for overhanging glyphs), so pagination, anchors,
  page numbers and the page cache stay those of the paged modes.
- **Column alignment.** Left, centre or right across the pane; off the
  centre, the page margin keeps the column off the edge, at least 16 pt
  (mareader lets it touch at a zero margin).
- **Fit.** Fits measure whole pages, so the type is the same size in the
  stream as on the paged modes' pages.
- **Settings.** Book layout and column alignment sit in Settings › Fonts.

## Navigation as built (M4 slice 5)

- **Places.** A place is a block and a byte offset, never a page, so it
  outlives any re-pagination. Every element `id` (and old `<a name>`)
  marks one: mid-paragraph at its offset, otherwise at the next block.
  Each chapter file marks its first block.
- **Contents.** The EPUB 3 nav, else the EPUB 2 NCX, flattened with
  depth (capped at 20 000 entries and 5 levels deep). An entry whose
  target never resolves is dropped. A book that lists nothing gets its
  headings, all six levels, as mareader outlines its Markdown. Pages are
  worked out again from the places on every re-set, and the outline panel
  marks the active entry as it does for a PDF.
- **Links.** Linked words are underlined in the ink they're set in, not
  mareader's accent: pages are drawn in greys and themed afterwards.
  Each line a link crosses gets its own hit box, read from parley. Links
  inside the book open on the page holding their place; others go through
  the PDF path's scheme check, so `javascript:` and `file:` do nothing.
- **Streams.** A sheet is hit-tested only where its band shows, so a link
  near a band edge isn't taken by the neighbouring page's hidden part.
- **Resume.** A book read at exit saves its place with its page; the next
  launch opens on the page holding that place, even in other type.

## Selection and ink as built (M4 slice 6)

- **Characters.** Each page's text comes from parley's clusters, line by
  line: one box per character, as wide as its share of the cluster and as
  tall as the line. Spaces are kept as characters, while soft hyphens and
  the hyphens drawn at line ends are not. List markers are left out, as a
  browser leaves out CSS markers. The page reaches the views as the same
  text layer a PDF's page gives, read on a worker, so every selection
  gesture works unchanged: drag, wheel and edge scroll, double click,
  ⌘C or Ctrl+C, Escape.
- **Copying.** A line that wrapped joins the next with nothing between
  them: the space it broke at stays on the line, and a hyphenated word
  rejoins. A line that ended (a new block, `<br>`, a line of `<pre>`)
  gives a line break. A page that opens mid-paragraph joins the page
  before in the same way.
- **Triple click.** It takes the paragraph's lines on that page, as a
  browser does; on a PDF, where no line wraps into the next, it still
  takes one line.
- **Ink dial.** The dial is mareader's "Text ink intensity", 10 to 100 %.
  It sits under the tint in the Appearance popover while the active pane
  shows a book, and in Settings › Fonts. The page pass mixes the drawn
  greys toward white before the theme, folded into the first matrix of
  the chain, so it costs nothing and moves with the slider. Everything
  drawn in ink lightens (headings, rules, borders and code grounds too, as
  mareader's tints derive from its ink). Pictures don't: each is drawn a
  second time over its own box in the pane's plain tone. Thumbnails keep
  full ink. Without a GPU, pages aren't themed and the dial does nothing.
  *Since the follow-ups, pictures keep their own colours unless Theme
  pictures is on (§ Follow-ups as built).*

## Hostile books as built (M4 slice 7)

A book is untrusted input. Whatever its bytes, opening it ends in a book or
in one of three refusals (`OpenError`): `Invalid` (not a zip or no
package), `Empty` (nothing to read) and `TooLarge` (past a cap below). On
a refusal the pane closes and a toast says why, such as "the book is too
large to open safely". Nothing crashes.
`crates/epub/tests/hostile.rs` builds each attack and checks the outcome.

**Caps.** Each is far past any real book and stops one kind of attack.

| Where | Cap | Stops |
|---|---|---|
| Zip directory (`guard.rs`) | 10,000 files; 128 MiB a file; 512 MiB in all | zip bombs, before anything inflates |
| Inflating | the size the directory declares | a file lying about its size: zip stops at the declared size |
| Markup (`book.rs`) | 64 MiB of chapter text | books too big to lay out |
| Contents (`guard.rs`) | 64 levels of nav or NCX nesting | stack overflow in rbook, which builds and frees the tree recursively |
| Metadata (`guard.rs`) | 8 links in a `refines` chain | the same in rbook's recursive metadata |
| XHTML (`xhtml.rs`) | 256 open elements; 16 stylesheets a book | deep markup; one sheet re-linked by every chapter |
| CSS (`css.rs`) | 1 MiB a sheet; 10,000 distinct selectors | rule floods; a selector repeated merges into one rule |
| Pictures | header-probed sizes, as in slice 2 | a small file declaring 100,000 px a side shows its alt text |
| Layout (`book.rs`) | 1 GiB estimated | out-of-memory: see below |

**The guard.** Before rbook sees a book, `guard::open` checks the zip
directory. It then reads the book bare, without contents or metadata, and
scans the package file with quick-xml the way rbook reads it. A nav or NCX
nesting deeper than the cap is skipped and the headings stand in, as for a
book listing nothing. A refinement chain past the cap skips the metadata, so
the book opens untitled. If rbook still rejects the contents or metadata,
the book opens without them rather than not at all.

**Broken markup reads.** html5ever's tokenizer recovers from unclosed and
stray tags and unknown entities. A chapter whose bytes aren't all UTF-8 is
decoded lossily: the stray bytes become U+FFFD and the rest reads. rbook's
own reader drops such a chapter whole, so chapters are taken from the spine
directly. A spine entry naming no file, a picture path escaping the book
(`../`, `/`, `file:`) and a picture that won't decode are skipped or shown
as alt text.

**The layout budget.** The pipeline above plans lazy layout by chapter. As
built, a book is laid out whole when it opens, and every paragraph keeps its
parley layout, about 3,000 bytes plus 48 a byte of text (measured; see
`docs/memory.md`). A book whose estimate passes 1 GiB is refused before
layout. *War and Peace* comes to about 190 MB, the complete Shakespeare to
about 560 MB. Laying out on demand would lift the limit; it is a follow-up.
*Superseded: layouts are now laid on demand and blocks are capped instead
(§ Follow-ups as built).*

**EPUB 2.** Books with only an NCX, an OPF 2 cover (`<meta name="cover">`)
and HTML entities open as EPUB 3 books do. A real one is a test fixture:
Project Gutenberg's *The Raven* (`crates/epub/tests/books/`), with a nested
NCX, a page list and a cover wrapper page.

**Fuzzing, cheaply.** One test flips bytes in copies of the sample book 48
times, seeded, and opens each; every copy opens or is refused.

## Follow-ups as built (before M5)

Owner decisions after M4, and what they changed.

- **Layout on demand.** Opening a book still lays every block out once, to
  measure its lines and cut exact pages, but keeps only the line boxes.
  A block's parley layouts are laid again when a page is drawn, its text is
  read or its links are asked for. The most recent are kept, up to 32 MiB by
  `Block::layout_cost` (`MAX_LAID`); past that the oldest go until half is
  left. Laying again uses the same blocks, type and fonts, so pages, places
  and selections are identical (`a_page_laid_again_draws_and_reads_the_same`).
  Only pieces in view are laid, so a tile of a page lays only its part. The
  fonts sit behind a lock, so workers lay one block at a time; drawing
  doesn't hold it.
- **Blocks capped, not layout.** With layouts no longer kept, the 1 GiB
  estimate is gone. What stays per block is its text and line boxes, about
  300 bytes for a one-word paragraph (`docs/memory.md`), so a book of more
  than 1,048,576 blocks (`MAX_BLOCKS`) is refused as too large.
- **Pictures keep their colours.** In every theme a book's pictures are
  drawn again over their boxes untoned, as mareader leaves them. Settings ›
  Fonts › Theme pictures tints them with the page instead (the ink dial
  still leaves them at full strength). PDF pages are themed whole, as before.
- **Column Width.** mareader's dial, 60 to 140 % in Settings › Layout ›
  Pages while a book is open. The text column scales, never below 160
  units, and the page grows or shrinks with it, keeping its margins. Each
  step re-sets the book off the UI thread and keeps the reading place.
- **Kept as built:** one binding-side rule for every view (§ View modes as
  built), and the line-end hyphen hanging just past the column's edge.

## Text files as EPUBs (owner request, 2026-10-14)

Markdown and plain text open by converting them to an EPUB in memory,
then opening that EPUB like any other book. Every EPUB setting applies,
with no second pipeline (`crates/epub/src/convert/`).

**What other readers do.** calibre's TXT input picks a paragraph style
(a blank line, an indent, or every line), can unwrap hard-wrapped lines,
detects chapter headings with heuristics, and reads Markdown with
footnotes, tables and a contents list. pandoc starts a chapter at each
`#` heading, builds the contents from headings to `###`, takes the title
and author from front matter, and packs the pictures. calibre also
guesses legacy encodings. Each is built in here.

- **Which files.** mareader's extensions: `.txt` and `.text` are plain
  text; `.md`, `.markdown` and `.mdown` are Markdown. They are told by
  name, because text has no signature; every other file is still told by
  its content. The open dialog offers them. A file is capped at 64 MiB
  (`MAX_TEXT`); past that it is refused as too large, like a large EPUB.
- **Encoding.** A byte order mark wins (UTF-8 or UTF-16), then valid
  UTF-8, then chardetng's guess (Windows-1252, GBK, Shift_JIS and so on)
  decoded by encoding_rs. mareader read everything as lossy UTF-8.
- **What gets built.** An EPUB 3 container with `content.opf`, a nav
  document and one XHTML file per chapter, stored uncompressed in memory.
  Nothing is written to disk. Each chapter starts a new page, as in any
  book.
- **Markdown.** pulldown-cmark with GitHub's flavour, as mareader rendered
  it: tables, task lists, strikethrough, footnotes, alerts, definition
  lists, and `{#id}` on headings.
  - **Chapters and contents.** Each `#` heading opens a chapter, as in
    pandoc; text before the first one is a chapter of its own. Headings
    `#` to `###` make the contents.
  - **Ids and links.** Ids follow GitHub's slugs, so `[x](#a-heading)`
    lands even in another chapter. Footnote marks link to their notes,
    which sit where they were written, under a rule.
  - **Title and metadata.** The title comes from front matter, else the
    first heading to `###`, else the file name. The author and language
    come from front matter too; YAML or TOML, as mareader read it.
  - **Pictures.** Local pictures (relative or absolute paths, PNG, JPEG,
    GIF or WebP) are read from beside the file and packed: 32 MiB each,
    256 MiB in all. Web pictures aren't fetched; their alt text shows.
  - **Shown as text.** Task boxes are drawn as □ and ■, which every
    bundled face has, in place of the bullet, as on GitHub: the XHTML
    reader now honours `list-style: none`, which EPUBs use too. Raw HTML passes through to the hostile-proof XHTML
    reader, as pandoc and calibre pass it; it runs no scripts.
- **Plain text.** Markup is never read, as in mareader: `# x` is two
  characters.
  - **Paragraphs.** As calibre's auto mode decides: blank lines if there
    is one per 20 lines or more; else an indent starts a paragraph if one
    line in 20 is indented; else every line is one.
  - **Wrapped lines.** A paragraph joins into one when at least four
    breaks in five fell where the next word wouldn't fit the file's width
    (its 95th-percentile line). Verse breaks where the next word would
    have fit, so it keeps its lines, and their indents. A word hyphenated
    at a wrap joins without a space.
  - **Headings.** One-line paragraphs: `Part`, `Book` or `Volume` with a
    number are level 1; `Chapter`, `Act`, `Section`, `Canto`, `Scene` or
    `Letter` with a number, a word such as `Prologue` or `Epilogue`, or
    `第…章` style lines are level 2. So are a lone numeral where blank
    lines are the paragraph breaks, and a line in capitals with two or
    more blank lines before it (`COMMENT ON THE POEM.`, but not a
    signature after one). Each heading opens a chapter. A file with none
    gets one entry, its title.
  - **Title.** The file name, as calibre names a text file.
- **Not built.** The converted EPUB isn't saved: a library copy can come
  with M5 if wanted. Strikethrough shows as plain text, as `<del>` does
  in any EPUB here. The system isn't asked to open `.txt` or `.md` with
  mareader; mareader registered only `.pdf`.
- **Proof.** `crates/epub/tests/convert.rs` opens Project Gutenberg's
  *The Raven* as plain text (CRLF, wrapped prose, verse, headings in
  capitals) and `river.md`. Unit tests in `convert/tests.rs` cover
  encodings, paragraph styles, headings, front matter, chapters, links,
  footnotes and pictures. The session tests open both formats by name.

## Revision history

| Date | Summary |
|---|---|
| 2026-10-13 | Created with the M0 foundation |
| 2026-10-14 | Added the XHTML subset as built in M4 slice 2 |
| 2026-10-14 | Added typography as built in M4 slice 3 |
| 2026-10-14 | Added view modes as built in M4 slice 4 |
| 2026-10-14 | Added navigation as built in M4 slice 5 |
| 2026-10-14 | Added selection and ink as built in M4 slice 6 |
| 2026-10-14 | Added hostile books as built in M4 slice 7 |
| 2026-10-14 | Added the follow-ups as built: layout on demand, pictures, Column Width |
| 2026-10-14 | Added text files as EPUBs: Markdown and plain text converted on open |
