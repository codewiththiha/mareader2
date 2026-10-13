# ADR 0004: Lucide icons through egui-lucide, behind a facade

- **Status:** accepted, 2026-10-13
- **Context:** mareader draws its own SVG paths, a fixed set in
  `app-chrome/src/icon.rs`. The rewrite wants a large, consistent and openly
  licensed set that can grow with features without drawing each glyph.
- **Decision:**
  - Lucide 1.34.0 (ISC; 1,777 icons) through `egui-lucide` 0.1 (MIT), whose only
    dependency is egui 0.36.
  - Code names icons by meaning (`ui::icons::Icon`). Only `Icon::lucide` names a
    glyph.
  - Icons rasterise white at a chosen stroke width and are tinted with
    `Image::tint`. egui-lucide puts colour in the texture key, so a colour fade
    would otherwise rasterise every frame.
  - The images need `egui_extras::install_image_loaders` and its `svg` feature.
- **Consequences:**
  - Changing the glyph set, or vendoring the SVGs if the crate goes stale, is a
    one-file change.
  - ATTRIBUTION.toml lists the set as `[[bundled]]`.
