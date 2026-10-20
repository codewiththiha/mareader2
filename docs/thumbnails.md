# Thumbnails: picking pages

The sidebar's thumbnails do more than jump. Pages picked there can be
copied, given to page AI as groups, or made into a word list of their
hard words.

## Picking

- **Long-press** a page (0.45 s, held still) to pick it. Letting go doesn't
  jump to the page.
- While anything is picked, a **click** picks or lets go of a page. With
  nothing picked, a click goes to the page as before.
- **Cmd/Ctrl-click** picks without a long-press. **Shift-click** adds every
  page from the last one picked.
- A bar over the grid says how many pages are picked, with **All** and
  **Clear**. **Esc** clears too, before it would close the sidebar.
- Picked pages get the accent ring, a light wash and a tick in the corner.
- Picks belong to the focused pane. Focusing another pane or opening another
  document clears them. When page AI results are laid in or out of a book,
  the same pages stay picked, renumbered.

## The menu

Right-click a picked page for the picks' menu:

- **Copy text**: the picked pages' text, in reading order, a blank line
  between pages.
- **Copy word list**: the picked pages' words above your level, with their
  dictionary meanings, as Markdown. See [Word lists](#word-lists).
- **Summarize**, **Translate to {language}** (the last language picked) with
  **Other language…**, or **Translate…** before one is picked, and
  **Simplify**: page AI over the picked pages, a group at a time. See
  [Page AI › Picked pages](page-ai.md#picked-pages).
- With no model set up, the three rows give way to **Set up AI…**, which
  opens Settings › AI.
- **Clear selection**.

The picks stay after an AI run, so the same pages can be copied or asked
again.

Right-click a page that isn't picked for **Copy page text** and **Select**.

Copying reads the pages off the frame when a PDF's pick is long, so the
reader never stalls; a toast says when the text is on the clipboard. Pages
without a text layer are skipped; if none has text, the toast says so.

## Word lists

A word list holds the picked pages' **hard words**: the ones shaded above
your level (Settings › Vocabulary). It needs the word levels downloaded;
without them a toast says where to get them.

- **Each word once.** A word is listed on the first picked page it shows
  on; later pages don't repeat it. Case and curly apostrophes don't make a
  new word.
- **Meanings.** Each word is looked up in up to three dictionary languages,
  in the order chosen; each gets a column (or a line). A language whose
  dictionary isn't downloaded is left out, and the toast names it. With no
  languages, the list holds the words alone.
- **Fields.** The CEFR level, part of speech, definition, romanization and
  sense can each be kept or left out. A cell keeps up to four meanings.
- **Order and groups.** Reading order (the default), A→Z, Z→A, or hardest
  first. Groups are the outline's top chapters (none when the book has no
  outline), each page, or none.
- **Near words.** With *similar words* on, dictionary words near each word
  come too: what it may be inflected from ("running" → "run") and longer
  words of its family ("ephemeral" → "ephemerality"). Near spellings are
  tried only when neither is found. Each sits under the word that found it,
  as "↳ ephemera *(like ephemeral)*" in a table or "Near *ephemeral*:" in a
  list, and shows once. A near word that is itself a listed word is left
  out.
- **Layout.** A table (a row per word) or a list (a paragraph per word). The
  book's title can head it, and each word can carry its page.

**Copy word list** uses the instant options (Settings keeps them; the
preview that edits them and saves presets comes next), copies the
Markdown, and toasts how many words it holds.

The pages are read off the frame when a PDF's pick is long; levels are
asked 2,000 words at a time and meanings 48 words at a time, so the
dictionary card still answers while a long list is made.
