# Design

> **Status:** M0 · **Last updated:** 2026-10-13

mareader2's design system. It starts from mareader's: calm, paper-first, chrome
that gets out of the way. But it is ours to improve (ADR 0005): where egui offers a
better native answer, or a mareader detail reads poorly, change it here, then in
the code.

Every UI change is looked at with the shot tool (`CLAUDE.md` § The look loop)
in light, dark and one tint, at desktop and narrow sizes.

## Principles

- **The page is the app.** Chrome is translucent, auto-hiding and quiet; the
  pane's paper colour fills everything the page doesn't.
- **One theme source.** Colours and shape come from the appearance
  (`crates/appearance` → `Tokens` → egui `Visuals` and `Style`). Stock widgets
  get them from the theme; custom widgets read `Tokens`. No colour or size
  literals in views except platform caption conventions.
- **Instant.** Moving an appearance control restyles the whole window, pages
  included, on the next frame.
- **Native over imitation.** Use egui's widgets, containers and interaction
  states. Hover responds at once, as egui does; animate only motion that
  explains something (the bar fading, the sidebar sliding, a zoom easing).
- **Nothing interactive under the hover band.** The top 48 pt reveals the title
  bar, so pane controls sit below it.

## How the theme works

Several looks, plus accent, contrast, brightness or text-size knobs, all
come from one small mechanism:

1. **Tokens are the only source.** `theme::Tokens` holds colours *and* shape:
   radii, control and icon size, text sizes, gap. Views and widgets read
   `Tokens::get(ctx)`; a theme can change shape, not just colour.
2. **Tokens are a pure function of settings:** `Tokens::new(&Appearance)`.
   A new knob is a small pure modifier on top (a `with_brightness`, a
   `with_appearance(accent, contrast)`), with a test.
3. **One apply.** `theme::apply(ctx, &tokens)` writes egui's `Visuals` and
   `Style`, covering all five widget states (noninteractive, inactive, hovered,
   active, open), and stores the tokens in egui's context data. Stock widgets
   follow with no per-call styling.
4. **Applied on change, at once.** `MareaderApp::restyle` compares the settings
   with what it last applied. An appearance `Command` restyles at the end of
   the frame it lands in and asks for one repaint, so the next frame shows it.
   A slider sends `Command::SetAppearance` on every tick; only disk writes are
   debounced. Test: `an_appearance_change_restyles_on_the_frame_it_lands`.
5. **Pages follow without re-rendering.** Filter, blend, texture, grain and the
   EPUB ink and paper colours are uniforms of the page pass (ADR 0003). A tint
   or brightness drag re-rasterises nothing; only typography (font, size,
   spacing) re-lays out EPUB text.
6. **Tune it live.** Debug builds read `MAREADER_TOKENS=tokens.json` (keys are
   token field names; values `"#rrggbb[aa]"` or numbers) and re-apply when the
   file changes. Unset, it costs nothing.

**Knobs this makes cheap** (not mareader features; the owner decides): interface
text size, a contrast boost, roundness, density. Each is one modifier.

**Not done:** per-app grammar flags (`pro`, `bevel`); mareader2 has one
design. A brightness knob would go through OKLCH like the tints, not per sRGB
channel.

## Colour

Seven tokens per base mode, from `crates/appearance/src/tokens.rs` (the
authority; fix this table if they disagree).

| Token | Light | Dark | Dim | Used for |
|---|---|---|---|---|
| paper | `#ffffff` | `#131316` | `#1a1c1f` | page background, panes |
| ink | `#1f2937` | `#e5e7eb` | `#c3c6cb` | text and icons |
| muted | `#6b7280` | `#9ca3af` | `#8b8f96` | secondary text, hints |
| surface | `#f3f4f6` | `#1a1a1e` | `#202328` | chrome, the title bar |
| line | `#e5e7eb` | `#2b2b31` | `#2e3238` | separators, hover fill |
| accent | `#2563eb` | `#60a5fa` | `#7a9bd4` | selection, the active pane, toggles |
| accent-soft | `#dbeafe` | `#1d2b3a` | `#232b36` | selection wash |

**Tint.** A tint colours the whole palette in its hue. Each token takes the
tint's hue outright (in OKLCH), keeps its lightness, and gains chroma toward a
per-token ceiling as the strength rises; strength 50 gives the full effect.
Colours sRGB cannot show are gamut-mapped by lowering chroma; near-white tokens
first darken up to 0.06 L so a sepia paper is cream, not white. Taking the hue
outright means no tint passes through other hues on the way (mareader's sepia
went magenta).

