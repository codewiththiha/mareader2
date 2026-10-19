# Page AI: summarize, translate, simplify

A right-click on a page offers three tasks that act on that whole page:
**Summarize**, **Translate** and **Simplify** (plain English, about B1).
The result covers the page, and the page itself is one click away. The
owner set the behaviour on 2026-10-19; it is new to mareader2, so there is
no parity entry.

## Asking

- **Where.** Anywhere on a page except a selection or a mark, including on
  text when nothing is selected. Those keep their own menus.
- **No model yet.** The menu says what the tasks do and offers
  "Set up AI…", which opens Settings › AI.
- **Translate.** "Translate to X" uses the last language picked; "Other
  language…" (or "Translate page…" before the first pick) opens a list
  with a search field, recent languages first. A pick is remembered, with
  the last five.
- **This page or continuous.** Translate and Simplify follow their
  setting: ask each time (the default), this page, or continuous. Asking
  shows a second step with "This page", "Continuous" and "Remember my
  choice", which saves the answer as the setting. A task set to
  continuous says so beside its row.
- **Already running.** The same task on the same text just shows the run
  under way; it is not asked twice.

## The result

- **Over the page.** The result is drawn on the pane's paper colour, so
  it matches the theme, texture and blend mode, and fades in over 0.3 s.
  The text underneath can't be selected or marked while it shows.
- **Type.** A book's result is set in the pane's own typography, in the
  page's text column. A PDF's result uses the pane's font, scaled to the
  page, inside an 8% margin. PDF results are text only.
- **Fitting.** A longer result, often a translation, is set smaller until
  it fits the page, down to 72% of the reading size (never under 9). Past
  that it scrolls inside the page with the wheel, and fades at the cut
  edge.
- **Streaming.** Words appear as they come, set again at most every 0.4 s.
  A redo keeps the old result on the page until the new words arrive.
- **Bubbles.** Each result on a page gets a numbered bubble at the page's
  end side. A click shows that result; a click on the active bubble shows
  the page again. A turning arc marks a result being written, "!" a failed
  one, and a dot one made for another layout of the book (see below).
- **A bubble's menu** (right-click): show it or the page; redo; copy the
  text; delete. While writing: show as it comes, or stop. Failed: the
  reason, try again, or dismiss. It also says which model made the
  result, and when.
- **Failures.** A toast names the reason in the reader's words. The page
  goes back to the original, and the bubble keeps the reason.

## Continuous mode

For Translate and Simplify. The aim: pages are ready as the reader gets
to them, without paying for pages they skim past.

- **The pill.** A small pill at the pane's bottom, above the page bar,
  names the run ("Translating to Burmese"), shows a ring, and has ✕ to
  stop. Its tooltip says what it is doing in a sentence.
- **Staying on a page.** The ring fills only while the page's result is
  on screen: by default 25 s, 5–120 s in Settings. Scrolling past pages
  never fills it, because it starts again on each page and only once that
  page's result shows.
- **Asking ahead.** When the ring fills, the next pages are asked
  together, up to "pages at once" (default 5, 1–16), and never more than
  the model can run at once (its "at once" times its usable keys). After
  that it tops up as you read: each full ring asks only as many pages as
  keep that many ready ahead. So at most that many pages are asked or
  being written, and it never runs far ahead.
- **Pages already done** with the same task and text show their kept
  result instead of being asked again. Pages with no text are skipped.
- **Reading past it.** On a page beyond what was asked, but still near,
  the run asks that page once it has stayed still for a moment.
- **Going far away.** More than "pages at once" + 2 pages outside the
  run's pages pauses it ("· paused"). Once the page stays still for
  1.2 s, the pill asks "Continue translating here?" with Continue and
  ✕. Continue carries on from this page; ✕ stops until the next manual
  run. Coming back near the run's pages resumes it by itself.
- **Stopping.** ✕ on the pill, "Stop continuous mode" in the page menu, or
  a new run in the same pane. Pages ahead that were never reached are let
  go: their requests stop.
- **Ends.** A page that fails stops the run, and its toast says so; that
  page's bubble offers a retry. A new layout (the book's text size
  changed) starts the run again from the page shown. Closing the pane, or
  opening another book in it, ends the run. At the book's end the pill
  says so.
