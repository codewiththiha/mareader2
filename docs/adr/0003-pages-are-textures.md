# ADR 0003: every page is a texture; one GPU pass applies the look

- **Status:** accepted, 2026-10-13
- **Context:** mareader recolours PDF pages with a CSS filter chain and
  `mix-blend-mode`, overlays textures and grain with CSS, and recolours text pages
  through tokens. In egui, per-pixel effects have to be GPU work, and every
  feature must look the same in both formats.
- **Decision:**
  - Both formats produce RGBA rasters (PDF tiles from hayro, EPUB pages from
    vello_cpu), uploaded as egui textures.
  - One `egui_wgpu` paint callback draws a page. It applies:
    - the filter matrices (`Appearance::page_filter`, at most `MAX_FILTER_OPS`);
    - the blend family (`page_blend`);
    - the texture tile, anchored to the page;
    - the grain.
  - EPUB rasters are drawn in the token colours already, so their filter chain is
    empty (mareader: "blend mode stays a PDF feature").
- **Consequences:**
  - The CPU reference (`apply_filter`, `Blend::apply`) is the oracle for shader
    tests.
  - Texture memory is bounded by a byte-capped LRU per session.
  - Zoom stretches the current texture and re-rasterises once at rest.
