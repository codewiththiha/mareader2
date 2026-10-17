# UX audit

Every screen of the desktop app, checked against published guidance,
with what is wrong, why it matters and the fix. Findings come from
screenshots of the real app (the CI shot harness, 1200 × 800 at 2×).
Severity: **P1** confuses or blocks, **P2** slows or looks broken,
**P3** polish.

## Guidance applied

- **Icons need visible labels.** Outside a tiny universal set (search,
  close, print) icon meaning is guessed; hover tooltips are a weak
  fallback, slow and absent on touch. (NN/g, *Icon Usability*;
  *Tooltip Guidelines*: if a few icons get labels, all should.)
- **Anchored popovers point at their trigger.** The caret sits on the
  anchor's midline, not the card's, so it still points correctly when
  the card is pushed back inside the window. It mirrors when the card
  flips sides, shares the card's fill and border, and the card keeps
  8–12 px from its anchor. A card must never cover the thing it
  explains. (Floating UI / CSS anchor positioning write-ups; Base UI
  `Popover.Arrow`.)
- **Lookup text reads at reading size.** A dictionary definition is
  content, not a UI label. Definitions set at label size, and a panel
  covering the word, are common complaints about lookup panels.
- **Recognition over recall, consistency, minimalism** (Nielsen's
  heuristics). One control per job, in one place. Related controls
  sit together, rare ones go last.
- **Empty states teach the next step**, and say what the app accepts.

## 1. Lookup card and AI card: fixed in this slice

| # | Was | Why it hurt | Now |
|---|-----|-------------|-----|
| 1.1 P1 | No caret. The AI card floated beside the word and could land far from it. | Nothing tied the card to its word, so readers lost their place. | Both cards share one anchored bubble. The caret sits on the word's midline (kept clear of the rounded corners) and mirrors when the card flips. |
| 1.2 P1 | A card too tall for both sides slid over the words. | It hid the word being explained. | The card takes the side it fits, else the roomier side, and its content scrolls. It never covers the word. |
| 1.3 P2 | The side was guessed from a fixed 240 pt before measuring, so a card could jump on its second frame. | Visible jitter. | Every new card measures in an invisible first pass and opens on the right side. It keeps that side while paging, unless the new entry no longer fits. |
| 1.4 P2 | The lookup card was a flat stack: the word, the languages beside ↑ ↓ arrows, then the entry. | Up and down arrows suggest scrolling, not paging; the pager crowded the header. | Header (word, close, languages) / entry / footer `‹ 2 of 5 ›`, split by full-width hairlines. The footer appears only when there is more than one entry. |
| 1.5 P2 | Definitions were set at UI label size. | Hard to read, especially Burmese. | The answer reads 15 % larger. Tags sit left and "via English" right, on one quiet line. |
| 1.6 P3 | Cards appeared at once. | Abrupt. | A 160 ms ease-out rise of 4 pt plus fade, off when reduced motion or animations are off. |

## 2. Selection bar

| # | Issue | Why | Fix |
|---|-------|-----|-----|
| 2.1 P1 | It sits on the next text line and starts at the selection's left edge. | It covers what you are reading and points at nothing. | Put it in the same bubble: centred over the selection with a caret, above by default, below when there is no room. |
| 2.2 P2 | Five unlabelled icons: copy, explain, look up and two pens. | Explain and look-up icons are not universal. | Make Look up and Explain text buttons, keep the pens as swatches, and separate the groups with a divider. |

## 3. Title bar and sidebar header: duplicated, crowded

| # | Issue | Why | Fix |
|---|-------|-----|-----|
| 3.1 P1 | With the sidebar docked, Search, Settings and a pin appear in both the sidebar header and the title bar. | The same control in two places looks like two different things. | When the sidebar is docked, the title bar drops the controls the sidebar header already has. |
| 3.2 P1 | The same pin icon means "Dock sidebar" in one place and "Pin title bar" in the other. | One icon, two meanings. | Use the panel-left icon for docking. The title-bar pin moves to View ▸ "Keep title bar shown". |
| 3.3 P2 | Five right-side icons (search, settings, appearance, pin, menu) plus the window buttons. | Visual noise around the book title. | Keep three: Search, Appearance, Menu. Settings goes into the menu, with its shortcut. |
| 3.4 P3 | A darker band shows between the pinned title bar and the page in dark mode. | It looks like a rendering seam. | Match the band to the reader background, or remove the inset. |

