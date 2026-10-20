# Thumbnails: picking pages

The sidebar's thumbnails do more than jump. Pages picked there can be
copied or given to page AI as groups; a later slice adds a vocabulary
export.

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