**Book paper.** A reflowed page's paper follows the same rule as the tokens: its
target chroma is capped at what sRGB holds within 0.06 L of the mode's paper,
then scaled by the strength, and near white it darkens rather than losing
chroma. So every step of the slider shows in every mode and hue (light paper
used to stop at `#fff7f0` by strength 10). The popover shows strength as a share
of the full effect, 0–100 % over stored strengths 0–50.

**Appearance popover.** Groups (Presets, each preset section, Mode & colour,
Page texture, Film grain) fold from their header; a folded header reads out its
setting, and folds persist. Presets are five-across cards with "Save current
look" as the header's +. Choices are segmented tracks; texture and grain dials
show only when they act. While split, the per-pane switches sit pinned under
the scroll, together.

**Derived in `theme.rs`:**
- hover fill = `line`; selected = `accent-soft` with an accent icon;
- glass (the title bar) = surface at 96 % until the blur pass (M2 slice 3) gives
  surface at 72 % with a 20 pt backdrop blur.

**Contrast floor.** Muted and accent ink are small text, so after any tint
`readable` moves their OKLCH lightness just far enough for 4.5:1 (WCAG AA)
on paper, surface and accent-soft. Light muted and light accent darken a
little; dark mode already passes. The table holds the inputs.

**Preset names show whole.** A centred name may run into the gaps beside
its card, then shrink by up to a point; only then do cards drop below five
across ("Parchment" was "Parchme…"). Past three it is elided, whole on hover.

## Type

- Inter (Regular, Medium, SemiBold), embedded from `assets/fonts`.
- Body 14 pt, small 12 pt, title 13 pt Medium.
- Reading fonts for EPUB arrive with M4 (ADR 0002).
- egui's named families exist only from the second frame; `MareaderApp::ui`
  skips frame one for that reason.

## Metrics

| Thing | Value | Where |
|---|---|---|
| Title bar | 48 pt | `theme::TITLE_BAR_H` |
| Toolbar button | 36 pt square, 18 pt icon, radius 8 | `Tokens::{control, icon, radius}` |
| Popover, window | radius 12, shadow 0 8 24 (black 24 % light, 47 % dark) | `Tokens::{radius_lg, shadow}` |
| Spacing | gap 8: items 8 × 6, button padding 10 × 6; toolbar clusters 4 | `Tokens::gap`, `titlebar.rs` |
| Body, small text | 14 pt, 12 pt | `Tokens::{text, text_small}` |
| Sidebar (M1) | ~288 pt, slides 300 ms | — |
| Toast (M1) | 3.5 s | — |

## Title bar

`crates/ui/src/titlebar.rs`. An `egui::Area` at `Order::Foreground` floating
over the panes, so content never moves when it appears.

- **Reveal.** The top 48 pt (after a docked sidebar) shows it at once.
- **Hide.** 400 ms after the pointer leaves, then a 200 ms fade.
- **Held** while any popup is open, or while a press that began on it is down.
- **Pinned** by the pin button; persisted. The library's bar starts pinned (M5).
- **Pin, since the UX audit.** The bar has no pin button now: View ▸ "Keep
  title bar shown" (also in Settings) pins it. The pin icon means one thing
  only, docking the sidebar from its header.
- **Chromeless** mode never shows it.
- **Fading out, it ignores clicks**, so a click meant for the page lands there.
- **Window moves.** Dragging the empty bar moves the window; double-click
  toggles maximise.
- **Layout.** `egui::Sides`: leading cluster (sidebar, library) and trailing
  cluster (search, appearance, settings, pin, then the captions). No split
  buttons (owner, 2026-10-17): panes split by dragging a book from the
  sidebar's Library panel onto a pane edge. The title is centred on the window when it fits, else in the gap,
  truncated with an ellipsis, and hidden when the gap is under 48 pt.
- **Document controls.** When documents open (M2), the trailing cluster gains
  the pane's own controls (zoom, view mode).

### Window controls

| Platform | Look |
|---|---|
| macOS | Native traffic lights at x 20, centred in the 48 pt row; the bar leaves 80 pt for them when it hosts them |
| Windows | 46 pt full-height squares, flush right; hover `line`; close hover `#e81123`, pressed `#c50f1f`, white glyph; 14 pt glyphs |
| GNOME/Linux | 24 pt circles, 8 pt apart; ink at 12 / 24 / 34 % for rest / hover / pressed; close `#e0524f` / `#c53a37`, white glyph; 12 pt glyphs |