## 4. Sidebar panels

| # | Issue | Why | Fix |
|---|-------|-----|-----|
| 4.1 P1 | The bottom tabs are icon-only (pages, contents, library, dictionary). | The panel names are not guessable. | Add a short label under each icon (Pages · Contents · Library · Words), at least in the docked sidebar. |
| 4.2 P2 | Thumbnails: the current page number is a blue circle, and a landscape page throws its row out of line. | Inconsistent markers and a ragged grid. | Show the current page with an accent ring around the page and plain numbers. Centre each page in a fixed cell. |
| 4.3 P2 | Contents: chapters and sections share one weight; only the indent tells them apart. | The structure is hard to scan. | Medium weight for top-level entries, regular for nested ones, muted page numbers. |
| 4.4 P2 | Library panel: a "LIBRARY 8" uppercase header, the same book icon on every row next to a format badge, and a missing book greyed out with no reason. | Repetition and a mystery state. | Use sentence-case headers. Drop the per-row icon, keeping the badge. Missing books get "Moved or deleted" on hover and a Locate action. |
| 4.5 P2 | Dictionary panel: "Dictionaries need the desktop app." sits flush left with no padding. | Looks broken. | Give it the standard panel padding and the empty-state layout: icon, line, action. |

## 5. Reader canvas

| # | Issue | Why | Fix |
|---|-------|-----|-----|
| 5.1 P1 | The empty reader and empty split pane say "Drop a PDF here". | EPUB, Markdown and text files open too. | "Drop a book here — PDF, EPUB, Markdown or text", plus Open… and the three most recent books. |
| 5.2 P2 | Split: no visible divider, a harsh 2 px blue outline on the active pane, and PDF pages left-aligned with no gutter. | Heavy focus chrome and an unbalanced page. | A 1 px divider with a wider grab zone. Show the active pane with a subtle top accent line or dim the other pane. Centre the pages. |
| 5.3 P3 | The page pill "1 / 20" is a dark grey slab. | Low polish next to the glass title bar. | Glass pill matching the title bar, with tabular numbers. |
| 5.4 P2 | EPUB: a faint running title out of line with the text column, a hairline progress sliver, and a picture page cut off. | Looks unfinished. | Align the running title to the text column. Use a 2 px track with a percentage on hover. Fit images to the page box. |

## 6. Appearance popover

| # | Issue | Why | Fix |
|---|-------|-----|-----|
| 6.1 P2 | "Presets" header above a "CLASSIC" sub-header. | Two headers for one group. | A single "Presets" header. |
| 6.2 P2 | Five presets in a 4-column grid; Cinema sits alone on the second row. | An orphaned tile looks accidental. | A 5-across row of smaller swatches, or 3 + 2 centred. |
| 6.3 P2 | "Strength" and "Hue" labels differ in size and indent; units are "0 %" with a space and "34°" without. | Inconsistent rhythm. | One label style and one unit style ("0%", "34°"). |
| 6.4 P1 | Texture rows are clipped at the bottom with no scroll cue. | Options look missing. | Size to content up to the window, with a fade at the scroll edge. |

## 7. View menu

| # | Issue | Why | Fix |
|---|-------|-----|-----|
| 7.1 P2 | The view mode is four unlabelled icons. | Scroll, spread, single and continuous are not guessable. | A segmented control with labels, or icons with a caption of the current mode. |
| 7.2 P2 | Zoom, fit buttons, view modes and auto-scroll share one block. | Unrelated things grouped together. | Separate groups: Layout (mode, spread), Zoom (− 100% +, Fit width, Fit page), Reading (auto-scroll, full screen). |

## 8. Settings window

