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

sha256 of the book: `27f25284b9b9bec5fa1f4fd282efd1d98cd013df833bb524bb53e353930e7c00`

sha256 of `direction.epub`: `d10c8ded2d32f023152ac04b8296d2b533bcaeb5d01f6c38ba0580341d49e8db`

sha256 of `vertical.epub`: `18aadeea5e9ac0d666d5fddc2b5537cb4348ab481945b52bf72d5fd7472231f8`

sha256 of `fxl.epub`: `85764517a37055bb1ffea996559fad292c7067a9f66a5cf4c4357ca056333361`

## Text files

Markdown and plain text open converted to EPUBs (`docs/epub.md`).

| File | What it is | Why it's here |
|---|---|---|
| `the-raven-gutenberg-17192.txt` | The same book's Plain Text UTF-8 edition (66 KB), fetched 2026-10-14 from <https://www.gutenberg.org/cache/epub/17192/pg17192.txt> | A text file as published: CRLF line ends, prose wrapped near 75 columns beside verse, headings in capitals framed by blank lines |
| `river.md` and `river-dusk.png` | Written for these tests | Front matter, chapters, links between them, a footnote, a table, code, a task list, an alert and a local picture |

sha256 of the text file: `6f0bb853f923de9804d9e65eb78d57b4046e2b6addcd21f217e8941e44030d13`