**macOS traffic lights** (owner decision, 2026-10-13). AppKit puts them at
the top of a 28 pt strip; `apps/mareader/src/traffic_lights.rs` moves them to
the middle of the 48 pt row; `NativeLights` (`crates/ui/src/platform.rs`)
says where, each frame:

- **A drawn sidebar hosts them**, docked or floating: they sit in its header
  at full opacity and slide with it, leaving past the window's left edge.
- **Otherwise the bar does**, fading with it (and in after a rail's exit).
- **Overlay mode, no rail:** hidden, and the bar keeps no gutter (mareader).

Hidden lights can't take clicks. Full screen is left to AppKit. Off a Mac,
shots and the preview draw three stand-in dots in their place. It is the
workspace's one `unsafe` module and the `traffic-lights` feature (on by
default); `--no-default-features` keeps AppKit's position.

**Frameless edges.** On Windows and Linux the outer 5 pt resize the window
(`crates/ui/src/resize.rs`); corners win over edges.

## Panes

Model `crates/workspace`, view `crates/ui/src/panes.rs`. Each pane is drawn in
a scoped `Ui` region (`Ui::scope_builder` with a `max_rect`), the way
egui_tiles lays out tiles. That is layout only: panes share the app's
documents, caches and workers (ADR 0006).

- Up to 4 panes; a split refuses below a 0.15 ratio or 200 pt per side.
- Dividers: an 8 pt drag band with resize cursors.
- The active pane (when split) has an accent outline, 2 pt by default.
- Workspace look settings: gap, outline width, rounded corners
  (`Tokens::radius_lg`), shadow (0 5 18, `Tokens::shadow`).
- Dragging a divider sends `Command::SetRatio` every frame it moves.
- A close button appears at the top right of a hovered pane, below the hover
  band.

### Lifting a pane (owner request, 2026-10-18)

In a split, pressing empty space in a pane (off the pages, or away from
their words) and holding still for a second lifts the pane; moving more
than 4 pt first makes the press a pan instead. A ring fills round the
press from a fifth of a second on, so plain clicks never flash it.

- While lifted, the pane dims where it was, a chip "Moving <title>"
  follows the pointer, and the box it would get lights in the accent
  with the move in words.
- Over another pane's middle third it swaps with that pane; elsewhere it
  docks beside the nearest edge with room for two 200 pt halves. A pane
  too small to halve either way swaps.
- Letting go moves it (`PaneTree::swap`, `PaneTree::move_to`); the pane
  keeps its id, so its document, page bar and look go with it. Letting
  go off any pane, or Escape, leaves everything as it was.
- One pane alone never lifts; a move that would not fit is refused
  whole, with a note.

## Settings that follow the focused pane

The settings window shows only what reaches the focused pane, as mareader
does, but from one rule instead of a condition at every row.
`settings_window::Scope` says what the focused pane shows (nothing, a PDF,
or a book read as a stream or as pages), whether the window is split,
whether any pane shows a book, and whether pages scroll sideways. Each tab,
section and row names an `Applies`, and `Scope::applies` is the only place
that decides. A section's rows go through `Rows`, which draws a divider only
between rows it drew, so hiding a row never leaves a double line.

| Applies | Shown when | Used by |
|---|---|---|
| `Always` | always | most rows |
| `Book` | a book is focused | the Fonts tab, Column width included |
| `NotBook` | a PDF or nothing is focused | Auto resize, the Paper section |
| `Pages` | not a book's vertical stream | No gap, Page shadow |
| `Split` | the window is split | Split pane appearance |
| `SplitWithBook` | split, with a book in any pane | Pane text |

A new setting picks its `Applies` once; nothing else changes.

## Tooltips, toasts and the menu

- **Tooltips.** `widgets::tip`: a chip on `surface` with a `line` edge,
  radius 6, 12 pt text, the shortcut beside it in `muted`. 350 ms delay.
- **Toasts.** `crates/ui/src/toast.rs`. One at a time, a new one replaces it;
  bottom centre of the panes, 24 pt up, at most 480 pt wide. Info shows a
  check in the accent, error an alert in `danger` with a tinted edge. Up
  3.5 s; hovering holds it, then 1.5 s more; 200 ms fade and rise.
