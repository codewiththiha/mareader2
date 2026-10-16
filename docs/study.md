# Study tools (M6)

Reading aids: explain a word in place, keep it marked, look words up in
offline dictionaries, and shade words above a chosen CEFR level. The parity
entries are `study.*` in `parity/mareader-features.toml`.

## What mareader does

- **Selection menu.** Any non-empty selection gets a small icon bar
  centred 10 px under it: Copy, Explain with AI, Look up. It hides while a
  card is open and when the selection scrolls away. A right-click on a live
  selection belongs to the bar, not the system menu.
- **Explain.** Only one word: trimmed, no inner whitespace, at most 60
  characters; anything else gets a toast saying so. The word and about 60
  characters either side go to the model, which answers part of speech,
  meaning in context, 2–5 synonyms and 2–3 usages, as JSON.
- **Marks.** Explaining a word marks it where it stands. The same word in
  the same spot reuses its mark. A click on a mark opens its card again; a
  right-click offers "Remove highlight", with Undo in the toast. Long press
  selects many marks to remove at once.
- **Card.** The word, then Meaning, Synonyms (chips) and Usages; a shimmer
  while waiting; an error with Retry when retrying can help. Escape or a
  press outside closes it. Answers are saved with their mark (see "Saved
  answers" below); failures are not kept.
- **Answers.** Apple Intelligence through the `fm-bridge` helper on macOS
  with Apple silicon; elsewhere the card says why it can't answer.
- **Dictionaries.** Parquet packs downloaded with mirrors and resume,
  converted to SQLite, looked up on click or hover.
- **CEFR.** A tagger and a word-level table shade words at or above a
  chosen level; a click explains.

## Where it lives

| Piece | Home | Why |
|---|---|---|
| The one-word rule, context, the answer's shape and limits, the `Explainer` contract | `crates/study` (L1) | pure rules, no egui, tested alone |
| Mark records | `crates/library` | every synced row lives in one store, merged and absorbed alike (`docs/sync.md`) |
| Bar, card, mark layer | `crates/ui/src/study/` | egui; one place hooks into the page view |

The `study` crate is the "gloss requests and marks" row of
`docs/architecture.md`; the marks' records join the library's rows so a
sync carries them with the books they belong to.

## Marks

A mark is a synced record: an id, its book (stamped, so merging two
copies of one book can move marks), the word, its context, where it is,
when it was made, and `alive` (stamped, so a removal and an Undo merge
like a membership). Unknown fields from newer builds are kept.

Where a mark is depends on the page:

- **A PDF** keeps the page and the word's box in page units. Text read
  again lands in the same place, whatever the zoom.
- **A book** keeps the block and the byte range in its text, the same
  terms as a position. Re-typesetting moves the word to another page; the
  mark follows, because the page is found from the block each time.

Every character of a book's page knows its block and byte, so a selection
turns into a range and a range back into boxes on whichever page shows it.
Table cells have no such place; a word in one is explained, not marked.

## The answer

`Explainer::explain(word, context)` answers once, on a worker thread:
`Ok(WordInfo)` or an error with a kind, a message for the card and whether
Retry can help. `WordInfo` is clamped as mareader clamps it: meaning 1200
characters, 16 synonyms of 80, 8 usages of 400, cut text ending in "…".

- `Unavailable` explains nothing and says why. It is what every build has
  until the Apple bridge lands.
- `Canned` answers from a small table, for tests and shots
  (`explain=canned`); the word `fail` gives a retryable error.

## Slices

| # | Slice | Check |
|---|---|---|
| 1 | Selection bar (Copy, Explain) on PDFs and books; Explain marks the word and opens its card; marks drawn in the gloss colour and opacity, kept per book as synced records; a click opens a mark's card, a right-click removes it with Undo | select a word, click Explain: it's shaded and a card opens; click elsewhere, click the shading: the card is back; right-click it, Remove highlight, then Undo; `pdf=sample:5 explain=canned selection=0:quick gloss=open` in shots |
| 2 | Apple Intelligence through `fm-bridge` (macOS, Apple silicon, feature `ai`), the card filling as the answer streams, Retry | on a Mac with Apple Intelligence, Explain answers in place |
| 3 | Highlighters (owner, 2026-10-16; see "Highlighters" below): full, half and underline looks; your own highlights from the selection menu on PDFs and books; Settings › Highlights for each highlighter's colour, the menu's buttons and the colour-on-click choice; AI marks on their own reserved look; density; selecting many marks to remove at once | select text, click Highlight or Underline: it stays after a relaunch; add Half in Settings and it joins the menu; change a colour and the highlights follow while the slider moves |
| 4 | Dictionary packs: download with mirrors and resume, convert, Look up in the bar, a lookup card | download a pack, select a word, Look up |
| 5 | CEFR shading: the dataset and tagger, a level setting, click to explain; drawn in its own reserved highlighter and colour (red, as mareader's, by default) | turn CEFR on at B2: harder words are shaded, and that look is no longer offered for your own highlights |

Look up stays out of the bar until slice 4 gives it something to do.

## Slice 1 behaviour

- **The bar** sits under the selection's last line, centred on it, or above
  its first line when there's no room below. It shows once the button is
  up, for any non-empty selection in the focused pane, and goes with the
  selection. Copy writes the clipboard and toasts "Copied".
- **Explain** checks the one-word rule (surrounding punctuation trimmed),
  marks the word unless the same word in the same spot is marked, clears
  the selection and opens the card. With no library (a browser build),
  the card opens without a mark.
- **The card** opens under the mark (above when there's no room), 320
  points wide, and closes on Escape or a press outside it. A mark's answer
  is asked once, then saved with it; a failed answer is asked again next
  time.
- **Marks** paint as rounded fills under the word in the gloss colour at
  the gloss opacity. A click opens the card; a right-click opens a menu
  with Remove highlight; the toast offers Undo for six seconds.

## Slice 2 behaviour

The owner widened this slice (2026-10-16): answers come from Apple
Intelligence on a Mac or, on every desktop, from an OpenAI-compatible
server, with no `ai` feature to turn on. How tasks, prompts and providers
fit together is in `docs/assist.md`.

- **Settings › AI** (between Dictionary and Workspace) picks who explains:
  Automatic, Apple Intelligence (Macs only), OpenAI, Gemini, OpenRouter,
  Ollama, LM Studio or a custom address. Off a Mac, Automatic reads None
  and Explain says to choose one. Apple Intelligence's line says whether
  the model is ready, downloading, switched off or missing.
- **A server** shows its model, with the preset's default as the hint and
  a Choose menu listing the server's models, and its address: in the main
  group for local and custom servers, under Advanced for the cloud
  presets. Try it explains a sample word and says how long it took, or why
  it failed.
- **API keys** (cloud presets; optional for Custom) are listed by their
  ends with their state: ready, resting after a rate limit, or set aside
  with the provider's reason. Each can be set aside, brought back or
  removed. A password field adds one on Add or Enter. Keys are encrypted
  on this computer and requests take turns among them.
- **The card fills as the answer streams**: the meaning appears while the
  synonyms and usages are still coming. A failure names the reason
  (no key, keys turned away, offline, a model to check, a locked
  keychain) with the provider's own words underneath, and Try again shows
  only where asking again can help.
- **Changing the AI settings** takes effect on the next new word; saved
  answers stay, and Ask again on a card reaches the new provider.
- The browser preview hides the AI tab; `Unavailable` still answers there.

## Saved answers

The owner asked for this (2026-10-16): a click on a mark showed a new
answer each launch, and a settings change dropped every answer.

- **With the mark.** A finished answer is saved in the mark's library
  record as its `gloss` field: part of speech, meaning, synonyms, usages
  and who answered. It survives restarts and, once sync exists, travels
  with the mark, so another machine doesn't pay to ask again. Answers
  that merge keep the later one.
- **Who answered.** The card's small print names the provider and model,
  e.g. "Ollama · gemma4:31b-cloud", or "Apple Intelligence".
- **Ask again** on a finished card asks the AI chosen now. The saved
  answer stays until the new one arrives and replaces it, so a failed
  retry loses nothing.
- **Removing a mark** hides its answer with it; Undo brings both back.
  Explaining the same spot afresh makes a new mark and asks again.
- **Failures are never saved.** A book outside the library keeps its marks
  and answers for the session only, as before.

## Highlighters

Asked for by the owner (2026-10-16) for slice 3; the CEFR half lands with
slice 5.

- **Three looks:** **full**, a square fill per
  line of text at half opacity; **half**, the same fill over the lower half
  of the line only; **underline**, a thin line under the text. No
  strikethrough (owner). They replace today's rounded mark fills.
- **Your own highlights.** The selection menu offers Highlight and
  Underline by default; Half can be added. A highlight covers any
  selection, not just one word, on PDFs and books alike, and is a synced
  record like a mark.
- **Settings › Highlights.** Each highlighter's default colour; which
  buttons the selection menu shows and in what order (Copy and Explain
  too); and what a click does: use the default colour, or show a colour
  row each time.
- **Reserved looks.** AI marks (owner) and, from slice 5, CEFR each pick
  a highlighter and a colour. That pair is then not offered for your own
  highlights, so nothing you highlight can pass for an AI mark or a CEFR
  word. AI marks default to a full violet highlight.


## Slice 3 behaviour

- **Records.** A highlight is a mark with `kind: highlight`, its `pen` and
  maybe a `color`; marks saved before keep reading as Explain marks. Its
  spot spans pages: `Chars` keeps a PDF's `[page, char]` from and to,
  `Blocks` a book's `[block, byte]`, so type changes find it again. The
  same text with the same pen is one highlight; another pen is another.
- **Colours.** A highlight made with "Use its colour" keeps none and
  follows its pen's colour in Settings. One picked from the colour row
  keeps it. A kept colour that later meets a reserved look is drawn in
  its pen's colour instead.
- **Drawing.** Fills are square, at Fill strength (50% by default, 20 to
  80%); half fills the lower half of each line; underlines are solid, about
  8% of the line high. Highlights are drawn first and AI marks over them.
- **Clicks.** A click on an AI mark opens its card, as before; a click on
  your highlight opens its menu with Remove highlight, with Undo.
- **The bar** shows the menu's buttons in order, each pen's button with
  its colour underneath. With every button hidden no bar shows; Copy's
  shortcut still works. With "Choose a colour each time" a pen opens a
  colour row: its own colour, then the palette, without reserved pairs.
- **Settings › Highlights.** Each pen's colour (eight preset colours or
  a custom one), Fill strength, the click choice, the menu's buttons with
  show switches and up and down, then the AI's pen, colour and the card's
  spacing. Reserved pairs show crossed out. The AI cannot take a pen's own
  colour; if the AI moves to a pen whose colour it holds, that pen moves
  to the next free colour.
- **Left for later.** Selecting many marks to remove at once moves to the
  next study slice.
- **Checks.** Unit tests cover the records, old rows, spans and reserved
  pairs; kittests highlight and underline from the bar, remove with a
  click, pick from the colour row, set the tab, and stage highlights with
  the `highlight=full|half|underline` option after `selection=`.
