# PDF files

How mareader2 treats PDFs that are odd, broken or locked.

## Opening: what can go wrong (2026-10-16)

hayro reads what it can: junk before `%PDF-` is skipped, a broken
cross-reference table is rebuilt from the objects, and a file locked only
against editing (an owner password) opens, with a silent empty password
unlock. Only when hayro refuses does `pdf::check::diagnose`
look at the bytes to name the cause:

| Error | When | What the toast suggests |
| --- | --- | --- |
| `EmptyFile` | nothing but whitespace | copy or download it again |
| `NotPdf` | no `%PDF-` in the first kilobyte | it may be a web page or another file named .pdf |
| `CutShort` | no `%%EOF` in the last kilobyte | download it again |
| `Damaged` | a header and an end, but unreadable, or the parser panicked | another app may save a readable copy |
| `Locked` | a user password is needed | an app that can unlock it may save an unlocked copy |
| `Encryption` | an unknown or broken encryption dictionary | as for `Locked` |
| `NoPages` | the page tree is empty | none |
| `TooManyPages` | more than `MAX_PAGES` | none |

The checks run only after a refusal, so they never turn away a file hayro
can read. The toast reads "Couldn't open name: reason. Suggestion."

There is no password prompt (owner, 2026-10-16): a locked file is named as
locked and nothing more.

## Opening a cut-short file

A file without `%%EOF` near its end that hayro still reads opens as usual,
and a warning toast (the alert in the accent colour) says "name looks cut
short; some pages may be missing." `Document::cut_short` carries the fact.
A cut-short file with no readable pages is `CutShort`, not `NoPages`.

## Text strings

Titles, authors and outline entries go through `strings::decode_text`:

1. A BOM picks UTF-16 BE, UTF-16 LE or UTF-8.
2. Bytes that are valid UTF-8 read as UTF-8, as many writers skip the
   BOM. Bytes 0x18–0x1F are PDFDocEncoding accents, so a string
   with them skips this step.
3. Anything else is PDFDocEncoding: Latin-1 except 0x18–0x1F, 0x80–0x9E
   and 0xA0 (`€`), which map to accents, dashes, quotes, ligatures and the
   like.
4. Tabs and line breaks become spaces; other control characters and stray
   BOMs are dropped.

Not done: a loose UTF-8 step that accepts one replacement character and
strips it (a lone Latin-1 `é` would vanish), and a GB18030 fallback, which
would need large tables.