| # | Issue | Why | Fix |
|---|-------|-----|-----|
| 8.1 P1 | Eight tabs, but only the active one shows its name. | Mystery-meat navigation; NN/g's top icon finding. | A left sidebar list with icon + label. It fits the 1000 pt window and grows without crowding. |
| 8.2 P2 | Tab order: Layout, Theme, Animations, Fonts, Highlights, Vocabulary, Dictionary, AI, Workspace. Tabs also appear and vanish (Fonts, Animations). | Rare items sit early, and a moving tab bar breaks muscle memory. | Order by use: Appearance (theme + fonts), Layout, Highlights, Words (vocabulary + dictionary), AI, Workspace, Motion. Keep every tab; show "Open an EPUB to change its fonts" instead of hiding Fonts. |
| 8.3 P2 | The Theme tab holds only "Open Appearance" and a Blend section. | A tab that redirects elsewhere. | Merge it with Fonts into Appearance, embedding the presets and tint controls. |
| 8.4 P2 | A fixed-height window leaves half the panel empty on short tabs and clips long ones mid-row with no fade. | Wasted space; hidden options. | A scroll fade at the bottom edge, and section cards that end where the content ends. |
| 8.5 P2 | Dropdowns use egui's solid triangle; everything else is Lucide. | Mixed icon language. | Lucide chevron-down on all combo boxes. |
| 8.6 P2 | "Font" sits above "Serif font" and "Sans-serif font", with no hint how they relate. | Unclear relationship. | "Body font" first. Put the three family overrides under an "Advanced: book's own families" disclosure. |
| 8.7 P2 | Workspace: "Light, Dark and Dim change every pane" shows on and greyed out at once. | You can't tell whether it applies. | Hide a dependent setting until its parent is on, or indent it under the parent with a line of why. |
| 8.8 P3 | AI: a footnote "Choose a provider to explain words." repeats the section title. | Redundant text. | Drop it. |
| 8.9 P3 | Layout: three floating-label settings sit apart from each other. | Related settings scattered. | One "Page label" group. |

## 9. Library

| # | Issue | Why | Fix |
|---|-------|-----|-----|
| 9.1 P2 | Generated covers print the title and author, and the caption below repeats both. | Every word shown twice. | On generated covers show only the caption, or set the cover text larger and drop the caption's title. |
| 9.2 P2 | Every card shows a progress track, even books not started. | Noise on most cards. | Show the track only after reading starts; show a "New" dot for unopened books. |
| 9.3 P2 | A missing book shows a "Missing" badge and "Moved or deleted". | Two labels for one state. | Keep the badge; move the reason to the hover tip and a Locate… action. |
| 9.4 P3 | The count appears twice: "Library 24 books" and "Search 24 books". | Redundant text. | Keep the count in the header; the search placeholder says just "Search". |
| 9.5 P2 | The "+" card is a blank grey tile, and its menu covers books with no caret. | The tile doesn't say what it does. | "+ Add books" caption under the tile; open the menu in the anchored bubble. |
| 9.6 P3 | "Copied into the library; the originals can go". | Awkward phrasing. | "Copies the files into your library". |
| 9.7 P2 | List view: the "All books" chip sits about 16 pt off the title's edge, and the thumbnails are flat colour squares. | Misalignment; empty thumbs. | Align to one left edge; use the cover thumbnail or a format glyph. |
| 9.8 P2 | Selection bar: "Remove…" in red beside "New shelf from these", and the bar overlaps the last row of cards. | The destructive button looks like a peer, and content is hidden. | Reserve bottom padding while selecting. Move Remove to the end, after a divider, as a quiet danger button. |

## 10. Feedback

| # | Issue | Why | Fix |
|---|-------|-----|-----|
| 10.1 P2 | A toast overlaps the bottom page content and page pill. | Hides content. | Lift toasts above the page pill and stack them from the bottom centre. |
| 10.2 P3 | "Could not open the file" names neither the file nor a next step. | Dead end. | Name the file and offer Retry or Show in folder. |

## Order of work

1. **Anchored cards** (this slice): §1.
2. **Selection bar and title bar**: §2, §3.
3. **Settings window**: §8, the largest, most-seen surface.
4. **Sidebar and reader canvas**: §4, §5.
5. **Appearance, View menu and Library**: §6, §7, §9, §10.
