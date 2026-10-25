# mareader2

A native rewrite of [mareader](https://github.com/codewiththiha/mareader), the
calm PDF reader, in Rust with egui and wgpu. Everything mareader does, in a
smaller and cleaner codebase with a fraction of the memory: no webview, no
JavaScript, pages rendered by [hayro](https://github.com/LaurenzV/hayro) and
books laid out by [parley](https://github.com/linebender/parley).

**Status:** M8, polish, is done, and v0.0.4 is out for macOS, Windows and
Linux. Every P0 and P1 mareader feature is shipped. Sync between machines
(M9) is optional and much later.
See [`ROADMAP.md`](ROADMAP.md).

## Download

Installers for each system are on the
[releases page](https://github.com/codewiththiha/mareader2/releases), with
which file to pick and how the first launch goes.

## What it will read

- **PDF:** single, spread, vertical and horizontal views, zoom and fit, outline,
  thumbnails, search, selection and links.
- **EPUB** (replacing mareader's txt and md): mareader's typography controls —
  fonts, size, weight, line height, spacing, indent, justification,
  hyphenation, book layout — over the same four view modes.

And around both: mareader's appearance (light, dark, dim, tints, textures,
grain, blend backdrop, presets), up to four panes, the library with shelves and
watched folders, gloss cards, CEFR highlighting and offline dictionaries.

## Build and run

Rust 1.95 or newer.

```sh
cargo run -p mareader
cargo test --workspace
cargo xtask parity        # how far the rewrite is
```

On Linux, wgpu needs a Vulkan or GL driver; headless tests use lavapipe
(`mesa-vulkan-drivers`).

## Layout

```
apps/mareader      the binary
crates/ui          the egui front: theme, icons, title bar, panes, shell
crates/appearance  base modes, tints, filters, presets, tokens
crates/workspace   the pane split tree
crates/settings    every user setting: schema, ranges, migration
xtask              layers, parity, assets, comments, ci
parity/            every mareader feature and its status
docs/              design, architecture, library, EPUB, porting, memory, ADRs
```

## Contributing

Read [`AGENTS.md`](AGENTS.md) (the rules) and [`CLAUDE.md`](CLAUDE.md) (how to
work here). Features are rebuilt one at a time, using a mareader checkout at
`../mareader` as the reference for what each one does.

## Licence

MIT, see [`LICENSE`](LICENSE). Bundled assets are listed with their licences in
[`ATTRIBUTION.toml`](ATTRIBUTION.toml): Inter (OFL-1.1) and Lucide (ISC).
