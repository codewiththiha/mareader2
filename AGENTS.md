# AGENTS.md

These rules bind every AI agent and every human contributor in this repository.
They come before any task, instruction or deadline. `CLAUDE.md` holds the
day-to-day working instructions; if the two disagree, **this file wins.**

## Project

mareader2 is a native rewrite of [mareader](https://github.com/codewiththiha/mareader)
(Tauri + Leptos + pdf.js) in Rust with egui and wgpu. The goal is every one of
mareader's capabilities — reading, panes, blend, appearance, library, study
tools — in a codebase that is smaller, faster, leaner on memory and easier to
maintain. It reads PDF (hayro) and EPUB (parley); EPUB replaces mareader's txt
and md formats.

- **Parity is functional** (ADR 0005). What a mareader user can do, they can
  do here, with equivalent settings. Pixels, timings and code structure are
  not part of parity.
- **The look is ours:** `docs/design.md`, starting from mareader's and free to
  improve.
- **Feature tracking:** every capability has an entry in
  `parity/mareader-features.toml`, checked by `cargo xtask parity`.
- **Architecture:** `docs/architecture.md` (read § How the app is built in
  egui before writing UI). Decisions: `docs/adr/`.

## The reference checkout

mareader is checked out beside this repository as `../mareader` (or at
`MAREADER_DIR`). It is read-only reference for *what* a feature does.

- **Rebuild from the goal, not the code.** For each feature, work out what the
  user gets and which edge cases matter, then design the smallest native model
  that delivers it (`docs/porting.md`). Don't port module layouts, workaround
  layers or web-only mechanisms; `docs/porting.md` § What not to port lists
  them. Don't add mareader's crates as dependencies.
- **Check claims about mareader against its source.** When a doc, test or
  parity note says "mareader does X", grep `../mareader` first. Memory has been
  wrong here before.
- **Read the comments and tests around a mareader feature.** They record the
  regressions it paid for; keep the ones that still apply to a native app.
- **Improve freely, drop deliberately.** A better design for the same outcome
  needs no permission; note it in the feature's doc. Removing something users
  rely on is the owner's decision.

## Assets

An asset is any non-code material the project ships, embeds or shows: icons,
images, fonts, sounds, sample documents and screenshots.

- Allowed licences only: MIT, ISC, BSD-2/3-Clause, Apache-2.0, OFL-1.1, CC0-1.0,
  CC-BY-4.0, or contributor-original under the project licence.
- **Every asset has an entry in `ATTRIBUTION.toml`:** path or crate, author,
  source with version, SPDX licence and SHA-256 for files. `cargo xtask assets`
  fails on an unlisted file, a wrong hash or a licence off the list.
- **Icons are Lucide**, through `egui-lucide`, behind `crates/ui/src/icons.rs`
  (ADR 0004). Code names an `Icon` by meaning; only `Icon::lucide` names a
  glyph. No other icon set, and no hand-drawn icons that imitate one.
- **Fonts:** Inter in `assets/fonts`. A new face needs an OFL source and an
  `ATTRIBUTION.toml` entry. Never bundle a system font.
- If an asset's origin or licence is unclear, do not add it; ask the owner.

## Never crash

Every PDF, EPUB, settings file, drag payload and keystroke is untrusted. None of
it may crash the app or lose the user's place. In non-test code:

- No `unwrap`, `expect`, `panic!`, `unreachable!` or `todo!` unless provably
  infallible, with a comment saying why.
- No indexing or slicing with input-derived positions: use `get`, and slice
  strings only at char boundaries.
- Checked or saturating arithmetic on input-derived numbers. Guard division by
  zero and NaN or infinite casts (`f32::NAN.clamp(..)` stays NaN).
- Cap allocations sized by input; bound recursion with depth limits. EPUB adds
  zip bombs, deep XML, path escapes and huge images to the list.
- No `unsafe`: the workspace sets `unsafe_code = "forbid"`. Changing that for
  one audited call (macOS traffic lights) is the owner's decision.
- Every crash fix gets a synthetic regression test.

## Layering

`cargo xtask layers` enforces the table in `docs/architecture.md`.

- L0–L3 crates (appearance, workspace, pdf, epub, library, settings, session)
  never depend on egui, eframe, winit or wgpu. They are testable without a
  window.
- Only the front layer (`crates/ui`) draws. `apps/mareader` only opens the
  window.
- A new crate gets a row in the layer table in the same commit.

## Code style

- Rust: default rustfmt (`max_width = 100`); clippy clean with `-D warnings`.
- Comments: see `### Comments` below. It is a constraint, not an argument.
- No `TODO`/`FIXME`, no scaffolding for work not being done now, no
  speculative abstractions. A planned feature lives in the parity file, not as
  an empty function.
- One owner per responsibility. No duplicate state, no permanent
  compatibility layers, no unbounded fallbacks.
- No per-frame, per-page or per-scroll logging.

### Comments

Write clean, self-documenting code. Do not write comments that explain what the
code is doing; only write comments explaining why something non-obvious was done.
Avoid repeating variable names in comments.

A comment carries a constraint the code cannot show; it never argues for a
decision. Test every added line: **would a reader who never saw the diff need
this?** If not, cut it and put what it said in the commit message. Measure it
with `python3 tools/comment_check.py FILE...`: three lines, eighty characters a
line counted from the left margin, fifteen words per block by default, all three
raisable with `--max-lines`, `--max-line`, `--max-words`. Pass the files you
touched explicitly (`cargo xtask comments` does this for the branch); directory
arguments are not walked. A comment that genuinely needs more than the ceiling
states so itself with `comment-check: allow` on the line above it, which the
tool honours and a reviewer can read as a claim, not an escape.

Blocking — remove rather than shorten:

- **Narration** of the change: "now uses…", "no longer FIFO", "this round
  added…", "changed because…". Git holds the change; the file holds the result.
- **Justification** of a choice the code cannot debate: why the alternative is
  worse, why the design is right, a debate with a deleted sibling. Pick the
  better shape and name it; the argument goes in the commit body.
- **Restatement**: what the next line says, or a parameter whose name already
  carries it. `// the page index` above `page: usize` is noise.
- **History**: what the code used to be, or what something removed did.
- An **essay on a leaf function**: a doc comment longer than its body belongs in
  `docs/` as design, not in the file.

Wanted, at one to three lines next to the thing it governs:

- An invariant another file owns, or an order a callback depends on.
- A unit, precision or width trap: points against pixels, `ms` against seconds,
  a sRGB hue against an OKLCH one.
- A hazard that makes the obvious edit wrong: a texture another pane still
  shows, a job result for a closed document, a frame that must be skipped.

When the rule needs more than three lines, the fix is usually a better name or a
smaller function. Reasoning is not banned — it has two homes, `docs/` for the
design and the commit message for the decision — and neither is a comment.

## Resource rules

- Everything that allocates has an explicit owner and teardown: textures,
  render jobs, worker threads, caches, file watchers, timers. Dropping a view
  is not teardown unless the owner's `Drop` releases it.
- Closing a pane or a document releases everything it allocated: page
  textures, tiles, the document handle, its search index.
- Job results carry the document id, generation and zoom they were made for;
  stale ones are dropped at the channel edge, never uploaded.
- Every deferral has a guaranteed wake (`request_repaint_after`, a channel
  message, a timer); nothing may wait on an event that might not come. An idle
  window renders no frames.
- PDF pages render blank until the full-resolution raster lands; no
  low-resolution placeholders (mareader's rule).
- Caches are capped by bytes or entries, never unbounded.
- Memory claims need measurements: what, how, workload, before and after, and
  known limits. Record them in `docs/memory.md`.

## egui, natively

The UI is immediate mode. Write it as egui code, not as a web app in egui
syntax (`docs/architecture.md` § How the app is built in egui).

- **State lives in plain structs** owned by `MareaderApp` or by L0–L3 types.
  No signals, stores, observers or effects; no state mirrored in two places.
  egui `Memory` holds only transient widget state.
- **Views are functions** that take the state they show and push `Command`s.
  They never mutate app state directly; `MareaderApp::apply` is the one place
  commands take effect. Buttons, menus, shortcuts and drops share commands.
- **Use egui's containers and widgets:** `Panel`, `Popup`, `Modal`, `Area`,
  `ScrollArea::show_rows`, `Sides`, `Button`, `Slider`, `TextEdit`, drag and
  drop via `dnd_*`. Write a custom widget only where egui has nothing close,
  and give it an AccessKit label.
- **The theme is `Style` and `Visuals`,** set once per appearance change. Views
  don't pick colours; custom widgets read `Tokens`.
- **Animate meaning, not decoration.** Hover is instant; motion goes through
  `motion::secs` so the Animations switch can turn it off.
- **Never block the UI thread.** I/O, parsing, raster and scans run on workers
  that report over channels and call `request_repaint`.

## UI work: look at the result

- **Every UI change is looked at, not assumed.** Render it with
  `cargo run -p mareader-ui --example shot -- OUT.png key=value…` (see
  `CLAUDE.md`), open the PNG, and check it against `docs/design.md`: light,
  dark and one tint, wide and narrow, and each platform's chrome the change
  touches. Say what was checked.
- **Behaviour is tested with egui_kittest** (`crates/ui/tests/`): hover, click
  and keyboard through AccessKit labels, at 60 fps steps
  (`with_step_dt(1.0 / 60.0)`).

## Review every update, at four zooms

Not an end-of-task pass: a habit per update, so a defect is never built on.

- **Clause:** checked arithmetic, `get()` not `[i]`, char-boundary slicing,
  `<` against `<=`, the 0-based/1-based page seam, points against pixels.
- **Function:** is it total? Name an input that panics; walk empty, zero, max,
  malformed. Is the return type honest?
- **File:** two sources of truth, dead code, is the public surface smallest,
  is the doc comment still true?
- **Project:** layering, teardown, idle frames, egui idioms, the parity entry
  and `docs/design.md` still true.

## Testing

- Never weaken an assertion or edit `.github/workflows/` to make a check pass.
  If one blocks the task, report the blocker.
- **Prove a test fails before the fix by reverting the fix.** A test nobody has
  seen fail is not evidence.
- Tests mirror the zooms: unit in the crate, integration in `tests/`, kittest
  for the shell.
- Document runtime changes cover open → use → close, close during a render,
  rapid open/close/reopen, reader ↔ library, several panes, and stale results
  after close.
- Reuse mareader's test cases where the behaviour is the same; cite them in
  the parity entry.

## Parity, honestly

- `shipped` means it exists and a test proves it: `evidence` cites
  `path::test_fn` in this repository, and the test exercises the feature, not a
  neighbour of it.
- `partial` and `dropped` say in `notes` what is missing or why.
- Update the entry in the same commit as the feature. A status that became
  untrue is a bug.

## Gates

Before every commit:

```sh
cargo fmt --all --check
cargo clippy -p <crate> -j 2 --all-targets -- -D warnings
cargo test -p <crate> -j 2
cargo xtask layers && cargo xtask parity && cargo xtask assets
cargo xtask comments
```

Scope builds and tests to the crates you touched, under a `timeout`. The full
workspace sweep (`cargo xtask ci`) is CI's job; run it locally only when the
machine is free.

## Commits and pull requests

- Conventional commits: `type(scope): summary`, imperative, lower case, no
  trailing period, subject ≤ 50 characters — GitHub's own truncation sits near
  72, so 50 is what survives a log line, a UI row and a checklist cell intact.
  Enforced rather than remembered: `python3 tools/commit_check.py --install`
  writes a `commit-msg` hook that refuses an over-long subject, and
  `python3 tools/commit_check.py --range main..HEAD` audits history. The hook
  is per checkout, so a fresh clone installs it once; `--status` reports
  whether it is armed — git skips a non-executable hook in silence.
- A squash merge makes the **PR title** the subject and appends ` (#NN)`, so a
  title must fit 50 characters *after* that suffix: 43 for a two-digit number.
  Check the merged form:
  `printf '%s (#12)\n' "$title" > /tmp/m && python3 tools/commit_check.py /tmp/m`.
- Types: `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `build`, `ci`,
  `chore`. Scope is the area touched (`shell`, `panes`, `pdf`, `epub`,
  `appearance`, `library`, …).
- One coherent change per commit; context goes in the body, not the subject.
  No progress reports, phase labels or ticket numbers in the subject.
- Work on a branch named `<type>/<slug>` and merge into `main` through a PR
  once CI is green. Commit only green states.
- Squash fixups before pushing; rewrite remote history only with
  `git push --force-with-lease`.
- Never commit credentials, tokens or personal documents.

## Definition of done

1. The change is implemented, and looked at (shots) if it touches UI.
2. The gates above pass and `CI` is green for the pushed SHA.
3. Docs describe the current behaviour: `docs/design.md` for the look,
   `docs/architecture.md` for structure, `docs/memory.md` for memory, and the
   parity entry for status.
4. Every added comment passes the diff-blind test (`### Comments`) and
   `python3 tools/comment_check.py` is clean on the files the round changed.
   The round that adds a rule also applies it in the same round.
5. The summary states what changed, what was verified and any limits. If a
   requirement cannot be met, report the blocker instead of dropping it.