- **Reading ahead in PDFs.** Pages not yet on screen are read on a thread,
  so asking ahead never stalls a frame.

## Fewer tokens

- **Placeholders.** Code blocks, long inline code, pictures, tables and
  long URLs are sent as `[[c1]]`-style tokens with tiny hints, never their
  content. The answer's tokens are put back as the real thing; any the
  model dropped go back at their place in the source. Summaries keep only
  the ones they mention.
- **Verse.** Preformatted blocks are treated as code only if they look
  like code, so poems are still translated.
- **PDF clean-up.** Page numbers and running heads are dropped, and words
  split across lines by hyphens are joined.
- **Size.** A page sends at most about 16,000 characters. Summaries ask
  for 40–220 words. Blank or picture-only pages make no request.

## Kept with the book

Results are saved in the library (`crates/library/src/ai_pages.rs`), in
the same sync-ready rows as marks: ids, stamps and tombstones, so they
survive restarts and can sync later. A row holds the task (with its
language), the text range it was made for, a key of the source text, the
result, the model, and when. A new result for the same task and range
replaces the old one. Deleting tombstones it.

- **Books** anchor a result to its text range (block and offset), not a
  page number. After a re-layout, its bubble shows on the page that now
  starts that range. If the page no longer covers exactly that text, the
  bubble is marked as made for another text size and offers a redo.
- **PDFs** anchor to the page.
- **Files outside the library** keep their results for the session only.

## Settings › AI › Pages

At the end of the AI tab, after the models:

| Row | Default |
|---|---|
| Summarize, Translate, Simplify models | Same as Explain words |
| Summary style | Paragraphs (or key points) |
| Translate to | Picked when first translating |
| Translating, Simplifying | Ask each time |
| Time on a page | 25 s |
| Pages at once | 5 |

## Where it lives

| Part | Path |
|---|---|
| Prompts, placeholders, reading answers, PDF clean-up | `crates/assist/src/page.rs` |
| The request, partials and retries | `Assist::rewrite` in `crates/assist/src/service.rs` |
| Page types shared with the UI | `crates/study/src/page.rs` |
| Saved results | `crates/library/src/ai_pages.rs` |
| Runs, results, bubbles' state | `crates/ui/src/page_ai/mod.rs` |
| Continuous mode | `crates/ui/src/page_ai/follow.rs` |
| A page's text, and results back to blocks | `crates/ui/src/page_ai/extract.rs` |
| Typesetting results off the frame | `crates/ui/src/page_ai/cards.rs` |
| Drawing cards, bubbles and the pill | `crates/ui/src/page_ai/paint.rs` |
| The page and bubble menus | `crates/ui/src/page_ai/menu.rs` |
| Settings | `crates/settings/src/ai.rs`, `crates/ui/src/settings_window/ai.rs` |

## Tests and shots

- `crates/assist/src/page_tests.rs`: placeholders kept, dropped and
  invented; answers read leniently; PDF clean-up.
- `crates/ui/src/page_ai/extract_tests.rs`: a page's text and blocks.
- `crates/ui/tests/page_ai.rs`, headless, through the real menus. Without
  a model the menu offers setting one up; with one, the three tasks. A
  picked language is remembered. Marks keep their own menu. A kept result
  covers the page until the original is asked for. Asking how, and
  remembering continuous. A task set to "this page" doesn't ask. The pill
  stops a run. Going far away pauses, then Continue carries on there. A
  five-second stay asks exactly "pages at once" pages ahead.
- With `explain=canned`, page runs answer at once with the page's own
  text in capitals, so tests need no AI.
- Shot options: `page_ai=3:simplify` keeps and shows a result on page 4
  (0-based, a language tag translates); `page_ai=follow:simplify` starts a
  canned continuous run on the first page; `right=x,y` right-clicks once
  open; `wait=frames` runs that many frames before the shot. For example:

```sh
python3 tools/ci.py shots \
  "ai-pdf pdf=sample:20 workers=0 page_ai=0:simplify" \
  "ai-menu pdf=sample:20 workers=0 provider=ollama right=600,400" \
  "ai-ask pdf=sample:20 workers=0 page_ai=follow:simplify goto=15 wait=110"
```
