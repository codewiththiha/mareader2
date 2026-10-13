# Porting from mareader

> **Status:** living map · **Last updated:** 2026-10-13

mareader (`../mareader`, read-only) is the behavioural reference: what a
feature does, its numbers, its edge cases and its tests. It is **not** a code
source. Its crates are built for Tauri, Leptos signals, iframes and pdf.js, so
features are rebuilt for egui here. The pure-logic crates are the exception:
they are ported when they have no web types, keeping their tests.

## Crate map

| mareader | mareader2 | How |
|---|---|---|
| `reader-core/src/appearance` | `crates/appearance` | **ported** (M0): model, tokens, tint, filter chain, presets |
| `reader-core/src/settings` | `crates/settings` | port types, serde names and defaults; drop CSS-specific helpers |
| `reader-runtime/src/host/{tree,drop_target}` | `crates/workspace` | **ported** (M0): split tree, ratio clamps, edges |
| `library-core` | `crates/library` | port; storage moves from localStorage to files |
| `dict-core`, `cefr-core`, `ai-core` | `crates/study` | port the pure logic; transports are new |
| `download-core` | inside `study` (dictionary packs) | port |
| `pdf-core`, `pdf-engine`, `pdf-paper`, `public/pdfEngine.ts` | `crates/pdf` | **rebuild** on hayro; behaviour from the tests |
| `txt-core`, `md-core`, `reflow-core` | `crates/reflow` + `crates/epub` | **replace**: EPUB, not txt/md (`docs/epub.md`) |
| `virtual-list`, `virtual-list-leptos` | `egui::ScrollArea::show_viewport` + our own row math | rebuild |
| `app-chrome` | `crates/ui/src/{titlebar,widgets,resize,platform}.rs` | **rebuilt** (M0); spec in `docs/look.md` |
| `app-ui`, `app-state`, `reader-runtime` (components) | `crates/ui` | rebuild view by view |
| `frame-transport`, `runtime-contract`, `tauri-bridge`, `storage` | — | **dropped**: no realms or bridge in one process |
| `src-tauri/src/{ai,cefr,dict,download.rs}` | `crates/study` + `crates/ui` | rebuild; the Apple Intelligence bridge needs a design (M6) |
| `styles/*.css` | `crates/ui/src/theme.rs` + painters | tokens and metrics in `docs/look.md` |

## Concept map

| In mareader | In mareader2 |
|---|---|
| Leptos signal / memo | a field on the app or view struct; derived values computed in `ui` |
| An effect that reacts | the code that changes the value also does the follow-up |
| CSS `transition` | `ctx.animate_bool_with_time` / `animate_value_with_time` with `motion::secs` |
| `backdrop-filter: blur` | a wgpu blur pass under the bar (M1); until then glass at 96 % |
| CSS `filter` on the page | the page pass (ADR 0003) |
| `mix-blend-mode` texture | the page pass blend family |
| DOM text layer | glyph geometry plus painted selection rectangles |
| `aria-*` | `Response::widget_info` (accesskit); kittest finds widgets by these labels |
| iframe disposal + counters | `Drop` + job cancellation + a texture-count test |
| localStorage `mareader.settings.v1` | eframe storage under the same key (JSON) |
| `setTimeout` / rAF | `request_repaint_after` and frame time |
| browser smoke tests | kittest + `examples/shot.rs` screenshots |

## How to port one feature

1. Pick a `planned` entry in `parity/mareader-features.toml` with the lowest
   milestone and highest tier.
2. Read its `reference` paths in `../mareader`: the code, the comments that
   record past regressions, and the tests. Write down the numbers.
3. Write the failing test in mareader2 first. Port mareader's test cases where
   they apply.
4. Implement for egui. Keep egui out of L0–L3.
5. Look at the result: `shot` before and after, at desktop and narrow sizes, in
   light, dark and a tint.
6. Set the parity entry to `shipped` or `partial` with evidence and honest
   notes, and update `ROADMAP.md`.

## Revision history

| Date | Summary |
|---|---|
| 2026-10-13 | Created with the M0 foundation |
