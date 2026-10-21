# Test books

Real books the EPUB tests open. Each is small and free to redistribute.

| File | What it is | Why it's here |
|---|---|---|
| `the-raven-gutenberg-17192.epub` | *The Raven* by Edgar Allan Poe, Project Gutenberg eBook #17192, the EPUB 2 "no images" edition (53 KB), fetched 2026-10-14 from <https://www.gutenberg.org/ebooks/17192.epub.noimages> | An EPUB 2 book as published: an NCX with nesting and a page list, the cover named by `<meta name="cover">`, a cover wrapper page, `.htm.html` chapters with the XHTML 1.1 doctype |

The text is in the public domain in the USA. The file keeps the Project
Gutenberg License it ships with, unchanged.

sha256 of the file: `8b39cb96cd0a413abb05677a6064bcabc5d07a499f74662e424ae0c0f7a8679e`

| File | What it is | Why it's here |
|---|---|---|
| `frames.epub` | *Frames and Shades*, written for these tests (3 KB) | Box shadows, gradients, a framing picture cut in nine, carved borders, outlines, opacity, hidden text and rules the book styles |
| `values.epub` | *Values and Queries*, written for these tests (4 KB) | Sums and limits, custom properties, `@import`, `@supports`, `@media`, `@layer`, alternate and print sheets, global keywords |
| `fonts.epub` | *Fonts and Figures*, written for these tests (93 KB); its fonts are subsets of the bundled Inter and JetBrains Mono (SIL Open Font License) | The same fonts hidden by the IDPF and Adobe schemes and packed as WOFF and WOFF2, then type features, last-line alignment and unbroken boxes |
| `direction.epub` | *Directions and Notes*, written for these tests (64 KB); its fonts are subsets of DejaVu Sans (DejaVu Fonts License), Noto Sans CJK JP (SIL Open Font License) and the bundled JetBrains Mono | Arabic and Hebrew read right to left with right-to-left page order, ruby over Japanese and Latin bases, and a run in its own font family |
| `vertical.epub` | *Columns Down the Page*, written for these tests (63 KB); its font is a subset of Noto Serif CJK JP (SIL Open Font License) | A Japanese chapter in vertical columns with ruby, combined digits, upright and sideways runs, an inline and a tall picture, then a level English colophon |
| `fxl.epub` | *Pages as Drawn*, written for these tests (4 KB); its pictures are flat colours | Fixed pages at their own viewports: a cover on the far side, boxes placed by `position` and ordered by `z-index` over a background picture, a landscape page, a picture in the spine, then a reflowable chapter |
| `math.epub` | *Lines of Formulas*, written for these tests (2 KB); its picture is a flat colour | MathML in a line, a display formula centred as its own block, a prefixed `m:math`, formulas in table cells, and formulas too deep to set falling back to their `alttext` or `altimg` |
| `media.epub` | *Sound and Fallbacks*, written for these tests (4 KB); its pictures are flat colours and its clips are stand-in bytes | A video with a poster and a song as cards that play outside the reader, a file that must never play, `epub:switch`, `object` and `noscript` fallbacks, an image map, and manifest fallbacks for a picture and a spine item |
| `narration.epub` | *Read to Me*, written for these tests (4 KB); its sound is a generated tone | Media overlays: sentences timed by id and by `xpointer`, clock values in every form, a table read as one, a page number and a note marked skippable, a chapter narrated whole, and the active-class colour |
| `subtitles.epub` | *Words Under the Waves*, written for these tests (4 KB); its clips are stand-in bytes | Subtitle tracks: a video with English WebVTT (default), French SubRip and Burmese captions, a chapters track and a missing file to skip, a song with TTML lyrics, a clip in a table cell, and a clip missing from the book whose words remain |
| `triggers.epub` | *Press to Find Out*, written for these tests (4 KB); its clips are stand-in bytes | `epub:trigger`: a hidden answer shown and hidden, a hint hidden in its line, a tip hidden with `display: none` inside a line, a bordered box hidden, a hidden sound played and muted, a video played, a picture that plays, a hover trigger to ignore, a hidden ending at a chapter's end, a hidden table row, a rule hidden, a `visibility: hidden` picture and an empty hidden `img` shown, a small picture in a line as observer, and a fixed page with a hidden note |
| `scripts.epub` | *Quick Quiz*, written for these tests (4 KB) | Scripted chapters: a quiz with an external script and style sheet, a list that answers its items' clicks, an answer shown and hidden, a checkbox through its label, a text field read by a button and a `noscript` note; then a counter written inline with a timer and an alert, and a plain chapter |

sha256 of the book: `27f25284b9b9bec5fa1f4fd282efd1d98cd013df833bb524bb53e353930e7c00`

sha256 of `direction.epub`: `d10c8ded2d32f023152ac04b8296d2b533bcaeb5d01f6c38ba0580341d49e8db`

sha256 of `vertical.epub`: `18aadeea5e9ac0d666d5fddc2b5537cb4348ab481945b52bf72d5fd7472231f8`

sha256 of `fxl.epub`: `85764517a37055bb1ffea996559fad292c7067a9f66a5cf4c4357ca056333361`

sha256 of `math.epub`: `a9b54ffb7b329a7352072e0379844b7b1a351122880b5b5260f385217f6cb488`

sha256 of `media.epub`: `1c735eea7efc902ce4e1231a3cef6cfb5d4f0c20a8d585fa16578ee5c82c64d8`

sha256 of `narration.epub`: `480889f7ab1f0ca0d752d2e24e74f93112f8ae737bdac3f51470ea420006b645`

sha256 of `subtitles.epub`: `db6ee795ebaea483877a8d3ccdcb0e1b50a5a686caeb892a2bf8ee30b708142d`

sha256 of `triggers.epub`: `2aa2ea9eaa31efb7a821dd4acaa4182cdbbbe7d073d43a4552d32b6f2dd49cda`

sha256 of `scripts.epub`: `3bdb9d8c054c336d1697221cfdc0f3d66347aca9bbcd96eb97d3c440f0c82ddf`

## Text files

Markdown and plain text open converted to EPUBs (`docs/epub.md`).

| File | What it is | Why it's here |
|---|---|---|
| `the-raven-gutenberg-17192.txt` | The same book's Plain Text UTF-8 edition (66 KB), fetched 2026-10-14 from <https://www.gutenberg.org/cache/epub/17192/pg17192.txt> | A text file as published: CRLF line ends, prose wrapped near 75 columns beside verse, headings in capitals framed by blank lines |
| `river.md` and `river-dusk.png` | Written for these tests | Front matter, chapters, links between them, a footnote, a table, code, a task list, an alert and a local picture |

sha256 of the text file: `6f0bb853f923de9804d9e65eb78d57b4046e2b6addcd21f217e8941e44030d13`
