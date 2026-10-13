# The look

> **Status:** M0 · **Last updated:** 2026-10-13

mareader2 must look like mareader: same tokens, same metrics, same motion. This
file is the spec. Where it is silent, mareader's source is (paths relative to the
`../mareader` checkout). Verify every UI change with a screenshot
(`CLAUDE.md` § Looking at the result); a change that only compiles isn't done.

## Colour tokens

Seven tokens per base mode, computed in `crates/appearance` (`tokens.rs`), applied
to egui in `crates/ui/src/theme.rs`. Values are mareader's `styles/tokens.css`.

| Token | Light | Dark | Dim | Used for |
|---|---|---|---|---|
| paper | `#ffffff` | `#131316` | `#1a1c1f` | page background, panes |
| ink | `#1f2937` | `#e5e7eb` | `#c3c6cb` | text and icons |
| muted | `#6b7280` | `#9ca3af` | `#8b8f96` | secondary text, hints |
| surface | `#f3f4f6` | `#1a1a1e` | `#202328` | chrome, the title bar |
| line | `#e5e7eb` | `#2b2b31` | `#2e3238` | separators, borders |
| accent | `#2563eb` | `#60a5fa` | `#7a9bd4` | selection, the active pane |
| accent-soft | `#dbeafe` | `#1d2b3a` | `#232b36` | selected button wash |

`crates/appearance/src/tokens.rs` is authoritative if this table and the code
disagree. Fix the table.

**Tint.** The tint hue is an sRGB angle and the tokens move on the OKLCH circle.
`ui_hue_oklch` converts between them; reusing the angle turns a warm tan pink.
Each token keeps its lightness and rises towards a per-token chroma ceiling.
Strength 50 already gives the full effect.

**Known upstream quirk (kept on purpose).** Neutral tokens move their hue from
their own blue-grey hue along the shortest arc. Under Sepia that arc passes
through magenta, so the surface and accent come out pink-violet. mareader does
the same (`appearance/shared/tint.rs`). Fix it in both apps or neither, as a
decision in `docs/adr/`.

**Derived egui colours** (`theme.rs`):
- hover = `line`, as every mareader button (`hover:bg-line`); no pressed shade;
- glass = surface at 96 %. This is mareader's no-`backdrop-filter` fallback, used
  until the blur pass lands (M1).

## Type

- Inter 4.001 (Regular, Medium, SemiBold), embedded, in `assets/fonts`.
- Body 14 pt, small 12 pt, title 13 pt Medium.
- The named families `inter-medium` and `inter-semibold` exist only from the
  second frame. `MareaderApp::ui` skips frame one for that reason.

## Metrics

| Thing | Value | mareader source |
|---|---|---|
| Title bar height | 48 pt | `crates/app-chrome/src/titlebar/mod.rs` `TITLE_BAR_H` |
| Icon button | 36 pt square (`h-9 w-9`), radius 8, icon 18 pt, stroke 2; hover `line`; toggled = accent icon, no fill; disabled opacity .45 | `crates/app-chrome/src/icon_button.rs` |
| Ghost button (icon) | 40 × 36 pt (`h-9 px-2.5`); compact 32 × 32 (`h-8 px-2`) with a 14 pt icon | `crates/app-ui/src/components/primitives/controls/button.rs` |
| Button transition | 150 ms | `transition-colors duration-150` on the buttons |
| Popover | radius 12 (`0.75rem`), line ring + shadow 0 8 24 black 30 % | `styles/components/shell.css`, `--shadow-float` in `styles/tokens.css` |
| Popover entrance | 160 ms, `cubic-bezier(0.22, 1, 0.36, 1)` | `bubble-in`, `styles/components/shell.css` |
| Sidebar | 288 pt wide (`w-72`), slides 300 ms | `crates/reader-runtime/src/components/shell/sidebar/container.rs` |
| Thumbnail cell | 120 pt, two abreast, 12 pt gap, scale 0.25 | `crates/reader-runtime/src/components/shell/sidebar/panels/thumbnails/geometry.rs` |
| Scrollbar | 10 pt, thumb radius 5 in `line`, 2 pt inset | `styles/components/shell.css` |
| Glass | surface 72 % (oklab mix), blur 20, saturate 180 %; 96 % without blur | `styles/components/title_bar.css` `.toolbar-glass` |
| Toast | 3500 ms | `crates/app-ui/src/components/primitives/overlay/toast.rs` |
| Search hit | current `#f59e0b` | `--match-current`, `styles/tokens.css` |

## Title bar

Behaviour reference: `app-chrome/src/titlebar/root.rs` and
`hooks/hover_reveal.rs`. Implementation: `crates/ui/src/titlebar.rs`.

- **Floating, not docked.** The bar is an `egui::Area` at `Order::Foreground`
  over the content. Content never moves when it appears.
- **Reveal band.** The top 48 pt, from the docked sidebar's right edge to the
  window's right edge. When the pointer enters, the bar shows at once.
- **Hide grace.** 400 ms after the pointer leaves (`HIDE_GRACE`), then a 200 ms
  fade (`FADE`).
- **Held.** The bar stays while any popup it opened is up, or while a press that
  began on it is held.
- **Pinned.** The pin button keeps it up. The pin is persisted, separately for
  the reader and the library: the library's bar starts pinned.
- **Chromeless.** Never shows (the distraction-free mode).
- **Clicks while fading out do nothing**, so a click meant for the page doesn't
  hit a vanishing button.