- **View & tools menu.** The last button in the bar (`view_menu.rs`). It
  holds full screen, the keyboard shortcut list and the version. When the
  bar is narrow, Settings, Search and the splits fold into it, leftmost
  first, so the title keeps 160 pt.
- **Shortcuts.** `keymap.rs` is the only table: the handler, tooltips and
  the list all read it, so they cannot disagree. macOS shows symbols (⌘,),
  other desktops words (Ctrl+,).
- **Shortcut sheet.** "Keyboard shortcuts" in either View menu, or Ctrl+/
  (⌘/), opens a centred modal (`shortcuts.rs`) instead of unfolding a list in
  the menu. Groups (Moving around, View, Find, General, Panes, Controls) sit
  in two balanced columns, one when narrow; every key for an action shows as
  a key cap. The filter takes focus, matching action names, group names or a
  whole key. Esc, the backdrop, ✕ or its own key close it; page keys stay
  inert behind it.
- **Selection bar icons.** The bar over selected text shows icons alone at
  every width; each name is its tooltip and accessible label. Words made it
  far too wide.
- **Remove alert.** Removing asks in a small alert docked at the window's foot
  (`library/remove_alert.rs`), fixed at 440 px so it never jumps. It holds the
  question by name ("Remove “Dune”?"), one line on what happens to the files,
  and switches only where they mean something: "Remove everything inside" for
  a shelf with contents, and "Erase highlights and reading position" when
  there is any. Both start off each time; the erase switch turns its card red
  and the button to "Erase & remove". No counts, sizes or shelf lists. Enter
  removes, Esc or Cancel keeps; a showing toast is dismissed so nothing covers
  it, and a short toast follows.
- **Import sheet.** A short form (`library/import_sheet.rs`, 460 px): the
  folder as a field the whole row of which picks another; format checkboxes,
  ticked ones counted; "File size larger than" with its dial on one line;
  "Read books in place" with "Check for new books" nested under it; then the
  folder structure as two radios. Each answer has one small line under it,
  no paragraphs. Import stays off until a format is ticked; Enter imports.
- **Dictionary card, compact.** 300 px wide with tighter padding; the
  languages sit on the word's line before ✕. Burmese and other tall scripts
  draw answers at 1.05× the text size, not 1.15×: they run taller at a size.
- **Long highlights.** A notes-panel quote stops at three lines and a note at
  four; a cut row ends in "Show more", which opens it in place, then "Show
  less". The row's accessible label always carries the whole text.
- **Shaped text in panels.** Burmese, Thai, Arabic and Indic text egui would
  garble goes through `shaped.rs`: notes, chapter titles in Contents and the
  notes headings, Library titles, and the AI card's word, meaning and usages.
  Heights come from a cheap measure; a texture is drawn only once on screen.
  Search does not wash words in shaped text.

## Icons

Lucide 1.34.0 through `egui-lucide`, behind `crates/ui/src/icons.rs` (ADR 0004).

- Code asks for an `Icon` by meaning; `Icon::lucide` is the only place that
  names a glyph. Add a variant when a feature needs one.
- Icons are rasterised white and tinted: in a `Button` by
  `image_tint_follows_text_color`, in custom widgets by `Icon::paint`.
- Stroke 2 throughout.

## Motion

- **Animations** is the master switch (`crates/ui/src/motion.rs`). Off zeroes
  egui's `animation_time` and every `motion::secs`; end states still arrive in
  the same frame.
- Per-motion switches (M2): sidebar slide, canvas follows the window, zoom ease,
  scroll to page.
- A loading indicator fades rather than freezes when animations are off.

## Layers

| What | egui |
|---|---|
| Panes and pages | the root `Ui` |
| Title bar | `Area`, `Order::Foreground` |
| Popovers, menus | `Popup` |
| Settings, confirmations | `Modal` |
| Toasts | `Area`, `Order::Tooltip` |

## Revision history

| Date | Summary |
|---|---|
| 2026-10-13 | Created with the M0 foundation as `look.md` |
| 2026-10-13 | Became `design.md`: our own design system (ADR 0005); one-hue tints; egui-native controls |
| 2026-10-13 | How the theme works: shape tokens, one apply, instant restyle, live token file |
| 2026-10-15 | Settings that follow the focused pane: `Scope`, `Applies` and `Rows` |
| 2026-10-18 | Lifting a pane to move or swap it |