- **Drag and double-click.** The empty bar drags the window and double-click
  toggles maximise. The drag sense is registered first, so the buttons win.
- **Layout, left to right:**
  - the left padding: `TRAFFIC_LIGHTS_GUTTER_PX` 88 pt on macOS, otherwise
    `TITLEBAR_REST_PADDING_PX` 12 pt (`app-ui/.../shell/controller/mod.rs`);
  - the sidebar toggle and library button (Ghost buttons, `gap-2` = 8 pt);
  - the title. mareader centres it in the slot between the clusters and hides
    it below a minimum width; here it is centred on the window until M1;
  - the trailing cluster (`gap-1` = 4 pt): the pane's trailing slot, the
    appearance menu, then the pin ("Pin titlebar open");
  - the platform captions, 8 pt after the cluster; the row's right padding is
    8 pt (`pr-2`), which Windows captions cancel to sit flush.
- **Trailing slot.** In mareader the open pane fills it (zoom, view mode, search
  and so on). Until those exist, M0 shows split right, split down, search and
  settings there so the shell can be exercised.
- **Pane close.** A compact Ghost `x` 8 pt from the pane's top right (56 pt under
  a pinned bar), fading in on hover (`host/view.rs`).

### Caption buttons

| Platform | Shape | Rest | Hover | Pressed | Close hover |
|---|---|---|---|---|---|
| macOS | native traffic lights, x inset 20, centred on the 48 pt bar | — | — | — | — |
| Windows | 46 × 48 pt, square, flush right, **no transition** | transparent | `line` | opacity .72 | `#e81123`, glyph white; pressed `#c50f1f` |
| GNOME/Linux | 24 pt circles, 8 pt apart, 10 pt inside the row, 120 ms ease | ink 12 % | ink 24 % | ink 34 % | `#e0524f`, glyph white; pressed `#c53a37` |

**Open: macOS traffic-light placement.** mareader moves them with AppKit
(`src-tauri/src/macos/traffic_light.rs`). winit has no safe API for it, and this
workspace forbids `unsafe`. Until the owner decides (accept the default position,
or allow one audited `objc2` call), the 88 pt gutter fits the default position.

The glyphs are Lucide `minus`, `square` (maximise), `copy` (restore) and `x`.
Windows glyphs are 14 pt and GNOME glyphs 12 pt, both at stroke 2, the
sizes in `styles/components/title_bar.css`.

**Frameless edges.** On Windows and Linux the outer 5 pt resize the window
(`crates/ui/src/resize.rs`). Corners win over edges.

## Panes

Reference: `reader-runtime/src/host/{tree,drop_target}.rs`. Model:
`crates/workspace`. View: `crates/ui/src/panes.rs`.

- Up to 4 panes. A split refuses below a 0.15 ratio or 200 pt per side.
- Divider hit band 8 pt. Drop edges are checked in the order Right, Bottom, Left,
  Top; "here" drops only into an empty pane.
- `WorkspaceSettings` defaults:
  - outline 2 pt in the accent ("auto");
  - gap 0;
  - no shadow (when on: `0 5px 18px` black 24 %);
  - square corners (when rounded: 10 pt).
- The backdrop behind the panes, seen in gaps, is `paper` (`.reader-bg`).
  
  Named outline colours (`#e56b64` for red, and so on) are in
  `reader-core/src/settings/workspace.rs`.

## Icons

Lucide 1.34.0 through `egui-lucide`, behind `crates/ui/src/icons.rs`.

- Code asks for an `Icon` by meaning; `Icon::lucide` is the only place that names
  a glyph.
- Icons are rasterised **white** and tinted with `Image::tint`. egui-lucide puts
  the colour in the texture key, so a hover that animates colour would otherwise
  rasterise a new texture every frame.
- Stroke width is a parameter; mareader draws every icon at 2.

| Icon | Lucide |
|---|---|
| Sidebar | `panel-left` |
| Library | `library-big` |
| Open | `folder-open` |
| Search | `search` |
| Appearance | `palette` |
| Settings | `settings` |
| Pin | `pin` (accent while pinned) |
| Menu | `menu` |
| SplitRight / SplitDown | `columns-2` / `rows-2` |
| Close | `x` |
| WindowMinimize / Maximize / Restore | `minus` / `square` / `copy` |
| Book | `book-open` |

## Motion

Reference: mareader README § Motion and `reader-core/src/settings/animation.rs`.

- **Animations** is the master switch (`crates/ui/src/motion.rs`). Off zeroes
  egui's `animation_time` and every `motion::secs`. The end state still arrives
  in the same frame.
- **Per-motion switches** (later milestones):
  - sidebar slide, 300 ms;
  - canvas follows the window;
  - zoom ease;
  - scroll to page.
- **Exempt:** the loading mark. Under the master switch it trades travel for a
  fade, because a frozen spinner looks hung.

## Z-order

mareader stacks controls 30, bar 40, popover 50, context menu 70, toast 100. In
egui this maps to the following:

| Layer | egui |
|---|---|
| Content | `Order::Background` / the root `Ui` |
| Controls | `Order::Middle` |
| Bar | `Order::Foreground` |
| Popover, context menu | egui popups (`Order::Foreground`, opened later) |
| Toast | `Order::Tooltip` |

## Revision history

| Date | Summary |
|---|---|
| 2026-10-13 | Created with the M0 foundation |
